//! Live acceptance for the complete AI question/answer cross-exam chain.
//!
//! `cargo test --lib real_cross_exam_control_chain -- --ignored --nocapture`

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    use uuid::Uuid;

    use crate::application::ai_turn::generate_turn_text;
    use crate::application::cross_exam_service::CrossExamService;
    use crate::application::desk::Desk;
    use crate::application::live_acceptance_support::{
        advance_to_next_cross_exam, retry_unavailable,
    };
    use crate::application::scoring_service::stage_context;
    use crate::domain::ruleset::{MatchMode, Side};
    use crate::domain::text_units::{count_effective_units, split_effective};
    use crate::domain::visibility::Difficulty;
    use crate::infrastructure::credentials::{KeyringSecretStore, SecretStore};
    use crate::infrastructure::db::{AppSettings, SqliteStore};

    #[test]
    #[ignore = "requires DeepSeek API key in Windows Credential Manager"]
    fn real_cross_exam_control_chain() {
        let key = KeyringSecretStore::app()
            .read_api_key()
            .expect("先到设置页保存 DeepSeek API Key");
        assert!(!key.is_empty());

        let mut interrupted = ready_at_cross(0xCE01);
        let mut cross = CrossExamService::default();
        let question = generate(&mut cross, &interrupted, false);
        cross
            .submit_question(&mut interrupted, question.clone(), 800)
            .expect("submit generated question");
        let answer = generate(&mut cross, &interrupted, true);
        cross
            .prepare_answer_with_metrics(&interrupted, answer.clone(), None)
            .expect("prepare generated answer");
        let shown_units = (count_effective_units(&answer) / 2).max(1);
        let visible = cross.reveal(shown_units, 1_500).expect("reveal prefix");
        let (_, hidden_tail) = split_effective(&answer, shown_units);
        assert!(
            !hidden_tail.is_empty(),
            "live answer must leave a hidden tail"
        );
        cross
            .interrupt_and_commit_with_input(&mut interrupted, "验收打断".to_owned(), None)
            .expect("interrupt commit");

        let aggregate = interrupted.aggregate.as_ref().expect("aggregate");
        let transcript = aggregate.transcripts.last().expect("answer transcript");
        assert_eq!(transcript.effective_text, visible);
        assert_eq!(transcript.response_target_text, question);
        let score_context = stage_context(
            aggregate,
            transcript.id,
            "切题性,论证质量,反驳,证据,逻辑,临场应对,团队协作",
            "未展示尾部不得评分",
        )
        .expect("scoring context");
        assert!(score_context.own_strategy.contains(&question));
        assert!(!score_context.own_strategy.contains(hidden_tail));

        let db_path = acceptance_db_path();
        if db_path.exists() {
            std::fs::remove_file(&db_path).expect("remove stale acceptance db");
        }
        let profile_id = interrupted.profile_id.expect("profile");
        let match_id = aggregate.id;
        {
            let store = SqliteStore::open(&db_path).expect("open acceptance db");
            store
                .create_profile(profile_id, "质询验收", "2026-09-24T14:00:00Z")
                .expect("create acceptance profile");
            store
                .save_match(profile_id, aggregate, "2026-09-24T14:00:00Z")
                .expect("save interrupted match");
        }
        let restored = SqliteStore::open(&db_path)
            .expect("reopen acceptance db")
            .load_match(match_id)
            .expect("restore interrupted match");
        assert_eq!(restored.cross_exam_exchanges.len(), 1);
        let restored_json = serde_json::to_string(&restored.to_snapshot()).expect("snapshot json");
        assert!(!restored_json.contains(hidden_tail));
        std::fs::remove_file(&db_path).expect("remove acceptance db");

        let mut completed = ready_at_cross(0xCE02);
        let mut complete_cross = CrossExamService::default();
        let second_question = generate(&mut complete_cross, &completed, false);
        complete_cross
            .submit_question(&mut completed, second_question, 800)
            .expect("submit second question");
        let second_answer = generate(&mut complete_cross, &completed, true);
        complete_cross
            .prepare_answer_with_metrics(&completed, second_answer.clone(), None)
            .expect("prepare second answer");
        complete_cross
            .reveal(count_effective_units(&second_answer), 3_000)
            .expect("reveal full answer");
        complete_cross
            .complete_and_commit(&mut completed, None)
            .expect("complete answer");
        assert_eq!(
            completed
                .aggregate
                .as_ref()
                .and_then(|state| state.transcripts.last())
                .map(|entry| entry.effective_text.as_str()),
            Some(second_answer.as_str())
        );
        eprintln!(
            "cross-exam live acceptance passed: interrupted_units={}, completed_units={}",
            count_effective_units(&visible),
            count_effective_units(&second_answer)
        );
    }

    fn ready_at_cross(seed: u128) -> Desk {
        let mut desk = Desk::new();
        desk.live = true;
        desk.profile_id = Some(Uuid::from_u128(seed));
        let mut config =
            crate::infrastructure::ai::ModelConfig::from_settings(&AppSettings::default());
        config.timeout_ms = 300_000;
        config.reasoning_effort = "high".to_owned();
        desk.model_config = config;
        desk.open_template(
            MatchMode::Training,
            "人工智能应当受到限制".to_owned(),
            "高风险用途需要许可并保留人工复核。".to_owned(),
            Some(Side::Affirmative),
            &[0],
            Difficulty::Standard,
        )
        .expect("template");
        desk.confirm_rules().expect("confirm rules");
        desk.analyze().expect("analyze");
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .expect("ready");
        desk.start().expect("start");
        advance_to_next_cross_exam(&mut desk).expect("advance to cross exam");
        desk
    }

    fn generate(cross: &mut CrossExamService, desk: &Desk, answer: bool) -> String {
        let request = cross
            .generation_request(desk, answer)
            .expect("cross-exam generation request");
        let cancel = AtomicBool::new(false);
        retry_unavailable("cross-exam generation", || {
            generate_turn_text(
                request.live,
                request.config.clone(),
                &request.context,
                request.expected_slot,
                request.word_cap,
                &cancel,
                |_| {},
            )
        })
        .expect("real DeepSeek cross-exam generation")
    }

    fn acceptance_db_path() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("target");
        path.push("cross-exam-live.sqlite");
        path
    }
}
