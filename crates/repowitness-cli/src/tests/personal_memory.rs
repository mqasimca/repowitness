use std::{ffi::OsString, process};

use super::super::run_personal_memory;

#[test]
fn personal_memory_append_and_read_are_explicit_profile_scoped() {
    let database = std::env::temp_dir().join(format!(
        "repowitness-cli-personal-memory-{}-{}.db",
        process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos(),
    ));
    let repository = format!("rwi1:h:{}", "12".repeat(32));
    let profile = "ab".repeat(16);
    let append = vec![
        OsString::from("append"),
        OsString::from("--repository-id"),
        OsString::from(&repository),
        OsString::from("--database"),
        database.as_os_str().to_os_string(),
        OsString::from("--profile"),
        OsString::from(&profile),
        OsString::from("--kind"),
        OsString::from("preference"),
        OsString::from("--title"),
        OsString::from("prefer local evidence"),
        OsString::from("--body"),
        OsString::from("never publish this personal memory"),
        OsString::from("--lifecycle"),
        OsString::from("active"),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    assert_eq!(
        run_personal_memory(append.into_iter(), &mut stdout, &mut stderr),
        0
    );
    assert!(stderr.is_empty());
    let receipt = String::from_utf8(stdout).expect("receipt is UTF-8");
    assert!(receipt.contains("operation=personal-memory-append\n"));
    assert!(receipt.contains("scope=personal\n"));
    assert!(!receipt.contains("prefer local evidence"));

    let read = vec![
        OsString::from("read"),
        OsString::from("--repository-id"),
        OsString::from(&repository),
        OsString::from("--database"),
        database.as_os_str().to_os_string(),
        OsString::from("--profile"),
        OsString::from(&profile),
        OsString::from("--limit"),
        OsString::from("1"),
    ];
    let mut stdout = Vec::new();
    assert_eq!(
        run_personal_memory(read.into_iter(), &mut stdout, &mut stderr),
        0
    );
    assert!(stderr.is_empty());
    let output: serde_json::Value = serde_json::from_slice(&stdout).expect("read output is JSON");
    assert_eq!(output["scope"], "personal");
    assert_eq!(output["records"].as_array().expect("records").len(), 1);
    assert_eq!(output["records"][0]["title"], "prefer local evidence");
}

#[test]
fn personal_memory_rejects_profile_substitution_and_sensitive_content() {
    let repository = format!("rwi1:h:{}", "34".repeat(32));
    let database = std::env::temp_dir().join(format!(
        "repowitness-cli-personal-memory-secret-{}-{}.db",
        process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos(),
    ));
    let invalid_profile = vec![
        OsString::from("read"),
        OsString::from("--repository-id"),
        OsString::from(&repository),
        OsString::from("--database"),
        database.as_os_str().to_os_string(),
        OsString::from("--profile"),
        OsString::from("AA".repeat(16)),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_personal_memory(invalid_profile.into_iter(), &mut stdout, &mut stderr),
        64
    );
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        b"error: personal-memory --profile must be 32 lowercase hex characters\n"
    );

    let secret = vec![
        OsString::from("append"),
        OsString::from("--repository-id"),
        OsString::from(&repository),
        OsString::from("--database"),
        database.as_os_str().to_os_string(),
        OsString::from("--profile"),
        OsString::from("ab".repeat(16)),
        OsString::from("--kind"),
        OsString::from("fact"),
        OsString::from("--title"),
        OsString::from("credential"),
        OsString::from("--body"),
        OsString::from("AKIA1234567890ABCDEF"),
        OsString::from("--lifecycle"),
        OsString::from("active"),
    ];
    let mut stderr = Vec::new();
    assert_eq!(
        run_personal_memory(secret.into_iter(), &mut stdout, &mut stderr),
        70
    );
    assert_eq!(stderr, b"error: personal-memory append failed\n");
    assert!(
        !database.exists(),
        "secret admission must not initialize a database"
    );
}
