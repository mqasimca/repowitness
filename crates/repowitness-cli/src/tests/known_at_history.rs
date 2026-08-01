use super::*;

#[test]
fn known_at_history_requires_an_exact_lowercase_target() {
    let arguments = vec![
        OsString::from("--repository-id"),
        OsString::from("rwri1_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        OsString::from("--database"),
        OsString::from("memory.sqlite3"),
        OsString::from("--known-at"),
        OsString::from("1722000000000"),
        OsString::from("--git-commit"),
        OsString::from("ab".repeat(20)),
        OsString::from("repository"),
    ];
    let invocation = parse_known_at_history_arguments(&arguments)
        .expect("the exact lower-case commit should parse");
    assert_eq!(invocation.max_results, 32);
    match invocation.target {
        MemoryObservationSource::Git(MemoryCommitId::Sha1(bytes)) => {
            assert_eq!(bytes, [0xab; 20]);
        }
        _ => panic!("the SHA-1 target must preserve its exact object format"),
    }

    let mut upper_case = arguments;
    upper_case[7] = OsString::from("AB".repeat(20));
    match parse_known_at_history_arguments(&upper_case) {
        Err(error) => assert_eq!(
            error,
            "error: memory-history target must be lowercase hexadecimal\n"
        ),
        Ok(_) => panic!("upper-case commit must be rejected"),
    }
}

#[test]
fn known_at_history_rejects_branch_like_and_multiple_targets() {
    let arguments = vec![
        OsString::from("--repository-id"),
        OsString::from("repository"),
        OsString::from("--database"),
        OsString::from("memory.sqlite3"),
        OsString::from("--known-at"),
        OsString::from("1"),
        OsString::from("--git-commit"),
        OsString::from("main"),
        OsString::from("repository"),
    ];
    match parse_known_at_history_arguments(&arguments) {
        Err(error) => assert_eq!(
            error,
            "error: memory-history Git target must be lowercase SHA-1 or SHA-256\n"
        ),
        Ok(_) => panic!("branch-like target must be rejected"),
    }
}
