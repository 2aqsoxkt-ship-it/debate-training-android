use uuid::Uuid;

use crate::domain::ruleset::{expand_schedule, RulesetV1, ScheduleError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesProblem {
    NegativeDuration,
    NonPositiveDuration,
    FreeDebateInterruptible,
    InterruptOutsideCrossExam,
    CrossExamMissingRoles,
    RepeatNotFinite,
    EmptySchedule,
    UnknownSlot,
}

pub fn check_duration(seconds: i64) -> Result<(), RulesProblem> {
    if seconds < 0 {
        return Err(RulesProblem::NegativeDuration);
    }
    if seconds == 0 {
        return Err(RulesProblem::NonPositiveDuration);
    }
    Ok(())
}

pub fn check_ruleset(ruleset: &RulesetV1) -> Result<(), RulesProblem> {
    for stage in &ruleset.stages {
        check_duration(i64::from(stage.duration_seconds))?;
    }
    let mut salt = 1_u128;
    expand_schedule(ruleset, 240, || {
        salt += 1;
        Uuid::from_u128(salt)
    })
    .map(|_| ())
    .map_err(map_schedule)
}

fn map_schedule(error: ScheduleError) -> RulesProblem {
    match error {
        ScheduleError::NonPositiveDuration => RulesProblem::NonPositiveDuration,
        ScheduleError::FreeDebateInterruptible => RulesProblem::FreeDebateInterruptible,
        ScheduleError::InterruptOutsideCrossExam => RulesProblem::InterruptOutsideCrossExam,
        ScheduleError::CrossExamMissingRoles => RulesProblem::CrossExamMissingRoles,
        ScheduleError::RepeatNotFinite => RulesProblem::RepeatNotFinite,
        ScheduleError::EmptySchedule => RulesProblem::EmptySchedule,
        ScheduleError::UnknownSlot => RulesProblem::UnknownSlot,
    }
}

#[cfg(test)]
mod tests {
    use super::{check_duration, check_ruleset, RulesProblem};
    use crate::domain::ruleset::{
        four_on_four_template, sample_cross_exam, sample_ruleset, ClockOwner, StageKind,
    };

    #[test]
    fn template_is_executable() {
        assert!(check_ruleset(&four_on_four_template()).is_ok());
    }

    #[test]
    fn rejects_negative_and_zero_duration() {
        assert_eq!(check_duration(-1), Err(RulesProblem::NegativeDuration));
        assert_eq!(check_duration(0), Err(RulesProblem::NonPositiveDuration));
    }

    #[test]
    fn rejects_unbounded_repeat_and_missing_cross_exam_roles() {
        let mut ruleset = sample_ruleset();
        ruleset.stages[1].repeat = 0;
        assert_eq!(check_ruleset(&ruleset), Err(RulesProblem::RepeatNotFinite));
        ruleset.stages[1].repeat = 33;
        assert_eq!(check_ruleset(&ruleset), Err(RulesProblem::RepeatNotFinite));

        let mut cross = sample_cross_exam();
        cross.questioner = None;
        cross.respondent = None;
        cross.clock_owner = ClockOwner::Individual;
        let mut ruleset = sample_ruleset();
        ruleset.stages = vec![cross];
        assert_eq!(
            check_ruleset(&ruleset),
            Err(RulesProblem::CrossExamMissingRoles)
        );
    }

    #[test]
    fn free_debate_interrupt_cannot_be_confirmed() {
        let mut ruleset = sample_ruleset();
        ruleset.stages[1].interruptible = true;
        ruleset.stages[1].kind = StageKind::FreeDebate;
        assert_eq!(
            check_ruleset(&ruleset),
            Err(RulesProblem::FreeDebateInterruptible)
        );
    }

    #[test]
    fn cross_exam_clock_must_be_a_participant() {
        let mut cross = sample_cross_exam();
        cross.clock_owner = ClockOwner::Shared;
        let mut ruleset = sample_ruleset();
        ruleset.stages = vec![cross];
        assert!(check_ruleset(&ruleset).is_ok());
    }
}
