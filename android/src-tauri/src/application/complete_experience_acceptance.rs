//! Service-level live acceptance, not a claim of manual human or native GUI use.
use super::word_budget_acceptance::{MATERIALS, RULES, TOPIC};
use crate::application::{
    cross_exam_service::{CrossExamService, HumanAnswerMetrics},
    desk::{parse_rules_with, Desk, RuleSetup},
    AppState,
};
use crate::domain::{
    events::Controller,
    ruleset::{MatchMode, Side, SlotId, StageKind},
    visibility::Difficulty,
};
use crate::infrastructure::{
    ai::{DeepSeekTransport, Gateway, ModelConfig, ThreadSleep, UreqHttp},
    credentials::{KeyringSecretStore, SecretStore},
    db::{AppSettings, SqliteStore},
};
use std::sync::{atomic::AtomicBool, Mutex};

#[test]
#[ignore = "paid: requires the shared 5-CNY ledger and isolated acceptance directory"]
fn real_complete_experience_two_matches_with_one_budget() {
    let ledger = std::env::var_os("DEBATE_ACCEPTANCE_LEDGER").expect("不得无费用保护运行");
    assert!(!ledger.is_empty());
    let dir = std::path::PathBuf::from(
        std::env::var_os("DEBATE_ACCEPTANCE_DATA_DIR").expect("必须隔离用户数据库"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let key = KeyringSecretStore::app()
        .read_api_key()
        .expect("先在应用内保存密钥");
    let balance_text = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .get("https://api.deepseek.com/user/balance")
        .set("Authorization", &format!("Bearer {key}"))
        .call()
        .expect("只读核实账户计费货币")
        .into_string()
        .expect("账户货币响应");
    let balance: serde_json::Value = serde_json::from_str(&balance_text).expect("账户货币 JSON");
    assert!(
        balance["balance_infos"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["currency"] == "CNY")),
        "账户不是已核实的人民币计费，不进行付费请求"
    );
    eprintln!("[EXP] CNY billing verified; shared budget cap=5.00 CNY; human input is scripted simulation");
    let settings = AppSettings {
        model_id: "deepseek-flash".to_owned(),
        thinking_level: "high".to_owned(),
        connection_status: "ready".to_owned(),
        ..Default::default()
    };
    let mut config = ModelConfig::from_settings(&settings);
    config.timeout_ms = 120_000;
    for mixed in [false, true] {
        let store = SqliteStore::open(dir.join(if mixed {
            "mixed.sqlite"
        } else {
            "all-ai.sqlite"
        }))
        .unwrap();
        store
            .save_settings(&settings, "2026-10-12T00:00:00Z")
            .unwrap();
        let profile = uuid::Uuid::from_u128(if mixed { 0xF51002 } else { 0xF51001 });
        let existing = store.recent_matches(profile, 1).unwrap().first().cloned();
        if !store
            .list_profiles()
            .unwrap()
            .iter()
            .any(|p| p.id == profile)
        {
            store
                .create_profile(
                    profile,
                    "隔离真实验收（人工输入由脚本模拟）",
                    "2026-10-12T00:00:00Z",
                )
                .unwrap();
        }
        let mut desk = Desk::new();
        desk.profile_id = Some(profile);
        desk.live = true;
        desk.model_config = config.clone();
        if let Some(existing) = existing {
            let report = store.load_report(existing.id).unwrap();
            desk.restore_match(store.load_match(existing.id).unwrap(), report);
            desk.live = true;
            desk.model_config = config.clone();
            if desk.view().phase == "report_ready" {
                eprintln!("[EXP] already complete mixed={mixed}; no repeated paid calls");
                continue;
            }
            eprintln!(
                "[EXP] resumed mixed={mixed} cursor={} transcripts={}",
                desk.view().cursor,
                desk.view().transcripts.len()
            );
        } else {
            let (rules, confidence) =
                parse_rules_with(true, config.clone(), RULES, TOPIC, MATERIALS).unwrap();
            desk.apply_recognized_rules(
                rules,
                RULES,
                confidence,
                RuleSetup {
                    mode: if mixed {
                        MatchMode::Scoring
                    } else {
                        MatchMode::Training
                    },
                    topic: TOPIC.to_owned(),
                    materials: MATERIALS.to_owned(),
                    user_side: Some(Side::Affirmative),
                    difficulty: Difficulty::Contest,
                },
            )
            .unwrap();
            assert_eq!(desk.view().schedule.len(), 13);
            desk.confirm_rules().unwrap();
            desk.analyze().unwrap();
            if mixed {
                for _ in 0..2 {
                    let passage = desk.passage();
                    desk.accept_calibration(&passage, 60_000, false, &[])
                        .unwrap();
                }
            }
            desk.assign_and_ready(Some(Side::Affirmative), if mixed { &[0] } else { &[] })
                .unwrap();
            assert_eq!(desk.view().phase, "ready");
            desk.start().unwrap();
        }
        let state = AppState {
            store: Mutex::new(store),
            desk: Mutex::new(desk),
            active_stream: Mutex::new(None),
            cross_exam: Mutex::new(CrossExamService::default()),
        };
        crate::commands::persist_current_match(&state).unwrap();
        let gateway = Gateway::new(
            DeepSeekTransport::new(UreqHttp, config.clone(), key.clone()),
            ThreadSleep,
            config.clone(),
        );
        let cancel = AtomicBool::new(false);
        let (mut seen, mut free_rounds) = {
            let desk = state.desk.lock().unwrap();
            let a = desk.aggregate.as_ref().unwrap();
            let seen = a
                .transcripts
                .iter()
                .filter_map(|turn| {
                    a.schedule
                        .iter()
                        .find(|e| e.id == turn.event_id)
                        .map(|e| e.stage_id.clone())
                })
                .collect::<std::collections::HashSet<_>>();
            let rounds = a
                .transcripts
                .iter()
                .filter(|turn| {
                    a.schedule
                        .iter()
                        .any(|e| e.id == turn.event_id && e.kind == StageKind::FreeDebate)
                })
                .count();
            (seen, rounds)
        };
        loop {
            let event = {
                let desk = state.desk.lock().unwrap();
                let aggregate = desk.aggregate.as_ref().unwrap();
                if aggregate.cursor >= aggregate.schedule.len() {
                    break;
                }
                aggregate.schedule[aggregate.cursor].clone()
            };
            seen.insert(event.stage_id.clone());
            eprintln!(
                "[EXP] mixed={mixed} stage={} free_rounds={free_rounds}",
                event.stage_id
            );
            if event.kind == StageKind::CrossExam {
                for _ in 0..2 {
                    let mut desk = state.desk.lock().unwrap();
                    let mut cross = state.cross_exam.lock().unwrap();
                    let snapshot = cross.snapshot(&desk).unwrap();
                    if snapshot.phase == "completed" {
                        break;
                    }
                    let selection = cross.selection_request(&desk).unwrap();
                    let roles = gateway
                        .select_cross_exam(&selection.context, &cancel)
                        .unwrap();
                    cross
                        .apply_role_selection(
                            &mut desk,
                            roles.questioner,
                            roles.respondent,
                            roles.selection_summary,
                        )
                        .unwrap();
                    let question = cross.generation_request(&desk, false).unwrap();
                    let q = super::ai_turn::generate_turn_text(
                        true,
                        config.clone(),
                        &question.context,
                        question.expected_slot,
                        question.word_cap,
                        &cancel,
                        |_| {},
                    )
                    .unwrap();
                    cross.submit_question(&mut desk, q, 1000).unwrap();
                    let answer = cross.generation_request(&desk, true).unwrap();
                    let human = desk
                        .aggregate
                        .as_ref()
                        .unwrap()
                        .effective_controller(answer.expected_slot)
                        == Controller::Human;
                    let a = if human {
                        "是，高风险用途需要可审计的许可与复核，而低风险用途不适用同等限制。"
                            .to_owned()
                    } else {
                        super::ai_turn::generate_turn_text(
                            true,
                            config.clone(),
                            &answer.context,
                            answer.expected_slot,
                            answer.word_cap,
                            &cancel,
                            |_| {},
                        )
                        .unwrap()
                    };
                    cross
                        .prepare_answer_with_metrics(
                            &desk,
                            a.clone(),
                            human.then_some(HumanAnswerMetrics {
                                input_ms: 15000,
                                pause_ms: 0,
                                reverse_units: 0,
                                reverse_actions: 0,
                                pasted: false,
                                input_counts: Some((
                                    crate::domain::text_units::count_effective_units(&a),
                                    crate::domain::text_units::count_effective_units(&a),
                                )),
                            }),
                        )
                        .unwrap();
                    cross
                        .reveal(crate::domain::text_units::count_effective_units(&a), 1000)
                        .unwrap();
                    cross.complete_and_commit(&mut desk, None).unwrap();
                }
                {
                    let mut desk = state.desk.lock().unwrap();
                    state
                        .cross_exam
                        .lock()
                        .unwrap()
                        .end_session(&mut desk)
                        .unwrap();
                }
                crate::commands::persist_current_match(&state).unwrap();
                continue;
            }
            if event.kind == StageKind::FreeDebate && free_rounds >= 16 {
                let mut desk = state.desk.lock().unwrap();
                desk.waive_free_debate_time().unwrap();
                drop(desk);
                crate::commands::persist_current_match(&state).unwrap();
                continue;
            }
            let mut desk = state.desk.lock().unwrap();
            if mixed && event.kind == StageKind::FreeDebate && free_rounds == 0 {
                desk.switch_current_controller(SlotId::Aff2.index(), true, true)
                    .unwrap();
                let original = desk.aggregate.as_ref().unwrap().assignments.clone();
                let window = desk.aggregate.as_ref().unwrap().control_window_id();
                desk.switch_current_controller(SlotId::Aff2.index(), false, true)
                    .unwrap();
                assert_eq!(desk.aggregate.as_ref().unwrap().control_window_id(), window);
                assert_eq!(desk.aggregate.as_ref().unwrap().assignments, original);
                desk.switch_current_controller(SlotId::Aff2.index(), true, true)
                    .unwrap();
            }
            if desk.view().current_human {
                let text=if event.kind==StageKind::FreeDebate{"请对方区分高风险与低风险用途。我方提出可撤销许可与审计，既有救济机制也保留创新空间。"}else{"我方主张限制高风险用途，标准是降低可验证伤害并保留低风险创新。许可、审计、人工复核能提高问责；不是禁止所有人工智能。"}.to_owned();
                let events = vec![crate::domain::telemetry::InputEvent::Insert {
                    text: text.clone(),
                    source: crate::domain::telemetry::FragmentSource::Typed,
                }];
                let t = crate::domain::telemetry::fold_with_elapsed(&events, 30000).telemetry;
                let claimed = crate::domain::telemetry::ClaimedMetrics {
                    input_elapsed_ms: t.input_elapsed_ms,
                    forward_typed_units: t.forward_typed_units,
                    effective_positive_units: t.effective_positive_units,
                    reverse_units: t.reverse_edited_units,
                    reverse_actions: t.reverse_edit_actions,
                    pause_ms: t.pause_ms,
                    focus_loss_ms: t.focus_loss_ms,
                    paste_units: t.paste_units,
                };
                super::human_turn::submit_validated_human_turn(&mut desk, text, &events, &claimed)
                    .unwrap();
                drop(desk);
                crate::commands::persist_current_match(&state).unwrap();
            } else {
                let prepared = crate::commands::stream::prepare_turn(&mut desk).unwrap();
                drop(desk);
                let turn = if prepared.legal_team_slots.is_empty() {
                    super::ai_turn::TeamTurn {
                        slot: prepared.expected_slot,
                        text: super::ai_turn::generate_turn_text(
                            true,
                            prepared.config,
                            &prepared.context,
                            prepared.expected_slot,
                            prepared.word_cap,
                            &cancel,
                            |_| {},
                        )
                        .unwrap(),
                        reply_to: vec![],
                    }
                } else {
                    super::ai_turn::generate_team_turn(
                        prepared.config,
                        &prepared.context,
                        &prepared.legal_team_slots,
                        prepared.word_cap,
                        &cancel,
                        |_| {},
                        |_| {},
                    )
                    .unwrap()
                };
                crate::commands::mutate_submit_selected_ai(
                    &state,
                    turn.text,
                    1,
                    &prepared.ids.match_id,
                    &prepared.ids.event_id,
                    prepared.expected_slot,
                    prepared.revision,
                    &cancel,
                    turn.slot,
                    turn.reply_to,
                )
                .unwrap();
            }
            if event.kind == StageKind::FreeDebate {
                free_rounds += 1;
            }
        }
        assert_eq!(seen.len(), 13);
        assert_eq!(free_rounds, 16);
        let mut desk = state.desk.lock().unwrap();
        desk.finish(true).unwrap();
        assert!(desk
            .report
            .as_ref()
            .is_some_and(|r| r.scoring_status != "SCORING_PENDING"));
        eprintln!(
            "[EXP] COMPLETE mixed={mixed} transcripts={} report={}",
            desk.view().transcripts.len(),
            desk.report.as_ref().unwrap().scoring_status
        );
        drop(desk);
        crate::commands::persist_current_match(&state).unwrap();
    }
}
