//! 2026-11-05 (仕事 24): ラテン文字のつづりの指摘・自動修正の測り。判断は IME と同じ crates/client/src/engine/latin_spell.rs を読み込む。
//! 表は配っている次の語の候補の表 (next_<言語>.hwn)、直さない語は english_names・english_brands・english_case (リポジトリの頭)。
//! 文の組 (1 行 1 文) ごとに 3 つを数える:
//!   正しい文: 変えた語の数 (指摘・自動修正)
//!   1 字違い: 1 文に 1 か所、機械で 1 字違い (隣のキーの置き換え・抜け・余り・入れ替え。どの表にも無い綴りになるものだけ) を入れ、直った・違う形に直した・直さなかった
//!   アクセント: アクセントのある語を 1 つ選んでアクセントを外し、同じように数える
//! 文は Space で区切った塊ごとに、それより前の文を before にして確かめる (IME が Space で確定するときと同じ)。
//! 行の番号が偶数の文を dev、奇数の文を test にする。
//! 使い方: latin_spell_eval --tables <next_*.hwn のフォルダ> --words <english_*.txt のフォルダ> --data <言語.txt のフォルダ>
//!         --langs en,es,... --split dev|test [--seed N] [--limit N] [--show N] [--th 名前=値 ...]
#[path = "../../../crates/client/src/engine/latin_spell.rs"]
#[allow(dead_code)]
mod latin_spell;

use latin_spell::{base_char, check_text_with, suggestion_with, Capitals, Kind, Model, Thresholds};
use std::collections::HashSet;

const NEVER: u16 = u16::MAX;

/// 次の語の候補の表 1 つ (next_word.rs の Table と同じ読み方)
struct Hwn {
    data: Vec<u8>,
    words: usize,
    turkish: bool,
    word_index: usize,
    word_blob: usize,
    display_index: usize,
    display_blob: usize,
    uni: usize,
    mult: usize,
    pair_start: usize,
    pair_data: usize,
    alphabet: Vec<char>,
    /// アクセントを外した形 → 語の番号
    accents: std::collections::HashMap<String, Vec<usize>>,
}

fn u16_at(d: &[u8], at: usize) -> u16 { u16::from_le_bytes([d[at], d[at + 1]]) }
fn u32_at(d: &[u8], at: usize) -> u32 { u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]) }

impl Hwn {
    fn load(path: &std::path::Path) -> Hwn {
        let data = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(&data[0..4], b"HWNX");
        let words = u32_at(&data, 16) as usize;
        let flags = u32_at(&data, 24);
        let o = |i: usize| u32_at(&data, 28 + 4 * i) as usize;
        let mut t = Hwn { words, turkish: flags & 1 != 0, word_index: o(0), word_blob: o(1), display_index: o(2), display_blob: o(3),
                          uni: o(4), mult: o(5), pair_start: o(6), pair_data: o(7), alphabet: vec![], data: vec![], accents: Default::default() };
        t.data = data;
        let mut chars: HashSet<char> = HashSet::new();
        for i in 0..t.words {
            if let Ok(w) = std::str::from_utf8(t.word(i)) {
                chars.extend(w.chars().filter(|c| c.is_alphabetic() || *c == '\''));
            }
        }
        let mut alphabet: Vec<char> = chars.into_iter().collect();
        alphabet.sort();
        t.alphabet = alphabet;
        for i in 0..t.words {
            if let Ok(w) = std::str::from_utf8(t.word(i)) {
                let base = latin_spell::strip_accents(w);
                if base != w { t.accents.entry(base).or_default().push(i); }
            }
        }
        t
    }
    fn word(&self, i: usize) -> &[u8] {
        let s = u32_at(&self.data, self.word_index + 4 * i) as usize;
        let e = u32_at(&self.data, self.word_index + 4 * i + 4) as usize;
        &self.data[self.word_blob + s..self.word_blob + e]
    }
    fn lower_bound(&self, key: &[u8]) -> usize {
        let (mut lo, mut hi) = (0, self.words);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.word(mid) < key { lo = mid + 1 } else { hi = mid }
        }
        lo
    }
    fn find(&self, key: &[u8]) -> Option<usize> {
        let i = self.lower_bound(key);
        (i < self.words && self.word(i) == key).then_some(i)
    }
    fn display(&self, i: usize) -> String {
        let at = u32_at(&self.data, self.display_index + 4 * i);
        if at == u32::MAX {
            return String::from_utf8_lossy(self.word(i)).into_owned();
        }
        let at = self.display_blob + at as usize;
        let len = self.data[at] as usize;
        String::from_utf8_lossy(&self.data[at + 1..at + 1 + len]).into_owned()
    }
    fn pair(&self, w1: usize, target: usize) -> Option<u16> {
        let start = u32_at(&self.data, self.pair_start + 4 * w1) as usize;
        let end = u32_at(&self.data, self.pair_start + 4 * w1 + 4) as usize;
        let (mut pos, end) = (self.pair_data + start, self.pair_data + end);
        let mut id = 0usize;
        while pos < end {
            let (mut delta, mut shift) = (0usize, 0);
            loop {
                let b = self.data[pos];
                pos += 1;
                delta |= ((b & 0x7F) as usize) << shift;
                shift += 7;
                if b & 0x80 == 0 { break; }
            }
            id += delta;
            let score = u16_at(&self.data, pos);
            pos += 2;
            if id == target { return Some(score); }
            if id > target { return None; }
        }
        None
    }
}

fn lower(text: &str, turkish: bool) -> String {
    if turkish {
        text.chars().map(|c| match c { 'I' => "ı".to_string(), 'İ' => "i".to_string(), _ => c.to_lowercase().collect() }).collect()
    } else {
        text.to_lowercase()
    }
}

/// 表と、名詞の頭を大文字で書く言語 (ドイツ語) の大文字の表
struct Tables(Vec<Hwn>, Vec<Option<Capitals>>);

impl Model for Tables {
    fn languages(&self) -> usize { self.0.len() }
    fn lower(&self, l: usize, word: &str) -> String { lower(word, self.0[l].turkish) }
    fn find(&self, l: usize, w: &str) -> Option<usize> {
        let t = &self.0[l];
        t.find(w.as_bytes()).filter(|&i| u16_at(&t.data, t.uni + 2 * i) != NEVER)
    }
    fn uni(&self, l: usize, id: usize) -> u32 { u16_at(&self.0[l].data, self.0[l].uni + 2 * id) as u32 }
    fn after(&self, l: usize, prev: usize, id: usize) -> u32 {
        let t = &self.0[l];
        match t.pair(prev, id) {
            Some(s) => s as u32,
            None => self.uni(l, id) + u16_at(&t.data, t.mult + 2 * prev) as u32,
        }
    }
    fn display(&self, l: usize, id: usize) -> String { self.0[l].display(id) }
    fn pair(&self, l: usize, prev: usize, id: usize) -> Option<u32> { self.0[l].pair(prev, id).map(u32::from) }
    fn alphabet(&self, l: usize) -> &[char] { &self.0[l].alphabet }
    fn capitals(&self, l: usize) -> Option<&Capitals> { self.1[l].as_ref() }
    fn accent_variants(&self, l: usize, base: &str) -> Vec<(usize, String)> {
        let t = &self.0[l];
        t.accents.get(base).map_or(vec![], |ids| ids.iter().map(|&i| (i, String::from_utf8_lossy(t.word(i)).into_owned())).collect())
    }
    fn longer_word(&self, l: usize, prefix: &str) -> bool {
        let t = &self.0[l];
        let i = t.lower_bound(prefix.as_bytes());
        (i..t.words.min(i + 3)).any(|j| t.word(j).starts_with(prefix.as_bytes()) && t.word(j).len() > prefix.len())
    }
}

/// 決まった種の乱数 (xorshift)
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

const ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
fn neighbours(c: char) -> Vec<char> {
    let mut out = vec![];
    for a in 'a'..='z' {
        if a != c && latin_spell_near(a, c) { out.push(a); }
    }
    out
}
fn latin_spell_near(a: char, b: char) -> bool {
    let pos = |c: char| ROWS.iter().enumerate().find_map(|(r, row)| row.find(c).map(|i| (r as i32, i as i32)));
    let (Some((ra, ia)), Some((rb, ib))) = (pos(a), pos(b)) else { return false };
    (ra == rb && (ia - ib).abs() == 1) || ((ra - rb).abs() == 1 && {
        let (upper, lower) = if ra < rb { (ia, ib) } else { (ib, ia) };
        lower == upper || lower == upper - 1
    })
}

/// 1 字違いを入れる (打った人の誤りに近い形: 隣のキーの置き換え・抜け・余り (隣のキーか同じ字)・入れ替え)
fn typo(word: &str, rng: &mut Rng) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    for _ in 0..10 {
        let i = rng.below(n);
        let mut c = chars.clone();
        match rng.below(4) {
            0 => {
                let base = base_char(c[i].to_ascii_lowercase());
                let near = neighbours(base);
                if near.is_empty() { continue; }
                c[i] = near[rng.below(near.len())];
            }
            1 => { if n <= 3 { continue; } c.remove(i); }
            2 => {
                let base = base_char(c[i].to_ascii_lowercase());
                let mut options = neighbours(base);
                options.push(base);
                let extra = options[rng.below(options.len())];
                c.insert(i + rng.below(2), extra);
            }
            _ => { if i + 1 >= n || c[i] == c[i + 1] { continue; } c.swap(i, i + 1); }
        }
        let out: String = c.into_iter().collect();
        if out != word { return Some(out); }
    }
    None
}

fn strip_accents(word: &str) -> String { word.chars().map(|c| { let b = base_char(c.to_lowercase().next().unwrap_or(c)); if c.is_uppercase() { b.to_uppercase().next().unwrap_or(b) } else { b } }).collect() }

#[derive(Default)]
struct Counts { sentences: usize, words: usize, clean_suggest: usize, clean_auto: usize,
                typo: usize, typo_real: usize, s_fixed: usize, s_wrong: usize, a_fixed: usize, a_wrong: usize,
                accent: usize, accent_real: usize, as_fixed: usize, as_wrong: usize, aa_fixed: usize, aa_wrong: usize,
                prev_pairs: usize, prev_false: usize, prev_real: usize, prev_fixed: usize, prev_wrong: usize,
                // アクセントの数 (1・2・3 以上) ごと: 外した語の数, 全部戻った, 一部だけ戻った, 違う語にした, 大文字・小文字だけ違う (指摘の線で)
                by_marks: [[usize; 5]; 3],
                // ドイツ語の名詞の頭の大文字: 正しい文で小文字の語を大文字にした (指摘・自動)、小文字にした名詞の数・直った (指摘・自動)・違う形
                cap_clean_s: usize, cap_clean_a: usize, cap_nouns: usize, cap_fixed_s: usize, cap_fixed_a: usize, cap_wrong: usize }

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let tables_dir = std::path::PathBuf::from(get("--tables").expect("--tables"));
    let words_dir = std::path::PathBuf::from(get("--words").expect("--words"));
    let data_dir = std::path::PathBuf::from(get("--data").expect("--data"));
    let langs: Vec<String> = get("--langs").unwrap_or("en".into()).split(',').map(String::from).collect();
    let split = get("--split").unwrap_or("dev".into());
    let seed: u64 = get("--seed").map_or(20261105, |s| s.parse().unwrap());
    let limit: usize = get("--limit").map_or(usize::MAX, |s| s.parse().unwrap());
    let show: usize = get("--show").map_or(0, |s| s.parse().unwrap());
    let mut th = Thresholds::default();
    for (i, a) in args.iter().enumerate() {
        if a != "--th" { continue; }
        let (k, v) = args[i + 1].split_once('=').unwrap();
        let v: u32 = v.parse().unwrap();
        match k {
            "max_candidate_uni" => th.max_candidate_uni = v, "rare_known" => th.rare_known = v,
            "suggest_margin" => th.suggest_margin = v, "auto_margin" => th.auto_margin = v,
            "known_suggest_margin" => th.known_suggest_margin = v, "known_auto_margin" => th.known_auto_margin = v,
            "max_total" => th.max_total = v, "unknown_cost" => th.unknown_cost = v, "end_edit" => th.end_edit = v,
            "sentence_start_capital" => th.sentence_start_capital = v, "previous_accent_margin" => th.previous_accent_margin = v, "capital_suggest" => th.capital_suggest = v, "capital_auto" => th.capital_auto = v, "min_len" => th.min_len = v as usize, "auto_min_len" => th.auto_min_len = v as usize,
            _ => panic!("unknown threshold {k}"),
        }
    }
    // 直さない語 (IME と同じ: english_names・english_brands・english_case の英字だけの行。小文字)
    let mut protected: HashSet<String> = HashSet::new();
    for name in ["english_names.txt", "english_brands.txt", "english_case.txt"] {
        for line in std::fs::read_to_string(words_dir.join(name)).unwrap().lines() {
            let w = line.trim();
            if !w.is_empty() && !w.starts_with('#') && w.chars().all(|c| c.is_ascii_alphabetic()) {
                protected.insert(w.to_ascii_lowercase());
            }
        }
    }
    let is_protected = |w: &str| protected.contains(w);
    // 設定のアクセントの言語 → 表 (IME の next_word.rs の LANGUAGE_FILES と同じ)
    let files_of = |lang: &str| -> Vec<&'static str> {
        match lang {
            "en" => vec!["en"],
            "da" | "sv" | "nb" | "is" | "fi" => vec!["en", "da", "sv", "nb", "is", "fi"],
            "fr" => vec!["en", "fr"], "de" => vec!["en", "de"], "es" => vec!["en", "es"], "it" => vec!["en", "it"], "pt" => vec!["en", "pt"],
            "pl" => vec!["en", "pl"], "cs" => vec!["en", "cs"], "tr" => vec!["en", "tr"],
            _ => panic!("unknown language {lang}"),
        }
    };
    let mut total = Counts::default();
    for lang in &langs {
        let files = files_of(lang);
        let capitals = files.iter().map(|f| (*f == "de").then(|| Capitals::parse(&std::fs::read_to_string(words_dir.join("german_case.tsv")).unwrap()))).collect();
        let model = Tables(files.iter().map(|f| Hwn::load(&tables_dir.join(format!("next_{f}.hwn")))).collect(), capitals);
        let text = std::fs::read_to_string(data_dir.join(format!("{lang}.txt"))).unwrap();
        let sentences: Vec<&str> = text.lines().enumerate().filter(|(i, _)| (i % 2 == 0) == (split == "dev")).map(|(_, s)| s.trim())
            .filter(|s| !s.is_empty()).take(limit).collect();
        let mut rng = Rng(seed ^ (lang.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64)) | 1));
        let mut c = Counts::default();
        let mut shown = [0usize; 3];
        // 文を Space ごとの塊で確かめる。replace = (語の番号, 入れる綴り)
        let run = |sentence: &str, replace: Option<(usize, &str)>| -> Vec<(usize, latin_spell::Fix)> {
            let mut out = vec![];
            let mut before = String::new();
            let mut index = 0usize;
            for chunk in sentence.split(' ').filter(|c| !c.is_empty()) {
                let mut chunk_text = chunk.to_string();
                let found = latin_spell::words(chunk);
                if let Some((target, spelling)) = replace {
                    for (j, &(a, b, _)) in found.iter().enumerate().rev() {
                        if index + j == target { chunk_text.replace_range(a..b, spelling); }
                    }
                }
                let base_index = index;
                for fix in check_text_with(&model, &th, &is_protected, &before, &chunk_text) {
                    let k = base_index + latin_spell::words(&chunk_text).iter().position(|&(a, _, _)| a == fix.start).unwrap_or(0);
                    out.push((k, fix));
                }
                index += found.len();
                if !before.is_empty() { before.push(' '); }
                before.push_str(&chunk_text);
            }
            out
        };
        for sentence in &sentences {
            c.sentences += 1;
            let word_texts: Vec<String> = sentence.split(' ').filter(|c| !c.is_empty()).flat_map(|chunk| latin_spell::words(chunk).into_iter().map(move |(a, b, _)| chunk[a..b].to_string())).collect();
            c.words += word_texts.len();
            // 正しい文
            for (_, fix) in run(sentence, None) {
                if fix.kind == Kind::Capital {
                    c.cap_clean_s += 1;
                    if fix.autocorrect { c.cap_clean_a += 1; }
                    if shown[0] < show { shown[0] += 1; eprintln!("[{lang} 小文字の語を大文字に{}] {} → {}  | {}", if fix.autocorrect { "・自動" } else { "" }, fix.typed, fix.fixed, sentence); }
                }
                c.clean_suggest += 1;
                if fix.autocorrect { c.clean_auto += 1; }
                if shown[0] < show { shown[0] += 1; eprintln!("[{lang} 正しい文を変えた{}] {} → {}  | {}", if fix.autocorrect { "・自動" } else { "" }, fix.typed, fix.fixed, sentence); }
            }
            // 1 字違い
            let eligible: Vec<usize> = (0..word_texts.len()).filter(|&k| {
                let w = &word_texts[k];
                w.chars().count() >= 3 && w.chars().all(char::is_alphabetic) && !w.chars().skip(1).any(char::is_uppercase)
                    && (k == 0 || !w.chars().next().unwrap().is_uppercase()) && !protected.contains(&w.to_lowercase())
            }).collect();
            if !eligible.is_empty() {
                let k = eligible[rng.below(eligible.len())];
                if let Some(t) = typo(&word_texts[k], &mut rng) {
                    let real = (0..model.languages()).any(|l| model.find(l, &model.lower(l, &t)).is_some());
                    if real { c.typo_real += 1; } else {
                        c.typo += 1;
                        let fix = run(sentence, Some((k, &t))).into_iter().find(|(i, _)| *i == k).map(|(_, f)| f);
                        if let Some(fix) = fix {
                            let right = fix.fixed == word_texts[k];
                            if right { c.s_fixed += 1 } else { c.s_wrong += 1 }
                            if fix.autocorrect { if right { c.a_fixed += 1 } else { c.a_wrong += 1 } }
                            if !right && shown[1] < show { shown[1] += 1; eprintln!("[{lang} 違う形に直した{}] {} → {} (正 {})", if fix.autocorrect { "・自動" } else { "" }, t, fix.fixed, word_texts[k]); }
                        }
                    }
                }
            }
            // ドイツ語の名詞の頭: 文の途中の大文字で始まる語 (全部大文字でない・大文字の表にある語) を 1 つ小文字にする
            if lang == "de" {
                let found = latin_spell::words(sentence);
                let nouns: Vec<usize> = (0..found.len().min(word_texts.len())).filter(|&k| {
                    let w = &word_texts[k];
                    let lower = w.to_lowercase();
                    !found[k].2 && w.chars().next().is_some_and(char::is_uppercase) && !w.chars().skip(1).any(char::is_uppercase)
                        && model.1.iter().flatten().any(|cap| cap.words.contains_key(&lower))
                }).collect();
                if !nouns.is_empty() {
                    let k = nouns[rng.below(nouns.len())];
                    let lowered = word_texts[k].to_lowercase();
                    c.cap_nouns += 1;
                    if let Some((_, fix)) = run(sentence, Some((k, &lowered))).into_iter().find(|(i, _)| *i == k) {
                        if fix.fixed == word_texts[k] {
                            c.cap_fixed_s += 1;
                            if fix.autocorrect { c.cap_fixed_a += 1; }
                        } else {
                            c.cap_wrong += 1;
                        }
                    }
                }
            }
            // アクセント
            let accented: Vec<usize> = (0..word_texts.len()).filter(|&k| word_texts[k].chars().any(|ch| base_char(ch.to_lowercase().next().unwrap_or(ch)) != ch.to_lowercase().next().unwrap_or(ch))
                && (k == 0 || !word_texts[k].chars().next().unwrap().is_uppercase())).collect();
            if !accented.is_empty() {
                let k = accented[rng.below(accented.len())];
                let stripped = strip_accents(&word_texts[k]);
                let real = (0..model.languages()).any(|l| model.find(l, &model.lower(l, &stripped)).is_some());
                c.accent += 1;
                if real { c.accent_real += 1; }
                let fix = run(sentence, Some((k, &stripped))).into_iter().find(|(i, _)| *i == k).map(|(_, f)| f);
                let marks = word_texts[k].chars().filter(|&ch| { let l = ch.to_lowercase().next().unwrap_or(ch); base_char(l) != l }).count();
                let bucket = &mut c.by_marks[marks.min(3) - 1];
                bucket[0] += 1;
                if let Some(fix) = &fix {
                    // 一部だけ: 元の語のアクセントの一部だけが付いた形 (élève → elève)
                    let original: Vec<char> = word_texts[k].chars().collect();
                    let fixed: Vec<char> = fix.fixed.chars().collect();
                    let case_only = fix.fixed != word_texts[k] && fix.fixed.to_lowercase() == word_texts[k].to_lowercase();
                    let partial = !case_only && fixed.len() == original.len() && fix.fixed != word_texts[k]
                        && fixed.iter().zip(&original).all(|(&f, &o)| f == o || f == base_char(o));
                    if fix.fixed == word_texts[k] { bucket[1] += 1 } else if case_only { bucket[4] += 1 } else if partial { bucket[2] += 1 } else { bucket[3] += 1 }
                }
                if let Some(fix) = fix {
                    let right = fix.fixed == word_texts[k];
                    if right { c.as_fixed += 1 } else { c.as_wrong += 1 }
                    if fix.autocorrect { if right { c.aa_fixed += 1 } else { c.aa_wrong += 1 } }
                    if !right && shown[2] < show { shown[2] += 1; eprintln!("[{lang} アクセントを違う形に{}] {} → {} (正 {})", if fix.autocorrect { "・自動" } else { "" }, stripped, fix.fixed, word_texts[k]); }
                }
            }
        }
        // 前の語のアクセント (tu hablas → tú hablas): 正しい文では、後ろの語を 1 字ずつ打つ間に一度でも前の語の指摘が出たら誤った指摘。
        // アクセントを外すと表にある語になる語を外した文では、後ろの語を打ち終えたときに元の形の指摘が出たら戻った
        for sentence in &sentences {
            let found = latin_spell::words(sentence);
            for k in 0..found.len().saturating_sub(1) {
                let (s1, e1, _) = found[k + 1];
                let next = &sentence[s1..e1];
                let before = &sentence[..s1];
                c.prev_pairs += 1;
                let prefixes: Vec<&str> = next.char_indices().map(|(i, ch)| &next[..i + ch.len_utf8()]).filter(|p| p.chars().count() >= 2).collect();
                if let Some(fix) = prefixes.iter().find_map(|p| suggestion_with(&model, &th, &is_protected, before, p).filter(|f| f.previous)) {
                    c.prev_false += 1;
                    if shown[0] < show { shown[0] += 1; eprintln!("[{lang} 前の語の誤った指摘] {} → {} (後ろ {next})", fix.typed, fix.fixed); }
                }
                let (s0, e0, _) = found[k];
                let word = &sentence[s0..e0];
                let stripped = strip_accents(word);
                if stripped == word || !(0..model.languages()).any(|l| model.find(l, &model.lower(l, &stripped)).is_some()) {
                    continue;
                }
                c.prev_real += 1;
                let changed = format!("{}{}{}", &sentence[..s0], stripped, &sentence[e0..s1]);
                if let Some(fix) = suggestion_with(&model, &th, &is_protected, &changed, next).filter(|f| f.previous) {
                    if fix.fixed == word { c.prev_fixed += 1 } else {
                        c.prev_wrong += 1;
                        if shown[2] < show { shown[2] += 1; eprintln!("[{lang} 前の語を違う形に] {stripped} → {} (正 {word})", fix.fixed); }
                    }
                }
            }
        }
        report(lang, &c);
        if lang == "de" {
            println!("{lang}\t名詞の頭の大文字\t正しい文で小文字の語を大文字にした 指摘 {}・自動 {}\t小文字にした名詞 {}: 直った 指摘 {} ({:.1}%)・自動 {} ({:.1}%)・違う形 {}",
                     c.cap_clean_s, c.cap_clean_a, c.cap_nouns, c.cap_fixed_s, 100.0 * c.cap_fixed_s as f64 / c.cap_nouns.max(1) as f64,
                     c.cap_fixed_a, 100.0 * c.cap_fixed_a as f64 / c.cap_nouns.max(1) as f64, c.cap_wrong);
        }
        println!("{lang}\tアクセントの数ごと (指摘)\t{}", (0..3).map(|b| format!("{}: 外した {}・全部戻った {}・大小だけ違う {}・一部だけ {}・違う語 {}",
            ["1", "2", "3 以上"][b], c.by_marks[b][0], c.by_marks[b][1], c.by_marks[b][4], c.by_marks[b][2], c.by_marks[b][3])).collect::<Vec<_>>().join(" | "));
        for b in 0..3 { for j in 0..5 { total.by_marks[b][j] += c.by_marks[b][j]; } }
        for (t, v) in [(&mut total.sentences, c.sentences), (&mut total.words, c.words), (&mut total.clean_suggest, c.clean_suggest), (&mut total.clean_auto, c.clean_auto),
                       (&mut total.typo, c.typo), (&mut total.typo_real, c.typo_real), (&mut total.s_fixed, c.s_fixed), (&mut total.s_wrong, c.s_wrong),
                       (&mut total.a_fixed, c.a_fixed), (&mut total.a_wrong, c.a_wrong), (&mut total.accent, c.accent), (&mut total.accent_real, c.accent_real),
                       (&mut total.as_fixed, c.as_fixed), (&mut total.as_wrong, c.as_wrong), (&mut total.aa_fixed, c.aa_fixed), (&mut total.aa_wrong, c.aa_wrong),
                       (&mut total.prev_pairs, c.prev_pairs), (&mut total.prev_false, c.prev_false), (&mut total.prev_real, c.prev_real),
                       (&mut total.prev_fixed, c.prev_fixed), (&mut total.prev_wrong, c.prev_wrong)] {
            *t += v;
        }
    }
    if langs.len() > 1 {
        report("計", &total);
        println!("計\tアクセントの数ごと (指摘)\t{}", (0..3).map(|b| format!("{}: 外した {}・全部戻った {}・大小だけ違う {}・一部だけ {}・違う語 {}",
            ["1", "2", "3 以上"][b], total.by_marks[b][0], total.by_marks[b][1], total.by_marks[b][4], total.by_marks[b][2], total.by_marks[b][3])).collect::<Vec<_>>().join(" | "));
    }
}

fn report(lang: &str, c: &Counts) {
    let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
    println!("{lang}\t文 {}・語 {}\t正しい文を変えた 指摘 {} ({:.3}%)・自動 {} ({:.3}%)\t1 字違い {} (表にある綴り {} は除く): 指摘 直った {} ({:.1}%)・違う形 {} / 自動 直った {} ({:.1}%)・違う形 {}\tアクセント {} (外すと表にある {}): 指摘 直った {} ({:.1}%)・違う形 {} / 自動 直った {} ({:.1}%)・違う形 {}",
             c.sentences, c.words, c.clean_suggest, pct(c.clean_suggest, c.words), c.clean_auto, pct(c.clean_auto, c.words),
             c.typo, c.typo_real, c.s_fixed, pct(c.s_fixed, c.typo), c.s_wrong, c.a_fixed, pct(c.a_fixed, c.typo), c.a_wrong,
             c.accent, c.accent_real, c.as_fixed, pct(c.as_fixed, c.accent), c.as_wrong, c.aa_fixed, pct(c.aa_fixed, c.accent), c.aa_wrong);
    println!("{lang}\t前の語のアクセント\t語の組 {} で誤った指摘 {} ({:.3}%)\t外すと表にある語 {}: 戻った {} ({:.1}%)・違う形 {}",
             c.prev_pairs, c.prev_false, pct(c.prev_false, c.prev_pairs), c.prev_real, c.prev_fixed, pct(c.prev_fixed, c.prev_real), c.prev_wrong);
}
