//! Validate human turn telemetry then submit through Desk.

use crate::application::desk::Desk;
use crate::domain::telemetry::{
    validate_human_turn, ClaimedMetrics, InputEvent, TelemetryReject, TurnTelemetry,
};

pub fn submit_validated_human_turn(
    desk: &mut Desk,
    text: String,
    events: &[InputEvent],
    claimed: &ClaimedMetrics,
) -> Result<TurnTelemetry, String> {
    let metrics = validate_human_turn_for_desk(desk, &text, events, claimed)?;
    let pasted = metrics.paste_units > 0;
    desk.submit(
        text,
        metrics.input_elapsed_ms.max(1),
        metrics.pause_ms,
        metrics.reverse_edited_units,
        metrics.reverse_edit_actions,
        pasted,
    )?;
    desk.apply_last_validated_input_counts(
        metrics.forward_typed_units,
        metrics.effective_positive_units,
    );
    if metrics.unverified_input {
        desk.note_unverified_input();
    }
    Ok(metrics)
}

pub fn validate_human_turn_for_desk(
    desk: &Desk,
    text: &str,
    events: &[InputEvent],
    claimed: &ClaimedMetrics,
) -> Result<TurnTelemetry, String> {
    let word_cap = desk
        .aggregate
        .as_ref()
        .map(|state| state.current_word_cap())
        .unwrap_or(1);
    let metrics =
        validate_human_turn(events, text, claimed, word_cap).map_err(|err| match err {
            TelemetryReject::InconsistentMetrics
            | TelemetryReject::TextMismatch
            | TelemetryReject::EmptyTimeline
            | TelemetryReject::InvalidEdit => err.message().to_owned(),
        })?;
    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ruleset::MatchMode, telemetry::FragmentSource, visibility::Difficulty};

    #[test]
    fn unverified_input_is_saved_without_claiming_standard_typing_metrics() {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "手机输入验收".to_owned(),
            String::new(),
            None,
            &[0],
            Difficulty::Standard,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        desk.analyze().unwrap();
        desk.assign_and_ready(None, &[0]).unwrap();
        desk.start().unwrap();
        let events = [InputEvent::Replace {
            start: 0,
            end: 0,
            text: "未知来源".to_owned(),
            source: FragmentSource::Unverified,
        }];
        let claimed = ClaimedMetrics {
            input_elapsed_ms: 1000,
            forward_typed_units: 0,
            effective_positive_units: 0,
            reverse_units: 0,
            reverse_actions: 0,
            pause_ms: 0,
            focus_loss_ms: 0,
            paste_units: 0,
        };
        let metrics =
            submit_validated_human_turn(&mut desk, "未知来源".to_owned(), &events, &claimed)
                .unwrap();
        assert!(metrics.unverified_input);
        assert_eq!(desk.view().transcripts[0].effective_text, "未知来源");
        assert!(desk.view().low_confidence);
        assert!(!desk.view().standard);
    }
}
