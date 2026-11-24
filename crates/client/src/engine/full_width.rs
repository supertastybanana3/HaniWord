// https://www.unicode.org/charts/nameslist/n_FF00.html
// extracted with scripts/extract_fullwidth.py

use std::{cell::Cell, collections::HashMap, sync::LazyLock};

thread_local! {
    /// 2026-10-20: 直前に打った \ が ¥ キーのものか。「ろ」キー・US 配列の \ キーなら false で、全角にするとき ￥ ではなく ＼ にする
    static BACKSLASH_FROM_YEN_KEY: Cell<bool> = const { Cell::new(true) };
    /// 2026-11-05 (仕事 23): 未確定の文字列を見せる・確定するときに、打った数字を全角にするか (composition が描くたびに決める)
    static DIGIT_DISPLAY: Cell<DigitForm> = const { Cell::new(DigitForm::Off) };
}

/// 2026-11-05 (仕事 23): 数字の形 (設定 input.fullwidth_digits)。Off = 半角 (初期値)、Numbers = 英字・記号にくっついていない数字だけ全角
/// (5kg・GPT4o・mp3 は半角)、All = 英字にくっついた数字も全角 (５kg・GPT４o)。All でも式 (3x+1=0) と URL・メール・パスは半角のまま
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigitForm {
    Off,
    Numbers,
    All,
}

impl DigitForm {
    pub fn from_setting(value: &str) -> DigitForm {
        match value {
            "numbers" => DigitForm::Numbers,
            "all" => DigitForm::All,
            _ => DigitForm::Off,
        }
    }
}

/// 中で持つ数字 (打った列・変換サーバーへの読み・小数点・桁区切り・化学式・単位・数式・/ と . の判定) は半角のまま変えず、見せる所だけで全角にする
pub fn set_digit_display(form: DigitForm) {
    DIGIT_DISPLAY.set(form);
}

pub fn digit_display() -> DigitForm {
    DIGIT_DISPLAY.get()
}

/// 今の数字の形で見せる文字列 (Off ならそのまま)
pub fn displayed_digits(text: &str) -> String {
    match DIGIT_DISPLAY.get() {
        DigitForm::Off => text.to_string(),
        DigitForm::Numbers => fullwidth_digits(text),
        DigitForm::All => fullwidth_digits_all(text),
    }
}

/// All の形: 半角の英字・記号の並び (空白・かな・漢字で区切ったもの) ごとに、式 (= + * ^ < > ( ) を含む) と URL・メール・パス
/// (@・\・英字のとなりの / . :) でなければ、並びの中の数字を全角に (並びの中の数字にはさまれた . , / : も)。並びの外は Numbers と同じ
pub fn fullwidth_digits_all(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = fullwidth_digits(text).chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_graphic() { i += 1; continue; }
        let j = (i..chars.len()).find(|&k| !chars[k].is_ascii_graphic()).unwrap_or(chars.len());
        let token = &chars[i..j];
        let letter_side = |k: usize| (k > 0 && token[k - 1].is_ascii_alphabetic()) || token.get(k + 1).is_some_and(|c| c.is_ascii_alphabetic());
        let kept = token.iter().any(|c| "=+*^<>()@\\".contains(*c))
            || token.iter().enumerate().any(|(k, c)| matches!(c, '/' | '.' | ':') && letter_side(k));
        if !kept && token.iter().any(|c| c.is_ascii_digit()) {
            for k in i..j {
                let c = chars[k];
                out[k] = if c.is_ascii_digit() {
                    char::from_u32(c as u32 - '0' as u32 + '０' as u32).unwrap_or(c)
                } else if matches!(c, '.' | ',' | '/' | ':') && k > i && k + 1 < j && chars[k - 1].is_ascii_digit() && chars[k + 1].is_ascii_digit() {
                    match c { '.' => '．', ',' => '，', ':' => '：', _ => '／' }
                } else {
                    c
                };
            }
        }
        i = j;
    }
    out.into_iter().collect()
}

/// 見せる形: 半角の数字の並び (. , / でつないだ並び 2.5・1,000・1/2・2026/11/05 も 1 つ) のうち、両どなりが ASCII の英字・記号でないもの
/// (かな・漢字・全角の字・空白・端) を全角に。並びの中の . , / は ． ， ／ に (Wikipedia 日本語版で、全角の数字にはさまれた記号は
/// ． 149・. 68、， 101・, 23、／ 67・/ 31 と全角が多い)。英字や式の記号にくっついた数字 (GPT4o・mp3・5kg・3x+1=0) は半角のまま。
/// UTF-16 の長さは変わらない (下線・キャレットの位置はそのまま使える)
pub fn fullwidth_digits(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = chars.clone();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() { i += 1; continue; }
        let mut j = i;
        loop {
            while j < chars.len() && chars[j].is_ascii_digit() { j += 1; }
            if j + 1 < chars.len() && matches!(chars[j], '.' | ',' | '/') && chars[j + 1].is_ascii_digit() { j += 1; continue; }
            break;
        }
        let glued = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_graphic());
        if !glued(i.checked_sub(1).and_then(|k| chars.get(k))) && !glued(chars.get(j)) {
            for c in &mut out[i..j] {
                *c = match *c {
                    '.' => '．',
                    ',' => '，',
                    '/' => '／',
                    d => char::from_u32(d as u32 - '0' as u32 + '０' as u32).unwrap_or(d),
                };
            }
        }
        i = j;
    }
    out.into_iter().collect()
}

pub fn note_backslash_key(yen_key: bool) {
    BACKSLASH_FROM_YEN_KEY.set(yen_key);
}

// in azookey, fullwidth alphabet will not be processed
static HALF_FULL_AZOOKEY: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("!", "！"),
        ("\"", "”"),
        ("#", "＃"),
        ("$", "＄"),
        ("%", "％"),
        ("&", "＆"),
        ("'", "’"),
        ("(", "（"),
        (")", "）"),
        ("*", "＊"),
        ("+", "＋"),
        (",", "、"),
        ("-", "ー"),
        (".", "。"),
        ("/", "・"),
        // ("0", "０"),
        // ("1", "１"),
        // ("2", "２"),
        // ("3", "３"),
        // ("4", "４"),
        // ("5", "５"),
        // ("6", "６"),
        // ("7", "７"),
        // ("8", "８"),
        // ("9", "９"),
        (":", "："),
        (";", "；"),
        ("<", "＜"),
        ("=", "＝"),
        (">", "＞"),
        ("?", "？"),
        ("@", "＠"),
        // ("A", "Ａ"),
        // ("B", "Ｂ"),
        // ("C", "Ｃ"),
        // ("D", "Ｄ"),
        // ("E", "Ｅ"),
        // ("F", "Ｆ"),
        // ("G", "Ｇ"),
        // ("H", "Ｈ"),
        // ("I", "Ｉ"),
        // ("J", "Ｊ"),
        // ("K", "Ｋ"),
        // ("L", "Ｌ"),
        // ("M", "Ｍ"),
        // ("N", "Ｎ"),
        // ("O", "Ｏ"),
        // ("P", "Ｐ"),
        // ("Q", "Ｑ"),
        // ("R", "Ｒ"),
        // ("S", "Ｓ"),
        // ("T", "Ｔ"),
        // ("U", "Ｕ"),
        // ("V", "Ｖ"),
        // ("W", "Ｗ"),
        // ("X", "Ｘ"),
        // ("Y", "Ｙ"),
        // ("Z", "Ｚ"),
        ("[", "「"),
        ("\\", "￥"),
        ("]", "」"),
        ("^", "＾"),
        ("_", "＿"),
        ("`", "｀"),
        // ("a", "ａ"),
        // ("b", "ｂ"),
        // ("c", "ｃ"),
        // ("d", "ｄ"),
        // ("e", "ｅ"),
        // ("f", "ｆ"),
        // ("g", "ｇ"),
        // ("h", "ｈ"),
        // ("i", "ｉ"),
        // ("j", "ｊ"),
        // ("k", "ｋ"),
        // ("l", "ｌ"),
        // ("m", "ｍ"),
        // ("n", "ｎ"),
        // ("o", "ｏ"),
        // ("p", "ｐ"),
        // ("q", "ｑ"),
        // ("r", "ｒ"),
        // ("s", "ｓ"),
        // ("t", "ｔ"),
        // ("u", "ｕ"),
        // ("v", "ｖ"),
        // ("w", "ｗ"),
        // ("x", "ｘ"),
        // ("y", "ｙ"),
        // ("z", "ｚ"),
        ("{", "｛"),
        ("|", "｜"),
        ("}", "｝"),
        ("~", "～"),
    ])
});

static HALF_FULL: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("a", "ａ"),
        ("b", "ｂ"),
        ("c", "ｃ"),
        ("d", "ｄ"),
        ("e", "ｅ"),
        ("f", "ｆ"),
        ("g", "ｇ"),
        ("h", "ｈ"),
        ("i", "ｉ"),
        ("j", "ｊ"),
        ("k", "ｋ"),
        ("l", "ｌ"),
        ("m", "ｍ"),
        ("n", "ｎ"),
        ("o", "ｏ"),
        ("p", "ｐ"),
        ("q", "ｑ"),
        ("r", "ｒ"),
        ("s", "ｓ"),
        ("t", "ｔ"),
        ("u", "ｕ"),
        ("v", "ｖ"),
        ("w", "ｗ"),
        ("x", "ｘ"),
        ("y", "ｙ"),
        ("z", "ｚ"),
    ])
});

pub fn to_halfwidth(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '＼' {
                return "\\".to_string();
            }
            let key = c.to_string();
            if let Some((&k, _)) = HALF_FULL_AZOOKEY.iter().find(|(_, &v)| v == key) {
                k.to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

pub fn to_fullwidth(s: &str, process_alphabet: bool) -> String {
    s.chars()
        .map(|c| {
            let key = c.to_string();

            if process_alphabet {
                if let Some(&v) = HALF_FULL.get(key.as_str()) {
                    return v.to_string();
                }
            }

            if c == '\\' && !BACKSLASH_FROM_YEN_KEY.get() {
                "＼".to_string()
            } else if let Some(&v) = HALF_FULL_AZOOKEY.get(key.as_str()) {
                v.to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{fullwidth_digits, fullwidth_digits_all};

    /// 2026-11-05 (仕事 23): かなや漢字にはさまれた数字は全角 (間の . , / も)、英字や式の記号にくっついた数字は半角のまま
    #[test]
    fn digits_become_fullwidth_only_between_japanese() {
        for (text, expected) in [
            ("第3章", "第３章"),
            ("2.5キロ", "２．５キロ"),
            ("1,000円", "１，０００円"),
            ("1/2の", "１／２の"),
            ("2026/11/05に", "２０２６／１１／０５に"),
            ("1・2年生", "１・２年生"),
            ("GPT4oで", "GPT4oで"),
            ("5kgの", "5kgの"),
            ("3x+1=0", "3x+1=0"),
            ("H₂O", "H₂O"),
            ("12", "１２"),
            ("100。", "１００。"),
        ] {
            assert_eq!(fullwidth_digits(text), expected, "{text}");
            assert_eq!(fullwidth_digits(text).encode_utf16().count(), text.encode_utf16().count(), "{text}");
        }
    }

    /// 2026-11-05 (仕事 23、3 択の all): 英字にくっついた数字も全角。式と URL・メール・パスは半角のまま
    #[test]
    fn all_form_also_widens_digits_glued_to_letters() {
        for (text, expected) in [
            ("5kgの", "５kgの"),
            ("GPT4oで", "GPT４oで"),
            ("2.5kg", "２．５kg"),
            ("第3章", "第３章"),
            ("3x+1=0", "3x+1=0"),
            ("f(2)の", "f(2)の"),
            ("report2.pdfを", "report2.pdfを"),
            ("example.com/a1", "example.com/a1"),
            ("taro2@example.com", "taro2@example.com"),
            ("C:\\data1", "C:\\data1"),
            ("1/2の", "１／２の"),
            ("10:30に", "１０：３０に"),
        ] {
            assert_eq!(fullwidth_digits_all(text), expected, "{text}");
            assert_eq!(fullwidth_digits_all(text).encode_utf16().count(), text.encode_utf16().count(), "{text}");
        }
    }
}
