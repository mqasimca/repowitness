use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use super::*;

#[test]
fn request_limits_are_positive_bounded_and_debug_is_redacted() {
    let identity = format!("rwi1:h:{}", "AB".repeat(32));
    let index = LocalIndexRequest::new(
        Path::new("/private/repository"),
        Path::new("/private/index.sqlite3"),
        &identity,
        0,
    );
    let request = LocalWatchRequest::new(index)
        .with_max_runtime(Duration::from_millis(1))
        .expect("minimum positive runtime");
    let debug = format!("{request:?}");
    assert!(!debug.contains("/private"));
    assert!(!debug.contains(&identity));

    for duration in [
        Duration::ZERO,
        MAX_LOCAL_WATCH_RUNTIME + Duration::from_millis(1),
    ] {
        assert!(matches!(
            LocalWatchRequest::new(index).with_max_runtime(duration),
            Err(LocalWatchRequestError::MaxRuntime)
        ));
    }
}

#[test]
fn preexisting_cancellation_returns_without_filesystem_or_database_access() {
    let index = LocalIndexRequest::new(
        Path::new("/must/not/be/read"),
        Path::new("/must/not/be/created.sqlite3"),
        "invalid-identity-that-must-not-be-decoded",
        0,
    );
    let cancelled = Arc::new(AtomicBool::new(true));

    let report = watch_local_repository(LocalWatchRequest::new(index), cancelled.clone())
        .expect("pre-cancelled watch exits normally");

    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(report.exit(), LocalWatchExit::Cancelled);
    assert_eq!(report.state_counters(), WatcherStateCounters::default());
    assert_eq!(report.hint_counters(), WatcherHintCounters::default());
    assert_eq!(report.last_reconciliation(), None);
    assert_eq!(report.last_index(), None);
}
