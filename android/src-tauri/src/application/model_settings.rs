use serde::Serialize;

use crate::error::{AppError, ErrorCode};
use crate::infrastructure::ai::{endpoint, ModelConfig, DEFAULT_MODEL_ID};
use crate::infrastructure::credentials::SecretStore;
use crate::infrastructure::db::AppSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    NotConfigured,
    Untested,
    Testing,
    Ready,
    AuthenticationFailed,
    NetworkFailed,
    Timeout,
    ModelUnavailable,
    InsufficientBalance,
    IncompatibleResponse,
}

impl ConnectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::Untested => "untested",
            Self::Testing => "testing",
            Self::Ready => "ready",
            Self::AuthenticationFailed => "authentication_failed",
            Self::NetworkFailed => "network_failed",
            Self::Timeout => "timeout",
            Self::ModelUnavailable => "model_unavailable",
            Self::InsufficientBalance => "insufficient_balance",
            Self::IncompatibleResponse => "incompatible_response",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "not_configured" => Self::NotConfigured,
            "untested" => Self::Untested,
            "testing" => Self::Testing,
            "ready" => Self::Ready,
            "authentication_failed" => Self::AuthenticationFailed,
            "network_failed" => Self::NetworkFailed,
            "timeout" => Self::Timeout,
            "model_unavailable" => Self::ModelUnavailable,
            "insufficient_balance" => Self::InsufficientBalance,
            "incompatible_response" => Self::IncompatibleResponse,
            _ => return None,
        })
    }

    pub fn error_category(self) -> Option<&'static str> {
        match self {
            Self::AuthenticationFailed => Some("authentication"),
            Self::NetworkFailed => Some("network"),
            Self::Timeout => Some("timeout"),
            Self::ModelUnavailable => Some("model_unavailable"),
            Self::InsufficientBalance => Some("insufficient_balance"),
            Self::IncompatibleResponse => Some("incompatible_response"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettingsDto {
    pub model_id: String,
    pub base_url: String,
    pub reasoning_level: String,
    pub connection_status: ConnectionStatus,
    pub last_tested_at: String,
    pub last_error_category: String,
    pub has_api_key: bool,
}

impl ModelSettingsDto {
    pub fn from_parts(settings: &AppSettings, has_api_key: bool) -> Self {
        let stored = ConnectionStatus::parse(&settings.connection_status)
            .unwrap_or(ConnectionStatus::NotConfigured);
        let status = if !has_api_key {
            ConnectionStatus::NotConfigured
        } else if stored == ConnectionStatus::NotConfigured {
            ConnectionStatus::Untested
        } else {
            stored
        };
        Self {
            model_id: if settings.model_id.is_empty() {
                DEFAULT_MODEL_ID.to_owned()
            } else {
                settings.model_id.clone()
            },
            base_url: if settings.base_url.is_empty() {
                crate::infrastructure::ai::DEFAULT_BASE_URL.to_owned()
            } else {
                settings.base_url.clone()
            },
            reasoning_level: if settings.thinking_level.is_empty() {
                "high".to_owned()
            } else {
                settings.thinking_level.clone()
            },
            connection_status: status,
            last_tested_at: settings.last_tested_at.clone(),
            last_error_category: status.error_category().unwrap_or("").to_owned(),
            has_api_key,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeFault {
    Authentication,
    Network,
    Timeout,
    ModelUnavailable,
    InsufficientBalance,
    Incompatible,
}

pub trait ConnectionProbe {
    fn probe(&self, config: &ModelConfig, api_key: &str) -> Result<(), ProbeFault>;
}

pub struct DeepSeekConnectionProbe;

impl ConnectionProbe for DeepSeekConnectionProbe {
    fn probe(&self, config: &ModelConfig, api_key: &str) -> Result<(), ProbeFault> {
        if api_key.is_empty() {
            return Err(ProbeFault::Authentication);
        }
        let url = endpoint(&config.base_url);
        let body = serde_json::json!({
            "model": config.model_id,
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1,
            "stream": false,
            "thinking": { "type": "disabled" }
        })
        .to_string();
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(20))
            .build();
        let ticket = crate::infrastructure::ai::budget::BudgetTicket::from_env(&url, &body)
            .map_err(|_| ProbeFault::Network)?;
        match agent
            .post(&url)
            .set("Authorization", &format!("Bearer {api_key}"))
            .set("Content-Type", "application/json")
            .send_string(&body)
        {
            Ok(response) => {
                let status = response.status();
                let text = response.into_string().unwrap_or_default();
                if let Some(ticket) = ticket {
                    ticket
                        .finish(
                            crate::infrastructure::ai::budget::usage_from_body(&text),
                            true,
                        )
                        .map_err(|_| ProbeFault::Network)?;
                }
                classify_http(status, &text)
            }
            Err(ureq::Error::Status(code, response)) => {
                let text = response.into_string().unwrap_or_default();
                if let Some(ticket) = ticket {
                    let _ = ticket.finish(None, false);
                }
                classify_http(code, &text)
            }
            Err(error) => {
                if let Some(ticket) = ticket {
                    let _ = ticket.finish(None, false);
                }
                let message = error.to_string().to_ascii_lowercase();
                if message.contains("timed out") || message.contains("timeout") {
                    Err(ProbeFault::Timeout)
                } else {
                    Err(ProbeFault::Network)
                }
            }
        }
    }
}

fn classify_http(status: u16, body: &str) -> Result<(), ProbeFault> {
    if (200..300).contains(&status) {
        if body.contains("\"choices\"") || body.contains("\"id\"") {
            return Ok(());
        }
        return Err(ProbeFault::Incompatible);
    }
    let lower = body.to_ascii_lowercase();
    if status == 402 || lower.contains("insufficient balance") {
        return Err(ProbeFault::InsufficientBalance);
    }
    if status == 401 || status == 403 || lower.contains("invalid") && lower.contains("key") {
        return Err(ProbeFault::Authentication);
    }
    if status == 404 || lower.contains("model") && lower.contains("not found") {
        return Err(ProbeFault::ModelUnavailable);
    }
    if status == 408 || status == 504 {
        return Err(ProbeFault::Timeout);
    }
    if status == 429 {
        return Err(ProbeFault::Network);
    }
    if status >= 500 {
        return Err(ProbeFault::Network);
    }
    Err(ProbeFault::Incompatible)
}

pub fn save_api_key_only(secrets: &dyn SecretStore, key: &str) -> Result<(), AppError> {
    secrets.save_api_key(key)
}

pub fn clear_api_key_only(secrets: &dyn SecretStore) -> Result<(), AppError> {
    secrets.clear_api_key()
}

pub fn has_api_key(secrets: &dyn SecretStore) -> bool {
    secrets.read_api_key().is_ok()
}

pub fn apply_model_fields(
    settings: &mut AppSettings,
    model_id: Option<String>,
    base_url: Option<String>,
    reasoning_level: Option<String>,
) -> Result<(), AppError> {
    if let Some(model_id) = model_id {
        settings.model_id = model_id.trim().to_owned();
    }
    if let Some(base_url) = base_url {
        settings.base_url = base_url.trim().to_owned();
    }
    if let Some(level) = reasoning_level {
        let level = level.trim().to_owned();
        if !matches!(level.as_str(), "low" | "high" | "max") {
            return Err(AppError::new(
                ErrorCode::InvalidArgument,
                "thinking level is invalid",
            ));
        }
        settings.thinking_level = level;
    }
    Ok(())
}

pub fn mark_untested(settings: &mut AppSettings) {
    settings.connection_status = ConnectionStatus::Untested.as_str().to_owned();
    settings.last_tested_at.clear();
}

pub fn mark_not_configured(settings: &mut AppSettings) {
    settings.connection_status = ConnectionStatus::NotConfigured.as_str().to_owned();
    settings.last_tested_at.clear();
}

pub fn mark_probe_result(
    settings: &mut AppSettings,
    result: Result<(), ProbeFault>,
    stamped_at: &str,
) -> ConnectionStatus {
    let status = match result {
        Ok(()) => ConnectionStatus::Ready,
        Err(ProbeFault::Authentication) => ConnectionStatus::AuthenticationFailed,
        Err(ProbeFault::Network) => ConnectionStatus::NetworkFailed,
        Err(ProbeFault::Timeout) => ConnectionStatus::Timeout,
        Err(ProbeFault::ModelUnavailable) => ConnectionStatus::ModelUnavailable,
        Err(ProbeFault::InsufficientBalance) => ConnectionStatus::InsufficientBalance,
        Err(ProbeFault::Incompatible) => ConnectionStatus::IncompatibleResponse,
    };
    settings.connection_status = status.as_str().to_owned();
    settings.last_tested_at = stamped_at.to_owned();
    status
}

pub fn live_allowed(status: ConnectionStatus) -> bool {
    status == ConnectionStatus::Ready
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::credentials::MemorySecretStore;

    struct FakeProbe {
        result: Result<(), ProbeFault>,
    }

    impl ConnectionProbe for FakeProbe {
        fn probe(&self, _config: &ModelConfig, api_key: &str) -> Result<(), ProbeFault> {
            if api_key.is_empty() {
                return Err(ProbeFault::Authentication);
            }
            self.result
        }
    }

    #[test]
    fn without_a_key_the_status_is_not_configured() {
        let secrets = MemorySecretStore::new();
        let dto = ModelSettingsDto::from_parts(&AppSettings::default(), has_api_key(&secrets));
        assert!(!dto.has_api_key);
        assert_eq!(dto.connection_status, ConnectionStatus::NotConfigured);
        assert!(dto.last_error_category.is_empty());
    }

    #[test]
    fn saving_a_key_marks_untested_and_never_returns_the_secret() {
        let secrets = MemorySecretStore::new();
        let secret = "sk-r1-settings-secret";
        save_api_key_only(&secrets, secret).expect("save");
        let mut settings = AppSettings::default();
        mark_untested(&mut settings);
        let dto = ModelSettingsDto::from_parts(&settings, has_api_key(&secrets));
        assert!(dto.has_api_key);
        assert_eq!(dto.connection_status, ConnectionStatus::Untested);
        let encoded = serde_json::to_string(&dto).expect("json");
        assert!(!encoded.contains(secret));
        assert!(!encoded.contains("sk-r1"));
    }

    #[test]
    fn probe_success_becomes_ready_and_failures_keep_categories() {
        let secrets = MemorySecretStore::new();
        secrets.save_api_key("sk-r1-ok").expect("save");
        let mut settings = AppSettings::default();
        let probe = FakeProbe { result: Ok(()) };
        let config = ModelConfig::from_settings(&settings);
        let status = mark_probe_result(
            &mut settings,
            probe.probe(&config, &secrets.read_api_key().expect("key")),
            "2026-09-23T09:00:00Z",
        );
        assert_eq!(status, ConnectionStatus::Ready);
        assert!(live_allowed(status));

        let probe = FakeProbe {
            result: Err(ProbeFault::Authentication),
        };
        let status = mark_probe_result(
            &mut settings,
            probe.probe(&config, "sk-r1-ok"),
            "2026-09-23T09:01:00Z",
        );
        assert_eq!(status, ConnectionStatus::AuthenticationFailed);
        assert!(!live_allowed(status));
        let dto = ModelSettingsDto::from_parts(&settings, true);
        assert_eq!(dto.last_error_category, "authentication");
    }

    #[test]
    fn clearing_returns_to_not_configured() {
        let secrets = MemorySecretStore::new();
        secrets.save_api_key("sk-r1-clear").expect("save");
        clear_api_key_only(&secrets).expect("clear");
        let mut settings = AppSettings {
            connection_status: ConnectionStatus::Ready.as_str().to_owned(),
            ..AppSettings::default()
        };
        mark_not_configured(&mut settings);
        let dto = ModelSettingsDto::from_parts(&settings, has_api_key(&secrets));
        assert_eq!(dto.connection_status, ConnectionStatus::NotConfigured);
        assert!(!dto.has_api_key);
    }

    #[test]
    fn rate_limit_is_reported_as_temporary_network_failure() {
        assert_eq!(
            classify_http(429, r#"{"error":{"message":"rate limit"}}"#),
            Err(ProbeFault::Network)
        );
    }

    #[test]
    fn insufficient_balance_has_its_own_actionable_status() {
        assert_eq!(
            classify_http(402, r#"{"error":{"message":"Insufficient Balance"}}"#),
            Err(ProbeFault::InsufficientBalance)
        );
        let mut settings = AppSettings::default();
        assert_eq!(
            mark_probe_result(
                &mut settings,
                Err(ProbeFault::InsufficientBalance),
                "2026-10-03T10:00:00Z"
            ),
            ConnectionStatus::InsufficientBalance
        );
        assert_eq!(settings.connection_status, "insufficient_balance");
    }
}
