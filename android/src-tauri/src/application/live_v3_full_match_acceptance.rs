//! Full V3 live acceptance with the real DeepSeek transport and real SQLite.
//!
//! `cargo test --lib real_v3_full_match -- --ignored --nocapture`
//! `cargo test --lib real_word_budget_full_match -- --ignored --nocapture`

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    use uuid::Uuid;

    use crate::application::ai_turn::generate_turn_text;
    use crate::application::cross_exam_service::{CrossExamService, HumanAnswerMetrics};
    use crate::application::desk::{parse_rules_with, Desk, RuleSetup};
    use crate::application::live_acceptance_support::retry_unavailable;
    use crate::application::model_settings::{
        ConnectionProbe, DeepSeekConnectionProbe, ProbeFault,
    };
    use crate::application::word_budget_acceptance::{WordAccounting, MATERIALS, RULES, TOPIC};
    use crate::domain::context_builder::ContextBuilder;
    use crate::domain::match_engine::{MatchPhase, MatchResourceMode};
    use crate::domain::ruleset::{MatchMode, Side, SlotId, StageKind};
    use crate::domain::text_units::count_effective_units;
    use crate::domain::visibility::Difficulty;
    use crate::infrastructure::ai::{
        AiContext, DeepSeekTransport, Gateway, ModelConfig, ThreadSleep, UreqHttp,
    };
    use crate::infrastructure::credentials::{KeyringSecretStore, SecretStore};
    use crate::infrastructure::db::{AppSettings, SqliteStore};

    #[test]
    #[ignore = "requires DeepSeek API key in Windows Credential Manager"]
    fn real_v3_rule_interpretation_only() {
        let key = KeyringSecretStore::app()
            .read_api_key()
            .expect("先到设置页保存并验证 DeepSeek API Key");
        let mut config = ModelConfig::from_settings(&AppSettings::default());
        config.timeout_ms = 180_000;
        config.reasoning_effort = "high".to_owned();
        probe_with_retry(&config, &key).expect("DeepSeek connection probe");
        let gateway = Gateway::new(
            DeepSeekTransport::new(UreqHttp, config.clone(), key),
            ThreadSleep,
            config,
        );
        let result = gateway
            .parse_rules(
                &AiContext {
                    topic: TOPIC.to_owned(),
                    rules_text: RULES.to_owned(),
                    materials: MATERIALS.to_owned(),
                    ..AiContext::default()
                },
                &AtomicBool::new(false),
            )
            .expect("real rule interpretation");
        assert!(!result.ruleset.stages.is_empty());
    }

    #[test]
    #[ignore = "requires DeepSeek API key in Windows Credential Manager"]
    fn real_v3_prematch_analysis_only() {
        KeyringSecretStore::app()
            .test_api_key()
            .expect("先到设置页保存并验证 DeepSeek API Key");
        let mut config = ModelConfig::from_settings(&AppSettings::default());
        config.timeout_ms = 180_000;
        config.reasoning_effort = "high".to_owned();
        let mut desk = Desk::new();
        desk.live = true;
        desk.model_config = config;
        desk.open_template(
            MatchMode::Training,
            TOPIC.to_owned(),
            MATERIALS.to_owned(),
            Some(Side::Affirmative),
            &[],
            Difficulty::Contest,
        )
        .expect("template");
        desk.confirm_rules().expect("confirm");
        desk.analyze().expect("real prematch analysis");
        assert_eq!(desk.role_packets_ref().map_or(0, |items| items.len()), 8);
    }

    #[test]
    #[ignore = "requires DeepSeek API key in Windows Credential Manager and runs a full match"]
    fn real_v3_full_match() {
        run_full_match(MatchResourceMode::Time);
    }

    #[test]
    #[ignore = "requires DeepSeek API key in Windows Credential Manager and runs a full word-budget match"]
    fn real_word_budget_full_match() {
        run_full_match(MatchResourceMode::Words);
    }

    fn run_full_match(metering_mode: MatchResourceMode) {
        let key = KeyringSecretStore::app()
            .read_api_key()
            .expect("先到设置页保存并验证 DeepSeek API Key");
        assert!(!key.is_empty());

        let mut config = ModelConfig::from_settings(&AppSettings::default());
        config.timeout_ms = 300_000;
        config.reasoning_effort = "high".to_owned();
        probe_with_retry(&config, &key).expect("DeepSeek connection probe");
        let gateway = Gateway::new(
            DeepSeekTransport::new(UreqHttp, config.clone(), key),
            ThreadSleep,
            config.clone(),
        );
        let cancel = AtomicBool::new(false);
        let mut timings = Vec::new();

        let parse_context = AiContext {
            topic: TOPIC.to_owned(),
            rules_text: RULES.to_owned(),
            materials: MATERIALS.to_owned(),
            ..AiContext::default()
        };
        let started = Instant::now();
        let interpreted = retry_unavailable("V3 live rule interpretation", || {
            gateway
                .parse_rules(&parse_context, &cancel)
                .map_err(|error| error.to_string())
        })
        .expect("real rule interpretation");
        timings.push(("赛制解释".to_owned(), started.elapsed().as_millis() as u64));
        eprintln!(
            "[V3] 赛制解释完成：{} 个模型识别环节",
            interpreted.ruleset.stages.len()
        );
        assert!(!interpreted.ruleset.stages.is_empty());

        // The deterministic layer is authoritative for this known ruleset;
        // model interpretation is advisory and must never collapse the plan.
        let (ruleset, confidence) = parse_rules_with(true, config.clone(), RULES, TOPIC, MATERIALS)
            .expect("authoritative parse");
        assert_eq!(ruleset.stages.len(), 13);

        let profile_id = Uuid::from_u128(0x0000_0000_00F4_0004);
        let mut desk = Desk::new();
        desk.profile_id = Some(profile_id);
        desk.live = true;
        desk.model_config = config.clone();
        desk.apply_recognized_rules(
            ruleset,
            RULES,
            confidence,
            RuleSetup {
                mode: MatchMode::Scoring,
                topic: TOPIC.to_owned(),
                materials: MATERIALS.to_owned(),
                user_side: Some(Side::Affirmative),
                difficulty: Difficulty::Contest,
            },
        )
        .expect("apply V3 rules");
        {
            let aggregate = desk.aggregate.as_mut().expect("aggregate");
            aggregate.metering_mode = metering_mode;
            if metering_mode == MatchResourceMode::Time {
                aggregate.word_budgets = None;
            }
        }
        assert_eq!(desk.view().schedule.len(), 13);
        assert_eq!(desk.view().affirmative_main_ms, 18 * 60 * 1_000);
        assert_eq!(desk.view().negative_main_ms, 18 * 60 * 1_000);
        desk.confirm_rules().expect("confirm rules");

        let started = Instant::now();
        retry_unavailable("V3 live prematch analysis", || desk.analyze())
            .expect("real topic and role analysis");
        timings.push((
            "拆题与八辩任务".to_owned(),
            started.elapsed().as_millis() as u64,
        ));
        assert_eq!(desk.role_packets_ref().map_or(0, |items| items.len()), 8);
        eprintln!("[V3] 拆题、双方策略和八辩任务完成");
        for _ in 0..2 {
            let passage = desk.passage();
            desk.accept_calibration(&passage, 60_000, false, &[])
                .expect("two-round typing calibration");
        }
        assert!(desk.view().baseline_cpm > 0.0);
        // AFF_1 is an automated human-seat proxy so the live personal scoring
        // pipeline is exercised; every submitted text still comes from DeepSeek.
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .expect("lineup");
        desk.start().expect("start");

        let db_path = temp_db_path();
        let store = SqliteStore::open(&db_path).expect("open acceptance db");
        store
            .create_profile(profile_id, "V3真实全场验收", "2026-10-03T00:00:00Z")
            .expect("create acceptance profile");

        let mut visited_stages = Vec::new();
        let mut cross_pairs = HashSet::new();
        let mut cross_participants = HashSet::new();
        let mut cross_count = 0_u32;
        let mut interrupted_count = 0_u32;
        let mut free_rounds = 0_u32;
        let mut free_affirmative = HashSet::new();
        let mut free_negative = HashSet::new();
        let mut recovered = false;
        let mut free_recovered = false;
        let mut cross = CrossExamService::default();

        while desk.aggregate.as_ref().expect("aggregate").cursor
            < desk.aggregate.as_ref().expect("aggregate").schedule.len()
        {
            let event = desk
                .aggregate
                .as_ref()
                .and_then(|state| state.schedule.get(state.cursor).cloned())
                .expect("current event");
            if visited_stages.last() != Some(&event.stage_id) {
                visited_stages.push(event.stage_id.clone());
                eprintln!(
                    "[V3] 环节 {}/13 开始：{}",
                    visited_stages.len(),
                    event.stage_id
                );
            }

            match event.kind {
                StageKind::CrossExam => {
                    let request = cross.selection_request(&desk).expect("selection request");
                    let started = Instant::now();
                    let selection = retry_unavailable("V3 live role selection", || {
                        gateway
                            .select_cross_exam(&request.context, &cancel)
                            .map_err(|error| error.to_string())
                    })
                    .expect("AI selects cross-exam roles");
                    timings.push((
                        format!("质询派位-{}", cross_count + 1),
                        started.elapsed().as_millis() as u64,
                    ));
                    cross
                        .apply_role_selection(
                            &mut desk,
                            selection.questioner,
                            selection.respondent,
                            selection.selection_summary,
                        )
                        .expect("apply AI roles");
                    cross_pairs.insert((selection.questioner, selection.respondent));
                    cross_participants.insert(selection.questioner);
                    cross_participants.insert(selection.respondent);
                    let before = WordAccounting::capture(&desk);

                    let question_request = cross
                        .generation_request(&desk, false)
                        .expect("question request");
                    let question = generate_live(
                        &desk,
                        &question_request.context,
                        question_request.expected_slot,
                        question_request.word_cap,
                        &mut timings,
                        format!("质询问题-{}", cross_count + 1),
                    );
                    let question_ms = acceptance_input_ms(&desk, &question, metering_mode);
                    cross
                        .submit_question(&mut desk, question.clone(), question_ms)
                        .expect("submit question");

                    let answer_request = cross
                        .generation_request(&desk, true)
                        .expect("answer request");
                    let answer = generate_live(
                        &desk,
                        &answer_request.context,
                        answer_request.expected_slot,
                        answer_request.word_cap,
                        &mut timings,
                        format!("质询回答-{}", cross_count + 1),
                    );
                    cross
                        .prepare_answer_with_metrics(
                            &desk,
                            answer.clone(),
                            (metering_mode == MatchResourceMode::Words).then_some(
                                HumanAnswerMetrics {
                                    input_ms: 45 * 60 * 1_000,
                                    ..HumanAnswerMetrics::default()
                                },
                            ),
                        )
                        .expect("prepare answer");
                    if cross_count == 0 {
                        let reveal_units = (count_effective_units(&answer) / 2).max(1);
                        cross.reveal(reveal_units, 1_000).expect("reveal prefix");
                        cross
                            .interrupt_and_commit_with_input(
                                &mut desk,
                                "问方制止回避并结束本轮".to_owned(),
                                None,
                            )
                            .expect("legal interrupt");
                        interrupted_count += 1;
                    } else {
                        cross
                            .reveal(count_effective_units(&answer), 1_000)
                            .expect("reveal answer");
                        cross
                            .complete_and_commit(&mut desk, None)
                            .expect("complete answer");
                    }
                    cross.end_session(&mut desk).expect("end cross session");
                    if metering_mode == MatchResourceMode::Words {
                        before.assert_main_charge(&desk, selection.questioner.side());
                    }
                    cross_count += 1;
                }
                StageKind::FreeDebate => {
                    let affirmative_slots = [SlotId::Aff2, SlotId::Aff3, SlotId::Aff4];
                    let negative_slots = [SlotId::Neg1, SlotId::Neg2, SlotId::Neg3];
                    for round in 0..12_u32 {
                        let side = desk
                            .aggregate
                            .as_ref()
                            .and_then(|state| state.current_free_debate_side())
                            .expect("free-debate side");
                        let side_round = (round / 2) as usize;
                        let slot = match side {
                            Side::Affirmative => {
                                affirmative_slots[side_round % affirmative_slots.len()]
                            }
                            Side::Negative => negative_slots[side_round % negative_slots.len()],
                        };
                        {
                            let aggregate = desk.aggregate.as_mut().expect("aggregate");
                            aggregate.schedule[aggregate.cursor].speaker = slot;
                        }
                        let context = current_turn_context(&desk);
                        let text = generate_live(
                            &desk,
                            &context,
                            slot,
                            desk.view().current_word_cap.min(100),
                            &mut timings,
                            format!("自由辩-{}", round + 1),
                        );
                        let before = desk.view();
                        let before_words = WordAccounting::capture(&desk);
                        desk.submit(
                            text.clone(),
                            acceptance_input_ms(&desk, &text, metering_mode),
                            0,
                            0,
                            0,
                            false,
                        )
                        .expect("submit free-debate turn");
                        let after = desk.view();
                        if metering_mode == MatchResourceMode::Words {
                            before_words.assert_free_charge(&desk, side);
                        }
                        match side {
                            Side::Affirmative => {
                                if metering_mode == MatchResourceMode::Time {
                                    assert!(after.affirmative_ms < before.affirmative_ms);
                                    assert_eq!(after.negative_ms, before.negative_ms);
                                }
                                free_affirmative.insert(slot);
                            }
                            Side::Negative => {
                                if metering_mode == MatchResourceMode::Time {
                                    assert!(after.negative_ms < before.negative_ms);
                                    assert_eq!(after.affirmative_ms, before.affirmative_ms);
                                }
                                free_negative.insert(slot);
                            }
                        }
                        free_rounds += 1;
                        if metering_mode == MatchResourceMode::Words && round == 5 {
                            let before_restore = WordAccounting::capture(&desk);
                            desk.mark_recovery().expect("mark free-debate recovery");
                            let aggregate = desk.aggregate.as_ref().expect("aggregate");
                            store
                                .save_match(profile_id, aggregate, "2026-10-04T00:00:01Z")
                                .expect("save active word free-debate pool");
                            let loaded = store
                                .load_match(aggregate.id)
                                .expect("reload active word free-debate pool");
                            desk.restore_match(loaded, None);
                            before_restore.assert_restored(&desk);
                            free_recovered = true;
                            eprintln!("[V3] 自由辩中途 SQLite 字数额度恢复通过");
                        }
                    }
                    assert!(desk
                        .interrupt_shown("自由辩不可打断".to_owned(), 3)
                        .is_err());
                    desk.waive_free_debate_time().expect("first side waives");
                    if desk
                        .aggregate
                        .as_ref()
                        .and_then(|state| state.schedule.get(state.cursor))
                        .is_some_and(|next| next.kind == StageKind::FreeDebate)
                    {
                        desk.waive_free_debate_time().expect("second side waives");
                    }
                }
                _ => {
                    let before = WordAccounting::capture(&desk);
                    let context = current_turn_context(&desk);
                    let text = generate_live(
                        &desk,
                        &context,
                        event.speaker,
                        desk.view().current_word_cap.min(140),
                        &mut timings,
                        format!("固定发言-{}", event.stage_id),
                    );
                    desk.submit(
                        text.clone(),
                        acceptance_input_ms(&desk, &text, metering_mode),
                        0,
                        0,
                        0,
                        false,
                    )
                    .expect("submit fixed speech");
                    if metering_mode == MatchResourceMode::Words {
                        before.assert_main_charge(&desk, event.speaker.side());
                    }
                }
            }

            if !recovered && desk.view().cursor >= 6 {
                let aggregate = desk.aggregate.as_ref().expect("aggregate");
                let before_transcripts = aggregate.transcripts.len();
                let before_clocks = (
                    aggregate.affirmative_main_remaining_ms,
                    aggregate.negative_main_remaining_ms,
                );
                let before_words = WordAccounting::capture(&desk);
                desk.mark_recovery().expect("mark recovery");
                store
                    .save_match(
                        profile_id,
                        desk.aggregate.as_ref().expect("aggregate"),
                        "2026-10-03T00:00:01Z",
                    )
                    .expect("save recovery snapshot");
                let loaded = store
                    .load_match(desk.aggregate.as_ref().expect("aggregate").id)
                    .expect("reload recovery snapshot");
                desk.restore_match(loaded, None);
                let restored = desk.aggregate.as_ref().expect("restored");
                assert_eq!(restored.phase, MatchPhase::InProgress);
                assert_eq!(restored.transcripts.len(), before_transcripts);
                assert_eq!(
                    (
                        restored.affirmative_main_remaining_ms,
                        restored.negative_main_remaining_ms,
                    ),
                    before_clocks
                );
                assert_eq!(restored.metering_mode, metering_mode);
                if metering_mode == MatchResourceMode::Words {
                    before_words.assert_restored(&desk);
                }
                recovered = true;
                eprintln!("[V3] 中途 SQLite 恢复通过");
            }
        }

        assert_eq!(visited_stages.len(), 13);
        assert_eq!(cross_count, 4);
        assert!(cross_pairs.len() >= 2);
        assert!(cross_participants.len() >= 4);
        assert_eq!(interrupted_count, 1);
        assert_eq!(free_rounds, 12);
        assert!(free_affirmative.len() >= 3);
        assert!(free_negative.len() >= 3);
        assert!(recovered);
        if metering_mode == MatchResourceMode::Words {
            assert!(free_recovered);
        }

        let view = desk.view();
        if metering_mode == MatchResourceMode::Time {
            assert!(view.affirmative_main_ms < 18 * 60 * 1_000);
            assert!(view.negative_main_ms < 18 * 60 * 1_000);
        } else {
            let budgets = desk
                .aggregate
                .as_ref()
                .unwrap()
                .word_budgets
                .as_ref()
                .unwrap();
            assert!(view.affirmative_main_units < budgets.affirmative_total_units);
            assert!(view.negative_main_units < budgets.negative_total_units);
            assert_eq!(view.affirmative_main_ms, 18 * 60 * 1_000);
            assert_eq!(view.negative_main_ms, 18 * 60 * 1_000);
        }
        let free_session = desk
            .aggregate
            .as_ref()
            .and_then(|aggregate| aggregate.free_debate_sessions.last())
            .expect("completed free-debate session");
        assert!(free_session.completed);
        if metering_mode == MatchResourceMode::Time {
            assert_eq!(free_session.affirmative_remaining_ms, 0);
            assert_eq!(free_session.negative_remaining_ms, 0);
        } else {
            assert_eq!(free_session.affirmative_remaining_units, 0);
            assert_eq!(free_session.negative_remaining_units, 0);
            assert_eq!(free_session.affirmative_remaining_ms, 4 * 60 * 1_000);
            assert_eq!(free_session.negative_remaining_ms, 4 * 60 * 1_000);
        }

        let started = Instant::now();
        eprintln!("[V3] 13 个环节完成，开始真实评分");
        desk.finish(true).expect("real V3 scoring");
        timings.push(("整场评分".to_owned(), started.elapsed().as_millis() as u64));
        let report = desk.report.clone().expect("report");
        assert_eq!(report.formula_version, "score-v3");
        assert_eq!(report.scoring_status, "ready");
        assert_eq!(report.dimensions.len(), 7);
        assert!(!report.review_summary.trim().is_empty());
        assert!(!report.team_summary.trim().is_empty());
        assert!(report.personal <= 100.0);

        let aggregate = desk.aggregate.as_ref().expect("aggregate");
        store
            .save_match(profile_id, aggregate, "2026-10-03T00:00:02Z")
            .expect("save completed match");
        store
            .save_report(aggregate.id, &report, "2026-10-03T00:00:02Z")
            .expect("save report");
        assert_eq!(
            store.load_report(aggregate.id).unwrap(),
            Some(report.clone())
        );

        let records = gateway.records();
        assert!(records
            .iter()
            .all(|record| record.model_id == "deepseek-flash"));
        assert!(records
            .iter()
            .all(|record| record.thinking.contains("high")));
        let timing_rows = timings
            .iter()
            .map(|(label, elapsed)| format!("- {label}: {elapsed} ms"))
            .collect::<Vec<_>>()
            .join("\n");
        let artifact = artifact_path(metering_mode);
        let report_name = match metering_mode {
            MatchResourceMode::Time => "v3-full-match-report.json",
            MatchResourceMode::Words => "word-budget-full-match-live.json",
        };
        let transcript_name = match metering_mode {
            MatchResourceMode::Time => "v3-full-match-transcript.md",
            MatchResourceMode::Words => "word-budget-full-match-transcript.md",
        };
        let report_artifact = if metering_mode == MatchResourceMode::Words {
            serde_json::json!({
                "meteringMode": "words",
                "wordBudgets": aggregate.word_budgets,
                "sourceMainRemainingMs": {
                    "affirmative": aggregate.affirmative_main_remaining_ms,
                    "negative": aggregate.negative_main_remaining_ms,
                },
                "crossExamSessions": aggregate.cross_exam_sessions,
                "freeDebateSessions": aggregate.free_debate_sessions,
                "visitedStages": visited_stages,
                "crossExamCount": cross_count,
                "interruptedCount": interrupted_count,
                "freeRounds": free_rounds,
                "sqliteRecoveryVerified": recovered,
                "activeFreePoolRecoveryVerified": free_recovered,
                "inputMsDoesNotChargeBudgetVerified": true,
                "report": report,
                "timingsMs": timings,
            })
        } else {
            serde_json::to_value(&report).expect("serialize report")
        };
        fs::write(
            artifact.with_file_name(report_name),
            serde_json::to_string_pretty(&report_artifact).expect("serialize report"),
        )
        .expect("write report artifact");
        let transcript_rows = aggregate
            .transcripts
            .iter()
            .map(|entry| format!("### {:?}\n\n{}\n", entry.slot, entry.effective_text))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            artifact.with_file_name(transcript_name),
            format!("# 真实 DeepSeek 自动比赛实录\n\n{}\n", transcript_rows),
        )
        .expect("write public transcript artifact");
        let resource_rows = if metering_mode == MatchResourceMode::Words {
            format!(
                "- 资源模式：字数额度\n- input_ms 不扣字数余额：逐笔精确验证通过\n- 主额度剩余：正方 {}、反方 {} 有效单位\n- 自由辩剩余额度：双弃权后均为 0\n- 主额度与自由辩原始时间字段：保留\n- SQLite 字数额度及评分数据恢复：通过\n",
                aggregate.remaining_main_units(Side::Affirmative),
                aggregate.remaining_main_units(Side::Negative),
            )
        } else {
            "- 资源模式：时间\n".to_owned()
        };
        fs::write(
            &artifact,
            format!(
                "# V3 真实 DeepSeek 全场验收\n\n{resource_rows}- 赛制：13 个环节 / 4 个资源池\n- DeepSeek 赛制解释阶段数：{}\n- 多人质询：{} 段，{} 组实际派位，合法打断 {} 次\n- 自由辩：{} 轮，正方 {} 名、反方 {} 名实际出场\n- 中途 SQLite 恢复：通过\n- 评分公式：{}\n- 个人总分：{}\n- 团队结果：{}\n- 模型：deepseek-flash / high\n\n## 脱敏耗时\n{}\n",
                interpreted.ruleset.stages.len(),
                cross_count,
                cross_pairs.len(),
                interrupted_count,
                free_rounds,
                free_affirmative.len(),
                free_negative.len(),
                report.formula_version,
                report.personal,
                report.team_winner,
                timing_rows,
            ),
        )
        .expect("write acceptance artifact");
        eprintln!("{}", fs::read_to_string(&artifact).unwrap());
        drop(store);
        let _ = fs::remove_file(db_path);
    }

    fn current_turn_context(desk: &Desk) -> AiContext {
        let view = desk.view();
        let packets = desk
            .role_packets_ref()
            .map(|items| items.to_vec())
            .unwrap_or_default();
        let packet = ContextBuilder::build(
            desk.aggregate.as_ref().expect("aggregate"),
            desk.strategies_ref(),
            &packets,
            desk.difficulty,
            view.remaining_ms,
            view.affirmative_ms.max(view.negative_ms),
        )
        .expect("turn context");
        AiContext {
            topic: packet.topic,
            rules_text: packet.rules_summary,
            materials: packet.materials,
            own_strategy: format!(
                "{}\nrole={}\nduty={}\nmain={}\nbackup={}\nfloor={}",
                packet.own_strategy,
                packet.role_packet.identity,
                packet.role_packet.duty,
                packet.role_packet.main_argument,
                packet.role_packet.backup_rebuttal,
                packet.role_packet.defense_floor,
            ),
            opponent_strategy: String::new(),
            public_transcript: packet
                .public_turns
                .iter()
                .map(|turn| format!("{}: {}", turn.id, turn.text))
                .collect(),
        }
    }

    fn probe_with_retry(config: &ModelConfig, key: &str) -> Result<(), ProbeFault> {
        const MAX_ATTEMPTS: u8 = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            match DeepSeekConnectionProbe.probe(config, key) {
                Err(
                    error @ (ProbeFault::Network | ProbeFault::Timeout | ProbeFault::Incompatible),
                ) if attempt < MAX_ATTEMPTS => {
                    eprintln!(
                        "DeepSeek probe temporary failure {error:?} on attempt {attempt}/{MAX_ATTEMPTS}; retrying"
                    );
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
                result => return result,
            }
        }
        unreachable!("the final probe retry always returns")
    }

    fn generate_live(
        desk: &Desk,
        context: &AiContext,
        slot: SlotId,
        word_cap: u32,
        timings: &mut Vec<(String, u64)>,
        label: String,
    ) -> String {
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let text = retry_unavailable(&label, || {
            generate_turn_text(
                true,
                desk.model_config.clone(),
                context,
                slot,
                word_cap.max(1),
                &cancel,
                |_| {},
            )
        })
        .unwrap_or_else(|error| panic!("{label} failed: {error}"));
        let elapsed_ms = started.elapsed().as_millis() as u64;
        eprintln!(
            "[V3] {label} 完成：{} 有效单位，{elapsed_ms} ms",
            count_effective_units(&text)
        );
        timings.push((label, elapsed_ms));
        text
    }

    fn temp_db_path() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        path.push(format!(
            "debate-v3-full-live-{}-{nonce}.sqlite",
            std::process::id()
        ));
        path
    }

    fn acceptance_input_ms(desk: &Desk, text: &str, mode: MatchResourceMode) -> u64 {
        match mode {
            // Deliberately exceed every source clock: words alone must determine
            // the charge and this must not cause a time-based advance/timeout.
            MatchResourceMode::Words => 30 * 60 * 1_000,
            MatchResourceMode::Time => desk.speech_display_ms(text).max(1),
        }
    }

    fn artifact_path(mode: MatchResourceMode) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("target");
        path.push(match mode {
            MatchResourceMode::Time => "v3-full-match-live.md",
            MatchResourceMode::Words => "word-budget-full-match-live.md",
        });
        path
    }
}
