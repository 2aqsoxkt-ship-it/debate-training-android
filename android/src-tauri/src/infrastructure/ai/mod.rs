pub mod budget;
mod deepseek;
mod gateway;
mod prompt;
pub mod schema;
mod speech_text;
mod stream;
mod types;

pub use deepseek::{completion_body, endpoint, fold_stream_chunks, DeepSeekTransport, UreqHttp};
pub use gateway::{FixtureTransport, Gateway, MockGateway, ThreadSleep, Wait};
pub use prompt::{build_prompt, joined, repair_messages, SYSTEM_CONSTRAINT};
pub use schema::{JudgementWinner, TeamStrategyV1, TopicAnalysisV1};
pub use speech_text::SpeechTextExtractor;
pub use stream::{parse_sse_data, CollectingSink, SsePiece, StreamEvent, StreamIds, StreamSink};
pub use types::{
    log_line, AiCallRecord, AiContext, CallStatus, CompletionRequest, CompletionResponse,
    ModelConfig, ModelTransport, PromptMessage, Purpose, TransportFault, DEFAULT_BASE_URL,
    DEFAULT_MODEL_ID, DEFAULT_REASONING_EFFORT, DEFAULT_THINKING, PROMPT_BUNDLE_VERSION,
};
