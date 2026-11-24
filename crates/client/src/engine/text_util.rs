// thanks to https://github.com/ensan-hcl/AzooKeyKanaKanjiConverter/blob/develop/Sources/KanaKanjiConverterModule/Roman2Kana.swift
use std::collections::HashMap;
use std::sync::LazyLock;

use super::full_width::to_halfwidth;

#[allow(dead_code)]
static KANA_MAP: LazyLock<HashMap<&'static str, (&'static str, &'static str)>> =
    LazyLock::new(|| {
        HashMap::from([
            ("あ", ("ア", "ｱ")),
            ("ぁ", ("ァ", "ｧ")),
            ("い", ("イ", "ｲ")),
            ("ぃ", ("ィ", "ｨ")),
            ("う", ("ウ", "ｳ")),
            ("ゔ", ("ヴ", "ｳﾞ")),
            ("ぅ", ("ゥ", "ｩ")),
            ("え", ("エ", "ｴ")),
            ("ぇ", ("ェ", "ｪ")),
            ("お", ("オ", "ｵ")),
            ("ぉ", ("ォ", "ｫ")),
            ("か", ("カ", "ｶ")),
            ("が", ("ガ", "ｶﾞ")),
            ("か゚", ("カ゚", "ｶﾟ")),
            ("ゕ", ("ヵ", "ｶ")),
            ("き", ("キ", "ｷ")),
            ("ぎ", ("ギ", "ｷﾞ")),
            ("き゚", ("キ゚", "ｷﾟ")),
            ("く", ("ク", "ｸ")),
            ("ぐ", ("グ", "ｸﾞ")),
            ("く゚", ("ク゚", "ｸﾟ")),
            ("け", ("ケ", "ｹ")),
            ("げ", ("ゲ", "ｹﾞ")),
            ("け゚", ("ケ゚", "ｹﾟ")),
            ("ゖ", ("ヶ", "ｹ")),
            ("こ", ("コ", "ｺ")),
            ("ご", ("ゴ", "ｺﾞ")),
            ("こ゚", ("コ゚", "ｺﾟ")),
            ("さ", ("サ", "ｻ")),
            ("ざ", ("ザ", "ｻﾞ")),
            ("さ゚", ("サ゚", "ｻﾟ")),
            ("し", ("シ", "ｼ")),
            ("じ", ("ジ", "ｼﾞ")),
            ("し゚", ("シ゚", "ｼﾟ")),
            ("す", ("ス", "ｽ")),
            ("ず", ("ズ", "ｽﾞ")),
            ("す゚", ("ス゚", "ｽﾟ")),
            ("せ", ("セ", "ｾ")),
            ("ぜ", ("ゼ", "ｾﾞ")),
            ("せ゚", ("セ゚", "ｾﾟ")),
            ("そ", ("ソ", "ｿ")),
            ("ぞ", ("ゾ", "ｿﾞ")),
            ("そ゚", ("ソ゚", "ｿﾟ")),
            ("た", ("タ", "ﾀ")),
            ("だ", ("ダ", "ﾀﾞ")),
            ("た゚", ("タ゚", "ﾀﾟ")),
            ("ち", ("チ", "ﾁ")),
            ("ぢ", ("ヂ", "ﾁﾞ")),
            ("ち゚", ("チ゚", "ﾁﾟ")),
            ("つ", ("ツ", "ﾂ")),
            ("づ", ("ヅ", "ﾂﾞ")),
            ("つ゚", ("ツ゚", "ﾂﾟ")),
            ("っ", ("ッ", "ｯ")),
            ("て", ("テ", "ﾃ")),
            ("で", ("デ", "ﾃﾞ")),
            ("て゚", ("テ゚", "ﾃﾟ")),
            ("と", ("ト", "ﾄ")),
            ("ど", ("ド", "ﾄﾞ")),
            ("と゚", ("ト゚", "ﾄﾟ")),
            ("な", ("ナ", "ﾅ")),
            ("に", ("ニ", "ﾆ")),
            ("ぬ", ("ヌ", "ﾇ")),
            ("ね", ("ネ", "ﾈ")),
            ("の", ("ノ", "ﾉ")),
            ("は", ("ハ", "ﾊ")),
            ("ば", ("バ", "ﾊﾞ")),
            ("ぱ", ("パ", "ﾊﾟ")),
            ("ひ", ("ヒ", "ﾋ")),
            ("び", ("ビ", "ﾋﾞ")),
            ("ぴ", ("ピ", "ﾋﾟ")),
            ("ふ", ("フ", "ﾌ")),
            ("ぶ", ("ブ", "ﾌﾞ")),
            ("ぷ", ("プ", "ﾌﾟ")),
            ("へ", ("ヘ", "ﾍ")),
            ("べ", ("ベ", "ﾍﾞ")),
            ("ぺ", ("ペ", "ﾍﾟ")),
            ("ほ", ("ホ", "ﾎ")),
            ("ぼ", ("ボ", "ﾎﾞ")),
            ("ぽ", ("ポ", "ﾎﾟ")),
            ("ま", ("マ", "ﾏ")),
            ("み", ("ミ", "ﾐ")),
            ("む", ("ム", "ﾑ")),
            ("め", ("メ", "ﾒ")),
            ("も", ("モ", "ﾓ")),
            ("や", ("ヤ", "ﾔ")),
            ("ゃ", ("ャ", "ｬ")),
            ("ゆ", ("ユ", "ﾕ")),
            ("ゅ", ("ュ", "ｭ")),
            ("よ", ("ヨ", "ﾖ")),
            ("ょ", ("ョ", "ｮ")),
            ("ら", ("ラ", "ﾗ")),
            ("ら゚", ("ラ゚", "ﾗﾟ")),
            ("り", ("リ", "ﾘ")),
            ("り゚", ("リ゚", "ﾘﾟ")),
            ("る", ("ル", "ﾙ")),
            ("る゚", ("ル゚", "ﾙﾟ")),
            ("れ", ("レ", "ﾚ")),
            ("れ゚", ("レ゚", "ﾚﾟ")),
            ("ろ", ("ロ", "ﾛ")),
            ("ろ゚", ("ロ゚", "ﾛﾟ")),
            ("わ", ("ワ", "ﾜ")),
            ("ゎ", ("ヮ", "ﾜ")),
            ("ゐ", ("ヰ", "ｲ")),
            ("ゑ", ("ヱ", "ｴ")),
            ("を", ("ヲ", "ｦ")),
            ("ん", ("ン", "ﾝ")),
        ])
    });

pub fn to_katakana(s: &str) -> String {
    let mut result = String::new();
    let mut iter = s.chars().peekable();

    while let Some(c) = iter.next() {
        if let Some(&(katakana, _)) = KANA_MAP.get(&c.to_string().as_str()) {
            result.push_str(katakana);
        } else {
            result.push(c);
        }
    }

    result
}

pub fn to_half_katakana(s: &str) -> String {
    let mut result = String::new();
    let mut iter = s.chars().peekable();

    while let Some(c) = iter.next() {
        let c = to_halfwidth(&c.to_string()).chars().next().unwrap();

        if let Some(&(_, hankaku_katakana)) = KANA_MAP.get(&c.to_string().as_str()) {
            result.push_str(hankaku_katakana);
        } else {
            result.push(c);
        }
    }

    result
}

/// かなの読みをローマ字に戻す (英字に直すとき用。読みに混ざった英字はそのまま)。
/// 音ごとに `spelling` の綴り (訓令式 si・tu・hu か ヘボン式 shi・tsu・fu か、ん を nn か n か) で書く
pub fn to_romaji(reading: &str, spelling: &super::romaji_style::Spelling) -> String {
    const YOON: &[(&str, &str, &str)] = &[
        ("きゃ", "kya", "kya"), ("きゅ", "kyu", "kyu"), ("きょ", "kyo", "kyo"),
        ("しゃ", "sya", "sha"), ("しゅ", "syu", "shu"), ("しょ", "syo", "sho"), ("しぇ", "sye", "she"),
        ("ちゃ", "tya", "cha"), ("ちゅ", "tyu", "chu"), ("ちょ", "tyo", "cho"), ("ちぇ", "tye", "che"),
        ("にゃ", "nya", "nya"), ("にゅ", "nyu", "nyu"), ("にょ", "nyo", "nyo"),
        ("ひゃ", "hya", "hya"), ("ひゅ", "hyu", "hyu"), ("ひょ", "hyo", "hyo"),
        ("みゃ", "mya", "mya"), ("みゅ", "myu", "myu"), ("みょ", "myo", "myo"),
        ("りゃ", "rya", "rya"), ("りゅ", "ryu", "ryu"), ("りょ", "ryo", "ryo"),
        ("ぎゃ", "gya", "gya"), ("ぎゅ", "gyu", "gyu"), ("ぎょ", "gyo", "gyo"),
        ("じゃ", "zya", "ja"), ("じゅ", "zyu", "ju"), ("じょ", "zyo", "jo"), ("じぇ", "zye", "je"),
        ("びゃ", "bya", "bya"), ("びゅ", "byu", "byu"), ("びょ", "byo", "byo"),
        ("ぴゃ", "pya", "pya"), ("ぴゅ", "pyu", "pyu"), ("ぴょ", "pyo", "pyo"),
        ("ふぁ", "fa", "fa"), ("ふぃ", "fi", "fi"), ("ふぇ", "fe", "fe"), ("ふぉ", "fo", "fo"),
        ("てぃ", "thi", "thi"), ("でぃ", "dhi", "dhi"), ("てゅ", "thu", "thu"), ("でゅ", "dhu", "dhu"),
        ("うぃ", "wi", "wi"), ("うぇ", "we", "we"), ("ゔぁ", "va", "va"), ("ゔぃ", "vi", "vi"), ("ゔぇ", "ve", "ve"), ("ゔぉ", "vo", "vo"),
    ];
    const SINGLE: &[(char, &str, &str)] = &[
        ('あ', "a", "a"), ('い', "i", "i"), ('う', "u", "u"), ('え', "e", "e"), ('お', "o", "o"),
        ('か', "ka", "ka"), ('き', "ki", "ki"), ('く', "ku", "ku"), ('け', "ke", "ke"), ('こ', "ko", "ko"),
        ('さ', "sa", "sa"), ('し', "si", "shi"), ('す', "su", "su"), ('せ', "se", "se"), ('そ', "so", "so"),
        ('た', "ta", "ta"), ('ち', "ti", "chi"), ('つ', "tu", "tsu"), ('て', "te", "te"), ('と', "to", "to"),
        ('な', "na", "na"), ('に', "ni", "ni"), ('ぬ', "nu", "nu"), ('ね', "ne", "ne"), ('の', "no", "no"),
        ('は', "ha", "ha"), ('ひ', "hi", "hi"), ('ふ', "hu", "fu"), ('へ', "he", "he"), ('ほ', "ho", "ho"),
        ('ま', "ma", "ma"), ('み', "mi", "mi"), ('む', "mu", "mu"), ('め', "me", "me"), ('も', "mo", "mo"),
        ('や', "ya", "ya"), ('ゆ', "yu", "yu"), ('よ', "yo", "yo"),
        ('ら', "ra", "ra"), ('り', "ri", "ri"), ('る', "ru", "ru"), ('れ', "re", "re"), ('ろ', "ro", "ro"),
        ('わ', "wa", "wa"), ('を', "wo", "wo"),
        ('が', "ga", "ga"), ('ぎ', "gi", "gi"), ('ぐ', "gu", "gu"), ('げ', "ge", "ge"), ('ご', "go", "go"),
        ('ざ', "za", "za"), ('じ', "zi", "ji"), ('ず', "zu", "zu"), ('ぜ', "ze", "ze"), ('ぞ', "zo", "zo"),
        ('だ', "da", "da"), ('ぢ', "di", "di"), ('づ', "du", "du"), ('で', "de", "de"), ('ど', "do", "do"),
        ('ば', "ba", "ba"), ('び', "bi", "bi"), ('ぶ', "bu", "bu"), ('べ', "be", "be"), ('ぼ', "bo", "bo"),
        ('ぱ', "pa", "pa"), ('ぴ', "pi", "pi"), ('ぷ', "pu", "pu"), ('ぺ', "pe", "pe"), ('ぽ', "po", "po"),
        ('ゔ', "vu", "vu"),
        ('ぁ', "xa", "xa"), ('ぃ', "xi", "xi"), ('ぅ', "xu", "xu"), ('ぇ', "xe", "xe"), ('ぉ', "xo", "xo"),
        ('ゃ', "xya", "xya"), ('ゅ', "xyu", "xyu"), ('ょ', "xyo", "xyo"), ('ゎ', "xwa", "xwa"),
        ('ー', "-", "-"), ('、', ",", ","), ('。', ".", "."),
    ];
    // かなの音 → 表示を選ぶ音 (romaji_style::DISPLAY_GROUPS)
    let group_of = |kana: &str| -> &'static str {
        match kana {
            "し" => "si", "ち" => "ti", "つ" => "tu", "じ" => "zi", "ふ" => "hu",
            "しゃ" | "しゅ" | "しょ" => "sya", "しぇ" => "sye", "ちゃ" | "ちゅ" | "ちょ" => "tya", "ちぇ" => "tye",
            "じゃ" | "じゅ" | "じょ" => "zya", "じぇ" => "zye",
            _ => "",
        }
    };
    let pick = |kana: &str, k: &'static str, h: &'static str| if spelling.is_kunrei(group_of(kana)) { k } else { h };
    let chars: Vec<char> = reading.chars().collect();
    let mut out = String::new();
    let mut sokuon = false;
    let mut i = 0;
    while i < chars.len() {
        let mut piece: Option<&str> = None;
        let mut used = 1;
        if i + 1 < chars.len() {
            let pair: String = chars[i..i + 2].iter().collect();
            if let Some((_, k, h)) = YOON.iter().find(|(kana, _, _)| *kana == pair) {
                piece = Some(pick(&pair, k, h));
                used = 2;
            }
        }
        if piece.is_none() {
            let c = chars[i];
            if c == 'っ' {
                sokuon = true;
                i += 1;
                continue;
            }
            if c == 'ん' {
                // n のあとに母音・y が来るときは nn にしないと読み違える
                let next_vowel = chars.get(i + 1).is_some_and(|n| "あいうえおやゆよ".contains(*n));
                piece = Some(if spelling.nn || next_vowel { "nn" } else { "n" });
            } else if let Some((_, k, h)) = SINGLE.iter().find(|(kana, _, _)| *kana == c) {
                piece = Some(pick(&c.to_string(), k, h));
            }
        }
        match piece {
            Some(p) => {
                if sokuon {
                    // っ は次の子音を重ねる (っち は tti / tchi)
                    let first = p.chars().next().unwrap_or('x');
                    if first.is_ascii_alphabetic() && !"aiueo".contains(first) {
                        out.push(if p.starts_with("ch") { 't' } else { first });
                    } else {
                        out.push_str("xtu");
                    }
                    sokuon = false;
                }
                out.push_str(p);
            }
            None => {
                if sokuon {
                    out.push_str("xtu");
                    sokuon = false;
                }
                out.push(chars[i]);
            }
        }
        i += used;
    }
    if sokuon {
        out.push_str("xtu");
    }
    out
}

#[cfg(test)]
mod romaji_tests {
    use super::to_romaji;

    #[test]
    fn back_to_romaji() {
        use crate::engine::romaji_style::Spelling;
        assert_eq!(to_romaji("けltype", &Spelling::uniform(true)), "keltype");
        assert_eq!(to_romaji("きょうはしんぶん", &Spelling::uniform(true)), "kyouhasinnbunn");
        assert_eq!(to_romaji("きょうはしんぶん", &Spelling::uniform(false)), "kyouhashinbun");
        assert_eq!(to_romaji("まっちゃ", &Spelling::uniform(false)), "matcha");
        assert_eq!(to_romaji("きって", &Spelling::uniform(true)), "kitte");
        assert_eq!(to_romaji("こんや", &Spelling::uniform(false)), "konnya");
    }
}
