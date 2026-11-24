// 2026-10-19: 次の語の候補 (英語と、設定で選んだアクセントの言語)。前の語と打ちかけの文字から、続きそうな語を出す
// (my f → first favorite family、mein v → Vater)。表は FineWeb / FineWeb-2 と Tatoeba の隣り合う 2 語の数から作った
// next_<言語>.hwn (data_ext\bigram_v2 と ~/haniword_ngram の scripts/hwn.py の形)。IME 本体の隣の next_words フォルダに置き、
// どのアプリでも同じファイルを読み取り専用で写像する (Windows がページを共有するので、アプリの数だけは増えない)。
// 表の中身はすべて範囲を確かめてから読む (壊れたファイルでも落ちず、候補が出ないだけ)。
use std::collections::HashMap;
use std::sync::OnceLock;

const NEVER: u16 = u16::MAX;
/// 前の語を知らない言語の候補は、前の語を知っている言語の候補より後ろに回す (-ln の 1000 倍で e^-2 ≒ 0.14 倍)
const UNKNOWN_CONTEXT_PENALTY: u32 = 2000;

/// 1 つの言語の表 (ファイルの中身そのもの。読み取り専用)
pub struct Table {
    data: &'static [u8],
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
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

/// 2026-10-20 (Codex の再調査 L4・L5): 表の語の長さ (UTF-8 のバイト数) と表のファイルの大きさの上限。
/// 配っている表は語が最長 34 バイト・ファイルが最大 20MB。壊れた表で 1 回の候補探しが重くならないように、読み込むときに確かめる
const MAX_WORD_BYTES: u32 = 64;
const MAX_TABLE_BYTES: i64 = 64 << 20;

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// 言語に合わせた小文字 (トルコ語は I → ı、İ → i)
fn lower(text: &str, turkish: bool) -> String {
    if turkish {
        text.chars().map(|c| match c { 'I' => "ı".to_string(), 'İ' => "i".to_string(), _ => c.to_lowercase().collect() }).collect()
    } else {
        text.to_lowercase()
    }
}

impl Table {
    /// ファイルの中身から表を作る。形が合わなければ None
    pub fn parse(data: &'static [u8]) -> Option<Self> {
        if data.get(0..4)? != b"HWNX" || u32_at(data, 4)? != 1 {
            return None;
        }
        let words = u32_at(data, 16)? as usize;
        // 語の数はファイルの大きさを超えない (32 ビットで 4 * words が折り返さないように。Fable の再調査 L2)
        if words > data.len() / 4 {
            return None;
        }
        let flags = u32_at(data, 24)?;
        let offset = |i: usize| u32_at(data, 28 + 4 * i).map(|v| v as usize);
        let table = Table {
            data,
            words,
            turkish: flags & 1 != 0,
            word_index: offset(0)?,
            word_blob: offset(1)?,
            display_index: offset(2)?,
            display_blob: offset(3)?,
            uni: offset(4)?,
            mult: offset(5)?,
            pair_start: offset(6)?,
            pair_data: offset(7)?,
        };
        // 表の大きさが合っているか (各配列の終わりがファイルの中にあるか)
        let fits = |start: usize, unit: usize, count: usize| {
            unit.checked_mul(count).and_then(|len| start.checked_add(len)).is_some_and(|end| end <= data.len())
        };
        if !(fits(table.word_index, 4, words + 1)
            && fits(table.display_index, 4, words)
            && fits(table.uni, 2, words)
            && fits(table.mult, 2, words)
            && fits(table.pair_start, 4, words + 1))
        {
            return None;
        }
        // 語の位置は小さい順で、どの語も MAX_WORD_BYTES 以下 (候補探しの手間が語の長さで決まるので、読み込むときに 1 度だけ確かめる)
        let mut previous = u32_at(data, table.word_index)?;
        for i in 1..=words {
            let next = u32_at(data, table.word_index + 4 * i)?;
            if next < previous || next - previous > MAX_WORD_BYTES {
                return None;
            }
            previous = next;
        }
        if table.word_blob.checked_add(previous as usize)? > data.len() {
            return None;
        }
        Some(table)
    }

    fn word(&self, i: usize) -> Option<&'static [u8]> {
        let at = self.word_index.checked_add(i.checked_mul(4)?)?;
        let start = u32_at(self.data, at)? as usize;
        let end = u32_at(self.data, at.checked_add(4)?)? as usize;
        self.data.get(self.word_blob.checked_add(start)?..self.word_blob.checked_add(end)?)
    }

    fn display(&self, i: usize) -> Option<&'static str> {
        let at = u32_at(self.data, self.display_index.checked_add(i.checked_mul(4)?)?)?;
        let bytes = if at == u32::MAX {
            self.word(i)?
        } else {
            let at = self.display_blob.checked_add(at as usize)?;
            let len = *self.data.get(at)? as usize;
            self.data.get(at.checked_add(1)?..at.checked_add(1 + len)?)?
        };
        std::str::from_utf8(bytes).ok()
    }

    /// key 以上の最初の語の番号
    fn lower_bound(&self, key: &[u8]) -> usize {
        let (mut lo, mut hi) = (0, self.words);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.word(mid).is_some_and(|w| w < key) { lo = mid + 1 } else { hi = mid }
        }
        lo
    }

    /// prefix で始まる語の番号の範囲
    fn prefix_range(&self, prefix: &[u8]) -> (usize, usize) {
        let lo = self.lower_bound(prefix);
        let (mut a, mut b) = (lo, self.words);
        // 範囲の終わりも二分探索で (prefix で始まらない最初の語)
        while a < b {
            let mid = (a + b) / 2;
            if self.word(mid).is_some_and(|w| w.starts_with(prefix)) { a = mid + 1 } else { b = mid }
        }
        (lo, a)
    }

    fn find(&self, word: &[u8]) -> Option<usize> {
        let i = self.lower_bound(word);
        (self.word(i)? == word).then_some(i)
    }

    /// w1 のあとに表にある語 (番号, -ln P * 1000)。番号の小さい順
    fn successors(&self, w1: usize, mut visit: impl FnMut(usize, u16)) {
        let Some(at) = w1.checked_mul(4).and_then(|o| self.pair_start.checked_add(o)) else { return };
        let (Some(start), Some(end)) = (u32_at(self.data, at), at.checked_add(4).and_then(|a| u32_at(self.data, a))) else {
            return;
        };
        let (Some(mut pos), Some(end)) = (self.pair_data.checked_add(start as usize), self.pair_data.checked_add(end as usize)) else {
            return;
        };
        if end > self.data.len() {
            return;
        }
        let mut id = 0usize;
        let mut first = true;
        while pos < end {
            let (mut delta, mut shift) = (0usize, 0);
            loop {
                let Some(&byte) = self.data.get(pos) else { return };
                pos += 1;
                delta |= ((byte & 0x7F) as usize) << shift;
                shift += 7;
                if byte & 0x80 == 0 || shift > 28 {
                    break;
                }
            }
            // 番号は表の語の中で、小さい順に 1 回ずつ (同じ番号の繰り返しで走査が膨らまないように。Codex の再調査 L4)
            if !first && delta == 0 {
                return;
            }
            first = false;
            let Some(next) = id.checked_add(delta) else { return };
            if next >= self.words {
                return;
            }
            id = next;
            let Some(score) = u16_at(self.data, pos) else { return };
            pos += 2;
            visit(id, score);
        }
    }

    /// 前の語 prev のあとに、typed で始まる語の候補 (点 = -ln P * 1000、小さいほど良い, 小文字, 書く形)。
    /// 打った文字より min_gain 字以上長い語だけ。prev を知らなければ単語の頻度だけで並べる
    fn candidates(&self, prev: &str, typed: &str, n: usize, min_gain: usize) -> (bool, Vec<(u32, &'static str, &'static str)>) {
        let typed_lower = lower(typed, self.turkish);
        let (lo, hi) = self.prefix_range(typed_lower.as_bytes());
        // 範囲が表の語の数を超えるのは壊れた表だけ。走査が終わらなくならないように出さない
        if hi < lo || hi - lo > self.words.min(1 << 20) {
            return (false, vec![]);
        }
        let need = typed_lower.chars().count() + min_gain;
        let long_enough = |i: usize| self.word(i).and_then(|w| std::str::from_utf8(w).ok()).is_some_and(|w| w.chars().count() >= need);
        let mut scored: Vec<(u32, usize)> = Vec::new();
        let known = if prev.is_empty() { None } else { self.find(lower(prev, self.turkish).as_bytes()) };
        let mut mult = 0u32;
        if let Some(w1) = known {
            mult = u16_at(self.data, self.mult + 2 * w1).unwrap_or(0) as u32;
            self.successors(w1, |id, score| {
                if (lo..hi).contains(&id) && long_enough(id) {
                    scored.push((score as u32, id));
                }
            });
        }
        let stored: std::collections::HashSet<usize> = scored.iter().map(|&(_, id)| id).collect();
        // 頻度だけの候補は、範囲の中で点の良いものから n + 表にある数だけ見れば足りる
        let mut fallback: Vec<(u32, usize)> = (lo..hi)
            .filter_map(|i| {
                let u = u16_at(self.data, self.uni + 2 * i)?;
                (u != NEVER && !stored.contains(&i)).then_some((u as u32 + mult, i))
            })
            .collect();
        fallback.sort_unstable();
        scored.extend(fallback.into_iter().filter(|&(_, i)| long_enough(i)).take(n + 1));
        scored.sort_unstable();
        scored.truncate(n);
        let out = scored
            .into_iter()
            .filter_map(|(score, i)| {
                let word = std::str::from_utf8(self.word(i)?).ok()?;
                Some((score, word, self.display(i)?))
            })
            .collect();
        (known.is_some(), out)
    }
}

/// 2026-11-05 (仕事 24): つづりの指摘・自動修正 (latin_spell.rs) が表を語の出やすさとして引くための関数。点は -ln P * 1000
impl Table {
    /// 小文字の語の番号 (表にあって、出たことのある語だけ)
    pub(crate) fn spell_find(&self, lower_word: &str) -> Option<usize> {
        self.find(lower_word.as_bytes()).filter(|&i| u16_at(self.data, self.uni + 2 * i).is_some_and(|u| u != NEVER))
    }
    pub(crate) fn spell_uni(&self, id: usize) -> u32 {
        u16_at(self.data, self.uni + 2 * id).unwrap_or(NEVER) as u32
    }
    /// 前の語のあとの点 (組が表に無ければ 語だけの点 + 前の語の係数)
    pub(crate) fn spell_after(&self, prev: usize, id: usize) -> u32 {
        self.spell_pair(prev, id).unwrap_or_else(|| self.spell_uni(id) + u16_at(self.data, self.mult + 2 * prev).unwrap_or(0) as u32)
    }
    /// 2026-11-08 (仕事 12): 前の語のあとに表にある語と点 (番号 → -ln P * 1000)。英語の句の切り方で、同じ前の語を何度も引くときに 1 度だけ読む
    pub(crate) fn spell_successors(&self, prev: usize) -> HashMap<usize, u32> {
        let mut out = HashMap::new();
        self.successors(prev, |next, score| { out.insert(next, score as u32); });
        out
    }
    /// 2026-11-08 (仕事 12): 前の語の係数 (組が表に無いときに 語だけの点 に足す)
    pub(crate) fn spell_mult(&self, prev: usize) -> u32 {
        u16_at(self.data, self.mult + 2 * prev).unwrap_or(0) as u32
    }
    /// 表に残っている組の点だけ
    pub(crate) fn spell_pair(&self, prev: usize, id: usize) -> Option<u32> {
        let mut stored = None;
        self.successors(prev, |next, score| if next == id { stored = Some(score as u32) });
        stored
    }
    /// 表の言語 (ファイルの頭の 8 バイト。de・en など)
    pub(crate) fn spell_language(&self) -> String {
        self.data.get(8..16).map(|b| String::from_utf8_lossy(b).trim_end_matches('\0').trim().to_string()).unwrap_or_default()
    }
    pub(crate) fn spell_words(&self) -> usize {
        self.words
    }
    pub(crate) fn spell_word(&self, id: usize) -> String {
        self.word(id).and_then(|w| std::str::from_utf8(w).ok()).unwrap_or_default().to_string()
    }
    pub(crate) fn spell_display(&self, id: usize) -> String {
        self.display(id).unwrap_or_default().to_string()
    }
    pub(crate) fn spell_lower(&self, word: &str) -> String {
        lower(word, self.turkish)
    }
    /// prefix で始まる、より長い語があるか
    pub(crate) fn spell_longer(&self, prefix: &str) -> bool {
        let (lo, hi) = self.prefix_range(prefix.as_bytes());
        (lo..hi.min(lo + 2)).any(|i| self.word(i).is_some_and(|w| w.len() > prefix.len()))
    }
    /// 語に出てくる字 (小文字。アクセントつきの字も)。表を開いたときに 1 度だけ作る
    pub(crate) fn spell_alphabet(&self) -> Vec<char> {
        let mut set = std::collections::BTreeSet::new();
        for i in 0..self.words {
            if let Some(word) = self.word(i).and_then(|w| std::str::from_utf8(w).ok()) {
                set.extend(word.chars().filter(|c| c.is_alphabetic() || *c == '\''));
            }
        }
        set.into_iter().collect()
    }
}

/// つづりの指摘・自動修正が引く表 (次の語の候補と同じ: 英語と、設定で選んだアクセントの言語)
pub(crate) fn spell_tables() -> &'static [Table] {
    tables()
}

/// 打った文字の大文字に合わせる (F → Father、FA → FATHER)。表の書く形 (Vater・I) はそのまま使う
fn match_case(display: &str, typed: &str) -> String {
    let mut letters = typed.chars().filter(|c| c.is_alphabetic());
    let Some(first) = letters.next() else { return display.to_string() };
    let all_upper = first.is_uppercase() && typed.chars().filter(|c| c.is_alphabetic()).count() >= 2
        && typed.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase());
    if all_upper {
        return display.to_uppercase();
    }
    if first.is_uppercase() {
        let mut chars = display.chars();
        if let Some(head) = chars.next() {
            return head.to_uppercase().chain(chars).collect();
        }
    }
    display.to_string()
}

/// 前の語と打ちかけの文字から、続きそうな語を n 個 (書く形で、打った文字の大文字に合わせる)。
/// tables を全部引いて点の良い順にまとめる。前の語を知らない言語の候補は後ろに回す
pub fn suggest_from(tables: &[Table], prev: &str, typed: &str, n: usize) -> Vec<String> {
    if !typed.chars().all(|c| c.is_alphabetic() || c == '\'') {
        return vec![];
    }
    let results: Vec<(bool, Vec<(u32, &str, &str)>)> = tables.iter().map(|t| t.candidates(prev, typed, n, 2)).collect();
    let any_known = results.iter().any(|(known, _)| *known);
    let mut all: Vec<(u32, &str, &str)> = results
        .into_iter()
        .flat_map(|(known, list)| {
            let penalty = if any_known && !known { UNKNOWN_CONTEXT_PENALTY } else { 0 };
            list.into_iter().map(move |(score, word, display)| (score + penalty, word, display))
        })
        .collect();
    all.sort_by_key(|&(score, _, _)| score);
    let mut seen = std::collections::HashSet::new();
    all.into_iter()
        .filter(|(_, word, _)| seen.insert(*word))
        .take(n)
        .map(|(_, _, display)| match_case(display, typed))
        .collect()
}

/// 設定の言語 → 表のファイル名 (設定の文字列はパスに使わない)
// 2026-11-06: 対応は shared に 1 か所 (設定アプリ・インストーラーと同じ)
const LANGUAGE_FILES: &[(&str, &[&str])] = shared::NEXT_WORD_LANGUAGE_FILES;

/// ファイルを読み取り専用で写像する (閉じない。プロセスが終わるまで使う)
fn map_file(path: &std::path::Path) -> Option<&'static [u8]> {
    use windows::{
        core::HSTRING,
        Win32::{
            Foundation::{CloseHandle, GENERIC_READ, HANDLE},
            Storage::FileSystem::{CreateFileW, GetFileSizeEx, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, OPEN_EXISTING},
            System::Memory::{CreateFileMappingW, MapViewOfFile, FILE_MAP_READ, PAGE_READONLY},
        },
    };
    unsafe {
        let file = CreateFileW(&HSTRING::from(path.as_os_str()), GENERIC_READ.0, FILE_SHARE_READ, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, HANDLE::default()).ok()?;
        let mut size = 0i64;
        let sized = GetFileSizeEx(file, &mut size).is_ok();
        // 大きすぎる表は写像しない (32 ビットの番地を使い切らない・スライスの長さが isize::MAX を超えない。Codex の再調査 L5)
        let mapping = if sized && size > 64 && size <= MAX_TABLE_BYTES { CreateFileMappingW(file, None, PAGE_READONLY, 0, 0, None).ok() } else { None };
        let _ = CloseHandle(file);
        let mapping = mapping?;
        let view = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0);
        let _ = CloseHandle(mapping);
        if view.Value.is_null() {
            return None;
        }
        Some(std::slice::from_raw_parts(view.Value as *const u8, size as usize))
    }
}

fn tables_dir() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    if let Ok(dir) = std::env::var("AZK_NEXT_WORDS_DIR") {
        return Some(std::path::PathBuf::from(dir));
    }
    super::auto_english::dll_dir().map(|dir| dir.join("next_words"))
}

/// 英語と、設定で選んだアクセントの言語の表 (初めて使うときに開く)
fn tables() -> &'static [Table] {
    static TABLES: OnceLock<Vec<Table>> = OnceLock::new();
    TABLES.get_or_init(|| {
        let Some(dir) = tables_dir() else { return vec![] };
        let languages = super::diacritics::languages();
        let mut files = vec!["en"];
        for (code, names) in LANGUAGE_FILES {
            if languages.iter().any(|l| l.as_str() == *code) {
                files.extend_from_slice(names);
            }
        }
        files
            .into_iter()
            .filter_map(|name| map_file(&dir.join(format!("next_{name}.hwn"))).and_then(Table::parse))
            .collect()
    })
}

/// 前の語と打ちかけの文字から、続きそうな語を 3 つまで
pub fn suggest(prev: &str, typed: &str) -> Vec<String> {
    suggest_from(tables(), prev, typed, 3)
}

/// 設定 input.english_predictions (2026-11-05 から初期値はオフ。前の項目 next_word_suggestions は読まない。仕事 27)
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| shared::AppConfig::read_or_default().input.english_predictions)
}

thread_local! {
    // 最後に確定した文字列 (キャレットの前を読ませてくれないアプリ用。入力欄が変わったら忘れる)
    static LAST_COMMIT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// 2026-11-05 (仕事 24): 最後に確定した文字列 (後ろの 64 字。キャレットの前を読めないアプリで、つづりの指摘・自動修正の前の文字列に使う)
pub fn last_commit_text() -> String {
    LAST_COMMIT.with(|last| last.borrow().clone())
}

/// 確定した文字列 (後ろに足した空白を含む) を覚える。None で忘れる (入力欄が変わったとき)
pub fn note_commit(text: Option<&str>) {
    LAST_COMMIT.with(|last| {
        let mut last = last.borrow_mut();
        match text {
            Some(text) => {
                last.push_str(text);
                // 直前の語が分かれば足りるので、後ろの 64 字だけ残す
                let count = last.chars().count();
                if count > 64 {
                    *last = last.chars().skip(count - 64).collect();
                }
            }
            None => last.clear(),
        }
    });
}

/// キャレットの前の文字列 (読めなければ最後に確定した文字列) から、空白で区切られた直前の語。
/// 空白が無い (語の途中)・句読点や数字で区切られている・前が無いときは空 (表も空白だけで隔たった 2 語から作っている)
pub fn previous_word(before: Option<&str>) -> String {
    let text = match before {
        Some(text) => text.to_string(),
        None => LAST_COMMIT.with(|last| last.borrow().clone()),
    };
    let trimmed = text.trim_end_matches(|c: char| c == ' ' || c == '\t');
    if trimmed.len() == text.len() {
        return String::new();
    }
    // ラテン文字だけ (かな・漢字の語は表に無い)
    let latin = |c: &char| c.is_alphabetic() && ((*c as u32) < 0x250 || (0x1E00..0x1F00).contains(&(*c as u32)));
    let mut word: Vec<char> = trimmed.chars().rev().take_while(|c| latin(c) || *c == '\'' || *c == '’').collect();
    word.reverse();
    let word: String = word.into_iter().collect::<String>().replace('’', "'");
    word.trim_matches('\'').to_string()
}

/// 英数モードで打っている語のあとに出している候補 (アプリの文字はそのまま。Tab で補う)
#[derive(Default, Clone, Debug)]
pub struct LatinState {
    /// 候補を作ったときの打ちかけ (Tab で補ったあとは補った語)
    pub partial: String,
    pub list: Vec<String>,
    /// Tab で補った候補の番号
    pub pick: Option<usize>,
}

thread_local! {
    static LATIN: std::cell::RefCell<Option<LatinState>> = const { std::cell::RefCell::new(None) };
}

/// 出している候補 (出していなければ None)
pub fn latin_state() -> Option<LatinState> {
    LATIN.with(|s| s.borrow().clone())
}

pub fn set_latin_state(state: Option<LatinState>) {
    LATIN.with(|s| *s.borrow_mut() = state);
}

/// キャレットの前の文字列を (前の語, 打ちかけの語) に分ける。打ちかけ = キャレットの直前に続くラテン文字 (と語中の ')。
/// 打ちかけが無ければ None
pub fn split_before_caret(before: &str) -> Option<(String, String)> {
    let latin = |c: char| c.is_alphabetic() && ((c as u32) < 0x250 || (0x1E00..0x1F00).contains(&(c as u32)));
    let chars: Vec<char> = before.chars().collect();
    let mut start = chars.len();
    while start > 0 && (latin(chars[start - 1]) || (chars[start - 1] == '\'' && start >= 2 && latin(chars[start - 2]))) {
        start -= 1;
    }
    let partial: String = chars[start..].iter().collect::<String>().trim_start_matches('\'').to_string();
    if partial.is_empty() {
        return None;
    }
    let rest: String = chars[..start].iter().collect();
    Some((previous_word(Some(&rest)), partial))
}

/// 英数モードで候補を出してよいアプリか。Tab を補完・インデントに使う端末とコードエディタでは出さない
/// (パスワード・数字・IS_PRIVATE などの欄は、composition.rs の latin_field_blocked がその都度確かめる)
pub fn latin_allowed() -> bool {
    enabled() && app_allows_latin_help()
}

/// 2026-11-05: 英字の手助け (次の語の候補・つづりの指摘と自動修正) を出してよいアプリか (端末・コードエディタでない)。設定は見ない
pub fn app_allows_latin_help() -> bool {
    static ALLOWED: OnceLock<bool> = OnceLock::new();
    *ALLOWED.get_or_init(|| {
        let exe = std::env::current_exe().ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()))
            .unwrap_or_default();
        !EXCLUDED_APPS.contains(&exe.as_str())
    })
}

/// 英数モードで候補を出さないアプリ (実行ファイル名、小文字、ABC 順)。Tab をシェルの補完やインデントに使う端末と、
/// コードエディタ・IDE (Windows 版の主な製品の実行ファイル名)。ここに無いアプリでは、候補が出ているときだけ Tab を補完に使う
const EXCLUDED_APPS: &[&str] = &[
    // 端末
    "alacritty.exe", "cmd.exe", "conhost.exe", "hyper.exe", "kitty.exe", "mintty.exe", "mobaxterm.exe", "openconsole.exe",
    "powershell.exe", "powershell_ise.exe", "putty.exe", "pwsh.exe", "rlogin.exe", "tabby.exe", "teraterm.exe", "termius.exe",
    "ttermpro.exe", "warp.exe", "wezterm-gui.exe", "windowsterminal.exe",
    // コードエディタ・IDE
    "atom.exe", "brackets.exe", "clion64.exe", "code - insiders.exe", "code.exe", "cursor.exe", "datagrip64.exe", "devenv.exe",
    "eclipse.exe", "emacs.exe", "fleet.exe", "goland64.exe", "gvim.exe", "idea64.exe", "neovide.exe", "notepad++.exe",
    "nvim-qt.exe", "phpstorm64.exe", "pycharm64.exe", "rider64.exe", "rubymine64.exe", "runemacs.exe", "rustrover64.exe",
    "studio64.exe", "sublime_text.exe", "texstudio.exe", "webstorm64.exe", "windsurf.exe", "zed.exe",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static [u8] {
        // scripts/hwn.py で作った小さな表 (語 9 個: father favorite first friend i my mein vater was)
        include_bytes!("next_word_fixture.hwn")
    }

    /// 2026-11-06: インストーラーは英語の表だけ必ず入れ、ほかの言語は選んだものだけ入れる。入れていない言語の表は開けずに飛ばす (落ちない)
    #[test]
    fn missing_language_table_is_skipped() {
        let dir = std::env::temp_dir().join(format!("haniword_next_words_missing_{}", std::process::id()));
        assert!(map_file(&dir.join("next_fr.hwn")).is_none());
        let names = ["en", "fr"];
        let found: Vec<_> = names.iter().filter_map(|n| map_file(&dir.join(format!("next_{n}.hwn")))).collect();
        assert!(found.is_empty());
    }

    #[test]
    fn previous_word_ranks_its_successors_first_and_keeps_written_forms() {
        let tables = vec![Table::parse(fixture()).unwrap()];
        assert_eq!(suggest_from(&tables, "my", "f", 3), vec!["father", "friend", "first"]);
        // 前の語を知らなければ単語の頻度の順
        assert_eq!(suggest_from(&tables, "zzz", "f", 3)[0], "first");
        // 打った大文字に合わせる / 表の書く形 (Vater) はそのまま
        assert_eq!(suggest_from(&tables, "my", "F", 1), vec!["Father"]);
        assert_eq!(suggest_from(&tables, "my", "FA", 1), vec!["FATHER"]);
        assert_eq!(suggest_from(&tables, "mein", "v", 1), vec!["Vater"]);
        // 2 字以上は補えない語 (打った字 + 1 字) と、英字以外を含む打ちかけは出さない
        assert!(suggest_from(&tables, "my", "fathe", 3).is_empty());
        assert!(suggest_from(&tables, "my", "f1", 3).is_empty());
    }

    #[test]
    fn previous_word_needs_a_space_and_stops_at_punctuation() {
        assert_eq!(previous_word(Some("I love my ")), "my");
        assert_eq!(previous_word(Some("これは my ")), "my");
        assert_eq!(previous_word(Some("don’t  ")), "don't");
        assert_eq!(previous_word(Some("my")), "");
        assert_eq!(previous_word(Some("Hello, ")), "");
        assert_eq!(previous_word(Some("日本語 ")), "");
        assert_eq!(previous_word(Some("")), "");
        note_commit(None);
        note_commit(Some("father "));
        assert_eq!(previous_word(None), "father");
        note_commit(None);
        assert_eq!(previous_word(None), "");
    }

    #[test]
    fn caret_text_splits_into_previous_and_partial_words() {
        assert_eq!(split_before_caret("I love my fa"), Some(("my".into(), "fa".into())));
        assert_eq!(split_before_caret("my"), Some(("".into(), "my".into())));
        assert_eq!(split_before_caret("Hello, wor"), Some(("".into(), "wor".into())));
        assert_eq!(split_before_caret("don't k"), Some(("don't".into(), "k".into())));
        assert_eq!(split_before_caret("c'est u"), Some(("c'est".into(), "u".into())));
        assert_eq!(split_before_caret("my "), None);
        assert_eq!(split_before_caret("これは"), None);
        assert_eq!(split_before_caret("Españ"), Some(("".into(), "Españ".into())));
    }

    #[test]
    fn broken_tables_are_refused_without_panicking() {
        let data = fixture();
        for cut in [0, 3, 30, 64, 100, data.len() - 1] {
            let leaked: &'static [u8] = Box::leak(data[..cut].to_vec().into_boxed_slice());
            if let Some(table) = Table::parse(leaked) {
                let _ = suggest_from(&[table], "my", "f", 3);
            }
        }
        let mut bad = data.to_vec();
        bad[0] = b'X';
        assert!(Table::parse(Box::leak(bad.into_boxed_slice())).is_none());
        // 語の位置が逆に並んでいる表は読まない
        let table = Table::parse(fixture()).unwrap();
        let mut bad = data.to_vec();
        let at = table.word_index + 4;
        bad[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Table::parse(Box::leak(bad.into_boxed_slice())).is_none());
    }

    #[test]
    fn repeated_successor_ids_stop_the_scan() {
        let table = Table::parse(fixture()).unwrap();
        let w1 = (0..table.words).find(|&w| {
            let mut n = 0;
            table.successors(w, |_, _| n += 1);
            n >= 2
        }).unwrap();
        let start = u32_at(table.data, table.pair_start + 4 * w1).unwrap() as usize;
        let mut bad = table.data.to_vec();
        // 2 つ目の組の差分 (varint) を 0 にする: 1 つ目の組は varint + 点 2 バイト
        let mut at = table.pair_data + start;
        while bad[at] & 0x80 != 0 { at += 1; }
        at += 3;
        bad[at] = 0;
        let broken = Table::parse(Box::leak(bad.into_boxed_slice())).unwrap();
        let mut seen = 0;
        broken.successors(w1, |_, _| seen += 1);
        assert_eq!(seen, 1);
    }

    /// 本物の表で、scripts/hwn.py の答えと同じか (AZK_NEXT_WORDS_DIR と AZK_NEXT_WORDS_EXPECT を指定したときだけ)。
    /// 期待の行は「言語<TAB>前の語<TAB>打ちかけ<TAB>候補1|候補2|候補3」
    #[test]
    fn real_tables_match_the_python_reader() {
        let (Ok(dir), Ok(expect)) = (std::env::var("AZK_NEXT_WORDS_DIR"), std::env::var("AZK_NEXT_WORDS_EXPECT")) else { return };
        let text = std::fs::read_to_string(expect).unwrap();
        let mut checked = 0;
        // 言語ごとに 1 回だけ写像する (32 ビットでは行ごとに写像すると番地が足りなくなる)
        let mut tables: std::collections::HashMap<String, Table> = std::collections::HashMap::new();
        for line in text.lines().filter(|l| !l.is_empty()) {
            let cols: Vec<&str> = line.split('\t').collect();
            let table = tables.entry(cols[0].to_string()).or_insert_with(|| {
                Table::parse(map_file(&std::path::Path::new(&dir).join(format!("next_{}.hwn", cols[0]))).unwrap()).unwrap()
            });
            let got = suggest_from(std::slice::from_ref(table), cols[1], cols[2], 3);
            let want: Vec<&str> = cols[3].split('|').filter(|s| !s.is_empty()).collect();
            assert_eq!(got, want, "{line}");
            checked += 1;
        }
        assert!(checked > 0);
    }
}
