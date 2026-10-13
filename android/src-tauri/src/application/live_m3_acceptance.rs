//! M3 live acceptance: real per-stage scoring, match review, judgement, and report recovery.
//!
//! `cargo test --lib real_m3_scoring_review_and_recovery -- --ignored --nocapture`

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use uuid::Uuid;

    use crate::application::desk::{Desk, RuleSetup};
    use crate::domain::ruleset::{four_on_four_template, MatchMode, Side, SlotId, StageKind};
    use crate::domain::visibility::Difficulty;
    use crate::infrastructure::ai::ModelConfig;
    use crate::infrastructure::credentials::{KeyringSecretStore, SecretStore};
    use crate::infrastructure::db::{AppSettings, SqliteStore};

    #[test]
    #[ignore = "requires DeepSeek API key in Windows Credential Manager"]
    fn real_m3_scoring_review_and_recovery() {
        KeyringSecretStore::app()
            .test_api_key()
            .expect("先到设置页保存 DeepSeek API Key");

        let mut ruleset = four_on_four_template();
        let mut affirmative = ruleset.stages[0].clone();
        affirmative.id = "aff-opening".to_owned();
        affirmative.name = "正方一辩陈词".to_owned();
        affirmative.speakers = vec![SlotId::Aff1];
        affirmative.speaker_durations = vec![90];
        affirmative.duration_seconds = 90;
        affirmative.kind = StageKind::Opening;
        let mut negative = affirmative.clone();
        negative.id = "neg-opening".to_owned();
        negative.name = "反方一辩陈词".to_owned();
        negative.speakers = vec![SlotId::Neg1];
        let mut affirmative_reply = affirmative.clone();
        affirmative_reply.id = "aff-reply".to_owned();
        affirmative_reply.name = "正方一辩回应".to_owned();
        affirmative_reply.kind = StageKind::Summary;
        ruleset.ruleset_name = "M3真实评分最小赛制".to_owned();
        ruleset.source_text = "正方陈词、反方回应、正方总结各90秒。".to_owned();
        ruleset.stages = vec![affirmative, negative, affirmative_reply];

        let mut desk = Desk::new();
        desk.profile_id = Some(Uuid::from_u128(0x_0000_0000_00F3_0003));
        desk.apply_recognized_rules(
            ruleset,
            "正方陈词、反方回应、正方总结各90秒。",
            100,
            RuleSetup {
                mode: MatchMode::Training,
                topic: "人工智能应当受到限制".to_owned(),
                materials: "高风险用途需要行政许可，并保留人工复核。".to_owned(),
                user_side: Some(Side::Affirmative),
                difficulty: Difficulty::Standard,
            },
        )
        .expect("rules");
        desk.confirm_rules().expect("confirm");
        // Keep prematch setup deterministic; this acceptance targets the live scoring pipeline.
        desk.analyze().expect("offline analysis");
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .expect("ready");
        desk.start().expect("start");
        desk.submit(
            "我方主张限制高风险人工智能用途。材料明确要求行政许可与人工复核，这说明事前治理能够防止医疗、交通等场景出现不可逆伤害。限制应按风险分级，不是全面禁止，因此兼顾安全与创新。"
                .to_owned(),
            35_000,
            2_000,
            3,
            1,
            false,
        )
        .expect("human opening");
        desk.submit(
            "反方认为应针对具体违法行为治理，而不是限制通用技术。过宽许可可能抬高中小企业成本，现有责任、审计与保险制度也能处理风险。"
                .to_owned(),
            24_000,
            0,
            0,
            0,
            false,
        )
        .expect("ai teammate context");
        desk.submit(
            "回应反方：责任和保险只能在损害发生后补偿，无法挽回医疗误判造成的生命损失；而材料中的许可与人工复核正是可执行的事前机制。我方限制对象始终是高风险用途，并未扩大到普通研发。"
                .to_owned(),
            31_000,
            1_500,
            2,
            1,
            false,
        )
        .expect("human reply");

        let mut config = ModelConfig::from_settings(&AppSettings::default());
        config.timeout_ms = 300_000;
        config.reasoning_effort = "high".to_owned();
        desk.model_config = config;
        desk.live = true;
        desk.finish(true).expect("live scoring");

        let report = desk.report.clone().expect("report");
        assert_eq!(report.scoring_status, "ready", "{}", desk.message);
        assert_eq!(report.stage_evaluations.len(), 2);
        assert_eq!(report.dimensions.len(), 7);
        assert!(!report.review_summary.trim().is_empty());
        assert!(!report.team_summary.trim().is_empty());
        assert!(report.semantic > 0.0);
        assert!(report.personal >= report.program);
        assert!(matches!(
            report.team_winner.as_str(),
            "affirmative" | "negative" | "tie"
        ));
        let transcripts = &desk.aggregate.as_ref().unwrap().transcripts;
        let human_ids = [transcripts[0].id.to_string(), transcripts[2].id.to_string()];
        assert_eq!(
            report
                .stage_evaluations
                .iter()
                .map(|stage| stage.transcript_id.clone())
                .collect::<Vec<_>>(),
            human_ids
        );
        assert!(!report
            .stage_evaluations
            .iter()
            .any(|stage| stage.transcript_id == transcripts[1].id.to_string()));
        assert!(report
            .dimensions
            .iter()
            .all(|dimension| dimension.transcript_id == human_ids[0]));

        let db_path = temp_db_path();
        let store = SqliteStore::open(&db_path).expect("open temp db");
        let profile = desk.profile_id.unwrap();
        store
            .create_profile(profile, "M3真实验收", "2026-09-24T00:00:00Z")
            .expect("profile");
        let aggregate = desk.aggregate.as_ref().unwrap();
        store
            .save_match(profile, aggregate, "2026-09-24T00:00:01Z")
            .expect("save match");
        store
            .save_report(aggregate.id, &report, "2026-09-24T00:00:01Z")
            .expect("save report");
        assert_eq!(
            store.load_report(aggregate.id).unwrap(),
            Some(report.clone())
        );
        drop(store);
        let _ = fs::remove_file(&db_path);

        let artifact = artifact_path();
        fs::write(
            &artifact,
            format!(
                "# M3 真实 DeepSeek 评分验收\n\n- 真人评分段数：{}\n- 七维评分：{}\n- 语义分：{}\n- 程序分：{}\n- 个人总分：{}\n- 团队结果：{}\n- 报告 SQLite 往返：通过\n- 模型：deepseek-flash / high\n",
                report.stage_evaluations.len(),
                report.dimensions.len(),
                report.semantic,
                report.program,
                report.personal,
                report.team_winner,
            ),
        )
        .expect("write artifact");
        eprintln!("{}", fs::read_to_string(&artifact).unwrap());
    }

    fn temp_db_path() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        path.push(format!(
            "debate-m3-live-{}-{nonce}.sqlite",
            std::process::id()
        ));
        path
    }

    fn artifact_path() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("target");
        path.push("m3-scoring-live.md");
        path
    }
}
