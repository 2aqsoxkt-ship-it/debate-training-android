use crate::domain::events::TranscriptEntryV1;
use crate::domain::ruleset::SlotId;
use serde::Serialize;
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicResponseLink {
    pub source_turn_id: String,
    pub target_turn_ids: Vec<String>,
}

/// Public, replayable claims. Presence of a response is not proof of rebuttal.
/// No private plans, unsubmitted drafts, overflow or unrevealed tails enter it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicIssue {
    pub source_turn_id: String,
    pub slot: SlotId,
    pub public_claim: String,
    pub response_turn_ids: Vec<String>,
    pub status: String,
}

pub fn replay(entries: &[TranscriptEntryV1]) -> Vec<PublicIssue> {
    entries
        .iter()
        .filter(|entry| !entry.effective_text.trim().is_empty())
        .map(|source| {
            let responses = entries
                .iter()
                .filter(|entry| {
                    entry.id != source.id
                        && entry.cross_exam_exchange_id.is_some()
                        && entry.cross_exam_exchange_id == source.cross_exam_exchange_id
                        && entry.cross_exam_turn_kind
                            == Some(crate::domain::events::CrossExamTurnKind::Answer)
                })
                .map(|entry| entry.id.to_string())
                .collect::<Vec<_>>();
            PublicIssue {
                source_turn_id: source.id.to_string(),
                slot: source.slot,
                public_claim: source.effective_text.clone(),
                status: if responses.is_empty() {
                    "公开主张；回应情况待核"
                } else {
                    "存在公开回答；是否充分待核"
                }
                .to_owned(),
                response_turn_ids: responses,
            }
        })
        .collect()
}

pub fn prompt(entries: &[TranscriptEntryV1]) -> String {
    let ledger = replay(entries);
    let recent = ledger.iter().rev().take(24).collect::<Vec<_>>();
    format!("\n[可追溯公开交锋记录]\n{}\n引用只能使用真实 sourceTurnId。只记录已公开事实；回答存在不等于反驳成功，不得将待核主张当成胜负或自动加减分。优先回应对方最近主张，延续己方论证；指出重复和需要队友补证的问题。", serde_json::to_string(&recent).unwrap_or_default())
}

pub fn with_links(entries: &[TranscriptEntryV1], links: &[PublicResponseLink]) -> Vec<PublicIssue> {
    let mut ledger = replay(entries);
    for link in links {
        if !entries
            .iter()
            .any(|t| t.id.to_string() == link.source_turn_id)
        {
            continue;
        }
        for target in &link.target_turn_ids {
            if let Some(issue) = ledger.iter_mut().find(|i| i.source_turn_id == *target) {
                if !issue.response_turn_ids.contains(&link.source_turn_id) {
                    issue.response_turn_ids.push(link.source_turn_id.clone());
                }
                issue.status = "模型声明回应此处；是否充分待核".to_owned();
            }
        }
    }
    ledger
}

#[cfg(test)]
mod tests {
    #[test]
    fn replay_never_uses_private_candidate_or_overflow() {
        let mut aggregate = crate::domain::match_engine::play_constructed_match();
        aggregate.transcripts[0].overflow_text = "绝密超额正文".to_owned();
        aggregate.transcripts[0].full_text.push_str("绝密候选尾部");
        let prompt = super::prompt(&aggregate.transcripts);
        assert!(!prompt.contains("绝密"));
        assert!(super::replay(&aggregate.transcripts)
            .iter()
            .all(|issue| aggregate
                .transcripts
                .iter()
                .any(|t| t.id.to_string() == issue.source_turn_id)));
    }
}
