use repowitness_domain::ConnectedWorkspaceId;

use crate::sqlite::{
    WorkspaceViewId,
    writer::{
        expected_view_member_count, load_pinned_view_members, validate_pinned_view_members,
    },
};

impl OwnedSqliteReader {
    /// Pins the current or one exact published immutable workspace view.
    pub fn pin_workspace_view(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        requested_view: Option<i64>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Option<PinnedWorkspaceView>, SqliteStoreError> {
        if requested_view.is_some_and(|view| view <= 0) {
            return Err(SqliteStoreError::InvalidWorkspaceView);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::WorkspaceView(Box::new(WorkspaceViewCommand {
                connected_workspace,
                requested_view,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(view) => Ok(view),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}

fn execute_workspace_view_command(
    connection: &mut Connection,
    command: &WorkspaceViewCommand,
) -> Result<Option<PinnedWorkspaceView>, SqliteStoreError> {
    check_control(&command.cancelled, command.deadline)?;
    let progress_cancelled = Arc::clone(&command.cancelled);
    let deadline = command.deadline;
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || {
                progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline
            }),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = pin_workspace_view_transaction(
        connection,
        command.connected_workspace,
        command.requested_view,
    );
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(view) => {
            check_control(&command.cancelled, command.deadline)?;
            Ok(view)
        }
        Err(error) if error.sqlite_error_code() == Some(ErrorCode::OperationInterrupted) => {
            check_control(&command.cancelled, command.deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(_) => Err(SqliteStoreError::DatabaseOperationFailed),
    }
}

fn pin_workspace_view_transaction(
    connection: &mut Connection,
    connected_workspace: ConnectedWorkspaceId,
    requested_view: Option<i64>,
) -> rusqlite::Result<Option<PinnedWorkspaceView>> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let selected = match requested_view {
        Some(view) => transaction
            .query_row(
                "SELECT workspace_view_id
                 FROM workspace_views
                 WHERE connected_workspace_id = ?1
                   AND workspace_view_id = ?2
                   AND lifecycle_state = 'published'",
                params![connected_workspace.as_bytes().as_slice(), view],
                |row| row.get::<_, i64>(0),
            )
            .optional()?,
        None => transaction
            .query_row(
                "SELECT active.workspace_view_id
                 FROM active_workspace_views AS active
                 JOIN workspace_views AS view
                   ON view.connected_workspace_id = active.connected_workspace_id
                  AND view.workspace_view_id = active.workspace_view_id
                 WHERE active.connected_workspace_id = ?1
                   AND view.lifecycle_state = 'published'",
                [connected_workspace.as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?,
    };
    let Some(selected) = selected else {
        return Ok(None);
    };
    let view = WorkspaceViewId::from_database(selected);
    let expected = expected_view_member_count(&transaction, connected_workspace, view)
        .map_err(store_as_sqlite)?;
    let members = load_pinned_view_members(&transaction, view).map_err(store_as_sqlite)?;
    validate_pinned_view_members(&members, expected).map_err(store_as_sqlite)?;
    Ok(Some(PinnedWorkspaceView::new(
        connected_workspace,
        view,
        members,
    )))
}

fn store_as_sqlite(_error: SqliteStoreError) -> rusqlite::Error {
    rusqlite::Error::InvalidQuery
}
