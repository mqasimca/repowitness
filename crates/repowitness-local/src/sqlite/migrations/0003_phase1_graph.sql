-- RepoWitness Phase 1 immutable Rust syntax graph and generation projection.

CREATE TABLE rust_graph_artifacts (
    artifact_digest BLOB PRIMARY KEY
        REFERENCES analysis_artifacts(artifact_digest),
    site_profile_version INTEGER NOT NULL
        CHECK (site_profile_version BETWEEN 1 AND 4294967295),
    site_count INTEGER NOT NULL CHECK (site_count >= 0),
    max_observed_depth INTEGER NOT NULL
        CHECK (max_observed_depth BETWEEN 0 AND 65535),
    owned_text_bytes INTEGER NOT NULL CHECK (owned_text_bytes >= 0),
    UNIQUE (artifact_digest, site_profile_version)
) STRICT, WITHOUT ROWID;

CREATE TABLE rust_graph_sites (
    artifact_digest BLOB NOT NULL
        REFERENCES rust_graph_artifacts(artifact_digest),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 4294967295),
    site_kind TEXT NOT NULL CHECK (
        site_kind IN ('import', 'reference', 'call', 'macro_call', 'test_marker')
    ),
    extraction_evidence TEXT NOT NULL CHECK (
        extraction_evidence IN ('direct_syntax', 'syntax_heuristic')
    ),
    occurrence_start INTEGER NOT NULL CHECK (occurrence_start >= 0),
    occurrence_end INTEGER NOT NULL CHECK (occurrence_end >= occurrence_start),
    target_start INTEGER NOT NULL CHECK (target_start >= occurrence_start),
    target_end INTEGER NOT NULL CHECK (
        target_end >= target_start AND target_end <= occurrence_end
    ),
    raw_target TEXT NOT NULL CHECK (
        length(CAST(raw_target AS BLOB)) BETWEEN 1 AND 16384
    ),
    enclosing_kind TEXT CHECK (
        enclosing_kind IS NULL OR enclosing_kind IN (
            'function', 'method', 'struct', 'enum', 'union', 'trait',
            'module', 'type_alias', 'constant', 'static', 'macro'
        )
    ),
    enclosing_name TEXT CHECK (
        enclosing_name IS NULL OR
        length(CAST(enclosing_name AS BLOB)) BETWEEN 1 AND 1024
    ),
    enclosing_qualified_name TEXT CHECK (
        enclosing_qualified_name IS NULL OR
        length(CAST(enclosing_qualified_name AS BLOB)) BETWEEN 1 AND 16384
    ),
    enclosing_name_start INTEGER CHECK (
        enclosing_name_start IS NULL OR enclosing_name_start >= 0
    ),
    enclosing_name_end INTEGER CHECK (
        enclosing_name_end IS NULL OR
        enclosing_name_end >= enclosing_name_start
    ),
    enclosing_declaration_start INTEGER CHECK (
        enclosing_declaration_start IS NULL OR
        enclosing_declaration_start >= 0
    ),
    enclosing_declaration_end INTEGER CHECK (
        enclosing_declaration_end IS NULL OR
        enclosing_declaration_end >= enclosing_declaration_start
    ),
    PRIMARY KEY (artifact_digest, ordinal),
    UNIQUE (
        artifact_digest, ordinal, site_kind,
        occurrence_start, occurrence_end, target_start, target_end
    ),
    CHECK (
        (enclosing_kind IS NULL
         AND enclosing_name IS NULL
         AND enclosing_qualified_name IS NULL
         AND enclosing_name_start IS NULL
         AND enclosing_name_end IS NULL
         AND enclosing_declaration_start IS NULL
         AND enclosing_declaration_end IS NULL)
        OR
        (enclosing_kind IS NOT NULL
         AND enclosing_name IS NOT NULL
         AND enclosing_qualified_name IS NOT NULL
         AND enclosing_name_start IS NOT NULL
         AND enclosing_name_end IS NOT NULL
         AND enclosing_declaration_start IS NOT NULL
         AND enclosing_declaration_end IS NOT NULL
         AND enclosing_name_start >= enclosing_declaration_start
         AND enclosing_name_end <= enclosing_declaration_end)
    )
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_graph_requirements (
    generation_id INTEGER PRIMARY KEY
        REFERENCES index_generations(generation_id),
    resolver_profile_version INTEGER NOT NULL
        CHECK (resolver_profile_version BETWEEN 1 AND 4294967295)
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_graph_publications (
    generation_id INTEGER PRIMARY KEY
        REFERENCES generation_graph_requirements(generation_id),
    connected_workspace_id BLOB NOT NULL
        REFERENCES connected_workspaces(connected_workspace_id)
        CHECK (length(connected_workspace_id) = 32),
    lifecycle_state TEXT NOT NULL
        CHECK (lifecycle_state IN ('staging', 'complete')),
    resolver_profile_version INTEGER NOT NULL
        CHECK (resolver_profile_version BETWEEN 1 AND 4294967295),
    input_digest BLOB NOT NULL CHECK (length(input_digest) = 32),
    output_digest BLOB NOT NULL CHECK (length(output_digest) = 32),
    source_count INTEGER NOT NULL CHECK (source_count BETWEEN 1 AND 256),
    artifact_count INTEGER NOT NULL CHECK (artifact_count >= 0),
    definition_count INTEGER NOT NULL CHECK (definition_count >= 0),
    site_count INTEGER NOT NULL CHECK (site_count >= 0),
    unresolved_count INTEGER NOT NULL CHECK (unresolved_count >= 0),
    unique_count INTEGER NOT NULL CHECK (unique_count >= 0),
    ambiguous_count INTEGER NOT NULL CHECK (ambiguous_count >= 0),
    unsupported_count INTEGER NOT NULL CHECK (unsupported_count >= 0),
    truncated_site_count INTEGER NOT NULL CHECK (truncated_site_count >= 0),
    retained_candidate_count INTEGER NOT NULL
        CHECK (retained_candidate_count >= 0),
    edge_count INTEGER NOT NULL CHECK (edge_count >= 0),
    input_text_bytes INTEGER NOT NULL CHECK (input_text_bytes >= 0),
    output_bytes INTEGER NOT NULL CHECK (output_bytes >= 0),
    syntax_error_node_count INTEGER NOT NULL
        CHECK (syntax_error_node_count >= 0),
    macro_site_count INTEGER NOT NULL CHECK (macro_site_count >= 0),
    test_marker_site_count INTEGER NOT NULL CHECK (test_marker_site_count >= 0),
    heuristic_site_count INTEGER NOT NULL CHECK (heuristic_site_count >= 0)
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_graph_sources (
    generation_id INTEGER NOT NULL
        REFERENCES generation_graph_publications(generation_id),
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 255),
    source_slot_id BLOB NOT NULL CHECK (length(source_slot_id) = 32),
    source_generation_id INTEGER NOT NULL
        REFERENCES index_generations(generation_id),
    PRIMARY KEY (generation_id, ordinal),
    UNIQUE (generation_id, source_slot_id),
    UNIQUE (generation_id, source_slot_id, source_generation_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_graph_artifacts (
    generation_id INTEGER NOT NULL,
    source_slot_id BLOB NOT NULL CHECK (length(source_slot_id) = 32),
    source_generation_id INTEGER NOT NULL,
    repository_path BLOB NOT NULL
        CHECK (length(repository_path) BETWEEN 1 AND 1048576),
    graph_artifact_digest BLOB NOT NULL
        REFERENCES rust_graph_artifacts(artifact_digest),
    PRIMARY KEY (generation_id, source_slot_id, repository_path),
    FOREIGN KEY (generation_id, source_slot_id, source_generation_id)
        REFERENCES generation_graph_sources(
            generation_id, source_slot_id, source_generation_id
        )
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_graph_definitions (
    generation_id INTEGER NOT NULL,
    source_slot_id BLOB NOT NULL CHECK (length(source_slot_id) = 32),
    source_generation_id INTEGER NOT NULL,
    repository_path BLOB NOT NULL
        CHECK (length(repository_path) BETWEEN 1 AND 1048576),
    artifact_digest BLOB NOT NULL CHECK (length(artifact_digest) = 32),
    fact_ordinal INTEGER NOT NULL CHECK (fact_ordinal >= 0),
    symbol_kind TEXT NOT NULL CHECK (
        symbol_kind IN (
            'function', 'method', 'struct', 'enum', 'union', 'trait',
            'module', 'type_alias', 'constant', 'static', 'macro'
        )
    ),
    name_start INTEGER NOT NULL CHECK (name_start >= 0),
    name_end INTEGER NOT NULL CHECK (name_end >= name_start),
    declaration_start INTEGER NOT NULL CHECK (declaration_start >= 0),
    declaration_end INTEGER NOT NULL
        CHECK (declaration_end >= declaration_start),
    PRIMARY KEY (
        generation_id, source_slot_id, repository_path,
        artifact_digest, fact_ordinal
    ),
    UNIQUE (
        generation_id, source_slot_id, repository_path, artifact_digest,
        fact_ordinal, symbol_kind, name_start, name_end,
        declaration_start, declaration_end
    ),
    FOREIGN KEY (generation_id, source_slot_id, source_generation_id)
        REFERENCES generation_graph_sources(
            generation_id, source_slot_id, source_generation_id
        ),
    FOREIGN KEY (artifact_digest, fact_ordinal)
        REFERENCES artifact_facts(artifact_digest, ordinal)
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_graph_resolutions (
    generation_id INTEGER NOT NULL,
    source_slot_id BLOB NOT NULL CHECK (length(source_slot_id) = 32),
    source_generation_id INTEGER NOT NULL,
    repository_path BLOB NOT NULL
        CHECK (length(repository_path) BETWEEN 1 AND 1048576),
    site_artifact_digest BLOB NOT NULL CHECK (length(site_artifact_digest) = 32),
    site_ordinal INTEGER NOT NULL CHECK (
        site_ordinal BETWEEN 0 AND 4294967295
    ),
    site_kind TEXT NOT NULL CHECK (
        site_kind IN ('import', 'reference', 'call', 'macro_call', 'test_marker')
    ),
    occurrence_start INTEGER NOT NULL CHECK (occurrence_start >= 0),
    occurrence_end INTEGER NOT NULL CHECK (occurrence_end >= occurrence_start),
    target_start INTEGER NOT NULL CHECK (target_start >= occurrence_start),
    target_end INTEGER NOT NULL CHECK (
        target_end >= target_start AND target_end <= occurrence_end
    ),
    outcome_kind TEXT NOT NULL CHECK (
        outcome_kind IN ('unresolved', 'unique', 'ambiguous')
    ),
    unresolved_reason TEXT CHECK (
        unresolved_reason IS NULL OR unresolved_reason IN (
            'no_candidate', 'unsupported_site_kind',
            'unsupported_import_shape', 'dynamic_or_method_call',
            'unsupported_qualified_syntax'
        )
    ),
    candidate_count INTEGER NOT NULL
        CHECK (candidate_count BETWEEN 0 AND 4294967295),
    candidates_truncated INTEGER NOT NULL
        CHECK (candidates_truncated IN (0, 1)),
    PRIMARY KEY (
        generation_id, source_slot_id, repository_path,
        site_artifact_digest, site_ordinal
    ),
    UNIQUE (
        generation_id, source_slot_id, repository_path,
        site_artifact_digest, site_ordinal, site_kind,
        occurrence_start, occurrence_end, target_start, target_end
    ),
    FOREIGN KEY (generation_id, source_slot_id, source_generation_id)
        REFERENCES generation_graph_sources(
            generation_id, source_slot_id, source_generation_id
        ),
    FOREIGN KEY (
        site_artifact_digest, site_ordinal, site_kind,
        occurrence_start, occurrence_end, target_start, target_end
    ) REFERENCES rust_graph_sites(
        artifact_digest, ordinal, site_kind,
        occurrence_start, occurrence_end, target_start, target_end
    ),
    CHECK (
        (outcome_kind = 'unresolved'
         AND unresolved_reason IS NOT NULL
         AND candidate_count = 0
         AND candidates_truncated = 0)
        OR
        (outcome_kind = 'unique'
         AND unresolved_reason IS NULL
         AND candidate_count = 1
         AND candidates_truncated = 0)
        OR
        (outcome_kind = 'ambiguous'
         AND unresolved_reason IS NULL
         AND candidate_count >= 2)
    )
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_graph_candidates (
    generation_id INTEGER NOT NULL,
    site_source_slot_id BLOB NOT NULL
        CHECK (length(site_source_slot_id) = 32),
    site_repository_path BLOB NOT NULL
        CHECK (length(site_repository_path) BETWEEN 1 AND 1048576),
    site_artifact_digest BLOB NOT NULL CHECK (length(site_artifact_digest) = 32),
    site_ordinal INTEGER NOT NULL CHECK (
        site_ordinal BETWEEN 0 AND 4294967295
    ),
    candidate_ordinal INTEGER NOT NULL
        CHECK (candidate_ordinal BETWEEN 0 AND 4294967295),
    target_source_slot_id BLOB NOT NULL
        CHECK (length(target_source_slot_id) = 32),
    target_repository_path BLOB NOT NULL
        CHECK (length(target_repository_path) BETWEEN 1 AND 1048576),
    target_artifact_digest BLOB NOT NULL
        CHECK (length(target_artifact_digest) = 32),
    target_fact_ordinal INTEGER NOT NULL CHECK (target_fact_ordinal >= 0),
    target_kind TEXT NOT NULL CHECK (
        target_kind IN (
            'function', 'method', 'struct', 'enum', 'union', 'trait',
            'module', 'type_alias', 'constant', 'static', 'macro'
        )
    ),
    target_name_start INTEGER NOT NULL CHECK (target_name_start >= 0),
    target_name_end INTEGER NOT NULL
        CHECK (target_name_end >= target_name_start),
    target_declaration_start INTEGER NOT NULL
        CHECK (target_declaration_start >= 0),
    target_declaration_end INTEGER NOT NULL
        CHECK (target_declaration_end >= target_declaration_start),
    resolution_evidence TEXT NOT NULL CHECK (
        resolution_evidence IN (
            'qualified_syntax', 'lexical_syntax',
            'import_syntax', 'exact_name_heuristic'
        )
    ),
    PRIMARY KEY (
        generation_id, site_source_slot_id, site_repository_path,
        site_artifact_digest, site_ordinal, candidate_ordinal
    ),
    FOREIGN KEY (
        generation_id, site_source_slot_id, site_repository_path,
        site_artifact_digest, site_ordinal
    ) REFERENCES generation_graph_resolutions(
        generation_id, source_slot_id, repository_path,
        site_artifact_digest, site_ordinal
    ),
    FOREIGN KEY (
        generation_id, target_source_slot_id, target_repository_path,
        target_artifact_digest, target_fact_ordinal, target_kind,
        target_name_start, target_name_end,
        target_declaration_start, target_declaration_end
    ) REFERENCES generation_graph_definitions(
        generation_id, source_slot_id, repository_path,
        artifact_digest, fact_ordinal, symbol_kind,
        name_start, name_end, declaration_start, declaration_end
    )
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_graph_edges (
    generation_id INTEGER NOT NULL,
    site_source_slot_id BLOB NOT NULL
        CHECK (length(site_source_slot_id) = 32),
    site_repository_path BLOB NOT NULL
        CHECK (length(site_repository_path) BETWEEN 1 AND 1048576),
    site_artifact_digest BLOB NOT NULL CHECK (length(site_artifact_digest) = 32),
    site_ordinal INTEGER NOT NULL CHECK (
        site_ordinal BETWEEN 0 AND 4294967295
    ),
    candidate_ordinal INTEGER NOT NULL CHECK (candidate_ordinal = 0),
    edge_kind TEXT NOT NULL CHECK (
        edge_kind IN ('import', 'reference', 'call')
    ),
    resolution_evidence TEXT NOT NULL CHECK (
        resolution_evidence IN (
            'qualified_syntax', 'lexical_syntax',
            'import_syntax', 'exact_name_heuristic'
        )
    ),
    PRIMARY KEY (
        generation_id, site_source_slot_id, site_repository_path,
        site_artifact_digest, site_ordinal
    ),
    FOREIGN KEY (
        generation_id, site_source_slot_id, site_repository_path,
        site_artifact_digest, site_ordinal, candidate_ordinal
    ) REFERENCES generation_graph_candidates(
        generation_id, site_source_slot_id, site_repository_path,
        site_artifact_digest, site_ordinal, candidate_ordinal
    )
) STRICT, WITHOUT ROWID;

CREATE INDEX generation_graph_edges_by_kind
ON generation_graph_edges(generation_id, edge_kind);

CREATE INDEX generation_graph_candidates_by_target
ON generation_graph_candidates(
    generation_id, target_source_slot_id, target_repository_path,
    target_artifact_digest, target_fact_ordinal
);

CREATE TRIGGER rust_graph_artifacts_insert_staging_only
BEFORE INSERT ON rust_graph_artifacts
WHEN
    (SELECT lifecycle_state FROM analysis_artifacts
     WHERE artifact_digest = NEW.artifact_digest) != 'staging'
    OR
    (SELECT language FROM analysis_artifacts
     WHERE artifact_digest = NEW.artifact_digest) != 'rust'
    OR
    (SELECT fact_count FROM analysis_artifacts
     WHERE artifact_digest = NEW.artifact_digest) != 0
BEGIN
    SELECT RAISE(ABORT, 'graph artifact is not accepting metadata');
END;

CREATE TRIGGER rust_graph_artifacts_no_update
BEFORE UPDATE ON rust_graph_artifacts BEGIN
    SELECT RAISE(ABORT, 'immutable graph artifact metadata');
END;

CREATE TRIGGER rust_graph_artifacts_complete_no_delete
BEFORE DELETE ON rust_graph_artifacts
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = OLD.artifact_digest) = 'complete'
BEGIN
    SELECT RAISE(ABORT, 'immutable complete graph artifact metadata');
END;

CREATE TRIGGER rust_graph_sites_insert_staging_only
BEFORE INSERT ON rust_graph_sites
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = NEW.artifact_digest) != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'graph artifact is not accepting sites');
END;

CREATE TRIGGER rust_graph_sites_no_update
BEFORE UPDATE ON rust_graph_sites BEGIN
    SELECT RAISE(ABORT, 'immutable graph artifact sites');
END;

CREATE TRIGGER rust_graph_sites_complete_no_delete
BEFORE DELETE ON rust_graph_sites
WHEN (SELECT lifecycle_state FROM analysis_artifacts
      WHERE artifact_digest = OLD.artifact_digest) = 'complete'
BEGIN
    SELECT RAISE(ABORT, 'immutable complete graph artifact sites');
END;

CREATE TRIGGER rust_graph_artifact_completion_requires_sites
BEFORE UPDATE OF lifecycle_state ON analysis_artifacts
WHEN
    OLD.lifecycle_state = 'staging'
    AND NEW.lifecycle_state = 'complete'
    AND EXISTS (
        SELECT 1 FROM rust_graph_artifacts
        WHERE artifact_digest = NEW.artifact_digest
    )
    AND (
        NEW.fact_count != 0
        OR NEW.payload_digest IS NULL
        OR (SELECT site_count FROM rust_graph_artifacts
            WHERE artifact_digest = NEW.artifact_digest) !=
           (SELECT count(*) FROM rust_graph_sites
            WHERE artifact_digest = NEW.artifact_digest)
        OR EXISTS (
            SELECT 1
            FROM rust_graph_sites AS site
            WHERE site.artifact_digest = NEW.artifact_digest
              AND site.ordinal != (
                  SELECT count(*)
                  FROM rust_graph_sites AS prior
                  WHERE prior.artifact_digest = site.artifact_digest
                    AND prior.ordinal < site.ordinal
              )
        )
    )
BEGIN
    SELECT RAISE(ABORT, 'incomplete graph artifact');
END;

CREATE TRIGGER generation_graph_requirements_insert_eligible
BEFORE INSERT ON generation_graph_requirements
WHEN (SELECT lifecycle_state FROM index_generations
      WHERE generation_id = NEW.generation_id) NOT IN (
          'resolving', 'validating', 'ready'
      )
BEGIN
    SELECT RAISE(ABORT, 'generation is not accepting graph requirement');
END;

CREATE TRIGGER generation_graph_requirements_no_update
BEFORE UPDATE ON generation_graph_requirements BEGIN
    SELECT RAISE(ABORT, 'immutable graph requirement');
END;

CREATE TRIGGER generation_graph_publications_insert_staging_only
BEFORE INSERT ON generation_graph_publications
WHEN
    NEW.lifecycle_state != 'staging'
    OR
    NEW.resolver_profile_version != (
        SELECT resolver_profile_version
        FROM generation_graph_requirements
        WHERE generation_id = NEW.generation_id
    )
    OR
    (SELECT lifecycle_state FROM index_generations
     WHERE generation_id = NEW.generation_id) != 'ready'
BEGIN
    SELECT RAISE(ABORT, 'generation is not accepting graph publication');
END;

CREATE TRIGGER generation_graph_publications_no_semantic_update
BEFORE UPDATE OF
    generation_id, connected_workspace_id, resolver_profile_version,
    input_digest, output_digest, source_count, artifact_count,
    definition_count, site_count, unresolved_count, unique_count,
    ambiguous_count, unsupported_count, truncated_site_count,
    retained_candidate_count, edge_count, input_text_bytes, output_bytes,
    syntax_error_node_count, macro_site_count, test_marker_site_count,
    heuristic_site_count
ON generation_graph_publications BEGIN
    SELECT RAISE(ABORT, 'immutable generation graph publication');
END;

CREATE TRIGGER generation_graph_publication_lifecycle_transition
BEFORE UPDATE OF lifecycle_state ON generation_graph_publications
WHEN NOT (
    OLD.lifecycle_state = 'staging' AND NEW.lifecycle_state = 'complete'
)
BEGIN
    SELECT RAISE(ABORT, 'invalid generation graph publication transition');
END;

CREATE TRIGGER generation_graph_sources_insert_staging_only
BEFORE INSERT ON generation_graph_sources
WHEN
    (SELECT lifecycle_state FROM generation_graph_publications
     WHERE generation_id = NEW.generation_id) != 'staging'
    OR NOT EXISTS (
        SELECT 1
        FROM generation_graph_publications AS publication
        JOIN workspace_source_slots AS slot
          ON slot.connected_workspace_id = publication.connected_workspace_id
         AND slot.source_slot_id = NEW.source_slot_id
        JOIN index_generations AS source_generation
          ON source_generation.workspace_id = slot.generation_workspace_id
         AND source_generation.generation_id = NEW.source_generation_id
        WHERE publication.generation_id = NEW.generation_id
          AND source_generation.lifecycle_state IN (
              'ready', 'active', 'retained'
          )
    )
BEGIN
    SELECT RAISE(ABORT, 'invalid generation graph source');
END;

CREATE TRIGGER generation_graph_artifacts_insert_staging_only
BEFORE INSERT ON generation_graph_artifacts
WHEN
    (SELECT lifecycle_state FROM generation_graph_publications
     WHERE generation_id = NEW.generation_id) != 'staging'
    OR NOT EXISTS (
        SELECT 1
        FROM generation_files AS file
        JOIN analysis_artifacts AS graph_artifact
          ON graph_artifact.artifact_digest = NEW.graph_artifact_digest
         AND graph_artifact.lifecycle_state = 'complete'
         AND graph_artifact.source_content_digest = file.content_digest
        WHERE file.generation_id = NEW.source_generation_id
          AND file.repository_path = NEW.repository_path
    )
BEGIN
    SELECT RAISE(ABORT, 'invalid generation graph artifact');
END;

CREATE TRIGGER generation_graph_definitions_insert_staging_only
BEFORE INSERT ON generation_graph_definitions
WHEN
    (SELECT lifecycle_state FROM generation_graph_publications
     WHERE generation_id = NEW.generation_id) != 'staging'
    OR NOT EXISTS (
        SELECT 1
        FROM generation_files AS file
        JOIN artifact_facts AS fact
          ON fact.artifact_digest = file.artifact_digest
         AND fact.ordinal = NEW.fact_ordinal
         AND fact.kind = NEW.symbol_kind
         AND fact.name_start = NEW.name_start
         AND fact.name_end = NEW.name_end
         AND fact.declaration_start = NEW.declaration_start
         AND fact.declaration_end = NEW.declaration_end
        WHERE file.generation_id = NEW.source_generation_id
          AND file.repository_path = NEW.repository_path
          AND file.artifact_digest = NEW.artifact_digest
    )
BEGIN
    SELECT RAISE(ABORT, 'invalid generation graph definition');
END;

CREATE TRIGGER generation_graph_resolutions_insert_staging_only
BEFORE INSERT ON generation_graph_resolutions
WHEN
    (SELECT lifecycle_state FROM generation_graph_publications
     WHERE generation_id = NEW.generation_id) != 'staging'
    OR NOT EXISTS (
        SELECT 1 FROM generation_graph_artifacts AS occurrence
        WHERE occurrence.generation_id = NEW.generation_id
          AND occurrence.source_slot_id = NEW.source_slot_id
          AND occurrence.source_generation_id = NEW.source_generation_id
          AND occurrence.repository_path = NEW.repository_path
          AND occurrence.graph_artifact_digest = NEW.site_artifact_digest
    )
BEGIN
    SELECT RAISE(ABORT, 'invalid generation graph resolution');
END;

CREATE TRIGGER generation_graph_candidates_insert_staging_only
BEFORE INSERT ON generation_graph_candidates
WHEN (SELECT lifecycle_state FROM generation_graph_publications
      WHERE generation_id = NEW.generation_id) != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'generation graph is not accepting candidates');
END;

CREATE TRIGGER generation_graph_edges_insert_staging_only
BEFORE INSERT ON generation_graph_edges
WHEN
    (SELECT lifecycle_state FROM generation_graph_publications
     WHERE generation_id = NEW.generation_id) != 'staging'
    OR NOT EXISTS (
        SELECT 1
        FROM generation_graph_resolutions AS resolution
        JOIN generation_graph_candidates AS candidate
          ON candidate.generation_id = resolution.generation_id
         AND candidate.site_source_slot_id = resolution.source_slot_id
         AND candidate.site_repository_path = resolution.repository_path
         AND candidate.site_artifact_digest = resolution.site_artifact_digest
         AND candidate.site_ordinal = resolution.site_ordinal
         AND candidate.candidate_ordinal = NEW.candidate_ordinal
        WHERE resolution.generation_id = NEW.generation_id
          AND resolution.source_slot_id = NEW.site_source_slot_id
          AND resolution.repository_path = NEW.site_repository_path
          AND resolution.site_artifact_digest = NEW.site_artifact_digest
          AND resolution.site_ordinal = NEW.site_ordinal
          AND resolution.outcome_kind = 'unique'
          AND resolution.site_kind = NEW.edge_kind
          AND candidate.resolution_evidence = NEW.resolution_evidence
    )
BEGIN
    SELECT RAISE(ABORT, 'invalid generation graph edge');
END;

CREATE TRIGGER generation_graph_sources_no_update
BEFORE UPDATE ON generation_graph_sources BEGIN
    SELECT RAISE(ABORT, 'immutable generation graph source');
END;

CREATE TRIGGER generation_graph_artifacts_no_update
BEFORE UPDATE ON generation_graph_artifacts BEGIN
    SELECT RAISE(ABORT, 'immutable generation graph artifact occurrence');
END;

CREATE TRIGGER generation_graph_definitions_no_update
BEFORE UPDATE ON generation_graph_definitions BEGIN
    SELECT RAISE(ABORT, 'immutable generation graph definition');
END;

CREATE TRIGGER generation_graph_resolutions_no_update
BEFORE UPDATE ON generation_graph_resolutions BEGIN
    SELECT RAISE(ABORT, 'immutable generation graph resolution');
END;

CREATE TRIGGER generation_graph_candidates_no_update
BEFORE UPDATE ON generation_graph_candidates BEGIN
    SELECT RAISE(ABORT, 'immutable generation graph candidate');
END;

CREATE TRIGGER generation_graph_edges_no_update
BEFORE UPDATE ON generation_graph_edges BEGIN
    SELECT RAISE(ABORT, 'immutable generation graph edge');
END;
