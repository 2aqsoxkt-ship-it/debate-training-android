use crate::domain::debate_context::RolePacket;
use crate::domain::ruleset::{RulesetV1, Side, SlotId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicBrief {
    pub summary: String,
    pub affirmative_burden: String,
    pub negative_burden: String,
    pub contentions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBrief {
    pub thesis: String,
    pub arguments: Vec<String>,
    pub risks: Vec<String>,
}

pub fn assign_role_packets(
    ruleset: &RulesetV1,
    analysis: &TopicBrief,
    affirmative: &SideBrief,
    negative: &SideBrief,
) -> Vec<RolePacket> {
    SlotId::ALL
        .into_iter()
        .map(|slot| {
            let side = slot.side();
            let duties = if side == Side::Affirmative {
                &ruleset.affirmative_duties
            } else {
                &ruleset.negative_duties
            };
            let duty = duties
                .get(slot.index() % 4)
                .cloned()
                .unwrap_or_else(|| "发言".to_owned());
            let strategy = if side == Side::Affirmative {
                affirmative
            } else {
                negative
            };
            packet_for(slot, &duty, analysis, strategy)
        })
        .collect()
}

fn packet_for(slot: SlotId, duty: &str, analysis: &TopicBrief, strategy: &SideBrief) -> RolePacket {
    let identity = identity_of(slot);
    let argument = strategy
        .arguments
        .get(slot.index() % 4)
        .cloned()
        .or_else(|| strategy.arguments.first().cloned())
        .unwrap_or_else(|| strategy.thesis.clone());
    let risk = if strategy.risks.is_empty() {
        "对方偷换定义".to_owned()
    } else {
        strategy.risks[slot.index() % strategy.risks.len()].clone()
    };
    let contention = if analysis.contentions.is_empty() {
        analysis.summary.clone()
    } else {
        analysis.contentions[slot.index() % analysis.contentions.len()].clone()
    };
    let burden = if slot.side() == Side::Affirmative {
        &analysis.affirmative_burden
    } else {
        &analysis.negative_burden
    };
    RolePacket {
        slot,
        identity: identity.to_owned(),
        duty: duty.to_owned(),
        main_argument: format!("{identity}主线：{argument}。服务举证责任：{burden}"),
        backup_rebuttal: format!("{identity}备用反驳：针对「{risk}」，回扣争点「{contention}」。"),
        cross_exam_target: format!("质询目标：逼对方交代「{contention}」下的定义与证据。"),
        defense_floor: format!("底线：不放弃「{}」。", strategy.thesis),
        summary_callback: format!(
            "总结回调：回收本方主张「{}」与未回应的「{contention}」。",
            strategy.thesis
        ),
    }
}

fn identity_of(slot: SlotId) -> &'static str {
    match slot {
        SlotId::Aff1 => "正方一辩",
        SlotId::Aff2 => "正方二辩",
        SlotId::Aff3 => "正方三辩",
        SlotId::Aff4 => "正方四辩",
        SlotId::Neg1 => "反方一辩",
        SlotId::Neg2 => "反方二辩",
        SlotId::Neg3 => "反方三辩",
        SlotId::Neg4 => "反方四辩",
    }
}

#[cfg(test)]
mod tests {
    use super::{assign_role_packets, SideBrief, TopicBrief};
    use crate::domain::ruleset::{four_on_four_template, SlotId};

    #[test]
    fn eight_packets_differ_by_slot_and_side() {
        let ruleset = four_on_four_template();
        let analysis = TopicBrief {
            summary: "政策辩题".to_owned(),
            affirmative_burden: "证明应当限制".to_owned(),
            negative_burden: "证明不应全面禁止".to_owned(),
            contentions: vec!["安全".to_owned(), "创新".to_owned()],
        };
        let affirmative = SideBrief {
            thesis: "许可制".to_owned(),
            arguments: vec!["风险分层".to_owned(), "责任归属".to_owned()],
            risks: vec!["定义过宽".to_owned()],
        };
        let negative = SideBrief {
            thesis: "创新优先".to_owned(),
            arguments: vec!["误伤创新".to_owned(), "监管成本".to_owned()],
            risks: vec!["忽视外部性".to_owned()],
        };
        let packets = assign_role_packets(&ruleset, &analysis, &affirmative, &negative);
        assert_eq!(packets.len(), 8);
        assert_eq!(packets[0].slot, SlotId::Aff1);
        assert_eq!(packets[4].slot, SlotId::Neg1);
        assert!(packets[0].defense_floor.contains("许可制"));
        assert!(packets[4].defense_floor.contains("创新优先"));
        assert_ne!(packets[0].main_argument, packets[4].main_argument);
        assert_ne!(packets[0].main_argument, packets[1].main_argument);
    }
}
