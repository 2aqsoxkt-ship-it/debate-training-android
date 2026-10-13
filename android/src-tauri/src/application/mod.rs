pub mod ai_turn;
#[cfg(test)]
mod complete_experience_acceptance;
pub mod cross_exam_service;
pub mod desk;
pub mod free_debate_service;
pub mod human_turn;
#[cfg(test)]
pub(crate) mod live_acceptance_support;
pub mod live_cross_exam_acceptance;
pub mod live_m2_acceptance;
pub mod live_m3_acceptance;
pub mod live_multi_role_cross_exam_acceptance;
pub mod live_opening_acceptance;
pub mod live_v3_full_match_acceptance;
pub mod model_settings;
pub mod practice_service;
pub mod scoring_service;
#[cfg(test)]
pub(crate) mod word_budget_acceptance;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::infrastructure::db::SqliteStore;

use cross_exam_service::CrossExamService;
use desk::Desk;

pub struct ActiveStream {
    pub cancel: Arc<AtomicBool>,
    pub request_id: String,
}

pub struct AppState {
    pub store: Mutex<SqliteStore>,
    pub desk: Mutex<Desk>,
    pub active_stream: Mutex<Option<ActiveStream>>,
    pub cross_exam: Mutex<CrossExamService>,
}
