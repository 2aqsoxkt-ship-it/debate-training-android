use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamIds {
    pub request_id: String,
    pub match_id: String,
    pub event_id: String,
}

impl StreamIds {
    pub fn new(match_id: impl Into<String>, event_id: impl Into<String>) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(1);
        Self {
            request_id: Uuid::from_u128(nanos).to_string(),
            match_id: match_id.into(),
            event_id: event_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    Started {
        #[serde(flatten)]
        ids: StreamIds,
    },
    Status {
        #[serde(flatten)]
        ids: StreamIds,
        label: String,
    },
    TextDelta {
        #[serde(flatten)]
        ids: StreamIds,
        text: String,
    },
    Usage {
        #[serde(flatten)]
        ids: StreamIds,
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    Completed {
        #[serde(flatten)]
        ids: StreamIds,
        text: String,
    },
    Failed {
        #[serde(flatten)]
        ids: StreamIds,
        category: String,
        message: String,
    },
    Cancelled {
        #[serde(flatten)]
        ids: StreamIds,
    },
}

pub trait StreamSink {
    fn emit(&mut self, event: StreamEvent);
}

pub struct CollectingSink {
    pub events: Vec<StreamEvent>,
}

impl CollectingSink {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }
}

impl Default for CollectingSink {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamSink for CollectingSink {
    fn emit(&mut self, event: StreamEvent) {
        self.events.push(event);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsePiece {
    Reasoning,
    Text(String),
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    Done,
}

pub fn parse_sse_data(data: &str) -> Vec<SsePiece> {
    let data = data.trim();
    if data.is_empty() {
        return Vec::new();
    }
    if data == "[DONE]" {
        return vec![SsePiece::Done];
    }
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };
    let mut pieces = Vec::new();
    let delta = &value["choices"][0]["delta"];
    if delta
        .get("reasoning_content")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.is_empty())
    {
        pieces.push(SsePiece::Reasoning);
    }
    if let Some(text) = delta.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            pieces.push(SsePiece::Text(text.to_owned()));
        }
    }
    if let Some(text) = value["choices"][0]["message"]["content"].as_str() {
        if !text.is_empty() {
            pieces.push(SsePiece::Text(text.to_owned()));
        }
    }
    if let Some(tokens) = value.get("usage") {
        pieces.push(SsePiece::Usage {
            prompt_tokens: tokens["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: tokens["completion_tokens"].as_u64().unwrap_or(0) as u32,
        });
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::{parse_sse_data, CollectingSink, SsePiece, StreamEvent, StreamIds, StreamSink};

    #[test]
    fn sse_text_deltas_are_separated_from_reasoning() {
        assert_eq!(
            parse_sse_data(r#"{"choices":[{"delta":{"reasoning_content":"..."}}]}"#),
            vec![SsePiece::Reasoning]
        );
        assert_eq!(
            parse_sse_data(r#"{"choices":[{"delta":{"content":"你好"}}]}"#),
            vec![SsePiece::Text("你好".to_owned())]
        );
        assert_eq!(parse_sse_data("[DONE]"), vec![SsePiece::Done]);
    }

    #[test]
    fn sink_keeps_request_binding() {
        let ids = StreamIds {
            request_id: "req-1".to_owned(),
            match_id: "match-1".to_owned(),
            event_id: "event-1".to_owned(),
        };
        let mut sink = CollectingSink::new();
        sink.emit(StreamEvent::Started { ids: ids.clone() });
        sink.emit(StreamEvent::TextDelta {
            ids: ids.clone(),
            text: "正".to_owned(),
        });
        sink.emit(StreamEvent::TextDelta {
            ids: ids.clone(),
            text: "方".to_owned(),
        });
        sink.emit(StreamEvent::Completed {
            ids,
            text: "正方".to_owned(),
        });
        assert_eq!(sink.events.len(), 4);
        match &sink.events[1] {
            StreamEvent::TextDelta { text, .. } => assert_eq!(text, "正"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
