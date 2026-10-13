use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};

use super::types::{
    CompletionRequest, CompletionResponse, ModelConfig, ModelTransport, Purpose, TransportFault,
};

pub const CHAT_PATH: &str = "/chat/completions";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedHttp {
    pub url: String,
    pub authorization: String,
    pub body: String,
    pub timeout_ms: u64,
}

/// Reads the HTTP body while calling `on_chunk` for each network read.
/// Implementations must check `cancel` between reads and must not buffer the
/// entire response before the first callback.
pub trait HttpRoundTrip {
    fn stream(
        &self,
        request: &PreparedHttp,
        cancel: &AtomicBool,
        on_chunk: &mut dyn FnMut(&str) -> Result<(), TransportFault>,
    ) -> Result<(), TransportFault>;
}

pub struct DeepSeekTransport<H> {
    http: H,
    config: ModelConfig,
    api_key: String,
}

impl<H: HttpRoundTrip> DeepSeekTransport<H> {
    pub fn new(http: H, config: ModelConfig, api_key: impl Into<String>) -> Self {
        Self {
            http,
            config,
            api_key: api_key.into(),
        }
    }

    pub fn live(config: ModelConfig, api_key: impl Into<String>) -> DeepSeekTransport<UreqHttp> {
        DeepSeekTransport::new(UreqHttp, config, api_key)
    }
}

impl<H: HttpRoundTrip> DeepSeekTransport<H> {
    pub fn complete_with_text_sink<F>(
        &self,
        request: &CompletionRequest,
        cancel: &AtomicBool,
        on_text: F,
    ) -> Result<CompletionResponse, TransportFault>
    where
        F: FnMut(&str),
    {
        self.complete_with_progress_sink(request, cancel, on_text, |_| {})
    }

    pub fn complete_with_progress_sink(
        &self,
        request: &CompletionRequest,
        cancel: &AtomicBool,
        on_text: impl FnMut(&str),
        mut on_phase: impl FnMut(&str),
    ) -> Result<CompletionResponse, TransportFault> {
        if self.api_key.is_empty() {
            return Err(TransportFault::Unavailable);
        }
        if cancel.load(Ordering::SeqCst) {
            return Err(TransportFault::Cancelled);
        }
        let prepared = PreparedHttp {
            url: endpoint(&self.config.base_url),
            authorization: format!("Bearer {}", self.api_key),
            body: completion_body(&self.config, request).to_string(),
            timeout_ms: self.config.timeout_ms,
        };
        on_phase("等待模型响应");
        self.complete_prepared(&prepared, cancel, on_text, on_phase)
    }

    fn complete_prepared<F>(
        &self,
        prepared: &PreparedHttp,
        cancel: &AtomicBool,
        mut on_text: F,
        mut on_phase: impl FnMut(&str),
    ) -> Result<CompletionResponse, TransportFault>
    where
        F: FnMut(&str),
    {
        let mut content = String::new();
        let mut prompt_tokens = 0_u32;
        let mut completion_tokens = 0_u32;
        let mut pending = String::new();
        self.http.stream(prepared, cancel, &mut |chunk| {
            if cancel.load(Ordering::SeqCst) {
                return Err(TransportFault::Cancelled);
            }
            pending.push_str(chunk);
            while let Some(idx) = pending.find('\n') {
                let line = pending[..idx].to_owned();
                pending.drain(..=idx);
                if let Some(data) = line.trim().strip_prefix("data:") {
                    for piece in super::stream::parse_sse_data(data) {
                        match piece {
                            super::stream::SsePiece::Reasoning => on_phase("模型推理中"),
                            super::stream::SsePiece::Text(_) => on_phase("接收候选稿"),
                            _ => {}
                        }
                    }
                }
                consume_sse_line(
                    &line,
                    &mut content,
                    &mut prompt_tokens,
                    &mut completion_tokens,
                    &mut on_text,
                );
            }
            Ok(())
        })?;
        if cancel.load(Ordering::SeqCst) {
            return Err(TransportFault::Cancelled);
        }
        if !pending.trim().is_empty() {
            consume_sse_line(
                &pending,
                &mut content,
                &mut prompt_tokens,
                &mut completion_tokens,
                &mut on_text,
            );
        }
        if content.is_empty() {
            let limit = serde_json::from_str::<Value>(&prepared.body)
                .ok()
                .and_then(|v| v["max_tokens"].as_u64())
                .unwrap_or(u64::MAX);
            if u64::from(completion_tokens) >= limit {
                return Err(TransportFault::OutputBudgetExhausted);
            }
            return Err(TransportFault::Unavailable);
        }
        Ok(CompletionResponse {
            prompt_tokens,
            completion_tokens,
            content,
            elapsed_ms: 0,
        })
    }
}

impl<H: HttpRoundTrip> ModelTransport for DeepSeekTransport<H> {
    fn complete(
        &self,
        request: &CompletionRequest,
        cancel: &AtomicBool,
    ) -> Result<CompletionResponse, TransportFault> {
        self.complete_with_text_sink(request, cancel, |_| {})
    }
}

/// Kept under the existing adapter name; network I/O now supports cancellation
/// while waiting for headers or a silent SSE body, not only between reads.
pub struct UreqHttp;

impl HttpRoundTrip for UreqHttp {
    fn stream(
        &self,
        request: &PreparedHttp,
        cancel: &AtomicBool,
        on_chunk: &mut dyn FnMut(&str) -> Result<(), TransportFault>,
    ) -> Result<(), TransportFault> {
        if cancel.load(Ordering::SeqCst) {
            return Err(TransportFault::Cancelled);
        }
        let ticket = super::budget::BudgetTicket::from_env(&request.url, &request.body)?;
        let mut usage_body = String::new();
        let mut observe = |chunk: &str| {
            if ticket.is_some() {
                usage_body.push_str(chunk);
            }
            on_chunk(chunk)
        };
        static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, std::io::Error>> = OnceLock::new();
        let runtime = RUNTIME
            .get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
            })
            .as_ref()
            .map_err(|_| TransportFault::Unavailable)?;
        let result=runtime.block_on(async {
            let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter()
                .map(|der| reqwest::Certificate::from_der(der.as_ref()))
                .collect::<Result<Vec<_>, _>>().map_err(|_| TransportFault::Unavailable)?;
            let client = reqwest::Client::builder()
                .tls_certs_only(roots)
                .connect_timeout(Duration::from_secs(20))
                .timeout(Duration::from_millis(request.timeout_ms.max(1)))
                .redirect(reqwest::redirect::Policy::none())
                .build().map_err(|_| TransportFault::Unavailable)?;
            let network = async {
                let mut response = client.post(&request.url)
                    .header("Authorization", &request.authorization)
                    .header("Content-Type", "application/json")
                    .body(request.body.clone()).send().await.map_err(map_reqwest)?;
                if !response.status().is_success() {
                    return Err(match map_http_status(response.status().as_u16(), "") {
                        HttpFault::Timeout => TransportFault::Timeout,
                        _ => TransportFault::Unavailable,
                    });
                }
                let mut utf8_pending = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(map_reqwest)? {
                    if cancel.load(Ordering::SeqCst) { return Err(TransportFault::Cancelled); }
                    utf8_pending.extend_from_slice(&chunk);
                    emit_complete_utf8(&mut utf8_pending, false, &mut observe)?;
                }
                emit_complete_utf8(&mut utf8_pending, true, &mut observe)?;
                Ok(())
            };
            tokio::select! {
                biased;
                _ = async {
                    while !cancel.load(Ordering::SeqCst) { tokio::time::sleep(Duration::from_millis(20)).await; }
                } => Err(TransportFault::Cancelled),
                _ = tokio::time::sleep(Duration::from_millis(request.timeout_ms.max(1))) => Err(TransportFault::Timeout),
                result = network => result,
            }
        });
        if let Some(ticket) = ticket {
            ticket.finish(super::budget::usage_from_body(&usage_body), result.is_ok())?;
        }
        result
    }
}

fn emit_complete_utf8(
    pending: &mut Vec<u8>,
    end_of_stream: bool,
    on_chunk: &mut dyn FnMut(&str) -> Result<(), TransportFault>,
) -> Result<(), TransportFault> {
    if pending.is_empty() {
        return Ok(());
    }
    match std::str::from_utf8(pending) {
        Ok(text) => {
            on_chunk(text)?;
            pending.clear();
            Ok(())
        }
        Err(error) => {
            if error.error_len().is_some() {
                return Err(TransportFault::Unavailable);
            }
            let valid = error.valid_up_to();
            if valid > 0 {
                let prefix = std::str::from_utf8(&pending[..valid])
                    .map_err(|_| TransportFault::Unavailable)?
                    .to_owned();
                on_chunk(&prefix)?;
                pending.drain(..valid);
            }
            if end_of_stream && !pending.is_empty() {
                return Err(TransportFault::Unavailable);
            }
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamText {
    pub content: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

pub fn endpoint(base_url: &str) -> String {
    format!("{}{CHAT_PATH}", base_url.trim_end_matches('/'))
}

pub fn completion_body(config: &ModelConfig, request: &CompletionRequest) -> Value {
    let thinking = if config.thinking == "disabled" {
        "disabled"
    } else {
        "enabled"
    };
    let effort = match config.reasoning_effort.as_str() {
        "low" => "low",
        "max" => "max",
        _ => "high",
    };
    json!({
        "model": config.model_id,
        "messages": request.messages.iter().map(|message| json!({
            "role": message.role,
            "content": message.content,
        })).collect::<Vec<_>>(),
        "thinking": { "type": thinking },
        "reasoning_effort": effort,
        // In thinking mode reasoning tokens share the output budget. Complex
        // ruleset extraction could exhaust 8K before any final JSON appeared,
        // while short live turns should stay bounded for latency and cost.
        "max_tokens": match request.purpose {
            Purpose::ParseRules => 32768,
            Purpose::AnalyzeTopic
            | Purpose::BuildStrategy
            | Purpose::ScoreStage
            | Purpose::ReviewMatch
            | Purpose::JudgeMatch => 16384,
            Purpose::GenerateTurn => 16384,
            Purpose::SelectCrossExam
            | Purpose::DecideInterruption => 8192,
        },
        "stream": true,
        "stream_options": {"include_usage": true},
        "response_format": { "type": "json_object" }
    })
}

pub fn fold_stream_chunks<F>(
    chunks: &[String],
    mut on_text: F,
) -> Result<StreamText, TransportFault>
where
    F: FnMut(&str),
{
    let mut content = String::new();
    let mut prompt_tokens = 0;
    let mut completion_tokens = 0;
    let mut pending = String::new();
    for chunk in chunks {
        pending.push_str(chunk);
        while let Some(idx) = pending.find('\n') {
            let line = pending[..idx].to_owned();
            pending.drain(..=idx);
            consume_sse_line(
                &line,
                &mut content,
                &mut prompt_tokens,
                &mut completion_tokens,
                &mut on_text,
            );
        }
    }
    if !pending.trim().is_empty() {
        consume_sse_line(
            &pending,
            &mut content,
            &mut prompt_tokens,
            &mut completion_tokens,
            &mut on_text,
        );
    }
    if content.is_empty() {
        Err(TransportFault::Unavailable)
    } else {
        Ok(StreamText {
            content,
            prompt_tokens,
            completion_tokens,
        })
    }
}

fn consume_sse_line<F>(
    line: &str,
    content: &mut String,
    prompt_tokens: &mut u32,
    completion_tokens: &mut u32,
    on_text: &mut F,
) where
    F: FnMut(&str),
{
    let Some(data) = line.trim().strip_prefix("data:") else {
        return;
    };
    for piece in super::stream::parse_sse_data(data) {
        match piece {
            super::stream::SsePiece::Text(text) => {
                on_text(&text);
                content.push_str(&text);
            }
            super::stream::SsePiece::Usage {
                prompt_tokens: prompt,
                completion_tokens: completion,
            } => {
                *prompt_tokens = prompt;
                *completion_tokens = completion;
            }
            super::stream::SsePiece::Reasoning | super::stream::SsePiece::Done => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpFault {
    Authentication,
    Network,
    Timeout,
    ModelUnavailable,
    Incompatible,
}

pub fn map_http_status(status: u16, body: &str) -> HttpFault {
    let lower = body.to_ascii_lowercase();
    if status == 401 || status == 403 {
        return HttpFault::Authentication;
    }
    if status == 404 || (lower.contains("model") && lower.contains("not found")) {
        return HttpFault::ModelUnavailable;
    }
    if status == 408 || status == 504 {
        return HttpFault::Timeout;
    }
    if status >= 500 {
        return HttpFault::Network;
    }
    HttpFault::Incompatible
}

fn map_reqwest(error: reqwest::Error) -> TransportFault {
    if error.is_timeout() {
        TransportFault::Timeout
    } else {
        TransportFault::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{
        completion_body, emit_complete_utf8, endpoint, fold_stream_chunks, DeepSeekTransport,
        PreparedHttp,
    };
    use crate::infrastructure::ai::gateway::Gateway;
    use crate::infrastructure::ai::gateway::NoWait;
    use crate::infrastructure::ai::schema;
    use crate::infrastructure::ai::types::{
        log_line, AiContext, CompletionRequest, ModelConfig, PromptMessage, Purpose,
        TransportFault, DEFAULT_BASE_URL, DEFAULT_MODEL_ID, DEFAULT_REASONING_EFFORT,
        DEFAULT_THINKING,
    };

    struct FakeHttp {
        prepared: Arc<Mutex<Option<PreparedHttp>>>,
        chunks: Vec<String>,
        cancel_after: Option<usize>,
        reads: Arc<AtomicUsize>,
    }

    impl super::HttpRoundTrip for FakeHttp {
        fn stream(
            &self,
            request: &PreparedHttp,
            cancel: &AtomicBool,
            on_chunk: &mut dyn FnMut(&str) -> Result<(), TransportFault>,
        ) -> Result<(), TransportFault> {
            *self.prepared.lock().expect("prepared") = Some(request.clone());
            for (index, chunk) in self.chunks.iter().enumerate() {
                if cancel.load(Ordering::SeqCst) {
                    return Err(TransportFault::Cancelled);
                }
                self.reads.fetch_add(1, Ordering::SeqCst);
                on_chunk(chunk)?;
                if self.cancel_after == Some(index + 1) {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
            if cancel.load(Ordering::SeqCst) {
                return Err(TransportFault::Cancelled);
            }
            Ok(())
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

    #[test]
    fn reasoning_only_at_the_output_limit_is_not_misreported_as_network_failure() {
        let http=FakeHttp {prepared:Arc::new(Mutex::new(None)),chunks:vec!["data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"private reasoning\"}}]}\ndata: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":16384}}\ndata: [DONE]\n".to_owned()],cancel_after:None,reads:Arc::new(AtomicUsize::new(0))};
        let transport = DeepSeekTransport::new(http, config(), "fixture-key");
        let request = CompletionRequest {
            purpose: Purpose::GenerateTurn,
            messages: vec![],
            repair: false,
        };
        let mut phases = Vec::new();
        let mut text = String::new();
        let result = transport.complete_with_progress_sink(
            &request,
            &AtomicBool::new(false),
            |delta| text.push_str(delta),
            |phase| phases.push(phase.to_owned()),
        );
        assert_eq!(result, Err(TransportFault::OutputBudgetExhausted));
        assert!(text.is_empty());
        assert!(phases.contains(&"模型推理中".to_owned()));
        assert!(phases.iter().all(|p| !p.contains("private reasoning")));
    }

    #[test]
    fn cancelling_a_silent_http_connection_stops_before_its_deadline() {
        use super::HttpRoundTrip;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::{Duration, Instant};
        for headers in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = [0_u8; 4096];
                let _ = socket.read(&mut request);
                if headers {
                    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
                }
                ready_tx.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(1_000));
            });
            let cancel = Arc::new(AtomicBool::new(false));
            let signal = Arc::clone(&cancel);
            let canceller = std::thread::spawn(move || {
                ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                std::thread::sleep(Duration::from_millis(50));
                signal.store(true, Ordering::SeqCst);
            });
            let request = PreparedHttp {
                url: format!("http://{address}/"),
                authorization: "Bearer local-fixture".into(),
                body: "{}".into(),
                timeout_ms: 4_000,
            };
            let started = Instant::now();
            let result = super::UreqHttp.stream(&request, &cancel, &mut |_| Ok(()));
            let elapsed = started.elapsed();
            canceller.join().unwrap();
            server.join().unwrap();
            assert_eq!(result, Err(TransportFault::Cancelled), "headers={headers}");
            assert!(
                elapsed < Duration::from_millis(600),
                "headers={headers}, elapsed={elapsed:?}"
            );
        }
    }

    #[test]
    fn request_uses_the_verified_model_and_high_thinking() {
        let request = CompletionRequest {
            purpose: Purpose::ParseRules,
            messages: vec![PromptMessage {
                role: "system".to_owned(),
                content: "constraint".to_owned(),
            }],
            repair: false,
        };
        let body = completion_body(&config(), &request);
        assert_eq!(body["model"], DEFAULT_MODEL_ID);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "high");
        assert_eq!(body["max_tokens"], 32768);
        assert_eq!(body["stream"], true);
        assert_eq!(body["response_format"]["type"], "json_object");
        assert_eq!(
            endpoint(DEFAULT_BASE_URL),
            "https://api.deepseek.com/chat/completions"
        );
        let settings = crate::infrastructure::db::AppSettings::default();
        let resolved = ModelConfig::from_settings(&settings);
        assert_eq!(resolved.model_id, DEFAULT_MODEL_ID);
        assert_eq!(resolved.base_url, DEFAULT_BASE_URL);
        assert_eq!(resolved.thinking, "enabled");
        assert_eq!(resolved.reasoning_effort, "high");
    }

    #[test]
    fn high_thinking_request_contract_matches_official_shape() {
        let request = CompletionRequest {
            purpose: Purpose::GenerateTurn,
            messages: vec![PromptMessage {
                role: "user".to_owned(),
                content: "发言".to_owned(),
            }],
            repair: false,
        };
        let mut config = config();
        config.reasoning_effort = "max".to_owned();
        let body = completion_body(&config, &request);
        assert_eq!(body["model"], "deepseek-flash");
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "max");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn adapter_parses_sse_and_keeps_the_key_out_of_the_log() {
        let key = "sk-phase3-adapter-secret";
        let payload = schema::fixture_json(Purpose::ParseRules);
        let chunk = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}}}}],\"usage\":{{\"prompt_tokens\":8,\"completion_tokens\":9}}}}\n\ndata: [DONE]\n",
            serde_json::to_string(&payload).expect("encode")
        );
        let prepared = Arc::new(Mutex::new(None));
        let http = FakeHttp {
            prepared: Arc::clone(&prepared),
            chunks: vec![chunk],
            cancel_after: None,
            reads: Arc::new(AtomicUsize::new(0)),
        };
        let gateway = Gateway::new(
            DeepSeekTransport::new(http, config(), key),
            NoWait,
            config(),
        );
        let cancel = AtomicBool::new(false);
        gateway
            .parse_rules(&AiContext::default(), &cancel)
            .expect("parse");
        let prepared = prepared.lock().expect("prepared").clone().expect("request");
        assert_eq!(prepared.url, "https://api.deepseek.com/chat/completions");
        assert_eq!(prepared.authorization, format!("Bearer {key}"));
        let logged = log_line(&gateway.records()[0]);
        assert!(!logged.contains(key));
        assert!(logged.contains("prompt_tokens=8"));
        assert_eq!(gateway.records()[0].completion_tokens, 9);
    }

    #[test]
    fn streaming_cancel_stops_before_the_next_chunk() {
        let reads = Arc::new(AtomicUsize::new(0));
        let http = FakeHttp {
            prepared: Arc::new(Mutex::new(None)),
            chunks: vec![
                "data: {\"choices\":[{\"delta\":{\"content\":\"{\"}}]}\n".to_owned(),
                "LATE-CHUNK".to_owned(),
            ],
            cancel_after: Some(1),
            reads: Arc::clone(&reads),
        };
        let gateway = Gateway::new(
            DeepSeekTransport::new(http, config(), "local-key"),
            NoWait,
            config(),
        );
        let error = gateway
            .parse_rules(&AiContext::default(), &AtomicBool::new(false))
            .expect_err("cancel");
        assert_eq!(error.code, crate::error::ErrorCode::Cancelled);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(!log_line(&gateway.records()[0]).contains("LATE-CHUNK"));
    }

    #[test]
    fn text_sink_receives_deltas_before_stream_ends() {
        let midstream_seen = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&midstream_seen);
        let http = FakeHttp {
            prepared: Arc::new(Mutex::new(None)),
            chunks: vec![
                "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n".to_owned(),
                "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n".to_owned(),
            ],
            cancel_after: None,
            reads: Arc::new(AtomicUsize::new(0)),
        };
        let transport = DeepSeekTransport::new(http, config(), "local-key");
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();
        let response = transport
            .complete_with_text_sink(
                &CompletionRequest {
                    purpose: Purpose::GenerateTurn,
                    messages: vec![],
                    repair: false,
                },
                &cancel,
                |delta| {
                    seen.push(delta.to_owned());
                    if seen.len() == 1 {
                        flag.store(true, Ordering::SeqCst);
                    }
                },
            )
            .expect("stream");
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(seen, vec!["hello".to_owned(), " world".to_owned()]);
        assert_eq!(response.content, "hello world");
    }

    #[test]
    fn sse_content_is_joined_from_deltas() {
        let text = fold_stream_chunks(
            &[
                "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"ok\\\":\"}}]}\n".to_owned(),
                "data: {\"choices\":[{\"delta\":{\"content\":\"true}\"}}]}\n".to_owned(),
            ],
            |_| {},
        )
        .expect("sse");
        assert_eq!(text.content, "{\"ok\":true}");
    }

    #[test]
    fn network_reads_preserve_chinese_split_inside_a_utf8_character() {
        let bytes = "进行全面限制".as_bytes();
        let split = bytes
            .windows(3)
            .position(|window| window == "全".as_bytes())
            .expect("全")
            + 1;
        let mut pending = Vec::new();
        let mut output = String::new();
        pending.extend_from_slice(&bytes[..split]);
        emit_complete_utf8(&mut pending, false, &mut |chunk| {
            output.push_str(chunk);
            Ok(())
        })
        .unwrap();
        assert!(!output.contains('�'));
        pending.extend_from_slice(&bytes[split..]);
        emit_complete_utf8(&mut pending, true, &mut |chunk| {
            output.push_str(chunk);
            Ok(())
        })
        .unwrap();
        assert_eq!(output, "进行全面限制");
        assert!(pending.is_empty());
    }

    #[test]
    fn sse_lines_survive_chunk_boundaries() {
        let text = fold_stream_chunks(
            &[
                "data: {\"choices\":[{\"delta\":{\"content\":\"ab\"}}]}\n data: {\"choices\":[{\"delta\":{\"cont"
                    .to_owned(),
                "ent\":\"cd\"}}]}\n".to_owned(),
            ],
            |_| {},
        )
        .expect("sse");
        assert_eq!(text.content, "abcd");
    }
}
