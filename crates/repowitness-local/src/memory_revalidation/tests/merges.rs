use repowitness_application::MemoryEvidenceOutcome;
use repowitness_domain::{CorrespondenceFingerprintDigest, RepositoryPath};

use super::*;
use crate::{
    memory_revalidation::reject_automatic_many_to_one,
    sqlite::memory_projection::{
        PreparedProjectionEvidence, ProjectionCandidateRelation, ProjectionEvidenceAssurance,
        ProjectionEvidenceOutcome, ProjectionOccurrence,
    },
};

fn occurrence(byte: u8, ordinal: u64) -> ProjectionOccurrence {
    ProjectionOccurrence::new(
        RepositoryPath::try_from_bytes(
            if byte == 1 {
                b"src/merged.rs"
            } else {
                b"src/other.rs"
            },
            RepositoryPathLimits::new(1_048_576, 65_535),
        )
        .expect("fixture path should be valid"),
        AnalysisArtifactDigest::new([byte; 32]),
        ordinal,
        DeclarationDigest::new([byte; 32]),
        CorrespondenceFingerprintDigest::new([byte; 32]),
    )
}

fn automatic(target: ProjectionOccurrence) -> PreparedProjectionEvidence {
    PreparedProjectionEvidence::resolved(
        ProjectionEvidenceOutcome::SamePathRename,
        ProjectionEvidenceAssurance::Automatic,
        target,
        1,
    )
    .expect("automatic fixture should be valid")
}

#[test]
fn one_to_many_split_candidates_require_review_without_an_automatic_relink() {
    let (_fixture, repository, database, _repository_identity, identity) =
        exact_commit_projection_fixture();
    fs::write(
        repository.join("src/lib.rs"),
        b"pub fn first() -> bool { true }\npub fn second() -> bool { true }\n",
    )
    .expect("split source should be written");
    git(&repository, &["add", "src/lib.rs"]);
    git(&repository, &["commit", "--quiet", "-m", "split function"]);
    index_local_repository(
        LocalIndexRequest::new(&repository, &database, &identity, 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("split source index should activate");

    let report = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, &identity, 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("split projection should activate conservatively");
    assert_eq!(report.unresolved_records(), 1);

    let connection = Connection::open(database).expect("database should open");
    let state: (String, String, String, i64, i64) = connection
        .query_row(
            "SELECT record.effective_state, evidence.outcome, evidence.assurance,
                    count(candidate.ordinal),
                    count(candidate.ordinal) FILTER (
                        WHERE candidate.proposed_relation = 'split'
                    )
             FROM active_memory_projections AS active
             JOIN memory_projection_records AS record
               ON record.projection_id = active.projection_id
             JOIN memory_projection_evidence AS evidence
               ON evidence.projection_id = record.projection_id
              AND evidence.record_ordinal = record.ordinal
             LEFT JOIN memory_projection_candidates AS candidate
               ON candidate.projection_id = evidence.projection_id
              AND candidate.record_ordinal = evidence.record_ordinal
              AND candidate.evidence_ordinal = evidence.evidence_ordinal
             GROUP BY record.effective_state, evidence.outcome, evidence.assurance",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("split projection should be readable");
    assert_eq!(
        state,
        (
            "needs_review".to_owned(),
            "ambiguous".to_owned(),
            "none".to_owned(),
            2,
            2,
        )
    );
}

#[test]
fn repeated_evidence_cannot_become_current_through_one_automatic_target() {
    let fixture = TempDirectory::new();
    let repository = fixture.repository();
    let database = fixture.database();
    initialize_repository(&repository);
    let repository_identity = RepositoryIdentityDigest::new([0xA5; 32]);
    let identity = RepositoryIdentityTextV1::encode(repository_identity);
    index_local_repository(
        LocalIndexRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("source index should activate");
    import_repeated_exact_memory(&database, repository_identity, 2);

    let report = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("many-to-one projection should activate conservatively");
    assert_eq!(report.unresolved_records(), 1);

    let connection = Connection::open(database).expect("database should open");
    let evidence: (String, i64, i64, i64) = connection
        .query_row(
            "SELECT record.effective_state,
                    count(*),
                    count(*) FILTER (WHERE evidence.outcome = 'ambiguous'),
                    count(*) FILTER (WHERE evidence.assurance = 'none')
             FROM active_memory_projections AS active
             JOIN memory_projection_records AS record
               ON record.projection_id = active.projection_id
             JOIN memory_projection_evidence AS evidence
               ON evidence.projection_id = record.projection_id
              AND evidence.record_ordinal = record.ordinal
             GROUP BY record.effective_state",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("projected evidence should be readable");
    assert_eq!(evidence, ("needs_review".to_owned(), 2, 2, 2));
    let candidates: (i64, i64) = connection
        .query_row(
            "SELECT count(*),
                    count(*) FILTER (WHERE proposed_relation = 'merged')
             FROM memory_projection_candidates",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("merge candidates should be readable");
    assert_eq!(candidates, (2, 2));
}

#[test]
fn automatic_many_to_one_targets_require_review_in_every_input_order() {
    for arrangement in [[0_u8, 1, 0], [1_u8, 0, 0]] {
        let merged_target = occurrence(1, 7);
        let independent_target = occurrence(2, 9);
        let mut outcomes = arrangement
            .iter()
            .map(|_| MemoryEvidenceOutcome::Corresponded)
            .collect::<Vec<_>>();
        let mut evidence = arrangement
            .iter()
            .map(|selector| {
                automatic(if *selector == 0 {
                    merged_target.clone()
                } else {
                    independent_target.clone()
                })
            })
            .collect::<Vec<_>>();

        reject_automatic_many_to_one(&mut outcomes, &mut evidence)
            .expect("many-to-one correspondence should be classified");

        for (ordinal, selector) in arrangement.iter().enumerate() {
            if *selector == 0 {
                assert_eq!(outcomes[ordinal], MemoryEvidenceOutcome::NeedsReview);
                assert_eq!(
                    evidence[ordinal].outcome,
                    ProjectionEvidenceOutcome::Ambiguous
                );
                assert_eq!(
                    evidence[ordinal].assurance,
                    ProjectionEvidenceAssurance::None
                );
                assert!(evidence[ordinal].target.is_none());
                assert_eq!(evidence[ordinal].candidates.len(), 1);
                assert_eq!(
                    evidence[ordinal].candidates[0].relation,
                    ProjectionCandidateRelation::Merged
                );
                assert_eq!(evidence[ordinal].candidates[0].occurrence, merged_target);
            } else {
                assert_eq!(outcomes[ordinal], MemoryEvidenceOutcome::Corresponded);
                assert_eq!(
                    evidence[ordinal].outcome,
                    ProjectionEvidenceOutcome::SamePathRename
                );
                assert_eq!(evidence[ordinal].target.as_ref(), Some(&independent_target));
            }
        }
    }
}

#[test]
fn an_explicit_review_remains_authoritative_while_the_automatic_merge_abstains() {
    let target = occurrence(1, 7);
    let mut outcomes = vec![
        MemoryEvidenceOutcome::Corresponded,
        MemoryEvidenceOutcome::Corresponded,
    ];
    let mut evidence = vec![
        PreparedProjectionEvidence::reviewed_link(target.clone(), 1, true),
        automatic(target.clone()),
    ];

    reject_automatic_many_to_one(&mut outcomes, &mut evidence)
        .expect("reviewed and automatic targets should be classified");

    assert_eq!(outcomes[0], MemoryEvidenceOutcome::Corresponded);
    assert_eq!(evidence[0].outcome, ProjectionEvidenceOutcome::ReviewedLink);
    assert_eq!(evidence[0].assurance, ProjectionEvidenceAssurance::Reviewed);
    assert_eq!(evidence[0].target.as_ref(), Some(&target));

    assert_eq!(outcomes[1], MemoryEvidenceOutcome::NeedsReview);
    assert_eq!(evidence[1].outcome, ProjectionEvidenceOutcome::Ambiguous);
    assert_eq!(
        evidence[1].candidates[0].relation,
        ProjectionCandidateRelation::Merged
    );
}
