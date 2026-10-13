mod redact;

use std::fmt;

use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::registry::LookupSpan;

pub use redact::{redact, redact_json};

pub fn init() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .event_format(RedactingFormat)
        .try_init();
}

struct RedactingFormat;

struct FieldVisitor {
    parts: Vec<String>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.parts
            .push(redact(&format!("{}={value:?}", field.name())));
    }
}

impl<S, N> FormatEvent<S, N> for RedactingFormat
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut visitor = FieldVisitor { parts: Vec::new() };
        event.record(&mut visitor);
        writeln!(
            writer,
            "{} {}",
            event.metadata().level(),
            visitor.parts.join(" ")
        )
    }
}
