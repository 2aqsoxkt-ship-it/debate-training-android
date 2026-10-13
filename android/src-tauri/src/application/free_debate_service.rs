//! Free-debate seat picking and acceptance report helpers.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::domain::floor::choose_slot;
use crate::domain::free_debate::{format_acceptance_markdown, run_alternating_scenario};
use crate::domain::ruleset::SlotId;

pub fn pick_free_debate_slot(
    candidates: &[SlotId],
    appearance_counts: &HashMap<SlotId, u32>,
) -> Option<SlotId> {
    choose_slot(candidates, appearance_counts)
}

/// Writes a human-inspectable 12-round free-debate acceptance markdown under `target/`.
#[allow(dead_code)]
pub fn write_twelve_round_acceptance() -> Result<PathBuf, String> {
    let aff = [SlotId::Aff1, SlotId::Aff2, SlotId::Aff3, SlotId::Aff4];
    let neg = [SlotId::Neg1, SlotId::Neg2, SlotId::Neg3, SlotId::Neg4];
    let report = run_alternating_scenario(12, &aff, &neg, 1_000, |round, side, slot, prev| {
        if prev.is_empty() {
            format!("开场主张{round}-{:?}-{:?}", side, slot)
        } else {
            format!("针对{prev}作出回应{round}-{:?}", slot)
        }
    })?;
    let md = format_acceptance_markdown(&report);
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target");
    path.push("m2-free-debate-12.md");
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&path, md).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::write_twelve_round_acceptance;

    #[test]
    fn writes_twelve_round_markdown() {
        let path = write_twelve_round_acceptance().expect("write");
        let text = std::fs::read_to_string(path).expect("read");
        assert!(text.contains("| 12 |"));
    }
}
