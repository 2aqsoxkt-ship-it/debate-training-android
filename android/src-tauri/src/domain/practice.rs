use crate::domain::events::Controller;
use crate::domain::match_engine::MatchAggregate;
use crate::domain::ruleset::SlotId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeContext {
    pub topic: String,
    pub materials: String,
    pub slot: SlotId,
    pub task: String,
    pub word_cap: u32,
    pub before_turns: Vec<(String, SlotId, String)>,
    pub original_text: String,
}

pub fn freeze(aggregate: &MatchAggregate, source: Uuid) -> Result<PracticeContext, String> {
    let index = aggregate
        .transcripts
        .iter()
        .position(|turn| turn.id == source)
        .ok_or("原发言不存在")?;
    let turn = &aggregate.transcripts[index];
    if turn.controller != Some(Controller::Human) {
        return Err("独立重练仅针对实际人工发言；旧存档作者不明时不猜测".to_owned());
    }
    let event = aggregate
        .schedule
        .iter()
        .find(|event| event.id == turn.event_id)
        .ok_or("原环节不存在")?;
    let stage_name = aggregate
        .ruleset
        .as_ref()
        .and_then(|r| r.stages.iter().find(|s| s.id == event.stage_id))
        .map(|s| s.name.clone())
        .unwrap_or_else(|| format!("{:?}", event.kind));
    let duty = aggregate
        .ruleset
        .as_ref()
        .and_then(|r| {
            if turn.slot.side() == crate::domain::ruleset::Side::Affirmative {
                r.affirmative_duties.get(turn.slot.index() % 4)
            } else {
                r.negative_duties.get(turn.slot.index() % 4)
            }
        })
        .cloned()
        .unwrap_or_default();
    let task = format!(
        "{stage_name} / {:?} · {:?} · 职责：{duty}。按此辩位原任务重新作答，不得引用后续比赛内容",
        turn.slot, turn.cross_exam_turn_kind
    );
    Ok(PracticeContext {
        topic: aggregate.topic.clone(),
        materials: aggregate.materials.clone(),
        slot: turn.slot,
        task,
        word_cap: turn.word_cap_at_submission.unwrap_or(event.word_cap),
        before_turns: aggregate.transcripts[..index]
            .iter()
            .map(|t| (t.id.to_string(), t.slot, t.effective_text.clone()))
            .collect(),
        original_text: turn.effective_text.clone(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn frozen_context_excludes_later_turns_and_candidate_tails() {
        let mut aggregate = crate::domain::match_engine::play_constructed_match();
        aggregate.transcripts[0].controller = Some(crate::domain::events::Controller::Human);
        aggregate.transcripts[0].full_text = "隐藏尾部".to_owned();
        let context = super::freeze(&aggregate, aggregate.transcripts[0].id).unwrap();
        assert!(context.before_turns.is_empty());
        assert!(!serde_json::to_string(&context)
            .unwrap()
            .contains("隐藏尾部"));
        aggregate.transcripts[0].controller = Some(crate::domain::events::Controller::Ai);
        assert!(super::freeze(&aggregate, aggregate.transcripts[0].id).is_err());
    }
}
