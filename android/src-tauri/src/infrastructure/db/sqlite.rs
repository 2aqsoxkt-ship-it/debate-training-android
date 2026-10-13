use std::path::Path;

use rusqlite::{params, Connection};
use uuid::Uuid;

use super::repository::{require_utc_timestamp, MatchRepository};
use crate::domain::events::{MatchEventV1, TranscriptEntryV1, TurnMetricSummaryV1};
use crate::domain::match_engine::{MatchAggregate, MatchPhase, MatchSnapshotV1};
use crate::error::{AppError, ErrorCode};

const PROFILE_SCHEMA: &str = "profile.v1";
const MATCH_SCHEMA: &str = "match-record.v1";
const SETTINGS_SCHEMA: &str = "app-settings.v1";
const MATERIAL_SCHEMA: &str = "match-materials.v1";
const RULE_SNAPSHOT_SCHEMA: &str = "match-rule-snapshot.v1";
const TURN_SCHEMA: &str = "turn.v1";
const THEMES: [&str; 3] = ["system", "light", "dark"];
const THINKING_LEVELS: [&str; 3] = ["low", "high", "max"];

pub mod experience;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRecord {
    pub id: Uuid,
    pub display_name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    pub model_id: String,
    pub base_url: String,
    pub speech_rate_per_minute: u32,
    pub theme: String,
    pub thinking_level: String,
    pub connection_status: String,
    pub last_tested_at: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            model_id: String::new(),
            base_url: String::new(),
            speech_rate_per_minute: crate::domain::timing::DEFAULT_SPEECH_RATE,
            theme: "system".to_owned(),
            thinking_level: "high".to_owned(),
            connection_status: "not_configured".to_owned(),
            last_tested_at: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchListItem {
    pub id: Uuid,
    pub topic: String,
    pub phase: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedTurn {
    pub event: MatchEventV1,
    pub turn_id: Uuid,
    pub input_ms: u64,
    pub transcript: TranscriptEntryV1,
    pub metrics: TurnMetricSummaryV1,
    pub phase: MatchPhase,
    pub charged_ms: u64,
    pub ai_wait_ms: u64,
    pub cursor: usize,
    pub saved_at: String,
}

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let path = path.as_ref();
        let conn = Connection::open(path).map_err(map_sql)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(map_sql)?;
        let store = Self { conn };
        if store.schema_version()? == 1 && path.is_file() {
            // A consistent SQLite snapshot, including committed WAL contents.
            // Never overwrite an earlier backup or rebuild the user's database.
            let serial = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| internal())?
                .as_nanos();
            let backup = path.with_extension(format!("v1-backup-{serial}.sqlite"));
            store
                .conn
                .execute("VACUUM INTO ?1", params![backup.to_string_lossy()])
                .map_err(map_sql)?;
        }
        store.migrate()?;
        Ok(store)
    }

    pub fn migrate(&self) -> Result<(), AppError> {
        let version = self.schema_version()?;
        if version >= 2 {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction().map_err(map_sql)?;
        if version == 0 {
            tx.execute_batch(include_str!("migrations/001_init.sql"))
                .map_err(map_sql)?;
            tx.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (1, '2026-09-23T00:00:00Z')",
            [],
        )
        .map_err(map_sql)?;
        }
        tx.execute_batch(include_str!("migrations/002_experience.sql"))
            .map_err(map_sql)?;
        tx.execute("INSERT INTO schema_migrations (version, applied_at) VALUES (2, '2026-10-12T00:00:00Z')", []).map_err(map_sql)?;
        tx.commit().map_err(map_sql)?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64, AppError> {
        let exists: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
                [],
                |row| row.get(0),
            )
            .map_err(map_sql)?;
        if exists == 0 {
            return Ok(0);
        }
        self.conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .map_err(map_sql)
    }

    pub fn create_profile(
        &self,
        id: Uuid,
        display_name: &str,
        created_at: &str,
    ) -> Result<ProfileRecord, AppError> {
        let display_name = normalize_name(display_name)?;
        require_utc_timestamp(created_at)?;
        self.conn
            .execute(
                "INSERT INTO profiles (id, schema_version, display_name, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![id.to_string(), PROFILE_SCHEMA, display_name, created_at],
            )
            .map_err(map_write)?;
        Ok(ProfileRecord {
            id,
            display_name,
            created_at: created_at.to_owned(),
            updated_at: created_at.to_owned(),
        })
    }

    pub fn rename_profile(
        &self,
        id: Uuid,
        display_name: &str,
        updated_at: &str,
    ) -> Result<(), AppError> {
        let display_name = normalize_name(display_name)?;
        require_utc_timestamp(updated_at)?;
        let changed = self
            .conn
            .execute(
                "UPDATE profiles SET display_name = ?1, updated_at = ?2 WHERE id = ?3",
                params![display_name, updated_at, id.to_string()],
            )
            .map_err(map_sql)?;
        if changed == 0 {
            return Err(not_found("profile was not found"));
        }
        Ok(())
    }

    pub fn get_profile(&self, id: Uuid) -> Result<ProfileRecord, AppError> {
        self.conn
            .query_row(
                "SELECT display_name, created_at, updated_at FROM profiles WHERE id = ?1",
                params![id.to_string()],
                |row| {
                    Ok(ProfileRecord {
                        id,
                        display_name: row.get(0)?,
                        created_at: row.get(1)?,
                        updated_at: row.get(2)?,
                    })
                },
            )
            .map_err(map_missing("profile was not found"))
    }

    pub fn recent_calibration_cpm(&self, profile_id: Uuid) -> Result<Vec<f64>, AppError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT units_per_minute FROM (
                    SELECT units_per_minute, created_at FROM typing_calibrations
                    WHERE profile_id = ?1
                    ORDER BY created_at DESC
                    LIMIT 10
                 ) ORDER BY created_at ASC",
            )
            .map_err(map_sql)?;
        let rows = statement
            .query_map(params![profile_id.to_string()], |row| {
                let units: i64 = row.get(0)?;
                Ok(units as f64)
            })
            .map_err(map_sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_sql)
    }

    pub fn insert_calibration(
        &self,
        id: Uuid,
        profile_id: Uuid,
        units_per_minute: i64,
        created_at: &str,
    ) -> Result<(), AppError> {
        require_utc_timestamp(created_at)?;
        self.conn
            .execute(
                "INSERT INTO typing_calibrations
                 (id, schema_version, profile_id, units_per_minute, created_at)
                 VALUES (?1, 'typing-calibration.v1', ?2, ?3, ?4)",
                params![
                    id.to_string(),
                    profile_id.to_string(),
                    units_per_minute,
                    created_at
                ],
            )
            .map_err(map_write)?;
        Ok(())
    }

    pub fn list_profiles(&self) -> Result<Vec<ProfileRecord>, AppError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, display_name, created_at, updated_at FROM profiles ORDER BY created_at, id",
            )
            .map_err(map_sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok(ProfileRecord {
                    id: parse_uuid(&row.get::<_, String>(0)?)?,
                    display_name: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })
            .map_err(map_sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_sql)
    }

    pub fn delete_profile(&self, id: Uuid, confirmed: bool) -> Result<(), AppError> {
        if !confirmed {
            return Err(AppError::new(
                ErrorCode::PreconditionFailed,
                "delete requires confirmation",
            ));
        }
        let changed = self
            .conn
            .execute(
                "DELETE FROM profiles WHERE id = ?1",
                params![id.to_string()],
            )
            .map_err(map_sql)?;
        if changed == 0 {
            return Err(not_found("profile was not found"));
        }
        Ok(())
    }

    pub fn delete_match(&self, id: Uuid, confirmed: bool) -> Result<(), AppError> {
        if !confirmed {
            return Err(AppError::new(
                ErrorCode::PreconditionFailed,
                "delete requires confirmation",
            ));
        }
        let changed = self
            .conn
            .execute("DELETE FROM matches WHERE id = ?1", params![id.to_string()])
            .map_err(map_sql)?;
        if changed == 0 {
            return Err(not_found("match was not found"));
        }
        Ok(())
    }

    pub fn save_settings(&self, settings: &AppSettings, updated_at: &str) -> Result<(), AppError> {
        validate_settings(settings)?;
        require_utc_timestamp(updated_at)?;
        let values = [
            ("model_id", settings.model_id.clone()),
            ("base_url", settings.base_url.clone()),
            (
                "speech_rate_per_minute",
                settings.speech_rate_per_minute.to_string(),
            ),
            ("theme", settings.theme.clone()),
            ("thinking_level", settings.thinking_level.clone()),
            ("connection_status", settings.connection_status.clone()),
            ("last_tested_at", settings.last_tested_at.clone()),
        ];
        let tx = self.conn.unchecked_transaction().map_err(map_sql)?;
        for (key, value) in values {
            tx.execute(
                "INSERT INTO app_settings (key, schema_version, value, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                params![key, SETTINGS_SCHEMA, value, updated_at],
            )
            .map_err(map_sql)?;
        }
        tx.commit().map_err(map_sql)?;
        Ok(())
    }

    pub fn load_settings(&self) -> Result<AppSettings, AppError> {
        let mut settings = AppSettings::default();
        let mut statement = self
            .conn
            .prepare("SELECT key, value FROM app_settings")
            .map_err(map_sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(map_sql)?;
        for row in rows {
            let (key, value) = row.map_err(map_sql)?;
            match key.as_str() {
                "model_id" => settings.model_id = value,
                "base_url" => settings.base_url = value,
                "speech_rate_per_minute" => {
                    settings.speech_rate_per_minute = value.parse().map_err(|_| {
                        AppError::new(ErrorCode::Internal, "database operation failed")
                    })?;
                }
                "theme" => settings.theme = value,
                "thinking_level" => settings.thinking_level = value,
                "connection_status" => settings.connection_status = value,
                "last_tested_at" => settings.last_tested_at = value,
                _ => {}
            }
        }
        Ok(settings)
    }

    pub fn recent_matches(
        &self,
        profile_id: Uuid,
        limit: usize,
    ) -> Result<Vec<MatchListItem>, AppError> {
        let limit = limit.min(50);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, topic, phase, updated_at FROM matches
                 WHERE profile_id = ?1
                 ORDER BY updated_at DESC, id ASC
                 LIMIT ?2",
            )
            .map_err(map_sql)?;
        let rows = statement
            .query_map(params![profile_id.to_string(), limit as i64], |row| {
                Ok(MatchListItem {
                    id: parse_uuid(&row.get::<_, String>(0)?)?,
                    topic: row.get(1)?,
                    phase: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })
            .map_err(map_sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_sql)
    }

    pub fn save_match(
        &self,
        profile_id: Uuid,
        state: &MatchAggregate,
        saved_at: &str,
    ) -> Result<(), AppError> {
        require_utc_timestamp(saved_at)?;
        self.ensure_profile(profile_id)?;
        if let Some(owner) = self.match_owner(state.id)? {
            if owner != profile_id {
                return Err(AppError::new(
                    ErrorCode::Conflict,
                    "match belongs to another profile",
                ));
            }
        }
        let snapshot = serde_json::to_string(&state.to_snapshot()).map_err(|_| internal())?;
        let mode = enum_name(&state.mode)?;
        let phase = enum_name(&state.phase)?;
        let tx = self.conn.unchecked_transaction().map_err(map_sql)?;
        tx.execute(
            "INSERT INTO matches (
                id, schema_version, profile_id, mode, phase, topic, speech_rate_per_minute,
                rules_hash, lineup_hash, non_standard, charged_ms, ai_wait_ms, snapshot_json,
                created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)
             ON CONFLICT(id) DO UPDATE SET
                mode = excluded.mode,
                phase = excluded.phase,
                topic = excluded.topic,
                speech_rate_per_minute = excluded.speech_rate_per_minute,
                rules_hash = excluded.rules_hash,
                lineup_hash = excluded.lineup_hash,
                non_standard = excluded.non_standard,
                charged_ms = excluded.charged_ms,
                ai_wait_ms = excluded.ai_wait_ms,
                snapshot_json = excluded.snapshot_json,
                updated_at = excluded.updated_at",
            params![
                state.id.to_string(),
                MATCH_SCHEMA,
                profile_id.to_string(),
                mode,
                phase,
                state.topic,
                state.speech_rate_per_minute,
                state.rules_hash,
                state.lineup_hash,
                i64::from(state.non_standard),
                as_i64(state.charged_ms)?,
                as_i64(state.ai_wait_ms)?,
                snapshot,
                saved_at,
            ],
        )
        .map_err(map_sql)?;
        write_materials(&tx, state)?;
        write_rules(&tx, state)?;
        write_assignments(&tx, state)?;
        write_existing_turns(&tx, state, saved_at)?;
        // Close older windows atomically with their formal submission. Keep
        // tombstones so delayed autosave cannot resurrect a submitted draft.
        tx.execute(
            "UPDATE draft_records SET closed = 1 WHERE match_id = ?1 AND window_id <> ?2",
            params![state.id.to_string(), state.control_window_id()],
        )
        .map_err(map_sql)?;
        if let Some(session) = state.schedule.get(state.cursor).and_then(|event| {
            state
                .cross_exam_sessions
                .iter()
                .find(|session| session.event_id == event.id)
        }) {
            if !matches!(
                session.phase,
                crate::domain::events::CrossExamSessionPhase::Selecting
                    | crate::domain::events::CrossExamSessionPhase::QuestionPending
            ) {
                tx.execute("UPDATE draft_records SET closed = 1 WHERE match_id = ?1 AND window_id = ?2 AND field_kind = 'question'", params![state.id.to_string(), state.control_window_id()]).map_err(map_sql)?;
            }
        }
        tx.commit().map_err(map_sql)?;
        Ok(())
    }

    pub fn load_match(&self, id: Uuid) -> Result<MatchAggregate, AppError> {
        let json: String = self
            .conn
            .query_row(
                "SELECT snapshot_json FROM matches WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(map_missing("match was not found"))?;
        let snapshot: MatchSnapshotV1 = serde_json::from_str(&json)
            .map_err(|_| AppError::new(ErrorCode::Internal, "database operation failed"))?;
        MatchAggregate::from_snapshot(snapshot)
    }

    pub fn save_report(
        &self,
        match_id: Uuid,
        report: &crate::domain::report::PublicReport,
        saved_at: &str,
    ) -> Result<(), AppError> {
        require_utc_timestamp(saved_at)?;
        let body = serde_json::to_string(report).map_err(|_| internal())?;
        self.conn
            .execute(
                "INSERT INTO reports (id, match_id, schema_version, formula_version, standard, body_json, created_at)
                 VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                    formula_version = excluded.formula_version,
                    standard = excluded.standard,
                    body_json = excluded.body_json,
                    created_at = excluded.created_at",
                params![
                    match_id.to_string(),
                    crate::domain::events::REPORT_SCHEMA_VERSION,
                    report.formula_version,
                    i64::from(report.standard),
                    body,
                    saved_at,
                ],
            )
            .map_err(map_sql)?;
        Ok(())
    }

    pub fn load_report(
        &self,
        match_id: Uuid,
    ) -> Result<Option<crate::domain::report::PublicReport>, AppError> {
        let result = self.conn.query_row(
            "SELECT body_json FROM reports WHERE match_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![match_id.to_string()],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(body) => serde_json::from_str(&body)
                .map(Some)
                .map_err(|_| internal()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(map_sql(error)),
        }
    }

    pub fn append_completed_turn(
        &self,
        match_id: Uuid,
        turn: &CompletedTurn,
    ) -> Result<(), AppError> {
        require_utc_timestamp(&turn.saved_at)?;
        if turn.metrics.turn_id != turn.turn_id || turn.transcript.event_id != turn.event.id {
            return Err(AppError::new(
                ErrorCode::InvalidArgument,
                "turn references do not match",
            ));
        }
        let mut snapshot = self.load_match(match_id)?.to_snapshot();
        if snapshot
            .events
            .iter()
            .any(|event| event.id == turn.event.id)
        {
            return Err(AppError::new(
                ErrorCode::Conflict,
                "match event already exists",
            ));
        }
        snapshot.events.push(turn.event.clone());
        snapshot.transcripts.push(turn.transcript.clone());
        snapshot.phase = turn.phase;
        snapshot.charged_ms = turn.charged_ms;
        snapshot.ai_wait_ms = turn.ai_wait_ms;
        snapshot.cursor = turn.cursor;
        if snapshot.next_sequence <= turn.event.sequence {
            snapshot.next_sequence = turn.event.sequence + 1;
        }
        let snapshot_json = serde_json::to_string(&snapshot).map_err(|_| internal())?;
        let tx = self.conn.unchecked_transaction().map_err(map_sql)?;
        insert_event(&tx, match_id, &turn.event, &turn.saved_at)?;
        insert_turn(
            &tx,
            match_id,
            turn.turn_id,
            turn.event.id,
            &turn.transcript,
            turn.input_ms,
            &turn.saved_at,
        )?;
        insert_transcript(&tx, match_id, turn.turn_id, &turn.transcript)?;
        insert_metrics(&tx, match_id, &turn.metrics)?;
        tx.execute(
            "UPDATE matches
             SET phase = ?1, charged_ms = ?2, ai_wait_ms = ?3, non_standard = ?4,
                 snapshot_json = ?5, updated_at = ?6
             WHERE id = ?7",
            params![
                enum_name(&turn.phase)?,
                as_i64(turn.charged_ms)?,
                as_i64(turn.ai_wait_ms)?,
                i64::from(snapshot.non_standard),
                snapshot_json,
                turn.saved_at,
                match_id.to_string(),
            ],
        )
        .map_err(map_sql)?;
        tx.commit().map_err(map_sql)?;
        Ok(())
    }

    fn ensure_profile(&self, id: Uuid) -> Result<(), AppError> {
        let exists: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM profiles WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(map_sql)?;
        if exists == 0 {
            Err(not_found("profile was not found"))
        } else {
            Ok(())
        }
    }

    fn match_owner(&self, id: Uuid) -> Result<Option<Uuid>, AppError> {
        match self.conn.query_row(
            "SELECT profile_id FROM matches WHERE id = ?1",
            params![id.to_string()],
            |row| row.get::<_, String>(0),
        ) {
            Ok(value) => parse_uuid(&value).map(Some).map_err(map_sql),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(map_sql(err)),
        }
    }
}

impl MatchRepository for SqliteStore {
    fn save(
        &mut self,
        profile_id: Uuid,
        state: &MatchAggregate,
        saved_at: &str,
    ) -> Result<(), AppError> {
        self.save_match(profile_id, state, saved_at)
    }

    fn load(&self, id: Uuid) -> Result<MatchAggregate, AppError> {
        self.load_match(id)
    }
}

fn write_materials(tx: &rusqlite::Transaction<'_>, state: &MatchAggregate) -> Result<(), AppError> {
    tx.execute(
        "INSERT INTO match_materials (match_id, schema_version, materials)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(match_id) DO UPDATE SET materials = excluded.materials",
        params![state.id.to_string(), MATERIAL_SCHEMA, state.materials],
    )
    .map_err(map_sql)?;
    Ok(())
}

fn write_rules(tx: &rusqlite::Transaction<'_>, state: &MatchAggregate) -> Result<(), AppError> {
    let Some(ruleset) = &state.ruleset else {
        return Ok(());
    };
    let json = serde_json::to_string(ruleset).map_err(|_| internal())?;
    tx.execute(
        "INSERT INTO match_rule_snapshots (match_id, schema_version, ruleset_json)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(match_id) DO UPDATE SET ruleset_json = excluded.ruleset_json",
        params![state.id.to_string(), RULE_SNAPSHOT_SCHEMA, json],
    )
    .map_err(map_sql)?;
    Ok(())
}

fn write_assignments(
    tx: &rusqlite::Transaction<'_>,
    state: &MatchAggregate,
) -> Result<(), AppError> {
    tx.execute(
        "DELETE FROM debater_assignments WHERE match_id = ?1",
        params![state.id.to_string()],
    )
    .map_err(map_sql)?;
    for assignment in &state.assignments {
        tx.execute(
            "INSERT INTO debater_assignments (match_id, slot, schema_version, controller)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                state.id.to_string(),
                enum_name(&assignment.slot)?,
                assignment.schema_version,
                enum_name(&assignment.controller)?,
            ],
        )
        .map_err(map_sql)?;
    }
    Ok(())
}

fn write_existing_turns(
    tx: &rusqlite::Transaction<'_>,
    state: &MatchAggregate,
    saved_at: &str,
) -> Result<(), AppError> {
    for event in &state.events {
        insert_event(tx, state.id, event, saved_at)?;
    }
    for transcript in &state.transcripts {
        insert_turn(
            tx,
            state.id,
            transcript.id,
            transcript.event_id,
            transcript,
            0,
            saved_at,
        )?;
        insert_transcript(tx, state.id, transcript.id, transcript)?;
        let metrics = TurnMetricSummaryV1 {
            schema_version: crate::domain::events::METRIC_SCHEMA_VERSION.to_owned(),
            turn_id: transcript.id,
            forward_typed_units: transcript.effective_units,
            effective_positive_units: transcript.effective_units,
            reverse_edited_units: 0,
            paste_units: 0,
            pause_ms: 0,
        };
        insert_metrics(tx, state.id, &metrics)?;
    }
    Ok(())
}

fn insert_event(
    tx: &rusqlite::Transaction<'_>,
    match_id: Uuid,
    event: &MatchEventV1,
    saved_at: &str,
) -> Result<(), AppError> {
    tx.execute(
        "INSERT OR IGNORE INTO match_events
         (id, match_id, schema_version, sequence, kind, slot, summary, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            event.id.to_string(),
            match_id.to_string(),
            event.schema_version,
            event.sequence,
            enum_name(&event.kind)?,
            option_name(&event.slot)?,
            event.summary,
            saved_at,
        ],
    )
    .map_err(map_sql)?;
    Ok(())
}

fn insert_turn(
    tx: &rusqlite::Transaction<'_>,
    match_id: Uuid,
    turn_id: Uuid,
    event_id: Uuid,
    transcript: &TranscriptEntryV1,
    input_ms: u64,
    saved_at: &str,
) -> Result<(), AppError> {
    tx.execute(
        "INSERT OR IGNORE INTO turns
         (id, match_id, event_id, schema_version, slot, input_ms, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            turn_id.to_string(),
            match_id.to_string(),
            event_id.to_string(),
            TURN_SCHEMA,
            enum_name(&transcript.slot)?,
            as_i64(input_ms)?,
            saved_at,
        ],
    )
    .map_err(map_sql)?;
    Ok(())
}

fn insert_transcript(
    tx: &rusqlite::Transaction<'_>,
    match_id: Uuid,
    turn_id: Uuid,
    transcript: &TranscriptEntryV1,
) -> Result<(), AppError> {
    tx.execute(
        "INSERT OR IGNORE INTO transcript_entries (
            id, match_id, turn_id, schema_version, event_id, slot, full_text, effective_text,
            effective_units, overflow_text
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            transcript.id.to_string(),
            match_id.to_string(),
            turn_id.to_string(),
            transcript.schema_version,
            transcript.event_id.to_string(),
            enum_name(&transcript.slot)?,
            transcript.full_text,
            transcript.effective_text,
            transcript.effective_units,
            transcript.overflow_text,
        ],
    )
    .map_err(map_sql)?;
    Ok(())
}

fn insert_metrics(
    tx: &rusqlite::Transaction<'_>,
    match_id: Uuid,
    metrics: &TurnMetricSummaryV1,
) -> Result<(), AppError> {
    tx.execute(
        "INSERT OR IGNORE INTO turn_metric_summaries (
            turn_id, schema_version, match_id, forward_typed_units, effective_positive_units,
            reverse_edited_units, paste_units, pause_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            metrics.turn_id.to_string(),
            metrics.schema_version,
            match_id.to_string(),
            metrics.forward_typed_units,
            metrics.effective_positive_units,
            metrics.reverse_edited_units,
            metrics.paste_units,
            as_i64(metrics.pause_ms)?,
        ],
    )
    .map_err(map_sql)?;
    Ok(())
}

fn validate_settings(settings: &AppSettings) -> Result<(), AppError> {
    reject_secret_text(&settings.model_id)?;
    reject_secret_text(&settings.base_url)?;
    reject_secret_text(&settings.theme)?;
    if !(1..=2_000).contains(&settings.speech_rate_per_minute) {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "speech rate is invalid",
        ));
    }
    if !settings.model_id.is_empty()
        && !settings
            .model_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._:-".contains(character))
    {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "model id is invalid",
        ));
    }
    if settings.base_url.contains('@') {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "base url is invalid",
        ));
    }
    if !THEMES.contains(&settings.theme.as_str()) {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "theme is invalid",
        ));
    }
    reject_secret_text(&settings.thinking_level)?;
    reject_secret_text(&settings.connection_status)?;
    reject_secret_text(&settings.last_tested_at)?;
    if !THINKING_LEVELS.contains(&settings.thinking_level.as_str()) {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "thinking level is invalid",
        ));
    }
    if crate::application::model_settings::ConnectionStatus::parse(&settings.connection_status)
        .is_none()
    {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "connection status is invalid",
        ));
    }
    Ok(())
}

fn reject_secret_text(value: &str) -> Result<(), AppError> {
    if value.contains("sk-") {
        Err(AppError::new(
            ErrorCode::InvalidArgument,
            "settings cannot hold a credential",
        ))
    } else {
        Ok(())
    }
}

fn normalize_name(value: &str) -> Result<String, AppError> {
    let name = value.trim();
    let length = name.chars().count();
    if name.is_empty() || length > 80 {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "profile name is invalid",
        ));
    }
    Ok(name.to_owned())
}

fn enum_name<T: serde::Serialize>(value: &T) -> Result<String, AppError> {
    let json = serde_json::to_value(value).map_err(|_| internal())?;
    json.as_str().map(str::to_owned).ok_or_else(internal)
}

fn option_name<T: serde::Serialize>(value: &Option<T>) -> Result<Option<String>, AppError> {
    value.as_ref().map(enum_name).transpose()
}

fn as_i64(value: u64) -> Result<i64, AppError> {
    i64::try_from(value)
        .map_err(|_| AppError::new(ErrorCode::InvalidArgument, "duration is too large"))
}

fn parse_uuid(value: &str) -> Result<Uuid, rusqlite::Error> {
    Uuid::parse_str(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn internal() -> AppError {
    AppError::new(ErrorCode::Internal, "database operation failed")
}

fn not_found(message: &str) -> AppError {
    AppError::new(ErrorCode::NotFound, message)
}

fn map_sql(_err: rusqlite::Error) -> AppError {
    internal()
}

fn map_write(err: rusqlite::Error) -> AppError {
    if let rusqlite::Error::SqliteFailure(error, _) = &err {
        if error.code == rusqlite::ErrorCode::ConstraintViolation {
            return AppError::new(ErrorCode::Conflict, "the record already exists");
        }
    }
    internal()
}

fn map_missing(message: &'static str) -> impl Fn(rusqlite::Error) -> AppError {
    move |err| {
        if matches!(err, rusqlite::Error::QueryReturnedNoRows) {
            not_found(message)
        } else {
            map_sql(err)
        }
    }
}

#[cfg(test)]
impl SqliteStore {
    fn testing_execute(&self, sql: &str) -> Result<(), AppError> {
        self.conn.execute_batch(sql).map_err(map_sql)
    }

    fn child_counts(&self, id: Uuid) -> Result<(i64, i64, i64, i64), AppError> {
        let count = |sql: &str| {
            self.conn
                .query_row(sql, params![id.to_string()], |row| row.get(0))
                .map_err(map_sql)
        };
        Ok((
            count("SELECT COUNT(*) FROM match_events WHERE match_id = ?1")?,
            count("SELECT COUNT(*) FROM turns WHERE match_id = ?1")?,
            count("SELECT COUNT(*) FROM transcript_entries WHERE match_id = ?1")?,
            count("SELECT COUNT(*) FROM turn_metric_summaries WHERE match_id = ?1")?,
        ))
    }

    fn column_names(&self) -> Result<Vec<String>, AppError> {
        let mut master = self
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .map_err(map_sql)?;
        let tables = master
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(map_sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql)?;
        let mut names = Vec::new();
        for table in tables {
            if !table
                .chars()
                .all(|character| character.is_ascii_lowercase() || character == '_')
            {
                continue;
            }
            let mut info = self
                .conn
                .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .map_err(map_sql)?;
            let columns = info
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(map_sql)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql)?;
            names.extend(columns);
        }
        Ok(names)
    }

    fn table_names(&self) -> Result<Vec<String>, AppError> {
        let mut master = self
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .map_err(map_sql)?;
        let names = master
            .query_map([], |row| row.get(0))
            .map_err(map_sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql)?;
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use uuid::Uuid;

    use super::{AppSettings, CompletedTurn, SqliteStore};
    use crate::domain::events::{
        MatchEventKind, MatchEventV1, TranscriptEntryV1, TurnMetricSummaryV1, EVENT_SCHEMA_VERSION,
        METRIC_SCHEMA_VERSION, TRANSCRIPT_SCHEMA_VERSION,
    };
    use crate::domain::match_engine::{play_constructed_match, MatchAggregate, MatchPhase};
    use crate::domain::report::{PublicReport, REPORT_FORMULA};
    use crate::domain::ruleset::SlotId;
    use crate::error::ErrorCode;
    use crate::infrastructure::credentials::{MemorySecretStore, SecretStore};

    const CREATED: &str = "2026-09-23T02:00:00Z";
    const UPDATED: &str = "2026-09-23T03:00:00Z";

    #[test]
    fn experience_migration_adds_private_drafts_and_practice_without_rebuilding_v1() {
        let store = SqliteStore::open(":memory:").unwrap();
        assert_eq!(store.schema_version().unwrap(), 2);
        assert!(store
            .table_names()
            .unwrap()
            .contains(&"draft_records".to_owned()));
        assert!(store
            .table_names()
            .unwrap()
            .contains(&"practice_records".to_owned()));
        assert!(store
            .table_names()
            .unwrap()
            .contains(&"transcript_entries".to_owned()));
    }

    #[test]
    fn v1_migration_keeps_existing_profile_bytes_and_creates_a_consistent_backup() {
        let path = TempDb::new();
        let conn = rusqlite::Connection::open(path.path()).unwrap();
        conn.execute_batch(include_str!("migrations/001_init.sql"))
            .unwrap();
        conn.execute("INSERT INTO schema_migrations VALUES (1,?1)", [CREATED])
            .unwrap();
        conn.execute(
            "INSERT INTO profiles VALUES ('legacy','profile.v1','已有真实档案',?1,?1)",
            [CREATED],
        )
        .unwrap();
        drop(conn);
        let store = SqliteStore::open(path.path()).unwrap();
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT display_name FROM profiles WHERE id='legacy'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "已有真实档案"
        );
        let prefix = format!(
            "{}.v1-backup-",
            path.path().file_stem().unwrap().to_string_lossy()
        );
        let backup = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .find(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .unwrap()
            .path();
        let original = rusqlite::Connection::open(&backup).unwrap();
        assert_eq!(
            original
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            original
                .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        drop(original);
        let _ = std::fs::remove_file(backup);
    }

    fn sample_report() -> PublicReport {
        PublicReport {
            formula_version: REPORT_FORMULA.to_owned(),
            model_id: "deepseek-flash".to_owned(),
            difficulty: "标准".to_owned(),
            speech_rate: 240,
            personal: 88.0,
            program: 18.0,
            semantic: 70.0,
            team_winner: "affirmative".to_owned(),
            standard: true,
            non_standard_reason: String::new(),
            length: 8.0,
            input_control: 7.0,
            execution: 3.0,
            trace: "formula".to_owned(),
            scoring_status: "ready".to_owned(),
            low_confidence: false,
            dimensions: Vec::new(),
            stage_evaluations: Vec::new(),
            review_summary: String::new(),
            team_summary: String::new(),
        }
    }

    struct TempDb(PathBuf);

    impl TempDb {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            Self(std::env::temp_dir().join(format!("debate-phase2-{nanos}.sqlite")))
        }

        fn path(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn open() -> (TempDb, SqliteStore) {
        let file = TempDb::new();
        let store = SqliteStore::open(file.path()).expect("open");
        (file, store)
    }

    #[test]
    fn migrations_upgrade_an_empty_database_and_are_idempotent() {
        let (_file, store) = open();
        assert_eq!(store.schema_version().expect("version"), 2);
        let tables = store.table_names().expect("tables");
        for name in [
            "profiles",
            "typing_calibrations",
            "rule_templates",
            "matches",
            "match_rule_snapshots",
            "match_materials",
            "debater_assignments",
            "match_events",
            "turns",
            "transcript_entries",
            "turn_metric_summaries",
            "ai_request_metadata",
            "stage_evaluations",
            "match_reviews",
            "reports",
            "app_settings",
        ] {
            assert!(tables.iter().any(|table| table == name), "{name}");
        }
        store.migrate().expect("second migrate");
        assert_eq!(store.schema_version().expect("version"), 2);
        assert_eq!(store.table_names().expect("tables").len(), tables.len());
        let columns = store.column_names().expect("columns");
        assert!(columns.iter().all(|name| {
            let lower = name.to_ascii_lowercase();
            !lower.contains("api_key")
                && !lower.contains("password")
                && !lower.contains("secret")
                && lower != "token"
        }));
    }

    #[test]
    fn failed_turn_transaction_keeps_the_previous_match() {
        let (_file, store) = open();
        let profile = Uuid::from_u128(10);
        let mut state = MatchAggregate::new(
            Uuid::from_u128(11),
            crate::domain::ruleset::MatchMode::Training,
            "辩题",
            "",
            240,
        );
        store
            .create_profile(profile, "正方", CREATED)
            .expect("profile");
        store.save_match(profile, &state, CREATED).expect("save");
        let before = store.load_match(state.id).expect("load").to_snapshot();
        store
            .testing_execute(
                "CREATE TRIGGER phase2_fail_transcript
                 BEFORE INSERT ON transcript_entries
                 BEGIN
                     SELECT RAISE(ABORT, 'injected fault');
                 END;",
            )
            .expect("trigger");
        let turn = completed_turn();
        let error = store
            .append_completed_turn(state.id, &turn)
            .expect_err("fault");
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(
            store.load_match(state.id).expect("load").to_snapshot(),
            before
        );
        assert_eq!(store.child_counts(state.id).expect("counts"), (0, 0, 0, 0));

        store
            .testing_execute("DROP TRIGGER phase2_fail_transcript")
            .expect("drop trigger");
        store
            .append_completed_turn(state.id, &turn)
            .expect("append");
        assert_eq!(store.child_counts(state.id).expect("counts"), (1, 1, 1, 1));
        state = store.load_match(state.id).expect("reload");
        assert_eq!(state.phase, MatchPhase::InProgress);
        assert_eq!(state.transcripts.len(), 1);
        assert_eq!(state.charged_ms, 1_500);
        assert_eq!(state.ai_wait_ms, 0);
        let duplicate = store
            .append_completed_turn(state.id, &turn)
            .expect_err("duplicate");
        assert_eq!(duplicate.code, ErrorCode::Conflict);
        assert_eq!(store.child_counts(state.id).expect("counts"), (1, 1, 1, 1));
    }

    #[test]
    fn deleting_a_profile_cascades_only_its_matches() {
        let (_file, store) = open();
        let affirmative = Uuid::from_u128(20);
        let negative = Uuid::from_u128(21);
        store
            .create_profile(affirmative, "正方档案", CREATED)
            .expect("aff");
        store
            .create_profile(negative, "反方档案", CREATED)
            .expect("neg");
        let older = MatchAggregate::new(
            Uuid::from_u128(22),
            crate::domain::ruleset::MatchMode::Training,
            "旧题",
            "",
            240,
        );
        let newer = MatchAggregate::new(
            Uuid::from_u128(23),
            crate::domain::ruleset::MatchMode::Scoring,
            "新题",
            "",
            240,
        );
        store
            .save_match(affirmative, &older, CREATED)
            .expect("older");
        store.save_match(negative, &newer, UPDATED).expect("newer");
        store
            .delete_profile(affirmative, false)
            .expect_err("confirm");
        assert!(store.get_profile(affirmative).is_ok());
        assert!(store.load_match(older.id).is_ok());
        store.delete_profile(affirmative, true).expect("delete");
        assert_eq!(
            store.get_profile(affirmative).expect_err("gone").code,
            ErrorCode::NotFound
        );
        assert_eq!(
            store.load_match(older.id).expect_err("gone").code,
            ErrorCode::NotFound
        );
        assert_eq!(store.load_match(newer.id).expect("kept").topic, "新题");
        assert_eq!(
            store.get_profile(negative).expect("kept").display_name,
            "反方档案"
        );
        let recent = store.recent_matches(negative, 1).expect("recent");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, newer.id);
        store.delete_match(newer.id, false).expect_err("confirm");
        store.delete_match(newer.id, true).expect("delete match");
        assert!(store.get_profile(negative).is_ok());
        assert_eq!(
            store.load_match(newer.id).expect_err("match gone").code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn reopening_the_database_restores_profile_settings_and_match() {
        let file = TempDb::new();
        let profile = Uuid::from_u128(30);
        let finished = play_constructed_match();
        let expected = finished.to_snapshot();
        let settings = AppSettings {
            model_id: "local-model".to_owned(),
            base_url: String::new(),
            speech_rate_per_minute: 180,
            theme: "dark".to_owned(),
            thinking_level: "high".to_owned(),
            connection_status: "not_configured".to_owned(),
            last_tested_at: String::new(),
        };
        {
            let store = SqliteStore::open(file.path()).expect("open");
            store
                .create_profile(profile, "参赛者", CREATED)
                .expect("profile");
            store.save_settings(&settings, CREATED).expect("settings");
            store
                .save_match(profile, &finished, UPDATED)
                .expect("match");
        }
        let store = SqliteStore::open(file.path()).expect("reopen");
        assert_eq!(
            store.get_profile(profile).expect("profile").display_name,
            "参赛者"
        );
        assert_eq!(store.load_settings().expect("settings"), settings);
        assert_eq!(
            store.load_match(finished.id).expect("match").to_snapshot(),
            expected
        );
        assert_eq!(
            store.recent_matches(profile, 5).expect("recent")[0].id,
            finished.id
        );
    }

    #[test]
    fn report_is_persisted_and_loaded_with_the_match() {
        let (_file, store) = open();
        let profile = Uuid::from_u128(0x44);
        store.create_profile(profile, "报告档案", CREATED).unwrap();
        let state = MatchAggregate::new(
            Uuid::from_u128(0x45),
            crate::domain::ruleset::MatchMode::Training,
            "辩题",
            "",
            240,
        );
        store.save_match(profile, &state, CREATED).unwrap();
        let report = sample_report();
        store.save_report(state.id, &report, UPDATED).unwrap();
        assert_eq!(store.load_report(state.id).unwrap(), Some(report));
    }

    #[test]
    fn api_key_is_not_written_into_sqlite() {
        let file = TempDb::new();
        let secret = "sk-phase2-do-not-store";
        let secrets = MemorySecretStore::new();
        secrets.save_api_key(secret).expect("credential");
        secrets.test_api_key().expect("test");
        assert_eq!(secrets.read_api_key().expect("read"), secret);
        let store = SqliteStore::open(file.path()).expect("open");
        let rejected = AppSettings {
            model_id: secret.to_owned(),
            ..AppSettings::default()
        };
        assert_eq!(
            store
                .save_settings(&rejected, CREATED)
                .expect_err("reject")
                .code,
            ErrorCode::InvalidArgument
        );
        store
            .save_settings(&AppSettings::default(), CREATED)
            .expect("settings");
        let profile = Uuid::from_u128(40);
        let state = MatchAggregate::new(
            Uuid::from_u128(41),
            crate::domain::ruleset::MatchMode::Training,
            "公开辩题",
            "",
            240,
        );
        store
            .create_profile(profile, "档案", CREATED)
            .expect("profile");
        store.save_match(profile, &state, CREATED).expect("match");
        drop(store);
        let bytes = std::fs::read(file.path()).expect("bytes");
        assert!(!bytes
            .windows(secret.len())
            .any(|window| window == secret.as_bytes()));
        secrets.clear_api_key().expect("clear");
        assert_eq!(
            secrets.read_api_key().expect_err("cleared").code,
            ErrorCode::NotFound
        );
    }

    fn completed_turn() -> CompletedTurn {
        let event_id = Uuid::from_u128(50);
        let turn_id = Uuid::from_u128(51);
        let transcript_id = Uuid::from_u128(52);
        CompletedTurn {
            event: MatchEventV1 {
                schema_version: EVENT_SCHEMA_VERSION.to_owned(),
                id: event_id,
                sequence: 1,
                kind: MatchEventKind::SpeechSubmitted,
                slot: Some(SlotId::Aff1),
                summary: "提交".to_owned(),
            },
            turn_id,
            input_ms: 1_500,
            transcript: TranscriptEntryV1 {
                schema_version: TRANSCRIPT_SCHEMA_VERSION.to_owned(),
                id: transcript_id,
                event_id,
                slot: SlotId::Aff1,
                controller: None,
                full_text: "陈词".to_owned(),
                effective_text: "陈词".to_owned(),
                effective_units: 2,
                word_cap_at_submission: None,
                overflow_text: String::new(),
                cross_exam_exchange_id: None,
                response_target_text: String::new(),
                cross_exam_turn_kind: None,
                cross_exam_round_number: 0,
            },
            metrics: TurnMetricSummaryV1 {
                schema_version: METRIC_SCHEMA_VERSION.to_owned(),
                turn_id,
                forward_typed_units: 2,
                effective_positive_units: 2,
                reverse_edited_units: 0,
                paste_units: 0,
                pause_ms: 0,
            },
            phase: MatchPhase::InProgress,
            charged_ms: 1_500,
            ai_wait_ms: 0,
            cursor: 1,
            saved_at: UPDATED.to_owned(),
        }
    }
}
