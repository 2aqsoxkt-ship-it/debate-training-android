use std::sync::atomic::{AtomicBool, Ordering};

use crate::domain::ruleset::SlotId;
use crate::domain::text_units::count_effective_units;
use crate::infrastructure::ai::schema::{self, GeneratedTurnV1};
use crate::infrastructure::ai::{
    build_prompt, repair_messages, AiContext, CompletionRequest, DeepSeekTransport, MockGateway,
    ModelConfig, ModelTransport, Purpose, SpeechTextExtractor, UreqHttp,
};
use crate::infrastructure::credentials::{KeyringSecretStore, SecretStore};

#[cfg(test)]
pub fn generate_turn_text(
    live: bool,
    config: ModelConfig,
    context: &AiContext,
    expected_slot: SlotId,
    word_cap: u32,
    cancel: &AtomicBool,
    on_delta: impl FnMut(&str),
) -> Result<String, String> {
    generate_turn_text_with_progress(
        live,
        config,
        context,
        expected_slot,
        word_cap,
        cancel,
        on_delta,
        |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub fn generate_turn_text_with_progress(
    live: bool,
    config: ModelConfig,
    context: &AiContext,
    expected_slot: SlotId,
    word_cap: u32,
    cancel: &AtomicBool,
    mut on_delta: impl FnMut(&str),
    mut on_phase: impl FnMut(&str),
) -> Result<String, String> {
    on_phase("准备发言上下文");
    let context = constrained_context(context, expected_slot, word_cap);
    let turn = if live {
        let key = KeyringSecretStore::app()
            .read_api_key()
            .map_err(|_| "还没有可用的 DeepSeek 密钥".to_owned())?;
        let transport = DeepSeekTransport::new(UreqHttp, config, key);
        let request = CompletionRequest {
            purpose: Purpose::GenerateTurn,
            messages: build_prompt(Purpose::GenerateTurn, &context),
            repair: false,
        };
        let mut extractor = SpeechTextExtractor::default();
        let response = transport
            .complete_with_progress_sink(
                &request,
                cancel,
                |delta| {
                    if let Some(speech) = extractor.push(delta) {
                        on_delta(&speech);
                    }
                },
                &mut on_phase,
            )
            .map_err(|err| {
                if err == crate::infrastructure::ai::TransportFault::OutputBudgetExhausted {
                    "模型思考耗尽输出预算，没有正文；本轮未扣字，请重试或人工接管".to_owned()
                } else {
                    format!("{err:?}")
                }
            })?;
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".to_owned());
        }
        on_phase(
            match serde_json::from_str::<GeneratedTurnV1>(&response.content) {
                Ok(turn) if count_effective_units(&turn.text) > word_cap => "压缩重写并重新校验",
                Ok(_) => "校验结构、辩位及篇幅",
                Err(_) => "修复发言结构并重新校验",
            },
        );
        decode_or_repair_turn(
            &transport,
            &response.content,
            &context,
            expected_slot,
            word_cap,
            cancel,
        )?
    } else {
        let gateway = MockGateway::mock(config);
        let turn = gateway
            .generate_turn(&context, cancel)
            .map_err(|err| err.to_string())?;
        let mid = turn.text.chars().count() / 2;
        let first: String = turn.text.chars().take(mid.max(1)).collect();
        let second: String = turn.text.chars().skip(mid.max(1)).collect();
        if !first.is_empty() {
            on_delta(&first);
        }
        if !second.is_empty() {
            on_delta(&second);
        }
        turn
    };
    if turn.slot != expected_slot {
        return Err(format!(
            "模型辩位 {:?} 与当前环节 {:?} 不一致",
            turn.slot, expected_slot
        ));
    }
    ensure_budget(&turn, word_cap)?;
    Ok(turn.text)
}

fn decode_or_repair_turn<T: ModelTransport>(
    transport: &T,
    content: &str,
    context: &AiContext,
    expected_slot: SlotId,
    word_cap: u32,
    cancel: &AtomicBool,
) -> Result<GeneratedTurnV1, String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".to_owned());
    }
    if let Ok(turn) = decode_turn(content, expected_slot) {
        return fit_budget(transport, turn, context, expected_slot, word_cap, cancel);
    }
    let mut messages = build_prompt(Purpose::GenerateTurn, context);
    let mut repair = repair_messages(Purpose::GenerateTurn, content);
    if let Some(message) = repair.last_mut() {
        message.content.push_str(&format!(
            "\nThe only legal slot is {}. Preserve the original role, side, strategy and word_cap constraints above.",
            serde_json::to_string(&expected_slot).expect("slot label")
        ));
    }
    messages.extend(repair.into_iter().skip(1));
    let request = CompletionRequest {
        purpose: Purpose::GenerateTurn,
        messages,
        repair: true,
    };
    let repaired = transport
        .complete(&request, cancel)
        .map_err(|error| format!("模型发言 JSON 修复请求失败：{error:?}"))?;
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".to_owned());
    }
    let turn = decode_turn(&repaired.content, expected_slot)
        .map_err(|error| format!("模型发言 JSON 修复失败：{error}"))?;
    fit_budget(transport, turn, context, expected_slot, word_cap, cancel)
}

fn ensure_budget(turn: &GeneratedTurnV1, cap: u32) -> Result<(), String> {
    let units = count_effective_units(&turn.text);
    if units == 0 || cap == 0 {
        return Err("当前额度不足或模型发言有效字数为 0".to_owned());
    }
    if units > cap {
        return Err(format!(
            "模型未能在 {cap} 字预算内完成（实测 {units} 字），未提交、未扣字；请重试或人工接管"
        ));
    }
    Ok(())
}

fn fit_budget<T: ModelTransport>(
    transport: &T,
    turn: GeneratedTurnV1,
    context: &AiContext,
    expected_slot: SlotId,
    cap: u32,
    cancel: &AtomicBool,
) -> Result<GeneratedTurnV1, String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".to_owned());
    }
    if ensure_budget(&turn, cap).is_ok() {
        return Ok(turn);
    }
    if cap == 0 || count_effective_units(&turn.text) == 0 {
        ensure_budget(&turn, cap)?;
    }
    let mut messages = build_prompt(Purpose::GenerateTurn, context);
    let encoded = serde_json::to_string(&turn).map_err(|_| "模型稿编码失败".to_owned())?;
    let mut correction = repair_messages(Purpose::GenerateTurn, &encoded);
    correction.last_mut().expect("repair message").content.push_str(&format!(
        "\n这是篇幅压缩，不是截断。原稿实测 {} 有效字，硬上限 {cap} 字。只返回辩位为 {} 的完整新稿，目标不超过 {} 字，保留核心主张、必要理由及完整结尾；删除铺垫、重复、次要论点。不得截半句。按中文字符逐字计数，连续英文词/数字各计一单位，标点、空白、emoji不计。",
        count_effective_units(&turn.text), serde_json::to_string(&expected_slot).expect("slot"), cap.saturating_mul(90) / 100
    ));
    messages.extend(correction.into_iter().skip(1));
    let request = CompletionRequest {
        purpose: Purpose::GenerateTurn,
        messages,
        repair: true,
    };
    let response = transport
        .complete(&request, cancel)
        .map_err(|error| format!("篇幅压缩失败，未提交：{error:?}"))?;
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".to_owned());
    }
    let shortened = decode_turn(&response.content, expected_slot)?;
    ensure_budget(&shortened, cap)?;
    Ok(shortened)
}

fn decode_turn(content: &str, expected_slot: SlotId) -> Result<GeneratedTurnV1, String> {
    let parsed: GeneratedTurnV1 =
        serde_json::from_str(content).map_err(|error| format!("模型发言 JSON 无效：{error}"))?;
    schema::ensure_turn(&parsed).map_err(|error| error.to_string())?;
    if parsed.slot != expected_slot {
        return Err(format!(
            "模型辩位 {:?} 与当前环节 {:?} 不一致",
            parsed.slot, expected_slot
        ));
    }
    Ok(parsed)
}

fn constrained_context(context: &AiContext, expected_slot: SlotId, word_cap: u32) -> AiContext {
    let mut constrained = context.clone();
    constrained.own_strategy.push_str(&format!(
        "\nrequired_slot={expected_slot:?}\nword_cap={word_cap}\ntarget_units_low={}\ntarget_units_high={}",
        word_cap.saturating_mul(85) / 100,
        word_cap.saturating_mul(90) / 100
    ));
    constrained.own_strategy.push_str("\n篇幅规划：在预算内先回应核心交锋，再给关键理由，最后完整收束。按阶段职责决定篇幅，不用长篇铺垫凑字；问句必须完整，回答先直接回答，再作必要解释。中文按字符、连续英文词/数字各计一单位；标点、空白和emoji不计。预留约10%余量，不得超过硬上限，结尾不要留给系统截断。");
    constrained
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamTurn {
    pub slot: SlotId,
    pub text: String,
    #[serde(default)]
    pub reply_to: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn generate_team_turn(
    config: ModelConfig,
    context: &AiContext,
    candidates: &[SlotId],
    word_cap: u32,
    cancel: &AtomicBool,
    mut on_delta: impl FnMut(&str),
    mut on_phase: impl FnMut(&str),
) -> Result<TeamTurn, String> {
    if candidates.is_empty() {
        return Err("没有合法 AI 辩位".to_owned());
    }
    let key = KeyringSecretStore::app()
        .read_api_key()
        .map_err(|_| "没有可用 DeepSeek 密钥")?;
    let transport = DeepSeekTransport::new(UreqHttp, config, key);
    let mut team_context = context.clone();
    team_context.own_strategy = team_context
        .own_strategy
        .lines()
        .filter(|line| !line.starts_with("required_slot="))
        .collect::<Vec<_>>()
        .join("\n");
    let mut messages = build_prompt(Purpose::GenerateTurn, &team_context);
    messages[0].content.push_str("\n本次是自由辩团队联合决策：slot 不固定，必须从末尾 legal_candidates 中选本方合法 AI 辩位。根据公开交锋缺口和各辩位职责选择，不机械按次数轮换。在同一次 JSON 输出选择的 slot、完整 text、replyTo 真实公开发言 ID 数组。只声明回应关系，不宣称已反驳成功。不得选择人工席位或对方席位。这个团队选择约束覆盖上述固定 required_slot 约束。");
    messages.push(crate::infrastructure::ai::PromptMessage{role:"user".to_owned(),content:format!("legal_candidates={}\nword_cap={word_cap}\n目标不超过 {} 有效字，先回应核心交锋，再完整收束。replyTo 必须从公开实录 ID 中选择。",serde_json::to_string(candidates).unwrap(),word_cap*90/100)});
    on_phase("团队联合选择辩位并生成回应");
    let request = CompletionRequest {
        purpose: Purpose::GenerateTurn,
        messages,
        repair: false,
    };
    let mut extractor = SpeechTextExtractor::default();
    let response = transport
        .complete_with_progress_sink(
            &request,
            cancel,
            |delta| {
                if let Some(speech) = extractor.push(delta) {
                    on_delta(&speech);
                }
            },
            &mut on_phase,
        )
        .map_err(|e| format!("{e:?}"))?;
    on_phase("校验团队派位、回应引用及篇幅");
    let decoded = serde_json::from_str::<TeamTurn>(&response.content)
        .ok()
        .filter(|turn| validate_team_turn(turn, candidates, context).is_ok());
    let mut turn = if let Some(turn) = decoded {
        turn
    } else {
        on_phase("修复团队派位或回应引用（最多一次）");
        let mut repair = request.messages.clone();
        repair.push(crate::infrastructure::ai::PromptMessage {
            role: "assistant".to_owned(),
            content: response.content,
        });
        repair.push(crate::infrastructure::ai::PromptMessage{role:"user".to_owned(),content:format!("修复为完整 JSON {{slot,text,replyTo}}。slot 只能来自 {}，replyTo 只能引用上面公开实录中真实存在的 ID，不能捏造 ID 或选择人工席位。保留队伍、职责和 {word_cap} 字上限。",serde_json::to_string(candidates).unwrap())});
        let fixed = transport
            .complete(
                &CompletionRequest {
                    purpose: Purpose::GenerateTurn,
                    messages: repair,
                    repair: true,
                },
                cancel,
            )
            .map_err(|e| format!("团队约束修复失败：{e:?}"))?;
        serde_json::from_str::<TeamTurn>(&fixed.content).map_err(|_| "团队约束修复后结构仍无效")?
    };
    validate_team_turn(&turn, candidates, context)?;
    if count_effective_units(&turn.text) > word_cap {
        on_phase("压缩团队发言，锁定已选辩位");
    }
    let generated = crate::infrastructure::ai::schema::GeneratedTurnV1 {
        schema_version: "generated-turn.v1".to_owned(),
        slot: turn.slot,
        text: turn.text,
    };
    let fit = fit_budget(
        &transport,
        generated,
        &constrained_context(context, turn.slot, word_cap),
        turn.slot,
        word_cap,
        cancel,
    )?;
    turn.text = fit.text;
    Ok(turn)
}

fn validate_team_turn(
    turn: &TeamTurn,
    candidates: &[SlotId],
    context: &AiContext,
) -> Result<(), String> {
    if !candidates.contains(&turn.slot) || turn.text.trim().is_empty() {
        return Err("团队派位不在合法 AI 候选中，未提交".to_owned());
    }
    for target in &turn.reply_to {
        if !context
            .public_transcript
            .iter()
            .any(|line| line.starts_with(&format!("{target}:")))
        {
            return Err("团队回应引用了不存在的公开发言，未提交".to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    use super::{constrained_context, decode_or_repair_turn};
    use crate::domain::ruleset::SlotId;
    use crate::infrastructure::ai::{
        joined, AiContext, CompletionRequest, CompletionResponse, ModelTransport, Purpose,
        TransportFault,
    };

    struct RepairingTransport {
        requests: Mutex<Vec<CompletionRequest>>,
        repaired_slot: SlotId,
    }

    #[test]
    fn team_decision_rejects_illegal_seats_and_invented_reply_ids() {
        let context = AiContext {
            public_transcript: vec!["real-id: 已公开发言".to_owned()],
            ..Default::default()
        };
        let mut turn = super::TeamTurn {
            slot: SlotId::Aff2,
            text: "完整回应".to_owned(),
            reply_to: vec!["real-id".to_owned()],
        };
        assert!(super::validate_team_turn(&turn, &[SlotId::Aff2], &context).is_ok());
        turn.slot = SlotId::Neg2;
        assert!(super::validate_team_turn(&turn, &[SlotId::Aff2], &context).is_err());
        turn.slot = SlotId::Aff2;
        turn.reply_to = vec!["future-or-invented".to_owned()];
        assert!(super::validate_team_turn(&turn, &[SlotId::Aff2], &context).is_err());
    }

    impl ModelTransport for RepairingTransport {
        fn complete(
            &self,
            request: &CompletionRequest,
            _cancel: &AtomicBool,
        ) -> Result<CompletionResponse, TransportFault> {
            self.requests.lock().unwrap().push(request.clone());
            Ok(CompletionResponse {
                content: serde_json::json!({"schemaVersion":"generated-turn.v1", "slot":self.repaired_slot, "text":"修复后的发言"}).to_string(),
                prompt_tokens: 1,
                completion_tokens: 1,
                elapsed_ms: 1,
            })
        }
    }

    #[test]
    fn zero_budget_or_cancelled_candidate_never_requests_compression() {
        let transport = RepairingTransport {
            requests: Mutex::new(Vec::new()),
            repaired_slot: SlotId::Aff3,
        };
        let content =
            r#"{"schemaVersion":"generated-turn.v1","slot":"AFF_3","text":"尚未公开的完整候选。"}"#;
        for (cap, cancelled) in [(0, false), (5, true)] {
            assert!(decode_or_repair_turn(
                &transport,
                content,
                &AiContext::default(),
                SlotId::Aff3,
                cap,
                &AtomicBool::new(cancelled)
            )
            .is_err());
        }
        assert!(transport.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn compression_cannot_change_the_speaker_to_an_opposing_seat() {
        let transport = RepairingTransport {
            requests: Mutex::new(Vec::new()),
            repaired_slot: SlotId::Neg3,
        };
        let content = r#"{"schemaVersion":"generated-turn.v1","slot":"AFF_3","text":"必须由原来的正方三辩完整作答。"}"#;
        assert!(decode_or_repair_turn(
            &transport,
            content,
            &AiContext::default(),
            SlotId::Aff3,
            8,
            &AtomicBool::new(false)
        )
        .is_err());
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn turn_generation_always_injects_the_expected_slot_and_length() {
        let context = constrained_context(&AiContext::default(), SlotId::Aff3, 120);
        let prompt = joined(&crate::infrastructure::ai::build_prompt(
            Purpose::GenerateTurn,
            &context,
        ));
        assert!(prompt.contains("required_slot=Aff3"));
        assert!(prompt.contains("word_cap=120"));
        assert!(prompt.contains("target_units_low=102"));
        assert!(prompt.contains("target_units_high=108"));
    }

    #[test]
    fn malformed_streamed_turn_gets_one_schema_repair_before_failure() {
        let transport = RepairingTransport {
            requests: Mutex::new(Vec::new()),
            repaired_slot: SlotId::Aff3,
        };
        let malformed = r#"{"schemaVersion":"generated-turn.v1","slot":"AFF_3" "text":"缺少逗号"}"#;
        let turn = decode_or_repair_turn(
            &transport,
            malformed,
            &constrained_context(&AiContext::default(), SlotId::Aff3, 120),
            SlotId::Aff3,
            120,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(turn.slot, SlotId::Aff3);
        assert_eq!(turn.text, "修复后的发言");
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].repair);
        assert_eq!(requests[0].purpose, Purpose::GenerateTurn);
        assert!(joined(&requests[0].messages).contains(malformed));
    }

    #[test]
    fn streamed_turn_repair_retains_the_negative_role_and_length_constraint() {
        let transport = RepairingTransport {
            requests: Mutex::new(Vec::new()),
            repaired_slot: SlotId::Neg2,
        };
        let wrong_slot = r#"{"slot":"AFF_1","text":"错误辩位"}"#;
        let context = constrained_context(&AiContext::default(), SlotId::Neg2, 140);
        let turn = decode_or_repair_turn(
            &transport,
            wrong_slot,
            &context,
            SlotId::Neg2,
            140,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(turn.slot, SlotId::Neg2);
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let prompt = joined(&requests[0].messages);
        assert!(prompt.contains("The only legal slot is \"NEG_2\""));
        assert!(prompt.contains("word_cap=140"));
    }

    #[test]
    fn over_budget_candidate_is_rewritten_instead_of_clipped() {
        let transport = RepairingTransport {
            requests: Mutex::new(Vec::new()),
            repaired_slot: SlotId::Aff3,
        };
        let content = serde_json::json!({
            "schemaVersion": "generated-turn.v1", "slot": "AFF_3",
            "text": "这是完整的原始论证但是明显超过了允许字数需要压缩而不是剪掉结尾。"
        })
        .to_string();
        let turn = decode_or_repair_turn(
            &transport,
            &content,
            &constrained_context(&AiContext::default(), SlotId::Aff3, 12),
            SlotId::Aff3,
            12,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(turn.text, "修复后的发言");
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn still_over_budget_after_one_rewrite_fails_without_clipping() {
        let transport = RepairingTransport {
            requests: Mutex::new(Vec::new()),
            repaired_slot: SlotId::Aff3,
        };
        let content = serde_json::json!({"schemaVersion":"generated-turn.v1", "slot":"AFF_3", "text":"超限正文不能截"}).to_string();
        let result = decode_or_repair_turn(
            &transport,
            &content,
            &constrained_context(&AiContext::default(), SlotId::Aff3, 1),
            SlotId::Aff3,
            1,
            &AtomicBool::new(false),
        );
        assert!(result.unwrap_err().contains("未提交、未扣字"));
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn exact_budget_uses_effective_units_and_needs_no_rewrite() {
        let transport = RepairingTransport {
            requests: Mutex::new(Vec::new()),
            repaired_slot: SlotId::Aff3,
        };
        let content = serde_json::json!({"schemaVersion":"generated-turn.v1", "slot":"AFF_3", "text":"AI 123你好。🙂"}).to_string();
        let turn = decode_or_repair_turn(
            &transport,
            &content,
            &constrained_context(&AiContext::default(), SlotId::Aff3, 4),
            SlotId::Aff3,
            4,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(turn.text, "AI 123你好。🙂");
        assert!(transport.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn a_repaired_turn_with_the_wrong_slot_is_still_rejected() {
        let transport = RepairingTransport {
            requests: Mutex::new(Vec::new()),
            repaired_slot: SlotId::Aff3,
        };
        let context = constrained_context(&AiContext::default(), SlotId::Neg2, 140);
        assert!(decode_or_repair_turn(
            &transport,
            "invalid",
            &context,
            SlotId::Neg2,
            140,
            &AtomicBool::new(false)
        )
        .is_err());
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }
}
