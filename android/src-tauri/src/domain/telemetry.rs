use serde::{Deserialize, Serialize};

use crate::domain::calibration::excess_pause;
use crate::domain::text_units::{count_effective_units, split_effective};

pub const PAUSE_GRACE_MS: u64 = 2_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FragmentSource {
    Typed,
    Pasted,
    Overflow,
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fragment {
    pub text: String,
    pub source: FragmentSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceSpan {
    pub start_units: u32,
    pub end_units: u32,
    pub source: FragmentSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InputEvent {
    Composition,
    Insert {
        text: String,
        source: FragmentSource,
    },
    Delete {
        units: u32,
    },
    /// Actual committed edit, using DOM UTF-16 offsets (not effective units).
    Replace {
        start: u32,
        end: u32,
        text: String,
        source: FragmentSource,
    },
    Pause {
        ms: u64,
    },
    FocusLoss {
        ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnTelemetry {
    pub forward_typed_units: u32,
    pub effective_positive_units: u32,
    pub reverse_edited_units: u32,
    pub reverse_edit_actions: u32,
    pub paste_units: u32,
    pub pause_ms: u64,
    pub focus_loss_ms: u64,
    pub input_elapsed_ms: u64,
    pub fragments: Vec<Fragment>,
    pub provenance: Vec<ProvenanceSpan>,
    pub final_text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unverified_input: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputRound {
    pub telemetry: TurnTelemetry,
    timeline: Vec<InputEvent>,
    valid_edits: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimedMetrics {
    pub input_elapsed_ms: u64,
    pub forward_typed_units: u32,
    pub effective_positive_units: u32,
    pub reverse_units: u32,
    pub reverse_actions: u32,
    pub pause_ms: u64,
    pub focus_loss_ms: u64,
    pub paste_units: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetryReject {
    InconsistentMetrics,
    TextMismatch,
    EmptyTimeline,
    InvalidEdit,
}

impl TelemetryReject {
    pub fn message(self) -> &'static str {
        match self {
            Self::InconsistentMetrics => "输入统计与事件摘要不一致",
            Self::TextMismatch => "最终文本与事件回放结果不一致",
            Self::EmptyTimeline => "缺少可回放的输入事件",
            Self::InvalidEdit => "输入事件包含非法编辑位置",
        }
    }
}

pub fn fold(events: &[InputEvent]) -> InputRound {
    fold_with_elapsed(events, 0)
}

pub fn fold_with_elapsed(events: &[InputEvent], input_elapsed_ms: u64) -> InputRound {
    let mut fragments: Vec<Fragment> = Vec::new();
    let mut forward = 0_u32;
    let mut reverse_units = 0_u32;
    let mut reverse_actions = 0_u32;
    let mut pause_ms = 0_u64;
    let mut focus_loss_ms = 0_u64;
    let mut valid_edits = true;
    let mut unverified_input = false;
    for event in events {
        match event {
            InputEvent::Composition => {}
            InputEvent::Insert { text, source } => {
                unverified_input |= *source == FragmentSource::Unverified;
                let units = count_effective_units(text);
                if *source == FragmentSource::Typed {
                    forward = forward.saturating_add(units);
                }
                fragments.push(Fragment {
                    text: text.clone(),
                    source: *source,
                });
            }
            InputEvent::Delete { units } => {
                let removed = remove_units(&mut fragments, *units);
                reverse_units = reverse_units.saturating_add(removed);
                reverse_actions = reverse_actions.saturating_add(1);
            }
            InputEvent::Replace {
                start,
                end,
                text,
                source,
            } => {
                unverified_input |= *source == FragmentSource::Unverified;
                if let Some(removed) = replace_range(&mut fragments, *start, *end, text, *source) {
                    if *source == FragmentSource::Typed {
                        forward = forward.saturating_add(count_effective_units(text));
                    }
                    if end > start {
                        reverse_units = reverse_units.saturating_add(removed);
                        reverse_actions = reverse_actions.saturating_add(1);
                    }
                } else {
                    valid_edits = false;
                }
            }
            InputEvent::Pause { ms } => pause_ms = pause_ms.saturating_add(excess_pause(*ms)),
            InputEvent::FocusLoss { ms } => focus_loss_ms = focus_loss_ms.saturating_add(*ms),
        }
    }
    let paste_units = fragments
        .iter()
        .filter(|fragment| fragment.source == FragmentSource::Pasted)
        .map(|fragment| count_effective_units(&fragment.text))
        .fold(0_u32, u32::saturating_add);
    let effective_positive_units = fragments
        .iter()
        .filter(|fragment| fragment.source == FragmentSource::Typed)
        .map(|fragment| count_effective_units(&fragment.text))
        .fold(0_u32, u32::saturating_add);
    let final_text: String = fragments
        .iter()
        .map(|fragment| fragment.text.as_str())
        .collect();
    let provenance = provenance_spans(&fragments);
    InputRound {
        telemetry: TurnTelemetry {
            forward_typed_units: forward,
            effective_positive_units,
            reverse_edited_units: reverse_units,
            reverse_edit_actions: reverse_actions,
            paste_units,
            pause_ms,
            focus_loss_ms,
            input_elapsed_ms,
            fragments,
            provenance,
            final_text,
            unverified_input,
        },
        timeline: events.to_vec(),
        valid_edits,
    }
}

pub fn apply_word_cap(round: &mut InputRound, word_cap: u32) {
    let final_snapshot = round.telemetry.final_text.clone();
    let (_prefix, overflow) = split_effective(&final_snapshot, word_cap);
    if overflow.is_empty() {
        return;
    }
    // Rebuild fragments: keep prefix sources proportionally by replaying unit budget.
    let mut kept = Vec::new();
    let mut used = 0_u32;
    for fragment in &round.telemetry.fragments {
        let units = count_effective_units(&fragment.text);
        if used >= word_cap {
            kept.push(Fragment {
                text: fragment.text.clone(),
                source: FragmentSource::Overflow,
            });
            continue;
        }
        if used + units <= word_cap {
            kept.push(fragment.clone());
            used += units;
        } else {
            let keep = word_cap - used;
            let (head, tail) = split_effective(&fragment.text, keep);
            if !head.is_empty() {
                kept.push(Fragment {
                    text: head.to_owned(),
                    source: fragment.source,
                });
            }
            if !tail.is_empty() {
                kept.push(Fragment {
                    text: tail.to_owned(),
                    source: FragmentSource::Overflow,
                });
            }
            used = word_cap;
        }
    }
    let final_text: String = kept.iter().map(|f| f.text.as_str()).collect();
    let effective_positive_units = kept
        .iter()
        .filter(|f| f.source == FragmentSource::Typed)
        .map(|f| count_effective_units(&f.text))
        .fold(0_u32, u32::saturating_add);
    let paste_units = kept
        .iter()
        .filter(|f| f.source == FragmentSource::Pasted)
        .map(|f| count_effective_units(&f.text))
        .fold(0_u32, u32::saturating_add);
    let provenance = provenance_spans(&kept);
    round.telemetry.fragments = kept;
    round.telemetry.final_text = final_text;
    round.telemetry.effective_positive_units = effective_positive_units;
    round.telemetry.paste_units = paste_units;
    round.telemetry.provenance = provenance;
}

pub fn scoreable_text(fragments: &[Fragment]) -> String {
    fragments
        .iter()
        .filter(|fragment| fragment.source == FragmentSource::Typed)
        .map(|fragment| fragment.text.as_str())
        .collect()
}

pub fn commit_round(round: &mut InputRound) -> &TurnTelemetry {
    round.timeline.clear();
    &round.telemetry
}

pub fn timeline_is_empty(round: &InputRound) -> bool {
    round.timeline.is_empty()
}

pub fn validate_human_turn(
    events: &[InputEvent],
    final_text: &str,
    claimed: &ClaimedMetrics,
    word_cap: u32,
) -> Result<TurnTelemetry, TelemetryReject> {
    if events.is_empty() && !final_text.is_empty() {
        return Err(TelemetryReject::EmptyTimeline);
    }
    let mut round = fold_with_elapsed(events, claimed.input_elapsed_ms);
    if !round.valid_edits {
        return Err(TelemetryReject::InvalidEdit);
    }
    apply_word_cap(&mut round, word_cap);
    if round.telemetry.final_text != final_text {
        // Allow whitespace-normalized match failures to reject.
        return Err(TelemetryReject::TextMismatch);
    }
    let t = &round.telemetry;
    if t.forward_typed_units != claimed.forward_typed_units
        || t.effective_positive_units != claimed.effective_positive_units
        || t.reverse_edited_units != claimed.reverse_units
        || t.reverse_edit_actions != claimed.reverse_actions
        || t.pause_ms != claimed.pause_ms
        || t.focus_loss_ms != claimed.focus_loss_ms
        || t.paste_units != claimed.paste_units
        || t.input_elapsed_ms != claimed.input_elapsed_ms
    {
        return Err(TelemetryReject::InconsistentMetrics);
    }
    commit_round(&mut round);
    Ok(round.telemetry.clone())
}

fn is_false(value: &bool) -> bool {
    !value
}

fn utf16_byte_offset(text: &str, target: u32) -> Option<usize> {
    let mut units = 0_u32;
    for (byte, character) in text.char_indices() {
        if units == target {
            return Some(byte);
        }
        units += character.len_utf16() as u32;
        if units > target {
            return None;
        }
    }
    (units == target).then_some(text.len())
}

fn replace_range(
    fragments: &mut Vec<Fragment>,
    start: u32,
    end: u32,
    text: &str,
    source: FragmentSource,
) -> Option<u32> {
    if start > end {
        return None;
    }
    let original: String = fragments.iter().map(|f| f.text.as_str()).collect();
    let from = utf16_byte_offset(&original, start)?;
    let to = utf16_byte_offset(&original, end)?;
    let removed = count_effective_units(&original[from..to]);
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut offset = 0_usize;
    for fragment in fragments.iter() {
        let length = fragment.text.len();
        let head = &fragment.text[..from.saturating_sub(offset).min(length)];
        let tail = &fragment.text[to.saturating_sub(offset).min(length)..];
        if !head.is_empty() {
            before.push(Fragment {
                text: head.to_owned(),
                source: fragment.source,
            });
        }
        if !tail.is_empty() {
            after.push(Fragment {
                text: tail.to_owned(),
                source: fragment.source,
            });
        }
        offset += length;
    }
    if !text.is_empty() {
        before.push(Fragment {
            text: text.to_owned(),
            source,
        });
    }
    before.extend(after);
    fragments.clear();
    for fragment in before {
        if let Some(last) = fragments
            .last_mut()
            .filter(|last| last.source == fragment.source)
        {
            last.text.push_str(&fragment.text);
        } else {
            fragments.push(fragment);
        }
    }
    Some(removed)
}

fn provenance_spans(fragments: &[Fragment]) -> Vec<ProvenanceSpan> {
    let mut cursor = 0_u32;
    let mut spans = Vec::new();
    for fragment in fragments {
        let units = count_effective_units(&fragment.text);
        if units == 0 {
            continue;
        }
        spans.push(ProvenanceSpan {
            start_units: cursor,
            end_units: cursor + units,
            source: fragment.source,
        });
        cursor += units;
    }
    spans
}

fn remove_units(fragments: &mut Vec<Fragment>, mut units: u32) -> u32 {
    let mut removed = 0_u32;
    while units > 0 {
        let Some(fragment) = fragments.last_mut() else {
            break;
        };
        let available = count_effective_units(&fragment.text);
        if available == 0 {
            fragments.pop();
            continue;
        }
        if available <= units {
            units -= available;
            removed += available;
            fragments.pop();
        } else {
            let keep = available - units;
            let (prefix, _) = crate::domain::text_units::split_effective(&fragment.text, keep);
            fragment.text = prefix.to_owned();
            removed += units;
            units = 0;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::{
        apply_word_cap, commit_round, fold, fold_with_elapsed, timeline_is_empty,
        validate_human_turn, ClaimedMetrics, Fragment, FragmentSource, InputEvent, TelemetryReject,
    };

    #[test]
    fn mobile_edits_match_shared_fixture_without_relaxing_validation() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            name: String,
            events: Vec<InputEvent>,
            text: String,
            claimed: ClaimedMetrics,
        }
        let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/telemetry/mobile-edits.json"
        ))
        .unwrap();
        for fixture in fixtures {
            let telemetry =
                validate_human_turn(&fixture.events, &fixture.text, &fixture.claimed, 720)
                    .unwrap_or_else(|err| panic!("{}: {:?}", fixture.name, err));
            assert_eq!(telemetry.final_text, fixture.text);
            assert_eq!(
                validate_human_turn(&fixture.events, "被篡改的最终文本", &fixture.claimed, 720),
                Err(TelemetryReject::TextMismatch)
            );
        }
    }

    #[test]
    fn rejects_surrogate_split_and_out_of_range_edits_even_when_final_text_matches() {
        let mut events = vec![InputEvent::Insert {
            text: "😀甲".to_owned(),
            source: FragmentSource::Typed,
        }];
        let claimed = ClaimedMetrics {
            input_elapsed_ms: 1,
            forward_typed_units: 1,
            effective_positive_units: 1,
            reverse_units: 0,
            reverse_actions: 0,
            pause_ms: 0,
            focus_loss_ms: 0,
            paste_units: 0,
        };
        for (start, end) in [(1, 1), (4, 4), (2, 1)] {
            events.truncate(1);
            events.push(InputEvent::Replace {
                start,
                end,
                text: String::new(),
                source: FragmentSource::Typed,
            });
            assert_eq!(
                validate_human_turn(&events, "😀甲", &claimed, 720),
                Err(TelemetryReject::InvalidEdit)
            );
        }
    }

    #[test]
    fn composition_is_not_extra_input() {
        let round = fold(&[
            InputEvent::Composition,
            InputEvent::Composition,
            InputEvent::Insert {
                text: "你好".to_owned(),
                source: FragmentSource::Typed,
            },
        ]);
        assert_eq!(round.telemetry.forward_typed_units, 2);
        assert_eq!(round.telemetry.reverse_edit_actions, 0);
    }

    #[test]
    fn deleting_committed_han_counts_units_and_actions() {
        let round = fold(&[
            InputEvent::Insert {
                text: "你好".to_owned(),
                source: FragmentSource::Typed,
            },
            InputEvent::Delete { units: 1 },
        ]);
        assert_eq!(round.telemetry.reverse_edited_units, 1);
        assert_eq!(round.telemetry.reverse_edit_actions, 1);
        assert_eq!(round.telemetry.effective_positive_units, 1);
    }

    #[test]
    fn pasted_remainder_keeps_its_source() {
        let round = fold(&[
            InputEvent::Insert {
                text: "你好世界".to_owned(),
                source: FragmentSource::Pasted,
            },
            InputEvent::Delete { units: 1 },
        ]);
        assert_eq!(round.telemetry.fragments.len(), 1);
        assert_eq!(round.telemetry.fragments[0].source, FragmentSource::Pasted);
        assert_eq!(round.telemetry.paste_units, 3);
        assert_eq!(round.telemetry.effective_positive_units, 0);
    }

    #[test]
    fn folding_twice_is_stable_and_commit_drops_the_timeline() {
        let events = [InputEvent::Insert {
            text: "论点".to_owned(),
            source: FragmentSource::Typed,
        }];
        let first = fold(&events);
        let second = fold(&events);
        assert_eq!(first.telemetry, second.telemetry);
        let mut round = first;
        let metrics = commit_round(&mut round).clone();
        assert!(timeline_is_empty(&round));
        assert_eq!(metrics.forward_typed_units, 2);
    }

    #[test]
    fn a_pause_keeps_only_the_excess_over_the_grace() {
        let round = fold(&[
            InputEvent::Pause { ms: 2_500 },
            InputEvent::Pause { ms: 4_000 },
        ]);
        assert_eq!(round.telemetry.pause_ms, 1_500);
    }

    #[test]
    fn overflow_paste_and_interrupted_text_are_not_scoreable() {
        use super::scoreable_text;
        let text = scoreable_text(&[
            Fragment {
                text: "有效".to_owned(),
                source: FragmentSource::Typed,
            },
            Fragment {
                text: "粘贴".to_owned(),
                source: FragmentSource::Pasted,
            },
            Fragment {
                text: "超时".to_owned(),
                source: FragmentSource::Overflow,
            },
        ]);
        assert_eq!(text, "有效");
    }

    #[test]
    fn pinyin_composition_then_commit_counts_once() {
        let events = [
            InputEvent::Composition,
            InputEvent::Composition,
            InputEvent::Insert {
                text: "人工智能".to_owned(),
                source: FragmentSource::Typed,
            },
        ];
        let a = fold(&events);
        let b = fold(&events);
        assert_eq!(a.telemetry.forward_typed_units, 4);
        assert_eq!(a.telemetry, b.telemetry);
    }

    #[test]
    fn select_replace_three_han() {
        let round = fold(&[
            InputEvent::Insert {
                text: "甲乙丙丁".to_owned(),
                source: FragmentSource::Typed,
            },
            InputEvent::Delete { units: 3 },
            InputEvent::Insert {
                text: "替换词".to_owned(),
                source: FragmentSource::Typed,
            },
        ]);
        assert_eq!(round.telemetry.reverse_edit_actions, 1);
        assert_eq!(round.telemetry.reverse_edited_units, 3);
        assert_eq!(round.telemetry.final_text, "甲替换词");
    }

    #[test]
    fn paste_then_delete_half() {
        let round = fold(&[
            InputEvent::Insert {
                text: "一二三四".to_owned(),
                source: FragmentSource::Pasted,
            },
            InputEvent::Delete { units: 2 },
        ]);
        assert_eq!(round.telemetry.paste_units, 2);
        assert_eq!(round.telemetry.final_text, "一二");
    }

    #[test]
    fn initial_mid_and_submit_pauses() {
        let round = fold(&[
            InputEvent::Pause { ms: 3_000 },
            InputEvent::Insert {
                text: "点".to_owned(),
                source: FragmentSource::Typed,
            },
            InputEvent::Pause { ms: 5_000 },
            InputEvent::Insert {
                text: "评".to_owned(),
                source: FragmentSource::Typed,
            },
            InputEvent::Pause { ms: 4_000 },
        ]);
        assert_eq!(round.telemetry.pause_ms, 500 + 2_500 + 1_500);
    }

    #[test]
    fn focus_loss_is_tracked() {
        let round = fold(&[
            InputEvent::Insert {
                text: "开".to_owned(),
                source: FragmentSource::Typed,
            },
            InputEvent::FocusLoss { ms: 1_200 },
            InputEvent::Insert {
                text: "始".to_owned(),
                source: FragmentSource::Typed,
            },
        ]);
        assert_eq!(round.telemetry.focus_loss_ms, 1_200);
    }

    #[test]
    fn overflow_beyond_cap_marks_provenance() {
        let mut round = fold(&[InputEvent::Insert {
            text: "一二三四五六".to_owned(),
            source: FragmentSource::Typed,
        }]);
        apply_word_cap(&mut round, 4);
        assert!(round
            .telemetry
            .provenance
            .iter()
            .any(|span| span.source == FragmentSource::Overflow));
        assert_eq!(count_prefix_units(&round.telemetry.final_text, 4), 4);
    }

    #[test]
    fn rejects_inconsistent_claimed_metrics() {
        let events = [InputEvent::Insert {
            text: "你好".to_owned(),
            source: FragmentSource::Typed,
        }];
        let err = validate_human_turn(
            &events,
            "你好",
            &ClaimedMetrics {
                input_elapsed_ms: 100,
                forward_typed_units: 99,
                effective_positive_units: 2,
                reverse_units: 0,
                reverse_actions: 0,
                pause_ms: 0,
                focus_loss_ms: 0,
                paste_units: 0,
            },
            100,
        )
        .expect_err("reject");
        assert_eq!(err, TelemetryReject::InconsistentMetrics);
    }

    #[test]
    fn accepts_consistent_claimed_metrics_and_clears_timeline() {
        let events = [
            InputEvent::Pause { ms: 3_000 },
            InputEvent::Insert {
                text: "你好".to_owned(),
                source: FragmentSource::Typed,
            },
        ];
        let folded = fold_with_elapsed(&events, 4_000);
        let ok = validate_human_turn(
            &events,
            "你好",
            &ClaimedMetrics {
                input_elapsed_ms: 4_000,
                forward_typed_units: folded.telemetry.forward_typed_units,
                effective_positive_units: folded.telemetry.effective_positive_units,
                reverse_units: folded.telemetry.reverse_edited_units,
                reverse_actions: folded.telemetry.reverse_edit_actions,
                pause_ms: folded.telemetry.pause_ms,
                focus_loss_ms: folded.telemetry.focus_loss_ms,
                paste_units: folded.telemetry.paste_units,
            },
            100,
        )
        .expect("ok");
        assert_eq!(ok.forward_typed_units, 2);
    }

    fn count_prefix_units(text: &str, cap: u32) -> u32 {
        crate::domain::text_units::count_effective_units(
            crate::domain::text_units::split_effective(text, cap).0,
        )
    }
}
