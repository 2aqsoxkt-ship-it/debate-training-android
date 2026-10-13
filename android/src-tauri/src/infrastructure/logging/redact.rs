use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

const SENSITIVE_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "authorization",
    "password",
    "secret",
    "token",
    "cookie",
    "prompt",
    "draft",
    "keystroke",
    "private_key",
    "access_token",
    "refresh_token",
];

static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b(api[_-]?key|authorization|password|secret|token|cookie|prompt|draft|",
        r"keystroke|private[_-]?key|access[_-]?token|refresh[_-]?token)\b\s*[:=]\s*",
        r#"("[^"]*"|'[^']*'|\S+)"#
    ))
    .expect("assignment redaction pattern must compile")
});

static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bbearer\s+\S+").expect("bearer pattern must compile"));

static SECRET_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bsk-[A-Za-z0-9_\-]{8,}").expect("secret token pattern must compile")
});

pub fn redact(input: &str) -> String {
    let without_bearer = BEARER.replace_all(input, "Bearer [REDACTED]");
    let without_assignments =
        ASSIGNMENT.replace_all(&without_bearer, |captures: &regex::Captures| {
            let value = &captures[2];
            if value.eq_ignore_ascii_case("bearer") || value == "[REDACTED]" {
                captures[0].to_owned()
            } else {
                format!("{}=[REDACTED]", &captures[1])
            }
        });
    SECRET_TOKEN
        .replace_all(&without_assignments, "[REDACTED]")
        .into_owned()
}

pub fn is_sensitive_key(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .flat_map(|character| character.to_lowercase())
        .map(|character| if character == '-' { '_' } else { character })
        .collect();

    SENSITIVE_KEYS
        .iter()
        .any(|key| normalized == *key || normalized.ends_with(&format!("_{key}")))
}

pub fn redact_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *child = Value::String("[REDACTED]".to_owned());
                } else {
                    redact_json(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_json(item);
            }
        }
        Value::String(text) => {
            *text = redact(text);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{redact, redact_json};
    use serde_json::json;

    #[test]
    fn leaves_ordinary_debate_text_unchanged() {
        let text = "正方一辩讨论 token 这个概念，但不包含密钥。";
        assert_eq!(redact(text), text);
    }

    #[test]
    fn redacts_assignments_bearer_tokens_and_secret_prefixes() {
        assert_eq!(redact("api_key=sk-live-secretvalue"), "api_key=[REDACTED]");
        assert_eq!(
            redact("Authorization: Bearer abc.def.ghi"),
            "Authorization: Bearer [REDACTED]"
        );
        assert_eq!(redact("note sk-abcdefghijklmn"), "note [REDACTED]");
    }

    #[test]
    fn redacts_sensitive_json_fields_and_keeps_public_text() {
        let mut value = json!({
            "topic": "人工智能是否应被禁止",
            "apiKey": "sk-abcdefghijklmn",
            "nested": { "password": "hunter2", "note": "see sk-zzzzzzzzzz" }
        });

        redact_json(&mut value);

        assert_eq!(value["topic"], "人工智能是否应被禁止");
        assert_eq!(value["apiKey"], "[REDACTED]");
        assert_eq!(value["nested"]["password"], "[REDACTED]");
        assert_eq!(value["nested"]["note"], "see [REDACTED]");
    }
}
