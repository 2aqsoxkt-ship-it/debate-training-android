use uuid::Uuid;

use crate::domain::match_engine::aggregate::{scoring_assignments, training_assignments};
use crate::domain::match_engine::{MatchAggregate, MatchCommand};
use crate::domain::ruleset::{sample_ruleset, MatchMode, Side};
use crate::domain::timing::FakeClock;
use crate::infrastructure::db::{MatchRepository, MemoryMatchRepository};

pub fn play_constructed_match() -> MatchAggregate {
    let mut clock = FakeClock::new();
    let mut repository = MemoryMatchRepository::new();
    let mut match_state = prepare(MatchMode::Training, None);
    drive(&mut match_state, &mut clock);
    repository
        .save(Uuid::from_u128(1), &match_state, "2026-09-23T00:00:00Z")
        .expect("memory save");
    repository.load(match_state.id).expect("memory load")
}

pub fn prepare(mode: MatchMode, user_side: Option<Side>) -> MatchAggregate {
    let mut match_state = MatchAggregate::new(
        Uuid::from_u128(42),
        mode,
        "人工智能应被禁止",
        "闭卷材料",
        240,
    );
    match_state
        .handle(MatchCommand::SubmitParsedRules {
            ruleset: Box::new(sample_ruleset()),
        })
        .expect("parse rules");
    match_state
        .handle(MatchCommand::ConfirmRules)
        .expect("confirm rules");
    match_state
        .handle(MatchCommand::RecordTopicAnalysis {
            summary: "中立拆题".to_owned(),
        })
        .expect("analyze");
    let assignments = match user_side {
        Some(side) => scoring_assignments(side),
        None => training_assignments(),
    };
    match_state
        .handle(MatchCommand::AssignControl {
            user_side,
            assignments,
        })
        .expect("assign");
    if mode == MatchMode::Scoring {
        match_state
            .handle(MatchCommand::RecordCalibration)
            .expect("calibrate");
    }
    match_state
        .handle(MatchCommand::ConfirmLineup)
        .expect("lineup");
    match_state.handle(MatchCommand::MarkReady).expect("ready");
    match_state.handle(MatchCommand::Start).expect("start");
    match_state
}

fn drive(match_state: &mut MatchAggregate, clock: &mut FakeClock) {
    let event_ids: Vec<Uuid> = match_state.schedule.iter().map(|event| event.id).collect();
    for event_id in event_ids {
        let waited_from = clock.now_ms();
        clock.advance(1_000);
        match_state
            .handle(MatchCommand::NoteAiWait {
                waited_ms: clock.now_ms() - waited_from,
            })
            .expect("ai wait");
        let input_from = clock.now_ms();
        clock.advance(2_000);
        match_state
            .handle(MatchCommand::SubmitCurrent {
                event_id,
                text: "正方认为应当谨慎限制".to_owned(),
                input_ms: clock.now_ms() - input_from,
            })
            .expect("submit");
    }
    match_state
        .handle(MatchCommand::CompleteMatch)
        .expect("complete");
}

#[cfg(test)]
mod tests {
    use super::prepare;
    use crate::domain::events::MatchEventKind;
    use crate::domain::match_engine::scenario::play_constructed_match;
    use crate::domain::match_engine::{CommandReject, MatchCommand, MatchPhase};
    use crate::domain::ruleset::{sample_ruleset, MatchMode, Side, SlotId};
    use proptest::prelude::*;

    #[test]
    fn cannot_analyze_or_start_before_rules_are_confirmed() {
        let mut match_state = crate::domain::match_engine::MatchAggregate::new(
            uuid::Uuid::from_u128(7),
            MatchMode::Training,
            "辩题",
            "",
            240,
        );
        match_state
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(sample_ruleset()),
            })
            .expect("parsed");
        let analyzed = match_state.handle(MatchCommand::RecordTopicAnalysis {
            summary: "直接开赛".to_owned(),
        });
        assert_eq!(analyzed, Err(CommandReject::RulesNotConfirmed));
        assert_eq!(
            match_state.handle(MatchCommand::Start),
            Err(CommandReject::RulesNotConfirmed)
        );
    }

    #[test]
    fn topic_rules_and_lineup_changes_revoke_the_matching_hash() {
        let mut setup = crate::domain::match_engine::MatchAggregate::new(
            uuid::Uuid::from_u128(8),
            MatchMode::Training,
            "原辩题",
            "材料",
            240,
        );
        setup
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(sample_ruleset()),
            })
            .unwrap();
        setup.handle(MatchCommand::ConfirmRules).unwrap();
        let rules_hash = setup.rules_hash.clone().unwrap();
        setup
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "分析".to_owned(),
            })
            .unwrap();
        setup
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: crate::domain::match_engine::aggregate::training_assignments(),
            })
            .unwrap();
        setup.handle(MatchCommand::ConfirmLineup).unwrap();
        assert!(setup.lineup_hash.is_some());
        setup
            .handle(MatchCommand::SetTopic {
                topic: "新辩题".to_owned(),
            })
            .unwrap();
        assert_eq!(setup.rules_hash.as_deref(), Some(rules_hash.as_str()));
        assert!(setup.lineup_hash.is_none());
        assert_eq!(setup.phase, MatchPhase::RulesConfirmed);
        assert_eq!(
            setup.handle(MatchCommand::ConfirmLineup),
            Err(CommandReject::InvalidTransition)
        );
        setup
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "新辩题分析".to_owned(),
            })
            .unwrap();
        setup
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: crate::domain::match_engine::aggregate::training_assignments(),
            })
            .unwrap();
        setup.handle(MatchCommand::ConfirmLineup).unwrap();
        setup
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: crate::domain::match_engine::aggregate::training_assignments(),
            })
            .unwrap();
        assert!(setup.lineup_hash.is_none());
        setup.handle(MatchCommand::ConfirmLineup).unwrap();
        let mut revised = sample_ruleset();
        revised.ruleset_name = "修订赛制".to_owned();
        setup
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(revised),
            })
            .unwrap();
        assert!(setup.rules_hash.is_none());
        assert!(setup.lineup_hash.is_none());
    }

    #[test]
    fn free_debate_interrupts_do_not_change_transcript_or_clock() {
        let mut match_state = prepare(MatchMode::Training, None);
        let opening = match_state.schedule[0].id;
        match_state
            .handle(MatchCommand::SubmitCurrent {
                event_id: opening,
                text: "陈词".to_owned(),
                input_ms: 100,
            })
            .unwrap();
        assert_eq!(
            match_state.schedule[match_state.cursor].kind,
            crate::domain::ruleset::StageKind::FreeDebate
        );
        let transcripts = match_state.transcripts.len();
        let charged = match_state.charged_ms;
        let events = match_state.events.len();
        assert_eq!(
            match_state.handle(MatchCommand::Interrupt),
            Err(CommandReject::FreeDebateInterruptForbidden)
        );
        assert_eq!(match_state.transcripts.len(), transcripts);
        assert_eq!(match_state.charged_ms, charged);
        assert_eq!(match_state.events.len(), events);
    }

    #[test]
    fn scoring_mode_rejects_cross_team_takeover() {
        let mut match_state = prepare(MatchMode::Scoring, Some(Side::Affirmative));
        let before = match_state.assignments.clone();
        assert_eq!(
            match_state.handle(MatchCommand::RequestTakeover {
                slot_index: SlotId::Neg1.index(),
                second_confirmed: true,
            }),
            Err(CommandReject::CrossTeamTakeoverForbidden)
        );
        assert_eq!(match_state.assignments, before);
        assert!(!match_state.non_standard);
    }

    #[test]
    fn constructed_match_finishes_from_memory_and_fake_clock() {
        let finished = play_constructed_match();
        assert_eq!(finished.phase, MatchPhase::MatchComplete);
        assert_eq!(finished.slots().len(), 8);
        assert_eq!(finished.transcripts.len(), 4);
        assert_eq!(finished.charged_ms, 8_000);
        assert_eq!(finished.ai_wait_ms, 4_000);
        assert!(finished
            .events
            .iter()
            .any(|event| event.kind == MatchEventKind::AiWait));
        assert!(finished.events.len() >= finished.transcripts.len());
    }

    proptest! {
        #[test]
        fn random_commands_keep_eight_slots(seed in proptest::collection::vec(0u8..8, 0..30)) {
            let mut match_state = prepare(MatchMode::Training, None);
            for value in seed {
                let command = match value {
                    0 => MatchCommand::Interrupt,
                    1 => MatchCommand::Start,
                    2 => MatchCommand::RecordTopicAnalysis { summary: "x".to_owned() },
                    3 => MatchCommand::RequestTakeover { slot_index: 9, second_confirmed: true },
                    4 => MatchCommand::SetTopic { topic: "另一题".to_owned() },
                    5 => MatchCommand::SubmitCurrent { event_id: uuid::Uuid::from_u128(1), text: "文本".to_owned(), input_ms: 5 },
                    6 => MatchCommand::NoteAiWait { waited_ms: 20 },
                    _ => MatchCommand::ConfirmRules,
                };
                let _ = match_state.handle(command);
                prop_assert_eq!(match_state.slots(), SlotId::ALL);
                prop_assert_eq!(match_state.assignments.len(), 8);
            }
        }
    }
}
