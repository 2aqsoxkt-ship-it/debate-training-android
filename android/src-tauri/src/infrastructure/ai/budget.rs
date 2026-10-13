//! Only enabled for explicitly scoped acceptance. Normal user usage is not
//! permanently capped. All real adapters share this durable micro-CNY ledger.
use super::TransportFault;
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::Value;
use std::path::Path;

const CAP: i64 = 5_000_000;
// Round the published 1M context upward to 2^20, rather than assuming M
// denotes exactly 1,000,000. Includes all message/template input overhead.
const INPUT_BOUND: i64 = 1_048_576;
const PRICE_VERSION: &str = "deepseek-flash-2026-10-12-peak-cold-cny-2-8";

pub struct BudgetTicket {
    conn: Connection,
    id: i64,
    reservation: i64,
    output_bound: i64,
    input_bound: i64,
}

impl BudgetTicket {
    pub fn from_env(url: &str, body: &str) -> Result<Option<Self>, TransportFault> {
        let Some(path) = std::env::var_os("DEBATE_ACCEPTANCE_LEDGER") else {
            return Ok(None);
        };
        if std::env::var("DEBATE_ACCEPTANCE_PRICE_VERSION").as_deref() != Ok(PRICE_VERSION) {
            return Err(TransportFault::Unavailable);
        }
        let tokenizer =
            std::env::var_os("DEBATE_ACCEPTANCE_TOKENIZER").ok_or(TransportFault::Unavailable)?;
        use sha2::{Digest, Sha256};
        let bytes = std::fs::read(tokenizer).map_err(|_| TransportFault::Unavailable)?;
        if format!("{:X}", Sha256::digest(&bytes))
            != "89085F12EF79460AC5F66D1119325DDFC694B4AB209D80BBD81D35F081DC9614"
        {
            return Err(TransportFault::Unavailable);
        }
        let tokenizer_path = std::path::PathBuf::from(
            std::env::var_os("DEBATE_ACCEPTANCE_TOKENIZER").ok_or(TransportFault::Unavailable)?,
        );
        let config_bytes = std::fs::read(tokenizer_path.with_file_name("tokenizer_config.json"))
            .map_err(|_| TransportFault::Unavailable)?;
        if format!("{:X}", Sha256::digest(&config_bytes))
            != "841F8CF146E3F0AD1082594A31F68ECF7608C20467EF355081333D85BBAEB1CB"
        {
            return Err(TransportFault::Unavailable);
        }
        if url != "https://api.deepseek.com/chat/completions" {
            return Err(TransportFault::Unavailable);
        }
        let wire_bytes = body.len();
        let body: Value = serde_json::from_str(body).map_err(|_| TransportFault::Unavailable)?;
        if body["model"] != "deepseek-flash"
            || body.get("tools").is_some()
            || body["messages"].as_array().is_none_or(|m| {
                m.iter().any(|m| {
                    !m["content"].is_string()
                        || !matches!(m["role"].as_str(), Some("system" | "user" | "assistant"))
                })
            })
        {
            return Err(TransportFault::Unavailable);
        }
        let output = body["max_tokens"]
            .as_i64()
            .filter(|n| *n > 0 && *n <= 384_000)
            .ok_or(TransportFault::Unavailable)?;
        let input_bound = verified_template_bound(&body, wire_bytes)?;
        Self::reserve_limits(Path::new(&path), input_bound, output)
            .map(Some)
            .map_err(|message| {
                tracing::warn!("acceptance budget blocked: {message}");
                TransportFault::Unavailable
            })
    }

    #[cfg(test)]
    fn reserve(path: &Path, output_bound: i64) -> Result<Self, String> {
        Self::reserve_limits(path, INPUT_BOUND, output_bound)
    }

    fn reserve_limits(path: &Path, input_bound: i64, output_bound: i64) -> Result<Self, String> {
        let mut conn = Connection::open(path).map_err(|_| "账本无法打开")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "账本正忙")?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS budget_meta (id INTEGER PRIMARY KEY CHECK(id=1), cap INTEGER NOT NULL, price_version TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS budget_calls (id INTEGER PRIMARY KEY AUTOINCREMENT, input_bound INTEGER NOT NULL, output_bound INTEGER NOT NULL, reservation INTEGER NOT NULL, charged INTEGER NOT NULL, status TEXT NOT NULL, prompt_tokens INTEGER, completion_tokens INTEGER);").map_err(|_|"账本建表失败")?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "账本正忙")?;
        tx.execute(
            "INSERT OR IGNORE INTO budget_meta VALUES (1,?1,?2)",
            params![CAP, PRICE_VERSION],
        )
        .map_err(|_| "账本初始化失败")?;
        let (cap, price): (i64, String) = tx
            .query_row(
                "SELECT cap,price_version FROM budget_meta WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| "账本读取失败")?;
        if cap != CAP || price != PRICE_VERSION {
            return Err("账本价格或上限不匹配，不允许重置".to_owned());
        }
        let (spent,unknown):(i64,i64)=tx.query_row("SELECT COALESCE(SUM(charged),0), COALESCE(SUM(CASE WHEN status<>'settled' THEN 1 ELSE 0 END),0) FROM budget_calls",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|"账本读取失败")?;
        if unknown > 0 {
            return Err("有请求用量未确认，保留最坏预留并停止新增调用".to_owned());
        }
        let reservation = input_bound * 2 + output_bound * 8;
        if spent + reservation > CAP {
            return Err("剩余 5 元预算不足以预留下一请求".to_owned());
        }
        tx.execute("INSERT INTO budget_calls(input_bound,output_bound,reservation,charged,status) VALUES (?1,?2,?3,?3,'reserved')",params![input_bound,output_bound,reservation]).map_err(|_|"预留失败")?;
        let id = tx.last_insert_rowid();
        tx.commit().map_err(|_| "预留提交失败")?;
        Ok(Self {
            conn,
            id,
            reservation,
            output_bound,
            input_bound,
        })
    }

    pub fn finish(self, usage: Option<(u64, u64)>, success: bool) -> Result<(), TransportFault> {
        let valid = usage.filter(|(p, c)| {
            success
                && *p > 0
                && *c > 0
                && *p <= self.input_bound as u64
                && *c <= self.output_bound as u64
        });
        let (charged, status, p, c) = if let Some((p, c)) = valid {
            ((p * 2 + c * 8) as i64, "settled", Some(p), Some(c))
        } else {
            (self.reservation, "usage_unknown", None, None)
        };
        self.conn.execute("UPDATE budget_calls SET charged=?2,status=?3,prompt_tokens=?4,completion_tokens=?5 WHERE id=?1",params![self.id,charged,status,p,c]).map_err(|_|TransportFault::Unavailable)?;
        if valid.is_none() {
            Err(TransportFault::Unavailable)
        } else {
            Ok(())
        }
    }
}

/// Verified official V4 tokenizer: no normalization, regex splits followed by
/// ByteLevel(add_prefix_space=false), byte BPE only merges tokens. Therefore
/// input tokens cannot exceed UTF-8 bytes of the supported chat template.
/// Add the entire wire JSON byte count as a second, conservative allowance
/// for API control fields. This is not a Chinese-character/token estimate.
fn verified_template_bound(body: &Value, wire_bytes: usize) -> Result<i64, TransportFault> {
    let allowed = [
        "model",
        "messages",
        "max_tokens",
        "stream",
        "stream_options",
        "response_format",
        "thinking",
        "reasoning_effort",
    ];
    if body
        .as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err(TransportFault::Unavailable);
    }
    let messages = body["messages"]
        .as_array()
        .ok_or(TransportFault::Unavailable)?;
    if messages.is_empty() || messages.len() > 64 {
        return Err(TransportFault::Unavailable);
    }
    let mut template = "<｜begin▁of▁sentence｜>".to_owned();
    let system = messages
        .iter()
        .filter(|m| m["role"] == "system")
        .map(|m| m["content"].as_str().ok_or(TransportFault::Unavailable))
        .collect::<Result<Vec<_>, _>>()?;
    template.push_str(&system.join("\n\n"));
    for message in messages {
        if message
            .as_object()
            .is_none_or(|o| o.keys().any(|k| !matches!(k.as_str(), "role" | "content")))
        {
            return Err(TransportFault::Unavailable);
        }
        let text = message["content"]
            .as_str()
            .ok_or(TransportFault::Unavailable)?;
        match message["role"].as_str() {
            Some("system") => {}
            Some("user") => {
                template.push_str("<｜User｜>");
                template.push_str(text);
            }
            Some("assistant") => {
                template.push_str("<｜Assistant｜>");
                template.push_str(text);
                template.push_str("<｜end▁of▁sentence｜>");
            }
            _ => return Err(TransportFault::Unavailable),
        }
    }
    template.push_str("<｜Assistant｜><think>\n");
    let upper = wire_bytes
        .checked_add(template.len())
        .ok_or(TransportFault::Unavailable)? as i64;
    if upper > INPUT_BOUND {
        return Err(TransportFault::Unavailable);
    }
    Ok(upper)
}

pub fn usage_from_body(body: &str) -> Option<(u64, u64)> {
    let mut latest = None;
    for line in body.lines() {
        let json = line.trim().strip_prefix("data:").unwrap_or(line).trim();
        if let Ok(value) = serde_json::from_str::<Value>(json) {
            if let (Some(p), Some(c)) = (
                value["usage"]["prompt_tokens"].as_u64(),
                value["usage"]["completion_tokens"].as_u64(),
            ) {
                latest = Some((p, c));
            }
        }
    }
    latest
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn template_bound_is_utf8_based_and_rejects_unsupported_message_shapes() {
        let body = serde_json::json!({"model":"deepseek-flash","messages":[{"role":"system","content":"约束"},{"role":"user","content":"中文🙂\n\"test\""},{"role":"assistant","content":"修复稿"}],"max_tokens":16384,"thinking":{"type":"enabled"},"reasoning_effort":"high"});
        let wire = body.to_string();
        let upper = verified_template_bound(&body, wire.len()).unwrap();
        assert!(upper > wire.len() as i64);
        assert!(upper < 1000);
        let mut unsupported = body.clone();
        unsupported["messages"][0]["tool_calls"] = serde_json::json!([]);
        assert!(verified_template_bound(&unsupported, wire.len()).is_err());
    }
    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "debate-budget-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    #[test]
    fn unknown_usage_and_pending_reservations_survive_restart() {
        let path = path();
        let ticket = BudgetTicket::reserve(&path, 8192).unwrap();
        assert!(BudgetTicket::reserve(&path, 8192).is_err());
        assert!(ticket.finish(None, false).is_err());
        assert!(BudgetTicket::reserve(&path, 8192).is_err());
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn two_matches_and_retries_cannot_reset_the_shared_cap() {
        let path = path();
        for _ in 0..2 {
            BudgetTicket::reserve(&path, 8192)
                .unwrap()
                .finish(Some((1_000_000, 1)), true)
                .unwrap();
        }
        assert!(BudgetTicket::reserve(&path, 8192).is_err());
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn usage_includes_every_completion_token_and_no_unknown_refund() {
        assert_eq!(
            usage_from_body(
                "data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":20}}\ndata: [DONE]"
            ),
            Some((10, 20))
        );
        assert!(usage_from_body("data: [DONE]").is_none());
    }
}
