use unicode_segmentation::UnicodeSegmentation;

use crate::domain::text_units::count_effective_units;

pub const TEST_MS: u64 = 60_000;
pub const PAUSE_GRACE_MS: u64 = 2_500;

const PASSAGE_A: &str =
    "人工智能应当在安全、透明和人类负责的前提下发展。限制不是拒绝技术，而是划定可以核验的边界。";
const PASSAGE_B: &str =
    "公共讨论需要把事实、价值和政策分开。辩手应当说明主张、理由、影响，以及反方最强的反驳。";

pub fn passage_for(index: usize) -> String {
    let seed = if index % 2 == 0 { PASSAGE_A } else { PASSAGE_B };
    let mut text = String::new();
    while count_effective_units(&text) < 480 {
        text.push_str(seed);
    }
    text
}

pub fn excess_pause(interval_ms: u64) -> u64 {
    interval_ms.saturating_sub(PAUSE_GRACE_MS)
}

pub fn chargeable_pause(intervals: &[u64]) -> u64 {
    intervals.iter().fold(0, |sum, interval| {
        sum.saturating_add(excess_pause(*interval))
    })
}

pub fn grade(expected: &str, typed: &str) -> (u32, u32) {
    let expected: Vec<&str> = expected.graphemes(true).collect();
    let mut correct = 0_u32;
    let mut wrong = 0_u32;
    for (index, piece) in typed.graphemes(true).enumerate() {
        let units = count_effective_units(piece);
        if units == 0 {
            continue;
        }
        if expected.get(index).is_some_and(|item| *item == piece) {
            correct += units;
        } else {
            wrong += units;
        }
    }
    (correct, wrong)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationOutcome {
    pub current_cpm: f64,
    pub accuracy: f64,
    pub baseline_cpm: f64,
    pub low_confidence: bool,
    pub must_retest: bool,
    pub blocked: bool,
}

pub fn assess(
    correct_units: u32,
    wrong_units: u32,
    history: &[f64],
    attempt: u32,
) -> CalibrationOutcome {
    let current_cpm = f64::from(correct_units);
    let total = correct_units.saturating_add(wrong_units);
    let accuracy = if total == 0 {
        0.0
    } else {
        f64::from(correct_units) / f64::from(total)
    };
    let blocked = current_cpm <= 0.0;
    let low_accuracy = accuracy < 0.90;
    let must_retest = !blocked && low_accuracy && attempt <= 1;
    let low_confidence = !blocked && low_accuracy && attempt >= 2;
    let baseline_cpm = if history.is_empty() {
        current_cpm
    } else {
        let start = history.len().saturating_sub(10);
        let mut recent = history[start..].to_vec();
        recent.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
        0.70 * current_cpm + 0.30 * median(&recent)
    };
    CalibrationOutcome {
        current_cpm,
        accuracy,
        baseline_cpm,
        low_confidence,
        must_retest,
        blocked,
    }
}

pub fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::{assess, chargeable_pause, grade, passage_for, CalibrationOutcome};

    #[test]
    fn passage_is_at_least_twice_a_minute_of_units() {
        assert!(super::count_effective_units(&passage_for(0)) >= 480);
        assert!(super::count_effective_units(&passage_for(1)) >= 480);
    }

    #[test]
    fn only_the_part_beyond_two_and_a_half_seconds_counts() {
        assert_eq!(chargeable_pause(&[2_500, 1_000]), 0);
        assert_eq!(chargeable_pause(&[4_000]), 1_500);
        assert_eq!(chargeable_pause(&[3_000, 3_000]), 1_000);
    }

    #[test]
    fn baseline_uses_current_speed_or_the_seventy_thirty_mix() {
        let fresh = assess(120, 0, &[], 1);
        assert_eq!(fresh.current_cpm, 120.0);
        assert_eq!(fresh.baseline_cpm, 120.0);
        assert!(!fresh.must_retest);
        let mixed = assess(100, 0, &[80.0, 120.0, 200.0], 1);
        assert_eq!(mixed.baseline_cpm, 0.70 * 100.0 + 0.30 * 120.0);
    }

    #[test]
    fn low_accuracy_retests_once_then_marks_low_confidence() {
        let first = assess(80, 20, &[], 1);
        assert!(first.must_retest);
        assert!(!first.low_confidence);
        let second = assess(80, 20, &[100.0], 2);
        assert!(!second.must_retest);
        assert!(second.low_confidence);
        assert!(second.baseline_cpm > 0.0);
    }

    #[test]
    fn zero_speed_blocks_scoring() {
        let blocked = assess(0, 0, &[180.0], 1);
        assert!(blocked.blocked);
        let typed = "你好";
        let (correct, wrong) = grade(typed, "你啊");
        assert_eq!(correct, 1);
        assert_eq!(wrong, 1);
        let _ = CalibrationOutcome {
            current_cpm: 0.0,
            accuracy: 0.0,
            baseline_cpm: 0.0,
            low_confidence: false,
            must_retest: false,
            blocked: true,
        };
    }
}
