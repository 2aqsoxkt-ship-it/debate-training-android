use crate::application::desk::Desk;
use crate::domain::ruleset::StageKind;

pub(crate) fn advance_to_next_cross_exam(desk: &mut Desk) -> Result<(), String> {
    let remaining_events = desk
        .aggregate
        .as_ref()
        .map(|aggregate| aggregate.schedule.len().saturating_sub(aggregate.cursor))
        .ok_or_else(|| "验收赛程尚未创建".to_owned())?;

    for _ in 0..remaining_events {
        let current_kind = desk
            .aggregate
            .as_ref()
            .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
            .map(|event| event.kind)
            .ok_or_else(|| "验收赛程已结束，未找到质询环节".to_owned())?;
        if current_kind == StageKind::CrossExam {
            return Ok(());
        }
        desk.submit("验收推进到质询。".to_owned(), 1_000, 0, 0, 0, false)?;
    }

    Err("验收赛程中没有可到达的质询环节".to_owned())
}

pub(crate) fn retry_unavailable<T>(
    label: &str,
    mut operation: impl FnMut() -> Result<T, String>,
) -> Result<T, String> {
    const MAX_ATTEMPTS: u8 = 3;
    for attempt in 1..=MAX_ATTEMPTS {
        match operation() {
            Err(error)
                if error.to_ascii_lowercase().contains("unavailable") && attempt < MAX_ATTEMPTS =>
            {
                eprintln!(
                    "{label}: DeepSeek unavailable on attempt {attempt}/{MAX_ATTEMPTS}; retrying"
                );
            }
            result => return result,
        }
    }
    unreachable!("the final retry always returns")
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{advance_to_next_cross_exam, retry_unavailable};
    use crate::application::desk::Desk;
    use crate::domain::ruleset::{MatchMode, Side, StageKind};
    use crate::domain::visibility::Difficulty;

    #[test]
    fn default_template_fixture_advances_from_opening_to_team_cross_exam() {
        let mut desk = Desk::new();
        desk.profile_id = Some(Uuid::from_u128(0xCE04));
        desk.open_template(
            MatchMode::Training,
            "人工智能应当受到限制".to_owned(),
            "高风险用途需要许可并保留人工复核。".to_owned(),
            Some(Side::Affirmative),
            &[0],
            Difficulty::Standard,
        )
        .expect("template");
        desk.confirm_rules().expect("confirm rules");
        desk.analyze().expect("offline analysis");
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .expect("ready");
        desk.start().expect("start");

        advance_to_next_cross_exam(&mut desk).expect("advance to cross exam");

        assert_eq!(
            desk.aggregate
                .as_ref()
                .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
                .map(|event| event.kind),
            Some(StageKind::CrossExam)
        );
        assert_eq!(desk.aggregate.as_ref().expect("aggregate").cursor, 1);
    }

    #[test]
    fn live_retry_is_bounded_and_only_retries_unavailable() {
        let mut attempts = 0;
        let result = retry_unavailable("test", || {
            attempts += 1;
            if attempts < 3 {
                Err("Unavailable".to_owned())
            } else {
                Ok("ready")
            }
        });
        assert_eq!(result.as_deref(), Ok("ready"));
        assert_eq!(attempts, 3);

        let mut coded_attempts = 0;
        let coded = retry_unavailable("test", || {
            coded_attempts += 1;
            if coded_attempts < 2 {
                Err("E_UNAVAILABLE: model request failed".to_owned())
            } else {
                Ok("ready")
            }
        });
        assert_eq!(coded.as_deref(), Ok("ready"));
        assert_eq!(coded_attempts, 2);

        let mut non_retryable_attempts = 0;
        let error = retry_unavailable::<()>("test", || {
            non_retryable_attempts += 1;
            Err("InvalidArgument".to_owned())
        })
        .expect_err("non-retryable errors must surface immediately");
        assert_eq!(error, "InvalidArgument");
        assert_eq!(non_retryable_attempts, 1);
    }
}
