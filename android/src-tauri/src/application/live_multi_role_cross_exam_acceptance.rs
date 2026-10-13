//! Live acceptance for a three-round, multi-role cross-exam session.
//!
//! `cargo test --lib real_multi_role_cross_exam_session -- --ignored --nocapture`

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::atomic::AtomicBool;

    use uuid::Uuid;

    use crate::application::ai_turn::generate_turn_text;
    use crate::application::cross_exam_service::CrossExamService;
    use crate::application::desk::{Desk, RuleSetup};
    use crate::application::live_acceptance_support::{
        advance_to_next_cross_exam, retry_unavailable,
    };
    use crate::application::scoring_service::stage_context;
    use crate::domain::events::{CrossExamSessionEndReason, CrossExamTurnKind};
    use crate::domain::ruleset::{four_on_four_template, MatchMode, Side, SlotId};
    use crate::domain::text_units::count_effective_units;
    use crate::domain::visibility::Difficulty;
    use crate::infrastructure::ai::{DeepSeekTransport, Gateway, ThreadSleep, UreqHttp};
    use crate::infrastructure::credentials::{KeyringSecretStore, SecretStore};
    use crate::infrastructure::db::{AppSettings, SqliteStore};

    #[test]
    #[ignore = "requires DeepSeek API key in Windows Credential Manager"]
    fn real_multi_role_cross_exam_session() {
        let key = KeyringSecretStore::app()
            .read_api_key()
            .expect("先到设置页保存 DeepSeek API Key");
        let mut desk = ready_at_cross();
        let mut cross = CrossExamService::default();
        let mut remaining = Vec::new();
        let mut hidden_tail = String::new();

        for round in 1..=3 {
            let request = cross.selection_request(&desk).expect("selection request");
            let selection = Gateway::new(
                DeepSeekTransport::new(UreqHttp, request.config.clone(), key.clone()),
                ThreadSleep,
                request.config,
            )
            .select_cross_exam(&request.context, &AtomicBool::new(false))
            .ok();
            let applied = selection.and_then(|selection| {
                cross
                    .apply_role_selection(
                        &mut desk,
                        selection.questioner,
                        selection.respondent,
                        selection.selection_summary,
                    )
                    .ok()
            });
            if applied.is_none() {
                cross
                    .apply_role_selection(
                        &mut desk,
                        request.fallback_questioner,
                        request.fallback_respondent,
                        "真实模型派位不合法，使用确定性回退".to_owned(),
                    )
                    .expect("fallback selection");
            }

            if round == 2 {
                cross
                    .insert_human_questioner(&mut desk, SlotId::Neg2)
                    .expect("human inserts only the negative questioner");
            }
            let question = generate(&mut cross, &desk, false);
            cross
                .submit_question(&mut desk, question, 500)
                .expect("submit question");
            let answer = generate(&mut cross, &desk, true);
            cross
                .prepare_answer_with_metrics(&desk, answer.clone(), None)
                .expect("prepare answer");
            if round < 3 {
                cross
                    .reveal(count_effective_units(&answer), 2_000)
                    .expect("reveal answer");
                cross
                    .complete_and_commit(&mut desk, None)
                    .expect("complete exchange");
            } else {
                let shown = (count_effective_units(&answer) / 2).max(1);
                cross.reveal(shown, 1_000).expect("reveal prefix");
                hidden_tail = crate::domain::text_units::split_effective(&answer, shown)
                    .1
                    .to_owned();
                cross
                    .interrupt_and_commit_with_input(&mut desk, "验收打断".to_owned(), None)
                    .expect("interrupt third exchange");
            }
            remaining.push(cross.snapshot(&desk).expect("next snapshot").remaining_ms);
        }
        cross.end_session(&mut desk).expect("end session");

        let aggregate = desk.aggregate.as_ref().expect("aggregate");
        assert_eq!(aggregate.cross_exam_exchanges.len(), 3);
        let sides: Vec<_> = aggregate
            .cross_exam_exchanges
            .iter()
            .map(|exchange| exchange.questioning_side.expect("side"))
            .collect();
        assert_eq!(
            sides,
            vec![Side::Affirmative, Side::Negative, Side::Affirmative]
        );
        assert_eq!(aggregate.cross_exam_exchanges[1].questioner, SlotId::Neg2);
        let participants: HashSet<_> = aggregate
            .cross_exam_exchanges
            .iter()
            .flat_map(|exchange| [exchange.questioner, exchange.respondent])
            .collect();
        assert!(participants.len() >= 3);
        assert!(remaining.windows(2).all(|window| window[1] < window[0]));
        assert_eq!(
            aggregate.cross_exam_sessions[0].end_reason,
            Some(CrossExamSessionEndReason::UserEnded)
        );
        let question = aggregate
            .transcripts
            .iter()
            .find(|entry| entry.cross_exam_turn_kind == Some(CrossExamTurnKind::Question))
            .expect("question transcript");
        let answer = aggregate
            .transcripts
            .iter()
            .find(|entry| entry.cross_exam_turn_kind == Some(CrossExamTurnKind::Answer))
            .expect("answer transcript");
        assert!(stage_context(aggregate, question.id, "七维", "公开文本")
            .expect("question scoring")
            .own_strategy
            .contains("cross_exam_turn=question"));
        assert!(stage_context(aggregate, answer.id, "七维", "公开文本")
            .expect("answer scoring")
            .own_strategy
            .contains("cross_exam_turn=answer"));

        let db_path = std::env::temp_dir().join("debate-multi-role-cross-exam.sqlite");
        let _ = std::fs::remove_file(&db_path);
        let profile_id = desk.profile_id.expect("profile");
        let match_id = aggregate.id;
        {
            let store = SqliteStore::open(&db_path).expect("open db");
            store
                .create_profile(profile_id, "多角色质询验收", "2026-09-28T00:00:00Z")
                .expect("profile");
            store
                .save_match(profile_id, aggregate, "2026-09-28T00:00:00Z")
                .expect("save");
        }
        let restored = SqliteStore::open(&db_path)
            .expect("reopen")
            .load_match(match_id)
            .expect("restore");
        assert_eq!(restored.cross_exam_exchanges.len(), 3);
        assert_eq!(
            restored.cross_exam_sessions[0].used_ms,
            aggregate.cross_exam_sessions[0].used_ms
        );
        let json = serde_json::to_string(&restored.to_snapshot()).expect("json");
        assert!(hidden_tail.is_empty() || !json.contains(&hidden_tail));
        let _ = std::fs::remove_file(db_path);
    }

    fn ready_at_cross() -> Desk {
        let mut desk = Desk::new();
        desk.live = true;
        desk.profile_id = Some(Uuid::from_u128(0xCE03));
        let mut config =
            crate::infrastructure::ai::ModelConfig::from_settings(&AppSettings::default());
        config.timeout_ms = 300_000;
        config.reasoning_effort = "high".to_owned();
        desk.model_config = config;
        desk.apply_recognized_rules(
            four_on_four_template(),
            "一辩立论；双方多角色交替质询；自由辩；四辩总结。",
            100,
            RuleSetup {
                mode: MatchMode::Training,
                topic: "人工智能应当受到限制".to_owned(),
                materials: "高风险用途需要许可并保留人工复核。".to_owned(),
                user_side: Some(Side::Affirmative),
                difficulty: Difficulty::Standard,
            },
        )
        .expect("alternating multi-role rules");
        desk.confirm_rules().expect("confirm rules");
        desk.analyze().expect("analyze");
        desk.assign_and_ready(Some(Side::Affirmative), &[0, 5])
            .expect("ready");
        desk.start().expect("start");
        advance_to_next_cross_exam(&mut desk).expect("advance to cross exam");
        desk
    }

    fn generate(cross: &mut CrossExamService, desk: &Desk, answer: bool) -> String {
        let request = cross
            .generation_request(desk, answer)
            .expect("generation request");
        let cancel = AtomicBool::new(false);
        retry_unavailable("multi-role cross-exam generation", || {
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
        .expect("real DeepSeek generation")
    }
}
