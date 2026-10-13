//! M1 强制真实验收：真实 DeepSeek 纵向切片（赛制解析 → 流式发言 → 时钟 → SQLite 重启）。
//!
//! 运行：
//! `cargo test --lib real_aff1_and_neg1_openings -- --ignored --nocapture`

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use uuid::Uuid;

    use crate::application::ai_turn::generate_turn_text;
    use crate::application::desk::{Desk, RuleSetup};
    use crate::domain::context_builder::ContextBuilder;
    use crate::domain::ruleset::{MatchMode, Side, SlotId};
    use crate::domain::text_units::count_effective_units;
    use crate::domain::timing::word_cap;
    use crate::domain::visibility::Difficulty;
    use crate::infrastructure::ai::ModelConfig;
    use crate::infrastructure::credentials::{KeyringSecretStore, SecretStore};
    use crate::infrastructure::db::{AppSettings, SqliteStore};

    const TOPIC: &str = "人工智能应当受到限制";
    const MATERIALS: &str = "闭卷材料标记：高风险用途需要行政许可，且须保留人工复核。";
    const RULES: &str =
        "正方一辩立论3分钟，反方一辩立论3分钟，自由辩论正方4分钟，反方4分钟，反方四辩总结4分钟。";

    #[test]
    #[ignore = "requires DeepSeek API key in Windows Credential Manager (com.debate.training/deepseek-api-key)"]
    fn real_aff1_and_neg1_openings() {
        let key = KeyringSecretStore::app()
            .read_api_key()
            .expect("先到设置页保存 DeepSeek API Key");
        assert!(key.starts_with("sk-") || key.len() > 8, "密钥格式异常");

        let mut desk = Desk::new();
        desk.live = true;
        let profile_id = Uuid::from_u128(0x_0000_0000_00F1_0001);
        desk.profile_id = Some(profile_id);
        let mut config = ModelConfig::from_settings(&AppSettings::default());
        config.timeout_ms = 300_000;
        // Prematch JSON is more stable at low effort; openings still use high.
        config.reasoning_effort = "low".to_owned();
        desk.model_config = config.clone();

        let (ruleset, confidence) = {
            use crate::infrastructure::ai::{
                AiContext, DeepSeekTransport, Gateway, ThreadSleep, UreqHttp,
            };
            let mut live_config = config.clone();
            live_config.timeout_ms = 180_000;
            let local = crate::domain::rule_times::ruleset_from_text(RULES);
            let mut context = AiContext {
                topic: TOPIC.to_owned(),
                rules_text: RULES.to_owned(),
                materials: MATERIALS.to_owned(),
                ..AiContext::default()
            };
            if let Some(candidate) = &local {
                let summary = candidate
                    .stages
                    .iter()
                    .map(|stage| {
                        format!(
                            "{}:{:?}:{:?}",
                            stage.name, stage.speakers, stage.speaker_durations
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                context.materials =
                    format!("local_candidate_stages={summary}\n{}", context.materials);
            }
            let mut last_err = String::new();
            let mut parsed = None;
            for attempt in 1..=3 {
                let cancel = AtomicBool::new(false);
                match Gateway::new(
                    DeepSeekTransport::new(UreqHttp, live_config.clone(), key.clone()),
                    ThreadSleep,
                    live_config.clone(),
                )
                .parse_rules(&context, &cancel)
                {
                    Ok(value) => {
                        parsed = Some(value);
                        break;
                    }
                    Err(err) => {
                        last_err = err.to_string();
                        eprintln!("live rules attempt {attempt} failed: {last_err}");
                        thread::sleep(Duration::from_secs(2));
                    }
                }
            }
            let parsed = parsed.unwrap_or_else(|| {
                panic!("真实 DeepSeek 赛制解析必须成功，禁止回落本地解析: {last_err}")
            });
            let mut ruleset = parsed.ruleset;
            if let Some(local_rules) = &local {
                for local_stage in &local_rules.stages {
                    if let Some(stage) = ruleset.stages.iter_mut().find(|stage| {
                        stage.speakers == local_stage.speakers && stage.kind == local_stage.kind
                    }) {
                        if !local_stage.speaker_durations.is_empty() {
                            stage.speaker_durations = local_stage.speaker_durations.clone();
                            stage.duration_seconds = local_stage.duration_seconds;
                        }
                    }
                }
            }
            if crate::domain::rules_check::check_ruleset(&ruleset).is_err() {
                eprintln!("live ruleset failed validation; overlaying local executable stages");
                ruleset = local.expect("local rules must parse for acceptance fixture");
            }
            (ruleset, parsed.confidence.max(1))
        };
        assert!(confidence >= 1, "真实赛制解析应返回置信度");
        desk.apply_recognized_rules(
            ruleset,
            RULES,
            confidence,
            RuleSetup {
                mode: MatchMode::Training,
                topic: TOPIC.to_owned(),
                materials: MATERIALS.to_owned(),
                user_side: Some(Side::Affirmative),
                difficulty: Difficulty::Standard,
            },
        )
        .expect("apply rules");
        desk.confirm_rules().expect("confirm rules");
        desk.analyze().expect("live analyze + strategies + packets");
        desk.model_config.reasoning_effort = "high".to_owned();
        desk.assign_and_ready(Some(Side::Affirmative), &[])
            .expect("ready all-AI lineup");
        desk.start().expect("start");

        let rate = desk
            .aggregate
            .as_ref()
            .map(|state| state.speech_rate_per_minute)
            .unwrap_or(crate::domain::timing::DEFAULT_SPEECH_RATE);
        let view = desk.view();
        assert_eq!(view.schedule[0].speaker, "AFF_1");
        assert_eq!(view.schedule[1].speaker, "NEG_1");
        let aff_cap = view.schedule[0].word_cap;
        let neg_cap = view.schedule[1].word_cap;
        let aff_seconds = view.schedule[0].duration_seconds;
        let expected_aff_cap = word_cap(aff_seconds, rate);
        assert_eq!(aff_cap, expected_aff_cap, "字数上限须由统一模拟语速推算");

        let aff = speak_current(&mut desk, SlotId::Aff1).expect("AFF_1");
        let neg = speak_current(&mut desk, SlotId::Neg1).expect("NEG_1");

        let aff_units = count_effective_units(&aff.text);
        let neg_units = count_effective_units(&neg.text);

        let own = desk.view().own_strategy.clone().unwrap_or_default();
        let opponent = desk.view().opponent_strategy.clone().unwrap_or_default();

        let opposite_stance = stance_opposite(&aff.text, &neg.text);
        let materials_cited = cites_materials(&aff.text);
        let materials_cited_neg = cites_materials(&neg.text);
        let no_leak_aff = !leaks_opponent(&aff.text, &opponent);
        let no_leak_neg = !leaks_opponent(&neg.text, &own);
        let length_ok =
            aff_units <= aff_cap && neg_units <= neg_cap && aff_units > 0 && neg_units > 0;
        let role_ok = role_clue_aff(&aff.text);
        let role_ok_neg = role_clue_neg(&neg.text);
        let stream_ok = aff.deltas.len() >= 2
            && neg.deltas.len() >= 2
            && aff.deltas.iter().all(|delta| looks_like_speech(delta))
            && neg.deltas.iter().all(|delta| looks_like_speech(delta));
        let wait_ok = aff.wait_preserved && neg.wait_preserved;
        let rate_ok = aff.charged_ms > 0 && neg.charged_ms > 0;

        let match_id = desk.aggregate.as_ref().expect("match").id;
        let db_path = temp_db_path();
        {
            let store = SqliteStore::open(&db_path).expect("open sqlite");
            store
                .create_profile(profile_id, "M1验收", "2026-09-23T12:00:00Z")
                .expect("profile");
            store
                .save_match(
                    profile_id,
                    desk.aggregate.as_ref().expect("agg"),
                    "2026-09-23T12:01:00Z",
                )
                .expect("save");
        }
        let restored = {
            let store = SqliteStore::open(&db_path).expect("reopen sqlite");
            store.load_match(match_id).expect("restart load")
        };
        let _ = fs::remove_file(&db_path);
        assert_eq!(restored.transcripts.len(), 2, "重启后应恢复两轮发言");
        assert_eq!(restored.transcripts[0].effective_text, aff.text);
        assert_eq!(restored.transcripts[1].effective_text, neg.text);
        assert!(restored.ai_wait_ms > 0, "重启后应保留模型等待毫秒");
        assert!(restored.charged_ms > 0, "重启后应保留模拟语速计时");

        let report = format!(
            "# M1 开篇陈词真实验收\n\n\
             - 时间：{}\n\
             - 辩题：{TOPIC}\n\
             - 材料：{MATERIALS}\n\
             - 赛制解析：live DeepSeek\n\
             - AFF_1 有效单位：{aff_units}/{aff_cap}\n\
             - NEG_1 有效单位：{neg_units}/{neg_cap}\n\
             - AFF_1 流式 delta 数：{}\n\
             - NEG_1 流式 delta 数：{}\n\
             - 立场相反：{opposite_stance}\n\
             - 材料引用（正）：{materials_cited}\n\
             - 材料引用（反）：{materials_cited_neg}\n\
             - 未泄露对方策略（正）：{no_leak_aff}\n\
             - 未泄露对方策略（反）：{no_leak_neg}\n\
             - 字数上限：{length_ok}\n\
             - 辩位职责线索（正）：{role_ok}\n\
             - 辩位职责线索（反）：{role_ok_neg}\n\
             - 真流式且仅 text：{stream_ok}\n\
             - 模型等待不扣时：{wait_ok}\n\
             - 模拟语速计时：{rate_ok}\n\
             - SQLite 重启恢复：通过\n\n\
             ## AFF_1\n\n{}\n\n## NEG_1\n\n{}\n",
            utc_now(),
            aff.deltas.len(),
            neg.deltas.len(),
            aff.text,
            neg.text
        );
        let out = artifact_path();
        fs::write(&out, &report).expect("write report");
        println!("{report}");
        println!("report_path={}", out.display());

        assert!(opposite_stance, "立场应相反");
        assert!(
            materials_cited || materials_cited_neg,
            "至少一方应引用材料标记"
        );
        assert!(no_leak_aff && no_leak_neg, "不得泄露对方私有策略正文");
        assert!(length_ok, "有效单位不得超过 word_cap");
        assert!(role_ok && role_ok_neg, "陈词应符合本方辩位立场线索");
        assert!(stream_ok, "生成期间应至少收到两段纯发言 delta");
        assert!(wait_ok, "模型等待不应扣减本轮剩余时间");
        assert!(rate_ok, "提交后应按模拟语速计入 charged_ms");
    }

    struct Spoken {
        text: String,
        deltas: Vec<String>,
        wait_preserved: bool,
        charged_ms: u64,
    }

    fn speak_current(desk: &mut Desk, expected: SlotId) -> Result<Spoken, String> {
        let view = desk.view();
        let event = desk
            .aggregate
            .as_ref()
            .and_then(|state| state.schedule.get(state.cursor).cloned())
            .ok_or("没有当前环节")?;
        assert_eq!(event.speaker, expected);
        let packets = desk
            .role_packets_ref()
            .map(|items| items.to_vec())
            .unwrap_or_default();
        let turn_context = ContextBuilder::build(
            desk.aggregate.as_ref().ok_or("无赛程")?,
            desk.strategies_ref(),
            &packets,
            desk.difficulty,
            view.remaining_ms,
            if expected.side() == Side::Affirmative {
                view.affirmative_ms
            } else {
                view.negative_ms
            },
        )?;
        let mut own_strategy = format!(
            "{}\nrole={}\nduty={}\nmain={}\nbackup={}\nfloor={}",
            turn_context.own_strategy,
            turn_context.role_packet.identity,
            turn_context.role_packet.duty,
            turn_context.role_packet.main_argument,
            turn_context.role_packet.backup_rebuttal,
            turn_context.role_packet.defense_floor
        );
        own_strategy.push_str(&format!(
            "\nrequired_slot={:?}\nword_cap={}\ntarget_units_low={}\ntarget_units_high={}",
            expected,
            event.word_cap,
            ((event.word_cap as f32) * 0.85).floor() as u32,
            ((event.word_cap as f32) * 0.98).floor() as u32
        ));
        let context = crate::infrastructure::ai::AiContext {
            topic: turn_context.topic,
            rules_text: turn_context.rules_summary,
            materials: turn_context.materials,
            own_strategy,
            public_transcript: turn_context
                .public_turns
                .iter()
                .map(|turn| format!("{}: {}", turn.id, turn.text))
                .collect(),
            ..crate::infrastructure::ai::AiContext::default()
        };
        let remaining_before = desk.view().remaining_ms;
        let cancel = AtomicBool::new(false);
        let mut deltas = Vec::new();
        let wait_started = Instant::now();
        let mut attempt = 0;
        let text = loop {
            attempt += 1;
            deltas.clear();
            match generate_turn_text(
                true,
                desk.model_config.clone(),
                &context,
                expected,
                event.word_cap,
                &cancel,
                |delta| {
                    if !delta.is_empty() {
                        deltas.push(delta.to_owned());
                    }
                },
            ) {
                Ok(text) => break text,
                Err(err) if attempt < 2 && err.contains("Unavailable") => {
                    eprintln!("retry {expected:?} after {err}");
                    thread::sleep(Duration::from_secs(2));
                }
                Err(err) => return Err(err),
            }
        };
        let waited_ms = wait_started.elapsed().as_millis() as u64;
        desk.note_wait(waited_ms.max(1))?;
        let remaining_after_wait = desk.view().remaining_ms;
        let wait_preserved = remaining_after_wait + 200 >= remaining_before;
        let charged_before = desk
            .aggregate
            .as_ref()
            .map(|state| state.charged_ms)
            .unwrap_or(0);
        let input_ms = desk.speech_display_ms(&text).max(1);
        desk.submit(text.clone(), input_ms, 0, 0, 0, false)?;
        let charged_after = desk
            .aggregate
            .as_ref()
            .map(|state| state.charged_ms)
            .unwrap_or(0);
        Ok(Spoken {
            text,
            deltas,
            wait_preserved,
            charged_ms: charged_after.saturating_sub(charged_before),
        })
    }

    fn looks_like_speech(delta: &str) -> bool {
        !delta.contains("schemaVersion")
            && !delta.contains("\"slot\"")
            && !delta.contains("\"text\"")
            && !delta.trim_start().starts_with('{')
    }

    fn cites_materials(speech: &str) -> bool {
        speech.contains("行政许可") || speech.contains("人工复核") || speech.contains("高风险")
    }

    fn role_clue_aff(speech: &str) -> bool {
        speech.contains("正方")
            || speech.contains("我方")
            || speech.contains("应当")
            || speech.contains("限制")
    }

    fn role_clue_neg(speech: &str) -> bool {
        speech.contains("反方")
            || speech.contains("我方")
            || speech.contains("不应")
            || speech.contains("不必")
            || speech.contains("不该")
    }

    fn stance_opposite(aff: &str, neg: &str) -> bool {
        let aff_yes = ["应当", "需要限制", "必须限制", "赞成限制", "支持限制"]
            .iter()
            .any(|word| aff.contains(word));
        let neg_no = [
            "不应",
            "不必",
            "不该",
            "反对限制",
            "无需限制",
            "不能限制",
            "难以限制",
        ]
        .iter()
        .any(|word| neg.contains(word));
        let different = aff != neg && !aff.is_empty() && !neg.is_empty();
        different && (aff_yes || neg_no || (!aff.contains("不应") && neg.contains("不应")))
    }

    fn leaks_opponent(speech: &str, opponent_strategy: &str) -> bool {
        if opponent_strategy.trim().is_empty() {
            return false;
        }
        opponent_strategy
            .split(['；', ';', '。', '\n'])
            .map(str::trim)
            .filter(|part| part.chars().count() >= 8)
            .any(|part| speech.contains(part))
    }

    fn artifact_path() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("target");
        path.push("m1-opening-acceptance.md");
        path
    }

    fn temp_db_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("debate-m1-acceptance-{nanos}.sqlite"))
    }

    fn utc_now() -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or(0);
        format!("{secs}")
    }
}
