use std::path::Path;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use repowitness_domain::RepositoryPathLimits;

use crate::git_paths::{
    GitPathDiscoveryError, GitPathDiscoveryLimits, capture_git_output_from_command,
    capture_git_output_with_status_from_command, discovered_worktree_root,
    sanitized_git_base_command,
};

use super::{
    FullRef, ResolvedSourceSelector, SourceSelectorCommit, SourceSelectorFinalFenceError,
    SourceSelectorKind, SourceSelectorLimits, SourceSelectorObjectFormat,
    SourceSelectorResolutionError, SourceSelectorV1,
};

#[cfg(test)]
pub(super) fn resolve_source_selector(
    root: &Path,
    selector: SourceSelectorV1,
    limits: SourceSelectorLimits,
    cancelled: &AtomicBool,
) -> Result<ResolvedSourceSelector, SourceSelectorResolutionError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(SourceSelectorResolutionError::Cancelled);
    }
    if limits.deadline().is_zero() {
        return Err(SourceSelectorResolutionError::DeadlineExceeded {
            deadline: limits.deadline(),
        });
    }
    let deadline = Instant::now()
        .checked_add(limits.deadline())
        .ok_or(SourceSelectorResolutionError::DeadlineNotRepresentable)?;
    resolve_until(root, selector, limits, cancelled, deadline)
}

pub(super) fn confirm_source_selector(
    captured: &ResolvedSourceSelector,
    limits: SourceSelectorLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SourceSelectorFinalFenceError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(SourceSelectorFinalFenceError::Cancelled);
    }
    if Instant::now() >= deadline || limits.deadline().is_zero() {
        return Err(SourceSelectorFinalFenceError::DeadlineExceeded {
            deadline: limits.deadline(),
        });
    }
    let operation_deadline = Instant::now()
        .checked_add(limits.deadline())
        .ok_or(SourceSelectorFinalFenceError::Inspection {
            source: SourceSelectorResolutionError::DeadlineNotRepresentable,
        })?
        .min(deadline);
    let confirmed = resolve_until(
        &captured.worktree_root,
        captured.selector.clone(),
        limits,
        cancelled,
        operation_deadline,
    )
    .map_err(map_final_fence_error)?;
    if &confirmed != captured {
        return Err(SourceSelectorFinalFenceError::SourceChanged);
    }
    Ok(())
}

pub(super) fn resolve_until(
    root: &Path,
    selector: SourceSelectorV1,
    limits: SourceSelectorLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<ResolvedSourceSelector, SourceSelectorResolutionError> {
    check_control(limits, cancelled, deadline)?;
    let worktree_root = discovered_worktree_root(root)?;
    check_control(limits, cancelled, deadline)?;
    let object_format = resolve_object_format(&worktree_root, limits, cancelled, deadline)?;
    if let SourceSelectorKind::ExactRevision(commit) = selector.kind()
        && commit.object_format() != object_format
    {
        return Err(SourceSelectorResolutionError::ExactRevisionObjectFormatMismatch);
    }
    if let SourceSelectorKind::FullRef(reference) = selector.kind() {
        validate_ref_with_git(&worktree_root, reference, limits, cancelled, deadline)?;
    }
    check_control(limits, cancelled, deadline)?;
    let head = resolve_commit_expression(
        &worktree_root,
        "HEAD^{commit}",
        object_format,
        limits,
        cancelled,
        deadline,
        ResolutionTarget::Head,
    )?;
    check_control(limits, cancelled, deadline)?;
    let selected = match selector.kind() {
        SourceSelectorKind::WorktreeHead => head,
        SourceSelectorKind::ExactRevision(commit) => resolve_commit_expression(
            &worktree_root,
            &format!("{}^{{commit}}", commit.to_hex()),
            object_format,
            limits,
            cancelled,
            deadline,
            ResolutionTarget::Selector,
        )?,
        SourceSelectorKind::FullRef(reference) => resolve_commit_expression(
            &worktree_root,
            &format!("{}^{{commit}}", reference.as_str()),
            object_format,
            limits,
            cancelled,
            deadline,
            ResolutionTarget::Selector,
        )?,
    };
    if head != selected {
        return Err(SourceSelectorResolutionError::WorktreeHeadMismatch);
    }
    check_control(limits, cancelled, deadline)?;
    let moving_ref_digest = selector.moving_ref_digest();
    Ok(ResolvedSourceSelector {
        worktree_root,
        selector,
        commit: selected,
        moving_ref_digest,
    })
}

fn resolve_object_format(
    root: &Path,
    limits: SourceSelectorLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<SourceSelectorObjectFormat, SourceSelectorResolutionError> {
    let mut command = sanitized_git_base_command(root);
    command.arg("rev-parse").arg("--show-object-format");
    let output = capture_git_output_from_command(
        command,
        process_limits(limits),
        deadline,
        &mut cancellation(cancelled),
    )?;
    match single_lf_line(&output) {
        Some(b"sha1") => Ok(SourceSelectorObjectFormat::Sha1),
        Some(b"sha256") => Ok(SourceSelectorObjectFormat::Sha256),
        Some(line)
            if line
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()) =>
        {
            Err(SourceSelectorResolutionError::UnsupportedObjectFormat)
        }
        _ => Err(SourceSelectorResolutionError::InvalidObjectFormat),
    }
}

fn validate_ref_with_git(
    root: &Path,
    reference: &FullRef,
    limits: SourceSelectorLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SourceSelectorResolutionError> {
    let mut command = sanitized_git_base_command(root);
    command
        .arg("check-ref-format")
        .arg("--allow-onelevel")
        .arg(reference.as_str());
    let (status, output) = capture_git_output_with_status_from_command(
        command,
        process_limits(limits),
        deadline,
        &mut cancellation(cancelled),
    )?;
    if status.success() && output.is_empty() {
        return Ok(());
    }
    if output.is_empty() {
        return Err(SourceSelectorResolutionError::InvalidFullRef);
    }
    Err(SourceSelectorResolutionError::InvalidSelectorResolution)
}

#[derive(Clone, Copy)]
enum ResolutionTarget {
    Head,
    Selector,
}

fn resolve_commit_expression(
    root: &Path,
    expression: &str,
    object_format: SourceSelectorObjectFormat,
    limits: SourceSelectorLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
    target: ResolutionTarget,
) -> Result<SourceSelectorCommit, SourceSelectorResolutionError> {
    let mut command = sanitized_git_base_command(root);
    command
        .arg("rev-parse")
        .arg("--verify")
        .arg("--quiet")
        .arg("--end-of-options")
        .arg(expression);
    let (status, output) = capture_git_output_with_status_from_command(
        command,
        process_limits(limits),
        deadline,
        &mut cancellation(cancelled),
    )?;
    if status.success() {
        return parse_commit_output(&output, object_format).ok_or(match target {
            ResolutionTarget::Head => SourceSelectorResolutionError::InvalidHead,
            ResolutionTarget::Selector => SourceSelectorResolutionError::InvalidSelectorResolution,
        });
    }
    if is_exit_code(&status, 1) && output.is_empty() {
        return Err(match target {
            ResolutionTarget::Head => SourceSelectorResolutionError::HeadUnavailable,
            ResolutionTarget::Selector => SourceSelectorResolutionError::SelectorUnavailable,
        });
    }
    Err(GitPathDiscoveryError::GitUnsuccessful {
        code: status.code(),
    }
    .into())
}

fn parse_commit_output(
    output: &[u8],
    object_format: SourceSelectorObjectFormat,
) -> Option<SourceSelectorCommit> {
    let line = single_lf_line(output)?;
    if line.len() != object_format.object_id_bytes() * 2 {
        return None;
    }
    let commit = super::decode_exact_revision(std::str::from_utf8(line).ok()?)?;
    if commit.object_format() != object_format || commit.as_bytes().iter().all(|byte| *byte == 0) {
        return None;
    }
    Some(commit)
}

fn single_lf_line(output: &[u8]) -> Option<&[u8]> {
    let line = output.strip_suffix(b"\n")?;
    if line.is_empty() || line.contains(&b'\n') || line.contains(&b'\r') {
        return None;
    }
    Some(line)
}

fn check_control(
    limits: SourceSelectorLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SourceSelectorResolutionError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(SourceSelectorResolutionError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(SourceSelectorResolutionError::DeadlineExceeded {
            deadline: limits.deadline(),
        });
    }
    Ok(())
}

fn process_limits(limits: SourceSelectorLimits) -> GitPathDiscoveryLimits {
    GitPathDiscoveryLimits::new(
        limits.deadline(),
        limits.output_bytes(),
        1,
        RepositoryPathLimits::new(1, 1),
    )
}

fn cancellation(cancelled: &AtomicBool) -> impl FnMut() -> bool + '_ {
    || cancelled.load(Ordering::Relaxed)
}

fn is_exit_code(status: &ExitStatus, expected: i32) -> bool {
    status.code() == Some(expected)
}

fn map_final_fence_error(source: SourceSelectorResolutionError) -> SourceSelectorFinalFenceError {
    match source {
        SourceSelectorResolutionError::Cancelled => SourceSelectorFinalFenceError::Cancelled,
        SourceSelectorResolutionError::DeadlineExceeded { deadline } => {
            SourceSelectorFinalFenceError::DeadlineExceeded { deadline }
        }
        SourceSelectorResolutionError::HeadUnavailable
        | SourceSelectorResolutionError::ExactRevisionObjectFormatMismatch
        | SourceSelectorResolutionError::InvalidFullRef
        | SourceSelectorResolutionError::SelectorUnavailable
        | SourceSelectorResolutionError::WorktreeHeadMismatch => {
            SourceSelectorFinalFenceError::SourceChanged
        }
        source => SourceSelectorFinalFenceError::Inspection { source },
    }
}
