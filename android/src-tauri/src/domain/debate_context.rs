use crate::domain::ruleset::{Side, SlotId, StageKind};
use crate::domain::visibility::Difficulty;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePacket {
    pub slot: SlotId,
    pub identity: String,
    pub duty: String,
    pub main_argument: String,
    pub backup_rebuttal: String,
    pub cross_exam_target: String,
    pub defense_floor: String,
    pub summary_callback: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicTurn {
    pub id: String,
    pub slot: SlotId,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebateTurnContext {
    pub match_id: String,
    pub event_id: String,
    pub stage_id: String,
    pub topic: String,
    pub rules_summary: String,
    pub materials: String,
    pub side: Side,
    pub slot: SlotId,
    pub duty: String,
    pub stage_kind: StageKind,
    pub duration_seconds: u32,
    pub word_cap: u32,
    pub interruptible: bool,
    pub interrupt_limit: u32,
    pub own_strategy: String,
    pub role_packet: RolePacket,
    pub public_turns: Vec<PublicTurn>,
    pub latest_to_answer: Option<PublicTurn>,
    pub team_claims: Vec<String>,
    pub team_promises: Vec<String>,
    pub remaining_personal_ms: u64,
    pub remaining_team_ms: u64,
    pub difficulty: Difficulty,
}

impl DebateTurnContext {
    pub fn prompt_blocks(&self) -> String {
        let mut blocks = String::new();
        blocks.push_str(&format!("topic={}\n", self.topic));
        blocks.push_str(&format!("side={:?}\n", self.side));
        blocks.push_str(&format!("slot={:?}\n", self.slot));
        blocks.push_str(&format!("duty={}\n", self.duty));
        blocks.push_str(&format!("stage={:?}\n", self.stage_kind));
        blocks.push_str(&format!("duration_seconds={}\n", self.duration_seconds));
        blocks.push_str(&format!("word_cap={}\n", self.word_cap));
        blocks.push_str(&format!(
            "interruptible={} interrupt_limit={}\n",
            self.interruptible, self.interrupt_limit
        ));
        blocks.push_str(&format!("rules_summary={}\n", self.rules_summary));
        blocks.push_str(&format!("materials={}\n", self.materials));
        blocks.push_str(&format!("own_strategy={}\n", self.own_strategy));
        blocks.push_str(&format!(
            "role_packet.main_argument={}\n",
            self.role_packet.main_argument
        ));
        blocks.push_str(&format!("role_packet.duty={}\n", self.role_packet.duty));
        blocks.push_str("public_transcript:\n");
        for turn in &self.public_turns {
            blocks.push_str(&format!(
                "- id={} slot={:?} text={}\n",
                turn.id, turn.slot, turn.text
            ));
        }
        if let Some(latest) = &self.latest_to_answer {
            blocks.push_str(&format!(
                "latest_to_answer id={} text={}\n",
                latest.id, latest.text
            ));
        }
        blocks.push_str(&format!("team_claims={}\n", self.team_claims.join(" | ")));
        blocks.push_str(&format!(
            "team_promises={}\n",
            self.team_promises.join(" | ")
        ));
        blocks.push_str(&format!(
            "remaining_personal_ms={} remaining_team_ms={}\n",
            self.remaining_personal_ms, self.remaining_team_ms
        ));
        blocks.push_str(&format!("difficulty={:?}\n", self.difficulty));
        blocks
    }
}
