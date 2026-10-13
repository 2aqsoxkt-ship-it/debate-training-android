use std::collections::HashMap;

use crate::domain::ruleset::{Side, SlotId};
use crate::domain::text_units::split_effective;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebateFloor {
    pub affirmative_ms: u64,
    pub negative_ms: u64,
    pub holder: Option<Side>,
}

impl DebateFloor {
    pub fn new() -> Self {
        Self {
            affirmative_ms: 0,
            negative_ms: 0,
            holder: None,
        }
    }

    pub fn charge(&mut self, side: Side, milliseconds: u64) {
        match side {
            Side::Affirmative => {
                self.affirmative_ms = self.affirmative_ms.saturating_add(milliseconds);
            }
            Side::Negative => {
                self.negative_ms = self.negative_ms.saturating_add(milliseconds);
            }
        }
    }

    pub fn take_turn(&mut self, side: Side, alternate: bool) -> Result<(), FloorError> {
        if alternate && self.holder == Some(side) {
            return Err(FloorError::Consecutive);
        }
        self.holder = Some(side);
        Ok(())
    }
}

impl Default for DebateFloor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorError {
    Consecutive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptError {
    NotAllowed,
    Duplicate,
    TooEarly,
    Late,
}

pub fn interrupt_at(
    full: &str,
    displayed_units: u32,
    allowed: bool,
    already: bool,
    closed: bool,
) -> Result<(String, String), InterruptError> {
    if closed {
        return Err(InterruptError::Late);
    }
    if !allowed {
        return Err(InterruptError::NotAllowed);
    }
    if already {
        return Err(InterruptError::Duplicate);
    }
    if displayed_units == 0 {
        return Err(InterruptError::TooEarly);
    }
    let (prefix, tail) = split_effective(full, displayed_units);
    Ok((prefix.to_owned(), tail.to_owned()))
}

pub fn choose_slot(candidates: &[SlotId], counts: &HashMap<SlotId, u32>) -> Option<SlotId> {
    candidates
        .iter()
        .copied()
        .min_by_key(|slot| (counts.get(slot).copied().unwrap_or(0), slot_index(*slot)))
}

fn slot_index(slot: SlotId) -> u8 {
    match slot {
        SlotId::Aff1 => 0,
        SlotId::Aff2 => 1,
        SlotId::Aff3 => 2,
        SlotId::Aff4 => 3,
        SlotId::Neg1 => 4,
        SlotId::Neg2 => 5,
        SlotId::Neg3 => 6,
        SlotId::Neg4 => 7,
    }
}

#[cfg(test)]
mod tests {
    use super::{choose_slot, interrupt_at, DebateFloor, InterruptError};
    use crate::domain::ruleset::{Side, SlotId};
    use std::collections::HashMap;

    #[test]
    fn only_the_floor_holder_clock_moves() {
        let mut floor = DebateFloor::new();
        floor.take_turn(Side::Affirmative, true).unwrap();
        floor.charge(Side::Affirmative, 1_000);
        assert_eq!(floor.affirmative_ms, 1_000);
        assert_eq!(floor.negative_ms, 0);
    }

    #[test]
    fn rejected_interrupt_leaves_clocks_unchanged() {
        let mut floor = DebateFloor::new();
        floor.charge(Side::Affirmative, 500);
        let before = floor.clone();
        let error = interrupt_at("已经开口", 2, false, false, false).unwrap_err();
        assert_eq!(error, InterruptError::NotAllowed);
        assert_eq!(floor, before);
    }

    #[test]
    fn default_rules_block_the_same_side_twice() {
        let mut floor = DebateFloor::new();
        floor.take_turn(Side::Affirmative, true).unwrap();
        assert!(floor.take_turn(Side::Affirmative, true).is_err());
        floor.take_turn(Side::Negative, true).unwrap();
    }

    #[test]
    fn picker_rotates_away_from_the_busy_slot() {
        let mut counts = HashMap::new();
        counts.insert(SlotId::Aff3, 3);
        counts.insert(SlotId::Aff4, 1);
        assert_eq!(
            choose_slot(&[SlotId::Aff3, SlotId::Aff4], &counts),
            Some(SlotId::Aff4)
        );
    }

    #[test]
    fn interrupt_keeps_grapheme_boundaries() {
        let (prefix, tail) = interrupt_at("你好world", 2, true, false, false).unwrap();
        assert_eq!(prefix, "你好");
        assert_eq!(tail, "world");
        assert_eq!(
            interrupt_at("你好", 0, true, false, false),
            Err(InterruptError::TooEarly)
        );
        assert_eq!(
            interrupt_at("你好", 1, true, true, false),
            Err(InterruptError::Duplicate)
        );
        assert_eq!(
            interrupt_at("你好", 1, true, false, true),
            Err(InterruptError::Late)
        );
    }
}
