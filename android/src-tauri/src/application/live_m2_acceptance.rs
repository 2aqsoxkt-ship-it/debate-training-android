//! M2 live acceptance: free-debate reply + one cross-exam interrupt with real DeepSeek.
//!
//! `cargo test --lib real_m2_free_debate_and_cross_exam -- --ignored --nocapture`

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    use uuid::Uuid;

    use crate::application::ai_turn::generate_turn_text;
    use crate::application::cross_exam_service::CrossExamService;
    use crate::application::desk::Desk;
    use crate::application::free_debate_service::write_twelve_round_acceptance;
    use crate::application::live_acceptance_support::retry_unavailable;
    use crate::domain::context_builder::ContextBuilder;
    use crate::domain::ruleset::{MatchMode, Side, StageKind};
    use crate::domain::text_units::count_effective_units;
    use crate::domain::visibility::Difficulty;
    use crate::infrastructure::ai::{AiContext, ModelConfig};
    use crate::infrastructure::credentials::{KeyringSecretStore, SecretStore};
    use crate::infrastructure::db::AppSettings;

    #[test]
    #[ignore = "requires DeepSeek API key in Windows Credential Manager"]
    fn real_m2_free_debate_and_cross_exam() {
        let key = KeyringSecretStore::app()
            .read_api_key()
            .expect("先到设置页保存 DeepSeek API Key");
        assert!(!key.is_empty());

        let twelve = write_twelve_round_acceptance().expect("12-round harness");
        assert!(twelve.exists());

        let mut desk = Desk::new();
        desk.live = true;
        desk.profile_id = Some(Uuid::from_u128(0x_0000_0000_00F2_0002));
        let mut config = ModelConfig::from_settings(&AppSettings::default());
        config.timeout_ms = 300_000;
        config.reasoning_effort = "high".to_owned();
        desk.model_config = config;

        desk.open_template(
            MatchMode::Training,
            "人工智能应当受到限制".to_owned(),
            "闭卷材料：高风险用途需许可".to_owned(),
            Some(Side::Affirmative),
            &[0, 2],
            Difficulty::Standard,
        )
        .expect("template");
        desk.confirm_rules().expect("confirm");
        desk.analyze().expect("analyze");
        desk.assign_and_ready(Some(Side::Affirmative), &[0, 2])
            .expect("ready");
        desk.start().expect("start");

        desk.submit(
            "正方开场：限制高风险用途是必要护栏。".to_owned(),
            2_000,
            0,
            0,
            0,
            false,
        )
        .expect("opening");

        let mut free_summary = String::new();
        let mut cross_summary = String::new();
        for _ in 0..64 {
            let event = desk
                .aggregate
                .as_ref()
                .and_then(|state| state.schedule.get(state.cursor).cloned());
            let Some(event) = event else { break };

            if event.kind == StageKind::FreeDebate || event.kind == StageKind::CrossExam {
                let view = desk.view();
                let packets = desk
                    .role_packets_ref()
                    .map(|p| p.to_vec())
                    .unwrap_or_default();
                let turn_context = ContextBuilder::build(
                    desk.aggregate.as_ref().unwrap(),
                    desk.strategies_ref(),
                    &packets,
                    desk.difficulty,
                    view.remaining_ms,
                    view.affirmative_ms.max(view.negative_ms),
                )
                .expect("ctx");
                let ai_context = AiContext {
                    topic: turn_context.topic.clone(),
                    rules_text: turn_context.rules_summary.clone(),
                    materials: turn_context.materials.clone(),
                    own_strategy: turn_context.own_strategy.clone(),
                    public_transcript: turn_context
                        .public_turns
                        .iter()
                        .map(|turn| format!("{}: {}", turn.id, turn.text))
                        .collect(),
                    ..AiContext::default()
                };
                let cancel = AtomicBool::new(false);
                eprintln!(
                    "M2 live generation: stage_id={}, kind={:?}, speaker={:?}, word_cap={}",
                    event.stage_id,
                    event.kind,
                    event.speaker,
                    event.word_cap.max(80)
                );
                let text = retry_unavailable("M2 live speech", || {
                    generate_turn_text(
                        true,
                        desk.model_config.clone(),
                        &ai_context,
                        event.speaker,
                        event.word_cap.max(80),
                        &cancel,
                        |_| {},
                    )
                })
                .expect("live speech");

                if event.kind == StageKind::FreeDebate {
                    assert!(
                        text.contains("回应")
                            || text.contains("针对")
                            || text.contains("对方")
                            || count_effective_units(&text) > 20,
                        "自由辩应回应上一轮"
                    );
                    let before = desk.view();
                    let before_aff = before.affirmative_ms;
                    let before_neg = before.negative_ms;
                    desk.submit(
                        text.clone(),
                        desk.speech_display_ms(&text).max(1),
                        0,
                        0,
                        0,
                        false,
                    )
                    .expect("submit free");
                    let after = desk.view();
                    assert!(
                        after.affirmative_ms < before_aff || after.negative_ms < before_neg,
                        "只应扣当前持权方的团队剩余时间"
                    );
                    free_summary = format!(
                        "自由辩一轮：单位{}，正方时钟{}→{}，反方{}→{}",
                        count_effective_units(&text),
                        before_aff,
                        after.affirmative_ms,
                        before_neg,
                        after.negative_ms
                    );
                    let clocks = (after.affirmative_ms, after.negative_ms);
                    let n = desk.aggregate.as_ref().unwrap().transcripts.len();
                    assert!(desk.interrupt_shown(text, 4).is_err());
                    assert_eq!(
                        (desk.view().affirmative_ms, desk.view().negative_ms),
                        clocks
                    );
                    assert_eq!(desk.aggregate.as_ref().unwrap().transcripts.len(), n);
                    break;
                }

                let mut cross = CrossExamService::default();
                cross
                    .submit_question(
                        &mut desk,
                        "请定义你们所说的高风险用途边界。".to_owned(),
                        500,
                    )
                    .expect("question");
                cross
                    .prepare_answer_with_metrics(&desk, text.clone(), None)
                    .expect("prepare");
                let half = (count_effective_units(&text) / 2).max(4);
                let visible = cross.reveal(half, 1_500).expect("reveal");
                assert_eq!(cross.visible_for_interrupt_decision(), visible);
                cross
                    .interrupt_and_commit_with_input(&mut desk, "回避定义".to_owned(), None)
                    .expect("interrupt");
                let view_after = desk.view();
                let last = view_after.transcripts.last().expect("transcript");
                assert!(
                    !last.interrupted_tail.is_empty() || last.effective_text.len() < text.len()
                );
                cross_summary = format!(
                    "质询打断：有效前缀单位{}，尾部已隔离",
                    count_effective_units(&last.effective_text)
                );
                cross.end_session(&mut desk).expect("end cross session");
                continue;
            }

            let _ = desk.submit("占位推进。".to_owned(), 500, 0, 0, 0, false);
        }

        assert!(!free_summary.is_empty(), "必须完成一轮真实自由辩");
        assert!(!cross_summary.is_empty(), "必须完成一次真实质询打断");

        let md = format!(
            "# M2 真实 DeepSeek 验收摘录\n\n- 12轮 harness: {}\n- {}\n- {}\n- 模型: deepseek-flash / high\n",
            twelve.display(),
            free_summary,
            cross_summary
        );
        let path = artifact_path();
        fs::write(&path, md).expect("write artifact");
        eprintln!("{}", fs::read_to_string(&path).unwrap());
    }

    fn artifact_path() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("target");
        path.push("m2-interaction-live.md");
        path
    }
}
