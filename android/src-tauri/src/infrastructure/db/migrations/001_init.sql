CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

CREATE TABLE profiles (
    id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE typing_calibrations (
    id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    units_per_minute INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE rule_templates (
    id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    source_text TEXT NOT NULL,
    ruleset_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE matches (
    id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    mode TEXT NOT NULL,
    phase TEXT NOT NULL,
    topic TEXT NOT NULL,
    speech_rate_per_minute INTEGER NOT NULL,
    rules_hash TEXT,
    lineup_hash TEXT,
    non_standard INTEGER NOT NULL,
    charged_ms INTEGER NOT NULL,
    ai_wait_ms INTEGER NOT NULL,
    snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_matches_profile_updated ON matches (profile_id, updated_at DESC, id);

CREATE TABLE match_rule_snapshots (
    match_id TEXT PRIMARY KEY REFERENCES matches(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    ruleset_json TEXT NOT NULL
);

CREATE TABLE match_materials (
    match_id TEXT PRIMARY KEY REFERENCES matches(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    materials TEXT NOT NULL
);

CREATE TABLE debater_assignments (
    match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    slot TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    controller TEXT NOT NULL,
    PRIMARY KEY (match_id, slot)
);

CREATE TABLE match_events (
    id TEXT PRIMARY KEY,
    match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    kind TEXT NOT NULL,
    slot TEXT,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (match_id, sequence)
);

CREATE TABLE turns (
    id TEXT PRIMARY KEY,
    match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    event_id TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    slot TEXT NOT NULL,
    input_ms INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE transcript_entries (
    id TEXT PRIMARY KEY,
    match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    turn_id TEXT NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    event_id TEXT NOT NULL,
    slot TEXT NOT NULL,
    full_text TEXT NOT NULL,
    effective_text TEXT NOT NULL,
    effective_units INTEGER NOT NULL,
    overflow_text TEXT NOT NULL
);

CREATE TABLE turn_metric_summaries (
    turn_id TEXT PRIMARY KEY REFERENCES turns(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    forward_typed_units INTEGER NOT NULL,
    effective_positive_units INTEGER NOT NULL,
    reverse_edited_units INTEGER NOT NULL,
    paste_units INTEGER NOT NULL,
    pause_ms INTEGER NOT NULL
);

CREATE TABLE ai_request_metadata (
    id TEXT PRIMARY KEY,
    match_id TEXT REFERENCES matches(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    purpose TEXT NOT NULL,
    model_id TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE stage_evaluations (
    id TEXT PRIMARY KEY,
    match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    stage_id TEXT NOT NULL,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE match_reviews (
    id TEXT PRIMARY KEY,
    match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE reports (
    id TEXT PRIMARY KEY,
    match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    formula_version TEXT NOT NULL,
    standard INTEGER NOT NULL,
    body_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
