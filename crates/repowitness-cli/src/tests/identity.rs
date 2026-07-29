use std::cell::{Cell, RefCell};

use super::*;

struct FakeIdentityGenerator {
    calls: Cell<u64>,
    kind: Cell<Option<LocalIdentityKind>>,
    outcome: RefCell<Result<String, LocalIdentityGenerationError>>,
}

impl FakeIdentityGenerator {
    fn success(identity: &str) -> Self {
        Self {
            calls: Cell::new(0),
            kind: Cell::new(None),
            outcome: RefCell::new(Ok(identity.to_owned())),
        }
    }

    fn failure() -> Self {
        Self {
            calls: Cell::new(0),
            kind: Cell::new(None),
            outcome: RefCell::new(Err(LocalIdentityGenerationError::EntropyUnavailable)),
        }
    }
}

impl IdentityGenerator for FakeIdentityGenerator {
    fn generate(&self, kind: LocalIdentityKind) -> Result<String, LocalIdentityGenerationError> {
        self.calls.set(self.calls.get() + 1);
        self.kind.set(Some(kind));
        self.outcome.borrow().clone()
    }
}

fn invoke_identity(arguments: &[&str], generator: &impl IdentityGenerator) -> (u8, String, String) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_identity(
        arguments.iter().map(OsString::from),
        &mut stdout,
        &mut stderr,
        generator,
    );
    (
        code,
        String::from_utf8(stdout).expect("identity stdout should be UTF-8"),
        String::from_utf8(stderr).expect("identity stderr should be UTF-8"),
    )
}

#[test]
fn exact_generate_grammar_maps_every_allowlisted_kind_and_emits_plain_text() {
    let cases = [
        (
            "repository",
            LocalIdentityKind::Repository,
            format!("rwi1:h:{}", "AB".repeat(32)),
        ),
        (
            "connected-workspace",
            LocalIdentityKind::ConnectedWorkspace,
            format!("cwi1:h:{}", "BC".repeat(32)),
        ),
        (
            "source-slot",
            LocalIdentityKind::SourceSlot,
            format!("ssi1:h:{}", "CD".repeat(32)),
        ),
    ];
    for (argument, expected_kind, identity) in cases {
        let generator = FakeIdentityGenerator::success(&identity);
        let (code, stdout, stderr) = invoke_identity(&["generate", argument], &generator);

        assert_eq!(code, EXIT_SUCCESS);
        assert_eq!(stdout, format!("{identity}\n"));
        assert!(stderr.is_empty());
        assert_eq!(generator.calls.get(), 1);
        assert_eq!(generator.kind.get(), Some(expected_kind));
    }
}

#[test]
fn help_and_invalid_grammar_do_not_request_entropy() {
    let invalid = [
        (&["--help"][..], EXIT_SUCCESS),
        (&["-h"][..], EXIT_SUCCESS),
        (&["generate", "--help"][..], EXIT_SUCCESS),
        (&["generate", "-h"][..], EXIT_SUCCESS),
        (&[][..], EXIT_USAGE),
        (&["generate"][..], EXIT_USAGE),
        (&["create", "repository"][..], EXIT_USAGE),
        (&["generate", "workspace"][..], EXIT_USAGE),
        (&["generate", "repository", "extra"][..], EXIT_USAGE),
        (&["--", "generate", "repository"][..], EXIT_USAGE),
    ];
    for (arguments, expected_code) in invalid {
        let generator = FakeIdentityGenerator::failure();
        let (code, _, _) = invoke_identity(arguments, &generator);

        assert_eq!(code, expected_code, "arguments: {arguments:?}");
        assert_eq!(generator.calls.get(), 0, "arguments: {arguments:?}");
    }
}

#[test]
fn entropy_failure_is_generic_and_emits_no_identity() {
    let generator = FakeIdentityGenerator::failure();
    let (code, stdout, stderr) = invoke_identity(&["generate", "repository"], &generator);

    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "error: identity generation failed\n");
    assert_eq!(generator.calls.get(), 1);
}
