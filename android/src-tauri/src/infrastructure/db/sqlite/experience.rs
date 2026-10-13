use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{enum_name, internal, map_sql, require_utc_timestamp, SqliteStore};
use crate::domain::ruleset::SlotId;
use crate::domain::telemetry::{validate_human_turn, ClaimedMetrics, InputEvent};
use crate::error::{AppError, ErrorCode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftKind {
    Speech,
    Question,
    Answer,
}

impl DraftKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Speech => "speech",
            Self::Question => "question",
            Self::Answer => "answer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftKey {
    pub match_id: Uuid,
    pub window_id: String,
    pub slot: SlotId,
    pub field_kind: DraftKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftPayload {
    pub text: String,
    pub events: Vec<InputEvent>,
    pub claimed: ClaimedMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedDraft {
    pub key: DraftKey,
    pub revision: u64,
    pub payload: DraftPayload,
    pub telemetry_trusted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeRecord {
    pub id: Uuid,
    pub match_id: Uuid,
    pub source_turn_id: Uuid,
    pub context: crate::domain::practice::PracticeContext,
    pub rewritten_text: String,
    pub feedback: Option<String>,
    pub revision: u64,
}

impl DraftPayload {
    fn trusted(&self) -> bool {
        validate_human_turn(&self.events, &self.text, &self.claimed, u32::MAX).is_ok()
    }
}

impl SqliteStore {
    pub fn create_practice(
        &self,
        profile: Uuid,
        match_id: Uuid,
        source: Uuid,
        id: Uuid,
        now: &str,
    ) -> Result<PracticeRecord, AppError> {
        require_utc_timestamp(now)?;
        if self.match_owner(match_id)? != Some(profile) {
            return Err(AppError::new(ErrorCode::NotFound, "原场次不存在"));
        }
        let aggregate = self.load_match(match_id)?;
        let context = crate::domain::practice::freeze(&aggregate, source)
            .map_err(|e| AppError::new(ErrorCode::PreconditionFailed, e))?;
        self.conn.execute("INSERT INTO practice_records (id,profile_id,match_id,source_turn_id,frozen_context_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?6)",params![id.to_string(),profile.to_string(),match_id.to_string(),source.to_string(),serde_json::to_string(&context).map_err(|_|internal())?,now]).map_err(map_sql)?;
        self.load_practice(profile, id)
    }

    pub fn load_practice(&self, profile: Uuid, id: Uuid) -> Result<PracticeRecord, AppError> {
        let (match_id,source,context,text,feedback,revision)=self.conn.query_row("SELECT match_id,source_turn_id,frozen_context_json,rewritten_text,feedback_json,revision FROM practice_records WHERE id=?1 AND profile_id=?2",params![id.to_string(),profile.to_string()],|row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,Option<String>>(4)?,row.get::<_,u64>(5)?))).map_err(super::map_missing("练习不存在"))?;
        Ok(PracticeRecord {
            id,
            match_id: Uuid::parse_str(&match_id).map_err(|_| internal())?,
            source_turn_id: Uuid::parse_str(&source).map_err(|_| internal())?,
            context: serde_json::from_str(&context).map_err(|_| internal())?,
            rewritten_text: text,
            feedback,
            revision,
        })
    }

    pub fn save_practice(
        &self,
        profile: Uuid,
        id: Uuid,
        text: &str,
        expected_revision: u64,
        feedback: Option<&str>,
        now: &str,
    ) -> Result<PracticeRecord, AppError> {
        require_utc_timestamp(now)?;
        let record = self.load_practice(profile, id)?;
        if crate::domain::text_units::count_effective_units(text) > record.context.word_cap {
            return Err(AppError::new(
                ErrorCode::InvalidArgument,
                "重练稿超过原字数上限",
            ));
        }
        if record.revision != expected_revision {
            return Err(AppError::new(
                ErrorCode::Conflict,
                "练习版本已更新，请重新读取",
            ));
        }
        let n=self.conn.execute("UPDATE practice_records SET rewritten_text=?3,feedback_json=?4,revision=revision+1,updated_at=?5 WHERE id=?1 AND profile_id=?2 AND revision=?6",params![id.to_string(),profile.to_string(),text,feedback,now,expected_revision]).map_err(map_sql)?;
        if n != 1 {
            return Err(AppError::new(ErrorCode::Conflict, "练习版本已更新"));
        }
        self.load_practice(profile, id)
    }
    pub fn save_draft(
        &self,
        profile: Uuid,
        key: &DraftKey,
        revision: u64,
        payload: &DraftPayload,
        now: &str,
    ) -> Result<bool, AppError> {
        require_utc_timestamp(now)?;
        if self.match_owner(key.match_id)? != Some(profile) {
            return Err(AppError::new(ErrorCode::NotFound, "草稿所属场次不存在"));
        }
        if revision == 0
            || revision > i64::MAX as u64
            || payload.text.len() > 1_000_000
            || payload.events.len() > 100_000
        {
            return Err(AppError::new(
                ErrorCode::InvalidArgument,
                "草稿大小或版本无效",
            ));
        }
        let aggregate = self.load_match(key.match_id)?;
        if aggregate.mode == crate::domain::ruleset::MatchMode::Scoring
            && aggregate.user_side != Some(key.slot.side())
        {
            return Err(AppError::new(
                ErrorCode::PreconditionFailed,
                "评分模式只保存本队草稿",
            ));
        }
        if key.window_id.is_empty() || aggregate.control_window_id() != key.window_id {
            return Ok(false);
        }
        if let Some(session) = aggregate.schedule.get(aggregate.cursor).and_then(|event| {
            aggregate
                .cross_exam_sessions
                .iter()
                .find(|session| session.event_id == event.id)
        }) {
            if key.field_kind == DraftKind::Question
                && !matches!(
                    session.phase,
                    crate::domain::events::CrossExamSessionPhase::Selecting
                        | crate::domain::events::CrossExamSessionPhase::QuestionPending
                )
            {
                return Ok(false);
            }
        }
        let json = serde_json::to_string(payload).map_err(|_| internal())?;
        let changed = self.conn.execute(
            "INSERT INTO draft_records (profile_id, match_id, window_id, slot, field_kind, revision, payload_json, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(profile_id, match_id, window_id, slot, field_kind) DO UPDATE SET revision=excluded.revision, payload_json=excluded.payload_json, updated_at=excluded.updated_at
             WHERE draft_records.closed=0 AND draft_records.revision < excluded.revision",
            params![profile.to_string(), key.match_id.to_string(), key.window_id, enum_name(&key.slot)?, key.field_kind.as_str(), revision as i64, json, now],
        ).map_err(map_sql)?;
        Ok(changed == 1)
    }

    pub fn load_draft(
        &self,
        profile: Uuid,
        key: &DraftKey,
    ) -> Result<Option<SavedDraft>, AppError> {
        if self.match_owner(key.match_id)? != Some(profile) {
            return Ok(None);
        }
        let row = self.conn.query_row(
            "SELECT revision, payload_json FROM draft_records WHERE profile_id=?1 AND match_id=?2 AND window_id=?3 AND slot=?4 AND field_kind=?5 AND closed=0",
            params![profile.to_string(), key.match_id.to_string(), key.window_id, enum_name(&key.slot)?, key.field_kind.as_str()],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
        ).optional().map_err(map_sql)?;
        row.map(|(revision, json)| {
            let (payload, parsed) = match serde_json::from_str::<DraftPayload>(&json) {
                Ok(payload) => (payload, true),
                Err(_) => {
                    let value: serde_json::Value =
                        serde_json::from_str(&json).map_err(|_| internal())?;
                    let text = value["text"].as_str().ok_or_else(internal)?.to_owned();
                    (
                        DraftPayload {
                            text,
                            events: Vec::new(),
                            claimed: ClaimedMetrics {
                                input_elapsed_ms: 0,
                                forward_typed_units: 0,
                                effective_positive_units: 0,
                                reverse_units: 0,
                                reverse_actions: 0,
                                pause_ms: 0,
                                focus_loss_ms: 0,
                                paste_units: 0,
                            },
                        },
                        false,
                    )
                }
            };
            let telemetry_trusted = parsed && payload.trusted();
            Ok(SavedDraft {
                key: key.clone(),
                revision,
                payload,
                telemetry_trusted,
            })
        })
        .transpose()
    }

    pub fn discard_draft(&self, profile: Uuid, key: &DraftKey, now: &str) -> Result<(), AppError> {
        require_utc_timestamp(now)?;
        self.conn.execute("UPDATE draft_records SET closed=1, updated_at=?6 WHERE profile_id=?1 AND match_id=?2 AND window_id=?3 AND slot=?4 AND field_kind=?5", params![profile.to_string(), key.match_id.to_string(), key.window_id, enum_name(&key.slot)?, key.field_kind.as_str(), now]).map_err(map_sql)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::match_engine::play_constructed_match;
    #[test]
    fn practice_versions_and_feedback_never_modify_the_original_match_or_report() {
        let store = SqliteStore::open(":memory:").unwrap();
        let profile = Uuid::from_u128(501);
        let now = "2026-10-12T00:00:00Z";
        store.create_profile(profile, "练习隔离", now).unwrap();
        let mut a = play_constructed_match();
        a.transcripts[0].controller = Some(crate::domain::events::Controller::Human);
        store.save_match(profile, &a, now).unwrap();
        let before = serde_json::to_string(&store.load_match(a.id).unwrap().to_snapshot()).unwrap();
        let id = Uuid::from_u128(502);
        let practice = store
            .create_practice(profile, a.id, a.transcripts[0].id, id, now)
            .unwrap();
        assert!(practice.context.before_turns.is_empty());
        let saved = store
            .save_practice(profile, id, "新练习稿", 0, None, now)
            .unwrap();
        assert!(store
            .save_practice(profile, id, "过期反馈稿", 0, Some("过期评语"), now)
            .is_err());
        store
            .save_practice(
                profile,
                id,
                &saved.rewritten_text,
                saved.revision,
                Some("独立练习评语"),
                now,
            )
            .unwrap();
        assert_eq!(
            before,
            serde_json::to_string(&store.load_match(a.id).unwrap().to_snapshot()).unwrap()
        );
        assert!(store.load_report(a.id).unwrap().is_none());
        assert!(store.load_practice(Uuid::from_u128(503), id).is_err());
    }

    #[test]
    fn draft_versions_are_monotonic_and_closed_windows_cannot_resurrect() {
        let store = SqliteStore::open(":memory:").unwrap();
        let profile = Uuid::from_u128(101);
        let now = "2026-10-12T00:00:00Z";
        store.create_profile(profile, "测试", now).unwrap();
        let mut aggregate = play_constructed_match();
        aggregate.phase = crate::domain::match_engine::MatchPhase::InProgress;
        aggregate.cursor = 0;
        store.save_match(profile, &aggregate, now).unwrap();
        let key = DraftKey {
            match_id: aggregate.id,
            window_id: aggregate.control_window_id(),
            slot: SlotId::Aff1,
            field_kind: DraftKind::Speech,
        };
        let payload = DraftPayload {
            text: "保存但不公开".to_owned(),
            events: vec![],
            claimed: ClaimedMetrics {
                input_elapsed_ms: 3000,
                forward_typed_units: 0,
                effective_positive_units: 0,
                reverse_units: 0,
                reverse_actions: 0,
                pause_ms: 0,
                focus_loss_ms: 0,
                paste_units: 0,
            },
        };
        assert!(store.save_draft(profile, &key, 2, &payload, now).unwrap());
        assert!(!store.save_draft(profile, &key, 1, &payload, now).unwrap());
        let restored = store.load_draft(profile, &key).unwrap().unwrap();
        assert_eq!(restored.payload.text, payload.text);
        assert!(!restored.telemetry_trusted);
        store
            .conn
            .execute(
                "UPDATE draft_records SET payload_json=?1",
                [r#"{"text":"统计损坏但文本仍在","events":"invalid","claimed":null}"#],
            )
            .unwrap();
        let damaged = store.load_draft(profile, &key).unwrap().unwrap();
        assert_eq!(damaged.payload.text, "统计损坏但文本仍在");
        assert!(!damaged.telemetry_trusted);
        assert_eq!(
            store.load_match(aggregate.id).unwrap().transcripts,
            aggregate.transcripts
        );
        store.discard_draft(profile, &key, now).unwrap();
        assert!(!store.save_draft(profile, &key, 3, &payload, now).unwrap());
        assert!(store.load_draft(profile, &key).unwrap().is_none());
        assert!(store
            .load_draft(Uuid::from_u128(102), &key)
            .unwrap()
            .is_none());
    }
}
