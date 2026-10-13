use serde::{Deserialize, Serialize};

use crate::domain::events::{Controller, CrossExamExchangeStatus};
use crate::domain::match_engine::{MatchAggregate, MatchPhase};
use crate::domain::ruleset::{SlotId, StageKind};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlScope {
    #[default]
    Turn,
    Match,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlOverride {
    pub window_id: String,
    pub slot: SlotId,
    pub controller: Controller,
}

pub fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl MatchAggregate {
    /// Stable through controller toggles and question -> answer, changes on
    /// formal submission/next round. Never use wall-clock time as identity.
    pub fn control_window_id(&self) -> String {
        if self.phase != MatchPhase::InProgress {
            return String::new();
        }
        let Some(event) = self.schedule.get(self.cursor) else {
            return String::new();
        };
        let serial = if event.kind == StageKind::CrossExam {
            u64::from(
                self.cross_exam_sessions
                    .iter()
                    .find(|session| session.event_id == event.id)
                    .map_or(1, |session| session.round_number),
            )
        } else {
            self.transcripts.len() as u64
        };
        format!("{}:{}:{serial}", self.id, event.id)
    }

    pub fn effective_controller(&self, slot: SlotId) -> Controller {
        let window = self.control_window_id();
        if let Some(item) = self
            .current_control_overrides
            .iter()
            .rev()
            .find(|item| !window.is_empty() && item.window_id == window && item.slot == slot)
        {
            return item.controller;
        }
        if let Some(event) = self
            .schedule
            .get(self.cursor)
            .filter(|event| event.kind == StageKind::CrossExam)
        {
            if let Some(exchange) = self.cross_exam_exchanges.iter().find(|exchange| {
                exchange.event_id == event.id
                    && matches!(
                        exchange.status,
                        CrossExamExchangeStatus::Selected
                            | CrossExamExchangeStatus::QuestionSubmitted
                    )
            }) {
                if slot == exchange.questioner {
                    if let Some(controller) = exchange.questioner_controller {
                        return controller;
                    }
                }
                if slot == exchange.respondent {
                    if let Some(controller) = exchange.respondent_controller {
                        return controller;
                    }
                }
            }
        }
        self.assignments[slot.index()].controller
    }

    pub fn set_turn_override(&mut self, slot: SlotId, controller: Controller) {
        let window_id = self.control_window_id();
        self.current_control_overrides
            .retain(|item| item.window_id == window_id && item.slot != slot);
        self.current_control_overrides.push(ControlOverride {
            window_id,
            slot,
            controller,
        });
    }

    pub fn active_overrides(&self) -> Vec<ControlOverride> {
        let window = self.control_window_id();
        self.current_control_overrides
            .iter()
            .filter(|item| !window.is_empty() && item.window_id == window)
            .cloned()
            .collect()
    }
}
