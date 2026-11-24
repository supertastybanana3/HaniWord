// ^ と _ で上付き・下付き (Unicode の文字) にする。x^2 → x²、H_2O → H₂O、10^{-3} → 10⁻³、x^{n+1} → xⁿ⁺¹
//
// ^ / _ はまず記号のまま入れ、次の文字が上付き・下付きにできれば記号と置き換える。
// 数字・符号は続けて打つ間ずっと上付き (10^23 → 10²³)。{ } で囲めば英字も含めてまとめて。
// _ のあとの英字 1 字も下付き (a_n → aₙ、x_i → xᵢ、_nC_r → ₙCᵣ。2026-10-17)。
// ただし英字 2 字以上の語のすぐ後ろの _ は、名前の区切り (file_name・my_var) なので英字を下付きにしない (log_{a} のように { } で囲めば下付き)
// 記号の直前の、母音 1 打で入ったかな (a_n の あ) は英字に戻す (vowel)

use std::sync::OnceLock;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum ScriptState {
    #[default]
    Off,
    /// ^ か _ を記号のまま入れた直後。sup = 上付き。letters = 次の英字も上付き・下付きにする。
    /// vowel = 記号の直前の かな を英字に戻すときの英字 (a_n の あ → a)
    Marker { sup: bool, letters: bool, vowel: Option<char> },
    /// 数字・符号を上付き (下付き) で打っている途中
    Digits(bool),
    /// { } の中
    Group(bool),
}

/// 打った 1 文字をどう入れるか
#[derive(Debug, PartialEq)]
pub enum ScriptOp {
    /// いつもどおり
    Normal,
    /// 記号 (^ _) をそのまま入れる
    Marker(char),
    /// 直前の記号を消してから入れる (空なら消すだけ)。英字があれば、記号の直前の かな 1 字もその英字に戻す
    ReplaceMarker(String, Option<char>),
    /// そのまま (上付き・下付きにした文字を) 入れる
    Insert(String),
    /// 何も入れない ({ } の閉じ)
    Nothing,
    /// 2026-11-01: 直前の 1 字 (上付きにした英字) を消して入れ直す (顔文字の ^o^ を、o を上付きにした ᵒ から ^o^ に戻す)
    Revert(String),
}

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| shared::AppConfig::read_or_default().input.caret_scripts)
}

pub fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹',
        '+' => '⁺', '-' => '⁻', '=' => '⁼', '(' => '⁽', ')' => '⁾',
        'a' => 'ᵃ', 'b' => 'ᵇ', 'c' => 'ᶜ', 'd' => 'ᵈ', 'e' => 'ᵉ', 'f' => 'ᶠ', 'g' => 'ᵍ', 'h' => 'ʰ', 'i' => 'ⁱ', 'j' => 'ʲ',
        'k' => 'ᵏ', 'l' => 'ˡ', 'm' => 'ᵐ', 'n' => 'ⁿ', 'o' => 'ᵒ', 'p' => 'ᵖ', 'r' => 'ʳ', 's' => 'ˢ', 't' => 'ᵗ', 'u' => 'ᵘ',
        'v' => 'ᵛ', 'w' => 'ʷ', 'x' => 'ˣ', 'y' => 'ʸ', 'z' => 'ᶻ', 'T' => 'ᵀ',
        _ => return None,
    })
}

/// 2026-11-01: 顔文字の目になる、上付きにした字の元の字 (ᵒ → o・⁻ → -)。数字は含めない
fn superscript_letter_source(c: char) -> Option<char> {
    ('a'..='z').chain(['-']).find(|&letter| superscript(letter) == Some(c))
}

/// 2026-11-01: 上付きにした字か (superscript の出す字。² ⁻ ⁿ など)
pub fn superscript_char(c: char) -> bool {
    "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ᵃᵇᶜᵈᵉᶠᵍʰⁱʲᵏˡᵐⁿᵒᵖʳˢᵗᵘᵛʷˣʸᶻᵀ".contains(c)
}

pub fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄', '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉',
        '+' => '₊', '-' => '₋', '=' => '₌', '(' => '₍', ')' => '₎',
        'a' => 'ₐ', 'e' => 'ₑ', 'h' => 'ₕ', 'i' => 'ᵢ', 'j' => 'ⱼ', 'k' => 'ₖ', 'l' => 'ₗ', 'm' => 'ₘ', 'n' => 'ₙ', 'o' => 'ₒ',
        'p' => 'ₚ', 'r' => 'ᵣ', 's' => 'ₛ', 't' => 'ₜ', 'u' => 'ᵤ', 'v' => 'ᵥ', 'x' => 'ₓ',
        _ => return None,
    })
}

fn convert(sup: bool, c: char) -> Option<char> {
    if sup { superscript(c) } else { subscript(c) }
}

fn is_number_like(c: char) -> bool {
    c.is_ascii_digit() || c == '+' || c == '-'
}

/// 打った字の前が、英字 2 字以上の語 (file・my。数字が混ざってもよい) で終わっているか
fn ends_with_word(before: &str) -> bool {
    let run: Vec<char> = before.chars().rev().take_while(|c| c.is_ascii_alphanumeric()).collect();
    run.iter().filter(|c| c.is_ascii_alphabetic()).count() >= 2
}

/// 1 文字進める。before = いま入っている文字 (打った字の前)、vowel = before の最後の かな を英字に戻すときの英字
pub fn step(state: &mut ScriptState, c: char, before: &str, vowel: Option<char>) -> ScriptOp {
    match *state {
        ScriptState::Off => {
            if c == '^' || c == '_' {
                // 2026-11-01: 顔文字 (^^・^_^・^.^・^-^) の記号は字のまま入れ、上付き・下付きを始めない。前は ^^ のあとの字が上付きになっていた
                // (よろしく^^mata → ^ᵐata)。すぐ前が ^ か _、または 2 つ前が ^ のとき。数式の ^ は 1 つずつ打つ (x^2) ので当たらない
                let mut back = before.chars().rev();
                let (last, second) = (back.next(), back.next());
                if matches!(last, Some('^' | '_')) || second == Some('^') {
                    return ScriptOp::Insert(c.to_string());
                }
                // ^o^・^w^: 記号のすぐ後ろの英字が上付きになっていて (その前は英数字でない)、閉じの ^ を打ったら、^ と英字に戻す。
                // 数式 (x^n の続き) は前が英数字なので当たらない
                if c == '^' {
                    if let Some(letter) = last.and_then(superscript_letter_source) {
                        if !second.is_some_and(|s| s.is_ascii_alphanumeric()) {
                            return ScriptOp::Revert(format!("^{letter}^"));
                        }
                    }
                }
                let sup = c == '^';
                *state = ScriptState::Marker { sup, letters: sup || !ends_with_word(before), vowel };
                ScriptOp::Marker(c)
            } else {
                ScriptOp::Normal
            }
        }
        ScriptState::Marker { sup, letters, vowel } => {
            if c == '{' {
                *state = ScriptState::Group(sup);
                return ScriptOp::ReplaceMarker(String::new(), vowel);
            }
            let allowed = letters || is_number_like(c);
            match convert(sup, c).filter(|_| allowed) {
                Some(s) => {
                    *state = if is_number_like(c) { ScriptState::Digits(sup) } else { ScriptState::Off };
                    ScriptOp::ReplaceMarker(s.to_string(), vowel)
                }
                None => {
                    *state = ScriptState::Off;
                    // ^^ や _^ は、次の記号として扱い直す
                    step(state, c, before, None)
                }
            }
        }
        ScriptState::Digits(sup) => {
            if c.is_ascii_digit() {
                ScriptOp::Insert(convert(sup, c).unwrap().to_string())
            } else {
                *state = ScriptState::Off;
                step(state, c, before, None)
            }
        }
        ScriptState::Group(sup) => {
            if c == '}' {
                *state = ScriptState::Off;
                return ScriptOp::Nothing;
            }
            match convert(sup, c) {
                Some(s) => ScriptOp::Insert(s.to_string()),
                None => ScriptOp::Insert(c.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-01: superscript が出す字は、どれも superscript_char に入っている (英語の判定が sin²x の ² を上付きの字として見るため)
    #[test]
    fn every_superscript_output_is_a_superscript_char() {
        for c in "0123456789+-=()abcdefghijklmnopqrstuvwxyzT".chars() {
            if let Some(s) = superscript(c) { assert!(superscript_char(s), "{c} → {s}"); }
        }
        assert!(!superscript_char('2') && !superscript_char('a'));
    }

    /// 打った列を、記号の置き換えまで再現して文字列にする (start = 前から入っている文字)
    fn typed_after(start: &str, vowel: Option<char>, s: &str) -> String {
        let mut state = ScriptState::Off;
        let mut out = start.to_string();
        for c in s.chars() {
            let v = if out == start { vowel } else { None };
            match step(&mut state, c, &out.clone(), v) {
                ScriptOp::Normal => out.push(c),
                ScriptOp::Marker(m) => out.push(m),
                ScriptOp::ReplaceMarker(t, v) => {
                    out.pop();
                    if let Some(v) = v {
                        out.pop();
                        out.push(v);
                    }
                    out.push_str(&t);
                }
                ScriptOp::Insert(t) => out.push_str(&t),
                ScriptOp::Nothing => {}
                ScriptOp::Revert(t) => {
                    out.pop();
                    out.push_str(&t);
                }
            }
        }
        out
    }

    fn typed(s: &str) -> String {
        typed_after("", None, s)
    }

    #[test]
    fn scripts() {
        assert_eq!(typed("x^2"), "x²");
        assert_eq!(typed("10^23"), "10²³");
        assert_eq!(typed("10^-3"), "10⁻³");
        assert_eq!(typed("x^{n+1}"), "xⁿ⁺¹");
        assert_eq!(typed("H_2O"), "H₂O");
        assert_eq!(typed("SO_4^{2-}"), "SO₄²⁻");
        assert_eq!(typed("file_name"), "file_name");
        assert_eq!(typed("a_{ij}"), "aᵢⱼ");
        assert_eq!(typed("^^"), "^^");
        assert_eq!(typed("x^2y"), "x²y");
        assert_eq!(typed("A^T"), "Aᵀ");
    }

    /// 2026-11-01: 顔文字の ^ _ のあとは上付き・下付きにしない (よろしく^^mata が ^ᵐata になっていた)。数式は今までどおり
    #[test]
    fn emoticons_do_not_start_scripts() {
        assert_eq!(typed_after("よろしく", None, "^^mata"), "よろしく^^mata");
        assert_eq!(typed_after("よろしく", None, "^_^mata"), "よろしく^_^mata");
        assert_eq!(typed_after("よろしく", None, "^o^mata"), "よろしく^o^mata");
        assert_eq!(typed_after("よろしく", None, "^w^"), "よろしく^w^");
        assert_eq!(typed_after("よろしく", None, "^.^mata"), "よろしく^.^mata");
        assert_eq!(typed_after("よろしく", None, "^-^mata"), "よろしく^-^mata");
        assert_eq!(typed("x^n"), "xⁿ");
        assert_eq!(typed("e^x"), "eˣ");
        assert_eq!(typed("x^2+y^2"), "x²+y²");
        assert_eq!(typed("e^{-x}"), "e⁻ˣ");
        assert_eq!(typed("a^b^c"), "aᵇᶜ");
    }

    #[test]
    fn single_letter_subscripts() {
        assert_eq!(typed("x_n"), "xₙ");
        assert_eq!(typed("S_n"), "Sₙ");
        assert_eq!(typed("_nC_r"), "ₙCᵣ");
        assert_eq!(typed("x_i+x_j"), "xᵢ+xⱼ");
        assert_eq!(typed("2x_k"), "2xₖ");
        // 下付きの字が Unicode に無い英字はそのまま
        assert_eq!(typed("x_b"), "x_b");
        assert_eq!(typed("x_y"), "x_y");
        // 英字 2 字以上の語の後ろは名前の区切り ({ } で囲めば下付き)
        assert_eq!(typed("file_name"), "file_name");
        assert_eq!(typed("my_var"), "my_var");
        assert_eq!(typed("var1_n"), "var1_n");
        assert_eq!(typed("log_a"), "log_a");
        assert_eq!(typed("log_{a}"), "logₐ");
        assert_eq!(typed("log_2"), "log₂");
        // 記号の直前の、母音 1 打で入ったかなは英字に戻す
        assert_eq!(typed_after("すうれつあ", Some('a'), "_n"), "すうれつaₙ");
        assert_eq!(typed_after("あ", Some('a'), "^2"), "a²");
        assert_eq!(typed_after("え", Some('e'), "^{x}"), "eˣ");
        // 下付き・上付きにならなければ戻さない (顔文字)
        assert_eq!(typed_after("おはよう", Some('u'), "^_^"), "おはよう^_^");
    }
}
