use std::time::{Duration, Instant};

use repowitness_domain::RepositoryIdentityDigest;
use rusqlite::Connection;

use super::TempDirectory;
use crate::{
    LocalMemoryDatabaseIdentity, LocalMemoryMaintenance, OwnedSqliteIndex,
    memory_management::finish_known_memory_mutation,
};

#[test]
fn maintenance_status_covers_every_truthful_completion_pair() {
    assert_eq!(
        LocalMemoryMaintenance::from_completion(true, true),
        LocalMemoryMaintenance::Complete
    );
    assert_eq!(
        LocalMemoryMaintenance::from_completion(false, true),
        LocalMemoryMaintenance::CheckpointDeferred
    );
    assert_eq!(
        LocalMemoryMaintenance::from_completion(true, false),
        LocalMemoryMaintenance::ShutdownDeferred
    );
    assert_eq!(
        LocalMemoryMaintenance::from_completion(false, false),
        LocalMemoryMaintenance::CheckpointAndShutdownDeferred
    );
    for database_identity in [
        LocalMemoryDatabaseIdentity::ChangedAfterCommit,
        LocalMemoryDatabaseIdentity::Unconfirmed,
    ] {
        let maintenance = LocalMemoryMaintenance::from_evidence(true, true, database_identity);
        assert!(!maintenance.complete());
        assert_eq!(maintenance.warning_count(), 1);
        assert_eq!(maintenance.database_identity(), database_identity);
    }
}

#[test]
fn post_commit_maintenance_deadline_preserves_the_known_receipt() {
    let outside = TempDirectory::new("post-commit-maintenance");
    let database = outside.path().join("index.sqlite3");
    let deadline = Instant::now() + Duration::from_secs(5);
    let (store, _) = OwnedSqliteIndex::start(&database, 1_722_000_000_000, deadline)
        .expect("store should start");

    let (receipt, maintenance) = finish_known_memory_mutation(store, 37_u8, Instant::now());
    assert_eq!(receipt, 37);
    assert_eq!(
        maintenance,
        LocalMemoryMaintenance::CheckpointAndShutdownDeferred
    );

    let connection = Connection::open(database).expect("database should reopen");
    let integrity: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .expect("database integrity should be readable");
    assert_eq!(integrity, "ok");
}

#[test]
fn busy_checkpoint_is_truthfully_deferred_while_the_known_receipt_is_preserved() {
    let outside = TempDirectory::new("busy-post-commit-checkpoint");
    let database = outside.path().join("index.sqlite3");
    let repository = RepositoryIdentityDigest::new([0x73; 32]);
    let deadline = Instant::now() + Duration::from_secs(5);
    let (store, _) = OwnedSqliteIndex::start(&database, 1_722_000_000_000, deadline)
        .expect("store should start");
    store
        .register_workspace(repository, 0, deadline)
        .expect("workspace should commit");

    let reader = Connection::open(&database).expect("reader should open");
    reader
        .execute_batch("BEGIN DEFERRED")
        .expect("reader transaction should begin");
    let observed_epoch: i64 = reader
        .query_row("SELECT source_epoch FROM workspaces", [], |row| row.get(0))
        .expect("reader should pin the pre-mutation WAL snapshot");
    assert_eq!(observed_epoch, 0);
    store
        .advance_source_epoch(repository, 0, 1, deadline)
        .expect("post-reader mutation should commit");

    let (receipt, maintenance) = finish_known_memory_mutation(store, 41_u8, deadline);
    assert_eq!(receipt, 41);
    assert_eq!(maintenance, LocalMemoryMaintenance::CheckpointDeferred);

    reader
        .execute_batch("ROLLBACK")
        .expect("reader transaction should release its WAL pin");
    drop(reader);
    let connection = Connection::open(database).expect("database should reopen");
    let durable_epoch: i64 = connection
        .query_row("SELECT source_epoch FROM workspaces", [], |row| row.get(0))
        .expect("committed epoch should remain readable");
    assert_eq!(durable_epoch, 1);
    let integrity: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .expect("database integrity should be readable");
    assert_eq!(integrity, "ok");
}
