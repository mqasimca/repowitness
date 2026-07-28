use repowitness_domain::{MemoryEvidence, MemoryRecord};

use super::LocalMemoryManageError;

const SENSITIVE_KEY_SPELLINGS: &[&[u8]] = &[
    b"password",
    b"passwd",
    b"secret",
    b"client_secret",
    b"client-secret",
    b"clientsecret",
    b"access_token",
    b"access-token",
    b"accesstoken",
    b"refresh_token",
    b"refresh-token",
    b"refreshtoken",
    b"api_key",
    b"api-key",
    b"apikey",
    b"private_key",
    b"private-key",
    b"privatekey",
];

const TOKEN_PREFIXES: &[(&[u8], usize)] = &[
    (b"AKIA", 20),
    (b"ghp_", 20),
    (b"github_pat_", 24),
    (b"glpat-", 20),
    (b"xoxb-", 20),
    (b"xoxp-", 20),
    (b"sk_live_", 20),
    (b"sk-proj-", 20),
];

pub(super) fn check_record(record: &MemoryRecord) -> Result<(), LocalMemoryManageError> {
    check_text(record.claim().title().as_str())?;
    check_text(record.claim().body().as_str())?;
    check_text(record.provenance().actor_id().as_str())?;
    for evidence in record.evidence() {
        let MemoryEvidence::RustSymbol(evidence) = evidence;
        check_text(evidence.name().as_str())?;
        check_text(evidence.qualified_name().as_str())?;
        check_text(evidence.producer().id().as_str())?;
        check_text(evidence.producer().version().as_str())?;
    }
    Ok(())
}

fn check_text(text: &str) -> Result<(), LocalMemoryManageError> {
    let bytes = text.as_bytes();
    if contains_ascii_case_insensitive(bytes, b"-----BEGIN PRIVATE KEY-----")
        || contains_ascii_case_insensitive(bytes, b"-----BEGIN RSA PRIVATE KEY-----")
        || contains_ascii_case_insensitive(bytes, b"-----BEGIN OPENSSH PRIVATE KEY-----")
        || TOKEN_PREFIXES
            .iter()
            .any(|(prefix, minimum)| contains_token_prefix(bytes, prefix, *minimum))
        || SENSITIVE_KEY_SPELLINGS
            .iter()
            .any(|key| contains_nonempty_assignment(bytes, key))
    {
        Err(LocalMemoryManageError::SensitiveContent)
    } else {
        Ok(())
    }
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn contains_token_prefix(bytes: &[u8], prefix: &[u8], minimum: usize) -> bool {
    bytes
        .windows(prefix.len())
        .enumerate()
        .filter(|(_, window)| *window == prefix)
        .any(|(start, _)| {
            let end = bytes[start..]
                .iter()
                .position(|byte| !token_byte(*byte))
                .map_or(bytes.len(), |offset| start + offset);
            end.saturating_sub(start) >= minimum
        })
}

fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn contains_nonempty_assignment(bytes: &[u8], key: &[u8]) -> bool {
    bytes
        .windows(key.len())
        .enumerate()
        .filter(|(_, window)| window.eq_ignore_ascii_case(key))
        .any(|(start, _)| assignment_has_value(bytes, start, key.len()))
}

fn assignment_has_value(bytes: &[u8], start: usize, key_len: usize) -> bool {
    if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        return false;
    }
    let mut cursor = start + key_len;
    if bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b'\'' | b'"'))
    {
        cursor += 1;
    }
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    if !bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b':' | b'='))
    {
        return false;
    }
    cursor += 1;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    if bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b'\'' | b'"'))
    {
        cursor += 1;
    }
    bytes
        .get(cursor)
        .is_some_and(|byte| !matches!(byte, b'\r' | b'\n' | b'\'' | b'"'))
}

#[cfg(test)]
mod tests {
    use super::check_text;

    #[test]
    fn high_confidence_secret_forms_are_rejected_without_echoing_values() {
        for text in [
            "password = hunter2",
            r#"{"api_key":"private-value"}"#,
            r#"{"apiKey":"private-value"}"#,
            "CLIENT-SECRET = private-value",
            "refreshToken: private-value",
            "private-key: private-value",
            "client_secret: private-value",
            "AKIAABCDEFGHIJKLMNOP",
            "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
        ] {
            let error = check_text(text).expect_err("sensitive form should fail");
            assert_eq!(
                error.to_string(),
                "memory record contains disallowed sensitive material"
            );
            assert!(!error.to_string().contains("private-value"));
        }
    }

    #[test]
    fn ordinary_security_guidance_and_empty_assignments_are_allowed() {
        for text in [
            "Never log passwords or API keys.",
            "password = \"\"",
            "api-key:",
            "monkey = ordinary-value",
            "the secret rotation procedure",
            "Use a redacted credential placeholder.",
        ] {
            check_text(text).expect("non-secret guidance should pass");
        }
    }
}
