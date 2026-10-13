use uuid::Uuid;

use crate::domain::match_engine::MatchAggregate;
use crate::error::{AppError, ErrorCode};

pub trait MatchRepository {
    fn save(
        &mut self,
        profile_id: Uuid,
        state: &MatchAggregate,
        saved_at: &str,
    ) -> Result<(), AppError>;

    fn load(&self, id: Uuid) -> Result<MatchAggregate, AppError>;
}

pub fn require_utc_timestamp(value: &str) -> Result<(), AppError> {
    let bytes = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|slice| slice.iter().all(|byte| byte.is_ascii_digit()))
    };
    let valid = bytes.len() == 20
        && digits(0..4)
        && bytes.get(4) == Some(&b'-')
        && digits(5..7)
        && bytes.get(7) == Some(&b'-')
        && digits(8..10)
        && bytes.get(10) == Some(&b'T')
        && digits(11..13)
        && bytes.get(13) == Some(&b':')
        && digits(14..16)
        && bytes.get(16) == Some(&b':')
        && digits(17..19)
        && bytes.get(19) == Some(&b'Z');
    if valid {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::InvalidArgument,
            "timestamp must be UTC",
        ))
    }
}
