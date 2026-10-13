use regex::Regex;

use crate::domain::ruleset::{
    ClockOwner, RulesetV1, SlotId, StageDefinitionV1, StageKind, RULESET_SCHEMA_VERSION,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Speech {
    slot: SlotId,
    seconds: u32,
    kind: StageKind,
    questioner: Option<SlotId>,
    respondent: Option<SlotId>,
}

pub fn ruleset_from_text(source: &str) -> Option<RulesetV1> {
    let source = normalize_digits(source);
    if is_team_bank_four_on_four(&source) {
        let mut preset = crate::domain::ruleset_v2::traditional_four_on_four_v2();
        preset.source_text = source;
        return crate::domain::ruleset_v2::compile_v2_to_v1(&preset).ok();
    }
    let speeches = read_speeches(&source);
    if speeches.is_empty() {
        return None;
    }
    let duty = ["立论", "展开", "攻辩", "总结"].map(str::to_owned);
    Some(RulesetV1 {
        schema_version: RULESET_SCHEMA_VERSION.to_owned(),
        ruleset_name: "原文读出".to_owned(),
        source_text: source,
        affirmative_duties: duty.clone(),
        negative_duties: duty,
        stages: speeches
            .into_iter()
            .enumerate()
            .map(|(index, speech)| stage_from(index, speech))
            .collect(),
    })
}

fn is_team_bank_four_on_four(source: &str) -> bool {
    source.contains("双方各18分钟")
        && source.contains("自由辩")
        && source.contains("各4分钟")
        && source.contains("正方一辩")
        && source.contains("反方一辩")
        && source.contains("质询小结")
        && source.contains("总结陈词")
}

fn normalize_digits(source: &str) -> String {
    source
        .chars()
        .map(|character| match character {
            '０'..='９' => {
                char::from_u32(character as u32 - '０' as u32 + '0' as u32).unwrap_or(character)
            }
            '′' => '分',
            '″' => '秒',
            _ => character,
        })
        .collect()
}

fn read_speeches(source: &str) -> Vec<Speech> {
    read_clauses(source)
}

fn read_clauses(source: &str) -> Vec<Speech> {
    let mut speeches = Vec::new();
    for clause in source.split(['。', '；', ';', '\n', '，', ',']) {
        let clause = clause.trim();
        if clause.is_empty() {
            continue;
        }
        let parsed = read_clause(clause);
        if parsed.is_empty() {
            if let Some(speech) = dangling_side(clause, &speeches) {
                speeches.push(speech);
            }
        } else {
            speeches.extend(parsed);
        }
    }
    speeches
}

fn read_clause(clause: &str) -> Vec<Speech> {
    let slots = slots_in(clause);
    let times = times_in(clause);
    let kind = kind_of(clause);
    if slots.is_empty() && kind == StageKind::FreeDebate {
        return free_debate_without_slots(clause, &times);
    }
    if slots.len() >= 2 && times.len() == 1 && clause.contains('各') {
        return slots
            .into_iter()
            .map(|slot| speech(slot, times[0], kind_of_slot(clause, kind)))
            .collect();
    }
    if slots.len() == 2 && kind == StageKind::CrossExam && !times.is_empty() {
        return vec![Speech {
            slot: slots[0],
            seconds: times[0],
            kind: StageKind::CrossExam,
            questioner: Some(slots[0]),
            respondent: Some(slots[1]),
        }];
    }
    if slots.len() == times.len() && !slots.is_empty() {
        return slots
            .into_iter()
            .zip(times)
            .map(|(slot, seconds)| speech(slot, seconds, kind_of_slot(clause, kind)))
            .collect();
    }
    if slots.len() == 1 && !times.is_empty() {
        return vec![speech(slots[0], times[0], kind_of_slot(clause, kind))];
    }
    if kind == StageKind::FreeDebate {
        return free_debate_without_slots(clause, &times);
    }
    Vec::new()
}

fn dangling_side(clause: &str, speeches: &[Speech]) -> Option<Speech> {
    if clause.contains('辩')
        || clause.contains("立论")
        || clause.contains("总结")
        || clause.contains("质询")
        || clause.contains("驳")
    {
        return None;
    }
    let seconds = times_in(clause).into_iter().next()?;
    let last = speeches.last()?;
    if last.kind != StageKind::FreeDebate {
        return None;
    }
    if clause.contains("反方") && last.slot == SlotId::Aff3 {
        return Some(speech(SlotId::Neg3, seconds, StageKind::FreeDebate));
    }
    if clause.contains("正方") && last.slot == SlotId::Neg3 {
        return Some(speech(SlotId::Aff3, seconds, StageKind::FreeDebate));
    }
    None
}

fn speech(slot: SlotId, seconds: u32, kind: StageKind) -> Speech {
    Speech {
        slot,
        seconds,
        kind,
        questioner: None,
        respondent: None,
    }
}

fn kind_of_slot(clause: &str, kind: StageKind) -> StageKind {
    if kind == StageKind::FreeDebate || kind == StageKind::CrossExam {
        kind
    } else {
        kind_of(clause)
    }
}

fn free_debate_without_slots(clause: &str, times: &[u32]) -> Vec<Speech> {
    let sides = side_limits(clause);
    if !sides.is_empty() {
        return sides
            .into_iter()
            .map(|(side, seconds)| {
                let slot = if side == "正方" {
                    SlotId::Aff3
                } else {
                    SlotId::Neg3
                };
                speech(slot, seconds, StageKind::FreeDebate)
            })
            .collect();
    }
    if clause.contains('各') {
        if let Some(seconds) = times.first().copied() {
            return vec![
                speech(SlotId::Aff3, seconds, StageKind::FreeDebate),
                speech(SlotId::Neg3, seconds, StageKind::FreeDebate),
            ];
        }
    }
    Vec::new()
}

fn side_limits(clause: &str) -> Vec<(&'static str, u32)> {
    let mut limits = Vec::new();
    for side in ["正方", "反方"] {
        if let Some(index) = clause.find(side) {
            if let Some(seconds) = times_in(&clause[index..]).into_iter().next() {
                limits.push((side, seconds));
            }
        }
    }
    limits
}

fn slots_in(text: &str) -> Vec<SlotId> {
    let pattern = Regex::new(r"(正方|反方)\s*([一二三四])\s*辩").expect("slot pattern");
    pattern
        .captures_iter(text)
        .filter_map(|captures| {
            let side = captures.get(1)?.as_str();
            let rank = captures.get(2)?.as_str();
            slot_of(side, rank)
        })
        .collect()
}

fn slot_of(side: &str, rank: &str) -> Option<SlotId> {
    let affirmative = side == "正方";
    Some(match (affirmative, rank) {
        (true, "一") => SlotId::Aff1,
        (true, "二") => SlotId::Aff2,
        (true, "三") => SlotId::Aff3,
        (true, "四") => SlotId::Aff4,
        (false, "一") => SlotId::Neg1,
        (false, "二") => SlotId::Neg2,
        (false, "三") => SlotId::Neg3,
        (false, "四") => SlotId::Neg4,
        _ => return None,
    })
}

fn kind_of(text: &str) -> StageKind {
    if text.contains("自由") {
        StageKind::FreeDebate
    } else if text.contains("总结") || text.contains("结辩") || text.contains("小结") {
        StageKind::Summary
    } else if text.contains("质询") || text.contains("盘问") || text.contains("攻辩") {
        StageKind::CrossExam
    } else if text.contains("对辩") {
        StageKind::Custom
    } else {
        StageKind::Opening
    }
}

fn times_in(text: &str) -> Vec<u32> {
    let pattern = Regex::new(
        r"(\d+)\s*分\s*(\d+)\s*秒|(\d+)\s*分钟|(\d+)\s*分|(\d+)\s*秒|([零一二三四五六七八九十两]+)\s*分\s*([零一二三四五六七八九十两]+)\s*秒|([零一二三四五六七八九十两]+)\s*分钟|([零一二三四五六七八九十两]+)\s*分",
    )
    .expect("time pattern");
    pattern
        .captures_iter(text)
        .filter_map(|captures| {
            if let (Some(minutes), Some(seconds)) = (captures.get(1), captures.get(2)) {
                return Some(
                    minutes.as_str().parse::<u32>().unwrap_or(0) * 60
                        + seconds.as_str().parse::<u32>().unwrap_or(0),
                );
            }
            if let Some(minutes) = captures.get(3).or(captures.get(4)) {
                return minutes.as_str().parse::<u32>().ok().map(|value| value * 60);
            }
            if let Some(seconds) = captures.get(5) {
                return seconds.as_str().parse::<u32>().ok();
            }
            if let (Some(minutes), Some(seconds)) = (captures.get(6), captures.get(7)) {
                return Some(cn_number(minutes.as_str())? * 60 + cn_number(seconds.as_str())?);
            }
            captures
                .get(8)
                .or(captures.get(9))
                .and_then(|minutes| cn_number(minutes.as_str()))
                .map(|value| value * 60)
        })
        .filter(|seconds| *seconds > 0)
        .collect()
}

fn cn_number(text: &str) -> Option<u32> {
    if text.is_empty() {
        return None;
    }
    if text == "十" {
        return Some(10);
    }
    if let Some(rest) = text.strip_prefix('十') {
        return Some(10 + cn_digit(rest).unwrap_or(0));
    }
    if let Some(index) = text.find('十') {
        let tens = cn_digit(&text[..index])?;
        let rest = &text[index + '十'.len_utf8()..];
        let ones = if rest.is_empty() { 0 } else { cn_digit(rest)? };
        return Some(tens * 10 + ones);
    }
    cn_digit(text)
}

fn cn_digit(text: &str) -> Option<u32> {
    Some(match text {
        "零" => 0,
        "一" => 1,
        "二" | "两" => 2,
        "三" => 3,
        "四" => 4,
        "五" => 5,
        "六" => 6,
        "七" => 7,
        "八" => 8,
        "九" => 9,
        _ => return None,
    })
}

fn stage_from(index: usize, speech: Speech) -> StageDefinitionV1 {
    let clock = match speech.kind {
        StageKind::FreeDebate => ClockOwner::Team,
        StageKind::CrossExam => ClockOwner::Questioner,
        _ => ClockOwner::Individual,
    };
    StageDefinitionV1 {
        id: format!("read-{index}"),
        name: stage_name(speech.slot, speech.kind),
        kind: speech.kind,
        speakers: vec![speech.slot],
        speaker_durations: vec![speech.seconds],
        clock_owner: clock,
        duration_seconds: speech.seconds,
        early_end_allowed: true,
        follow_up_allowed: speech.kind == StageKind::CrossExam,
        interruptible: speech.kind == StageKind::CrossExam,
        interrupt_limit: u32::from(speech.kind == StageKind::CrossExam),
        questioner: speech.questioner,
        respondent: speech.respondent,
        eligible_questioners: Vec::new(),
        eligible_respondents: Vec::new(),
        initial_questioning_side: None,
        max_cross_exam_rounds: None,
        weight: 1,
        repeat: 1,
    }
}

fn stage_name(slot: SlotId, kind: StageKind) -> String {
    let person = match slot {
        SlotId::Aff1 => "正方一辩",
        SlotId::Aff2 => "正方二辩",
        SlotId::Aff3 => "正方三辩",
        SlotId::Aff4 => "正方四辩",
        SlotId::Neg1 => "反方一辩",
        SlotId::Neg2 => "反方二辩",
        SlotId::Neg3 => "反方三辩",
        SlotId::Neg4 => "反方四辩",
    };
    let action = match kind {
        StageKind::Opening => "陈词",
        StageKind::CrossExam => "质询",
        StageKind::FreeDebate => "自由辩",
        StageKind::Summary => "总结",
        StageKind::Custom => "发言",
    };
    format!("{person}{action}")
}

#[cfg(test)]
mod tests {
    use super::ruleset_from_text;
    use crate::domain::ruleset::{expand_schedule, SlotId, StageKind};
    use uuid::Uuid;

    #[test]
    fn reads_each_persons_limit_as_seconds() {
        let source = "正方一辩立论3分钟，反方一辩立论3分钟，正方二辩驳论2分30秒，自由辩论正方4分钟，反方5分钟，正方三辩质询反方三辩90秒，反方四辩总结4分钟。";
        let ruleset = ruleset_from_text(source).expect("readable");
        let events = expand_schedule(&ruleset, 240, || Uuid::from_u128(1)).expect("schedule");
        let rows: Vec<_> = events
            .iter()
            .map(|event| (event.speaker, event.kind, event.duration_seconds))
            .collect();
        assert_eq!(
            rows,
            vec![
                (SlotId::Aff1, StageKind::Opening, 180),
                (SlotId::Neg1, StageKind::Opening, 180),
                (SlotId::Aff2, StageKind::Opening, 150),
                (SlotId::Aff3, StageKind::FreeDebate, 240),
                (SlotId::Neg3, StageKind::FreeDebate, 300),
                (SlotId::Aff3, StageKind::CrossExam, 90),
                (SlotId::Neg4, StageKind::Summary, 240),
            ]
        );
    }

    #[test]
    fn team_bank_rules_never_collapse_to_only_free_debate() {
        let source = "双方各18分钟，自由辩各4分钟另外计时。陈词1：正方一辩发言，反方四辩质询；反方一辩陈词，正方四辩质询。质询方可以打断，被质询方不能打断、反问。陈词2：正方二辩发言，反方三辩质询正方二辩；反方二辩发言，正方三辩质询反方二辩。质询小结由双方三辩完成，先反方再正方。自由辩正方先开始，正反自动轮流发言。总结陈词由双方四辩完成，反方先发言。";
        let ruleset = ruleset_from_text(source).expect("readable team-bank rules");
        assert_eq!(ruleset.stages.len(), 13);
        assert_eq!(
            ruleset
                .stages
                .iter()
                .filter(|stage| stage.kind == StageKind::FreeDebate)
                .count(),
            1
        );
        assert_eq!(ruleset.stages[10].speaker_durations, vec![240]);
    }
}
