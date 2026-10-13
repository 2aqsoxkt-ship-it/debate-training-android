use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, PartialEq, Eq)]
enum GraphemeClass {
    Cjk,
    Latin,
    Digit,
    Skip,
}

pub fn count_effective_units(text: &str) -> u32 {
    let mut units = 0;
    let mut previous: Option<GraphemeClass> = None;
    for grapheme in text.graphemes(true) {
        let class = classify(grapheme);
        if starts_unit(class, previous) {
            units += 1;
        }
        previous = Some(class);
    }
    units
}

pub fn effective_prefix(text: &str, cap: u32) -> &str {
    let mut units = 0;
    let mut previous: Option<GraphemeClass> = None;
    let mut end = 0;
    for (offset, grapheme) in text.grapheme_indices(true) {
        let class = classify(grapheme);
        let next = if starts_unit(class, previous) {
            units + 1
        } else {
            units
        };
        if next > cap {
            break;
        }
        units = next;
        previous = Some(class);
        end = offset + grapheme.len();
    }
    &text[..end]
}

pub fn split_effective(text: &str, cap: u32) -> (&str, &str) {
    let prefix = effective_prefix(text, cap);
    (prefix, &text[prefix.len()..])
}

fn starts_unit(class: GraphemeClass, previous: Option<GraphemeClass>) -> bool {
    match class {
        GraphemeClass::Cjk => true,
        GraphemeClass::Latin => previous != Some(GraphemeClass::Latin),
        GraphemeClass::Digit => previous != Some(GraphemeClass::Digit),
        GraphemeClass::Skip => false,
    }
}

fn classify(grapheme: &str) -> GraphemeClass {
    if is_emoji(grapheme) {
        return GraphemeClass::Skip;
    }
    let Some(base) = grapheme.chars().find(|character| !is_combining(*character)) else {
        return GraphemeClass::Skip;
    };
    if is_cjk(base) {
        GraphemeClass::Cjk
    } else if is_latin_letter(base) {
        GraphemeClass::Latin
    } else if is_digit(base) {
        GraphemeClass::Digit
    } else {
        GraphemeClass::Skip
    }
}

fn is_emoji(grapheme: &str) -> bool {
    grapheme.chars().any(|character| {
        matches!(
            character,
            '\u{1F000}'..='\u{1FAFF}' | '\u{2600}'..='\u{27BF}' | '\u{FE0F}' | '\u{200D}'
        )
    })
}

fn is_combining(character: char) -> bool {
    matches!(character, '\u{0300}'..='\u{036F}' | '\u{20D0}'..='\u{20FF}')
}

fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{3040}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

fn is_latin_letter(character: char) -> bool {
    matches!(
        character,
        'A'..='Z'
            | 'a'..='z'
            | '\u{00C0}'..='\u{00D6}'
            | '\u{00D8}'..='\u{00F6}'
            | '\u{00F8}'..='\u{024F}'
    )
}

fn is_digit(character: char) -> bool {
    character.is_ascii_digit() || ('\u{FF10}'..='\u{FF19}').contains(&character)
}

#[cfg(test)]
mod tests {
    use super::{count_effective_units, effective_prefix};

    #[test]
    fn counts_chinese_english_digits_punctuation_emoji_and_mixed_text() {
        assert_eq!(count_effective_units("你好"), 2);
        assert_eq!(count_effective_units("hello world"), 2);
        assert_eq!(count_effective_units("2024"), 1);
        assert_eq!(count_effective_units("12 34"), 2);
        assert_eq!(count_effective_units("hello, world!"), 2);
        assert_eq!(count_effective_units("😀"), 0);
        assert_eq!(count_effective_units("你好😀世界"), 4);
        assert_eq!(count_effective_units("AI辩论"), 3);
        assert_eq!(count_effective_units("**要点**"), 2);
        assert_eq!(count_effective_units("あ"), 1);
        assert_eq!(count_effective_units("가"), 1);
        assert_eq!(count_effective_units("１２"), 1);
    }

    #[test]
    fn counts_combining_marks_inside_one_grapheme() {
        assert_eq!(count_effective_units("cafe\u{0301}"), 1);
        assert_eq!(count_effective_units("a\u{0301}b"), 1);
        let family = "👨\u{200D}👩\u{200D}👧\u{200D}👦";
        assert_eq!(count_effective_units(family), 0);
        assert_eq!(effective_prefix(family, 0), family);
    }

    #[test]
    fn truncates_on_grapheme_boundaries_without_passing_the_cap() {
        assert_eq!(effective_prefix("你好世界", 2), "你好");
        assert_eq!(effective_prefix("hello world", 1), "hello ");
        assert_eq!(effective_prefix("hello世界", 2), "hello世");
        assert_eq!(effective_prefix("你", 0), "");
        let emoji = "😀你";
        assert_eq!(effective_prefix(emoji, 0), "😀");
        assert!(effective_prefix("你好", 1).chars().count() >= 1);
    }
}
