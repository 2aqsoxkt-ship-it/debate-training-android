//! Cross-examination prepared-answer reveal and interrupt state machine.

use crate::domain::floor::interrupt_at;
use crate::domain::text_units::{count_effective_units, split_effective};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossExamPhase {
    QuestionPending,
    QuestionSubmitted,
    AnswerPreparing,
    AnswerPrepared,
    AnswerRevealing,
    AnswerCompleted,
    AnswerInterrupted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAnswer {
    pub full_text: String,
    pub revealed_units: u32,
    pub interruptible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossExamState {
    pub phase: CrossExamPhase,
    pub question: String,
    pub prepared: Option<PreparedAnswer>,
    pub effective_answer: String,
    pub interrupted_tail: String,
    pub interrupt_reason: String,
    pub reveal_elapsed_ms: u64,
}

impl Default for CrossExamState {
    fn default() -> Self {
        Self::new()
    }
}

impl CrossExamState {
    pub fn new() -> Self {
        Self {
            phase: CrossExamPhase::QuestionPending,
            question: String::new(),
            prepared: None,
            effective_answer: String::new(),
            interrupted_tail: String::new(),
            interrupt_reason: String::new(),
            reveal_elapsed_ms: 0,
        }
    }

    pub fn submit_question(&mut self, question: String) -> Result<(), String> {
        if self.phase != CrossExamPhase::QuestionPending {
            return Err("当前不是提问阶段".to_owned());
        }
        if question.trim().is_empty() {
            return Err("问题不能为空".to_owned());
        }
        self.question = question;
        self.phase = CrossExamPhase::QuestionSubmitted;
        Ok(())
    }

    pub fn begin_prepare(&mut self) -> Result<(), String> {
        if self.phase != CrossExamPhase::QuestionSubmitted {
            return Err("请先提交问题".to_owned());
        }
        self.phase = CrossExamPhase::AnswerPreparing;
        Ok(())
    }

    pub fn set_prepared(&mut self, full_text: String, interruptible: bool) -> Result<(), String> {
        if self.phase != CrossExamPhase::AnswerPreparing
            && self.phase != CrossExamPhase::QuestionSubmitted
        {
            return Err("当前不能写入 prepared 回答".to_owned());
        }
        if full_text.trim().is_empty() {
            return Err("回答不能为空".to_owned());
        }
        self.prepared = Some(PreparedAnswer {
            full_text,
            revealed_units: 0,
            interruptible,
        });
        self.phase = CrossExamPhase::AnswerPrepared;
        Ok(())
    }

    pub fn start_reveal(&mut self) -> Result<(), String> {
        if self.phase != CrossExamPhase::AnswerPrepared {
            return Err("没有可展示的 prepared 回答".to_owned());
        }
        self.phase = CrossExamPhase::AnswerRevealing;
        Ok(())
    }

    /// Reveal up to `units` effective units (grapheme-safe via split_effective).
    pub fn reveal_units(&mut self, units: u32, elapsed_ms: u64) -> Result<String, String> {
        if self.phase != CrossExamPhase::AnswerRevealing {
            return Err("当前不在展示阶段".to_owned());
        }
        let prepared = self.prepared.as_mut().ok_or("缺少 prepared")?;
        let total = count_effective_units(&prepared.full_text);
        prepared.revealed_units = units.min(total);
        self.reveal_elapsed_ms = elapsed_ms;
        let (prefix, _) = split_effective(&prepared.full_text, prepared.revealed_units);
        Ok(prefix.to_owned())
    }

    /// Visible prefix only — never expose unrevealed tail to interrupter context.
    pub fn visible_prefix(&self) -> String {
        let Some(prepared) = &self.prepared else {
            return String::new();
        };
        split_effective(&prepared.full_text, prepared.revealed_units)
            .0
            .to_owned()
    }

    pub fn hidden_tail_for_storage_only(&self) -> String {
        let Some(prepared) = &self.prepared else {
            return String::new();
        };
        split_effective(&prepared.full_text, prepared.revealed_units)
            .1
            .to_owned()
    }

    pub fn interrupt(
        &mut self,
        reason: impl Into<String>,
        allowed: bool,
    ) -> Result<(String, String), String> {
        if self.phase != CrossExamPhase::AnswerRevealing {
            return Err("只能在展示过程中打断".to_owned());
        }
        let prepared = self.prepared.as_ref().ok_or("缺少 prepared")?;
        if !prepared.interruptible || !allowed {
            return Err("本环节不允许打断".to_owned());
        }
        let (prefix, tail) = interrupt_at(
            &prepared.full_text,
            prepared.revealed_units,
            true,
            false,
            false,
        )
        .map_err(|_| "打断位置无效".to_owned())?;
        self.effective_answer = prefix.clone();
        self.interrupted_tail = tail.clone();
        self.interrupt_reason = reason.into();
        self.phase = CrossExamPhase::AnswerInterrupted;
        // Drop prepared full text from active state after commit path reads it.
        Ok((prefix, tail))
    }

    pub fn complete_without_interrupt(&mut self) -> Result<String, String> {
        if self.phase != CrossExamPhase::AnswerRevealing {
            return Err("当前不在展示阶段".to_owned());
        }
        let prepared = self.prepared.as_ref().ok_or("缺少 prepared")?;
        let total = count_effective_units(&prepared.full_text);
        if prepared.revealed_units < total {
            return Err("回答尚未展示完毕".to_owned());
        }
        self.effective_answer = prepared.full_text.clone();
        self.interrupted_tail.clear();
        self.phase = CrossExamPhase::AnswerCompleted;
        Ok(self.effective_answer.clone())
    }

    pub fn clear_prepared_memory(&mut self) {
        self.prepared = None;
        self.interrupted_tail.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{CrossExamPhase, CrossExamState};
    use crate::domain::text_units::count_effective_units;

    #[test]
    fn interrupt_keeps_only_revealed_prefix_on_grapheme_boundary() {
        let mut state = CrossExamState::new();
        state.submit_question("请定义限制".into()).unwrap();
        state.begin_prepare().unwrap();
        state
            .set_prepared("我方认为应当限制高风险用途😀尾部".into(), true)
            .unwrap();
        state.start_reveal().unwrap();
        let visible = state.reveal_units(8, 2_000).unwrap();
        assert_eq!(count_effective_units(&visible), 8);
        assert!(!state.visible_prefix().contains("尾部"));
        let (prefix, tail) = state.interrupt("回避定义", true).unwrap();
        assert_eq!(prefix, visible);
        assert!(tail.contains("尾部") || !tail.is_empty());
        assert_eq!(state.phase, CrossExamPhase::AnswerInterrupted);
        assert!(!visible.contains("尾部"));
        state.clear_prepared_memory();
        assert!(state.prepared.is_none());
        assert!(state.interrupted_tail.is_empty());
    }

    #[test]
    fn four_cross_exam_controller_combinations_complete_or_interrupt() {
        for label in ["human_human", "human_ai", "ai_human", "ai_ai"] {
            let mut state = CrossExamState::new();
            state.submit_question(format!("问题-{label}")).unwrap();
            state.begin_prepare().unwrap();
            state
                .set_prepared(format!("完整回答内容足够长用于打断-{label}-尾部保密"), true)
                .unwrap();
            state.start_reveal().unwrap();
            let _ = state.reveal_units(6, 800).unwrap();
            if label == "ai_ai" {
                let full = state.complete_without_interrupt();
                assert!(full.is_err());
                let total = count_effective_units(&state.prepared.as_ref().unwrap().full_text);
                state.reveal_units(total, 2_000).unwrap();
                let text = state.complete_without_interrupt().unwrap();
                assert!(text.contains("完整回答"));
                assert!(state.interrupted_tail.is_empty());
            } else {
                let (prefix, tail) = state.interrupt(format!("打断-{label}"), true).unwrap();
                assert!(!prefix.is_empty());
                assert!(tail.contains("尾部") || !tail.is_empty());
            }
        }
    }

    #[test]
    fn free_debate_style_disallow_blocks_interrupt() {
        let mut state = CrossExamState::new();
        state.submit_question("问".into()).unwrap();
        state
            .set_prepared("完整回答内容足够长".into(), false)
            .unwrap();
        state.start_reveal().unwrap();
        state.reveal_units(4, 1_000).unwrap();
        assert!(state.interrupt("nope", false).is_err());
    }

    #[test]
    fn complete_writes_full_text_without_tail() {
        let mut state = CrossExamState::new();
        state.submit_question("问".into()).unwrap();
        let full = "这是完整回答";
        state.set_prepared(full.into(), true).unwrap();
        state.start_reveal().unwrap();
        let units = count_effective_units(full);
        state.reveal_units(units, 3_000).unwrap();
        let text = state.complete_without_interrupt().unwrap();
        assert_eq!(text, full);
        assert!(state.interrupted_tail.is_empty());
    }
}
