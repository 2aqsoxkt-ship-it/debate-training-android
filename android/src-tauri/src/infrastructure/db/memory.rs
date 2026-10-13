use std::collections::BTreeMap;

use uuid::Uuid;

use super::repository::{require_utc_timestamp, MatchRepository};
use crate::domain::match_engine::MatchAggregate;
use crate::error::{AppError, ErrorCode};

#[derive(Debug, Default)]
pub struct MemoryMatchRepository {
    records: BTreeMap<Uuid, MatchAggregate>,
}

impl MemoryMatchRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MatchRepository for MemoryMatchRepository {
    fn save(
        &mut self,
        _profile_id: Uuid,
        state: &MatchAggregate,
        saved_at: &str,
    ) -> Result<(), AppError> {
        require_utc_timestamp(saved_at)?;
        self.records.insert(state.id, state.clone());
        Ok(())
    }

    fn load(&self, id: Uuid) -> Result<MatchAggregate, AppError> {
        self.records
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "match was not found"))
    }
}
