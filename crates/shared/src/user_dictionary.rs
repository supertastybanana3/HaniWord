//! 2026-11-04: 単語登録 (%APPDATA%\HaniWord\user_dictionary.tsv。個人のデータで、配らない・git に入れない)。
//! 設定アプリの「単語登録」が読み書きし、変換サーバー (server-swift の user_dictionary.swift) が同じ形で読む。
//! 行: 読み (ひらがな)<TAB>語<TAB>品詞<TAB>メモ。# で始まる行は説明。
//! ほかの IME の辞書ファイル (Microsoft IME の「一覧の出力」と同じ 読み・語句・品詞[・コメント] のタブ区切り。Shift-JIS・UTF-16・UTF-8) も読み込める。
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

pub const FILE_NAME: &str = "user_dictionary.tsv";
/// 登録できる品詞 (変換サーバーの userDictionaryPartsOfSpeech と同じ)
pub const PARTS_OF_SPEECH: [&str; 5] = ["名詞", "固有名詞", "人名", "地名", "短縮よみ"];
/// 読みの長さの上限 (変換器が辞書を引く読みの長さの上限)
pub const MAX_READING: usize = 20;
/// 語とメモの長さの上限 (変換サーバーが読む欄の長さの上限)
pub const MAX_FIELD: usize = 256;
/// 登録できる語の数の上限 (変換サーバーが読む行の数の上限)
pub const MAX_ENTRIES: usize = 50_000;
/// ファイルの大きさの上限 (変換サーバーと同じ)
pub const MAX_BYTES: usize = 4 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub reading: String,
    pub word: String,
    pub pos: String,
    #[serde(default)]
    pub memo: String,
}

/// 読み込んだ結果。読めなかった行は理由つきで返す
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parsed {
    pub entries: Vec<Entry>,
    pub errors: Vec<String>,
}

pub fn path() -> PathBuf {
    super::get_config_root().join(FILE_NAME)
}

/// 読みをそろえる: 前後の空白を除き、カタカナをひらがなに (ヴ → ゔ)
pub fn normalize_reading(reading: &str) -> String {
    reading.trim().chars().map(|c| match c {
        'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }).collect()
}

fn is_reading_char(c: char) -> bool {
    ('ぁ'..='ゖ').contains(&c) || c == 'ー' || c == 'ゔ'
}

/// ほかの IME の品詞の名前 → 登録できる品詞 (知らない品詞は名詞)
pub fn map_part_of_speech(name: &str) -> &'static str {
    match name.trim() {
        "固有名詞" | "組織" | "固有名詞組織" | "固有名詞一般" => "固有名詞",
        "人名" | "姓" | "名" | "人名姓" | "人名名" => "人名",
        "地名" | "地名その他" => "地名",
        "短縮よみ" | "顔文字" | "記号" => "短縮よみ",
        _ => "名詞",
    }
}

/// 1 語を確かめる (直す前の形で。読みはそろえたあと)
pub fn check(entry: &Entry) -> Result<(), String> {
    let control = |s: &str| s.chars().any(|c| c.is_control());
    if entry.reading.is_empty() { return Err("読みが空です".into()); }
    if !entry.reading.chars().all(is_reading_char) { return Err(format!("読みはひらがなで入れてください: {}", entry.reading)); }
    if entry.reading.chars().count() > MAX_READING { return Err(format!("読みは {MAX_READING} 字までです: {}", entry.reading)); }
    if entry.word.trim().is_empty() { return Err(format!("語が空です: {}", entry.reading)); }
    if entry.word.chars().count() > MAX_FIELD || control(&entry.word) { return Err(format!("語が長すぎるか、使えない字があります: {}", entry.reading)); }
    if !PARTS_OF_SPEECH.contains(&entry.pos.as_str()) { return Err(format!("品詞が違います: {}", entry.pos)); }
    if entry.memo.chars().count() > MAX_FIELD || control(&entry.memo) { return Err(format!("メモが長すぎるか、使えない字があります: {}", entry.reading)); }
    Ok(())
}

/// タブ区切りの表を読む (自分の形とほかの IME の形)。! と # で始まる行・空行は飛ばす。読めない行と、前と同じ読み・語の行は errors に
pub fn parse_tsv(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut seen = std::collections::HashSet::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with('!') { continue; }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            parsed.errors.push(format!("{} 行目: 読みと語がタブで分かれていません", number + 1));
            continue;
        }
        let entry = Entry {
            reading: normalize_reading(cols[0]),
            word: cols[1].trim().to_string(),
            pos: map_part_of_speech(cols.get(2).copied().unwrap_or("")).to_string(),
            memo: cols.get(3).map(|s| s.trim().to_string()).unwrap_or_default(),
        };
        if let Err(error) = check(&entry) {
            parsed.errors.push(format!("{} 行目: {error}", number + 1));
            continue;
        }
        if !seen.insert((entry.reading.clone(), entry.word.clone())) {
            parsed.errors.push(format!("{} 行目: 同じ読みと語が前にあります: {} {}", number + 1, entry.reading, entry.word));
            continue;
        }
        if parsed.entries.len() >= MAX_ENTRIES {
            parsed.errors.push(format!("{MAX_ENTRIES} 語を超えたので、{} 行目から先は読みません", number + 1));
            break;
        }
        parsed.entries.push(entry);
    }
    parsed
}

/// ファイルの中身を文字列に (BOM つきの UTF-8・UTF-16LE・UTF-16BE、BOM なしの UTF-8、Shift-JIS (Windows の CP932) の順に試す)
pub fn decode(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_BYTES { return Err(format!("ファイルが大きすぎます ({} MB まで)", MAX_BYTES >> 20)); }
    let utf16 = |body: &[u8], little: bool| -> Result<String, String> {
        if body.len() % 2 != 0 { return Err("UTF-16 のファイルが途中で切れています".into()); }
        let units: Vec<u16> = body.chunks_exact(2).map(|p| if little { u16::from_le_bytes([p[0], p[1]]) } else { u16::from_be_bytes([p[0], p[1]]) }).collect();
        String::from_utf16(&units).map_err(|_| "UTF-16 として読めません".to_string())
    };
    if let Some(body) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(body.to_vec()).map_err(|_| "UTF-8 として読めません".into());
    }
    if let Some(body) = bytes.strip_prefix(&[0xFF, 0xFE]) { return utf16(body, true); }
    if let Some(body) = bytes.strip_prefix(&[0xFE, 0xFF]) { return utf16(body, false); }
    if let Ok(text) = std::str::from_utf8(bytes) { return Ok(text.to_string()); }
    decode_cp932(bytes)
}

#[cfg(windows)]
fn decode_cp932(bytes: &[u8]) -> Result<String, String> {
    use windows::Win32::Globalization::{MultiByteToWideChar, MB_ERR_INVALID_CHARS};
    let error = || "UTF-8・UTF-16・Shift-JIS のどれとしても読めません".to_string();
    let length = unsafe { MultiByteToWideChar(932, MB_ERR_INVALID_CHARS, bytes, None) };
    if length <= 0 { return Err(error()); }
    let mut wide = vec![0u16; length as usize];
    let written = unsafe { MultiByteToWideChar(932, MB_ERR_INVALID_CHARS, bytes, Some(&mut wide)) };
    if written <= 0 { return Err(error()); }
    String::from_utf16(&wide[..written as usize]).map_err(|_| error())
}

#[cfg(not(windows))]
fn decode_cp932(_bytes: &[u8]) -> Result<String, String> {
    Err("UTF-8・UTF-16 として読めません".into())
}

/// 表の中身 (説明の行のあと、読み<TAB>語<TAB>品詞<TAB>メモ)
pub fn to_tsv(entries: &[Entry]) -> String {
    let mut text = String::from("# HaniWord の単語登録 (読み<TAB>語<TAB>品詞<TAB>メモ)。品詞は 名詞・固有名詞・人名・地名・短縮よみ\n");
    for e in entries {
        text.push_str(&format!("{}\t{}\t{}\t{}\n", e.reading, e.word, e.pos, e.memo));
    }
    text
}

/// 登録を読む (無ければ空)。読めない行は飛ばす
pub fn load() -> Parsed {
    match std::fs::read(path()) {
        Ok(bytes) => match decode(&bytes) {
            Ok(text) => parse_tsv(&text),
            Err(error) => Parsed { entries: vec![], errors: vec![error] },
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Parsed::default(),
        Err(error) => Parsed { entries: vec![], errors: vec![error.to_string()] },
    }
}

/// 登録を書く (全部を確かめてから、一時ファイルに書いて置き換える)。個人のデータなので、ふつうの権限のデスクトップの処理からだけ
pub fn save(entries: &[Entry]) -> Result<(), String> {
    if !super::winsec::private_data_allowed() { return Err("単語登録は、管理者でない通常の設定アプリから保存してください".into()); }
    if entries.len() > MAX_ENTRIES { return Err(format!("登録できるのは {MAX_ENTRIES} 語までです")); }
    let mut seen = std::collections::HashSet::new();
    for e in entries {
        check(e)?;
        if !seen.insert((e.reading.as_str(), e.word.as_str())) { return Err(format!("同じ読みと語が 2 つあります: {} {}", e.reading, e.word)); }
    }
    let path = path();
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    super::write_atomic(&path, to_tsv(entries).as_bytes()).map_err(|e| format!("単語登録を保存できませんでした: {e}"))
}

/// 読み込んだ語を今の登録に足す (同じ読み・語は足さない)。足した数と、重なって足さなかった数
pub fn merge(current: &mut Vec<Entry>, imported: &[Entry]) -> (usize, usize) {
    let mut seen: std::collections::HashSet<(String, String)> = current.iter().map(|e| (e.reading.clone(), e.word.clone())).collect();
    let (mut added, mut duplicates) = (0, 0);
    for e in imported {
        if current.len() >= MAX_ENTRIES { break; }
        if seen.insert((e.reading.clone(), e.word.clone())) {
            current.push(e.clone());
            added += 1;
        } else {
            duplicates += 1;
        }
    }
    (added, duplicates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(reading: &str, word: &str, pos: &str) -> Entry {
        Entry { reading: reading.into(), word: word.into(), pos: pos.into(), memo: String::new() }
    }

    #[test]
    fn own_and_microsoft_ime_lines_are_read_alike() {
        let text = "!Microsoft IME Dictionary Tool\r\n# 説明\r\nはにわたろう\t埴輪太郎\t人名\t試験の例\r\nハニワード\tHaniWord\t固有名詞\r\nかおもじ\t(^_^)\t顔文字\r\nなにか\t何か\tその他\r\n";
        let parsed = parse_tsv(text);
        assert_eq!(parsed.errors, Vec::<String>::new());
        assert_eq!(parsed.entries, vec![
            Entry { reading: "はにわたろう".into(), word: "埴輪太郎".into(), pos: "人名".into(), memo: "試験の例".into() },
            entry("はにわーど", "HaniWord", "固有名詞"),
            entry("かおもじ", "(^_^)", "短縮よみ"),
            entry("なにか", "何か", "名詞"),
        ]);
    }

    #[test]
    fn bad_lines_are_reported_and_skipped() {
        let long = "あ".repeat(MAX_READING + 1);
        let text = format!("はにわ1\t埴輪\t名詞\nそら\t\t名詞\n{long}\t長い\t名詞\nただひとつ\n\nそら\t空\t名詞\nそら\t空\t地名\n");
        let parsed = parse_tsv(&text);
        assert_eq!(parsed.entries, vec![entry("そら", "空", "名詞")]);
        assert_eq!(parsed.errors.len(), 5, "{:?}", parsed.errors);
        assert!(parsed.errors[0].starts_with("1 行目: 読みはひらがな"));
        assert!(parsed.errors[4].starts_with("7 行目: 同じ読みと語"));
    }

    #[test]
    fn files_are_decoded_from_common_encodings() {
        let text = "そら\t空\t名詞\r\n";
        let mut utf16le = vec![0xFF, 0xFE];
        utf16le.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
        let mut utf16be = vec![0xFE, 0xFF];
        utf16be.extend(text.encode_utf16().flat_map(|u| u.to_be_bytes()));
        let mut utf8bom = vec![0xEF, 0xBB, 0xBF];
        utf8bom.extend(text.as_bytes());
        for bytes in [utf16le, utf16be, utf8bom, text.as_bytes().to_vec()] {
            assert_eq!(decode(&bytes).unwrap(), text);
        }
        // Shift-JIS の そら<TAB>空<TAB>名詞
        let sjis = [0x82, 0xBB, 0x82, 0xE7, 0x09, 0x8B, 0xF3, 0x09, 0x96, 0xBC, 0x8E, 0x8C];
        #[cfg(windows)]
        assert_eq!(decode(&sjis).unwrap(), "そら\t空\t名詞");
        let _ = sjis;
        assert!(decode(&[0xFF, 0xFE, 0x41]).is_err(), "odd UTF-16");
    }

    #[test]
    fn written_table_reads_back_and_merge_skips_repeats() {
        let entries = vec![entry("そら", "空", "名詞"), Entry { memo: "メモ".into(), ..entry("はにわたろう", "埴輪太郎", "人名") }];
        assert_eq!(parse_tsv(&to_tsv(&entries)).entries, entries);
        let mut current = vec![entry("そら", "空", "名詞")];
        assert_eq!(merge(&mut current, &[entry("そら", "空", "名詞"), entry("うみ", "海", "名詞")]), (1, 1));
        assert_eq!(current.len(), 2);
        assert_eq!(normalize_reading(" ヴァイオリン "), "ゔぁいおりん");
        assert!(check(&entry("そら", "空", "動詞")).is_err());
    }
}
