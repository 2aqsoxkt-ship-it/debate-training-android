pub const FORMULA_VERSION: &str = "score-v3";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgramInput {
    pub in_cap_units: u32,
    pub cap_units: u32,
    pub overflow_units: u32,
    pub reverse_units: u32,
    pub reverse_actions: u32,
    pub forward_typed_units: u32,
    pub pause_ms: u64,
    pub input_ms: u64,
    pub baseline_cpm: f64,
    pub started_on_time: bool,
    pub stayed_legal: bool,
    pub role_complete: bool,
    pub reviewable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgramScore {
    pub length: f64,
    pub input_control: f64,
    pub execution: f64,
    pub total: f64,
    pub speed_factor: f64,
    pub reverse_penalty: f64,
    pub pause_penalty: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimension {
    pub weight: f64,
    pub raw: f64,
}

pub fn score_program(input: ProgramInput) -> ProgramScore {
    let length = length_score(input.in_cap_units, input.cap_units, input.overflow_units);
    let speed_factor = speed_factor(input.baseline_cpm);
    let reverse_penalty = reverse_penalty(
        input.reverse_units,
        input.reverse_actions,
        input.forward_typed_units,
        speed_factor,
    );
    let pause_penalty = pause_penalty(input.pause_ms, input.input_ms, speed_factor);
    let input_control = round1((8.0 - reverse_penalty - pause_penalty).max(0.0));
    let execution = execution_score(input);
    ProgramScore {
        length,
        input_control,
        execution,
        total: round1(length + input_control + execution),
        speed_factor: round1(speed_factor),
        reverse_penalty: round1(reverse_penalty),
        pause_penalty: round1(pause_penalty),
    }
}

pub fn length_score(in_cap: u32, cap: u32, overflow: u32) -> f64 {
    let capacity = f64::from(cap.max(1));
    let base = 12.0 * clamp(f64::from(in_cap) / capacity, 0.0, 1.0);
    let penalty = (24.0 * f64::from(overflow) / capacity).min(12.0);
    round1((base - penalty).max(0.0))
}

pub fn speed_factor(baseline_cpm: f64) -> f64 {
    let cpm = if baseline_cpm <= 0.0 {
        180.0
    } else {
        baseline_cpm
    };
    clamp(180.0 / cpm, 0.75, 1.5)
}

fn reverse_penalty(units: u32, actions: u32, forward: u32, speed_factor: f64) -> f64 {
    let load = (f64::from(units) + 0.5 * f64::from(actions)) / f64::from(forward.max(1));
    4.0 * clamp(load * speed_factor / 0.35, 0.0, 1.0)
}

fn pause_penalty(pause_ms: u64, input_ms: u64, speed_factor: f64) -> f64 {
    let ratio = pause_ms as f64 / input_ms.max(1) as f64;
    4.0 * clamp((ratio * speed_factor - 0.10) / 0.30, 0.0, 1.0)
}

fn execution_score(input: ProgramInput) -> f64 {
    1.25 * (f64::from(u8::from(input.started_on_time))
        + f64::from(u8::from(input.stayed_legal))
        + f64::from(u8::from(input.role_complete))
        + f64::from(u8::from(input.reviewable)))
}

pub fn semantic_points(dimensions: &[Dimension]) -> f64 {
    let earned = dimensions
        .iter()
        .map(|dimension| dimension.weight * clamp(dimension.raw, 0.0, 100.0) / 100.0)
        .sum();
    round1(earned)
}

pub fn apply_review(raw: f64, delta: i32) -> Result<f64, ReviewError> {
    if !(-8..=8).contains(&delta) {
        return Err(ReviewError::OutOfRange);
    }
    Ok(clamp(raw + f64::from(delta), 0.0, 100.0))
}

pub fn personal_total(semantic: f64, program: f64) -> f64 {
    round1(semantic + program)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewError {
    OutOfRange,
}

pub fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn clamp(value: f64, low: f64, high: f64) -> f64 {
    value.max(low).min(high)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_review, length_score, personal_total, score_program, semantic_points, speed_factor,
        Dimension, ProgramInput, ReviewError,
    };

    fn sample(in_cap: u32, cap: u32, overflow: u32, baseline: f64) -> ProgramInput {
        ProgramInput {
            in_cap_units: in_cap,
            cap_units: cap,
            overflow_units: overflow,
            reverse_units: 0,
            reverse_actions: 0,
            forward_typed_units: in_cap.max(1),
            pause_ms: 0,
            input_ms: 1_000,
            baseline_cpm: baseline,
            started_on_time: true,
            stayed_legal: true,
            role_complete: true,
            reviewable: true,
        }
    }

    #[test]
    fn length_edges_match_the_formula() {
        assert_eq!(length_score(0, 100, 0), 0.0);
        assert_eq!(length_score(100, 100, 0), 12.0);
        assert_eq!(length_score(100, 100, 1), 11.8);
        assert_eq!(length_score(100, 100, 50), 0.0);
        assert!(length_score(95, 100, 0) > length_score(60, 100, 0));
    }

    #[test]
    fn a_slower_baseline_costs_more_for_the_same_pause() {
        let mut slow = sample(100, 100, 0, 90.0);
        let mut fast = sample(100, 100, 0, 360.0);
        slow.pause_ms = 400;
        fast.pause_ms = 400;
        slow.input_ms = 1_000;
        fast.input_ms = 1_000;
        let slow_score = score_program(slow);
        let fast_score = score_program(fast);
        assert!(slow_score.pause_penalty > fast_score.pause_penalty);
        assert!(speed_factor(90.0) > speed_factor(360.0));
        assert_eq!(slow_score.length, fast_score.length);
    }

    #[test]
    fn pasted_units_do_not_raise_the_length_base() {
        let typed = score_program(sample(40, 100, 0, 180.0));
        let pasted_as_overflow = score_program(sample(0, 100, 40, 180.0));
        assert!(typed.length > pasted_as_overflow.length);
    }

    #[test]
    fn review_and_team_result_stay_separate() {
        assert_eq!(apply_review(50.0, 9), Err(ReviewError::OutOfRange));
        assert_eq!(apply_review(50.0, -8).unwrap(), 42.0);
        let semantic = semantic_points(&[Dimension {
            weight: 80.0,
            raw: 50.0,
        }]);
        let personal = personal_total(semantic, 20.0);
        assert_eq!(personal, 60.0);
        let winner = "negative";
        assert_eq!(personal_total(semantic, 20.0), personal);
        assert_eq!(winner, "negative");
    }
}
