use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::domain::timing::word_cap;

pub const RULESET_SCHEMA_VERSION: &str = "ruleset.v1";
pub const MAX_STAGE_REPEAT: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SlotId {
    #[serde(rename = "AFF_1", alias = "Aff1", alias = "aff_1", alias = "aff1")]
    Aff1,
    #[serde(rename = "AFF_2", alias = "Aff2", alias = "aff_2", alias = "aff2")]
    Aff2,
    #[serde(rename = "AFF_3", alias = "Aff3", alias = "aff_3", alias = "aff3")]
    Aff3,
    #[serde(rename = "AFF_4", alias = "Aff4", alias = "aff_4", alias = "aff4")]
    Aff4,
    #[serde(rename = "NEG_1", alias = "Neg1", alias = "neg_1", alias = "neg1")]
    Neg1,
    #[serde(rename = "NEG_2", alias = "Neg2", alias = "neg_2", alias = "neg2")]
    Neg2,
    #[serde(rename = "NEG_3", alias = "Neg3", alias = "neg_3", alias = "neg3")]
    Neg3,
    #[serde(rename = "NEG_4", alias = "Neg4", alias = "neg_4", alias = "neg4")]
    Neg4,
}

impl SlotId {
    pub const ALL: [Self; 8] = [
        Self::Aff1,
        Self::Aff2,
        Self::Aff3,
        Self::Aff4,
        Self::Neg1,
        Self::Neg2,
        Self::Neg3,
        Self::Neg4,
    ];

    pub fn index(self) -> usize {
        match self {
            Self::Aff1 => 0,
            Self::Aff2 => 1,
            Self::Aff3 => 2,
            Self::Aff4 => 3,
            Self::Neg1 => 4,
            Self::Neg2 => 5,
            Self::Neg3 => 6,
            Self::Neg4 => 7,
        }
    }

    pub fn side(self) -> Side {
        match self {
            Self::Aff1 | Self::Aff2 | Self::Aff3 | Self::Aff4 => Side::Affirmative,
            Self::Neg1 | Self::Neg2 | Self::Neg3 | Self::Neg4 => Side::Negative,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Affirmative,
    Negative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    Training,
    Scoring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    Opening,
    CrossExam,
    FreeDebate,
    Summary,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockOwner {
    Individual,
    Team,
    Questioner,
    Respondent,
    Shared,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageDefinitionV1 {
    pub id: String,
    pub name: String,
    pub kind: StageKind,
    #[serde(default, deserialize_with = "null_default_vec")]
    pub speakers: Vec<SlotId>,
    #[serde(default, deserialize_with = "nullable_u32_vec")]
    pub speaker_durations: Vec<u32>,
    pub clock_owner: ClockOwner,
    #[serde(default, deserialize_with = "null_default_u32")]
    pub duration_seconds: u32,
    #[serde(default, deserialize_with = "null_default_bool")]
    pub early_end_allowed: bool,
    #[serde(default, deserialize_with = "null_default_bool")]
    pub follow_up_allowed: bool,
    #[serde(default, deserialize_with = "null_default_bool")]
    pub interruptible: bool,
    #[serde(default, deserialize_with = "null_default_u32")]
    pub interrupt_limit: u32,
    pub questioner: Option<SlotId>,
    pub respondent: Option<SlotId>,
    #[serde(
        default,
        deserialize_with = "null_default_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub eligible_questioners: Vec<SlotId>,
    #[serde(
        default,
        deserialize_with = "null_default_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub eligible_respondents: Vec<SlotId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_questioning_side: Option<Side>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cross_exam_rounds: Option<u32>,
    #[serde(default, deserialize_with = "null_default_u32")]
    pub weight: u32,
    #[serde(default, deserialize_with = "null_default_u32")]
    pub repeat: u32,
}

fn null_default_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<u32>::deserialize(deserializer)?.unwrap_or(0))
}

fn null_default_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<bool>::deserialize(deserializer)?.unwrap_or(false))
}

fn nullable_u32_vec<'de, D>(deserializer: D) -> Result<Vec<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<Option<u32>>>::deserialize(deserializer)?
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.unwrap_or(0))
        .collect())
}

fn null_default_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesetV1 {
    pub schema_version: String,
    pub ruleset_name: String,
    pub source_text: String,
    #[serde(deserialize_with = "four_duties")]
    pub affirmative_duties: [String; 4],
    #[serde(deserialize_with = "four_duties")]
    pub negative_duties: [String; 4],
    #[serde(default, deserialize_with = "null_default_vec")]
    pub stages: Vec<StageDefinitionV1>,
}

fn four_duties<'de, D>(deserializer: D) -> Result<[String; 4], D::Error>
where
    D: Deserializer<'de>,
{
    let mut values = Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default();
    while values.len() < 4 {
        let index = values.len() + 1;
        values.push(format!("辩位{index}"));
    }
    values.truncate(4);
    Ok([
        values[0].clone(),
        values[1].clone(),
        values[2].clone(),
        values[3].clone(),
    ])
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledEvent {
    pub id: Uuid,
    pub stage_id: String,
    pub kind: StageKind,
    pub speaker: SlotId,
    pub duration_seconds: u32,
    pub interruptible: bool,
    pub word_cap: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleError {
    NonPositiveDuration,
    FreeDebateInterruptible,
    InterruptOutsideCrossExam,
    CrossExamMissingRoles,
    RepeatNotFinite,
    EmptySchedule,
    UnknownSlot,
}

pub fn expand_schedule(
    ruleset: &RulesetV1,
    speech_rate_per_minute: u32,
    mut next_id: impl FnMut() -> Uuid,
) -> Result<Vec<ScheduledEvent>, ScheduleError> {
    if ruleset.stages.is_empty() {
        return Err(ScheduleError::EmptySchedule);
    }
    let mut events = Vec::new();
    for stage in &ruleset.stages {
        if stage.duration_seconds == 0 {
            return Err(ScheduleError::NonPositiveDuration);
        }
        if stage.repeat == 0 || stage.repeat > MAX_STAGE_REPEAT {
            return Err(ScheduleError::RepeatNotFinite);
        }
        if stage.speakers.is_empty() {
            return Err(ScheduleError::UnknownSlot);
        }
        if stage.kind == StageKind::FreeDebate && stage.interruptible {
            return Err(ScheduleError::FreeDebateInterruptible);
        }
        if stage.interruptible && stage.kind != StageKind::CrossExam {
            return Err(ScheduleError::InterruptOutsideCrossExam);
        }
        if stage.kind == StageKind::CrossExam
            && (stage.questioner.is_none()
                || stage.respondent.is_none()
                || !matches!(
                    stage.clock_owner,
                    ClockOwner::Questioner | ClockOwner::Respondent | ClockOwner::Shared
                ))
        {
            return Err(ScheduleError::CrossExamMissingRoles);
        }
        for _ in 0..stage.repeat {
            for (offset, speaker) in stage.speakers.iter().enumerate() {
                let seconds = stage
                    .speaker_durations
                    .get(offset)
                    .copied()
                    .filter(|seconds| *seconds > 0)
                    .unwrap_or(stage.duration_seconds);
                if seconds == 0 {
                    return Err(ScheduleError::NonPositiveDuration);
                }
                events.push(ScheduledEvent {
                    id: next_id(),
                    stage_id: stage.id.clone(),
                    kind: stage.kind,
                    speaker: *speaker,
                    duration_seconds: seconds,
                    interruptible: stage.interruptible && stage.kind == StageKind::CrossExam,
                    word_cap: word_cap(seconds, speech_rate_per_minute),
                });
            }
        }
    }
    if events.is_empty() {
        return Err(ScheduleError::EmptySchedule);
    }
    Ok(events)
}

pub fn sample_ruleset() -> RulesetV1 {
    RulesetV1 {
        schema_version: RULESET_SCHEMA_VERSION.to_owned(),
        ruleset_name: "四对四示例".to_owned(),
        source_text: "一辩陈词三分钟，自由辩交替，四辩总结。".to_owned(),
        affirmative_duties: [
            "立论".to_owned(),
            "展开".to_owned(),
            "攻辩".to_owned(),
            "总结".to_owned(),
        ],
        negative_duties: [
            "立论".to_owned(),
            "展开".to_owned(),
            "攻辩".to_owned(),
            "总结".to_owned(),
        ],
        stages: vec![opening_stage(), free_debate_stage(), summary_stage()],
    }
}

pub fn four_on_four_template() -> RulesetV1 {
    let mut ruleset = sample_ruleset();
    ruleset.ruleset_name = "四对四通用".to_owned();
    ruleset.source_text = "一辩立论，质询，自由辩，四辩总结。".to_owned();
    ruleset.stages = vec![
        opening_stage(),
        sample_cross_exam(),
        free_debate_stage(),
        summary_stage(),
    ];
    ruleset
}

fn opening_stage() -> StageDefinitionV1 {
    StageDefinitionV1 {
        id: "opening".to_owned(),
        name: "正方一辩陈词".to_owned(),
        kind: StageKind::Opening,
        speakers: vec![SlotId::Aff1],
        speaker_durations: vec![180],
        clock_owner: ClockOwner::Individual,
        duration_seconds: 180,
        early_end_allowed: true,
        follow_up_allowed: false,
        interruptible: false,
        interrupt_limit: 0,
        questioner: None,
        respondent: None,
        eligible_questioners: Vec::new(),
        eligible_respondents: Vec::new(),
        initial_questioning_side: None,
        max_cross_exam_rounds: None,
        weight: 1,
        repeat: 1,
    }
}

fn free_debate_stage() -> StageDefinitionV1 {
    StageDefinitionV1 {
        id: "free".to_owned(),
        name: "自由辩".to_owned(),
        kind: StageKind::FreeDebate,
        speakers: vec![SlotId::Aff3, SlotId::Neg3],
        speaker_durations: vec![30, 30],
        clock_owner: ClockOwner::Team,
        duration_seconds: 30,
        early_end_allowed: true,
        follow_up_allowed: false,
        interruptible: false,
        interrupt_limit: 0,
        questioner: None,
        respondent: None,
        eligible_questioners: Vec::new(),
        eligible_respondents: Vec::new(),
        initial_questioning_side: None,
        max_cross_exam_rounds: None,
        weight: 1,
        repeat: 1,
    }
}

fn summary_stage() -> StageDefinitionV1 {
    StageDefinitionV1 {
        id: "summary".to_owned(),
        name: "反方四辩总结".to_owned(),
        kind: StageKind::Summary,
        speakers: vec![SlotId::Neg4],
        speaker_durations: vec![180],
        clock_owner: ClockOwner::Individual,
        duration_seconds: 180,
        early_end_allowed: true,
        follow_up_allowed: false,
        interruptible: false,
        interrupt_limit: 0,
        questioner: None,
        respondent: None,
        eligible_questioners: Vec::new(),
        eligible_respondents: Vec::new(),
        initial_questioning_side: None,
        max_cross_exam_rounds: None,
        weight: 1,
        repeat: 1,
    }
}

pub fn sample_cross_exam() -> StageDefinitionV1 {
    StageDefinitionV1 {
        id: "cross".to_owned(),
        name: "质询".to_owned(),
        kind: StageKind::CrossExam,
        speakers: vec![SlotId::Aff3],
        speaker_durations: vec![90],
        clock_owner: ClockOwner::Questioner,
        duration_seconds: 90,
        early_end_allowed: true,
        follow_up_allowed: true,
        interruptible: true,
        interrupt_limit: 1,
        questioner: Some(SlotId::Aff3),
        respondent: Some(SlotId::Neg3),
        eligible_questioners: SlotId::ALL.to_vec(),
        eligible_respondents: SlotId::ALL.to_vec(),
        initial_questioning_side: Some(Side::Affirmative),
        max_cross_exam_rounds: None,
        weight: 1,
        repeat: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        expand_schedule, sample_cross_exam, sample_ruleset, ScheduleError, SlotId, StageKind,
    };
    use uuid::Uuid;

    #[test]
    fn expands_a_finite_queue_and_rejects_illegal_shapes() {
        let mut salt = 1u128;
        let events = expand_schedule(&sample_ruleset(), 240, || {
            salt += 1;
            Uuid::from_u128(salt)
        })
        .expect("sample ruleset");
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].speaker, SlotId::Aff1);
        assert!(!events[1].interruptible);
        assert!(events.iter().all(|event| event.word_cap >= 1));

        let mut split = sample_ruleset();
        split.stages[1].speaker_durations = vec![40, 90];
        let split_events = expand_schedule(&split, 240, || Uuid::from_u128(1)).expect("split");
        assert_eq!(split_events[1].duration_seconds, 40);
        assert_eq!(split_events[2].duration_seconds, 90);

        let mut illegal = sample_ruleset();
        illegal.stages[1].interruptible = true;
        let error = expand_schedule(&illegal, 240, || Uuid::from_u128(1)).unwrap_err();
        assert_eq!(error, ScheduleError::FreeDebateInterruptible);

        let mut missing = sample_ruleset();
        missing.stages = vec![sample_cross_exam()];
        missing.stages[0].questioner = None;
        let error = expand_schedule(&missing, 240, || Uuid::from_u128(1)).unwrap_err();
        assert_eq!(error, ScheduleError::CrossExamMissingRoles);

        let mut opening = sample_ruleset();
        opening.stages[0].interruptible = true;
        opening.stages[0].kind = StageKind::Opening;
        let error = expand_schedule(&opening, 240, || Uuid::from_u128(1)).unwrap_err();
        assert_eq!(error, ScheduleError::InterruptOutsideCrossExam);
    }
}
