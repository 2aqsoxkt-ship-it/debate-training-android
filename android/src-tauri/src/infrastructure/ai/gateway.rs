use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use serde::de::DeserializeOwned;

use crate::error::{AppError, ErrorCode};

use super::prompt::{build_prompt, repair_messages};
use super::schema::{
    self, CrossExamSelectionV1, GeneratedTurnV1, InterruptionDecisionV1, MatchJudgementV1,
    MatchReviewV1, ParseRulesOutputV1, StageScoreV1, TeamStrategyV1, TopicAnalysisV1,
};
use super::types::{
    log_line, AiCallRecord, AiContext, CallStatus, CompletionRequest, CompletionResponse,
    ModelConfig, ModelTransport, Purpose, TransportFault, PROMPT_BUNDLE_VERSION,
};

pub trait Wait {
    fn wait_ms(&self, millis: u64);
}

#[derive(Debug, Default)]
pub struct NoWait;

impl Wait for NoWait {
    fn wait_ms(&self, _millis: u64) {}
}

#[derive(Debug, Default)]
pub struct ThreadSleep;

impl Wait for ThreadSleep {
    fn wait_ms(&self, millis: u64) {
        if millis > 0 {
            std::thread::sleep(std::time::Duration::from_millis(millis));
        }
    }
}

#[derive(Debug, Default)]
pub struct FixtureTransport;

impl ModelTransport for FixtureTransport {
    fn complete(
        &self,
        request: &CompletionRequest,
        cancel: &AtomicBool,
    ) -> Result<CompletionResponse, TransportFault> {
        if cancel.load(Ordering::SeqCst) {
            return Err(TransportFault::Cancelled);
        }
        Ok(CompletionResponse {
            content: schema::fixture_json(request.purpose),
            prompt_tokens: 3,
            completion_tokens: 5,
            elapsed_ms: 7,
        })
    }
}

pub struct Gateway<T, S> {
    transport: T,
    sleeper: S,
    config: ModelConfig,
    records: Mutex<Vec<AiCallRecord>>,
    next_id: AtomicU64,
}

pub type MockGateway = Gateway<FixtureTransport, NoWait>;

impl MockGateway {
    pub fn mock(config: ModelConfig) -> Self {
        Self::new(FixtureTransport, NoWait, config)
    }
}

impl<T: ModelTransport, S: Wait> Gateway<T, S> {
    pub fn new(transport: T, sleeper: S, config: ModelConfig) -> Self {
        Self {
            transport,
            sleeper,
            config,
            records: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn records(&self) -> Vec<AiCallRecord> {
        self.records.lock().expect("call log").clone()
    }

    pub fn parse_rules(
        &self,
        context: &AiContext,
        cancel: &AtomicBool,
    ) -> Result<ParseRulesOutputV1, AppError> {
        self.call(Purpose::ParseRules, context, cancel, schema::ensure_parse)
    }

    pub fn analyze_topic(
        &self,
        context: &AiContext,
        cancel: &AtomicBool,
    ) -> Result<TopicAnalysisV1, AppError> {
        self.call(Purpose::AnalyzeTopic, context, cancel, schema::ensure_topic)
    }

    pub fn build_strategy(
        &self,
        context: &AiContext,
        cancel: &AtomicBool,
    ) -> Result<TeamStrategyV1, AppError> {
        self.call(
            Purpose::BuildStrategy,
            context,
            cancel,
            schema::ensure_strategy,
        )
    }

    pub fn generate_turn(
        &self,
        context: &AiContext,
        cancel: &AtomicBool,
    ) -> Result<GeneratedTurnV1, AppError> {
        self.call(Purpose::GenerateTurn, context, cancel, schema::ensure_turn)
    }

    pub fn select_cross_exam(
        &self,
        context: &AiContext,
        cancel: &AtomicBool,
    ) -> Result<CrossExamSelectionV1, AppError> {
        self.call(
            Purpose::SelectCrossExam,
            context,
            cancel,
            schema::ensure_cross_exam_selection,
        )
    }

    pub fn decide_interruption(
        &self,
        context: &AiContext,
        cancel: &AtomicBool,
    ) -> Result<InterruptionDecisionV1, AppError> {
        self.call(
            Purpose::DecideInterruption,
            context,
            cancel,
            schema::ensure_interrupt,
        )
    }

    pub fn score_stage(
        &self,
        context: &AiContext,
        cancel: &AtomicBool,
    ) -> Result<StageScoreV1, AppError> {
        self.call(Purpose::ScoreStage, context, cancel, schema::ensure_score)
    }

    pub fn review_match(
        &self,
        context: &AiContext,
        cancel: &AtomicBool,
    ) -> Result<MatchReviewV1, AppError> {
        self.call(Purpose::ReviewMatch, context, cancel, schema::ensure_review)
    }

    pub fn judge_match(
        &self,
        context: &AiContext,
        cancel: &AtomicBool,
    ) -> Result<MatchJudgementV1, AppError> {
        self.call(Purpose::JudgeMatch, context, cancel, schema::ensure_judge)
    }

    fn call<V, F>(
        &self,
        purpose: Purpose,
        context: &AiContext,
        cancel: &AtomicBool,
        check: F,
    ) -> Result<V, AppError>
    where
        V: DeserializeOwned,
        F: Fn(&V) -> Result<(), AppError>,
    {
        let request_id = format!("req-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
        let messages = build_prompt(purpose, context);
        let first = self.attempt(purpose, &messages, false, cancel);
        let (status, tokens, duration, result) = match first {
            Ok(response) => self.accept_or_repair(purpose, response, cancel, &check),
            Err(fault) => (status_of(fault), (0, 0), 0, Err(error_of(fault))),
        };
        self.finish(AiCallRecord {
            request_id,
            purpose,
            model_id: self.config.model_id.clone(),
            thinking: format!("{}:{}", self.config.thinking, self.config.reasoning_effort),
            prompt_bundle_version: PROMPT_BUNDLE_VERSION.to_owned(),
            prompt_tokens: tokens.0,
            completion_tokens: tokens.1,
            duration_ms: duration,
            status,
            summary: purpose.summary().to_owned(),
        });
        result
    }

    fn accept_or_repair<V, F>(
        &self,
        purpose: Purpose,
        response: CompletionResponse,
        cancel: &AtomicBool,
        check: &F,
    ) -> (CallStatus, (u32, u32), u64, Result<V, AppError>)
    where
        V: DeserializeOwned,
        F: Fn(&V) -> Result<(), AppError>,
    {
        let duration = response.elapsed_ms;
        let tokens = (response.prompt_tokens, response.completion_tokens);
        if let Ok(value) = decode(&response.content, check) {
            return (CallStatus::Success, tokens, duration, Ok(value));
        }
        let repair = repair_messages(purpose, &response.content);
        match self.attempt(purpose, &repair, true, cancel) {
            Ok(repaired) => {
                let duration = duration + repaired.elapsed_ms;
                let tokens = (
                    tokens.0 + repaired.prompt_tokens,
                    tokens.1 + repaired.completion_tokens,
                );
                match decode(&repaired.content, check) {
                    Ok(value) => (CallStatus::Success, tokens, duration, Ok(value)),
                    Err(error) => (CallStatus::SchemaInvalid, tokens, duration, Err(error)),
                }
            }
            Err(fault) => (status_of(fault), tokens, duration, Err(error_of(fault))),
        }
    }

    fn attempt(
        &self,
        purpose: Purpose,
        messages: &[super::types::PromptMessage],
        repair: bool,
        cancel: &AtomicBool,
    ) -> Result<CompletionResponse, TransportFault> {
        let max = if repair {
            1
        } else if purpose.live() || std::env::var_os("DEBATE_ACCEPTANCE_LEDGER").is_some() {
            2
        } else {
            3
        };
        let mut attempt = 0u32;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(TransportFault::Cancelled);
            }
            let request = CompletionRequest {
                purpose,
                messages: messages.to_vec(),
                repair,
            };
            match self.transport.complete(&request, cancel) {
                Ok(_response) if cancel.load(Ordering::SeqCst) => {
                    return Err(TransportFault::Cancelled);
                }
                Ok(response) => return Ok(response),
                Err(TransportFault::Cancelled) => return Err(TransportFault::Cancelled),
                Err(TransportFault::OutputBudgetExhausted) => {
                    return Err(TransportFault::OutputBudgetExhausted)
                }
                Err(fault) => {
                    attempt += 1;
                    if attempt >= max {
                        return Err(fault);
                    }
                    let shift = attempt - 1;
                    let delay = self.config.backoff_ms.saturating_mul(1_u64 << shift.min(4));
                    self.sleeper.wait_ms(delay);
                }
            }
        }
    }

    fn finish(&self, record: AiCallRecord) {
        tracing::info!("{}", log_line(&record));
        self.records.lock().expect("call log").push(record);
    }
}

fn decode<V, F>(content: &str, check: F) -> Result<V, AppError>
where
    V: DeserializeOwned,
    F: Fn(&V) -> Result<(), AppError>,
{
    let value: V = serde_json::from_str(content).map_err(|err| {
        AppError::new(
            ErrorCode::InvalidArgument,
            format!("model output is invalid: {err}"),
        )
    })?;
    check(&value)?;
    Ok(value)
}

fn status_of(fault: TransportFault) -> CallStatus {
    match fault {
        TransportFault::Timeout => CallStatus::Timeout,
        TransportFault::Cancelled => CallStatus::Cancelled,
        TransportFault::Unavailable | TransportFault::OutputBudgetExhausted => {
            CallStatus::Unavailable
        }
    }
}

fn error_of(fault: TransportFault) -> AppError {
    match fault {
        TransportFault::Timeout => AppError::new(ErrorCode::Timeout, "model request timed out"),
        TransportFault::Cancelled => {
            AppError::new(ErrorCode::Cancelled, "model request was cancelled")
        }
        TransportFault::Unavailable => {
            AppError::new(ErrorCode::Unavailable, "model request failed")
        }
        TransportFault::OutputBudgetExhausted => AppError::new(
            ErrorCode::Unavailable,
            "模型思考耗尽 token 输出预算，未生成有效正文；比赛字数额度没有扣除",
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use super::{Gateway, MockGateway};
    use crate::error::ErrorCode;
    use crate::infrastructure::ai::schema;
    use crate::infrastructure::ai::types::{
        log_line, AiContext, CallStatus, CompletionRequest, CompletionResponse, ModelConfig,
        ModelTransport, Purpose, TransportFault, DEFAULT_BASE_URL, DEFAULT_MODEL_ID,
        DEFAULT_REASONING_EFFORT, DEFAULT_THINKING,
    };

    enum Step {
        Ready(String),
        Timeout,
        Cancel,
        Late(String),
    }

    struct Scripted {
        steps: Mutex<VecDeque<Step>>,
        repairs: Mutex<Vec<bool>>,
    }

    impl ModelTransport for Scripted {
        fn complete(
            &self,
            request: &CompletionRequest,
            cancel: &AtomicBool,
        ) -> Result<CompletionResponse, TransportFault> {
            self.repairs.lock().expect("repairs").push(request.repair);
            match self.steps.lock().expect("steps").pop_front() {
                Some(Step::Ready(content)) => Ok(response(content)),
                Some(Step::Timeout) => Err(TransportFault::Timeout),
                Some(Step::Cancel) => Err(TransportFault::Cancelled),
                Some(Step::Late(content)) => {
                    cancel.store(true, Ordering::SeqCst);
                    Ok(response(content))
                }
                None => Err(TransportFault::Unavailable),
            }
        }
    }

    struct RecordingWait {
        waits: Mutex<Vec<u64>>,
    }

    impl super::Wait for RecordingWait {
        fn wait_ms(&self, millis: u64) {
            self.waits.lock().expect("waits").push(millis);
        }
    }

    fn response(content: String) -> CompletionResponse {
        CompletionResponse {
            content,
            prompt_tokens: 2,
            completion_tokens: 4,
            elapsed_ms: 5,
        }
    }

    fn config() -> ModelConfig {
        ModelConfig {
            model_id: DEFAULT_MODEL_ID.to_owned(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            thinking: DEFAULT_THINKING.to_owned(),
            reasoning_effort: DEFAULT_REASONING_EFFORT.to_owned(),
            backoff_ms: 10,
            timeout_ms: 1_000,
        }
    }

    fn script(steps: Vec<Step>) -> Gateway<Scripted, RecordingWait> {
        Gateway::new(
            Scripted {
                steps: Mutex::new(VecDeque::from(steps)),
                repairs: Mutex::new(Vec::new()),
            },
            RecordingWait {
                waits: Mutex::new(Vec::new()),
            },
            config(),
        )
    }

    #[test]
    fn mock_runs_every_operation_without_logging_private_text() {
        let gateway = MockGateway::mock(config());
        let cancel = AtomicBool::new(false);
        let context = AiContext {
            own_strategy: "OWN-PRIVATE-STRATEGY".to_owned(),
            opponent_strategy: "OPPONENT-PRIVATE-STRATEGY".to_owned(),
            materials: "sk-phase3-gateway-secret".to_owned(),
            ..AiContext::default()
        };
        gateway.parse_rules(&context, &cancel).expect("parse");
        gateway.analyze_topic(&context, &cancel).expect("topic");
        gateway.build_strategy(&context, &cancel).expect("strategy");
        gateway
            .select_cross_exam(&context, &cancel)
            .expect("cross-exam selection");
        gateway.generate_turn(&context, &cancel).expect("turn");
        gateway
            .decide_interruption(&context, &cancel)
            .expect("interrupt");
        gateway.score_stage(&context, &cancel).expect("score");
        gateway.review_match(&context, &cancel).expect("review");
        gateway.judge_match(&context, &cancel).expect("judge");
        let records = gateway.records();
        assert_eq!(records.len(), 9);
        assert!(records
            .iter()
            .all(|record| record.status == CallStatus::Success));
        assert!(records
            .iter()
            .all(|record| record.model_id == DEFAULT_MODEL_ID));
        let logged = records.iter().map(log_line).collect::<Vec<_>>().join("\n");
        assert!(!logged.contains("OWN-PRIVATE-STRATEGY"));
        assert!(!logged.contains("OPPONENT-PRIVATE-STRATEGY"));
        assert!(!logged.contains("sk-phase3-gateway-secret"));
        assert!(logged.contains("prompt_tokens=3"));
    }

    #[test]
    fn prematch_retries_twice_and_live_speech_retries_once() {
        let gateway = script(vec![
            Step::Timeout,
            Step::Timeout,
            Step::Ready(schema::fixture_json(Purpose::ParseRules)),
        ]);
        let cancel = AtomicBool::new(false);
        gateway
            .parse_rules(&AiContext::default(), &cancel)
            .expect("parse");
        assert_eq!(gateway.transport.repairs.lock().expect("repairs").len(), 3);
        assert_eq!(
            gateway.sleeper.waits.lock().expect("waits").clone(),
            vec![10, 20]
        );

        let gateway = script(vec![
            Step::Timeout,
            Step::Timeout,
            Step::Ready(schema::fixture_json(Purpose::GenerateTurn)),
        ]);
        let error = gateway
            .generate_turn(&AiContext::default(), &cancel)
            .expect_err("timeout");
        assert_eq!(error.code, ErrorCode::Timeout);
        assert_eq!(gateway.records()[0].status, CallStatus::Timeout);
        assert_eq!(gateway.transport.repairs.lock().expect("repairs").len(), 2);
        assert_eq!(
            gateway.sleeper.waits.lock().expect("waits").clone(),
            vec![10]
        );
        assert!(gateway.transport.steps.lock().expect("steps").len() == 1);
    }

    #[test]
    fn cancel_and_late_responses_are_not_parsed() {
        let gateway = script(vec![Step::Cancel]);
        let cancel = AtomicBool::new(false);
        let error = gateway
            .analyze_topic(&AiContext::default(), &cancel)
            .expect_err("cancel");
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert_eq!(gateway.records()[0].status, CallStatus::Cancelled);

        let secret = "LATE-PRIVATE-BODY";
        let gateway = script(vec![Step::Late(secret.to_owned())]);
        let error = gateway
            .build_strategy(&AiContext::default(), &cancel)
            .expect_err("late");
        assert_eq!(error.code, ErrorCode::Cancelled);
        let logged = log_line(&gateway.records()[0]);
        assert!(!logged.contains(secret));
        assert_eq!(gateway.records()[0].status, CallStatus::Cancelled);
    }

    #[test]
    fn invalid_json_is_repaired_once() {
        let gateway = script(vec![
            Step::Ready("{".to_owned()),
            Step::Ready(schema::fixture_json(Purpose::ScoreStage)),
        ]);
        let cancel = AtomicBool::new(false);
        gateway
            .score_stage(&AiContext::default(), &cancel)
            .expect("repaired");
        let repairs = gateway.transport.repairs.lock().expect("repairs").clone();
        assert_eq!(repairs, vec![false, true]);
        assert_eq!(gateway.records()[0].status, CallStatus::Success);

        let gateway = script(vec![
            Step::Ready("{".to_owned()),
            Step::Ready("[]".to_owned()),
        ]);
        let error = gateway
            .review_match(&AiContext::default(), &cancel)
            .expect_err("still invalid");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(gateway.records()[0].status, CallStatus::SchemaInvalid);
        assert_eq!(gateway.transport.repairs.lock().expect("repairs").len(), 2);
    }
}
