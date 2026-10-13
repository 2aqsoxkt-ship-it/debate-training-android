CREATE TABLE draft_records (
    profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    window_id TEXT NOT NULL,
    slot TEXT NOT NULL,
    field_kind TEXT NOT NULL CHECK (field_kind IN ('speech', 'question', 'answer')),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    payload_json TEXT NOT NULL,
    closed INTEGER NOT NULL DEFAULT 0 CHECK (closed IN (0, 1)),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (profile_id, match_id, window_id, slot, field_kind)
);

CREATE TABLE practice_records (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    match_id TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    source_turn_id TEXT NOT NULL,
    frozen_context_json TEXT NOT NULL,
    rewritten_text TEXT NOT NULL DEFAULT '',
    feedback_json TEXT,
    revision INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
