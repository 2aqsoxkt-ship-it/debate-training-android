//! Extract the JSON string value of `"text"` from a streaming object body.

#[derive(Debug, Default)]
pub struct SpeechTextExtractor {
    raw: String,
    emitted: usize,
}

impl SpeechTextExtractor {
    pub fn push(&mut self, delta: &str) -> Option<String> {
        if delta.is_empty() {
            return None;
        }
        self.raw.push_str(delta);
        let start = find_text_value_start(&self.raw)?;
        let value = decode_json_string_prefix(&self.raw[start..]);
        if value.len() <= self.emitted {
            return None;
        }
        let out = value[self.emitted..].to_owned();
        self.emitted = value.len();
        Some(out)
    }

    pub fn finished_text(&self) -> Option<String> {
        let start = find_text_value_start(&self.raw)?;
        Some(decode_json_string_prefix(&self.raw[start..]))
    }
}

fn find_text_value_start(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 6 < bytes.len() {
        if bytes[i] == b'"'
            && bytes.get(i + 1..i + 5) == Some(b"text")
            && bytes.get(i + 5) == Some(&b'"')
        {
            let mut j = i + 6;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) != Some(&b':') {
                i += 1;
                continue;
            }
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) == Some(&b'"') {
                return Some(j + 1);
            }
        }
        i += 1;
    }
    None
}

fn decode_json_string_prefix(source: &str) -> String {
    let mut out = String::new();
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => break,
            '\\' => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('u') => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        match chars.next() {
                            Some(digit) if digit.is_ascii_hexdigit() => hex.push(digit),
                            _ => return out,
                        }
                    }
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(decoded) = char::from_u32(code) {
                            out.push(decoded);
                        }
                    }
                }
                Some(other) => out.push(other),
                None => break,
            },
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::SpeechTextExtractor;

    #[test]
    fn extracts_only_the_speech_field_across_fragments() {
        let mut extractor = SpeechTextExtractor::default();
        assert!(extractor
            .push("{\"schemaVersion\":\"generated-turn.v1\",\"slot\":\"AFF_1\",\"tex")
            .is_none());
        assert_eq!(extractor.push("t\":\"你好").as_deref(), Some("你好"));
        assert_eq!(extractor.push("世界\"}").as_deref(), Some("世界"));
        assert_eq!(extractor.finished_text().as_deref(), Some("你好世界"));
    }

    #[test]
    fn ignores_text_mentions_outside_the_key() {
        let mut extractor = SpeechTextExtractor::default();
        let delta = "{\"schemaVersion\":\"generated-turn.v1\",\"slot\":\"NEG_1\",\"text\":\"材料含 text 字样\"}";
        assert_eq!(extractor.push(delta).as_deref(), Some("材料含 text 字样"));
    }
}
