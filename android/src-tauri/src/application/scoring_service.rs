//! Context construction for the two-stage semantic scoring pipeline.

use uuid::Uuid;

use crate::domain::events::CrossExamTurnKind;
use crate::domain::match_engine::{MatchAggregate, MatchResourceMode};
use crate::domain::ruleset::StageKind;
use crate::infrastructure::ai::AiContext;

pub fn stage_context(
    state: &MatchAggregate,
    transcript_id: Uuid,
    dimensions: &str,
    excluded_note: &str,
) -> Result<AiContext, String> {
    let entry = state
        .transcripts
        .iter()
        .find(|entry| entry.id == transcript_id)
        .ok_or("找不到待评分发言")?;
    let event = state
        .schedule
        .iter()
        .find(|event| event.id == entry.event_id)
        .ok_or("找不到待评分环节")?;
    let position = state
        .transcripts
        .iter()
        .position(|item| item.id == transcript_id)
        .ok_or("找不到待评分发言位置")?;
    let word_cap = entry.word_cap_at_submission.unwrap_or(event.word_cap);
    let metering_mode = match state.metering_mode {
        MatchResourceMode::Words => "words",
        MatchResourceMode::Time => "time",
    };
    let responds_to = if entry.cross_exam_turn_kind == Some(CrossExamTurnKind::Question) {
        let respondent = entry
            .cross_exam_exchange_id
            .and_then(|id| {
                state
                    .cross_exam_exchanges
                    .iter()
                    .find(|exchange| exchange.id == id)
            })
            .map(|exchange| format!("{:?}", exchange.respondent))
            .unwrap_or_else(|| "unknown".to_owned());
        format!(
            "cross_exam_turn=question round={} target_respondent={} previous_public_claim={}",
            entry.cross_exam_round_number, respondent, entry.response_target_text
        )
    } else if entry.cross_exam_turn_kind == Some(CrossExamTurnKind::Answer) {
        format!(
            "cross_exam_turn=answer round={} cross_exam_question={} answer_rubric=正面回应、论证证据、逻辑一致、防守有效、临场反应、团队论线",
            entry.cross_exam_round_number, entry.response_target_text
        )
    } else if entry.response_target_text.is_empty() {
        position
            .checked_sub(1)
            .and_then(|index| state.transcripts.get(index))
            .map(|previous| {
                format!(
                    "responds_to_id={} responds_to_text={}",
                    previous.id, previous.effective_text
                )
            })
            .unwrap_or_else(|| "responds_to=none".to_owned())
    } else {
        format!("cross_exam_question={}", entry.response_target_text)
    };
    let stage_rubric = match entry.cross_exam_turn_kind {
        Some(CrossExamTurnKind::Question) => {
            "cross_exam_rubric=切题性、问题清晰可回答、漏洞捕捉、公开主张承接、战术价值、临场性"
        }
        Some(CrossExamTurnKind::Answer) => {
            "cross_exam_rubric=正面回答、论证证据、逻辑一致性、防守有效性、临场反应、团队一致性"
        }
        None if event.kind == StageKind::FreeDebate && state.metering_mode == MatchResourceMode::Words => {
            "free_debate_rubric=是否回应上一轮、有效反驳、推进新论证、避免重复、辩位选择与职责、团队接续补位、争点回收、团队额度使用"
        }
        None if event.kind == StageKind::FreeDebate => {
            "free_debate_rubric=是否回应上一轮、有效反驳、推进新论证、避免重复、辩位选择与职责、团队接续补位、争点回收、团队时间使用"
        }
        None => "special_stage_rubric=not_applicable",
    };
    let duty = state
        .ruleset
        .as_ref()
        .map(|rules| {
            let duties = if entry.slot.side() == crate::domain::ruleset::Side::Affirmative {
                &rules.affirmative_duties
            } else {
                &rules.negative_duties
            };
            duties[entry.slot.index() % 4].clone()
        })
        .unwrap_or_default();

    Ok(AiContext {
        topic: state.topic.clone(),
        rules_text: state
            .ruleset
            .as_ref()
            .and_then(|rules| serde_json::to_string(rules).ok())
            .unwrap_or_default(),
        materials: state.materials.clone(),
        own_strategy: format!(
            "score_only_transcript_id={transcript_id}\nslot={:?}\nstage_id={}\nstage_kind={:?}\nduty={}\nmetering_mode={metering_mode}\nword_cap={}\n{}\n{}\nscore_dimensions={}\nexcluded={}\nhuman_effective_text={}",
            entry.slot,
            event.stage_id,
            event.kind,
            duty,
            word_cap,
            responds_to,
            stage_rubric,
            dimensions,
            excluded_note,
            entry.effective_text,
        ),
        opponent_strategy: String::new(),
        public_transcript: public_transcript(state),
    })
}

pub fn match_review_context(
    state: &MatchAggregate,
    stage_results: &[String],
    dimensions: &str,
    allowed_transcript_ids: &[String],
) -> AiContext {
    let metering_mode = match state.metering_mode {
        MatchResourceMode::Words => "words",
        MatchResourceMode::Time => "time",
    };
    AiContext {
        topic: state.topic.clone(),
        rules_text: state
            .ruleset
            .as_ref()
            .and_then(|rules| serde_json::to_string(rules).ok())
            .unwrap_or_default(),
        materials: state.materials.clone(),
        own_strategy: format!(
            "review_dimensions={dimensions}\nmetering_mode={metering_mode}\nallowed_adjustment_transcript_ids={}\nEach adjustment must be within -8..8 and cite one id from allowed_adjustment_transcript_ids. Use an empty adjustments array when no human adjustment is justified. AI teammate or opponent quality must never change a human personal score.\nstage_results:\n{}",
            allowed_transcript_ids.join(","),
            stage_results.join("\n")
        ),
        opponent_strategy: String::new(),
        public_transcript: public_transcript(state),
    }
}

fn public_transcript(state: &MatchAggregate) -> Vec<String> {
    state
        .transcripts
        .iter()
        .map(|entry| {
            format!(
                "transcript_id={} slot={:?} cross_exam_turn={:?} cross_exam_round={} response_target={} text={}",
                entry.id,
                entry.slot,
                entry.cross_exam_turn_kind,
                entry.cross_exam_round_number,
                entry.response_target_text,
                entry.effective_text
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{match_review_context, stage_context};
    use crate::domain::events::{Controller, DebaterAssignmentV1, ASSIGNMENT_SCHEMA_VERSION};
    use crate::domain::match_engine::{
        training_assignments, MatchAggregate, MatchCommand, MatchResourceMode, MatchSnapshotV1,
    };
    use crate::domain::ruleset::{four_on_four_template, MatchMode, Side, SlotId, StageKind};
    use uuid::Uuid;

    fn ready_semantic_match() -> MatchAggregate {
        let mut state = MatchAggregate::new(
            Uuid::from_u128(9_000),
            MatchMode::Training,
            "辩题",
            "材料",
            240,
        );
        state
            .submit_semantic_rules(crate::domain::ruleset_v2::traditional_four_on_four_v2())
            .unwrap();
        state.handle(MatchCommand::ConfirmRules).unwrap();
        state
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "拆题".to_owned(),
            })
            .unwrap();
        state
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: training_assignments(),
            })
            .unwrap();
        state.handle(MatchCommand::ConfirmLineup).unwrap();
        state.handle(MatchCommand::MarkReady).unwrap();
        state.handle(MatchCommand::Start).unwrap();
        state
    }

    fn assert_persisted_submission_cap(state: &MatchAggregate, transcript_id: Uuid, cap: u32) {
        let entry = state
            .transcripts
            .iter()
            .find(|entry| entry.id == transcript_id)
            .unwrap();
        assert_eq!(entry.word_cap_at_submission, Some(cap));
        let serialized = serde_json::to_value(state.to_snapshot()).unwrap();
        let index = state
            .transcripts
            .iter()
            .position(|entry| entry.id == transcript_id)
            .unwrap();
        assert_eq!(
            serialized["transcripts"][index]["wordCapAtSubmission"].as_u64(),
            Some(u64::from(cap))
        );
        let snapshot: MatchSnapshotV1 = serde_json::from_value(serialized).unwrap();
        let restored = MatchAggregate::from_snapshot(snapshot).unwrap();
        assert_eq!(
            restored.transcripts[index].word_cap_at_submission,
            Some(cap)
        );
        let context = stage_context(&restored, transcript_id, "七维", "只看公开文本").unwrap();
        assert!(context
            .own_strategy
            .contains(&format!("\nword_cap={cap}\n")));
        assert!(context.own_strategy.contains("\nmetering_mode=words\n"));
    }

    #[test]
    fn words_scoring_uses_the_normal_speech_submission_allowance() {
        let mut state = ready_semantic_match();
        let event = state.schedule[state.cursor].clone();
        assert_eq!(event.word_cap, 720);
        state
            .word_budgets
            .as_mut()
            .unwrap()
            .affirmative_remaining_units = 3;
        state
            .handle(MatchCommand::SubmitCurrent {
                event_id: event.id,
                text: "甲乙丙丁".to_owned(),
                input_ms: 9_000_000,
            })
            .unwrap();
        let entry = state.transcripts.last().unwrap();
        assert_eq!(entry.effective_units, 3);
        let context = stage_context(&state, entry.id, "七维", "只看公开文本").unwrap();
        assert!(context.own_strategy.contains("\nword_cap=3\n"));
        assert_persisted_submission_cap(&state, entry.id, 3);
    }

    #[test]
    fn words_scoring_uses_the_exact_free_debate_submission_allowance() {
        let mut state = ready_semantic_match();
        state.cursor = state
            .schedule
            .iter()
            .position(|event| event.kind == StageKind::FreeDebate)
            .unwrap();
        assert_eq!(state.schedule[state.cursor].word_cap, 960);
        state
            .semantic_ruleset
            .as_mut()
            .unwrap()
            .clock_pools
            .iter_mut()
            .find(|pool| {
                pool.kind == crate::domain::ruleset_v2::ClockPoolKindV3::FreeDebate
                    && pool.side == Side::Affirmative
            })
            .unwrap()
            .unit_budget = Some(1_003);
        let id = state
            .submit_free_debate_turn(SlotId::Aff1, "甲乙丙".to_owned(), 9_000_000)
            .unwrap();
        let context = stage_context(&state, id, "七维", "只看公开文本").unwrap();
        assert!(context.own_strategy.contains("\nword_cap=1003\n"));
        assert!(context.own_strategy.contains("团队额度使用"));
        assert!(!context.own_strategy.contains("团队时间使用"));
        assert_persisted_submission_cap(&state, id, 1_003);
    }

    #[test]
    fn words_scoring_uses_the_cross_exam_answer_submission_allowance() {
        let mut state = ready_semantic_match();
        state.cursor = state
            .schedule
            .iter()
            .position(|event| event.kind == StageKind::CrossExam)
            .unwrap();
        let side = state.current_cross_exam_roles().unwrap().0.side();
        let budgets = state.word_budgets.as_mut().unwrap();
        match side {
            Side::Affirmative => budgets.affirmative_remaining_units = 7,
            Side::Negative => budgets.negative_remaining_units = 7,
        }
        state
            .submit_cross_exam_question("甲乙丙".to_owned(), 9_000_000)
            .unwrap();
        let id = state
            .commit_cross_exam_answer("丁戊".to_owned(), 8_000_000, None)
            .unwrap();
        let context = stage_context(&state, id, "七维", "只看公开文本").unwrap();
        assert!(context.own_strategy.contains("\nword_cap=4\n"));
        assert_persisted_submission_cap(&state, state.transcripts[0].id, 7);
        assert_persisted_submission_cap(&state, id, 4);
    }

    #[test]
    fn historical_words_transcript_without_submission_cap_uses_the_source_event_cap() {
        let mut state = ready_semantic_match();
        let event = state.schedule[state.cursor].clone();
        state
            .word_budgets
            .as_mut()
            .unwrap()
            .affirmative_remaining_units = 3;
        state
            .handle(MatchCommand::SubmitCurrent {
                event_id: event.id,
                text: "甲乙丙丁".to_owned(),
                input_ms: 9_000_000,
            })
            .unwrap();
        let id = state.transcripts[0].id;
        let mut serialized = serde_json::to_value(state.to_snapshot()).unwrap();
        serialized["transcripts"][0]
            .as_object_mut()
            .unwrap()
            .remove("wordCapAtSubmission");
        let snapshot: MatchSnapshotV1 = serde_json::from_value(serialized).unwrap();
        let restored = MatchAggregate::from_snapshot(snapshot).unwrap();
        assert_eq!(restored.transcripts[0].word_cap_at_submission, None);
        let context = stage_context(&restored, id, "七维", "只看公开文本").unwrap();
        assert!(context.own_strategy.contains("\nword_cap=720\n"));
        assert!(context.own_strategy.contains("\nmetering_mode=words\n"));
    }

    #[test]
    fn time_transcripts_keep_the_source_event_cap_and_time_rubric() {
        let mut state = ready_semantic_match();
        state.metering_mode = MatchResourceMode::Time;
        let event = state.schedule[state.cursor].clone();
        state
            .handle(MatchCommand::SubmitCurrent {
                event_id: event.id,
                text: "甲乙".to_owned(),
                input_ms: 1_000,
            })
            .unwrap();
        let entry = &state.transcripts[0];
        assert_eq!(entry.word_cap_at_submission, None);
        assert!(!serde_json::to_value(entry)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("wordCapAtSubmission"));
        let context = stage_context(&state, entry.id, "七维", "只看公开文本").unwrap();
        assert!(context.own_strategy.contains("\nword_cap=720\n"));
        assert!(context.own_strategy.contains("\nmetering_mode=time\n"));
        state.cursor = state
            .schedule
            .iter()
            .position(|event| event.kind == StageKind::FreeDebate)
            .unwrap();
        let id = state
            .submit_free_debate_turn(SlotId::Aff1, "甲乙".to_owned(), 1_000)
            .unwrap();
        let context = stage_context(&state, id, "七维", "只看公开文本").unwrap();
        assert!(context.own_strategy.contains("团队时间使用"));
        assert!(!context.own_strategy.contains("团队额度使用"));
        assert!(context.own_strategy.contains("\nword_cap=960\n"));
        assert_eq!(
            state.transcripts.last().unwrap().word_cap_at_submission,
            None
        );
        let review = match_review_context(&state, &[], "七维", &[]);
        assert!(review.own_strategy.contains("\nmetering_mode=time\n"));
    }

    #[test]
    fn scoring_context_contains_real_text_role_and_response_not_only_ids() {
        let mut state = MatchAggregate::new(
            Uuid::from_u128(1),
            MatchMode::Training,
            "是否应限制人工智能",
            "材料甲",
            240,
        );
        state
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(four_on_four_template()),
            })
            .unwrap();
        state.handle(MatchCommand::ConfirmRules).unwrap();
        state
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "拆题".into(),
            })
            .unwrap();
        state
            .handle(MatchCommand::AssignControl {
                user_side: Some(Side::Affirmative),
                assignments: (0..8)
                    .map(|index| DebaterAssignmentV1 {
                        schema_version: ASSIGNMENT_SCHEMA_VERSION.into(),
                        slot: crate::domain::ruleset::SlotId::from_index(index).unwrap(),
                        controller: Controller::Human,
                    })
                    .collect(),
            })
            .unwrap();
        state.handle(MatchCommand::ConfirmLineup).unwrap();
        state.handle(MatchCommand::MarkReady).unwrap();
        state.handle(MatchCommand::Start).unwrap();
        let event_id = state.schedule[0].id;
        state
            .handle(MatchCommand::SubmitCurrent {
                event_id,
                text: "真实的人类论证正文".into(),
                input_ms: 1000,
            })
            .unwrap();
        let id = state.transcripts[0].id;
        let context = stage_context(&state, id, "七维", "排除超限尾部").unwrap();
        assert!(context.own_strategy.contains("真实的人类论证正文"));
        assert!(context.own_strategy.contains("duty="));
        assert!(context.public_transcript[0].contains("text=真实的人类论证正文"));
        assert_ne!(context.public_transcript, vec![id.to_string()]);

        let review = match_review_context(
            &state,
            &["stage-result".to_owned()],
            "七维",
            &[id.to_string()],
        );
        assert!(review
            .own_strategy
            .contains(&format!("allowed_adjustment_transcript_ids={id}")));
        assert!(review
            .own_strategy
            .contains("AI teammate or opponent quality must never change"));
    }

    #[test]
    fn cross_exam_questions_and_answers_receive_distinct_rubrics() {
        let mut state = MatchAggregate::new(
            Uuid::from_u128(2),
            MatchMode::Training,
            "是否应限制人工智能",
            "材料甲",
            240,
        );
        state
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(four_on_four_template()),
            })
            .unwrap();
        state.handle(MatchCommand::ConfirmRules).unwrap();
        state
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "拆题".into(),
            })
            .unwrap();
        state
            .handle(MatchCommand::AssignControl {
                user_side: Some(Side::Affirmative),
                assignments: (0..8)
                    .map(|index| DebaterAssignmentV1 {
                        schema_version: ASSIGNMENT_SCHEMA_VERSION.into(),
                        slot: crate::domain::ruleset::SlotId::from_index(index).unwrap(),
                        controller: Controller::Human,
                    })
                    .collect(),
            })
            .unwrap();
        state.handle(MatchCommand::ConfirmLineup).unwrap();
        state.handle(MatchCommand::MarkReady).unwrap();
        state.handle(MatchCommand::Start).unwrap();
        let opening = state.schedule[0].id;
        state
            .handle(MatchCommand::SubmitCurrent {
                event_id: opening,
                text: "开场主张".into(),
                input_ms: 100,
            })
            .unwrap();
        state
            .submit_cross_exam_question("请说明风险边界".into(), 100)
            .unwrap();
        state
            .commit_cross_exam_answer("风险边界是影响生命安全".into(), 100, None)
            .unwrap();
        let question = &state.transcripts[1];
        let answer = &state.transcripts[2];
        let question_context = stage_context(&state, question.id, "七维", "只看公开文本").unwrap();
        let answer_context = stage_context(&state, answer.id, "七维", "只看公开文本").unwrap();
        assert!(question_context
            .own_strategy
            .contains("cross_exam_turn=question"));
        assert!(question_context.own_strategy.contains("漏洞捕捉"));
        assert!(answer_context
            .own_strategy
            .contains("cross_exam_turn=answer"));
        assert!(answer_context.own_strategy.contains("请说明风险边界"));
        assert!(answer_context.own_strategy.contains("正面回答"));
    }
}
