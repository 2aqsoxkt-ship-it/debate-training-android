use crate::domain::scoring::{personal_total, round1, semantic_points, Dimension};

pub const PLACEHOLDER: &str = "〔不可评分〕";

pub const DIMENSIONS: [(&str, f64); 7] = [
    ("切题性", 15.0),
    ("论证质量", 15.0),
    ("反驳质量", 12.0),
    ("证据使用", 10.0),
    ("逻辑一致性", 10.0),
    ("临场应对", 8.0),
    ("团队协作", 5.0),
];

#[derive(Debug, Clone, PartialEq)]
pub struct SpeechPiece {
    pub text: String,
    pub human: bool,
    pub pasted: bool,
    pub overflow: bool,
    pub interrupted_tail: bool,
}

pub fn scoreable_text(piece: &SpeechPiece) -> Option<String> {
    if !piece.human {
        return None;
    }
    if piece.pasted || piece.overflow || piece.interrupted_tail {
        return Some(PLACEHOLDER.to_owned());
    }
    Some(piece.text.clone())
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageMark {
    pub transcript_id: String,
    pub raw: [f64; 7],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitedAdjustment {
    pub dimension: usize,
    pub delta: i32,
    pub transcript_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewReject {
    MissingCitation,
    OutOfRange,
    UnknownTranscript,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScoredMatch {
    pub semantic: f64,
    pub program: f64,
    pub personal: f64,
    pub team_winner: String,
    pub status: String,
    pub dimensions: Vec<(String, f64, f64, String)>,
}

pub fn raw_from_units(units: u32) -> f64 {
    f64::from(units).min(100.0)
}

pub fn combine(
    marks: &[StageMark],
    adjustments: &[CitedAdjustment],
    valid_ids: &[String],
    program: f64,
    team_winner: &str,
    semantic_ok: bool,
) -> Result<ScoredMatch, ReviewReject> {
    if !semantic_ok {
        return Ok(ScoredMatch {
            semantic: 0.0,
            program,
            personal: personal_total(0.0, program),
            team_winner: team_winner.to_owned(),
            status: "SCORING_PENDING".to_owned(),
            dimensions: Vec::new(),
        });
    }
    let mut marks = marks.to_vec();
    for adjustment in adjustments {
        let Some(transcript_id) = adjustment.transcript_id.clone() else {
            return Err(ReviewReject::MissingCitation);
        };
        if !valid_ids.iter().any(|id| id == &transcript_id) {
            return Err(ReviewReject::UnknownTranscript);
        }
        if !(-8..=8).contains(&adjustment.delta) || adjustment.dimension >= DIMENSIONS.len() {
            return Err(ReviewReject::OutOfRange);
        }
        if let Some(mark) = marks
            .iter_mut()
            .find(|mark| mark.transcript_id == transcript_id)
        {
            let next = crate::domain::scoring::apply_review(
                mark.raw[adjustment.dimension],
                adjustment.delta,
            )
            .map_err(|_| ReviewReject::OutOfRange)?;
            mark.raw[adjustment.dimension] = next;
        }
    }
    let averaged = average_raw(&marks);
    let dimensions: Vec<Dimension> = DIMENSIONS
        .iter()
        .enumerate()
        .map(|(index, (_, weight))| Dimension {
            weight: *weight,
            raw: averaged[index],
        })
        .collect();
    let semantic = semantic_points(&dimensions);
    let cited = marks
        .first()
        .map(|mark| mark.transcript_id.clone())
        .unwrap_or_default();
    Ok(ScoredMatch {
        personal: personal_total(semantic, program),
        semantic,
        program,
        team_winner: team_winner.to_owned(),
        status: "ready".to_owned(),
        dimensions: dimensions
            .iter()
            .enumerate()
            .map(|(index, dimension)| {
                (
                    DIMENSIONS[index].0.to_owned(),
                    dimension.raw,
                    round1(dimension.weight * dimension.raw / 100.0),
                    cited.clone(),
                )
            })
            .collect(),
    })
}

fn average_raw(marks: &[StageMark]) -> [f64; 7] {
    if marks.is_empty() {
        return [0.0; 7];
    }
    let mut totals = [0.0; 7];
    for mark in marks {
        for (index, raw) in mark.raw.iter().enumerate() {
            totals[index] += raw;
        }
    }
    let count = marks.len() as f64;
    for total in &mut totals {
        *total /= count;
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::{
        combine, raw_from_units, scoreable_text, CitedAdjustment, ReviewReject, SpeechPiece,
        StageMark, DIMENSIONS,
    };
    use crate::domain::scoring::score_program;
    use crate::domain::scoring::ProgramInput;

    fn perfect_program() -> f64 {
        score_program(ProgramInput {
            in_cap_units: 100,
            cap_units: 100,
            overflow_units: 0,
            reverse_units: 0,
            reverse_actions: 0,
            forward_typed_units: 100,
            pause_ms: 0,
            input_ms: 1_000,
            baseline_cpm: 180.0,
            started_on_time: true,
            stayed_legal: true,
            role_complete: true,
            reviewable: true,
        })
        .total
    }

    fn mark(id: &str, raw: f64) -> StageMark {
        StageMark {
            transcript_id: id.to_owned(),
            raw: [raw; 7],
        }
    }

    #[test]
    fn phase11_golden_sample_is_one_hundred_and_team_is_separate() {
        let program = perfect_program();
        assert_eq!(program, 25.0);
        let valid = vec!["t1".to_owned()];
        let scored = combine(&[mark("t1", 100.0)], &[], &valid, program, "tie", true).unwrap();
        assert_eq!(scored.semantic, 75.0);
        assert_eq!(scored.personal, 100.0);
        let mut flipped = scored.clone();
        flipped.team_winner = "affirmative".to_owned();
        assert_eq!(flipped.personal, scored.personal);
        assert_eq!(
            DIMENSIONS.iter().map(|(_, weight)| weight).sum::<f64>(),
            75.0
        );
    }

    #[test]
    fn phase11_rejects_missing_unknown_and_oversized_review() {
        let valid = vec!["t1".to_owned()];
        let missing = combine(
            &[mark("t1", 50.0)],
            &[CitedAdjustment {
                dimension: 0,
                delta: 1,
                transcript_id: None,
            }],
            &valid,
            20.0,
            "tie",
            true,
        );
        assert_eq!(missing.unwrap_err(), ReviewReject::MissingCitation);
        let unknown = combine(
            &[mark("t1", 50.0)],
            &[CitedAdjustment {
                dimension: 0,
                delta: 1,
                transcript_id: Some("missing".to_owned()),
            }],
            &valid,
            20.0,
            "tie",
            true,
        );
        assert_eq!(unknown.unwrap_err(), ReviewReject::UnknownTranscript);
        let wide = combine(
            &[mark("t1", 50.0)],
            &[CitedAdjustment {
                dimension: 0,
                delta: 9,
                transcript_id: Some("t1".to_owned()),
            }],
            &valid,
            20.0,
            "tie",
            true,
        );
        assert_eq!(wide.unwrap_err(), ReviewReject::OutOfRange);
        let adjusted = combine(
            &[mark("t1", 50.0)],
            &[CitedAdjustment {
                dimension: 0,
                delta: 8,
                transcript_id: Some("t1".to_owned()),
            }],
            &valid,
            20.0,
            "tie",
            true,
        )
        .unwrap();
        assert_eq!(adjusted.dimensions[0].1, 58.0);
    }

    #[test]
    fn phase11_failure_keeps_program_and_excludes_unscoreable_text() {
        let pending = combine(&[], &[], &[], 16.4, "tie", false).unwrap();
        assert_eq!(pending.status, "SCORING_PENDING");
        assert_eq!(pending.semantic, 0.0);
        assert_eq!(pending.program, 16.4);
        assert_eq!(pending.personal, 16.4);
        assert_eq!(
            scoreable_text(&SpeechPiece {
                text: "队友发言".to_owned(),
                human: false,
                pasted: false,
                overflow: false,
                interrupted_tail: false,
            }),
            None
        );
        assert_eq!(
            scoreable_text(&SpeechPiece {
                text: "被打断的尾巴".to_owned(),
                human: true,
                pasted: false,
                overflow: false,
                interrupted_tail: true,
            })
            .as_deref(),
            Some("〔不可评分〕")
        );
        assert!(!scoreable_text(&SpeechPiece {
            text: "粘贴进来".to_owned(),
            human: true,
            pasted: true,
            overflow: false,
            interrupted_tail: false,
        })
        .unwrap()
        .contains("粘贴"));
        assert_eq!(raw_from_units(4), 4.0);
    }
}
