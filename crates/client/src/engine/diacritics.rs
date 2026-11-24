// アクセント付きのラテン文字 (ñ é ü ø …) を長押しで出す (iOS と同じ)。
//
// 文字キーを押し続けると (キーの連打 = 押しっぱなし)、その文字のアクセント付きの候補を候補窓に出す。
// 先頭候補をすぐ入力し、数字・矢印キーで選び直すと同じ文字を置き換える。Esc やほかのキーで候補を閉じる。
// どの言語の文字を出すかは設定 input.latin_languages (インストーラーの「設定を選んでから使う」か設定アプリで選ぶ。既定は空 = 使わない)。

use std::cell::Cell;
use std::sync::OnceLock;

use super::diacritics_freq::FREQUENCIES;

/// 言語ごとのアクセント付きの文字 (小文字)。候補はここに並べた言語の順に出す
const LANGUAGES: &[(&str, &str)] = &[
    ("fr", "àâæçèéêëîïôœùûüÿ"),
    ("de", "äöüß"),
    ("es", "áéíñóúü"),
    ("it", "àèéìòù"),
    ("pt", "àáâãçéêíóôõú"),
    ("nordic", "åæøäöðþ"),
    ("pl", "ąćęłńóśźż"),
    ("cs", "áčďéěíňóřšťúůýž"),
    ("tr", "çğıöşü"),
    ("pinyin", "āáǎàēéěèīíǐìōóǒòūúǔùǖǘǚǜ"),
];

/// 選べる言語の名前 (設定画面用)
pub const LANGUAGE_NAMES: &[(&str, &str)] = &[
    ("fr", "フランス語"),
    ("de", "ドイツ語"),
    ("es", "スペイン語"),
    ("it", "イタリア語"),
    ("pt", "ポルトガル語"),
    ("nordic", "北欧の言語"),
    ("pl", "ポーランド語"),
    ("cs", "チェコ語"),
    ("tr", "トルコ語"),
    ("pinyin", "ピンイン (声調)"),
];

/// アクセント付きの文字の元の文字 (ñ → n、æ → a、ß → s、ı → i、ð → d、þ → t)
fn base_of(c: char) -> char {
    const GROUPS: &[(char, &str)] = &[
        ('a', "àáâãäåæāăąǎ"),
        ('c', "çćč"),
        ('d', "ďð"),
        ('e', "èéêëēėęě"),
        ('g', "ğ"),
        ('i', "ìíîïīįıǐ"),
        ('l', "łľĺ"),
        ('n', "ñńň"),
        ('o', "òóôõöøōőœǒ"),
        ('r', "řŕ"),
        ('s', "ßśšş"),
        ('t', "ťţþ"),
        ('u', "ùúûüūůűųǔǖǘǚǜ"),
        ('y', "ýÿ"),
        ('z', "źżž"),
    ];
    GROUPS.iter().find(|(_, letters)| letters.contains(c)).map_or(c, |(base, _)| *base)
}

/// 選んだ言語の、元の文字 → アクセント付きの文字の一覧。
/// よく使う順 (diacritics_freq.rs の頻度を、選んだ言語で平均した値の大きい順)。頻度が分からない文字は後ろに表の順で
pub fn variants_for(languages: &[String], c: char) -> Vec<char> {
    let upper = c.is_uppercase();
    let lower = c.to_lowercase().next().unwrap_or(c);
    let selected: Vec<&str> = LANGUAGES.iter().map(|(code, _)| *code).filter(|code| languages.iter().any(|l| l == code)).collect();
    let mut out: Vec<char> = vec![];
    for (code, letters) in LANGUAGES {
        if !selected.contains(code) {
            continue;
        }
        let frequent = FREQUENCIES.iter().find(|(c, _)| c == code).map(|(_, list)| list.iter().map(|(ch, _)| *ch).collect::<Vec<_>>()).unwrap_or_default();
        for v in letters.chars().chain(frequent) {
            if base_of(v) == lower && !out.contains(&v) {
                out.push(v);
            }
        }
    }
    let score = |v: char| -> f32 {
        let total: f32 = selected
            .iter()
            .filter_map(|code| FREQUENCIES.iter().find(|(c, _)| c == code))
            .filter_map(|(_, list)| list.iter().find(|(ch, _)| *ch == v).map(|(_, f)| *f))
            .sum();
        total / selected.len().max(1) as f32
    };
    // 安定な並べ替えなので、同じ点数 (頻度が分からない文字の 0) は表の順のまま
    out.sort_by(|a, b| score(*b).partial_cmp(&score(*a)).unwrap_or(std::cmp::Ordering::Equal));
    if upper {
        // 大文字が無い文字 (ß・ı) は除く
        out.into_iter()
            .filter_map(|v| {
                let mut up = v.to_uppercase();
                let u = up.next()?;
                (up.next().is_none() && u != v).then_some(u)
            })
            .collect()
    } else {
        out
    }
}

pub fn languages() -> &'static [String] {
    static LANGUAGES: OnceLock<Vec<String>> = OnceLock::new();
    LANGUAGES.get_or_init(|| shared::AppConfig::read_or_default().input.latin_languages)
}

pub fn variants(c: char) -> Vec<char> {
    #[cfg(test)]
    if let Some((languages, _)) = SETTINGS_FOR_TESTS.with(|settings| settings.borrow().clone()) {
        return variants_for(&languages, c);
    }
    variants_for(languages(), c)
}

/// 日本語入力 (あ) のときも長押しを使うか
pub fn in_kana() -> bool {
    #[cfg(test)]
    if let Some((_, in_kana)) = SETTINGS_FOR_TESTS.with(|settings| settings.borrow().clone()) {
        return in_kana;
    }
    static IN_KANA: OnceLock<bool> = OnceLock::new();
    *IN_KANA.get_or_init(|| shared::AppConfig::read_or_default().input.accents_in_kana)
}

pub fn enabled() -> bool {
    #[cfg(test)]
    if let Some((languages, _)) = SETTINGS_FOR_TESTS.with(|settings| settings.borrow().clone()) {
        return !languages.is_empty();
    }
    !languages().is_empty()
}

/// 試験で使う設定 (言語, かなでも長押し)。そのスレッドだけ
#[cfg(test)]
thread_local! {
    pub(crate) static SETTINGS_FOR_TESTS: std::cell::RefCell<Option<(Vec<String>, bool)>> = const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// いま処理しているキーが押しっぱなしの連打か (note_key で決める)
    static REPEAT: Cell<bool> = const { Cell::new(false) };
    static CONSUMED_HOLD: Cell<Option<char>> = const { Cell::new(None) };
    /// 2026-10-29: IME が最初の押下 (WM_KEYDOWN の lParam の 30 ビット目が 0) を見て、まだ離していないキー (仮想キーコード)
    static HELD: Cell<Option<usize>> = const { Cell::new(None) };
    /// 同じキー入力で OnTestKeyDown と OnKeyDown の 2 回呼ばれるので、いま判定したキー入力 (キー, lParam, メッセージの時刻) を覚える
    static LAST_EVENT: Cell<Option<(usize, isize, i32)>> = const { Cell::new(None) };
    /// 2026-10-29: 最後に押した文字のキーを、IME が受けずに字のままアプリへ渡したか (渡した字の小文字)
    static PASSED: Cell<Option<char>> = const { Cell::new(None) };
}

/// キーが押された (OnTestKeyDown・OnKeyDown)。押しっぱなしの連打かを決める。
/// 2026-10-29: 連打と数えるのは、lParam の 30 ビット目 (直前も押されていた = Windows のオートリピート) が立っていて、しかも IME 自身がそのキーの
/// 最初の押下を見て、まだ離していないときだけ。前は 30 ビット目だけを見ていたので、最初の押下を見ていないキー・押し続けていないのに 30 ビット目が
/// 立ったキーでも、打ち始めの字がアクセントになった (10-29 の不具合の報告)
pub fn note_key(vkey: usize, lparam: isize) {
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageExtraInfo, GetMessageTime};
    let (time, extra) = unsafe { (GetMessageTime(), GetMessageExtraInfo().0 as usize) };
    note_key_at(vkey, lparam, time, extra);
}

/// 2026-10-29: IME が SendInput で打ち直したキー (composition.rs の send_retype) の印 (dwExtraInfo)。
/// 打ち直すのは利用者がまだそのキーを押している間なので、Windows は 30 ビット目 (直前も押されていた) を立てて届ける。
/// 印のあるキーは新しい押下として数える (前は連打と数え、端末で入力待ちでなくなったあとの最初の母音・n が、BackSpace と á になった。10-29 の不具合の報告)
pub const RETYPED_KEY_MARKER: usize = 0x415a4b52;

pub(crate) fn note_key_at(vkey: usize, lparam: isize, time: i32, extra: usize) {
    let lparam = if extra == RETYPED_KEY_MARKER { lparam & !(1 << 30) } else { lparam };
    let event = (vkey, lparam, time);
    if LAST_EVENT.get() == Some(event) {
        return;
    }
    LAST_EVENT.set(Some(event));
    let os_repeat = (lparam >> 30) & 1 == 1;
    REPEAT.set(os_repeat && HELD.get() == Some(vkey));
    if !os_repeat {
        HELD.set(Some(vkey));
    }
}

/// OnTestKeyDown の答え: このキーを IME が受けたか (eaten)。文字のキーを受けずにアプリへ渡したら、その字を覚える (連打のときは最初の押下のまま)
pub fn note_passed(vkey: usize, eaten: bool) {
    if REPEAT.get() {
        return;
    }
    let letter = (0x41..=0x5a).contains(&vkey);
    PASSED.set((letter && !eaten).then(|| (vkey as u8 as char).to_ascii_lowercase()));
}

/// 2026-10-29: 最後に押した文字のキー (c) を、字のままアプリへ渡していたか (入力中でないときの長押しで、アプリの字をアクセント付きに置き換えてよいか)
pub fn passed_to_app(c: char) -> bool {
    PASSED.get() == Some(c.to_ascii_lowercase())
}

pub fn is_repeat() -> bool {
    REPEAT.get()
}

pub fn consume_hold(base: char) {
    CONSUMED_HOLD.set(Some(base.to_ascii_lowercase()));
}

pub fn consumed_repeat(c: char) -> bool {
    is_repeat() && CONSUMED_HOLD.get() == Some(c.to_ascii_lowercase())
}

pub fn note_key_up(vkey: usize) {
    if HELD.get() == Some(vkey) {
        HELD.set(None);
    }
    if (0x41..=0x5a).contains(&vkey) {
        let base = (vkey as u8 as char).to_ascii_lowercase();
        if CONSUMED_HOLD.get() == Some(base) { CONSUMED_HOLD.set(None); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOWN: isize = 0;
    const REPEATED: isize = 1 << 30;

    #[test]
    fn selected_accent_swallows_only_repeats_until_its_key_is_released() {
        consume_hold('N');
        note_key_at(0x4e, DOWN, 1, 0);
        assert!(!consumed_repeat('n'));
        note_key_at(0x4e, REPEATED, 2, 0);
        assert!(consumed_repeat('n'));
        assert!(consumed_repeat('N'));
        assert!(!consumed_repeat('a'));
        note_key_up(0x41);
        note_key_up(0x31);
        assert!(consumed_repeat('n'));
        note_key_up(0x4e);
        assert!(!consumed_repeat('n'));
        note_key_at(0x4e, DOWN, 3, 0);
    }

    /// 2026-10-29: 連打と数えるのは、30 ビット目が立っていて、IME がそのキーの最初の押下を見て、まだ離していないときだけ。
    /// 同じキー入力で 2 回呼ばれても (OnTestKeyDown と OnKeyDown) 答えは同じ
    #[test]
    fn a_repeat_counts_only_after_the_ime_saw_the_key_go_down() {
        // ふつうの長押し: 押す → 連打 (2 回目からは長押し)。離したら終わり
        note_key_at(0x41, DOWN, 100, 0);
        assert!(!is_repeat());
        note_key_at(0x41, DOWN, 100, 0);
        assert!(!is_repeat());
        note_key_at(0x41, REPEATED, 600, 0);
        assert!(is_repeat());
        note_key_at(0x41, REPEATED, 600, 0);
        assert!(is_repeat());
        note_key_at(0x41, REPEATED, 633, 0);
        assert!(is_repeat());
        note_key_up(0x41);
        note_key_at(0x41, REPEATED, 700, 0);
        assert!(!is_repeat(), "a repeat after the key-up");
        // 最初の押下を見ていないキー: 30 ビット目が立っていても、何度来ても長押しにしない
        note_key_up(0x41);
        note_key_at(0x45, REPEATED, 800, 0);
        assert!(!is_repeat());
        note_key_at(0x45, REPEATED, 833, 0);
        assert!(!is_repeat());
        // 押している間にほかのキーを押したら、前のキーの連打は数えない
        note_key_at(0x4f, DOWN, 900, 0);
        note_key_at(0x41, DOWN, 950, 0);
        note_key_at(0x4f, REPEATED, 1000, 0);
        assert!(!is_repeat());
        // 押し続けていないのに 30 ビット目が立った押下 (2 回呼ばれる) は、長押しにしない
        note_key_up(0x41);
        note_key_up(0x4f);
        note_key_at(0x55, REPEATED, 1100, 0);
        note_key_at(0x55, REPEATED, 1100, 0);
        assert!(!is_repeat());
        // IME が打ち直したキー (利用者がまだ押している間に送るので 30 ビット目が立つ) は、新しい押下として数える
        note_key_up(0x55);
        note_key_at(0x41, DOWN, 1200, 0);
        note_key_at(0x41, REPEATED, 1201, RETYPED_KEY_MARKER);
        assert!(!is_repeat(), "the IME's own retyped key counted as a hold");
        note_key_at(0x41, REPEATED, 1201, RETYPED_KEY_MARKER);
        assert!(!is_repeat());
        note_key_up(0x41);
    }

    /// 2026-10-29: 字のままアプリへ渡した文字のキーを覚える。連打の答えでは変えない。ほかのキー・受けたキーで消える
    #[test]
    fn remembers_letters_passed_to_the_app() {
        note_key_at(0x41, DOWN, 2000, 0);
        note_passed(0x41, false);
        assert!(passed_to_app('a') && passed_to_app('A') && !passed_to_app('e'));
        note_key_at(0x41, REPEATED, 2500, 0);
        note_passed(0x41, true);
        assert!(passed_to_app('a'));
        note_key_up(0x41);
        note_key_at(0x41, DOWN, 2600, 0);
        note_passed(0x41, true);
        assert!(!passed_to_app('a'));
        note_key_at(0x31, DOWN, 2700, 0);
        note_passed(0x31, false);
        assert!(!passed_to_app('a'));
        note_key_up(0x31);
    }

    fn langs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn variants_by_language() {
        assert_eq!(variants_for(&langs(&["es"]), 'n'), vec!['ñ']);
        assert_eq!(variants_for(&langs(&["es", "pl", "cs"]), 'n'), vec!['ñ', 'ń', 'ň']);
        assert_eq!(variants_for(&langs(&["de"]), 's'), vec!['ß']);
        assert_eq!(variants_for(&langs(&["de"]), 'S'), Vec::<char>::new());
        assert_eq!(variants_for(&langs(&["es"]), 'N'), vec!['Ñ']);
        // フランス語: é (1.5%) > è > ê > ë
        assert_eq!(variants_for(&langs(&["fr"]), 'e'), vec!['é', 'è', 'ê', 'ë']);
        // スペイン語: ó > í > á > é
        assert_eq!(variants_for(&langs(&["es"]), 'o'), vec!['ó']);
        // 仏＋独: a は à (仏 0.486 / 2) と ä (独 0.578 / 2) → ä が先
        assert_eq!(variants_for(&langs(&["fr", "de"]), 'a')[0], 'ä');
        // ピンイン: 4声 > 1声 > 2声 > 3声
        assert_eq!(variants_for(&langs(&["pinyin"]), 'a'), vec!['à', 'ā', 'á', 'ǎ']);
        assert!(variants_for(&langs(&[]), 'e').is_empty());
    }
}
