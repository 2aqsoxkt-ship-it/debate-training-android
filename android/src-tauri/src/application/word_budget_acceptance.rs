//! Offline full-match gate and accounting shared with the ignored live harness.

use crate::application::desk::Desk;
use crate::domain::events::{CrossExamSessionV1, FreeDebateSessionV1};
use crate::domain::match_engine::{HumanScoringStateV3, MatchResourceMode, WordBudgetsV1};
use crate::domain::ruleset::Side;

pub(crate) const TOPIC: &str = "人工智能应当受到限制";
pub(crate) const MATERIALS: &str = "闭卷材料：高风险人工智能用途应当经过许可、审计并保留人工复核。";
pub(crate) const RULES: &str = "双方各18分钟，自由辩各4分钟另外计时。陈词1：正方一辩发言，反方四辩质询。反方一辩陈词，正方四辩质询。质询方可以进行打断，被质询方不能对此环节进行打断、反问。质询回答时间计入质询者时间。陈词2：正方二辩发言，发言完毕后反方三辩质询正方二辩；反方二辩发言，发言完毕后正方三辩质询反方二辩。质询小结由双方三辩完成，先反方再正方。自由辩论正方先开始，此后正、反自动轮流发言。每位辩手发言次数、时间及每方四位辩手的发言次序均无限制，但一方辩手发言落座后，在对方发言之前这一方任何一名辩手不得再次发言。双方各有4分钟时间。总结陈词由该队四辩进行，由反方先发言。";

#[derive(Debug)]
pub(crate) struct WordAccounting {
    mode: MatchResourceMode,
    budgets: Option<WordBudgetsV1>,
    main: (u32, u32),
    source_main_ms: (u64, u64),
    free: Option<(u32, u32, Side)>,
    free_remaining: (u32, u32),
    free_sessions: Vec<FreeDebateSessionV1>,
    cross_sessions: Vec<CrossExamSessionV1>,
    scoring: Option<HumanScoringStateV3>,
    transcript_count: usize,
    cursor: usize,
}

impl WordAccounting {
    pub(crate) fn capture(desk: &Desk) -> Self {
        let aggregate = desk.aggregate.as_ref().expect("accounting aggregate");
        Self {
            mode: aggregate.metering_mode,
            budgets: aggregate.word_budgets.clone(),
            main: (
                aggregate.remaining_main_units(Side::Affirmative),
                aggregate.remaining_main_units(Side::Negative),
            ),
            source_main_ms: (
                aggregate.affirmative_main_remaining_ms,
                aggregate.negative_main_remaining_ms,
            ),
            free: aggregate.current_free_debate_units(),
            free_remaining: (
                aggregate.free_debate_remaining_units(Side::Affirmative),
                aggregate.free_debate_remaining_units(Side::Negative),
            ),
            free_sessions: aggregate.free_debate_sessions.clone(),
            cross_sessions: aggregate.cross_exam_sessions.clone(),
            scoring: aggregate.scoring_state.clone(),
            transcript_count: aggregate.transcripts.len(),
            cursor: aggregate.cursor,
        }
    }

    pub(crate) fn assert_main_charge(&self, desk: &Desk, charged_side: Side) {
        let aggregate = desk.aggregate.as_ref().expect("accounting aggregate");
        assert_eq!(aggregate.metering_mode, MatchResourceMode::Words);
        let units: u32 = aggregate.transcripts[self.transcript_count..]
            .iter()
            .map(|entry| entry.effective_units)
            .sum();
        assert!(
            units > 0,
            "each submitted event must publish effective text"
        );
        let expected = match charged_side {
            Side::Affirmative => (self.main.0.saturating_sub(units), self.main.1),
            Side::Negative => (self.main.0, self.main.1.saturating_sub(units)),
        };
        assert_eq!(
            (
                aggregate.remaining_main_units(Side::Affirmative),
                aggregate.remaining_main_units(Side::Negative),
            ),
            expected,
            "main budget must charge only public effective units to {charged_side:?}"
        );
        assert_eq!(aggregate.free_debate_sessions, self.free_sessions);
        assert_eq!(
            (
                aggregate.free_debate_remaining_units(Side::Affirmative),
                aggregate.free_debate_remaining_units(Side::Negative),
            ),
            self.free_remaining
        );
        self.assert_source_clocks(desk);
        assert_word_view(desk);
    }

    pub(crate) fn assert_free_charge(&self, desk: &Desk, charged_side: Side) {
        let aggregate = desk.aggregate.as_ref().expect("accounting aggregate");
        assert_eq!(aggregate.transcripts.len(), self.transcript_count + 1);
        let units = aggregate.transcripts.last().unwrap().effective_units;
        assert!(units > 0);
        let (affirmative, negative, side) = self.free.expect("free debate budget");
        assert_eq!(side, charged_side);
        let expected = match charged_side {
            Side::Affirmative => (affirmative.saturating_sub(units), negative),
            Side::Negative => (affirmative, negative.saturating_sub(units)),
        };
        let session = aggregate.free_debate_sessions.last().expect("free session");
        assert_eq!(
            (
                session.affirmative_remaining_units,
                session.negative_remaining_units
            ),
            expected,
            "free budget must charge only this turn's effective units"
        );
        assert_eq!(aggregate.word_budgets, self.budgets);
        self.assert_source_clocks(desk);
        assert_word_view(desk);
    }

    pub(crate) fn assert_restored(&self, desk: &Desk) {
        let aggregate = desk.aggregate.as_ref().expect("restored aggregate");
        assert_eq!(aggregate.metering_mode, self.mode);
        assert_eq!(aggregate.word_budgets, self.budgets);
        assert_eq!(aggregate.current_free_debate_units(), self.free);
        assert_eq!(
            (
                aggregate.free_debate_remaining_units(Side::Affirmative),
                aggregate.free_debate_remaining_units(Side::Negative),
            ),
            self.free_remaining
        );
        assert_eq!(aggregate.free_debate_sessions, self.free_sessions);
        assert_eq!(aggregate.cross_exam_sessions, self.cross_sessions);
        assert_eq!(aggregate.scoring_state, self.scoring);
        assert_eq!(aggregate.transcripts.len(), self.transcript_count);
        assert_eq!(aggregate.cursor, self.cursor);
        self.assert_source_clocks(desk);
        assert_word_view(desk);
    }

    fn assert_source_clocks(&self, desk: &Desk) {
        let aggregate = desk.aggregate.as_ref().expect("accounting aggregate");
        assert_eq!(
            (
                aggregate.affirmative_main_remaining_ms,
                aggregate.negative_main_remaining_ms,
            ),
            self.source_main_ms,
            "input_ms is evidence and must not charge source time pools in words mode"
        );
        for session in &aggregate.free_debate_sessions {
            assert_eq!(
                session.affirmative_remaining_ms,
                session.affirmative_total_ms
            );
            assert_eq!(session.negative_remaining_ms, session.negative_total_ms);
        }
    }
}

fn assert_word_view(desk: &Desk) {
    let aggregate = desk.aggregate.as_ref().expect("view aggregate");
    let view = desk.view();
    assert_eq!(view.metering_mode, "words");
    assert_eq!(view.remaining_units, aggregate.current_remaining_units());
    assert_eq!(view.current_word_cap, aggregate.current_word_cap());
    assert_eq!(
        view.affirmative_main_units,
        aggregate.remaining_main_units(Side::Affirmative)
    );
    assert_eq!(
        view.negative_main_units,
        aggregate.remaining_main_units(Side::Negative)
    );
    assert_eq!(
        view.affirmative_free_units,
        aggregate.free_debate_remaining_units(Side::Affirmative)
    );
    assert_eq!(
        view.negative_free_units,
        aggregate.free_debate_remaining_units(Side::Negative)
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::atomic::AtomicBool;
    use std::time::{SystemTime, UNIX_EPOCH};

    use uuid::Uuid;

    use super::{assert_word_view, WordAccounting, MATERIALS, RULES, TOPIC};
    use crate::application::cross_exam_service::{CrossExamService, HumanAnswerMetrics};
    use crate::application::desk::{parse_rules_with, Desk, RuleSetup};
    use crate::application::free_debate_service::pick_free_debate_slot;
    use crate::domain::match_engine::{MatchPhase, MatchResourceMode};
    use crate::domain::ruleset::{MatchMode, Side, SlotId, StageKind};
    use crate::domain::ruleset_v2::ClockPoolKindV3;
    use crate::domain::text_units::{count_effective_units, split_effective};
    use crate::domain::visibility::Difficulty;
    use crate::infrastructure::ai::schema::{fixture_json, GeneratedTurnV1};
    use crate::infrastructure::ai::{
        AiContext, CompletionRequest, CompletionResponse, Gateway, ModelTransport, Purpose,
        ThreadSleep, TransportFault,
    };
    use crate::infrastructure::db::SqliteStore;

    // A deterministic mock response obeys the requested slot; the production
    // Gateway still performs the normal strict generated-turn schema validation.
    struct SlotFixtureTransport(SlotId);

    impl ModelTransport for SlotFixtureTransport {
        fn complete(
            &self,
            request: &CompletionRequest,
            _cancel: &AtomicBool,
        ) -> Result<CompletionResponse, TransportFault> {
            assert_eq!(request.purpose, Purpose::GenerateTurn);
            let mut turn: GeneratedTurnV1 =
                serde_json::from_str(&fixture_json(request.purpose)).unwrap();
            turn.slot = self.0;
            Ok(CompletionResponse {
                content: serde_json::to_string(&turn).unwrap(),
                prompt_tokens: 3,
                completion_tokens: 5,
                elapsed_ms: 0,
            })
        }
    }

    fn mock_turn(desk: &Desk, slot: SlotId, cap: u32) -> String {
        assert!(cap > 0);
        let turn = Gateway::new(
            SlotFixtureTransport(slot),
            ThreadSleep,
            desk.model_config.clone(),
        )
        .generate_turn(
            &AiContext {
                topic: TOPIC.to_owned(),
                materials: MATERIALS.to_owned(),
                own_strategy: format!("required_slot={slot:?}\nword_cap={cap}"),
                ..AiContext::default()
            },
            &AtomicBool::new(false),
        )
        .expect("mock turn schema");
        assert_eq!(turn.slot, slot);
        let text = split_effective(&turn.text, cap).0.to_owned();
        assert!(count_effective_units(&text) > 0);
        text
    }

    fn save_and_restore(desk: &mut Desk, store: &SqliteStore, profile_id: Uuid) {
        let before = WordAccounting::capture(desk);
        desk.mark_recovery().expect("mark recovery");
        let aggregate = desk.aggregate.as_ref().unwrap();
        store
            .save_match(profile_id, aggregate, "2026-10-04T00:00:01Z")
            .unwrap();
        let loaded = store.load_match(aggregate.id).unwrap();
        desk.restore_match(loaded, None);
        assert_eq!(
            desk.aggregate.as_ref().unwrap().phase,
            MatchPhase::InProgress
        );
        before.assert_restored(desk);
    }

    #[test]
    fn word_budget_full_mock_match_smoke() {
        let mut desk = Desk::new();
        assert!(!desk.live, "this gate must never request a real API");
        let profile_id = Uuid::from_u128(0xABCD_0004);
        desk.profile_id = Some(profile_id);
        let (rules, confidence) =
            parse_rules_with(false, desk.model_config.clone(), RULES, TOPIC, MATERIALS)
                .expect("deterministic 13-stage parse");
        desk.apply_recognized_rules(
            rules,
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
        .expect("apply full rules");
        assert_eq!(
            desk.aggregate.as_ref().unwrap().metering_mode,
            MatchResourceMode::Words
        );
        let mut draft = desk.view().rule_draft.expect("editable semantic rules");
        for pool in &mut draft.clock_pools {
            pool.unit_budget = Some(match (pool.kind, pool.side) {
                (ClockPoolKindV3::Main, Side::Affirmative) => 4_327,
                (ClockPoolKindV3::Main, Side::Negative) => 5_101,
                (ClockPoolKindV3::FreeDebate, Side::Affirmative) => 1_003,
                (ClockPoolKindV3::FreeDebate, Side::Negative) => 1_207,
            });
        }
        desk.replace_semantic_rules(draft)
            .expect("four custom word pools");
        assert_eq!(desk.view().affirmative_free_units, 1_003);
        assert_eq!(desk.view().negative_free_units, 1_207);
        desk.confirm_rules().unwrap();
        desk.analyze()
            .expect("mock topic, strategy and eight-role pipeline");
        assert_eq!(desk.role_packets_ref().unwrap().len(), 8);
        for _ in 0..2 {
            let passage = desk.passage();
            desk.accept_calibration(&passage, 60_000, false, &[])
                .unwrap();
        }
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        desk.start().unwrap();
        assert_eq!(desk.view().schedule.len(), 13);
        assert_eq!(desk.view().affirmative_main_units, 4_327);
        assert_eq!(desk.view().negative_main_units, 5_101);
        assert_word_view(&desk);

        let mut db_path = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        db_path.push(format!(
            "debate-word-mock-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let store = SqliteStore::open(&db_path).unwrap();
        store
            .create_profile(profile_id, "字数额度离线全场验收", "2026-10-04T00:00:00Z")
            .unwrap();
        let mut stages = HashSet::new();
        let mut cross = CrossExamService::default();
        let mut cross_count = 0;
        let mut interrupted = 0;
        let mut recovered_main = false;
        let mut recovered_free = false;
        let mut free_slots = [HashSet::new(), HashSet::new()];
        let mut free_count = 0;

        while desk.view().cursor < desk.view().schedule.len() {
            let aggregate = desk.aggregate.as_ref().unwrap();
            let event = aggregate.schedule[aggregate.cursor].clone();
            assert!(
                stages.insert(event.stage_id.clone()),
                "each stage advances through the application"
            );
            match event.kind {
                StageKind::CrossExam => {
                    let request = cross.selection_request(&desk).unwrap();
                    // The system's legal fixture roles vary with each stage.
                    cross
                        .apply_role_selection(
                            &mut desk,
                            request.fallback_questioner,
                            request.fallback_respondent,
                            "离线全场确定性派位".to_owned(),
                        )
                        .unwrap();
                    let before = WordAccounting::capture(&desk);
                    let question = cross.generation_request(&desk, false).unwrap();
                    let text = mock_turn(&desk, question.expected_slot, question.word_cap);
                    cross
                        .submit_question(&mut desk, text, 30 * 60 * 1_000)
                        .unwrap();
                    let answer = cross.generation_request(&desk, true).unwrap();
                    let text = mock_turn(&desk, answer.expected_slot, answer.word_cap);
                    let full_units = count_effective_units(&text);
                    cross
                        .prepare_answer_with_metrics(
                            &desk,
                            text,
                            Some(HumanAnswerMetrics {
                                input_ms: 45 * 60 * 1_000,
                                ..HumanAnswerMetrics::default()
                            }),
                        )
                        .unwrap();
                    if cross_count == 0 {
                        cross.reveal((full_units / 2).max(1), 1_000).unwrap();
                        cross
                            .interrupt_and_commit_with_input(
                                &mut desk,
                                "合法问方打断".to_owned(),
                                None,
                            )
                            .unwrap();
                        interrupted += 1;
                        let entry = desk.aggregate.as_ref().unwrap().transcripts.last().unwrap();
                        assert!(entry.effective_units < full_units);
                    } else {
                        cross.reveal(full_units, 1_000).unwrap();
                        cross.complete_and_commit(&mut desk, None).unwrap();
                    }
                    cross.end_session(&mut desk).unwrap();
                    before.assert_main_charge(&desk, request.fallback_questioner.side());
                    cross_count += 1;
                }
                StageKind::FreeDebate => {
                    let main_before = desk.aggregate.as_ref().unwrap().word_budgets.clone();
                    assert_eq!(desk.view().affirmative_free_units, 1_003);
                    assert_eq!(desk.view().negative_free_units, 1_207);
                    assert_eq!(desk.view().current_word_cap, 1_003);
                    let mut counts = std::collections::HashMap::new();
                    for round in 0..12 {
                        let before = WordAccounting::capture(&desk);
                        let side = desk
                            .aggregate
                            .as_ref()
                            .unwrap()
                            .current_free_debate_side()
                            .unwrap();
                        let candidates: Vec<_> = SlotId::ALL
                            .into_iter()
                            .filter(|slot| slot.side() == side && *slot != SlotId::Aff1)
                            .collect();
                        let slot = pick_free_debate_slot(&candidates, &counts).unwrap();
                        // Selecting an AI speaker is the same schedule update
                        // performed by the application stream command.
                        let aggregate = desk.aggregate.as_mut().unwrap();
                        aggregate.schedule[aggregate.cursor].speaker = slot;
                        let text = mock_turn(&desk, slot, desk.view().current_word_cap.min(100));
                        let input_ms = if round % 2 == 0 { 1 } else { 45 * 60 * 1_000 };
                        desk.submit(text, input_ms, 0, 0, 0, false).unwrap();
                        before.assert_free_charge(&desk, side);
                        *counts.entry(slot).or_insert(0) += 1;
                        free_slots[if side == Side::Affirmative { 0 } else { 1 }].insert(slot);
                        free_count += 1;
                        if round == 5 {
                            save_and_restore(&mut desk, &store, profile_id);
                            recovered_free = true;
                        }
                    }
                    assert!(desk
                        .interrupt_shown("自由辩不可打断".to_owned(), 3)
                        .is_err());
                    desk.waive_free_debate_time().unwrap();
                    desk.waive_free_debate_time().unwrap();
                    let aggregate = desk.aggregate.as_ref().unwrap();
                    assert_eq!(aggregate.word_budgets, main_before);
                    let session = aggregate.free_debate_sessions.last().unwrap();
                    assert!(session.completed);
                    assert_eq!(session.affirmative_remaining_units, 0);
                    assert_eq!(session.negative_remaining_units, 0);
                    assert_eq!(session.affirmative_remaining_ms, 240_000);
                    assert_eq!(session.negative_remaining_ms, 240_000);
                    assert_eq!(desk.view().affirmative_free_units, 0);
                    assert_eq!(desk.view().negative_free_units, 0);
                }
                _ => {
                    let before = WordAccounting::capture(&desk);
                    let text =
                        mock_turn(&desk, event.speaker, desk.view().current_word_cap.min(140));
                    let input_ms = if desk.view().cursor % 2 == 0 {
                        1
                    } else {
                        45 * 60 * 1_000
                    };
                    desk.submit(text, input_ms, 0, 0, 0, false).unwrap();
                    before.assert_main_charge(&desk, event.speaker.side());
                }
            }
            if !recovered_main && desk.view().cursor >= 6 {
                save_and_restore(&mut desk, &store, profile_id);
                recovered_main = true;
            }
        }
        assert_eq!(stages.len(), 13);
        assert_eq!(cross_count, 4);
        assert_eq!(interrupted, 1);
        assert_eq!(free_count, 12);
        assert!(free_slots[0].len() >= 3);
        assert!(free_slots[1].len() >= 3);
        assert!(recovered_main && recovered_free);
        assert_eq!(desk.aggregate.as_ref().unwrap().transcripts.len(), 28);
        assert!(
            desk.aggregate
                .as_ref()
                .unwrap()
                .scoring_state
                .as_ref()
                .unwrap()
                .in_cap_units
                > 0
        );
        desk.finish(true).expect("offline personal and team report");
        let report = desk.report.clone().unwrap();
        assert_eq!(report.formula_version, "score-v3");
        assert_eq!(report.scoring_status, "ready");
        assert_eq!(report.dimensions.len(), 7);
        let aggregate = desk.aggregate.as_ref().unwrap();
        assert_eq!(aggregate.phase, MatchPhase::ReportReady);
        assert_eq!(desk.view().affirmative_free_units, 0);
        assert_eq!(desk.view().negative_free_units, 0);
        store
            .save_match(profile_id, aggregate, "2026-10-04T00:00:02Z")
            .unwrap();
        store
            .save_report(aggregate.id, &report, "2026-10-04T00:00:02Z")
            .unwrap();
        assert_eq!(store.load_report(aggregate.id).unwrap(), Some(report));
        let restored = store.load_match(aggregate.id).unwrap();
        assert_eq!(restored.word_budgets, aggregate.word_budgets);
        assert_eq!(
            restored.free_debate_sessions,
            aggregate.free_debate_sessions
        );
        drop(store);
        std::fs::remove_file(db_path).unwrap();
    }
}
