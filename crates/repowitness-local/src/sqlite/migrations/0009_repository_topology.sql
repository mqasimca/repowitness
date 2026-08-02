-- Immutable path-only repository topology receipts, paired with one source generation.

CREATE TABLE generation_repository_topology_requirements (
    generation_id INTEGER PRIMARY KEY
        REFERENCES index_generations(generation_id) ON DELETE CASCADE,
    topology_profile_version INTEGER NOT NULL
        CHECK (topology_profile_version BETWEEN 1 AND 4294967295)
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_repository_topology_publications (
    generation_id INTEGER PRIMARY KEY
        REFERENCES generation_repository_topology_requirements(generation_id) ON DELETE CASCADE,
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state IN ('staging', 'complete')),
    topology_profile_version INTEGER NOT NULL
        CHECK (topology_profile_version BETWEEN 1 AND 4294967295),
    topology_digest BLOB NOT NULL CHECK (length(topology_digest) = 32),
    discovered_path_count INTEGER NOT NULL CHECK (discovered_path_count >= 0),
    omitted_path_count INTEGER NOT NULL CHECK (omitted_path_count = 0),
    total_path_count INTEGER NOT NULL CHECK (total_path_count >= 0),
    CHECK (discovered_path_count = total_path_count + omitted_path_count)
) STRICT, WITHOUT ROWID;

CREATE TABLE generation_repository_topology_entries (
    generation_id INTEGER NOT NULL
        REFERENCES generation_repository_topology_publications(generation_id) ON DELETE CASCADE,
    repository_path BLOB NOT NULL CHECK (length(repository_path) BETWEEN 1 AND 1048576),
    category TEXT NOT NULL CHECK (category IN (
        'documentation', 'agent_instruction', 'workflow_descriptor', 'build_descriptor',
        'package_descriptor', 'configuration_descriptor', 'other_tracked_file'
    )),
    PRIMARY KEY (generation_id, repository_path)
) STRICT, WITHOUT ROWID;

CREATE INDEX generation_repository_topology_entries_by_category
ON generation_repository_topology_entries(generation_id, category);

CREATE TRIGGER generation_repository_topology_requirements_insert_eligible
BEFORE INSERT ON generation_repository_topology_requirements
WHEN (SELECT lifecycle_state FROM index_generations WHERE generation_id = NEW.generation_id)
     NOT IN ('resolving', 'validating', 'ready')
BEGIN
    SELECT RAISE(ABORT, 'generation is not accepting repository topology requirement');
END;

CREATE TRIGGER generation_repository_topology_requirements_no_update
BEFORE UPDATE ON generation_repository_topology_requirements BEGIN
    SELECT RAISE(ABORT, 'immutable repository topology requirement');
END;

CREATE TRIGGER generation_repository_topology_publications_insert_staging_only
BEFORE INSERT ON generation_repository_topology_publications
WHEN NEW.lifecycle_state != 'staging'
OR NEW.topology_profile_version != (
    SELECT topology_profile_version FROM generation_repository_topology_requirements
    WHERE generation_id = NEW.generation_id
)
OR (SELECT lifecycle_state FROM index_generations WHERE generation_id = NEW.generation_id) != 'ready'
BEGIN
    SELECT RAISE(ABORT, 'generation is not accepting repository topology publication');
END;

CREATE TRIGGER generation_repository_topology_publications_no_semantic_update
BEFORE UPDATE OF generation_id, topology_profile_version, topology_digest,
    discovered_path_count, omitted_path_count, total_path_count
ON generation_repository_topology_publications BEGIN
    SELECT RAISE(ABORT, 'immutable repository topology publication');
END;

CREATE TRIGGER generation_repository_topology_publication_lifecycle_transition
BEFORE UPDATE OF lifecycle_state ON generation_repository_topology_publications
WHEN NOT (OLD.lifecycle_state = 'staging' AND NEW.lifecycle_state = 'complete')
BEGIN
    SELECT RAISE(ABORT, 'invalid repository topology publication lifecycle transition');
END;

CREATE TRIGGER generation_repository_topology_entries_insert_staging_only
BEFORE INSERT ON generation_repository_topology_entries
WHEN (SELECT lifecycle_state FROM generation_repository_topology_publications
      WHERE generation_id = NEW.generation_id) != 'staging'
BEGIN
    SELECT RAISE(ABORT, 'generation is not accepting repository topology entry');
END;

CREATE TRIGGER generation_repository_topology_entries_no_update
BEFORE UPDATE ON generation_repository_topology_entries BEGIN
    SELECT RAISE(ABORT, 'immutable repository topology entry');
END;

CREATE TRIGGER generation_repository_topology_completion_requires_complete_inventory
BEFORE UPDATE OF lifecycle_state ON generation_repository_topology_publications
WHEN NEW.lifecycle_state = 'complete'
AND (
    (SELECT lifecycle_state FROM index_generations WHERE generation_id = NEW.generation_id) != 'ready'
    OR NEW.total_path_count != (
        SELECT count(*) FROM generation_repository_topology_entries WHERE generation_id = NEW.generation_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'incomplete repository topology publication');
END;

CREATE TRIGGER generation_activation_requires_repository_topology_when_required
BEFORE UPDATE OF lifecycle_state ON index_generations
WHEN OLD.lifecycle_state = 'ready'
AND NEW.lifecycle_state = 'active'
AND EXISTS (SELECT 1 FROM generation_repository_topology_requirements WHERE generation_id = NEW.generation_id)
AND NOT EXISTS (
    SELECT 1 FROM generation_repository_topology_publications
    WHERE generation_id = NEW.generation_id AND lifecycle_state = 'complete'
)
BEGIN
    SELECT RAISE(ABORT, 'required repository topology publication is incomplete');
END;
