use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::infrastructure::db::AppSettings;

pub const DEFAULT_MODEL_ID: &str = "deepseek-flash";
pub const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
pub const PROMPT_BUNDLE_VERSION: &str = "prompt-bundle.v1";
pub const DEFAULT_THINKING: &str = "enabled";
pub const DEFAULT_REASONING_EFFORT: &str = "high";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    ParseRules,
    AnalyzeTopic,
    BuildStrategy,
    SelectCrossExam,
    GenerateTurn,
    DecideInterruption,
    ScoreStage,
    ReviewMatch,
    JudgeMatch,
}

impl Purpose {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParseRules => "parse_rules",
            Self::AnalyzeTopic => "analyze_topic",
            Self::BuildStrategy => "build_strategy",
            Self::SelectCrossExam => "select_cross_exam",
            Self::GenerateTurn => "generate_turn",
            Self::DecideInterruption => "decide_interruption",
            Self::ScoreStage => "score_stage",
            Self::ReviewMatch => "review_match",
            Self::JudgeMatch => "judge_match",
        }
    }

    pub const fn summary(self) -> &'static str {
        match self {
            Self::ParseRules => "parse rules",
            Self::AnalyzeTopic => "analyze topic",
            Self::BuildStrategy => "build strategy",
            Self::SelectCrossExam => "select cross-exam roles",
            Self::GenerateTurn => "generate turn",
            Self::DecideInterruption => "decide interruption",
            Self::ScoreStage => "score stage",
            Self::ReviewMatch => "review match",
            Self::JudgeMatch => "judge match",
        }
    }

    pub const fn live(self) -> bool {
        matches!(
            self,
            Self::GenerateTurn | Self::DecideInterruption | Self::SelectCrossExam
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelConfig {
    pub model_id: String,
    pub base_url: String,
    pub thinking: String,
    pub reasoning_effort: String,
    pub backoff_ms: u64,
    pub timeout_ms: u64,
}

impl ModelConfig {
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            model_id: if settings.model_id.is_empty() {
                DEFAULT_MODEL_ID.to_owned()
            } else {
                settings.model_id.clone()
            },
            base_url: if settings.base_url.is_empty() {
                DEFAULT_BASE_URL.to_owned()
            } else {
                settings.base_url.clone()
            },
            thinking: DEFAULT_THINKING.to_owned(),
            reasoning_effort: if settings.thinking_level.is_empty() {
                DEFAULT_REASONING_EFFORT.to_owned()
            } else {
                settings.thinking_level.clone()
            },
            backoff_ms: 200,
            timeout_ms: 30_000,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AiContext {
    pub topic: String,
    pub rules_text: String,
    pub materials: String,
    pub own_strategy: String,
    pub opponent_strategy: String,
    pub public_transcript: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallStatus {
    Success,
    Timeout,
    Cancelled,
    SchemaInvalid,
    Unavailable,
}

impl CallStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::SchemaInvalid => "schema_invalid",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiCallRecord {
    pub request_id: String,
    pub purpose: Purpose,
    pub model_id: String,
    pub thinking: String,
    pub prompt_bundle_version: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub duration_ms: u64,
    pub status: CallStatus,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRequest {
    pub purpose: Purpose,
    pub messages: Vec<PromptMessage>,
    pub repair: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionResponse {
    pub content: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFault {
    OutputBudgetExhausted,
    Timeout,
    Cancelled,
    Unavailable,
}

pub trait ModelTransport {
    fn complete(
        &self,
        request: &CompletionRequest,
        cancel: &AtomicBool,
    ) -> Result<CompletionResponse, TransportFault>;
}

pub fn log_line(record: &AiCallRecord) -> String {
    format!(
        "request_id={} purpose={} model={} thinking={} prompt_bundle={} prompt_tokens={} completion_tokens={} duration_ms={} status={} summary={}",
        record.request_id,
        record.purpose.as_str(),
        record.model_id,
        record.thinking,
        record.prompt_bundle_version,
        record.prompt_tokens,
        record.completion_tokens,
        record.duration_ms,
        record.status.as_str(),
        record.summary
    )
}
