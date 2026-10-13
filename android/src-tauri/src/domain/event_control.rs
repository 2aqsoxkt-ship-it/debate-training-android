//! Pending event controller overrides for training-mode takeover of unstarted events.

use crate::domain::events::Controller;
use crate::domain::match_engine::MatchPhase;
use crate::domain::ruleset::{MatchMode, Side, SlotId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventControlError {
    NotTraining,
    EventStarted,
    CrossTeam,
    NeedsConfirmation,
    InvalidIndex,
}

impl EventControlError {
    pub fn message(self) -> &'static str {
        match self {
            Self::NotTraining => "评分模式不能自由改对方控制权",
            Self::EventStarted => "已经开始的环节不能回改控制权",
            Self::CrossTeam => "评分模式不能接管对方辩位",
            Self::NeedsConfirmation => "临时接管需要二次确认",
            Self::InvalidIndex => "环节编号无效",
        }
    }
}

/// Returns whether controller of schedule index may still be changed.
pub fn can_change_controller(cursor: usize, index: usize, phase: MatchPhase) -> bool {
    match phase {
        MatchPhase::ControlAssigned | MatchPhase::LineupConfirmed | MatchPhase::Ready => true,
        MatchPhase::InProgress => index > cursor,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn assign_controller(
    mode: MatchMode,
    user_side: Option<Side>,
    slot: SlotId,
    to_human: bool,
    index: usize,
    cursor: usize,
    phase: MatchPhase,
    second_confirmed: bool,
) -> Result<(Controller, bool), EventControlError> {
    if !can_change_controller(cursor, index, phase) {
        return Err(EventControlError::EventStarted);
    }
    match mode {
        MatchMode::Training => Ok((
            if to_human {
                Controller::Human
            } else {
                Controller::Ai
            },
            false,
        )),
        MatchMode::Scoring => {
            let side = user_side.ok_or(EventControlError::CrossTeam)?;
            if slot.side() != side {
                return Err(EventControlError::CrossTeam);
            }
            if to_human && !second_confirmed {
                return Err(EventControlError::NeedsConfirmation);
            }
            Ok((
                if to_human {
                    Controller::Human
                } else {
                    Controller::Ai
                },
                to_human,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{assign_controller, can_change_controller, EventControlError};
    use crate::domain::events::Controller;
    use crate::domain::match_engine::MatchPhase;
    use crate::domain::ruleset::{MatchMode, Side, SlotId};

    #[test]
    fn training_can_flip_unstarted_only() {
        assert!(!can_change_controller(2, 2, MatchPhase::InProgress));
        assert!(can_change_controller(2, 3, MatchPhase::InProgress));
        assert!(!can_change_controller(2, 1, MatchPhase::InProgress));
        let (controller, non_standard) = assign_controller(
            MatchMode::Training,
            Some(Side::Affirmative),
            SlotId::Neg2,
            true,
            3,
            2,
            MatchPhase::InProgress,
            false,
        )
        .unwrap();
        assert_eq!(controller, Controller::Human);
        assert!(!non_standard);
    }

    #[test]
    fn scoring_rejects_cross_team() {
        let err = assign_controller(
            MatchMode::Scoring,
            Some(Side::Affirmative),
            SlotId::Neg1,
            true,
            1,
            0,
            MatchPhase::InProgress,
            true,
        )
        .expect_err("cross");
        assert_eq!(err, EventControlError::CrossTeam);
    }

    #[test]
    fn scoring_takeover_needs_second_confirm() {
        let err = assign_controller(
            MatchMode::Scoring,
            Some(Side::Affirmative),
            SlotId::Aff2,
            true,
            1,
            0,
            MatchPhase::InProgress,
            false,
        )
        .expect_err("confirm");
        assert_eq!(err, EventControlError::NeedsConfirmation);
    }
}
