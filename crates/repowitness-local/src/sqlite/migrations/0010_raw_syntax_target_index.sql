-- Exact immutable raw-target lookup for bounded all-language syntax-site search.
--
-- The index adds no semantic association and does not modify persisted site
-- observations. It supports an equality predicate only; callers still receive
-- the complete generation-pinned coverage and no-resolution limitation.

CREATE INDEX syntax_sites_by_raw_target ON syntax_sites(raw_target);
