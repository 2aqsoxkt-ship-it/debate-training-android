use crate::domain::debate_context::RolePacket;
use crate::domain::ruleset::{MatchMode, Side};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyPacket {
    pub affirmative: String,
    pub negative: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleStrategies {
    pub own: Option<String>,
    pub opponent: Option<String>,
}

pub fn visible_strategies(
    mode: MatchMode,
    user_side: Side,
    packet: &StrategyPacket,
) -> VisibleStrategies {
    let (own, opponent) = match user_side {
        Side::Affirmative => (&packet.affirmative, &packet.negative),
        Side::Negative => (&packet.negative, &packet.affirmative),
    };
    match mode {
        MatchMode::Training => VisibleStrategies {
            own: Some(own.clone()),
            opponent: Some(opponent.clone()),
        },
        MatchMode::Scoring => VisibleStrategies {
            own: Some(own.clone()),
            opponent: None,
        },
    }
}

pub fn visible_role_packets(
    mode: MatchMode,
    user_side: Side,
    packets: &[RolePacket],
) -> Vec<RolePacket> {
    match mode {
        MatchMode::Training => packets.to_vec(),
        MatchMode::Scoring => packets
            .iter()
            .filter(|packet| packet.slot.side() == user_side)
            .cloned()
            .collect(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Beginner,
    Standard,
    Contest,
}

impl Difficulty {
    pub fn constraint(self) -> &'static str {
        match self {
            Self::Beginner => "优先短句和单一论点",
            Self::Standard => "保持完整论证并回应对方上一轮",
            Self::Contest => "压缩铺垫，优先攻击对方定义和证据缺口",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{visible_role_packets, visible_strategies, Difficulty, StrategyPacket};
    use crate::domain::debate_context::RolePacket;
    use crate::domain::ruleset::{MatchMode, Side, SlotId};

    fn packet() -> StrategyPacket {
        StrategyPacket {
            affirmative: "正方私有".to_owned(),
            negative: "反方私有".to_owned(),
        }
    }

    fn role(slot: SlotId, text: &str) -> RolePacket {
        RolePacket {
            slot,
            identity: text.to_owned(),
            duty: "立论".to_owned(),
            main_argument: format!("{text}-主线"),
            backup_rebuttal: String::new(),
            cross_exam_target: String::new(),
            defense_floor: String::new(),
            summary_callback: String::new(),
        }
    }

    #[test]
    fn training_can_see_both_sides() {
        let visible = visible_strategies(MatchMode::Training, Side::Affirmative, &packet());
        assert_eq!(visible.own.as_deref(), Some("正方私有"));
        assert_eq!(visible.opponent.as_deref(), Some("反方私有"));
    }

    #[test]
    fn scoring_hides_the_opponent() {
        let visible = visible_strategies(MatchMode::Scoring, Side::Negative, &packet());
        assert_eq!(visible.own.as_deref(), Some("反方私有"));
        assert!(visible.opponent.is_none());
    }

    #[test]
    fn scoring_hides_opponent_role_packets() {
        let packets = vec![
            role(SlotId::Aff1, "正方一辩"),
            role(SlotId::Neg1, "反方一辩"),
        ];
        let visible = visible_role_packets(MatchMode::Scoring, Side::Affirmative, &packets);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].slot, SlotId::Aff1);
        assert!(!visible[0].main_argument.contains("反方"));
    }

    #[test]
    fn difficulty_changes_the_constraint_only() {
        assert_ne!(
            Difficulty::Beginner.constraint(),
            Difficulty::Contest.constraint()
        );
    }
}
