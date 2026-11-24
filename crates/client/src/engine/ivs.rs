//! Registered IVS choices, in stable official selector order. No extra user history.

use std::{collections::BTreeMap, sync::OnceLock};

const DATA: &str = include_str!("../../data/ivs_sequences.txt");
const ENCODED_VARIANTS: &str = include_str!("../../data/ideograph_variants.txt");

fn variant_bases(base: char) -> Vec<char> {
    for line in ENCODED_VARIANTS.lines().filter(|line| !line.starts_with('#') && !line.trim().is_empty()) {
        let group: Vec<char> = line.split_whitespace().map(|value| {
            char::from_u32(u32::from_str_radix(value, 16).expect("generated variant hexadecimal"))
                .expect("generated variant scalar")
        }).collect();
        if group.contains(&base) { return group; }
    }
    vec![base]
}

fn table() -> &'static BTreeMap<char, Vec<char>> {
    static TABLE: OnceLock<BTreeMap<char, Vec<char>>> = OnceLock::new();
    TABLE.get_or_init(|| DATA.lines().filter(|line| !line.starts_with('#')).map(|line| {
        let mut scalars = line.split_whitespace().map(|value| {
            char::from_u32(u32::from_str_radix(value, 16).expect("generated IVS hexadecimal"))
                .expect("generated IVS scalar")
        });
        let base = scalars.next().expect("generated IVS base");
        (base, scalars.collect())
    }).collect())
}

pub fn is_selector(ch: char) -> bool {
    ('\u{e0100}'..='\u{e01ef}').contains(&ch)
}

pub fn contains_ivs(text: &str) -> bool {
    text.chars().any(is_selector)
}

pub fn contains_encoded_variant(text: &str) -> bool {
    text.chars().any(|ch| variant_bases(ch).iter().skip(1).any(|variant| *variant == ch))
}

pub fn shortcut(vk: usize, ctrl: bool, shift: bool) -> bool {
    vk == 0x1C && ctrl && shift
}

fn target(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let base = chars.next()?;
    if !table().contains_key(&base) { return None; }
    let selector = chars.next();
    if selector.is_some_and(|ch| !is_selector(ch)) || chars.next().is_some() { return None; }
    Some(base)
}

pub fn can_cycle(text: &str) -> bool {
    variation_target(text).is_some()
}

// A segment with one ideograph plus okurigana has an unambiguous target too.
fn variation_target(text: &str) -> Option<(&str, &str, &str)> {
    let mut found = None;
    let mut chars = text.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if is_selector(ch) { return None; }
        if table().contains_key(&ch) {
            if found.is_some() { return None; }
            let mut end = start + ch.len_utf8();
            if let Some(&(at, selector)) = chars.peek() {
                if is_selector(selector) { end = at + selector.len_utf8(); chars.next(); }
            }
            found = Some((&text[..start], &text[start..end], &text[end..]));
        } else if !('ぁ'..='ゖ').contains(&ch) && !('ァ'..='ヶ').contains(&ch) {
            return None;
        }
    }
    found
}

pub fn next_choices(text: &str) -> Option<(Vec<String>, usize)> {
    let (prefix, selected, suffix) = variation_target(text)?;
    let base = target(selected)?;
    let bases = variant_bases(base);
    let mut choices: Vec<String> = bases.iter().map(|base| format!("{prefix}{base}{suffix}")).collect();
    for base in bases {
        choices.extend(table().get(&base)?.iter().map(|selector| format!("{prefix}{base}{selector}{suffix}")));
    }
    let current = choices.iter().position(|choice| choice == text).unwrap_or(0);
    let next = (current + 1) % choices.len();
    Some((choices, next))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_index_is_complete_valid_and_deduplicated() {
        assert_eq!(table().len(), 15304);
        for (base, selectors) in table() {
            assert!(!selectors.is_empty());
            assert!(selectors.iter().all(|selector| is_selector(*selector)));
            assert!(selectors.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(target(&format!("{base}{}", selectors[0])).is_some());
        }
        assert_eq!(table()[&'邊'].len(), 21);
        assert_eq!(table()[&'葛'].len(), 10);
    }

    #[test]
    fn single_selected_ideograph_only_and_utf16_surrogates_preserved() {
        for text in ["", "あ", "A", "渡邊", "邊\n", "邊\u{e0100}\u{e0101}", "\u{e0100}"] {
            assert!(!can_cycle(text), "{text}");
        }
        for text in ["邊", "邊\u{e0100}", "\u{2000b}", "\u{2000b}\u{e0100}"] {
            assert!(can_cycle(text), "{text}");
            let (choices, next) = next_choices(text).unwrap();
            let encoded: Vec<_> = choices[next].encode_utf16().collect();
            assert_eq!(String::from_utf16(&encoded).unwrap(), choices[next]);
        }
    }

    #[test]
    fn cycling_wraps_to_standard_without_stacking_selectors() {
        let mut text = "邊".to_string();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..22 {
            let (choices, next) = next_choices(&text).unwrap();
            text = choices[next].clone();
            assert!(seen.insert(text.clone()));
            assert!(text.chars().filter(|ch| is_selector(*ch)).count() <= 1);
        }
        assert_eq!(text, "邊");
    }

    #[test]
    fn shortcut_does_not_take_dictionary_or_space_keys() {
        assert!(shortcut(0x1C, true, true));
        assert!(!shortcut(0x1C, true, false));
        assert!(!shortcut(0x20, true, true));
        let (choices, next) = next_choices("邊").unwrap();
        assert_eq!(next, 1);
        assert_eq!(&choices[..3], &["邊", "邊\u{e0100}", "邊\u{e0101}"]);
    }

    #[test]
    fn rabbit_and_high_have_real_registered_sequences_and_encoded_variants() {
        for (normal, alternative) in [("兎", "兔"), ("高", "髙")] {
            let (choices, next) = next_choices(normal).unwrap();
            assert_eq!(next, 1);
            assert_eq!(choices[1], alternative);
            assert!(choices.iter().any(|text| contains_ivs(text)));
            assert!(choices.iter().all(|text| {
                let base = target(text).unwrap();
                let selector = text.chars().nth(1);
                selector.is_none_or(|selector| table()[&base].contains(&selector))
            }));
        }
        let (choices, _) = next_choices("兎").unwrap();
        assert!(choices.contains(&"兎\u{e0101}".to_string()));
        let (choices, next) = next_choices("高い").unwrap();
        assert_eq!(choices[next], "髙い");
        assert!(choices.contains(&"高\u{e0100}い".to_string()));
        assert!(!can_cycle("高山"));
        assert!(!can_cycle("高\u{e0100}\u{e0101}い"));
    }
}
