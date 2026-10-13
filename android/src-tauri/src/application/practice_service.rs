use crate::infrastructure::{ai::AiContext, db::sqlite::experience::PracticeRecord};

pub fn feedback_context(record: &PracticeRecord) -> AiContext {
    AiContext {
        topic:record.context.topic.clone(),materials:record.context.materials.clone(),rules_text:record.context.task.clone(),
        own_strategy:format!("这是独立练习，不是原比赛评分。score_only_transcript_id={}\nword_cap={}\nrewritten_text={}\n只评价重练稿并给具体改进，不推断原比赛胜负，不引用未来正文。",record.id,record.context.word_cap,record.rewritten_text),
        public_transcript:record.context.before_turns.iter().map(|(id,slot,text)|format!("{id}: {slot:?}: {text}")).collect(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "paid: manually request one practice review using the same 5-CNY ledger"]
    fn real_practice_feedback_preserves_completed_match() {
        assert!(std::env::var_os("DEBATE_ACCEPTANCE_LEDGER").is_some());
        use crate::infrastructure::{
            ai::{DeepSeekTransport, Gateway, ModelConfig, ThreadSleep, UreqHttp},
            credentials::{KeyringSecretStore, SecretStore},
            db::SqliteStore,
        };
        let dir = std::path::PathBuf::from(
            std::env::var_os("DEBATE_ACCEPTANCE_DATA_DIR").expect("isolated directory"),
        );
        let store = SqliteStore::open(dir.join("mixed.sqlite")).unwrap();
        let profile = uuid::Uuid::from_u128(0xF51002);
        let match_id = store.recent_matches(profile, 1).unwrap()[0].id;
        let before = store.load_match(match_id).unwrap();
        assert_eq!(
            before.phase,
            crate::domain::match_engine::MatchPhase::ReportReady
        );
        let report = store.load_report(match_id).unwrap();
        let turn = before
            .transcripts
            .iter()
            .find(|t| t.controller == Some(crate::domain::events::Controller::Human))
            .unwrap();
        let id = uuid::Uuid::from_u128(0xF51101);
        let now = "2026-10-12T00:00:00Z";
        let practice = store
            .create_practice(profile, match_id, turn.id, id, now)
            .unwrap();
        let practice = store
            .save_practice(profile, id, &practice.context.original_text, 0, None, now)
            .unwrap();
        let mut config = ModelConfig::from_settings(&store.load_settings().unwrap());
        config.timeout_ms = 120_000;
        let transport = DeepSeekTransport::new(
            UreqHttp,
            config.clone(),
            KeyringSecretStore::app().read_api_key().unwrap(),
        );
        let feedback = Gateway::new(transport, ThreadSleep, config)
            .score_stage(
                &super::feedback_context(&practice),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert!(feedback.transcript_ids.contains(&id));
        store
            .save_practice(
                profile,
                id,
                &practice.rewritten_text,
                practice.revision,
                Some(&serde_json::to_string(&feedback).unwrap()),
                now,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(before.to_snapshot()).unwrap(),
            serde_json::to_value(store.load_match(match_id).unwrap().to_snapshot()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::to_value(store.load_report(match_id).unwrap()).unwrap()
        );
        eprintln!("[EXP] explicit real practice review passed; original report and match snapshot unchanged");
    }
}
