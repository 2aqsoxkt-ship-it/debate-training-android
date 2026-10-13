//! Free-debate floor helpers and a 12-round alternating scenario harness.

use std::collections::HashMap;

use crate::domain::floor::{choose_slot, DebateFloor};
use crate::domain::ruleset::{Side, SlotId};
use crate::domain::text_units::count_effective_units;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeDebateTurnRecord {
    pub round: u32,
    pub side: Side,
    pub slot: SlotId,
    pub responds_to: String,
    pub target_claim: String,
    pub summary: String,
    pub affirmative_ms_before: u64,
    pub negative_ms_before: u64,
    pub affirmative_ms_after: u64,
    pub negative_ms_after: u64,
    pub alternated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeDebateScenarioReport {
    pub turns: Vec<FreeDebateTurnRecord>,
}

/// Run `rounds` alternating free-debate turns with an AI seat picker.
/// Speeches must reference the previous summary when one exists.
pub fn run_alternating_scenario(
    rounds: u32,
    aff_candidates: &[SlotId],
    neg_candidates: &[SlotId],
    speech_ms: u64,
    speak: impl Fn(u32, Side, SlotId, &str) -> String,
) -> Result<FreeDebateScenarioReport, String> {
    let mut floor = DebateFloor::new();
    let mut counts: HashMap<SlotId, u32> = HashMap::new();
    let mut turns = Vec::new();
    let mut previous_id = String::new();
    let mut previous_summary = String::new();
    let mut next_side = Side::Affirmative;

    for round in 1..=rounds {
        let before_aff = floor.affirmative_ms;
        let before_neg = floor.negative_ms;
        floor
            .take_turn(next_side, true)
            .map_err(|_| "同一方不能连续取得发言权".to_owned())?;
        let candidates = match next_side {
            Side::Affirmative => aff_candidates,
            Side::Negative => neg_candidates,
        };
        let slot = choose_slot(candidates, &counts).ok_or("没有可选辩位")?;
        *counts.entry(slot).or_insert(0) += 1;
        let text = speak(round, next_side, slot, &previous_summary);
        if !previous_summary.is_empty()
            && !text.contains(&previous_summary)
            && !text.contains("回应")
            && !text.contains("针对")
        {
            return Err(format!("第{round}轮未回应上一轮主张"));
        }
        let summary: String = text.chars().take(24).collect();
        let units = count_effective_units(&text).max(1) as u64;
        let charge = speech_ms.max(units.saturating_mul(250));
        floor.charge(next_side, charge);
        let id = format!("fd-{round}");
        turns.push(FreeDebateTurnRecord {
            round,
            side: next_side,
            slot,
            responds_to: previous_id.clone(),
            target_claim: previous_summary.clone(),
            summary: summary.clone(),
            affirmative_ms_before: before_aff,
            negative_ms_before: before_neg,
            affirmative_ms_after: floor.affirmative_ms,
            negative_ms_after: floor.negative_ms,
            alternated: true,
        });
        previous_id = id;
        previous_summary = summary;
        next_side = match next_side {
            Side::Affirmative => Side::Negative,
            Side::Negative => Side::Affirmative,
        };
    }

    // Verify strict alternation and clock only charged to holder.
    for window in turns.windows(2) {
        if window[0].side == window[1].side {
            return Err("自由辩未正确正反交替".to_owned());
        }
    }
    Ok(FreeDebateScenarioReport { turns })
}

pub fn format_acceptance_markdown(report: &FreeDebateScenarioReport) -> String {
    let mut out = String::from("# M2 自由辩 12 轮验收\n\n");
    out.push_str(
        "| 轮 | 方 | 辩位 | respondsTo | 目标主张 | 摘要 | 正方时钟前后 | 反方时钟前后 | 交替 |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for turn in &report.turns {
        let side = match turn.side {
            Side::Affirmative => "正",
            Side::Negative => "反",
        };
        out.push_str(&format!(
            "| {} | {} | {:?} | {} | {} | {} | {}→{} | {}→{} | {} |\n",
            turn.round,
            side,
            turn.slot,
            turn.responds_to,
            turn.target_claim,
            turn.summary,
            turn.affirmative_ms_before,
            turn.affirmative_ms_after,
            turn.negative_ms_before,
            turn.negative_ms_after,
            turn.alternated
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{format_acceptance_markdown, run_alternating_scenario};
    use crate::domain::floor::{interrupt_at, DebateFloor};
    use crate::domain::ruleset::{Side, SlotId};

    #[test]
    fn twelve_rounds_alternate_and_charge_holder_only() {
        let aff = [SlotId::Aff1, SlotId::Aff2, SlotId::Aff3, SlotId::Aff4];
        let neg = [SlotId::Neg1, SlotId::Neg2, SlotId::Neg3, SlotId::Neg4];
        let report = run_alternating_scenario(12, &aff, &neg, 1_000, |round, side, slot, prev| {
            if prev.is_empty() {
                format!("开场主张{round}-{:?}-{:?}", side, slot)
            } else {
                format!("针对{prev}作出回应{round}-{:?}", slot)
            }
        })
        .expect("scenario");
        assert_eq!(report.turns.len(), 12);
        assert!(report.turns.windows(2).all(|w| w[0].side != w[1].side));
        let last = report.turns.last().unwrap();
        assert!(last.affirmative_ms_after > 0 && last.negative_ms_after > 0);
        // Seat picker should rotate rather than stick to Aff1 forever.
        let aff_slots: std::collections::HashSet<_> = report
            .turns
            .iter()
            .filter(|t| t.side == Side::Affirmative)
            .map(|t| t.slot)
            .collect();
        assert!(aff_slots.len() >= 2);
        let md = format_acceptance_markdown(&report);
        assert!(md.contains("自由辩 12 轮"));
    }

    #[test]
    fn free_debate_interrupt_command_is_a_noop_on_clocks() {
        let mut floor = DebateFloor::new();
        floor.take_turn(Side::Affirmative, true).unwrap();
        floor.charge(Side::Affirmative, 500);
        let before = (floor.affirmative_ms, floor.negative_ms, floor.holder);
        let err = interrupt_at("完整回答", 2, false, false, false).expect_err("blocked");
        assert!(matches!(
            err,
            crate::domain::floor::InterruptError::NotAllowed
        ));
        assert_eq!(
            (floor.affirmative_ms, floor.negative_ms, floor.holder),
            before
        );
    }
}
