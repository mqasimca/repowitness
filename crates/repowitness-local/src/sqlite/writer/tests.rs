use rusqlite::ffi::{Error, SQLITE_CORRUPT, SQLITE_INTERRUPT};

use super::{SqliteStoreError, projection_validation_error};

#[test]
fn projection_validation_preserves_interruption_for_control_diagnostics() {
    let interrupted = rusqlite::Error::SqliteFailure(Error::new(SQLITE_INTERRUPT), None);
    let corrupt = rusqlite::Error::SqliteFailure(Error::new(SQLITE_CORRUPT), None);

    assert_eq!(
        projection_validation_error(interrupted),
        SqliteStoreError::DatabaseOperationFailed
    );
    assert_eq!(
        projection_validation_error(corrupt),
        SqliteStoreError::IntegrityCheckFailed
    );
}
