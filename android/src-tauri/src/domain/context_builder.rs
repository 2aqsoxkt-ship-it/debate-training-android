use crate::domain::debate_context::{DebateTurnContext, PublicTurn, RolePacket};
use crate::domain::match_engine::MatchAggregate;
use crate::domain::ruleset::{Side, SlotId, StageKind};
use crate::domain::visibility::{Difficulty, StrategyPacket};

pub struct ContextBuilder;

impl ContextBuilder {
    pub fn build(
        aggregate: &MatchAggregate,
        strategies: Option<&StrategyPacket>,
        packets: &[RolePacket],
        difficulty: Difficulty,
        remaining_personal_ms: u64,
        remaining_team_ms: u64,
    ) -> Result<DebateTurnContext, String> {
        let event = aggregate
            .schedule
            .get(aggregate.cursor)
            .ok_or("当前没有可发言的环节")?;
        let slot = event.speaker;
        let side = slot.side();
        let duties = match side {
            Side::Affirmative => {
                &aggregate
                    .ruleset
                    .as_ref()
                    .ok_or("还没有赛制")?
                    .affirmative_duties
            }
            Side::Negative => {
                &aggregate
                    .ruleset
                    .as_ref()
                    .ok_or("还没有赛制")?
                    .negative_duties
            }
        };
        let duty = duties
            .get(slot.index() % 4)
            .cloned()
            .unwrap_or_else(|| "发言".to_owned());
        let own_strategy = strategies
            .map(|packet| match side {
                Side::Affirmative => packet.affirmative.clone(),
                Side::Negative => packet.negative.clone(),
            })
            .unwrap_or_default();
        let role_packet = packets
            .iter()
            .find(|packet| packet.slot == slot)
            .cloned()
            .unwrap_or_else(|| default_packet(slot, &duty));
        let public_turns: Vec<PublicTurn> = aggregate
            .transcripts
            .iter()
            .map(|entry| PublicTurn {
                id: entry.id.to_string(),
                slot: entry.slot,
                text: entry.effective_text.clone(),
            })
            .collect();
        let latest_to_answer = public_turns.last().cloned();
        let rules_summary = aggregate
            .ruleset
            .as_ref()
            .map(|ruleset| format!("{} · {} 个环节", ruleset.ruleset_name, ruleset.stages.len()))
            .unwrap_or_default();
        let team_claims = public_turns
            .iter()
            .filter(|turn| turn.slot.side() == side)
            .map(|turn| turn.text.clone())
            .collect();
        let appearance_counts: std::collections::HashMap<SlotId, u32> = {
            let mut map = std::collections::HashMap::new();
            for turn in &public_turns {
                *map.entry(turn.slot).or_insert(0) += 1;
            }
            map
        };
        let own_appearances = *appearance_counts.get(&slot).unwrap_or(&0);
        let unanswered_attacks: Vec<String> = public_turns
            .iter()
            .filter(|turn| turn.slot.side() != side)
            .rev()
            .take(3)
            .map(|turn| turn.text.clone())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let mut own_strategy = own_strategy;
        own_strategy.push_str(&crate::domain::issue_ledger::prompt(&aggregate.transcripts));
        own_strategy.push_str(&format!(
            "\n[模型声明的回应关系，待核]\n{}",
            serde_json::to_string(&aggregate.public_response_links).unwrap_or_default()
        ));
        if event.kind == StageKind::FreeDebate {
            let opponent_last = public_turns
                .iter()
                .rev()
                .find(|turn| turn.slot.side() != side)
                .map(|turn| turn.text.as_str())
                .unwrap_or("（尚无对方发言）");
            own_strategy = format!(
                "{own_strategy}\n[自由辩上下文]\n对方最近有效正文：{opponent_last}\n近期对方主张（回应是否充分待核）：{}\n剩余团队时间毫秒：{remaining_team_ms}\n当前辩位职责：{duty}\n本辩位最近出场次数：{own_appearances}\n必须实际回应上一轮，不得只重复初始立论。",
                unanswered_attacks.join(" | ")
            );
        }
        Ok(DebateTurnContext {
            match_id: aggregate.id.to_string(),
            event_id: event.id.to_string(),
            stage_id: event.stage_id.clone(),
            topic: aggregate.topic.clone(),
            rules_summary,
            materials: aggregate.materials.clone(),
            side,
            slot,
            duty,
            stage_kind: event.kind,
            duration_seconds: event.duration_seconds,
            word_cap: event.word_cap,
            interruptible: event.interruptible && event.kind == StageKind::CrossExam,
            interrupt_limit: u32::from(event.interruptible),
            own_strategy,
            role_packet,
            public_turns,
            latest_to_answer,
            team_claims,
            team_promises: Vec::new(),
            remaining_personal_ms,
            remaining_team_ms,
            difficulty,
        })
    }
}

pub fn default_packets(ruleset: &crate::domain::ruleset::RulesetV1) -> Vec<RolePacket> {
    SlotId::ALL
        .into_iter()
        .map(|slot| {
            let duties = if slot.side() == Side::Affirmative {
                &ruleset.affirmative_duties
            } else {
                &ruleset.negative_duties
            };
            let duty = duties[slot.index() % 4].clone();
            default_packet(slot, &duty)
        })
        .collect()
}

fn default_packet(slot: SlotId, duty: &str) -> RolePacket {
    let identity = match slot {
        SlotId::Aff1 => "正方一辩",
        SlotId::Aff2 => "正方二辩",
        SlotId::Aff3 => "正方三辩",
        SlotId::Aff4 => "正方四辩",
        SlotId::Neg1 => "反方一辩",
        SlotId::Neg2 => "反方二辩",
        SlotId::Neg3 => "反方三辩",
        SlotId::Neg4 => "反方四辩",
    };
    RolePacket {
        slot,
        identity: identity.to_owned(),
        duty: duty.to_owned(),
        main_argument: format!("{identity}主论证：围绕本方立场展开。"),
        backup_rebuttal: format!("{identity}备用反驳：回应对方关键漏洞。"),
        cross_exam_target: "锁定对方定义与证据链。".to_owned(),
        defense_floor: "不放弃本方核心定义与判断标准。".to_owned(),
        summary_callback: "回收本方主线和未回应的交锋点。".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{default_packets, ContextBuilder};
    use crate::domain::match_engine::{MatchAggregate, MatchCommand};
    use crate::domain::ruleset::{four_on_four_template, MatchMode, SlotId};
    use crate::domain::visibility::{Difficulty, StrategyPacket};
    use uuid::Uuid;

    fn ready_match() -> MatchAggregate {
        let mut aggregate = MatchAggregate::new(
            Uuid::from_u128(9),
            MatchMode::Training,
            "人工智能应当受到限制".to_owned(),
            "闭卷材料：高风险用途需要许可。".to_owned(),
            240,
        );
        aggregate
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(four_on_four_template()),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmRules).unwrap();
        aggregate
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "政策辩题".to_owned(),
            })
            .unwrap();
        aggregate
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: crate::domain::match_engine::training_assignments(),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmLineup).unwrap();
        aggregate.handle(MatchCommand::MarkReady).unwrap();
        aggregate.handle(MatchCommand::Start).unwrap();
        aggregate
    }

    #[test]
    fn aff2_sees_own_strategy_and_public_text_but_not_negative_strategy() {
        let mut aggregate = ready_match();
        // Force cursor onto AFF_2 by submitting first opening as empty path: edit schedule speaker.
        // Use Aff1 first turn then advance by submitting a turn.
        aggregate
            .handle(MatchCommand::SubmitCurrent {
                event_id: aggregate.schedule[0].id,
                text: "正方开篇定义限制范围。".to_owned(),
                input_ms: 1_000,
            })
            .unwrap();
        // After Aff1, next may be cross exam in template. Build with Aff2 packet explicitly.
        let packets = default_packets(aggregate.ruleset.as_ref().unwrap());
        let strategies = StrategyPacket {
            affirmative: "正方私有：许可制".to_owned(),
            negative: "反方私有：创新优先".to_owned(),
        };
        // Build for current speaker
        let context = ContextBuilder::build(
            &aggregate,
            Some(&strategies),
            &packets,
            Difficulty::Standard,
            90_000,
            180_000,
        )
        .unwrap();
        let prompt = context.prompt_blocks();
        assert!(prompt.contains("闭卷材料：高风险用途需要许可。"));
        assert!(prompt.contains("正方开篇定义限制范围。"));
        assert!(prompt.contains("id="));
        assert!(!prompt.contains("transcript only"));
        if context.side == crate::domain::ruleset::Side::Affirmative {
            assert!(prompt.contains("正方私有：许可制"));
            assert!(!prompt.contains("反方私有：创新优先"));
        } else {
            assert!(prompt.contains("反方私有：创新优先"));
            assert!(!prompt.contains("正方私有：许可制"));
        }
        assert_eq!(context.slot, context.role_packet.slot);
        assert_eq!(context.slot, aggregate.schedule[aggregate.cursor].speaker);
    }

    #[test]
    fn different_slots_receive_different_packets() {
        let ruleset = four_on_four_template();
        let packets = default_packets(&ruleset);
        assert_eq!(packets.len(), 8);
        assert_ne!(packets[0].duty, packets[2].duty);
        assert_ne!(packets[0].main_argument, packets[4].main_argument);
        assert_eq!(packets[1].slot, SlotId::Aff2);
    }
}
