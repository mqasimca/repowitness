//! Test-only FTS5 retrieval and generation-scoping spike.

use std::{error::Error, fmt};

use rusqlite::{Connection, params};

const MAX_QUERY_BYTES: usize = 256;
const MAX_QUERY_TERMS: usize = 8;
const MAX_QUERY_TERM_BYTES: usize = 64;
const MAX_RESULTS: u32 = 100;

#[derive(Clone, Eq, PartialEq)]
struct SearchHit {
    path: Vec<u8>,
    kind: String,
    qualified_name: String,
}

impl fmt::Debug for SearchHit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchHit")
            .field("path_bytes", &self.path.len())
            .field("kind_bytes", &self.kind.len())
            .field("qualified_name_bytes", &self.qualified_name.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SearchError {
    EmptyQuery,
    QueryTooLarge,
    TooManyTerms,
    TermTooLarge,
    InvalidResultLimit,
    Database,
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyQuery => "search query is empty",
            Self::QueryTooLarge => "search query exceeds its byte limit",
            Self::TooManyTerms => "search query contains too many terms",
            Self::TermTooLarge => "search query term exceeds its byte limit",
            Self::InvalidResultLimit => "search result limit is invalid",
            Self::Database => "search database operation failed",
        })
    }
}

impl Error for SearchError {}

fn create_schema(connection: &Connection) -> Result<(), SearchError> {
    connection
        .execute_batch(
            "
            PRAGMA trusted_schema = OFF;
            CREATE TABLE workspaces (
                id INTEGER PRIMARY KEY,
                active_generation_id INTEGER
            ) STRICT;
            CREATE TABLE generation_facts (
                id INTEGER PRIMARY KEY,
                generation_id INTEGER NOT NULL,
                ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
                path BLOB NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
                kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 64),
                name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 1024),
                qualified_name TEXT NOT NULL
                    CHECK (length(qualified_name) BETWEEN 1 AND 4096),
                UNIQUE (generation_id, ordinal)
            ) STRICT;
            CREATE VIRTUAL TABLE fact_search USING fts5(
                name,
                qualified_name,
                tokenize = 'unicode61 remove_diacritics 0 tokenchars _'
            );
            ",
        )
        .map_err(|_| SearchError::Database)
}

fn insert_fact(
    connection: &Connection,
    generation: i64,
    ordinal: i64,
    path: &[u8],
    kind: &str,
    name: &str,
    qualified_name: &str,
) -> Result<(), SearchError> {
    connection
        .execute(
            "INSERT INTO generation_facts (
                 generation_id, ordinal, path, kind, name, qualified_name
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![generation, ordinal, path, kind, name, qualified_name],
        )
        .map_err(|_| SearchError::Database)?;
    let row_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO fact_search (rowid, name, qualified_name)
             VALUES (?1, ?2, ?3)",
            params![row_id, name, qualified_name],
        )
        .map_err(|_| SearchError::Database)?;
    Ok(())
}

fn literal_match_query(query: &str) -> Result<String, SearchError> {
    if query.len() > MAX_QUERY_BYTES {
        return Err(SearchError::QueryTooLarge);
    }
    let terms = query.split_whitespace().collect::<Vec<_>>();
    if terms.is_empty() {
        return Err(SearchError::EmptyQuery);
    }
    if terms.len() > MAX_QUERY_TERMS {
        return Err(SearchError::TooManyTerms);
    }
    if terms.iter().any(|term| term.len() > MAX_QUERY_TERM_BYTES) {
        return Err(SearchError::TermTooLarge);
    }

    Ok(terms
        .iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND "))
}

fn search_active_generation(
    connection: &Connection,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchHit>, SearchError> {
    if limit == 0 || limit > MAX_RESULTS {
        return Err(SearchError::InvalidResultLimit);
    }
    let match_query = literal_match_query(query)?;
    let generation = connection
        .query_row(
            "SELECT active_generation_id FROM workspaces WHERE id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|_| SearchError::Database)?;
    let mut statement = connection
        .prepare(
            "SELECT facts.path, facts.kind, facts.qualified_name
             FROM fact_search
             JOIN generation_facts AS facts ON facts.id = fact_search.rowid
             WHERE fact_search MATCH ?1
               AND facts.generation_id = ?2
             ORDER BY bm25(fact_search, 10.0, 5.0) ASC,
                      facts.path ASC,
                      facts.ordinal ASC
             LIMIT ?3",
        )
        .map_err(|_| SearchError::Database)?;
    let rows = statement
        .query_map(params![match_query, generation, i64::from(limit)], |row| {
            Ok(SearchHit {
                path: row.get(0)?,
                kind: row.get(1)?,
                qualified_name: row.get(2)?,
            })
        })
        .map_err(|_| SearchError::Database)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| SearchError::Database)
}

fn rebuild_generation_projection(
    connection: &Connection,
    generation: i64,
) -> Result<(), SearchError> {
    connection
        .execute(
            "DELETE FROM fact_search
             WHERE rowid IN (
                 SELECT id FROM generation_facts WHERE generation_id = ?1
             )",
            [generation],
        )
        .map_err(|_| SearchError::Database)?;
    connection
        .execute(
            "INSERT INTO fact_search (rowid, name, qualified_name)
             SELECT id, name, qualified_name
             FROM generation_facts
             WHERE generation_id = ?1
             ORDER BY ordinal",
            [generation],
        )
        .map_err(|_| SearchError::Database)?;
    Ok(())
}

fn fixture() -> Result<Connection, SearchError> {
    let connection = Connection::open_in_memory().map_err(|_| SearchError::Database)?;
    create_schema(&connection)?;
    connection
        .execute(
            "INSERT INTO workspaces (id, active_generation_id) VALUES (1, 2)",
            [],
        )
        .map_err(|_| SearchError::Database)?;
    insert_fact(
        &connection,
        1,
        0,
        b"src/old.rs",
        "function",
        "stable",
        "old::stable",
    )?;
    insert_fact(
        &connection,
        1,
        1,
        b"src/old_only.rs",
        "function",
        "old_only",
        "old::old_only",
    )?;
    insert_fact(
        &connection,
        2,
        0,
        b"src/a.rs",
        "function",
        "stable",
        "current::stable",
    )?;
    insert_fact(
        &connection,
        2,
        1,
        b"src/b.rs",
        "function",
        "stable",
        "current::stable_helper",
    )?;
    insert_fact(
        &connection,
        2,
        2,
        b"src/new.rs",
        "struct",
        "new_only",
        "current::new_only",
    )?;
    Ok(connection)
}

#[test]
fn search_is_generation_scoped_bounded_and_deterministic() {
    let connection = fixture().expect("fixture should be created");
    let first = search_active_generation(&connection, "stable", 10).expect("search should succeed");
    let second = search_active_generation(&connection, "stable", 10).expect("search should repeat");

    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].path, b"src/a.rs");
    assert_eq!(first[1].path, b"src/b.rs");
    assert!(
        first
            .iter()
            .all(|hit| !hit.qualified_name.starts_with("old::"))
    );
    assert_eq!(
        search_active_generation(&connection, "stable", 1)
            .expect("bounded search should succeed")
            .len(),
        1
    );
}

#[test]
fn hostile_match_syntax_is_literal_and_never_interpolated_into_sql() {
    let connection = fixture().expect("fixture should be created");
    let hostile = "stable\" OR old_only*";
    let result =
        search_active_generation(&connection, hostile, 10).expect("literal query should be valid");

    assert!(result.is_empty());
    assert_eq!(
        search_active_generation(&connection, "new_only", 10)
            .expect("connection should remain usable")
            .len(),
        1
    );
}

#[test]
fn projection_rebuild_is_logically_equivalent() {
    let connection = fixture().expect("fixture should be created");
    let before =
        search_active_generation(&connection, "stable", 10).expect("search should succeed");
    rebuild_generation_projection(&connection, 2).expect("projection should rebuild");
    let after = search_active_generation(&connection, "stable", 10).expect("search should succeed");

    assert_eq!(after, before);
}

#[test]
fn query_and_result_limits_fail_closed_with_redacted_errors() {
    let connection = fixture().expect("fixture should be created");
    let cases = [
        (
            search_active_generation(&connection, " \t\n", 10),
            SearchError::EmptyQuery,
        ),
        (
            search_active_generation(&connection, &"x".repeat(MAX_QUERY_BYTES + 1), 10),
            SearchError::QueryTooLarge,
        ),
        (
            search_active_generation(&connection, "a b c d e f g h i", 10),
            SearchError::TooManyTerms,
        ),
        (
            search_active_generation(&connection, &"x".repeat(MAX_QUERY_TERM_BYTES + 1), 10),
            SearchError::TermTooLarge,
        ),
        (
            search_active_generation(&connection, "secret-query", 0),
            SearchError::InvalidResultLimit,
        ),
    ];

    for (result, expected) in cases {
        let error = result.expect_err("invalid search should fail");
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("secret-query"));
        assert!(!format!("{error:?}").contains("secret-query"));
    }
}
