// 英語と日本語の自動打ち分け。
//
// 打ったローマ字の列を、ローマ字の音節と英単語に区切り直す。英単語として扱うのは
// 「ローマ字として読めない綴り (google の gl, test の st, hello の l など) が英単語リストの語になっている」
// ときだけ。home・game・sushi のようにローマ字として読める英単語は日本語のまま (迷ったら日本語)。
//
// コスト: ローマ字の音節 0、英単語 1、読めない文字 3。いちばん安い区切り方を選ぶ。

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// ローマ字 (かなになる)。読めない文字もここに入れ、そのままローマ字変換に回す
    Romaji,
    /// 英単語 (英字のまま入れる)
    English,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub raw: String,
    pub kind: TokenKind,
}

/// 英単語リスト (小文字)。インストール先の english_words_scowl.txt と english_terms*.txt を読む
struct Words {
    set: HashSet<String>,
    sorted: Vec<String>,
    /// ローマ字として読めても英語を優先する語 (english_terms.txt の 4 文字以上: youtube = ようつべ, ipad = いぱd。3 文字は ram = らむ のように日本語に紛れるので入れない)
    priority: HashSet<String>,
    /// 3 文字の技術用語 (api・gui・ram)。語の切れ目に収まっているときだけ英語を優先する (segment_run の確かめ)
    short_priority: HashSet<String>,
    /// 2026-10-21: ローマ字として読めるが、かなが変換の辞書で日本語の語にならない英単語 (english_boundary_words.txt: issue = いっすえ・see = せえ)。
    /// short_priority と同じく、語の切れ目に収まっているときだけ英語にする
    boundary: HashSet<String>,
    /// 2026-10-13: english_terms.txt で頭に ^ を付けた名前 (yahoo = やほお)。並びの頭か、後ろが助詞・並びの終わりのときだけ英語を優先する
    /// (tsubura + yahoo + rudingu = 円谷ホールディングス の途中では優先しない)
    edge_priority: HashSet<String>,
    /// 利用者専用の英単語 (english_terms_personal.txt)。訓令式の規則の例外にしない (anki = Anki のまま)
    personal: HashSet<String>,
    /// ん を nn と打つ人の「n で終わる英単語 + n + 母音」(mainni = main + に) を英語と読む母音 (english_n_split.txt)。
    /// まいんい のように日本語の語にならないものだけ (tanni = たんい = 単位 は入っていない)
    n_split: HashMap<String, String>,
    latin: HashSet<String>,
    context: HashSet<String>,
    context_sorted: Vec<String>,
    /// 2026-10-25: 社名・製品名の表 (english_brands.txt) にしか無い語。打ち終えた語がそのまま当たったときだけ英字にし、
    /// 語の頭 (sorted・is_word_prefix) や語の途中の語 (segment_run_raw の inside_word・known_tail) には使わない。
    /// 1 万語の短い名前 (ORWO・LEDE・RSov) や名前の頭 (GStreamer の gst) が、ほかの語の途中にたまたま当たるため
    brands: HashSet<String>,
    /// 2026-11-06 (英単語＋する): サ変の形 (する・した・して …) のすぐ前なら英字にまとめる語 (english_suru_words.txt。release = れぇあせ・commit + する = こっみつる
    /// のように、ローマ字として読んだかなが変換の辞書で日本語の語にならない英単語。promote_before_suru)
    suru: HashSet<String>,
}

static WORDS: OnceLock<Words> = OnceLock::new();

/// この DLL (azookey.dll) が置かれているフォルダ。DllModule のロックを使わず、DLL 自身のアドレスから求める
pub(crate) fn dll_dir() -> Option<std::path::PathBuf> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::HMODULE,
            System::LibraryLoader::{
                GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            },
        },
    };
    unsafe {
        let mut module = HMODULE::default();
        let address = dll_dir as *const () as *const u16;
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(address),
            &mut module,
        )
        .ok()?;
        let mut buffer = [0u16; 1024];
        let length = GetModuleFileNameW(module, &mut buffer) as usize;
        if length == 0 {
            return None;
        }
        let path = std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..length]));
        path.parent().map(|p| p.to_path_buf())
    }
}

/// 書き込むファイル (覚えた語・予測の記録) と利用者専用の語の置き場 (%APPDATA%\HaniWord)。
/// IME 本体 (この DLL) は管理者のアプリにも読み込まれるので、配布版ではユーザーが書き換えられない Program Files に置く。
/// その隣には書けないので、書くものはここに分ける
/// 2026-10-14 (脆弱性の再調査 #5): 試験用の環境変数は試験のビルドでだけ読む。本番では、利用者の環境変数で
/// 管理者権限のアプリの中の表の置き場所を差し替えられないように
fn test_env(name: &str) -> Option<String> {
    if cfg!(test) { std::env::var(name).ok() } else { None }
}

pub(crate) fn data_dir() -> Option<std::path::PathBuf> {
    if !shared::winsec::private_data_allowed() { return None; }
    if let Some(dir) = test_env("AZK_WORDS_DIR") {
        return Some(std::path::PathBuf::from(dir));
    }
    // 2026-11-10: 試験のビルドでは一時フォルダ (shared::app_data_dir。予測の履歴・直さない綴りを、この PC の本物の置き場に書かない)
    shared::app_data_dir()
}

/// 2026-11-08: 設定の画面で学習をまっさらにした印 (%APPDATA%\HaniWord\learning_reset.txt。変換サーバーが書く) の更新時刻。無ければ None。
/// 学習をメモリに持つ所 (予測の候補・直さない英語の綴り) は、読んだときの値と違えば読み直す (まっさらにする前の学習を書き戻さないように)
pub(crate) fn learning_reset_mark() -> Option<std::time::SystemTime> {
    let path = data_dir()?.join("learning_reset.txt");
    std::fs::metadata(path).ok()?.modified().ok()
}

fn load_words() -> Option<Words> {
    let mut set = HashSet::new();
    let mut priority = HashSet::new();
    let mut short_priority = HashSet::new();
    let mut edge_priority = HashSet::new();
    let mut personal = HashSet::new();
    let mut brands = HashSet::new();
    let dir = test_env("AZK_WORDS_DIR").map(std::path::PathBuf::from).or_else(dll_dir)?;
    let n_split: HashMap<String, String> = std::fs::read_to_string(dir.join("english_n_split.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t').map(|(w, v)| (w.trim().to_ascii_lowercase(), v.trim().to_string())))
        .collect();
    let data = data_dir();
    // 2026-10-24: english_brands.txt は社名・製品名 (Wikidata から作った表。ローマ字として読めない語だけ)。判定の語に入れるだけで、優先 (priority) にはしない。語の頭・語の途中の語を探すときには使わない (Words の brands)
    // 2026-11-11: 製品名の表 (english_products.txt) の 1 語の表記 (iTunes) と、空白入りの表記をつないだ綴り (davinciresolve) も、社名・製品名の表と同じ扱い
    for name in ["english_words_scowl.txt", "english_terms.txt", "english_terms_personal.txt", "english_romaji_priority.txt", "english_case.txt", "english_brands.txt", "english_products.txt"] {
        if name == "english_terms_personal.txt" && data.is_none() { continue; }
        // 利用者専用の語は、利用者が書き足すので書き込める場所から読む
        let path = match (name, &data) {
            ("english_terms_personal.txt", Some(data)) => data.join(name),
            _ => dir.join(name),
        };
        if let Ok(text) = std::fs::read_to_string(path) {
            let joined: Vec<String> = if name == "english_products.txt" {
                text.lines().map(|l| l.trim().replace(' ', "")).filter(|l| !l.starts_with('#')).collect()
            } else { vec![] };
            let lines: Vec<&str> = if name == "english_products.txt" { joined.iter().map(String::as_str).collect() } else { text.lines().collect() };
            for line in lines {
                let mut w = line.trim();
                if name == "english_terms.txt" && w.starts_with('^') {
                    w = &w[1..];
                    if !w.is_empty() && w.chars().all(|c| c.is_ascii_alphabetic()) {
                        edge_priority.insert(w.to_ascii_lowercase());
                        set.insert(w.to_ascii_lowercase());
                    }
                    continue;
                }
                if w.is_empty() || w.starts_with('#') || !w.chars().all(|c| c.is_ascii_alphabetic()) {
                    continue;
                }
                // ローマ字として読めても英語を優先する語: english_terms.txt の 4 文字以上と、かなにすると日本語にならない語 (feature = ふぇあつれ)
                if (name == "english_terms.txt" && w.len() >= 4) || name == "english_romaji_priority.txt"
                    || (name == "english_case.txt" && case_entry_priority(w)) {
                    priority.insert(w.to_ascii_lowercase());
                } else if name == "english_terms.txt" && w.len() == 3 {
                    short_priority.insert(w.to_ascii_lowercase());
                }
                if name == "english_terms_personal.txt" {
                    personal.insert(w.to_ascii_lowercase());
                }
                // 社名・製品名の表は最後に読むので、ほかの表に無い語だけが残る
                if (name == "english_brands.txt" || name == "english_products.txt") && !set.contains(&w.to_ascii_lowercase()) {
                    brands.insert(w.to_ascii_lowercase());
                }
                set.insert(w.to_ascii_lowercase());
            }
        }
    }
    let boundary: HashSet<String> = std::fs::read_to_string(dir.join("english_boundary_words.txt")).unwrap_or_default().lines()
        .map(str::trim).filter(|w| !w.is_empty() && !w.starts_with('#') && w.chars().all(|c| c.is_ascii_lowercase()))
        .map(str::to_string).collect();
    for w in &boundary { set.insert(w.clone()); brands.remove(w); }
    let suru: HashSet<String> = std::fs::read_to_string(dir.join("english_suru_words.txt")).unwrap_or_default().lines()
        .map(str::trim).filter(|w| w.len() >= 4 && !w.starts_with('#') && w.chars().all(|c| c.is_ascii_lowercase()))
        .map(str::to_string).collect();
    let mut latin = HashSet::new();
    add_latin_words(&mut latin, &dir, &selected_latin_languages());
    let mut context = parse_context_words(&std::fs::read_to_string(dir.join("english_words_context.txt")).unwrap_or_default());
    // 2026-11-01: よく打つ分野の略語 (mRNA・NSAIDs・DBMS) は、文脈用の表の語と同じく、語の切れ目 (前は並びの頭か助詞、後ろは並びの終わりか
    // 助詞) に収まるときだけ英字の語にする。判定の語の表に入れると、3 字・4 字の略語が語の途中や打ち間違いの並びを取った
    // (na + dna の nad・escaping の esca・atrasii の atra・kdasai の kda に近い tno・nak・mta。例文の打鍵 11,964 件で 68〜99 件変わった)
    let domain_only: HashMap<String, String> = domain_abbreviations().iter()
        .filter(|(lower, _)| !set.contains(lower) && !context.contains(lower)).cloned().collect();
    context.extend(domain_only.keys().cloned());
    let _ = DOMAIN_ONLY.set(domain_only);
    let mut context_sorted: Vec<_> = context.iter().cloned().collect();
    context_sorted.sort();
    crate::debug_log::write(&format!(
        "auto_english: {} words from {dir:?} (google={}, windows={})",
        set.len(),
        set.contains("google"),
        set.contains("windows")
    ));
    if set.is_empty() {
        return None;
    }
    // 表が読めたときだけ足す (読めないときは足さずに None を返し、次の打鍵でもう一度読む)
    add_unit_words(&mut set);
    let mut sorted: Vec<String> = set.iter().filter(|w| !brands.contains(*w)).cloned().collect();
    sorted.sort();
    Some(Words { set, sorted, priority, short_priority, boundary, edge_priority, personal, n_split, latin, context, context_sorted, brands, suru })
}

fn selected_latin_languages() -> Vec<String> {
    #[cfg(test)]
    { std::env::var("AZK_TEST_LATIN_LANGUAGES").unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(str::to_string).collect() }
    #[cfg(not(test))]
    { super::diacritics::languages().to_vec() }
}

fn add_latin_words(set: &mut HashSet<String>, dir: &std::path::Path, languages: &[String]) {
    // Only configured languages, and never use configuration strings as paths.
    for code in ["fr", "de", "es", "it", "pt", "nordic", "pl", "cs", "tr"] {
        if !languages.iter().any(|language| language == code) { continue; }
        let Ok(text) = std::fs::read_to_string(dir.join("latin_words").join(format!("{code}.txt"))) else { continue; };
        for word in text.lines().map(str::trim) {
            if !word.is_empty() && word.chars().count() <= 30 && word.chars().all(|c| c.is_ascii_alphabetic() || is_accented_latin(c)) {
                set.insert(word.to_lowercase());
            }
        }
    }
}

// Multi-part names such as SuperMario and PowerShell are explicit English terms.
// Title case alone must not force Japanese-compatible words such as Anki.
fn case_entry_priority(word: &str) -> bool {
    word.len() >= 6 && word.chars().skip(1).any(|c| c.is_ascii_uppercase())
}

fn words() -> &'static Words {
    static EMPTY: OnceLock<Words> = OnceLock::new();
    if let Some(words) = WORDS.get() {
        return words;
    }
    // 読めなかったときは覚えずに、次の打鍵でもう一度読む
    match load_words() {
        Some(words) => WORDS.get_or_init(|| words),
        None => EMPTY.get_or_init(|| Words { set: HashSet::new(), sorted: vec![], priority: HashSet::new(), short_priority: HashSet::new(), boundary: HashSet::new(), edge_priority: HashSet::new(), personal: HashSet::new(), n_split: HashMap::new(), latin: HashSet::new(), context: HashSet::new(), context_sorted: vec![], brands: HashSet::new(), suru: HashSet::new() }),
    }
}

fn is_word(w: &str) -> bool {
    (words().set.contains(w) || words().latin.contains(w) || learned_english(w) || session_english(w)) && !learned_kana(w)
}

/// 2026-10-25: 文脈用の表 (english_words_context.txt。SCOWL の size 60) にしか無い 3 字以上の語で、ローマ字として読めないもの (bot・app・spam)。
/// 読める語 (emoji・meme・repo) は今までどおり日本語。打っている途中の末尾 (bot の t) は打ちかけとして読む
fn context_only_word(w: &str, at_end: bool) -> bool {
    w.len() >= 3 && !is_word(w) && words().context.contains(w) && !learned_kana(w) && !native_romaji_readable(w, at_end && !FINISHING.get())
}

/// 2026-10-25: 社名・製品名の表にしか無い語を除いた is_word (語の途中にたまたま当たる語を探すとき)
fn is_lexicon_word(w: &str) -> bool {
    is_word(w) && !words().brands.contains(w)
}

fn is_detection_word(w: &str, at_end: bool) -> bool {
    // Short native-language words wait for a word boundary or explicit English context.
    if !is_word(w) { return false; }
    if words().set.contains(w) || learned_english(w) || session_english(w) { return true; }
    (w.len() >= 4 || (FINISHING.get() && at_end)) && !native_romaji_readable(w, at_end && !FINISHING.get())
}

fn native_romaji_readable(word: &str, pending: bool) -> bool {
    if !word.is_ascii() { return false; }
    let bytes = word.as_bytes();
    let mut readable = vec![false; bytes.len() + 1];
    readable[0] = true;
    for i in 0..bytes.len() {
        if !readable[i] { continue; }
        let rest = &word[i..];
        // Foreign-language dictionaries must not turn legitimate ヴ kana into English.
        for syl in SYLLABLES.iter().chain(["va", "vi", "vu", "ve", "vo"].iter()) {
            if rest.starts_with(syl) { readable[i + syl.len()] = true; }
        }
        if bytes[i] == b'n' && (i + 1 == bytes.len() || (!is_vowel(bytes[i + 1]) && bytes[i + 1] != b'y')) {
            readable[i + 1] = true;
        }
        if i + 1 < bytes.len() && ((bytes[i] == bytes[i + 1] && !is_vowel(bytes[i]) && bytes[i] != b'n')
            || (bytes[i] == b't' && bytes[i + 1] == b'c')) {
            readable[i + 1] = true;
        }
        if pending && is_syllable_prefix(rest) { readable[bytes.len()] = true; }
    }
    readable[bytes.len()]
}

// One reverse pass also supplies the suffix evidence used at English/Japanese boundaries.
fn native_romaji_suffixes(word: &str, pending: bool) -> Vec<bool> {
    let bytes = word.as_bytes();
    let mut readable = vec![false; bytes.len() + 1];
    readable[bytes.len()] = true;
    for i in (0..bytes.len()).rev() {
        let rest = &word[i..];
        // Foreign-language dictionaries must not turn legitimate ヴ kana into English.
        for syl in SYLLABLES.iter().chain(["va", "vi", "vu", "ve", "vo"].iter()) {
            if rest.starts_with(syl) && readable[i + syl.len()] { readable[i] = true; break; }
        }
        if bytes[i] == b'n' && (i + 1 == bytes.len() || (!is_vowel(bytes[i + 1]) && bytes[i + 1] != b'y')) {
            readable[i] |= readable[i + 1];
        }
        if i + 1 < bytes.len() && ((bytes[i] == bytes[i + 1] && !is_vowel(bytes[i]) && bytes[i] != b'n')
            || (bytes[i] == b't' && bytes[i + 1] == b'c')) {
            readable[i] |= readable[i + 1];
        }
        if pending && is_syllable_prefix(rest) { readable[i] = true; }
    }
    readable
}

/// ^ を付けた名前 (yahoo) が lower[i..j] にあり、並びの頭か、後ろが並びの終わり・助詞 (yahoo + de・no・ni …) なら真
fn edge_priority_here(lower: &str, i: usize, j: usize) -> bool {
    let w = &lower[i..j];
    if !words().edge_priority.contains(w) || learned_kana(w) { return false; }
    let rest = &lower[j..];
    i == 0 || rest.is_empty() || !rest.as_bytes()[0].is_ascii_alphabetic()
        || STRONG_BOUNDARY.iter().chain(["ka", "ya", "wa", "kara", "made", "yori", "tte", "toka", "dake", "demo", "nado"].iter()).any(|p| rest.starts_with(p))
}

fn is_priority(w: &str) -> bool {
    (words().priority.contains(w) || learned_english(w) || session_english(w)) && !learned_kana(w)
}

/// 区切ったあとで、ローマ字の音節の並びがちょうど 3 文字の技術用語 (api = あ・ぴ) になっていて、前後が語の切れ目なら英語にまとめる。
/// 区切りの計算の中で優先すると、日本語の語の途中 (へあぴん・しかしながらむ) まで英語にしてしまうので、あとからまとめるだけにする
/// 2026-10-21: かなが日本語の語にならない英単語 (english_boundary_words.txt: no + issue + wo・文の頭の see) も同じ。
/// 入力の終わりに来たときは、打ち終えた (Space・Enter) ときか記号の前だけ (打っている途中は日本語の語の頭かもしれない: soda → そだつ)
fn promote_short_terms(tokens: Vec<Token>) -> Vec<Token> {
    if words().short_priority.is_empty() && words().boundary.is_empty() {
        return tokens;
    }
    let mut out: Vec<Token> = vec![];
    let mut i = 0;
    while i < tokens.len() {
        let mut merged = false;
        for len in 1..=8 {
            if i + len > tokens.len() || tokens[i..i + len].iter().any(|t| t.kind != TokenKind::Romaji) {
                continue;
            }
            let raw: String = tokens[i..i + len].iter().map(|t| t.raw.as_str()).collect();
            let w = raw.to_ascii_lowercase();
            let short = w.len() == 3 && words().short_priority.contains(&w);
            // 前が助詞の音節というだけでは語の切れ目と決められない (その + gore + の・に + dome + の: その・二度目 の中の の・に)。
            // 前は 並びの頭・英単語・英単語のすぐ後ろの助詞 (github + no + issue) のときだけ
            let left_hard = match out.len() {
                0 => true,
                k => out[k - 1].kind == TokenKind::English
                    || (k >= 2 && out[k - 2].kind == TokenKind::English && SHORT_TERM_BOUNDARY.contains(&out[k - 1].raw.to_ascii_lowercase().as_str())),
            };
            let boundary = words().boundary.contains(&w) && raw.bytes().all(|b| b.is_ascii_lowercase()) && left_hard
                && (i + len < tokens.len() || FINISHING.get() || !RUN_AT_END.get());
            if (short || boundary) && !learned_kana(&w) {
                // ssh can still be an unfinished っし/っしゃ inside Japanese.
                if i + len == tokens.len() && !FINISHING.get()
                    && w.as_bytes()[0] == w.as_bytes()[1] && is_syllable_prefix(&w[1..]) {
                    continue;
                }
                let mut candidate = out.clone();
                candidate.push(Token { raw: raw.clone(), kind: TokenKind::English });
                let at = candidate.len() - 1;
                candidate.extend(tokens[i + len..].iter().cloned());
                // 2026-10-21: かなが日本語の語にならない英単語は、後ろの助詞を打ち終えてから (ga + house + n の n は 方線 の途中かもしれない)
                if short_term_fits_with(&candidate, at, short) {
                    out.push(Token { raw, kind: TokenKind::English });
                    i += len;
                    merged = true;
                    break;
                }
            }
        }
        if !merged {
            out.push(tokens[i].clone());
            i += 1;
        }
    }
    out
}

/// 3 文字の技術用語が語の切れ目に収まっているか: 前は 打ち始め・英単語・助詞、後ろは 打ち終わり・英単語・助詞
fn short_term_fits(tokens: &[Token], index: usize) -> bool {
    short_term_fits_with(tokens, index, true)
}

/// 語の切れ目とみなす助詞 (short_term_fits)
const SHORT_TERM_BOUNDARY: &[&str] = &["ha", "ga", "wo", "no", "ni", "de", "to", "mo", "he", "ya"];

/// pending_next: 後ろが打ちかけ (apiw の w) でも収まっているとみなす
fn short_term_fits_with(tokens: &[Token], index: usize, pending_next: bool) -> bool {
    const BOUNDARY: &[&str] = SHORT_TERM_BOUNDARY;
    let fits = |t: Option<&Token>| match t {
        None => true,
        Some(t) => t.kind == TokenKind::English || BOUNDARY.contains(&t.raw.to_ascii_lowercase().as_str()),
    };
    let next = tokens.get(index + 1);
    // 後ろが打ちかけ (apiw → を の途中) なら、まだ決めない (英語のまま)
    let next_pending = pending_next && next.is_some_and(|t| index + 2 == tokens.len() && is_syllable_prefix(&t.raw.to_ascii_lowercase()));
    fits(index.checked_sub(1).and_then(|i| tokens.get(i))) && (fits(next) || next_pending)
}

/// 2026-11-06 (英単語＋する): 語＋サ変の形がふつうの音節だけで読める語の左の切れ目 (助詞と、て形の て)
const SURU_LEFT_BOUNDARY: &[&str] = &["ha", "ga", "wo", "no", "ni", "de", "to", "mo", "he", "ya", "te"];

/// 2026-11-06 (英単語＋する): サ変の形の頭 (する・すれ・した・して・します・しない・される・させる・しよう)
const SURU_FORMS: &[&str] = &["suru", "sure", "sita", "shita", "site", "shite", "sima", "shima", "sina", "shina", "sare", "sase", "siyo", "shiyo"];

/// 2026-11-06 (英単語＋する、利用者の報告 releasesuru): サ変の形のすぐ前で終わる英単語を英字にまとめる。語は english_suru_words.txt にあるもの
/// (ローマ字として読んだかなが変換の辞書で日本語の語にならない英単語。tools/english_lists/make_suru_words.py) だけ。直す崩れ方は 2 つ:
/// - 語全体がローマ字として読める (re|le|a|se + suru = れぇあせする・de|le|te・sa|ve)
/// - t で終わる語の t が、サ変の形の頭と合わさって 1 つの音になる (co|m|mi|tsu|ru = こっみつる・e|di|tsu|ru)。その音は t と残り (su) に分ける
/// ためした (tame)・までした (made) のように、かなが日本語の語になる語は表に入らないので替えない。
/// 語＋サ変の形の全体がローマ字として読めない (re|star|tsu|ru = restart + する) なら日本語ではありえないので、表に無くても英単語の表の語なら英字に
/// (matsuru = まつる の mat は全体が読めるので、表で確かめる)。
/// 語は小文字で打った 4 字以上で、頭は区切りの境目 (音節の途中から始めない)。その語がすでに 1 つの英字の区切りなら何もしない。
/// 語＋サ変の形の全体がふつうの音節だけで読める語 (rake = らけ・kinda = きんだ・commit + suru = こっみつる) は、左が切れ目 (並びの頭・英単語・
/// 助詞・て) のときだけ (和文の語の途中から切り出さない: から + 消して が か + rake + して、平均 + 出して が へい + kinda + して になっていた)。
/// サ変の形のすぐ前がもう 1 つの英字の区切りなら、その前の字を巻き込まない (HaniWord + で + test + した を detest + した にしない)
fn promote_before_suru(run: &str, tokens: Vec<Token>) -> Vec<Token> {
    if words().suru.is_empty() { return tokens; }
    let lower = run.to_ascii_lowercase();
    let n = lower.len();
    let mut starts = Vec::with_capacity(tokens.len() + 1);
    let mut at = 0;
    for t in &tokens {
        starts.push(at);
        at += t.raw.len();
    }
    if at != n { return tokens; }
    starts.push(n);
    let suru_at = |e: usize| SURU_FORMS.iter().find(|f| lower[e..].starts_with(*f)).map(|f| e + f.len());
    for k in 1..tokens.len() {
        if tokens[k].kind != TokenKind::Romaji { continue; }
        let raw = tokens[k].raw.to_ascii_lowercase();
        // サ変の形が始まる所: 区切りの頭 (release + suru) か、t で始まる音の 2 字目 (commi + tsu + ru の s・edi + tsi + ta の s)
        let split_t = raw.len() >= 2 && raw.starts_with('t') && suru_at(starts[k] + 1).is_some();
        let e = if split_t { starts[k] + 1 } else { starts[k] };
        let Some(form_end) = suru_at(e) else { continue };
        let Some(j) = (0..k).find(|&j| {
            let w = &lower[starts[j]..e];
            if e - starts[j] < 4 || !run.as_bytes()[starts[j]..e].iter().all(|b| b.is_ascii_lowercase()) || learned_kana(w) { return false; }
            let readable = native_romaji_readable(&lower[starts[j]..form_end], false);
            let left_fits = j == 0 || tokens[j - 1].kind == TokenKind::English
                || SURU_LEFT_BOUNDARY.contains(&tokens[j - 1].raw.to_ascii_lowercase().as_str());
            let keeps_english = !split_t && tokens[k - 1].kind == TokenKind::English && starts[k - 1] > starts[j];
            (words().suru.contains(w) || (is_lexicon_word(w) && !readable)) && (!readable || left_fits) && !keeps_english
        }) else { continue };
        if !split_t && k == j + 1 && tokens[j].kind == TokenKind::English { continue; }
        let mut out: Vec<Token> = tokens[..j].to_vec();
        out.push(Token { raw: run[starts[j]..e].to_string(), kind: TokenKind::English });
        if split_t {
            out.push(Token { raw: run[e..starts[k + 1]].to_string(), kind: TokenKind::Romaji });
            out.extend_from_slice(&tokens[k + 1..]);
        } else {
            out.extend_from_slice(&tokens[k..]);
        }
        // まとめた語のあとにも、もう一度 (release して commit する)。まとめた所は 1 つの英字の区切りになったので、同じ所では止まる
        return promote_before_suru(run, out);
    }
    tokens
}

/// 英数モードで打った語のうち、かなで打つと崩れる語 (keltype → けltype)。
/// パスワードを打つこともあるのでファイルには書かず、このアプリを開いている間だけ覚える。
/// かな入力でこの語を打って英字のまま確定したら、learn_from_commit がファイルに書く
static SESSION_ENGLISH: std::sync::Mutex<Option<HashSet<String>>> = std::sync::Mutex::new(None);

fn session_english(w: &str) -> bool {
    SESSION_ENGLISH.lock().map_or(false, |s| s.as_ref().is_some_and(|set| set.contains(w)))
}

#[cfg(test)]
pub(crate) fn session_english_for_test(w: &str) -> bool {
    session_english(w)
}

/// ローマ字として最後まで読めるか (読めない文字も英単語も入らない)
pub fn reads_as_romaji(word: &str) -> bool {
    let _learning = SegmentLearning::new();
    let (tokens, flags) = segment_run_raw(word, style());
    tokens.iter().all(|t| t.kind == TokenKind::Romaji) && !flags.iter().any(|f| *f)
}

/// 英数モードで打ち終えた語を受け取り、覚える対象 (かなで打つと崩れる語) ならメモリに覚えて true
pub fn note_latin_word(word: &str) -> bool {
    if word.len() < 3 || word.len() > 30 || !word.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let w = word.to_ascii_lowercase();
    if words().set.contains(&w) || learned_kana(&w) || learned_english(&w) || reads_as_romaji(&w) {
        return false;
    }
    if let Ok(mut guard) = SESSION_ENGLISH.lock() {
        let set = guard.get_or_insert_with(HashSet::new);
        if set.len() < 1000 {
            set.insert(w);
        }
    }
    true
}

/// 和文の中で英数モードにして打った語をファイルに書く (次からかな入力でも英字)
pub fn persist_english(word: &str) {
    if super::direct_field::private_field() {
        return;
    }
    let w = word.to_ascii_lowercase();
    if !english_learning_eligible(&w) || learned_english(&w) {
        return;
    }
    append_learned(&[format!("+{w}")]);
}

fn append_learned(lines: &[String]) {
    // 2026-10-20 (Codex の再調査 M1): 覚えてはいけない欄 (IS_PRIVATE) では、どの経路からも書かない
    if cfg!(test) || super::direct_field::private_field() { return; }
    let Some(path) = learned_path() else { return };
    crate::debug_log::write(&format!("auto_english: learned {lines:?}"));
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", lines.join("\n"));
    }
}

// Ordinary romaji must not become English merely because it was committed literally.
fn english_learning_eligible(word: &str) -> bool {
    (3..=30).contains(&word.len()) && word.bytes().all(|b| b.is_ascii_lowercase())
        && !native_romaji_readable(word, false)
}

/// + は既存の英字学習、? は明示的な英字変換の確認、- はかなへの訂正。
#[derive(Default)]
struct Learned {
    english: HashSet<String>,
    kana: HashSet<String>,
    confirmations: HashMap<String, u8>,
    modified: Option<std::time::SystemTime>,
    path: Option<std::path::PathBuf>,
}

impl Learned {
    fn matches(&self, path: &Option<std::path::PathBuf>, modified: Option<std::time::SystemTime>) -> bool {
        self.path == *path && self.modified == modified
    }
}

static LEARNED: std::sync::Mutex<Option<std::sync::Arc<Learned>>> = std::sync::Mutex::new(None);

thread_local! {
    static SEGMENT_LEARNING: std::cell::RefCell<Option<std::sync::Arc<Learned>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    static LEARNED_REFRESH_QUERIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// Keep one authenticated learning snapshot throughout a classification pass.
struct SegmentLearning(bool);

impl SegmentLearning {
    fn new() -> Self {
        let owns_snapshot = SEGMENT_LEARNING.with_borrow(Option::is_none);
        if owns_snapshot {
            let learned = refresh_learned();
            SEGMENT_LEARNING.with_borrow_mut(|snapshot| *snapshot = Some(learned));
        }
        Self(owns_snapshot)
    }
}

impl Drop for SegmentLearning {
    fn drop(&mut self) {
        if self.0 {
            SEGMENT_LEARNING.with_borrow_mut(|snapshot| *snapshot = None);
        }
    }
}

fn learned_path() -> Option<std::path::PathBuf> {
    if !shared::winsec::private_data_allowed() { return None; }
    if cfg!(test) {
        return std::env::var_os("AZK_TEST_LEARNED").map(std::path::PathBuf::from)
            .filter(|path| !path.components().any(|part| {
                let name = part.as_os_str().to_string_lossy();
                name.eq_ignore_ascii_case("AppData") || name.eq_ignore_ascii_case("dist")
            }));
    }
    Some(data_dir()?.join("english_learned.txt"))
}

/// ほかのアプリで覚えた語も使えるように、判定ごとにファイルの更新を確認する
fn refresh_learned() -> std::sync::Arc<Learned> {
    #[cfg(test)]
    LEARNED_REFRESH_QUERIES.set(LEARNED_REFRESH_QUERIES.get() + 1);
    let path = learned_path();
    let modified = path.as_ref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok());
    let mut guard = LEARNED.lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().map_or(true, |l| !l.matches(&path, modified)) {
        let text = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let mut learned = parse_learned(&text);
        learned.modified = modified;
        learned.path = path;
        *guard = Some(std::sync::Arc::new(learned));
    }
    std::sync::Arc::clone(guard.as_ref().unwrap())
}

fn with_learned<T>(f: impl FnOnce(&Learned) -> T) -> T {
    let snapshot = SEGMENT_LEARNING.with_borrow(Clone::clone).unwrap_or_else(refresh_learned);
    f(&snapshot)
}

fn parse_learned(text: &str) -> Learned {
    let mut learned = Learned::default();
    // 後の行が勝つ (英字 → かな → 英字 と直したら英字)
    for line in text.lines() {
        let line = line.trim();
        if let Some(w) = line.strip_prefix('+') {
            if !english_learning_eligible(w) { continue; }
            learned.kana.remove(w);
            learned.english.insert(w.to_string());
            learned.confirmations.remove(w);
        } else if let Some(w) = line.strip_prefix('?') {
            if !english_learning_eligible(w) { continue; }
            let count = learned.confirmations.entry(w.to_string()).or_default();
            *count = count.saturating_add(1).min(3);
            if *count == 3 {
                learned.kana.remove(w);
                learned.english.insert(w.to_string());
            }
        } else if let Some(w) = line.strip_prefix('-') {
            learned.english.remove(w);
            learned.confirmations.remove(w);
            learned.kana.insert(w.to_string());
        }
    }
    learned
}

fn learned_english(w: &str) -> bool {
    with_learned(|l| l.english.contains(w))
}

fn learned_kana(w: &str) -> bool {
    with_learned(|l| l.kana.contains(w))
}

/// 確定した内容から、利用者が直した語を覚える。`typed` は打った文字の列、`committed` は確定した文字列
pub fn learn_from_commit(typed: &str, committed: &str, explicit_latin: bool) {
    let lines = learning_updates(typed, committed, explicit_latin);
    if !lines.is_empty() { append_learned(&lines); }
}

fn learning_updates(typed: &str, committed: &str, explicit_latin: bool) -> Vec<String> {
    let tokens = segment(typed);
    let lower_commit = committed.to_ascii_lowercase();
    let mut lines = vec![];
    // 英字のまま確定した語: 英単語と判定していなかったのに英字で確定した (F10・候補から英字を選んだ)
    // アクセント付きの文字も語の一部 (España から Espa を切り出さない)。アクセント付きの文字を含む語はもともと英語なので覚えない
    for word in committed.split(|c: char| !c.is_ascii_alphabetic() && !is_accented_latin(c)) {
        if word.chars().any(is_accented_latin) {
            continue;
        }
        let w = word.to_ascii_lowercase();
        if !explicit_latin || !english_learning_eligible(&w) || !typed.to_ascii_lowercase().contains(&w) {
            continue;
        }
        // ローマ字の打ちかけで終わっているだけの語 (syanp = しゃん + p) は覚えない (候補で英字を選んでしまっただけのことが多い)
        if !w.ends_with(|c: char| "aiueo".contains(c)) && reads_as_romaji(&w) {
            continue;
        }
        let detected = automatically_detected_english(&tokens, &w);
        // 英数モードで打った語を、かな入力でも英字のまま確定した → ファイルに書いて次からも英字
        if (!detected || session_english(&w)) && !learned_english(&w) {
            lines.push(format!("?{w}"));
        }
    }
    // 英単語と判定したのにかなで確定した語 (F6・F7・無変換で直した)
    for t in tokens.iter().filter(|t| t.kind == TokenKind::English) {
        let w = t.raw.to_ascii_lowercase();
        if t.raw.chars().any(|c| c.is_ascii_uppercase()) || w.len() < 3 {
            continue;
        }
        // その語だけを打ったときに限る (texi を て+ぃ に直したとき、tex をかなと覚えない)
        if !typed.split(|c: char| !c.is_ascii_alphabetic()).any(|run| run.eq_ignore_ascii_case(&t.raw)) {
            continue;
        }
        if !lower_commit.contains(&w) && !learned_kana(&w) {
            lines.push(format!("-{w}"));
        }
    }
    lines
}

fn automatically_detected_english(tokens: &[Token], word: &str) -> bool {
    let mut span = String::new();
    for token in tokens {
        if token.kind == TokenKind::English && token.raw.chars().all(|c| c.is_ascii_alphabetic()) {
            span += &token.raw.to_ascii_lowercase();
        } else {
            if span == word { return true; }
            span.clear();
        }
    }
    span == word
}

fn is_word_prefix(p: &str) -> bool {
    let sorted = &words().sorted;
    let i = sorted.partition_point(|w| w.as_str() < p);
    sorted.get(i).is_some_and(|w| w.starts_with(p))
}

fn parse_context_words(text: &str) -> HashSet<String> {
    text.lines().map(str::trim).filter(|w| !w.is_empty() && w.len() <= 32
        && w.chars().any(|c| c.is_ascii_alphabetic())
        && w.chars().all(|c| c.is_ascii_alphabetic() || "'-".contains(c)))
        .map(|w| w.to_ascii_lowercase()).collect()
}

/// 2026-10-13: 行頭の / のあと (prefix_direct) で使う。英単語・文脈用の単語表の語の頭か
pub fn english_word_prefix(prefix: &str) -> bool {
    is_word_prefix(prefix) || context_word_prefix(prefix)
}

/// 2026-10-13: ローマ字で日本語として読めるか (末尾の打ちかけは可)
pub fn romaji_readable_prefix(word: &str) -> bool {
    native_romaji_readable(word, true)
}

fn context_word_prefix(prefix: &str) -> bool {
    let sorted = &words().context_sorted;
    let index = sorted.partition_point(|w| w.as_str() < prefix);
    sorted.get(index).is_some_and(|w| w.starts_with(prefix))
}

// ローマ字の音節 (v 行を含む)。l・x で打つ小書き (la・ltu・xa など) はここに入れず、下の SMALL_KANA で扱う
const SYLLABLES: &[&str] = &[
    "a", "i", "u", "e", "o", "ka", "ki", "ku", "ke", "ko", "sa", "si", "su", "se", "so", "ta", "ti", "tu",
    "te", "to", "na", "ni", "nu", "ne", "no", "ha", "hi", "hu", "he", "ho", "ma", "mi", "mu", "me", "mo",
    "ya", "yu", "ye", "yo", "ra", "ri", "ru", "re", "ro", "wa", "wi", "we", "wo", "ga", "gi", "gu", "ge",
    "go", "za", "zi", "zu", "ze", "zo", "da", "di", "du", "de", "do", "ba", "bi", "bu", "be", "bo", "pa",
    "pi", "pu", "pe", "po", "shi", "chi", "tsu", "fu", "ji", "ja", "ju", "jo", "je", "sha", "shu", "sho",
    "she", "cha", "chu", "cho", "che", "kya", "kyu", "kyo", "sya", "syu", "syo", "tya", "tyu", "tyo",
    "cya", "cyu", "cyo", "nya", "nyu", "nyo", "hya", "hyu", "hyo", "mya", "myu", "myo", "rya", "ryu",
    "ryo", "gya", "gyu", "gyo", "zya", "zyu", "zyo", "jya", "jyu", "jyo", "dya", "dyu", "dyo", "bya",
    "byu", "byo", "pya", "pyu", "pyo", "fa", "fi", "fe", "fo", "va", "vi", "vu", "ve", "vo", "tsa", "tsi", "tse", "tso", "thi", "thu",
    "dhi", "dhu", "twu", "dwu", "wha", "whi", "whe", "who", "kwa", "gwa", "nn", "ca", "cu", "co", "ce", "ci", "qa", "qi",
    "qe", "qo", "tye", "zye", "jye", "jya", "jyu", "jyo", "sye", "kye", "gye", "nye", "hye", "bye", "pye", "mye", "rye", "tsa",
];

/// Explicit small-kana commands; ordinary English remains cheaper unless they join a valid kana unit.
const SMALL_KANA: &[&str] = &[
    "xa", "xi", "xu", "xe", "xo", "xya", "xyu", "xyo", "xtu", "xtsu", "xwa", "xka", "xke",
    "la", "li", "lu", "le", "lo", "lya", "lyu", "lyo", "ltu", "ltsu", "lwa", "lka", "lke",
];

/// めったに使わない綴り (てゅ・でぃ・くぁ …)。英単語の一部として読めるときは英語を優先する (github = gi・thu・b)
/// 2026-10-20: c 行 (ca cu co ce ci = か く こ せ し) はここから外し、音ごとの推定 (ADAPTIVE) に任せる。
/// 前は ca cu co が音節の表に無く ce ci もこのコスト 4 で、c を含む英単語 (can・con・cute・taco) が日本語の途中でも勝っていた (cansoku → can + そく)
const RARE: &[&str] = &[
    "thi", "thu", "dhi", "dhu", "twu", "dwu", "wha", "whi", "whe", "who", "kwa", "gwa", "qa", "qi",
    "qe", "qo", "tsa", "tsi", "tse", "tso",
];

// Supported by the converter, but weak Japanese evidence during language detection.
// This is a conservative prior, not a measured population typing frequency.
const UNCOMMON_SMALL_KANA: &[&str] = &[
    "swa", "swi", "swu", "swe", "swo",
    "tha", "the", "tho", "dha", "dhe", "dho",
    "twa", "twi", "twe", "two", "dwa", "dwi", "dwe", "dwo",
    "jyi", "tyi", "cyi", "dyi", "nyi", "hyi", "byi", "pyi", "myi", "ryi",
    "hwa", "hwi", "hwe", "hwo", "fwa", "fwi", "fwu", "fwe", "fwo",
];

/// ヘボン式だけの綴り。訓令式の人 (romaji_style = "kunrei_nn") には英語のしるしになる
const HEPBURN_ONLY: &[&str] = &[
    "shi", "chi", "tsu", "ji", "ja", "ju", "jo", "je", "sha", "shu", "sho", "she", "cha", "chu", "cho", "che",
];

/// 2026-10-20: c 行の綴り (か く こ せ し)。訓令式にもヘボン式にも無いので、訓令式の人には英語のしるし (前と同じ)。
/// ほかの人は音ごとの推定 (ADAPTIVE) で、ふだん c 行を使わない人にだけ英語のしるしになる
const C_ROW: &[&str] = &["ca", "cu", "co", "ce", "ci"];

/// 音ごとの推定 (romaji_style::penalty) で加点する綴り: (音節, 音, 綴り)。英語の判定で、その人がまず使わない綴りを英語のしるしにする
const ADAPTIVE: &[(&str, &str, &str)] = &[
    ("si", "si", "si"), ("shi", "si", "shi"), ("ci", "si", "ci"),
    ("ti", "ti", "ti"), ("chi", "ti", "chi"),
    ("tu", "tu", "tu"), ("tsu", "tu", "tsu"),
    ("zi", "zi", "zi"), ("ji", "zi", "ji"),
    ("hu", "hu", "hu"), ("fu", "hu", "fu"),
    ("sya", "sya", "sy"), ("syu", "sya", "sy"), ("syo", "sya", "sy"),
    ("sha", "sya", "sh"), ("shu", "sya", "sh"), ("sho", "sya", "sh"),
    ("sye", "sye", "sy"), ("she", "sye", "sh"),
    ("tya", "tya", "ty"), ("tyu", "tya", "ty"), ("tyo", "tya", "ty"),
    ("cha", "tya", "ch"), ("chu", "tya", "ch"), ("cho", "tya", "ch"),
    ("cya", "tya", "cy"), ("cyu", "tya", "cy"), ("cyo", "tya", "cy"),
    ("tye", "tye", "ty"), ("che", "tye", "ch"),
    ("zya", "zya", "zy"), ("zyu", "zya", "zy"), ("zyo", "zya", "zy"),
    ("ja", "zya", "j"), ("ju", "zya", "j"), ("jo", "zya", "j"),
    ("jya", "zya", "jy"), ("jyu", "zya", "jy"), ("jyo", "zya", "jy"),
    ("zye", "zye", "zy"), ("je", "zye", "j"), ("jye", "zye", "jy"),
    ("fa", "fa", "f"), ("fi", "fa", "f"), ("fe", "fa", "f"), ("fo", "fa", "f"),
    // 2026-10-20: c 行 (ca = か・cu = く・co = こ・ce = せ。ci は上の si の組)
    ("ca", "ka", "ca"), ("cu", "ku", "cu"), ("co", "ko", "co"), ("ce", "se", "ce"),
    // ん を n 1 つ (+子音) で打つこと
    ("n", "n", "n"),
];

#[derive(Clone, Copy)]
struct Style {
    kunrei_nn: bool,
    /// 音ごとの推定による加点 (ADAPTIVE と同じ並び。コストの 1/100 単位)。None なら加点しない
    adaptive: Option<[u8; ADAPTIVE.len()]>,
}

const STANDARD: Style = Style { kunrei_nn: false, adaptive: None };

impl Style {
    /// 音節 `syl` (n 1 つの ん は "n") をローマ字として読むときに足すコスト (1/100 単位)
    fn penalty(&self, syl: &str) -> i64 {
        match self.adaptive {
            Some(table) => ADAPTIVE.iter().position(|(s, _, _)| *s == syl).map_or(0, |k| table[k] as i64),
            None => 0,
        }
    }
}

/// 今までの回数から加点の表を作る (回数が変わったときだけ計算し直す)
fn adaptive_table(stats: &super::romaji_style::Stats) -> [u8; ADAPTIVE.len()] {
    static CACHE: std::sync::Mutex<Option<(super::romaji_style::Stats, [u8; ADAPTIVE.len()])>> = std::sync::Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((s, table)) = guard.as_ref() {
        if s == stats {
            return *table;
        }
    }
    let mut table = [0u8; ADAPTIVE.len()];
    for (k, (_, group, variant)) in ADAPTIVE.iter().enumerate() {
        table[k] = (super::romaji_style::penalty(stats, group, variant) * 100.0).round() as u8;
    }
    *guard = Some((stats.clone(), table));
    table
}

/// ローマ字の打ち方 (設定 input.romaji_style): "kunrei_nn" / "standard" / "auto" (打った綴りから音ごとに判断、romaji_style.rs) /
/// "auto_binary" (以前の auto。訓令式＋nn かどうかの 2 値)
fn style() -> Style {
    static SETTING: OnceLock<String> = OnceLock::new();
    let setting = SETTING.get_or_init(|| {
        let style = test_env("AZK_ROMAJI_STYLE").unwrap_or_else(|| shared::AppConfig::read_or_default().input.romaji_style);
        crate::debug_log::write(&format!("auto_english: romaji_style={style}"));
        style
    });
    match setting.as_str() {
        "kunrei_nn" => Style { kunrei_nn: true, adaptive: None },
        "auto_binary" => Style { kunrei_nn: super::romaji_style::load().is_kunrei_nn(), adaptive: None },
        "auto" => Style { kunrei_nn: false, adaptive: Some(adaptive_table(&super::romaji_style::load())) },
        _ => STANDARD,
    }
}

/// 読みをローマ字に戻す・ん の 1 回目の n を見せるときの綴り (英語の判定とは別)。auto は音ごとに回数から選ぶ
pub fn spelling() -> super::romaji_style::Spelling {
    let style = style();
    if style.adaptive.is_some() {
        super::romaji_style::spelling(&super::romaji_style::load())
    } else {
        super::romaji_style::Spelling::uniform(style.kunrei_nn)
    }
}

/// 英単語の表記直し (github → GitHub) をするか。設定 input.english_case_fix (初期値はオン)
pub fn case_fix_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| shared::AppConfig::read_or_default().input.english_case_fix)
}

/// 2026-10-31: 決まった表記が全部大文字 (数字を含んでよい) の 2 字以上の略語か (dna = DNA・atp = ATP)。表記直しの表 (english_case.txt) の語だけで、
/// 社名・製品名の表にしか無い名前 (ZENT・TRES・ITER) は除く (zentai = 全体 を [zent] + あい にしていた)
fn caps_abbreviation(word: &str) -> bool {
    !words().brands.contains(word) && canonical_case(word).is_some_and(|form| form.len() >= 2 && form.bytes().any(|b| b.is_ascii_uppercase())
        && form.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
}

/// 小文字だけで打った英単語の決まった表記 (english_case.txt)。大文字を交ぜて打った語はそのまま
pub fn canonical_case(word: &str) -> Option<&'static str> {
    static TABLE: OnceLock<std::collections::HashMap<String, String>> = OnceLock::new();
    if !word.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    let table = TABLE.get_or_init(|| {
        let dir = test_env("AZK_WORDS_DIR").map(std::path::PathBuf::from).or_else(dll_dir);
        let read = |name: &str| dir.as_ref().and_then(|d| std::fs::read_to_string(d.join(name)).ok()).unwrap_or_default();
        // 2026-10-24: 社名・製品名の表 (english_brands.txt) の大文字を含む表記 (AQUOS・GitHub) も使う。english_case.txt に同じ語があればそちらが勝つ
        // 2026-11-11: 製品名の表の 1 語の表記 (iTunes・SharePoint) も。社名の表・表記直しの表に同じ語があればそちらが勝つ
        let mut table: std::collections::HashMap<String, String> = parse_case_table(&read("english_products.txt")).into_iter()
            .filter(|(_, form)| form.chars().any(|c| c.is_ascii_uppercase())).collect();
        table.extend(parse_case_table(&read("english_brands.txt")).into_iter()
            .filter(|(_, form)| form.chars().any(|c| c.is_ascii_uppercase())));
        // 2026-11-01: よく打つ分野の略語の表記 (mrna = mRNA・nsaids = NSAIDs)。表記直しの表に同じ語があればそちらが勝つ
        table.extend(domain_abbreviations().iter().cloned());
        table.extend(parse_case_table(&read("english_case.txt")));
        table
    });
    table.get(word).map(|s| s.as_str())
}

/// 2026-10-20: 普通の英単語と同じ綴りの名前 (english_case.txt の ~ で始まる行: windows・zoom・amazon)。
/// 和文にすき間なくつながっているとき (windowsno・zoomde) だけこの表記にする。英文の中や単独で打ったときは小文字のまま
pub fn japanese_context_case(word: &str) -> Option<&'static str> {
    static TABLE: OnceLock<std::collections::HashMap<String, String>> = OnceLock::new();
    if !word.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    let table = TABLE.get_or_init(|| {
        let dir = test_env("AZK_WORDS_DIR").map(std::path::PathBuf::from).or_else(dll_dir);
        let text = dir.and_then(|d| std::fs::read_to_string(d.join("english_case.txt")).ok()).unwrap_or_default();
        parse_japanese_context_case(&text)
    });
    table.get(word).map(|s| s.as_str())
}

fn parse_japanese_context_case(text: &str) -> std::collections::HashMap<String, String> {
    parse_case_table(&text.lines().filter_map(|l| l.trim().strip_prefix('~')).collect::<Vec<_>>().join("\n"))
}

/// 表の 1 行 = 1 表記。綴りが英字だけのものに限る (打った文字数と変わらないように)
fn parse_case_table(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|w| !w.is_empty() && !w.starts_with('#') && w.chars().all(|c| c.is_ascii_alphabetic()))
        .map(|w| (w.to_ascii_lowercase(), w.to_string()))
        .collect()
}

/// 2026-11-05 (仕事 22): / と . の打ち分け (数字・英字の語のすぐ後ろの / を半角のまま・数字のすぐ後ろの拡張子・拡張子の表を広げた分・
/// かなのすぐ後ろの . に続く小文字の拡張子) を使うか。試験のビルドでは AZK_SLASH_DOT=0 で前の形 (測りの比べ用)
pub fn slash_dot_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| test_env("AZK_SLASH_DOT").as_deref() != Some("0"))
}

/// 2026-11-05 (仕事 28 の 1): / キー (input.slash_key) と . キー (input.period_key) の打ち分け。Auto = 22 の自動の打ち分け (初期値)、
/// Japanese = いつも ・ か 。 (22 の前の動き)、Latin = かなの入力中もいつも半角の / か .
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyForm {
    Auto,
    Japanese,
    Latin,
}

thread_local! {
    /// 試験で使う (/ の形, . の形)。None なら設定
    pub(crate) static KEY_FORMS_FOR_TEST: std::cell::Cell<Option<(KeyForm, KeyForm)>> = const { std::cell::Cell::new(None) };
}

fn key_forms() -> (KeyForm, KeyForm) {
    if let Some(forms) = KEY_FORMS_FOR_TEST.get() { return forms; }
    if cfg!(test) { return (KeyForm::Auto, KeyForm::Auto); }
    static FORMS: OnceLock<(KeyForm, KeyForm)> = OnceLock::new();
    *FORMS.get_or_init(|| {
        let input = shared::AppConfig::read_or_default().input;
        let form = |value: &str, japanese: &str, latin: &str| if value == japanese { KeyForm::Japanese } else if value == latin { KeyForm::Latin } else { KeyForm::Auto };
        (form(&input.slash_key, "dot", "slash"), form(&input.period_key, "kuten", "period"))
    })
}

pub fn slash_key() -> KeyForm { key_forms().0 }
pub fn period_key() -> KeyForm { key_forms().1 }
/// / の自動の打ち分け (数字・小文字の英字の語のあとは /) を使うか
pub fn slash_auto() -> bool { slash_dot_enabled() && slash_key() == KeyForm::Auto }
/// . の自動の打ち分け (数字のあとの拡張子・かなのあとの拡張子・拡張子の表を広げた分) を使うか
pub fn period_auto() -> bool { slash_dot_enabled() && period_key() == KeyForm::Auto }

/// 2026-11-05 (仕事 28 の 5): 小文字の英字 1 字＋語の直し (jri-gu → Jリーグ。input.initial_letter_words、初期値オン)
fn initial_letter_words_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| cfg!(test) || shared::AppConfig::read_or_default().input.initial_letter_words)
}

/// 数字のあとの . , を小数点・桁区切りにするか (設定 input.number_punctuation、初期値はオン)
pub fn number_punctuation() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| if cfg!(test) { true } else { shared::AppConfig::read_or_default().input.number_punctuation })
}

/// 2026-10-28 (試験中): 。、？！ を打ったら、そこまでを変換するか (設定 input.punctuation_convert、初期値オフ)
pub fn punctuation_convert() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| !cfg!(test) && shared::AppConfig::read_or_default().input.punctuation_convert)
}

/// 英語の自動判別が有効か (設定で切れる)
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| shared::AppConfig::read_or_default().input.auto_english)
}

fn is_vowel(c: u8) -> bool {
    matches!(c, b'a' | b'i' | b'u' | b'e' | b'o')
}

/// 音節の打ちかけ (末尾だけ許す): k, ky, sh, ts など
fn is_syllable_prefix(s: &str) -> bool {
    SYLLABLES.iter().any(|syl| syl.len() > s.len() && syl.starts_with(s))
}

/// 2026-10-23: 変換のローマ字表 (ROMAJI_TABLE) の英字だけの綴りと、その打ちかけ (綴りの頭)。
/// 表に無い語の読めない並びを、変換が実際に読める綴り (l・x の小書き、てゅ・すぁ なども) で確かめるため
fn table_romaji_keys() -> &'static (HashSet<&'static str>, HashSet<&'static str>) {
    static KEYS: OnceLock<(HashSet<&'static str>, HashSet<&'static str>)> = OnceLock::new();
    KEYS.get_or_init(|| {
        let keys: HashSet<&'static str> = super::romaji_table::ROMAJI_TABLE.iter().map(|(keys, _)| *keys)
            .filter(|keys| keys.bytes().all(|b| b.is_ascii_lowercase())).collect();
        let prefixes = keys.iter().flat_map(|keys| (1..keys.len()).map(move |l| &keys[..l])).collect();
        (keys, prefixes)
    })
}

/// 2026-10-24: 表に無い語の規則 (segment_run_raw) を使うか。ベンチで規則の前と比べるため、試験のビルドで AZK_NO_UNKNOWN_RULE を指定したときだけ外す
fn unknown_word_rule() -> bool {
    static OFF: OnceLock<bool> = OnceLock::new();
    !*OFF.get_or_init(|| test_env("AZK_NO_UNKNOWN_RULE").is_some())
}

/// word の p 番目で読み切れる次の位置 (表の綴り・n + 子音 の ん・同じ子音の重ね と tch の っ)
fn table_romaji_steps(word: &[u8], p: usize) -> impl Iterator<Item = usize> + '_ {
    let (keys, _) = table_romaji_keys();
    let n = word.len();
    let spelled = (1..=4).filter(move |&l| p + l <= n && std::str::from_utf8(&word[p..p + l]).is_ok_and(|s| keys.contains(s))).map(move |l| p + l);
    let c = word[p];
    let d = word.get(p + 1).copied();
    let moraic = (c == b'n' && d.map_or(true, |d| !is_vowel(d) && d != b'y'))
        || d.is_some_and(|d| (c == d && !is_vowel(c) && c != b'n') || (c == b't' && d == b'c'));
    spelled.chain(moraic.then_some(p + 1))
}

/// word の各位置から終わりまでを変換のローマ字表で読み切れるか (pending なら終わりの打ちかけも読めるうちに数える)。
/// ここで読み切れる位置は、頭の 7 字を table_romaji_head で読んでも必ず読み切れる (表の綴りの途中で切れても打ちかけとして読める)
fn table_romaji_suffixes(word: &str, pending: bool) -> Vec<bool> {
    let (_, prefixes) = table_romaji_keys();
    let b = word.as_bytes();
    let mut readable = vec![false; b.len() + 1];
    readable[b.len()] = true;
    for p in (0..b.len()).rev() {
        readable[p] = (pending && b.len() - p < 4 && std::str::from_utf8(&b[p..]).is_ok_and(|s| prefixes.contains(s)))
            || table_romaji_steps(b, p).any(|next| readable[next]);
    }
    readable
}

/// word の頭を変換のローマ字表で読んだときの (読めなくなる手前までの長さ, そこまでで読み切れたいちばん長い長さ)。
/// pending_at_end なら word の終わりの打ちかけ (dy) も読めるうちに数える。word 全体が読めれば 1 つ目は word の長さ
fn table_romaji_head(word: &str, pending_at_end: bool) -> (usize, usize) {
    let (_, prefixes) = table_romaji_keys();
    let b = word.as_bytes();
    let mut reach = vec![false; b.len() + 1];
    reach[0] = true;
    let (mut readable, mut complete) = (0, 0);
    for p in 0..=b.len() {
        if !reach[p] { continue; }
        complete = p;
        readable = readable.max(p);
        // 打ちかけ (綴りの頭) は、後ろに字が続くか、終わりの打ちかけを許すときだけ読めるうちに数える
        let mut q = p + 1;
        while q <= b.len() && std::str::from_utf8(&b[p..q]).is_ok_and(|s| prefixes.contains(s)) && (q < b.len() || pending_at_end) {
            readable = readable.max(q);
            q += 1;
        }
        if p < b.len() {
            for next in table_romaji_steps(b, p) { reach[next] = true; }
        }
    }
    (readable, complete)
}

/// 2026-10-25: 同じ子音の重ねで始まり、続きがローマ字として読める打ちかけ (tte = って・sso = っそ)。英字の連続の頭の重ねは促音にしない
/// (ffmpeg の ff) ので、読めない字が残る日本語の読みより英単語の頭 (社名・製品名の表の TTEdit) が勝っていた。会話の文は って・っす から
/// 始まることがあるので、打ちかけを英単語の頭とはみなさない (打ち終えた語 ttedit は英字のまま)
fn leading_sokuon_kana(w: &str) -> bool {
    let b = w.as_bytes();
    b.len() >= 3 && b[0] == b[1] && !is_vowel(b[0]) && b[0] != b'n' && native_romaji_readable(&w[1..], true)
}

/// 2026-10-15: 同じ母音を小さい文字で伸ばす形 (ma + la = まぁ・e + xe = えぇ・so + lo = そぉ)。打った人は小さい文字を明示しているので、
/// てぃ・ふぁ のようにくっつく形と同じく守る (exe・ele で えぇ と打つ人がいる。mala・sala・ele は英単語の表に無いのに英語にされていた)
fn prolongs_vowel(previous_keys: &str, small_keys: &str) -> bool {
    matches!(small_keys, "xa" | "xi" | "xu" | "xe" | "xo" | "la" | "li" | "lu" | "le" | "lo")
        && previous_keys.len() >= 1 && previous_keys.as_bytes()[previous_keys.len() - 1] == small_keys.as_bytes()[1]
}

fn small_kana_boundaries(run: &str) -> (Vec<bool>, Vec<bool>) {
    small_kana_boundaries_with(run, true)
}

/// combined = false のときは、同じ母音を伸ばす形 (まぁ) だけを見る
fn small_kana_boundaries_with(run: &str, combined: bool) -> (Vec<bool>, Vec<bool>) {
    let mut joined = vec![false; run.len() + 1];
    let mut protected = joined.clone();
    if !run.is_ascii() { return (joined, protected); }
    static KANA: OnceLock<HashSet<&'static str>> = OnceLock::new();
    let kana = KANA.get_or_init(|| super::romaji_table::ROMAJI_TABLE.iter().map(|(_, kana)| *kana).collect());
    // 2026-11-03: できる形がまれな組 (UNCOMMON_SMALL_KANA の かな。to + li = とぃ・do + le = どぇ・su + la = すぁ) は、l・x で明示した小書きでも
    // くっつく形 (joined。小書きの費用が 0) にしない (atolinede が a|to|li|ne|de = あとぃねで になり、守った位置から始まる英単語の line が費用で負けていた)。
    // 小書きをまたぐ語 (社名の表の Atol) を止める守り (protected) はそのまま。てぃ・でぃ・ふぁ のようによく使う形 (teli・delisupurei) と、
    // 同じ母音を伸ばす形 (tolo = とぉ) は今までどおり
    // かなを作る綴りが全部まれなものだけ (ふぁ・ふぃ は hwa・fwi のほかに fa・fi でも打つので、まれではない)
    static UNCOMMON: OnceLock<HashSet<&'static str>> = OnceLock::new();
    let uncommon = UNCOMMON.get_or_init(|| {
        let table = super::romaji_table::ROMAJI_TABLE;
        table.iter().filter(|(input, _)| UNCOMMON_SMALL_KANA.contains(input)).map(|(_, kana)| *kana)
            .filter(|kana| table.iter().all(|(input, other)| other != kana || UNCOMMON_SMALL_KANA.contains(input))).collect()
    });
    let mut previous = String::new();
    let mut previous_keys = String::new();
    let mut previous_lowercase = false;
    let mut offset = 0;
    for unit in super::romaji_style::simulate(run) {
        if !unit.pending && SMALL_KANA.contains(&unit.keys.as_str())
            && previous_lowercase && run[offset..offset + unit.keys.len()].bytes().all(|byte| byte.is_ascii_lowercase())
            && !previous.is_empty()
            && ((combined && kana.contains((previous.clone() + &unit.kana).as_str())) || prolongs_vowel(&previous_keys, &unit.keys)) {
            joined[offset] = !uncommon.contains((previous.clone() + &unit.kana).as_str()) || prolongs_vowel(&previous_keys, &unit.keys);
            // Keep the base kana attached too: node + xi must not steal no + de + xi.
            for boundary in offset..offset + unit.keys.len() { protected[boundary] = true; }
        }
        previous_lowercase = run[offset..offset + unit.keys.len()].bytes().all(|byte| byte.is_ascii_lowercase());
        offset += unit.keys.len();
        previous_keys = if unit.pending { String::new() } else { unit.keys.clone() };
        previous = if unit.pending { String::new() } else { unit.kana };
    }
    (joined, protected)
}

/// 2026-11-03 (仕事 11): 英字の語のすぐ後ろ (空白なし) に来る読みの頭 (2 音・3 音) の、日本語の Wikipedia での回数 (after_latin.txt。tools/after_latin)。
/// 和文の中の英単語のすぐ後ろには、助詞 (で・の・を)・する (して・した)・接尾語 (版・系) の音が来る。来にくい並び (powers + たげ・powers + たちお) は、
/// 英単語 + 英単語 (power + stage) を英単語 1 つ + 読めるローマ字に割った形のことが多い (split_rare_after_english)
/// 4 回以上の並びだけを、かなの順に並べた表 (無い並びは 0 回)
const AFTER_LATIN: &str = include_str!("after_latin.txt");

/// かなの順に並べた「かな<TAB>回数」の行 (頭の # の行は飛ばす) から、key の回数を引く。表を読み込んで持たず、組み込んだ文字のまま二分探索する
/// (IME 本体はどのアプリにも読み込まれるので、表 (約 1MB) をアプリごとのメモリに広げない。引くときに触る数ページだけが読み込まれ、アプリどうしで共有される)
fn sorted_table_count(table: &'static str, key: &str) -> u32 {
    let mut start_of_body = 0;
    while table[start_of_body..].starts_with('#') {
        match table[start_of_body..].find('\n') { Some(p) => start_of_body += p + 1, None => return 0 }
    }
    let body = &table[start_of_body..];
    let b = body.as_bytes();
    let (mut lo, mut hi) = (0usize, b.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        let start = b[..mid].iter().rposition(|&c| c == b'\n').map_or(0, |p| p + 1).max(lo);
        let end = b[start..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| start + p);
        let line = &body[start..end];
        let (k, n) = line.split_once('\t').unwrap_or((line, ""));
        match k.cmp(key) {
            std::cmp::Ordering::Equal => return n.trim().parse().unwrap_or(0),
            std::cmp::Ordering::Less => lo = end + 1,
            std::cmp::Ordering::Greater => hi = start,
        }
    }
    0
}

/// 打った字の i から、かな の音を limit 個まで (重ねた子音は っ、n + 子音 は ん、小書きは前の音に付ける)。読めない所と末尾の n 1 つ (打ちかけ) で止まる
fn romaji_kana_table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| super::romaji_table::ROMAJI_TABLE.iter().copied().collect())
}

fn kana_morae(lower: &str, mut i: usize, limit: usize) -> Vec<String> {
    let table = romaji_kana_table();
    let b = lower.as_bytes();
    let mut out: Vec<String> = vec![];
    while i < b.len() && out.len() <= limit {
        let c = b[i];
        if i + 1 < b.len() && b[i + 1] == c && c.is_ascii_lowercase() && !is_vowel(c) && c != b'n' {
            out.push("っ".into());
            i += 1;
            continue;
        }
        if c == b'n' && i + 1 < b.len() && b[i + 1].is_ascii_lowercase() && !is_vowel(b[i + 1]) && b[i + 1] != b'y' && b[i + 1] != b'n' {
            out.push("ん".into());
            i += 1;
            continue;
        }
        let Some((len, kana)) = (1..=(b.len() - i).min(4)).rev().find_map(|l| lower.get(i..i + l).and_then(|k| table.get(k)).map(|kana| (l, *kana))) else { break };
        if len == 1 && c == b'n' { break; }
        if kana.chars().all(|ch| "ぁぃぅぇぉゃゅょゎ".contains(ch)) && !out.is_empty() {
            out.last_mut().unwrap().push_str(kana);
        } else {
            out.push(kana.to_string());
        }
        i += len;
    }
    out.truncate(limit);
    out
}

/// 来にくい並びの線 (英字の語のすぐ後ろの 2 音・3 音の回数がこの数以下) と、日本語の語の頭としてよく出る 3 音の線 (この数を超えたら分けない)。
/// 試験のビルドでは AZK_AFTER_LATIN_SPLIT=2 音の線,3 音の線,語の頭の線 で替えられる (0 で切る)
const AFTER_LATIN_FLOORS: (u32, u32, u32) = (3, 3, 40);

fn after_latin_floors() -> Option<(u32, u32, u32)> {
    static FLOORS: OnceLock<Option<(u32, u32, u32)>> = OnceLock::new();
    *FLOORS.get_or_init(|| match test_env("AZK_AFTER_LATIN_SPLIT") {
        None => Some(AFTER_LATIN_FLOORS),
        Some(text) => {
            let v: Vec<u32> = text.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            let d = AFTER_LATIN_FLOORS;
            if v.first() == Some(&0) { None } else { Some((v.first().copied().unwrap_or(d.0), v.get(1).copied().unwrap_or(d.1), v.get(2).copied().unwrap_or(d.2))) }
        }
    })
}

/// 2026-11-04 (仕事 11): 日本語の文の語の頭 (形態素の頭から 3 音) の回数 (japanese_heads.txt。日本語の Wikipedia の 3 万記事、41 回以上の並びだけ。
/// tools/after_latin/count_word_heads.py)。英字の語のすぐ後ろには来にくくても、語の頭としてよく出る並び (きてか 44・みてか 71・だれか 341・なって・しての) は、
/// 英単語のあとに助詞なしで続けた和文 (patch 来てから・highlight だれか) なので、英単語 2 つに分けない
fn japanese_head_count(key: &str) -> u32 {
    sorted_table_count(include_str!("japanese_heads.txt"), key)
}

/// i から始まるローマ字の読みの頭 (2 音・3 音) が、英字の語のすぐ後ろに来にくい並びか (日本語の Wikipedia での回数が線以下)。2 音そろわない (打ちかけ・読めない) ときは偽
fn rare_after_latin(lower: &str, i: usize, floors: (u32, u32, u32)) -> bool {
    let morae = kana_morae(lower, i, 3);
    if morae.len() < 2 { return false; }
    let count = |key: &str| sorted_table_count(AFTER_LATIN, key);
    count(morae[..2].concat().as_str()) <= floors.0 || (morae.len() == 3 && count(morae.concat().as_str()) <= floors.1)
}

/// 2026-11-04 (仕事 11): 区切ったあと、小文字で打った英単語 (3 字以上、表の語) のすぐ後ろのローマ字が、英字の語のすぐ後ろに来にくい並び
/// (powers + たげ・powers + たちお) なら、その英単語からローマ字のどこか (並びの終わりか、助詞から日本語が読める所) までを、表の英単語 2 つに
/// ちょうど分けられるかを見て、分けられればそうする (power + stage・sound + stage + で・power + station)。分けられなければ何も変えない。
/// 区切りの点に足す形も試したが、打ち間違いの多い並びで、表に無い語の長い塊が代わりに勝った (例文の打鍵 11,964 行の 330 行が変わった) ので、
/// 英単語 2 つに分けられる所だけを直す。is_word は分けた語が表の英単語か、japanese_at はその位置から助詞で日本語が読めるか
fn split_rare_after_english(run: &str, tokens: Vec<Token>, floors: (u32, u32, u32), is_word: impl Fn(&str) -> bool, japanese_at: impl Fn(usize) -> bool) -> Vec<Token> {
    let lower = run.to_ascii_lowercase();
    let n = lower.len();
    let mut starts = Vec::with_capacity(tokens.len() + 1);
    let mut at = 0;
    for t in &tokens {
        starts.push(at);
        at += t.raw.len();
    }
    starts.push(at);
    if at != n { return tokens; }
    for k in 0..tokens.len().saturating_sub(1) {
        let (s, end) = (starts[k], starts[k + 1]);
        let t = &tokens[k];
        if t.kind != TokenKind::English || tokens[k + 1].kind != TokenKind::Romaji || end - s < 3
            || !t.raw.bytes().all(|b| b.is_ascii_lowercase()) || !rare_after_latin(&lower, end, floors) { continue; }
        // 語の頭としてよく出る並び (patch + きてから) は和文
        let head = kana_morae(&lower, end, 3);
        if head.len() == 3 && japanese_head_count(head.concat().as_str()) > floors.2 { continue; }
        // 分けた後ろの切れ目は、もとの区切りの境目 (音節の途中で切らない)。間は小文字のローマ字の区切りだけ。長いほうから
        for j in ((k + 2)..=tokens.len()).rev() {
            let e = starts[j];
            if tokens[k + 1..j].iter().any(|x| x.kind != TokenKind::Romaji || !x.raw.bytes().all(|b| b.is_ascii_lowercase())) { continue; }
            if e < n && !japanese_at(e) { continue; }
            let Some(m) = ((s + 3)..=e.saturating_sub(3)).find(|&m| is_word(&lower[s..m]) && is_word(&lower[m..e])) else { continue };
            let mut out: Vec<Token> = tokens[..k].to_vec();
            out.push(Token { raw: run[s..m].to_string(), kind: TokenKind::English });
            out.push(Token { raw: run[m..e].to_string(), kind: TokenKind::English });
            out.extend_from_slice(&tokens[j..]);
            return split_rare_after_english(run, out, floors, is_word, japanese_at);
        }
    }
    tokens
}

/// 表の英単語 (社名・製品名の表にしか無い語と、英語以外の単語表の語は使わない) で分ける
fn split_rare_after_english_in(run: &str, tokens: Vec<Token>) -> Vec<Token> {
    let Some(floors) = after_latin_floors() else { return tokens };
    let lower = run.to_ascii_lowercase();
    let suffixes = std::cell::OnceCell::new();
    split_rare_after_english(run, tokens, floors,
        |w| words().set.contains(w) && !words().brands.contains(w) && !learned_kana(w),
        |e| cap_boundary(&lower[e..]) && suffixes.get_or_init(|| native_romaji_suffixes(run, false))[e])
}

/// 英字だけの連続 1 つを区切る (読めない文字の印つき。英単語へのまとめ直しはまだ)
fn segment_run_raw(run: &str, style: Style) -> (Vec<Token>, Vec<bool>) {
    let lower = run.to_ascii_lowercase();
    let n = lower.len();
    let japanese_suffixes = std::cell::OnceCell::new();
    // 表に無い語の規則で使う、終わりまでの読み切り (打ちかけ込み・打ちかけなし)。語の切れ目は、ふつうの音節 (SYLLABLES) で読めるかで決める
    // (dwo = どぉ のようなまれな綴りで、argoc|dwo と切らない)
    let unknown_tables: std::cell::OnceCell<(Vec<bool>, Vec<bool>)> = std::cell::OnceCell::new();
    // 2026-10-24: 変換のローマ字表で終わりまで読み切れる位置 (そこから始まる読めない並びは無いので、table_romaji_head を引かずに飛ばす。
    // 日本語の文で 1 打鍵あたり 2 割ほど遅くなっていた分)
    let table_readable: std::cell::OnceCell<Vec<bool>> = std::cell::OnceCell::new();
    // A complete known English lexeme retains the existing English decision.
    // 2026-10-13: 英語以外の単語表 (スペイン語の dolo など) と一致しただけでは、l/x で明示した小書き (do + lo = どぉ) の保護を外さない
    // 2026-10-15: 英語の文の中でだけ使う語の表 (ala・taxa) に当たっても、同じ母音を伸ばす形 (ala = あぁ) は守る
    let (joined_small_kana, protected_small_kana) = if words().set.contains(&lower) {
        (vec![false; n + 1], vec![false; n + 1])
    } else if words().context.contains(&lower) {
        small_kana_boundaries_with(run, false)
    } else { small_kana_boundaries(run) };
    const INF: i64 = i64::MAX / 2;
    // 位置ごとに「直前の区切りがローマ字 (0) か英語 (1) か」の 2 つの状態を持つ (英語の直後の母音 1 つは割高にするため、
    // 状態を分けないと、いちばん安い道だけ残して別の続き方を落としてしまう。6astra の指摘)。
    // cost[i][s] = run[..i] を、最後の区切りが状態 s になるように区切る最小コスト (1/100 単位)。
    // from[i][s] = (直前の位置, 直前の状態, 最後の区切りが「読めない文字」か)
    let mut cost = vec![[INF; 2]; n + 1];
    let mut from: Vec<[(usize, usize, bool); 2]> = vec![[(0, 0, false); 2]; n + 1];
    let state_of = |kind: &TokenKind| if *kind == TokenKind::English { 1 } else { 0 };
    cost[0][0] = 0;
    for i in 0..n {
        for prev in 0..2 {
            if cost[i][prev] >= INF {
                continue;
            }
            let base = cost[i][prev];
            // relax の add はふつうの単位、relax_fine は 1/100 単位 (音ごとの推定の加点が小数のため)
            let relax_mark = |j: usize, add: i64, kind: TokenKind, unreadable: bool, cost: &mut Vec<[i64; 2]>, from: &mut Vec<[(usize, usize, bool); 2]>| {
                let s = state_of(&kind);
                // 2026-10-20: 英単語のすぐ後ろに空白なしで英単語が続く区切りは 1/100 だけ割高に。同点のときだけ効き、
                // 英単語 + 助詞 + 英単語 (haniword + de + test) を、長い英単語 (haniword + detest) より選ぶ。和文の中の英単語は助詞でつながる
                let add = if s == 1 && prev == 1 { add + 1 } else { add };
                if base + add < cost[j][s] {
                    cost[j][s] = base + add;
                    from[j][s] = (i, prev, unreadable);
                }
            };
            let relax_fine = |j: usize, add: i64, kind: TokenKind, cost: &mut Vec<[i64; 2]>, from: &mut Vec<[(usize, usize, bool); 2]>| {
                relax_mark(j, add, kind, false, cost, from)
            };
            let relax = |j: usize, add: i64, kind: TokenKind, cost: &mut Vec<[i64; 2]>, from: &mut Vec<[(usize, usize, bool); 2]>| {
                relax_fine(j, add * 100, kind, cost, from)
            };
            let rest = &lower[i..];
            let after_english = prev == 1;
            // ローマ字の音節 (大文字は英語のしるしなのでローマ字としては読まない)
            let kunrei_nn = style.kunrei_nn;
            for syl in SYLLABLES.iter().chain(UNCOMMON_SMALL_KANA) {
                if kunrei_nn && (HEPBURN_ONLY.contains(syl) || C_ROW.contains(syl)) {
                    continue;
                }
                if run.as_bytes()[i..].starts_with(syl.as_bytes()) {
                    // ヴァ行は 1 だけ割高 (辞書にある英単語 video と同点にして英語を残し、英単語でない プリマヴェール は日本語のまま)
                    let mut add = if RARE.contains(syl) || UNCOMMON_SMALL_KANA.contains(syl) { 4 } else if syl.starts_with('v') { 1 } else { 0 };
                    // 英単語のすぐ後ろに母音だけ (い・う・え・お) が来る区切りは不自然 (tex+い より て+ぃ)。あ は「や・が」などの頭ではないが同じ扱い
                    if syl.len() == 1 && i > 0 && after_english {
                        // Only relax after a leading English span, not ruby inside ...ru + byuu...
                        let mut english_start = i;
                        let mut preceding_state = prev;
                        while english_start > 0 && preceding_state == 1 {
                            let (start, state, _) = from[english_start][preceding_state];
                            english_start = start;
                            preceding_state = state;
                        }
                        // 英単語 + その母音 がもっと長い英単語になる (stag + e = stage) なら、例外にしない
                        // ch・sh・ts で終わる英単語 + 母音 は音節 (cha・shi) の途中で割っている (match + あです)
                        // どちらも小文字で打った語だけ (大文字の IELTS + 入れた、OS + 選び、Agents + 上で は割ってよい)
                        let typed_lower = run.as_bytes()[english_start..i].iter().all(|c| !c.is_ascii_uppercase());
                        let longer_word = typed_lower && is_word(&lower[english_start..i + 1]);
                        let digraph_split = typed_lower && ["ch", "sh", "ts"].iter().any(|d| lower[english_start..i].ends_with(d));
                        let japanese_tail = english_start == 0 && rest.len() >= 4 && !longer_word && !digraph_split
                            && japanese_suffixes.get_or_init(|| native_romaji_suffixes(run, false))[i];
                        if !japanese_tail { add += 2; }
                    }
                    relax_fine(i + syl.len(), add * 100 + style.penalty(syl), TokenKind::Romaji, &mut cost, &mut from);
                }
            }
            for syl in SMALL_KANA {
                if run.as_bytes()[i..].starts_with(syl.as_bytes()) {
                    relax(i + syl.len(), if joined_small_kana[i] { 0 } else { 2 }, TokenKind::Romaji, &mut cost, &mut from);
                }
            }
            // 促音 (kk → っk) と撥音 (n + 子音 → ん)
            if i + 1 < n {
                let (c, d) = (run.as_bytes()[i], run.as_bytes()[i + 1]);
                // 促音: 同じ子音の重ね (kk) と、ち の前の t (tchi = っち)。
                // 2026-10-13: 大文字の重ね (MMD・GG) と、読めない子音の直後の重ね (dmmade = dm+まで、fpssatuei) は促音にしない。
                // っ は母音か ん の後ろにしか来ない。ww は後ろに母音が無ければ促音ではなく w の連打 (草)
                let lowercase_pair = c.is_ascii_lowercase() && d.is_ascii_lowercase();
                // 英単語の直後 (Uber + って、Unity + やってた) なら促音でよい
                // 連続の先頭 (+ffmpeg の ff) は促音にしない (日本語の語は っ から始まらない)
                let after_mora = i > 0 && (after_english || is_vowel(lower.as_bytes()[i - 1]) || lower.as_bytes()[i - 1] == b'n');
                let w_laugh = c == b'w' && d == b'w' && {
                    let end = i + lower.as_bytes()[i..].iter().take_while(|&&b| b == b'w').count();
                    end == n || !is_vowel(lower.as_bytes()[end])
                };
                if lowercase_pair && after_mora && !w_laugh && c == d && !is_vowel(c) && c != b'n' {
                    // 2026-10-29: 英単語のすぐ後ろの促音は、その子音を足すと表の語になるとき (plugin + s = plugins) だけ 1 点高く
                    // (plugins + して が plugin + っして になっていた。同点ではローマ字の側が勝つ)。Unity + って・Excel + っぽい は足しても表の語にならないので今のまま
                    let english_plus = after_english && is_word(&lower[from[i][1].0..i + 1]);
                    relax_fine(i + 1, if english_plus { 1 } else { 0 }, TokenKind::Romaji, &mut cost, &mut from);
                }
                // 2026-10-28: 同じ子音の 3 つ重ね (matttya の ttt) は促音の打ち間違い (子音の超過)。2 つめも っ の打ちかけとして読み、
                // 英単語 (mat・set・it) を取らない。英語に 3 つ重ねの語はまず無い。1 つ多い子音は sokuon_repairs で見つけ、変換するときに打った文字から消す (composition.rs の repair_typos)
                let after_sokuon = i >= 2 && lower.as_bytes()[i - 1] == c
                    && (is_vowel(lower.as_bytes()[i - 2]) || lower.as_bytes()[i - 2] == b'n');
                if lowercase_pair && after_sokuon && !w_laugh && c == d && !is_vowel(c) && c != b'n'
                    && lower.as_bytes().get(i + 2).is_some_and(|&e| is_vowel(e) || e == b'y' || e == b'h')
                    && !english_then_sokuon(&lower, i - 1) {
                    relax_fine(i + 1, 10, TokenKind::Romaji, &mut cost, &mut from);
                }
                // tch (matcha = まっちゃ) はまれな打ち方なので割高に。英単語 (match・watch・kitchen) が勝つように (2026-10-13)
                // 打っている途中で tc で止まっている (appuruuotc) ときも、っ の打ちかけとして読む
                let tc_pending = i + 2 == n && RUN_AT_END.get() && !FINISHING.get();
                // tch で止まっている (toitch・hatch) あいだは、母音が来て っち になるかもしれないので英単語 (itch・hatch) に勝たせる。
                // 打ち終えた (Space) ときは割高のまま (match は英字)
                let tch_pending = i + 3 == n && RUN_AT_END.get() && !FINISHING.get();
                if lowercase_pair && after_mora && c == b't' && d == b'c' && (lower.as_bytes().get(i + 2) == Some(&b'h') || tc_pending) {
                    relax_fine(i + 1, if tc_pending || tch_pending { 50 } else { 150 }, TokenKind::Romaji, &mut cost, &mut from);
                }
                // ん は nn と打つ人には、n 1 つ + 子音 は英語のしるし (anki = Anki、annki = 暗記)
                if c == b'n' && !is_vowel(d) && d != b'y' && d != b'n' && !kunrei_nn {
                    relax_fine(i + 1, style.penalty("n"), TokenKind::Romaji, &mut cost, &mut from);
                }
            }
            // 大文字の続き (PC, OCR, LINE) は英字のまま。大文字で始まる英単語 (Google) も
            if run.as_bytes()[i].is_ascii_uppercase() {
                let mut j = i;
                while j < n && run.as_bytes()[j].is_ascii_uppercase() {
                    j += 1;
                }
                relax(j, 0, TokenKind::English, &mut cost, &mut from);
                // 2026-10-13: 大文字の連続の途中 (GPT の T) からは小文字に伸ばさない (GPTha = GPT + は)。
                // 大文字 2 つ以上で始まる語 (AI・DM・CAD) も、後ろの小文字には伸ばさない (AIde = AI + で、CADshita)。
                // 例外は表記直しの表にある書き方そのもの (VSCode) と、末尾の複数形の s (PDFs)
                let word_start = i == 0 || !run.as_bytes()[i - 1].is_ascii_uppercase();
                // 2026-10-13: 大文字 1 字 + 小文字 の知らない語 (Figma・Suica・Qwen・Cubase) は、後ろに助詞などが来る位置までを 1 語に。
                // 大文字 1 字のすぐ後ろが助詞なら、その 1 字だけ (K + の罰金。Knob + あっきん にしない)
                if word_start && j - i == 1 {
                    let b = run.as_bytes();
                    let mut lower_end = j;
                    while lower_end < n && b[lower_end].is_ascii_lowercase() { lower_end += 1; }
                    let suffixes = japanese_suffixes.get_or_init(|| native_romaji_suffixes(run, false));
                    // 知っている語 (3 文字以上) のすぐ後ろが助詞か入力の切れ目なら (Code + に、React + と、Snow)、その読みを優先して以下の規則は使わない
                    // 2 文字の語は母音の無いもの (Hz) だけ (de・no のような外国語の語を知っている語に数えない)
                    // 英語の辞書と文脈用の単語表だけで判定する (スペイン語の cabo・cuba を知っている語に数えない)
                    let known_at = |k: usize| {
                        let w = &lower[i..k];
                        (words().set.contains(w) || words().context.contains(w) || learned_english(w)) && !learned_kana(w)
                            && (k - i >= 3 || !w.bytes().any(is_vowel))
                    };
                    let known_stop = ((i + 2)..=lower_end.min(i + 20)).find(|&k| known_at(k)
                        && (k == n || (cap_boundary(&lower[k..]) && suffixes[k])));
                    // 2026-10-15: 大文字 1 字のすぐ後ろが助詞で日本語が続くなら、3 文字以下の知っている語 (ono・ode) より大文字 1 字を優先 (O + の反応・O + では)
                    let short_known_stop = known_stop.is_some_and(|k| k - i <= 3);
                    if (known_stop.is_none() || short_known_stop) && j < n && cap_boundary(&lower[j..]) && suffixes[j] {
                        relax(j, -3, TokenKind::English, &mut cost, &mut from);
                    }
                    // 2026-10-15: 元素記号 (大文字 1 字か、大文字＋小文字の 2 字) を助詞でつなぐ形 (H + と + O・Na + と + Cl・Fe + と + Cu・H + も + O) は、
                    // 元素記号と助詞で切る。助詞のすぐ後ろが元素記号の形のときだけ (Nagasaki を Na + がさき、MotoGP を Mo + と + GP にしない)
                    for symbol_len in [1usize, 2] {
                        let e = i + symbol_len;
                        if e >= n || (symbol_len == 2 && !(b[i + 1].is_ascii_lowercase() && ELEMENT_SYMBOLS.contains(&&run[i..e]))) { continue; }
                        if let Some(particle) = PAIR_PARTICLES.iter().find(|p| lower[e..].starts_with(*p)) {
                            let after = e + particle.len();
                            // 助詞の後ろも元素記号の形 (大文字 1 字か、大文字＋小文字)。大文字が 2 つ続く略語 (MotoGP の GP) なら使わない
                            let next_is_symbol = after < n && b[after].is_ascii_uppercase() && !(after + 1 < n && b[after + 1].is_ascii_uppercase());
                            if next_is_symbol && b[e..after].iter().all(|c| c.is_ascii_lowercase()) {
                                relax(e, -3, TokenKind::English, &mut cost, &mut from);
                            }
                        }
                    }
                    // 読めない文字の直後で、残りが子音から日本語として読める最初の位置 (Pixiv|版、Yandex|しかない)。残りが 3 文字以上のときだけ
                    let first_readable = ((i + 2)..lower_end.min(i + 9)).find(|&k| suffixes[k]);
                    // 大文字が音節の頭 (Cu・De・Fu) なら語の一部なので、読める続き (Cubase の ubase) も 6 文字まで語に含める。
                    // 頭文字として置かれた大文字 (A + ランク、T + シャツ、K + の) は、読める続きを飲み込まない
                    let cap_syllable = j < n && !is_vowel(lower.as_bytes()[i]) && is_vowel(lower.as_bytes()[j]) && is_syllable(&lower[i..j + 1]);
                    let readable_limit = if cap_syllable { 6 } else { 0 };
                    // 知っている語のあとが日本語として読めるとき: 5 文字以上の語 (Video・Unity) ならその読みを優先し、
                    // 知らない語はその語より短く切らない (Markdow|ni にしない)。3 文字以上の語のあとに 3 文字以上の日本語 (Dot|kara) なら丸ごとにしない
                    // 知っている語 (3 文字以上) のあとが日本語として読めるなら (Bot + おやぶん、Uber + しとる)、知らない語の規則は使わない
                    let known_readable = ((i + 2)..=lower_end.min(i + 20)).any(|k| known_at(k) && (k == n || suffixes[k]));
                    // 続きのどこかで助詞から日本語が読めるなら、入力の最後まで丸ごと 1 語にはしない (Hz + する)
                    let inner_boundary = ((i + 2)..n).any(|p| cap_boundary(&lower[p..]) && suffixes[p]);
                    // 語の途中の大文字 (TikTok の 2 つ目の T) では使わない (英字の直後は、知っている語として丸ごと読むほうに任せる)
                    for k in (i + 2)..=lower_end.min(i + 8) {
                        if known_stop.is_some() || known_readable || after_english { break; }
                        if known_at(k) { continue; }
                        // 音節の途中では切らない (W + s|やほ にしない)
                        if k < n && !is_vowel(lower.as_bytes()[k - 1]) && SYLLABLES.iter().any(|syl| syl.len() > 1 && lower[k - 1..].starts_with(syl)) { continue; }
                        let tail = &lower[i + 1..k];
                        let limit = if k == n && RUN_BEFORE_DIGIT.get() { readable_limit.max(4) } else { readable_limit };
                        // n 1 つで終わる敬称 (kun・san) の直後が母音か y なら敬称ではない (Bhuzo + kuno = くの)
                        let honorific = k < n && suffixes[k] && ["sann", "san", "kunn", "kun", "tyann", "chann", "chan", "sama"].iter().any(|h| {
                            lower[k..].starts_with(h) && !(h.ends_with('n') && !h.ends_with("nn")
                                && lower.as_bytes().get(k + h.len()).is_some_and(|&c| is_vowel(c) || c == b'y'))
                        });
                        if native_romaji_readable(tail, false) && tail.len() > limit && !(honorific && tail.len() <= 4) { continue; }
                        let at_boundary = k < n && cap_boundary(&lower[k..]) && suffixes[k];
                        let at_first_readable = k < n && k >= i + 3 && first_readable == Some(k) && n - k >= 3 && !native_romaji_readable(tail, false)
                            && !is_vowel(lower.as_bytes()[k]) && lower.as_bytes()[k] != b'n' && lower.as_bytes()[k] != lower.as_bytes()[k - 1];
                        // 小文字の続きが大文字の直前で終わる (Mobius|P様)
                        let at_camel = k < n && k == lower_end && k >= i + 3 && b[k].is_ascii_uppercase();
                        let ok = if k < n {
                            at_boundary || at_first_readable || at_camel
                        } else {
                            k >= i + 3 && (FINISHING.get() || !RUN_AT_END.get()) && tail.len() <= 6 && !cap_boundary(tail) && !inner_boundary
                        };
                        if ok {
                            // 助詞の手前で切る候補 (Svelte|です) を、読めない文字の直後で切る候補 (Svel|てです) より優先する。
                            // ただし読めない文字のあとの読める部分を多く飲み込む候補は不利に (Pixivbann|の より Pixiv|版)
                            let last_unreadable = (i + 1..k).rev().find(|&p| !suffixes[p] && suffixes[p + 1]).map_or(i, |p| p);
                            let swallowed = if !native_romaji_readable(tail, false) { (k - last_unreadable - 1) as i64 } else { 0 };
                            // 境目が弱い助詞 (や・か・から・さん …) なら不利に (Yandexsi|か より Yandex|しかない)
                            let weak = at_boundary && !STRONG_BOUNDARY.iter().any(|b| lower[k..].starts_with(b));
                            let shape = if at_boundary || at_camel { 20 * swallowed + if weak { 25 } else { 0 } } else { 35 };
                            // 長さによらずほぼ一定 (短いほうを少しだけ優先)。長いほど安くすると日本語の助詞の位置まで伸びる (Figmadenippon)。
                            // 後ろが記号 (Example.config) なら語全体を優先
                            let whole = if k == n && !RUN_AT_END.get() { -50 } else { 0 };
                            // 知っている語 (大文字始まりは -長さ/100 ほど) よりは少し有利、ほかの読み (読めない文字 3〜300) よりはずっと安い
                            relax_fine(k, -100 + (k - i) as i64 + whole + shape, TokenKind::English, &mut cost, &mut from);
                        }
                    }
                }
                if word_start {
                    let b = run.as_bytes();
                    let mut e = j;
                    loop {
                        let mut l = e;
                        while l < n && b[l].is_ascii_lowercase() && l - e < 3 { l += 1; }
                        let mut u = l;
                        while u < n && b[u].is_ascii_uppercase() { u += 1; }
                        let middle = &lower[e..l];
                        if l == e || l - e > 2 || u == l || PARTICLES.contains(&middle) { break; }
                        e = u;
                    }
                    // 表記直しの表に、その先まで続く語 (McAfee) があるときは使わない (表の語のほうでまとめる)
                    let last_cap_continues = e < n && b[e].is_ascii_lowercase()
                        && ((e + 1)..=n.min(i + 30)).any(|k| canonical_case(&lower[i..k]) == Some(&run[i..k]));
                    if e > j && !last_cap_continues {
                        relax_fine(e, -((e - i) as i64), TokenKind::English, &mut cost, &mut from);
                    }
                }
                for k in (i + 2)..=n.min(i + 30) {
                    let w = &lower[i..k];
                    if k > j && (!word_start || j - i >= 2) {
                        let exact = canonical_case(w) == Some(&run[i..k]);
                        let plural = word_start && k == n && k == j + 1 && run.as_bytes()[j] == b's';
                        let short_known = word_start && k - j <= 2 && run.as_bytes()[j..k].iter().all(|c| c.is_ascii_lowercase())
                            && (words().set.contains(w) || words().context.contains(w)) && !cap_boundary(&lower[j..])
                            && lower[j..k].bytes().any(|b| !is_vowel(b))
                            && !(k < n && !is_vowel(lower.as_bytes()[k - 1]) && SYLLABLES.iter().any(|syl| syl.len() > 1 && lower[k - 1..].starts_with(syl)));
                        // 小文字の e・s が 1 字だけ付いた略語 (PCIe・PDFs) で、後ろが子音から始まり音節を割らないとき
                        // s から英単語が始まるなら (LM + studio) 使わない
                        // e も同じ (OS + 選び、OSe にしない)
                        let english_follows = (j + 3..=n.min(j + 20)).any(|e| is_detection_word(&lower[j..e], e == n));
                        let caps_suffix = word_start && k == j + 1 && matches!(run.as_bytes()[j], b'e' | b's') && !english_follows
                            && ((k == n && (RUN_AT_END.get() || RUN_BEFORE_DIGIT.get())) || (k < n && !is_vowel(lower.as_bytes()[k])
                                && !SYLLABLES.iter().any(|syl| syl.len() > 1 && lower[j..].starts_with(syl))));
                        if (caps_suffix || exact) && !plural && !short_known {
                            relax_fine(k, -((k - i) as i64), TokenKind::English, &mut cost, &mut from);
                            continue;
                        }
                        if !exact && !plural && !short_known { continue; }
                    }
                    // 英語以外の単語表の語 (Une・Aki) は使わない。2 文字はローマ字として読めないものだけ (Ai = あい にしない)。
                    // ただしこの 2 つの制限は母音の大文字 (A + 以上、U + ネクスト) のときだけ。子音の大文字 + 母音 (Go・Be・Fi・Hito・Memoria) は
                    // 大文字 1 字だけを英字にすると音節が割れる (G + お) ので、前と同じく外国語の単語表の語も 2 文字の語も使う
                    let cap_mora = j - i == 1 && !is_vowel(lower.as_bytes()[i]) && is_vowel(lower.as_bytes()[j]);
                    // 2026-11-01: 分野の略語は、大文字で打った頭の続きとしては表記どおりのときだけ (CDmada の CDma を CDMA・Cmat の Cma を CMA にしない)
                    let domain_case_ok = domain_abbreviation_form(w).is_none_or(|form| form == &run[i..k]);
                    let english_known = (words().set.contains(w) || (words().context.contains(w) && domain_case_ok) || learned_english(w) || session_english(w)
                        || (cap_mora && is_word(w))) && !learned_kana(w);
                    let before_particle = k < n && cap_boundary(&lower[k..]) && japanese_suffixes.get_or_init(|| native_romaji_suffixes(run, false))[k];
                    let known = english_known && (k - i >= 3 || cap_mora || !native_romaji_readable(w, false) || (k == n && !RUN_AT_END.get()) || before_particle);
                    let prefix = k == n && RUN_AT_END.get() && run.as_bytes()[i + 1..k].iter().all(|c| c.is_ascii_lowercase())
                        && (is_word_prefix(w) || context_word_prefix(w));
                    // A short initial must not consume half of the following kana syllable (T + syatu).
                    let kana_after_initial = k - i == 2 && k < n && run[i + 1..].len() >= 4
                        && native_romaji_readable(&run[i + 1..], false)
                        && SYLLABLES.iter().chain(SMALL_KANA.iter())
                            .any(|syl| syl.len() > 1 && run[i + 1..].starts_with(syl));
                    // 大文字の連続の途中で終わる語 (ASA の AS) は使わない (残りの A が後ろの sa と別の語にまとまる)
                    let inside_caps = k < j;
                    if (known || prefix) && !kana_after_initial && !inside_caps {
                        relax_fine(k, -((k - i) as i64), TokenKind::English, &mut cost, &mut from);
                    }
                }
            }
            // 2026-10-13: 小文字 1 字のすぐ後ろが大文字なら 1 語 (eSIM・iOS・eGPU、表記直しの表の iPad・iPhone)
            if run.as_bytes()[i].is_ascii_lowercase() && i + 1 < n && run.as_bytes()[i + 1].is_ascii_uppercase() {
                let mut j = i + 1;
                while j < n && run.as_bytes()[j].is_ascii_uppercase() {
                    j += 1;
                }
                // 日本語の語末の母音 (atarasii + MMD、yoi + AI) を取らないよう、表に無い語は打ち始めだけ
                // お・あ・う は日本語の接頭語 (おGPU) なので除く (iOS・eSIM・vLLM は残す)
                if i == 0 && j - (i + 1) >= 2 && !matches!(run.as_bytes()[i], b'o' | b'a' | b'u') {
                    relax(j, -1, TokenKind::English, &mut cost, &mut from);
                }
                // 2026-10-21: 大文字で終わる表の語 (iOS・iPadOS) も (前は j + 1 からで、がiOS が が + い + OS に割れていた)
                // 2026-10-29: 社名・製品名の表にしか無い表記 (iWeb・iChat・cPanel。97 語) は、並びの頭か、前が母音 (音の切れ目) のときだけ。
                // 前が子音だと頭の小文字はその音の母音 (tocuni + Web の ni の i) で、iWeb を取って残りの n と合わせ とcuniWeb になっていた
                // (B の手間 10-29。0.1.0 には社名の表が無かった)。10-25 の「社名の表にしか無い語は語の途中では使わない」と同じ考え
                let left_fits = i == 0 || is_vowel(lower.as_bytes()[i - 1]);
                for k in j..=n.min(i + 30) {
                    if canonical_case(&lower[i..k]) == Some(&run[i..k]) && (left_fits || !words().brands.contains(&lower[i..k])) {
                        relax_fine(k, -((k - i) as i64) * 100, TokenKind::English, &mut cost, &mut from);
                    }
                }
            }
            // 2026-10-13: 入力の末尾の w 1 つ (母音の後ろ。ですよねw) も草の w。打ち終えたときに前の音節と英単語 (new・row) にしない
            if FINISHING.get() && RUN_AT_END.get() && i + 1 == n && lower.as_bytes()[i] == b'w' && i > 0 && is_vowel(lower.as_bytes()[i - 1]) {
                relax(n, -1, TokenKind::English, &mut cost, &mut from);
            }
            // 2026-10-13: w の連打 (草) は英字。後ろが母音なら わ行 の促音かもしれないので除く
            if lower.as_bytes()[i] == b'w' && (i == 0 || lower.as_bytes()[i - 1] != b'w') {
                let end = i + lower.as_bytes()[i..].iter().take_while(|&&b| b == b'w').count();
                if end - i >= 2 && (end == n || !is_vowel(lower.as_bytes()[end])) && run.as_bytes()[i..end].iter().all(|b| b.is_ascii_lowercase()) {
                    relax(end, -1, TokenKind::English, &mut cost, &mut from);
                }
            }
            // 2026-10-13: 母音を含まない子音の並び (dm・md・km・sns・hz) は、促音や 1 字ずつの読めない文字より、1 語の英字として読む
            // 母音を含まない入力全体 (qzxqzx・wsl) には効かせない (日本語の中に混ざった略語のための規則。知らない語の学習を妨げない)
            if run.as_bytes()[i].is_ascii_lowercase() && !is_vowel(run.as_bytes()[i]) && run.as_bytes()[i] != b'n' && lower.bytes().any(is_vowel) {
                let mut end = i;
                // y は英語では母音のように働く (type・sync) ので、子音の並びはその手前で止める (keltype = ke + l + type)
                while end < n && run.as_bytes()[end].is_ascii_lowercase() && !is_vowel(run.as_bytes()[end]) && run.as_bytes()[end] != b'y' && end - i < 8 {
                    end += 1;
                }
                for e in (i + 2)..=end {
                    if lower.as_bytes()[i..e].iter().all(|&b| b == lower.as_bytes()[i]) { continue; }
                    relax(e, 3, TokenKind::English, &mut cost, &mut from);
                }
            }
            // 2026-10-23: 単語表に無い小文字の語で、頭のほうにローマ字として読めない並びがあるもの (dynabook の dy、kyndryl の ky、
            // lynx の ly、mozc の zc)。読めない文字を 1 字ずつ残すと、間の読める音節がかなになり、後ろの英単語だけが英字になっていた (dyなbook・これhadyな)。
            // 読めない並びから語の切れ目 (助詞の手前・読めない文字のすぐ後ろで残りが日本語として読める所・並びの終わり) までを 1 語の英字にする。
            // 読めるかどうかは変換のローマ字表で見る (l・x の小書き・てゅ も読める)。コストは知っている語 (1) より高く、読めない文字 1 字 (3) より安い 2。
            // 読めない並びの前の読める音節: 並びの頭ならそのまま含める (argocd・mozc)。それ以外では、子音で始まる助詞でない 1 音節で、
            // 読めない並びが英単語の頭にならないときだけ (kinou + bitbucket・dagster は含め、kara + dynabook の ra は含めない)。母音の音節は含めない
            // (kino + udynabook にしない。日本語の語は母音で終わることが多い。absorb_unreadable と同じ)。
            // 読めない文字のあとの読める続きを飲み込むほど少し高く。母音も y も無い入力全体 (tsmc だけ) には効かせない (上の子音の並びの規則と同じ)
            if unknown_word_rule() && run.as_bytes()[i].is_ascii_lowercase() && !protected_small_kana[i] && lower.bytes().any(|b| is_vowel(b) || b == b'y')
                && !table_readable.get_or_init(|| table_romaji_suffixes(&lower, RUN_AT_END.get() && !FINISHING.get()))[i] {
                // 読めない所は、読み切れた頭 (3 字まで) のすぐ後ろ 3 字以内 (keltype は ke のあと lt が ltu の打ちかけとして読めるので 5 字目)
                let window = n.min(i + 7);
                let (readable, head) = table_romaji_head(&lower[i..window], window < n || (RUN_AT_END.get() && !FINISHING.get()));
                if readable < window - i && head <= 3 && readable - head <= 3 {
                    let (suffixes, complete) = unknown_tables.get_or_init(|| (
                        native_romaji_suffixes(&lower, RUN_AT_END.get() && !FINISHING.get()),
                        native_romaji_suffixes(&lower, false),
                    ));
                    let head_text = &lower[i..i + head];
                    let cluster = &lower[i + head..n.min(i + head + 3)];
                    let head_ok = head == 0 || i == 0
                        || (is_syllable(head_text) && !is_vowel(lower.as_bytes()[i]) && !PARTICLES.contains(&head_text) && !is_word_prefix(cluster));
                    // 語は 3 字以上で、読めない所を含む (tuf + desu の f のように、語の終わりの打ちかけが後ろの字とつながらないものも含む)。
                    // 助詞の手前の切れ目は頭から見る (mo + zc + desu・tuf + desu の desu の前より先へ伸ばさない)。
                    // 残りが読める所の切れ目は 3 字目から (kd|di・dy|do・c|rio は読めない並びの途中なので切らない)
                    for k in (i + 1)..=n.min(i + 24) {
                        if !head_ok || !run.as_bytes()[k - 1].is_ascii_lowercase() { break; }
                        let b = lower.as_bytes();
                        // 4 字以上の知っている英単語の途中 (vaultwar|den の warden) は語の切れ目にしない。その英単語が並びの終わりか助詞の手前まで
                        // 続き、切れ目の両側に 2 字以上あるときだけ (dydo|desu の odes は数えない)
                        let inside_word = || (i..k.saturating_sub(1)).any(|q| ((k + 2)..=n.min(q + 30)).any(|r| r - q >= 4 && is_lexicon_word(&lower[q..r])
                            && (r == n || (cap_boundary(&lower[r..]) && suffixes[r]))));
                        let boundary = k < n && cap_boundary(&lower[k..]) && suffixes[k] && !inside_word();
                        let readable_rest = k < n && k - i >= 3 && n - k >= 3 && complete[k] && !complete[k - 1] && !is_vowel(b[k]) && b[k] != b'n' && b[k] != b[k - 1];
                        let unreadable = readable < k - i || (k < n && table_romaji_head(&lower[i..k], false).1 < k - i);
                        if k - i >= 3 && unreadable && (k == n || boundary || readable_rest) && !protected_small_kana[k] && !learned_kana(&lower[i..k]) {
                            // 読めない文字のあとの読める続き。語の終わりが知っている英単語なら (kel + type の pe)、その語の一部なので数えない
                            let last_unreadable = (i..k).rev().find(|&p| !suffixes[p] && suffixes[p + 1]).unwrap_or(i);
                            let known_tail = (i..=last_unreadable + 1).any(|q| k - q >= 3 && is_lexicon_word(&lower[q..k]));
                            let swallowed = if known_tail { 0 } else { (k - last_unreadable - 1) as i64 };
                            let add = 200 + if head == 0 { 5 } else { 0 } + 20 * swallowed + if k == n || boundary { 0 } else { 35 };
                            relax_fine(k, add, TokenKind::English, &mut cost, &mut from);
                        }
                        // 助詞の手前より先へは伸ばさない。残りが読める所は候補にするだけで、その先の切れ目 (keltype + tte) とも比べる
                        if boundary { break; }
                    }
                }
            }
            // 末尾の打ちかけ (n, k, ky, sh …)
            if run.as_bytes()[i..].iter().all(|c| c.is_ascii_lowercase()) && (is_syllable_prefix(rest) || rest == "n") {
                // ん を nn と打つ人が Space を押した時点で n が 1 つだけ残っていたら、英語のしるし (run = run、runn = るん)。
                // 英単語でなければ今までどおり ん (kon → こん)
                // 2026-10-13: Space の時点で n 以外の打ちかけ (fetch の ch、git の t) が残るのは日本語ではありえないので割高に
                // 数字の直前 (toeic + 900、teap + 300) も同じ。2026-10-13: 記号・空白の直前も (but, → ぶt、 にしない。so. ok?)。n は除く (ばん + 2)
                let add = if rest == "n" && FINISHING.get() { finishing_n_penalty(style) }
                    else if rest != "n" && (FINISHING.get() || !RUN_AT_END.get()) { 200 } else { 0 };
                relax_fine(n, add, TokenKind::Romaji, &mut cost, &mut from);
            } else if run.as_bytes()[i..].iter().all(|c| c.is_ascii_lowercase())
                && SMALL_KANA.iter().any(|syl| syl.len() > rest.len() && syl.starts_with(rest))
            {
                // 小さいかなの打ちかけ (x, xt) も割高に (tex・text は英語のまま)
                relax(n, 2, TokenKind::Romaji, &mut cost, &mut from);
            }
            // 英単語 (3 文字以上)。末尾なら打ちかけ (英単語の頭) も認める。
            // 2026-10-13: 大文字の連続の途中 (MMDen の D) からは始めない
            let inside_caps = i > 0 && run.as_bytes()[i].is_ascii_uppercase() && run.as_bytes()[i - 1].is_ascii_uppercase();
            let context_left_fits = i == 0 || (!after_english && SHORT_TERM_BOUNDARY.contains(&&lower[from[i][0].0..i]));
            for j in (i + 3)..=n.min(i + 30) {
                if inside_caps { break; }
                let w = &lower[i..j];
                // 2026-10-29: l・x で打った小書き (no + lo = のぉ・do + lo = どぉ) は守るが、そこから始まる 4 字以上の表の英単語 (社名・文脈用の表の語は除く) が
                // 語の切れ目 (並びの終わり・助詞の前) まで届くなら英単語として探す (konosaitono + login + gamenn が の + ぉ + ぎん になっていた。表に入れても直らなかった)
                // 2026-10-31: 切れ目の後ろは、並びの終わりまでではなく、次の表の英単語の手前まで読めればよい (japanese_until_english。
                // 文の後ろに英単語があると (…ni + linux + no + zeizyakusei + de + critical) linux が li|nu|x のままだった)
                let unprotected = protected_small_kana[i] && !protected_small_kana[j] && w.len() >= 4 && is_lexicon_word(w) && {
                    let suffixes = japanese_suffixes.get_or_init(|| native_romaji_suffixes(run, false));
                    j == n || (cap_boundary(&lower[j..]) && japanese_until_english(&lower, j, suffixes))
                };
                if (protected_small_kana[i] || protected_small_kana[j]) && !unprotected { continue; }
                // 2026-10-15: 小文字のすぐ後ろに大文字が来る並び (H + toO の toO) は、表記直しの表にその形で載っている語 (JavaScript・iPhone) でなければ
                // 英単語として読まない (toO を too にして HtoO が英字のまま残っていた)
                let typed = &run[i..j];
                if typed.as_bytes().windows(2).any(|p| p[0].is_ascii_lowercase() && p[1].is_ascii_uppercase()) && canonical_case(w) != Some(typed) {
                    continue;
                }
                // Old automatic joins can have been learned as deliberate English.
                // Do not let an unlisted learned span replace explicit kana commands inside Japanese.
                let suspicious_learned = i >= 4 && learned_english(w) && !session_english(w)
                    && !words().set.contains(w) && !words().latin.contains(w)
                    && joined_small_kana[i..j].iter().any(|joined| *joined)
                    && sentence_romaji_readable(&run[..i], false);
                if (!suspicious_learned && (is_priority(w) || edge_priority_here(&lower, i, j)))
                    || (is_word(w) && n_split_english(&lower, i, j, style)) {
                    relax_fine(j, -100 + splits_small_tsu(&lower, i, j), TokenKind::English, &mut cost, &mut from);
                } else if is_detection_word(w, j == n) {
                    // 英語以外の単語表だけにある語 (uti・clon・ani) は不利に (英単語・日本語の読みに負けるように)
                    let latin_only = !words().set.contains(w) && !words().context.contains(w) && !learned_english(w) && !session_english(w);
                    // 2026-10-28: 社名・製品名の表にしか無い語が、前は並びの頭か助詞・後ろは並びの終わりか助詞で日本語が続く所に収まるなら、少しだけ安く。
                    // 頭の英単語 + ローマ字 (able + とん・lie + んわれ) と同点になり、ローマ字の側が勝っていた (koreha + ableton + desu。表の 10,060 語のうち 988 語が
                    // 単独なら 1 語なのに文の中で崩れていた。B の写経 10-28 の flexceed)。語の途中・英単語のすぐ後ろでは今までどおり (gst・RSov の件)
                    let brand_fits = words().brands.contains(w) && context_left_fits && {
                        let suffixes = japanese_suffixes.get_or_init(|| native_romaji_suffixes(run, false));
                        j == n || (cap_boundary(&lower[j..]) && suffixes[j])
                    };
                    // 2026-10-31: 表記が全部大文字の略語 (DNA・RNA・ATP) も、並びの頭か助詞のすぐ後ろなら 1/100 だけ安く。前の助詞の音を取り込んだ
                    // 英単語との同点 (ha + dna と had + na・no + dna と nod + na) で略語のほうを選ぶ (生物の文で DNA が 50 か所崩れていた)。
                    // 助詞のあとに限るのは、長い英単語の途中 (sonar + nado の rna) を選ばないため。略語から始まるもっと長い表の英単語 (prin → print・
                    // lan → land) と、略語の前後にまたがる表の英単語 (tomcats の mcat・fumigated の ted) があるときも使わない (打っている途中の英単語を割らない)
                    let known = |a: usize, b: usize| is_word(&lower[a..b]) || words().context.contains(&lower[a..b]);
                    let caps = !latin_only && context_left_fits && caps_abbreviation(w)
                        && !((j + 1)..=n.min(i + 30)).any(|k| known(i, k))
                        && !(i.saturating_sub(30)..i).any(|a| (j..=n.min(a + 30)).any(|b| b - a >= 3 && known(a, b)));
                    relax_fine(j, if latin_only { 150 } else if brand_fits || caps { 99 } else { 100 } + splits_tch(&lower, j) + splits_small_tsu(&lower, i, j), TokenKind::English, &mut cost, &mut from);
                } else if context_left_fits && context_only_word(w, j == n) && {
                    // 2026-11-01: 打った字は小文字だけか、表記直しの表の表記どおり (CDmada の CDma を分野の略語 CDMA にしない)。
                    // 分野の略語は、最後の字から次の音節が始まる所 (c 行で打った ca + gya + ku の cag + ya) では使わない
                    let typed = &run[i..j];
                    (typed.bytes().all(|b| b.is_ascii_lowercase()) || canonical_case(w) == Some(typed))
                        && !(domain_abbreviation_form(w).is_some() && SYLLABLES.iter().any(|syl| syl.len() >= 2 && lower[j - 1..].starts_with(syl)))
                } && {
                    let suffixes = japanese_suffixes.get_or_init(|| native_romaji_suffixes(run, false));
                    let fits = |e: usize| e == n || (cap_boundary(&lower[e..]) && suffixes[e]);
                    fits(j) && !((j + 1)..=n.min(i + 30)).any(|e| is_word(&lower[i..e]) && fits(e))
                } {
                    // 2026-10-25: 文脈用の表にしか無い読めない語は、語の切れ目に収まるときだけ知っている語と同じに (short_term_fits に近い考え):
                    // 前は 並びの頭・助詞、後ろは 並びの終わり・助詞で日本語が続く所 (bot + を が ぼ + two・bot + w + おつくる になっていた)。
                    // 語の途中 (adm + いんけんげん・n + vim) と英単語のすぐ後ろ (koreha + res の hares + tic) では使わない。
                    // 同じ所から始まる知っている語が切れ目まで届くなら、そちらに譲る (linode・pasmo)
                    relax_fine(j, 100 + splits_tch(&lower, j) + splits_small_tsu(&lower, i, j), TokenKind::English, &mut cost, &mut from);
                } else if j == n && RUN_AT_END.get() && is_word_prefix(w) && !(i == 0 && leading_sokuon_kana(w)) {
                    // 2026-10-13: 語の頭でしかないもの (rha = rhapsody の頭) は、語として読める区切り (user + は) と並んだら負けるように
                    relax(j, 2, TokenKind::English, &mut cost, &mut from);
                }
            }
            // 2026-10-13: ローマ字として読めない 2 文字の英単語 (or・at・if・of・up) は英語に (o+r の r が読めない文字として残るより安く)
            // 2026-10-31: 2 文字の単位・記号 (kg・km・mL・Hz・pH) も。かなのすぐ後ろ (sonnnakgdesu) で前の な を巻き込んだ表に無い語 (nakg) にしていた。
            // 表に無い略語の一部 (nmr の nm・ims の ms・aml の ml・kyuumri の um) を切り出さないよう、単位として読んでよい形 (unit_word_shape) で、
            // 後ろが並びの終わりか子音で始まる音で、前が 1 字だけの母音 (a + ml・i + ms) でないときだけ
            if !after_english && i + 2 <= n && run.as_bytes()[i..i + 2].iter().all(|b| b.is_ascii_lowercase()) {
                let w2 = &lower[i..i + 2];
                let lone_vowel_before = i >= 1 && is_vowel(lower.as_bytes()[i - 1]) && (i == 1 || is_vowel(lower.as_bytes()[i - 2]));
                // 後ろは子音で始まる音 (母音の前の子音は次の音の頭: kban = k + ばん・mgi)。単位の 2 字目からの綴り (ms + hita の shi) を割らない
                let unit2 = QUANTITY_UNITS.iter().chain(LETTER_UNITS).any(|u| u.eq_ignore_ascii_case(w2)) && unit_word_shape(w2)
                    && !lone_vowel_before && (i + 2 == n || (!is_vowel(lower.as_bytes()[i + 2]) && SYLLABLES.iter().any(|syl| lower[i + 2..].starts_with(syl))))
                    && !SYLLABLES.iter().any(|syl| syl.len() >= 2 && lower[i + 1..].starts_with(syl));
                if (TWO_LETTER_DP.contains(&w2) || unit2) && !native_romaji_readable(w2, false) && !learned_kana(w2) {
                    relax_fine(i + 2, 150 + splits_tch(&lower, i + 2), TokenKind::English, &mut cost, &mut from);
                }
            }
            // 読めない文字はそのまま (ほかの区切り方より真に安いときだけ「読めない文字」として残る)
            relax_mark(i + 1, 300, TokenKind::Romaji, true, &mut cost, &mut from);
        }
    }
    // 後ろからたどる。同じコストなら、最後の区切りが前から始まるほう (abiding 全体 > ローマ字の din+g。状態を分ける前と同じ選び方)、
    // それも同じならローマ字で終わるほう
    // 2026-10-25: ただし、英単語 + 助詞で終わる区切り (chat + を) と、かなのあとの英単語で終わる区切り (ちゃ + two) が同じコストなら、
    // 英単語 + 助詞のほう (chatwo・jetwo・chathe が ちゃtwo・じぇtwo・ちゃthe になっていた。並びの途中 chatwosuru は前から chat + を)。
    // 助詞の前の英単語のほうが前から始まるときだけ。同じ所から始まる英単語どうし (techno 全体 > tech + の、pasmo 全体 > pas + も) は今までどおり
    let particle_after_english = {
        let ((q, q_state, _), (p, p_state, _)) = (from[n][0], from[n][1]);
        q_state == 1 && SHORT_TERM_BOUNDARY.contains(&&lower[q..n]) && p > 0 && p_state == 0 && from[q][1].0 < p
    };
    let mut tokens = vec![];
    let mut flags = vec![];
    let mut j = n;
    let mut s = match cost[n][1].cmp(&cost[n][0]) {
        std::cmp::Ordering::Less => 1,
        std::cmp::Ordering::Equal if from[n][1].0 < from[n][0].0 && !particle_after_english => 1,
        _ => 0,
    };
    while j > 0 {
        let (i, prev, unreadable) = from[j][s];
        let kind = if s == 1 { TokenKind::English } else { TokenKind::Romaji };
        tokens.push(Token { raw: run[i..j].to_string(), kind });
        flags.push(unreadable && j == i + 1);
        j = i;
        s = prev;
    }
    tokens.reverse();
    flags.reverse();
    (tokens, flags)
}

/// 2026-10-21: 訓令式・音ごとの推定の例外 (日本語に戻す) を確かめるときの比べ相手。ヘボン式の綴りと n 1 つの加点は外し、c 行の扱いだけ元のまま残す。
/// c 行はヘボン式でも訓令式でもないので、c 行のせいで英語になった語 (訓令式の人の code・cute) は日本語に戻さない
fn plain_style(style: Style) -> Style {
    let mut table = [0u8; ADAPTIVE.len()];
    for (k, (syl, _, _)) in ADAPTIVE.iter().enumerate() {
        if C_ROW.contains(syl) {
            // 訓令式の人は c 行を読まない (読めない文字の 3 より高い加点と同じ)
            table[k] = if style.kunrei_nn { 255 } else { style.adaptive.map_or(0, |t| t[k]) };
        }
    }
    Style { kunrei_nn: false, adaptive: Some(table) }
}

thread_local! {
    /// 上の例外を当てる語の長さの上限 (ベンチで比べるため。ふだんは制限なし)
    static EXCEPTION_MAX_LEN: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// 英字だけの連続 1 つを区切る
fn segment_run(run: &str, style: Style) -> Vec<Token> {
    let (tokens, flags) = segment_run_raw(run, style);
    // ん を nn と打つ人 (kunrei_nn・音ごとの推定で n 1 つに加点がある人) は n 1 つ + 子音 を英語のしるしにしているが、そのせいでだけ
    // 英語になった語 (pan・man の寄せ集めなど。長さは問わない。利用者専用の語・覚えた語・優先する語・大文字を含む語は除く) しか違いがなく、
    // n 1 つを ん と読めば全体がローマ字として読み切れるときは日本語を優先する (anpanmantyoko = アンパンマンチョコ、
    // tenpura = てんぷら、sinbun = しんぶん)
    if style.kunrei_nn || style.adaptive.is_some() {
        // 訓令式の規則 (n+子音、shi などのヘボン式の綴り) のせいで英語になった語 (ヘボン式として読むと英語にならない語) しか違いがなければ、
        // ヘボン式として読んだ区切りを使う (ashitahagoogledesagasu = 明日は google で さがす)
        let (plain, plain_flags) = segment_run_raw(run, plain_style(style));
        // 音ごとの推定 (auto) では、例外は 3 文字以下の語だけ (shin・font のように長い語は、その人がまず使わない綴りなら英語のまま)。
        // 2026-10-11 のベンチ (bench/adaptive_replay.txt): 日本語の文 3660 行を 4 通りの打ち方で流して誤検出 0 のまま、
        // 訓令式の人の英単語の検出率 0.78 → 0.96。設定で kunrei_nn を選んだ人は今までどおり長さを問わない (shinbun = しんぶん)
        let max_len = EXCEPTION_MAX_LEN.get().unwrap_or(if style.adaptive.is_some() { 3 } else { usize::MAX });
        let plain_english: HashSet<String> =
            plain.iter().filter(|t| t.kind == TokenKind::English).map(|t| t.raw.to_ascii_lowercase()).collect();
        // n で終わる英単語 + n + 母音 (mainni = main + に) は、n 1 つで ん と打つ読み方 (まいんに) に戻さない
        let lower_run = run.to_ascii_lowercase();
        let mut at = 0;
        let mut n_split = false;
        for t in &tokens {
            if t.kind == TokenKind::English && n_split_english(&lower_run, at, at + t.raw.len(), style) {
                n_split = true;
            }
            at += t.raw.len();
        }
        let extra: Vec<&Token> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::English && !plain_english.contains(&t.raw.to_ascii_lowercase()))
            .collect();
        // 利用者専用の英単語・覚えた語・優先する語・大文字を含む語は、訓令式の規則で英語になっていても例外にしない (anki = Anki)
        let only_short = !extra.is_empty()
            && extra.iter().all(|t| {
                let w = t.raw.to_ascii_lowercase();
                !learned_english(&w)
                    && !session_english(&w)
                    && !words().priority.contains(&w)
                    && !words().personal.contains(&w)
                    && t.raw.chars().all(|c| c.is_ascii_lowercase())
                    && t.raw.len() <= max_len
                    && !(FINISHING.get() && finishing_n_penalty(style) > 0 && run.ends_with(t.raw.as_str()) && w.ends_with('n') && !w.ends_with("nn"))
            });
        // ヘボン式の区切りも同じ所を英字にしているなら (vpn と vp、stage と stag)、日本語に戻す例外ではないので使わない
        let mut extra_spans = vec![];
        let mut at = 0;
        for t in &tokens {
            if t.kind == TokenKind::English && !plain_english.contains(&t.raw.to_ascii_lowercase()) { extra_spans.push((at, at + t.raw.len())); }
            at += t.raw.len();
        }
        let mut at = 0;
        let mut plain_overlaps = false;
        for t in &plain {
            let span = (at, at + t.raw.len());
            if t.kind == TokenKind::English && extra_spans.iter().any(|&(a, b)| span.0 < b && a < span.1) { plain_overlaps = true; }
            at += t.raw.len();
        }
        if only_short && !n_split && !plain_overlaps && !plain_flags.iter().any(|f| *f) {
            let plain = absorb_unreadable(plain, &plain_flags);
            return merge_case_terms(merge_latin_terms(split_rare_after_english_in(run, promote_before_suru(run, promote_short_terms(plain)))));
        }
    }
    let tokens = absorb_unreadable(tokens, &flags);
    let tokens = promote_short_terms(tokens);
    // 2026-11-06 (英単語＋する): サ変の形のすぐ前の英単語 (release + する・commit + する) を英字に
    let tokens = promote_before_suru(run, tokens);
    // 2026-11-04 (仕事 11): 英単語 + 来にくい かな (powers + たげ) を、英単語 2 つ (power + stage) に分けられれば分ける
    let tokens = split_rare_after_english_in(run, tokens);
    // 隣り合うローマ字はまとめずに音節ごと (Backspace で 1 音節ずつ消すため) にしておく
    merge_case_terms(merge_latin_terms(tokens))
}

fn merge_latin_terms(tokens: Vec<Token>) -> Vec<Token> {
    if words().latin.is_empty() { return tokens; }
    merge_latin_words(tokens, |word| words().latin.contains(word) && !learned_kana(word))
}

fn merge_latin_words(tokens: Vec<Token>, is_word: impl Fn(&str) -> bool) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let mut word = String::new();
        let mut english = false;
        let mut end = i + 1;
        for j in i..tokens.len() {
            word.push_str(&tokens[j].raw.to_ascii_lowercase());
            if word.len() > 30 { break; }
            english |= tokens[j].kind == TokenKind::English;
            let pending = FINISHING.get() && j + 1 == tokens.len() && word.len() >= 3 && tokens[j].raw != "n" && is_syllable_prefix(&tokens[j].raw);
            if j == i || (!english && !pending) || !is_word(&word) { continue; }
            // 2026-10-15: まとめた綴りの途中で小文字のあとに大文字が来る (to + O = toO) なら、語として打ったものではないのでまとめない (HtoO = H と O)
            let typed: String = tokens[i..=j].iter().map(|t| t.raw.as_str()).collect();
            if typed.as_bytes().windows(2).any(|p| p[0].is_ascii_lowercase() && p[1].is_ascii_uppercase()) { continue; }
            // 英字のすぐ後ろの助詞 (BBA + A + に) は外国語の語としてまとめない
            if tokens[j].kind == TokenKind::Romaji && PARTICLES.contains(&tokens[j].raw.to_ascii_lowercase().as_str())
                && tokens[i..j].iter().any(|t| t.kind == TokenKind::English) { continue; }
            // queso must not become que + そ, but never turn an all-kana word into Latin.
            let mut candidate = out.clone();
            candidate.push(Token { raw: word.clone(), kind: TokenKind::English });
            let index = candidate.len() - 1;
            candidate.extend(tokens[j + 1..].iter().cloned());
            if short_term_fits(&candidate, index) { end = j + 1; }
        }
        if end > i + 1 {
            out.push(Token { raw: tokens[i..end].iter().map(|t| t.raw.as_str()).collect(), kind: TokenKind::English });
        } else { out.push(tokens[i].clone()); }
        i = end;
    }
    out
}

fn merge_case_terms(tokens: Vec<Token>) -> Vec<Token> {
    merge_english_names(tokens, |name| canonical_case(name).is_some() && !learned_kana(name))
}

// Join only registered names; never pull neighboring Japanese syllables into a name.
fn merge_english_names(tokens: Vec<Token>, is_name: impl Fn(&str) -> bool) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let mut end = i + 1;
        if tokens[i].kind == TokenKind::English {
            let mut name = tokens[i].raw.to_ascii_lowercase();
            for j in i + 1..tokens.len() {
                if tokens[j].kind != TokenKind::English || name.len() + tokens[j].raw.len() > 30 {
                    break;
                }
                name.push_str(&tokens[j].raw.to_ascii_lowercase());
                if is_name(&name) {
                    end = j + 1;
                }
            }
        }
        if end == i + 1 {
            out.push(tokens[i].clone());
        } else {
            out.push(Token {
                raw: tokens[i..end].iter().map(|t| t.raw.as_str()).collect(),
                kind: TokenKind::English,
            });
        }
        i = end;
    }
    out
}

/// 文の切れ目になりやすい助詞などの音節。英単語の頭をさかのぼるときはここで止める
/// 2026-10-15: 元素記号 (2 字のもの)。H と O・Na と Cl のように助詞でつなぐ形を、元素記号と助詞で切るのに使う
const ELEMENT_SYMBOLS: &[&str] = &[
    "He", "Li", "Be", "Ne", "Na", "Mg", "Al", "Si", "Cl", "Ar", "Ca", "Sc", "Ti", "Cr", "Mn", "Fe", "Co", "Ni", "Cu", "Zn",
    "Ga", "Ge", "As", "Se", "Br", "Kr", "Rb", "Sr", "Zr", "Nb", "Mo", "Tc", "Ru", "Rh", "Pd", "Ag", "Cd", "In", "Sn", "Sb",
    "Te", "Xe", "Cs", "Ba", "La", "Ce", "Pr", "Nd", "Pm", "Sm", "Eu", "Gd", "Tb", "Dy", "Ho", "Er", "Tm", "Yb", "Lu", "Hf",
    "Ta", "Re", "Os", "Ir", "Pt", "Au", "Hg", "Tl", "Pb", "Bi", "Po", "At", "Rn", "Fr", "Ra", "Ac", "Th", "Pa", "Np", "Pu",
    "Am", "Cm",
];

/// 2026-10-15: 2 つの記号をつなぐ助詞 (H と O・A や B・C も D)
const PAIR_PARTICLES: &[&str] = &["to", "ya", "mo", "ka", "no", "ga", "wo", "ni", "de", "ha", "he"];

const PARTICLES: &[&str] = &[
    "ha", "ga", "wo", "no", "ni", "de", "to", "mo", "he", "ya", "ka", "yo", "ne", "te", "wa", "e", "nn", "n",
];

/// 読めない文字が英単語の直前にくっついていたら (keltype = ke・l・[type])、英単語の頭とみなして英字にまとめる。
/// 前のローマ字の音節も、助詞などに当たるまでさかのぼって含める (korehakeltype → これは + keltype)
fn absorb_unreadable(tokens: Vec<Token>, unreadable: &[bool]) -> Vec<Token> {
    let mut out: Vec<Token> = vec![];
    let mut out_unreadable: Vec<bool> = vec![];
    for (token, flag) in tokens.into_iter().zip(unreadable.iter().copied()) {
        if token.kind == TokenKind::English && out_unreadable.last() == Some(&true) {
            let mut start = out.len() - 1;
            while start > 0 && out_unreadable[start - 1] {
                start -= 1;
            }
            // 2026-10-13: さかのぼるのは子音で始まる 1 音節まで (ke + l + type = keltype)。何音節もさかのぼると日本語ごと英字になる
            // (atarasii + k + pop)。ただし、さかのぼった音節と読めない文字で知っている語になるなら、そこまで (i + bi + s + Paint = ibis)
            let unread: String = out[start..].iter().map(|t| t.raw.to_ascii_lowercase()).collect();
            let mut back = start;
            let mut word = unread.clone();
            let mut best_known = None;
            while back > 0 && start - back < 4 {
                let previous = &out[back - 1];
                let lower = previous.raw.to_ascii_lowercase();
                if previous.kind != TokenKind::Romaji || PARTICLES.contains(&lower.as_str()) || !is_syllable(&lower) { break; }
                back -= 1;
                word = lower + &word;
                if word.len() >= 3 && (is_word(&word) || words().context.contains(&word)) { best_known = Some(back); }
            }
            if let Some(known_start) = best_known {
                start = known_start;
            } else if start > 0 {
                let previous = &out[start - 1];
                let lower = previous.raw.to_ascii_lowercase();
                if previous.kind == TokenKind::Romaji && !PARTICLES.contains(&lower.as_str()) && is_syllable(&lower)
                    && lower.len() >= 2 && !is_vowel(lower.as_bytes()[0]) {
                    start -= 1;
                }
            }
            let mut raw: String = out.drain(start..).map(|t| t.raw).collect();
            out_unreadable.truncate(start);
            raw.push_str(&token.raw);
            out.push(Token { raw, kind: TokenKind::English });
            out_unreadable.push(false);
            continue;
        }
        out.push(token);
        out_unreadable.push(flag);
    }
    out
}

fn is_syllable(s: &str) -> bool {
    SYLLABLES.contains(&s)
}

/// 2026-11-03: Shift で始めた一時的な英字 (設定 input.shift_ascii_mode) を かな に戻す位置を決める規則 (ふだんは TEMP_ASCII_RULES。測るときに切り替える)
#[derive(Clone, Copy, Debug)]
pub struct TempAsciiRules {
    /// ① 英字の側が数字・記号で終わる (C₂ + について・E5 + 系・Win10 + で)
    pub after_digit: bool,
    /// ② 英字の側が表の英単語 (3 字以上) で、ローマ字の側が助詞・敬称などで始まる (Google + で・Python + の)
    pub known_particle: bool,
    /// ② を、英単語がローマ字として読めないとき (Google・Python) だけにする。読める語 (Take・Shin) のあとの助詞は、名前の途中のことがある
    /// (Takemoto = Take + もと、Shinya = Shin + や。名前の組で壊すのが 28 → 4。英単語 + 助詞の直る数は 2,000 → 1,909)
    pub known_unreadable: bool,
    /// ③ 英字の側が大文字 1 字で、ローマ字の側が敬称で始まる (A + さん)
    pub initial_honorific: bool,
    /// ④ 並び全体がローマ字として読めず、ローマ字の側がこの字数以上 (X + 線・T + シャツ。0 は使わない)
    pub unreadable_tail: usize,
    /// 区切りが英字の語のあとをローマ字と見れば、どれでも (測るときの比べ用)
    pub any: bool,
    /// 2026-11-03 (仕事 9c): 名前の表 (japanese_names_romaji.txt) で守って、もっと かな に戻す。並びの頭がそのまま名前で後ろが助詞・敬称なら
    /// 名前の後ろで切り (Aikoは・Takemotoさん)、名前か名前の打ちかけなら決めない。そうでなければ、大文字 1 字 + 読めるローマ字 (A + もんだい・
    /// S + さいず) と、表の英単語 (読める語も) + 助詞 (Amazon + で) でも切る。
    /// 2026-11-04 から初期値 (syllable_guard と組で)。Wikipedia の文と名前・英単語の組 (test の半分) で、大文字 1 字の組で直る 1,869 → 2,589/3,000・
    /// 頭が大文字の語の組で直る 1,890 → 2,118/3,000・表にある名前が割れる 12 → 6/5,701・表に無い名前 (測りの名前を全部除いた表で流す) が割れる 12 → 1,024/5,806・
    /// 名前 + 助詞が正しい 0 → 3,358/3,575・英単語 + 助詞 1,909 → 1,995/2,014
    pub name_guard: bool,
    /// name_guard の大文字 1 字で切る形を、頭の 2 字が音節の頭 (Ko・Shi・Kyo・Da) でないときだけにする (表に無い名前 Kotomi を K + おとみ にしない)
    pub syllable_guard: bool,
}

pub const TEMP_ASCII_RULES: TempAsciiRules = TempAsciiRules {
    after_digit: true, known_particle: true, known_unreadable: true, initial_honorific: true, unreadable_tail: 2, any: false,
    name_guard: true, syllable_guard: true,
};

const HONORIFICS: &[&str] = &["sann", "san", "kunn", "kun", "tyann", "chann", "chan", "sama"];

/// 社名・製品名の表 (english_brands.txt) に、この字で始まる語があるか (小文字で見る)
fn brand_prefix(prefix: &str) -> bool {
    static SORTED: OnceLock<Vec<String>> = OnceLock::new();
    let sorted = SORTED.get_or_init(|| {
        let mut v: Vec<String> = words().brands.iter().cloned().collect();
        v.sort();
        v
    });
    let i = sorted.partition_point(|w| w.as_str() < prefix);
    sorted.get(i).is_some_and(|w| w.starts_with(prefix))
}

/// 一時的な英字の並び run (Shift で打った大文字から、いま打った小文字まで。画面の字のまま) の、かな に戻す位置 (字の数)。
/// 区切り (segment) が英字の語のあとをローマ字と見たときだけ、その位置を規則で確かめて使う。
/// 区切りは知らない名前を割ることがある (Aiko = A + いこ、Haruto = Haru + と) ので、規則に当たらなければ今までどおり英字のまま
pub fn temp_ascii_return(run: &str) -> Option<usize> {
    temp_ascii_return_with(run, TEMP_ASCII_RULES)
}

pub fn temp_ascii_return_with(run: &str, rules: TempAsciiRules) -> Option<usize> {
    // 下付き・上付きの数字 (C₂) は数字として、全角の英数字は半角として見る (字の数は変わらない)
    const SUB: &str = "₀₁₂₃₄₅₆₇₈₉";
    const SUP: &str = "⁰¹²³⁴⁵⁶⁷⁸⁹";
    let ascii: String = run.chars().map(|c| {
        if let Some(d) = SUB.chars().position(|s| s == c).or_else(|| SUP.chars().position(|s| s == c)) {
            char::from(b'0' + d as u8)
        } else if ('\u{FF01}'..='\u{FF5E}').contains(&c) {
            char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
        } else {
            c
        }
    }).collect();
    if !ascii.is_ascii() || !ascii.as_bytes().first().is_some_and(u8::is_ascii_uppercase) {
        return None;
    }
    let tokens = segment(&ascii);
    if tokens.iter().map(|t| t.raw.len()).sum::<usize>() != ascii.len() {
        return None;
    }
    let lower = ascii.to_ascii_lowercase();
    // 並びがまだ英単語・社名の打ちかけ (Bord・Sni・Leoho・Smartsh) なら、② ③ ④ ⑤ では決めない。ローマ字の側の - は長音 (J + リーグ)
    let letters = lower.replace('-', "");
    let typing_word = is_word_prefix(&letters) || context_word_prefix(&letters) || brand_prefix(&letters);
    let letters_only = lower.bytes().all(|b| b.is_ascii_alphabetic());
    if rules.name_guard && letters_only {
        // 名前か名前の打ちかけなら決めない (Takemo の途中で Take + も にしない)。2026-11-04 (仕事 19 の 2): 英語の固有名詞 (Einstein) も、
        // 4 字から (Jha・Nge は J + は・N + ゲージ の打ちかけのことが多く、Jharkhand・Ngema の頭で待つと戻せなかった)
        if japanese_name_prefix(&lower) || (lower.len() >= 4 && english_name_prefix(&lower)) { return None; }
        // 頭がそのまま名前で、後ろが助詞・敬称なら、名前の後ろで切る (Aiko + は・Takemoto + さん・Einstein + の)。助詞・敬称の打ちかけ (Aikoh の h) なら待つ
        if let Some(k) = (2..lower.len()).rev().find(|&k| is_japanese_name(&lower[..k]) || english_name_case(&lower[..k]).is_some()) {
            let rest = &lower[k..];
            if !typing_word && (cap_boundary(rest) || HONORIFICS.iter().any(|h| rest.starts_with(h))) && native_romaji_readable(rest, true) {
                return Some(k);
            }
            if CAP_BOUNDARY.iter().chain(HONORIFICS).any(|b| b.starts_with(rest)) { return None; }
        }
    }
    // 最後の英字の語より後ろで、英字を含む最初のローマ字の語の頭
    let (mut pos, mut start, mut word) = (0, None, None);
    for t in &tokens {
        if t.kind == TokenKind::English {
            start = None;
            word = Some((pos, t.raw.as_str()));
        } else if start.is_none() && t.raw.bytes().any(|b| b.is_ascii_alphabetic()) {
            start = Some(pos);
        }
        pos += t.raw.len();
    }
    let known = |w: &str| w.len() >= 3 && (is_word(w) || words().brands.contains(w) || canonical_case(w).is_some()) && !learned_kana(w);
    let p = start.filter(|&p| p > 0);
    let head_word = p.and_then(|p| word.filter(|(at, w)| at + w.len() == p)).map(|(_, w)| w.to_ascii_lowercase());
    if rules.name_guard && letters_only && !typing_word {
        // ⑤ 表の英単語 (読める語も) + 助詞 (Amazon + で)。名前と名前の打ちかけは上で除いた
        if let (Some(p), Some(w)) = (p, head_word.as_deref()) {
            if known(w) && cap_boundary(&lower[p..]) { return Some(p); }
        }
        // ⑤ 大文字 1 字 + 読めるローマ字 (区切りの道具の位置によらず 1 字目で切る: A + もんだい・S + さいず)。頭の英字の語が表の英単語なら使わない。
        // syllable_guard なら、頭の 2 字が音節の頭 (Ko・Shi・Kyo・Da) でないときだけ
        let head_is_word = tokens.first().is_some_and(|t| t.kind == TokenKind::English && known(&t.raw.to_ascii_lowercase()));
        if !head_is_word && lower.len() >= 3 && native_romaji_readable(&lower[1..], true)
            && (!rules.syllable_guard || !SYLLABLES.iter().any(|syl| syl.len() >= 2 && lower.starts_with(syl))) {
            return Some(1);
        }
    }
    let p = p?;
    let tail = &lower[p..];
    let after_digit = !ascii.as_bytes()[p - 1].is_ascii_alphabetic();
    let known_particle = !typing_word && head_word.as_deref().is_some_and(|w| known(w)
        && (!rules.known_unreadable || !native_romaji_readable(w, false))) && cap_boundary(tail);
    let initial_honorific = !typing_word && p == 1 && HONORIFICS.iter().any(|h| tail.starts_with(h));
    let unreadable = !typing_word && rules.unreadable_tail > 0 && tail.len() >= rules.unreadable_tail
        && native_romaji_readable(&tail.replace('-', ""), true) && !native_romaji_readable(&letters, true);
    let ok = rules.any || (rules.after_digit && after_digit) || (rules.known_particle && known_particle)
        || (rules.initial_honorific && initial_honorific) || unreadable;
    ok.then_some(p)
}

thread_local! {
    /// 試験で名前の表を差し替える (None なら表のファイルを読む)
    static NAMES_FOR_TEST: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// 2026-11-03 (仕事 9c): よくある日本の名前のローマ字 (japanese_names_romaji.txt。2026-11-05 から Mozc の公開辞書の人名と Wikidata の日本の人物の名前のカナから
/// tools/names/build_names_v2.py・select_names.py で作る。34,353 綴り)。
/// 小文字で並べたもの。試験のビルドでは AZK_NAMES_FILE で別の表を読める (表に無い名前を測るため)
fn japanese_names() -> &'static Vec<String> {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let path = test_env("AZK_NAMES_FILE").map(std::path::PathBuf::from)
            .or_else(|| test_env("AZK_WORDS_DIR").map(std::path::PathBuf::from).or_else(dll_dir).map(|d| d.join("japanese_names_romaji.txt")));
        let text = path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let mut names: Vec<String> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_ascii_lowercase).collect();
        names.sort();
        names.dedup();
        names
    })
}

/// 2026-11-04 (仕事 19 の 2): 英語の固有名詞 (english_names.txt。Wikidata の言語版の多い人の姓・地名・組織の英語のラベルで、ローマ字として読めない語)。
/// (小文字, 表記) を小文字の順に。区切りの判定の語には入れない (入れると hand over が han + Dover に、merge が teMer + ge になった)。
/// 使うのは、すでに英字の語になった所の字の形 (einsteinno → Einsteinの) と、Shift で打った英字の守り (日本の名前の表と同じ所) だけ。
/// 試験のビルドでは AZK_ENGLISH_NAMES で別の表を読める (off で読まない)
fn english_names() -> &'static Vec<(String, String)> {
    static NAMES: OnceLock<Vec<(String, String)>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let path = match test_env("AZK_ENGLISH_NAMES") {
            Some(off) if off == "off" => None,
            Some(path) => Some(std::path::PathBuf::from(path)),
            None => test_env("AZK_WORDS_DIR").map(std::path::PathBuf::from).or_else(dll_dir).map(|d| d.join("english_names.txt")),
        };
        let text = path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let mut names: Vec<(String, String)> = parse_case_table(&text).into_iter().filter(|(lower, _)| lower.len() >= 4).collect();
        names.sort();
        names
    })
}

thread_local! {
    /// 試験で英語の固有名詞の表を差し替える (None なら表のファイルを読む)
    static ENGLISH_NAMES_FOR_TEST: std::cell::RefCell<Option<Vec<(String, String)>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn set_test_english_names(list: &[&str]) {
    let mut names: Vec<(String, String)> = list.iter().map(|w| (w.to_ascii_lowercase(), w.to_string())).collect();
    names.sort();
    ENGLISH_NAMES_FOR_TEST.with(|n| *n.borrow_mut() = Some(names));
}

fn with_english_names<T>(f: impl FnOnce(&[(String, String)]) -> T) -> T {
    ENGLISH_NAMES_FOR_TEST.with(|names| match names.borrow().as_ref() {
        Some(names) => f(names),
        None => f(english_names()),
    })
}

/// 英語の固有名詞の表の表記 (einstein → Einstein)。小文字だけの語
pub fn english_name_case(w: &str) -> Option<String> {
    if !w.bytes().all(|b| b.is_ascii_lowercase()) { return None; }
    with_english_names(|names| names.binary_search_by(|(lower, _)| lower.as_str().cmp(w)).ok().map(|i| names[i].1.clone()))
}

fn english_name_prefix(p: &str) -> bool {
    with_english_names(|names| {
        let i = names.partition_point(|(lower, _)| lower.as_str() < p);
        names.get(i).is_some_and(|(lower, _)| lower.starts_with(p))
    })
}

fn with_japanese_names<T>(f: impl FnOnce(&[String]) -> T) -> T {
    NAMES_FOR_TEST.with(|names| match names.borrow().as_ref() {
        Some(names) => f(names),
        None => f(japanese_names()),
    })
}

fn is_japanese_name(w: &str) -> bool {
    with_japanese_names(|names| names.binary_search_by(|n| n.as_str().cmp(w)).is_ok())
}

fn japanese_name_prefix(p: &str) -> bool {
    with_japanese_names(|names| {
        let i = names.partition_point(|n| n.as_str() < p);
        names.get(i).is_some_and(|n| n.starts_with(p))
    })
}

#[cfg(test)]
pub fn set_test_names(list: &[&str]) {
    let mut names: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    names.sort();
    NAMES_FOR_TEST.with(|n| *n.borrow_mut() = Some(names));
}

/// 打った文字列全体を区切る。英字以外 (記号・数字) はローマ字側に回す
pub fn segment(raw: &str) -> Vec<Token> {
    let _learning = SegmentLearning::new();
    english_punctuation(merge_contractions(raw, segment_with(raw, style())))
}

/// 2026-10-30: 英語の短縮形 (it's・I'm・don't・can't・you're・we'll・I've・she'd) を 1 つの英字の語にする。
/// 前は ' のあとの s・t・m を英字の語に数えず (it + ' + s)、Space が確定と空白にならずに変換へ回り、英語の文の文脈も続かなかった
/// (it's correct. の . が 。 になった)。n't の n はローマ字の ん になっていた (can't → かん't)。かなモードで ' を打つ日本語はまず無い
fn merge_contractions(raw: &str, tokens: Vec<Token>) -> Vec<Token> {
    if !raw.contains('\'') || !raw.is_ascii() {
        return tokens;
    }
    // 区切りの切れ目 (バイトの位置)
    let mut cuts = vec![0];
    for token in &tokens {
        cuts.push(cuts.last().unwrap() + token.raw.len());
    }
    if *cuts.last().unwrap() != raw.len() {
        return tokens;
    }
    let bytes = raw.as_bytes();
    let mut spans: Vec<(usize, usize)> = vec![];
    for (p, _) in raw.match_indices('\'') {
        // ' の後ろの字 (切れ目で終わる s・t・m・re・ve・ll・d)
        let after = bytes[p + 1..].iter().take_while(|b| b.is_ascii_alphabetic()).count();
        let suffix_end = ["re", "ve", "ll", "s", "t", "m", "d"].iter()
            .filter(|x| after >= x.len() && raw[p + 1..p + 1 + x.len()].eq_ignore_ascii_case(x) && cuts.contains(&(p + 1 + x.len())))
            .map(|x| p + 1 + x.len()).max();
        let Some(end) = suffix_end else { continue };
        let suffix = raw[p + 1..end].to_ascii_lowercase();
        // ' の前の語 (切れ目から始まるいちばん長いもの)
        let start = cuts.iter().copied().filter(|&c| c < p && bytes[c..p].iter().all(|b| b.is_ascii_alphabetic())).find(|&c| {
            let word = raw[c..p].to_ascii_lowercase();
            match suffix.as_str() {
                "t" => word.len() >= 3 && word.ends_with('n')
                    && (matches!(word.as_str(), "can" | "won" | "don" | "ain" | "shan") || is_word(&word[..word.len() - 1])),
                "m" => word == "i",
                "s" => is_word(&word) || matches!(word.as_str(), "it" | "he" | "she" | "that" | "what" | "who" | "there" | "here" | "let"),
                _ => is_word(&word) || matches!(word.as_str(), "i" | "you" | "we" | "they" | "he" | "she" | "it" | "who" | "that" | "there"),
            }
        });
        if let Some(start) = start {
            spans.push((start, end));
        }
    }
    if spans.is_empty() {
        return tokens;
    }
    let mut out = vec![];
    let mut at = 0;
    let mut k = 0;
    while k < tokens.len() {
        if let Some(&(start, end)) = spans.iter().find(|(start, _)| *start == at) {
            out.push(Token { raw: raw[start..end].to_string(), kind: TokenKind::English });
            while at < end {
                at += tokens[k].raw.len();
                k += 1;
            }
            continue;
        }
        at += tokens[k].raw.len();
        out.push(tokens[k].clone());
        k += 1;
    }
    out
}

thread_local! {
    // 2026-10-14: 未確定の文字列の前が、日本語を含む文の途中か (english_punctuation に使う)
    static JAPANESE_BEFORE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    // 確定した文字列だけから見た、今の文に日本語があるか (キャレットの前を読ませてくれないアプリ用)。入力欄が変わったら None
    static COMMITTED_JAPANESE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

fn japanese_char(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
        | '\u{ff66}'..='\u{ff9f}' | '\u{ff10}'..='\u{ff19}'
        | '、' | '，' | '「' | '」' | '『' | '』' | '（' | '）' | '・' | '々' | '〜')
}

/// 文字列の最後の文 (文の終わり 。！？ 改行、空白か末尾が続く . ! ? より後ろ) に日本語の字があるか。
/// 文の終わりが見つからず日本語も無ければ None (もっと前を見ないと分からない)
fn japanese_in_last_sentence(text: &str) -> Option<bool> {
    let chars: Vec<char> = text.chars().collect();
    for k in (0..chars.len()).rev() {
        let c = chars[k];
        if japanese_char(c) {
            return Some(true);
        }
        let next_blank = chars.get(k + 1).map_or(true, |n| n.is_whitespace());
        if matches!(c, '。' | '！' | '？' | '：' | '「' | '\n' | '\r') || (matches!(c, '.' | '!' | '?' | ':') && next_blank) {
            return Some(false);
        }
    }
    None
}

/// 確定した文字列を覚える (今の文に日本語があるかを引き継ぐ)。None で忘れる (入力欄が変わったとき)
pub fn note_commit(text: Option<&str>) {
    COMMITTED_JAPANESE.set(text.map_or(None, |text| japanese_in_last_sentence(text).or(COMMITTED_JAPANESE.get())));
}

/// 未確定の文字列を始めるときに、前の文字列を見ておく。before = (キャレットの前の文字列, 入力欄の先頭まで届いたか)、読めなければ None。
/// 文の始まりまでさかのぼって日本語があれば日本語の文。届かず決まらなければ、確定した文字列から見た状態で決める
pub fn set_text_before(before: Option<(&str, bool)>) {
    let committed = || COMMITTED_JAPANESE.get().unwrap_or(false);
    JAPANESE_BEFORE.set(match before {
        Some((text, reached_start)) => japanese_in_last_sentence(text).unwrap_or_else(|| if reached_start { false } else { committed() }),
        None => committed(),
    });
}

/// 2026-10-14: 英単語のすぐ後ろの , . は 、 。 にせずコンマ・ピリオドのまま。ただし日本語の文の中
/// (明日はhtml、) では 、 。 のまま。文 (。！？ にあたる . ! ? と 「 」 で区切る) ごとに、日本語のはっきりした手がかりが
/// あるか (japanese_evidence)、すぐ後ろに字が続く , . があるか (英語なら後ろに空白を入れる)、最初の文なら未確定の文字列の前が
/// 日本語を含む文の途中かで決める。後ろに日本語を打ち足せば、
/// さかのぼって 、 。 に戻る (区切りは毎回打った列全体から作り直すため)。
/// 英字の語にはさまれた . (example.com・config.json) は文の終わりではなく、いつもピリオドのまま
fn english_punctuation(mut tokens: Vec<Token>) -> Vec<Token> {
    let alnum_end = |t: &Token| t.kind == TokenKind::English && t.raw.chars().last().is_some_and(|c| c.is_ascii_alphanumeric());
    let alnum_start = |t: &Token| t.kind == TokenKind::English && t.raw.chars().next().is_some_and(|c| c.is_ascii_alphanumeric());
    for k in 1..tokens.len().saturating_sub(1) {
        if tokens[k].kind == TokenKind::Romaji && tokens[k].raw == "." && alnum_end(&tokens[k - 1]) && alnum_start(&tokens[k + 1]) {
            tokens[k].kind = TokenKind::English;
        }
    }
    let ends = |t: &Token| t.kind == TokenKind::Romaji && matches!(t.raw.as_str(), "." | "!" | "?" | "[" | "]");
    // 英語ではコンマ・ピリオドの後ろに空白を入れる。すぐ後ろに字が続く , . は日本語の 、 。 の打ち方 (OK、了解・API、DB、UI)
    let tight: Vec<bool> = (0..tokens.len()).map(|k| {
        tokens[k].kind == TokenKind::Romaji && matches!(tokens[k].raw.as_str(), "," | ".")
            && tokens.get(k + 1).and_then(|t| t.raw.chars().next()).is_some_and(|c| c.is_ascii_alphanumeric() || !c.is_ascii())
    }).collect();
    let mut start = 0;
    while start < tokens.len() {
        let end = (start..tokens.len()).find(|&k| ends(&tokens[k])).map_or(tokens.len(), |k| k + 1);
        // 2026-10-30: 英語の文の途中 (確定した it's + 空白 のあとに correct.tokanozyohaitta.) の最初の文は、すぐ後ろに字が続く . も
        // 英語の文の終わりの . (。 にしない)。. を打った時点では後ろが日本語かは分からない。日本語の手がかりが文の中にあれば今までどおり
        let japanese_sentence = if start == 0 && in_english_sentence() {
            japanese_evidence(&tokens[start..end])
        } else {
            (start == 0 && JAPANESE_BEFORE.get()) || tight[start..end].contains(&true) || japanese_evidence(&tokens[start..end])
        };
        if !japanese_sentence {
            for k in start.max(1)..end {
                if tokens[k].kind == TokenKind::Romaji && matches!(tokens[k].raw.as_str(), "," | ".") && tokens[k - 1].kind == TokenKind::English {
                    tokens[k].kind = TokenKind::English;
                }
            }
        }
        start = end;
    }
    tokens
}

/// 文の中に、日本語の文だとはっきり分かる手がかりがあるか。英語の文にも、ローマ字で読める語 (in・to・Osaka・ramen) や
/// 日本語の語 (I like 寿司, ...) は混ざるので、ローマ字の語があるだけでは日本語の文としない。手がかりにするのは、
/// 英字の語にすき間なくつながったローマ字 (ashitaha+html・css+woyaru)、は・を で終わる語 (kyouha)、
/// です・ます・した を含む語、10 字以上のローマ字の語 (watashinoyumehato)、打ったかな
fn japanese_evidence(tokens: &[Token]) -> bool {
    let letters = |t: &Token| t.kind == TokenKind::Romaji && !t.raw.is_empty() && t.raw.chars().all(|c| c.is_ascii_alphabetic());
    let mut k = 0;
    while k < tokens.len() {
        if !letters(&tokens[k]) {
            if tokens[k].kind == TokenKind::Romaji && tokens[k].raw.chars().any(|c| !c.is_ascii()) {
                return true;
            }
            k += 1;
            continue;
        }
        let first = k;
        while k < tokens.len() && letters(&tokens[k]) {
            k += 1;
        }
        let word: String = tokens[first..k].iter().map(|t| t.raw.to_ascii_lowercase()).collect();
        let glued = (first > 0 && tokens[first - 1].kind == TokenKind::English && tokens[first - 1].raw.chars().last().is_some_and(|c| c.is_ascii_alphanumeric()))
            || (k < tokens.len() && tokens[k].kind == TokenKind::English && tokens[k].raw.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()));
        let shaped = word.len() >= 10 || (word.len() >= 4
            && (word.ends_with("ha") || word.ends_with("wo") || word.starts_with("wo")
                || ["desu", "masu", "shita", "sita", "mashi", "deshi"].iter().any(|e| word.contains(e))));
        if glued || shaped {
            return true;
        }
    }
    false
}

/// ピリオドで終わる英語の略語。打ったピリオドまで英字のまま入れる (ex. → エx。 にしない)。
/// 日本語の語尾と区別がつかないもの (no. = の。、min. = …みん。) は入れない
const ABBREVIATIONS: &[&str] = &[
    "ex.", "e.g.", "i.e.", "etc.", "vs.", "cf.", "approx.", "fig.", "figs.", "eq.", "ref.", "refs.", "vol.", "pp.",
    "mr.", "mrs.", "ms.", "dr.", "prof.", "jr.", "sr.", "inc.", "ltd.", "corp.", "dept.", "est.", "avg.", "sec.",
    "ch.", "a.m.", "p.m.", "u.s.", "u.k.", "e.x.",
];

/// raw[..end] が略語で終わり、その前が語の切れ目なら略語の始まりの位置
fn abbreviation_start(raw: &str, end: usize) -> Option<usize> {
    let lower = raw[..end].to_ascii_lowercase();
    for abbreviation in ABBREVIATIONS {
        if !lower.ends_with(abbreviation) {
            continue;
        }
        let start = end - abbreviation.len();
        if start == 0 {
            return Some(start);
        }
        let previous = raw.as_bytes()[start - 1];
        if !previous.is_ascii_alphabetic() {
            return Some(start);
        }
        // 前が英字なら、その英字の連続がローマ字として読み切れるときだけ (korehaex. → これは + ex.)
        let run_start = raw[..start].rfind(|c: char| !c.is_ascii_alphabetic()).map_or(0, |i| i + 1);
        let (tokens, flags) = segment_run_raw(&raw[run_start..start], style_for_abbreviation());
        if tokens.iter().all(|t| t.kind == TokenKind::Romaji) && !flags.iter().any(|f| *f) {
            return Some(start);
        }
    }
    None
}

thread_local! {
    static ABBREVIATION_STYLE: std::cell::Cell<Option<Style>> = const { std::cell::Cell::new(None) };
}

fn style_for_abbreviation() -> Style {
    ABBREVIATION_STYLE.get().unwrap_or_else(style)
}

/// アクセント付きのラテン文字 (ñ é ü ø ß …)
fn is_accented_latin(c: char) -> bool {
    !c.is_ascii() && c.is_alphabetic() && ('\u{00C0}'..='\u{024F}').contains(&c) && c != '\u{00D7}' && c != '\u{00F7}'
}

/// 2026-10-13: 大文字で始まる語のすぐ後ろに来やすい日本語の頭 (助詞・敬称・する)。Figma|で、Suica|の、Qwen|した、K|の
const CAP_BOUNDARY: &[&str] = &["ha", "ga", "wo", "no", "ni", "de", "to", "mo", "he", "ya", "ka", "tte", "sann", "san", "kunn", "kun",
    "tyann", "chann", "chan", "sama", "sita", "shita", "suru", "site", "shite", "dato", "datte", "kara", "made", "dake", "toka", "nado", "yori"];

const STRONG_BOUNDARY: &[&str] = &["ha", "ga", "wo", "no", "ni", "de", "to", "mo", "he"];

/// 2026-10-13: 英単語が、l・x で打った小さい っ (ltu・ltsu・xtu・xtsu) や x で打った小さいかな (xa・xya) の頭の l・x で終わる区切り (moral + tute、max + abaito) は割高に。
/// 小書きを l・x で明示して打つのは日本語 (小書きは x でも l でも打たれる。moraltute → もらって)。
/// l・x の手前までがローマ字で読めるときだけ (mora + ltu)。tmux + tsukatteru の tmu は読めないので英単語のまま
fn splits_small_tsu(lower: &str, i: usize, j: usize) -> i64 {
    if j == 0 || j - 1 <= i { return 0; }
    let rest = &lower[j - 1..];
    // 2026-10-13 夜: x で打つ小さいかな (xa = ぁ・xya = ゃ) も同じ (まぁ = ma + xa を max + a にしない。まぁバイトでも)
    let small = ["ltu", "ltsu", "xtu", "xtsu", "xa", "xi", "xu", "xe", "xo", "xya", "xyu", "xyo", "xwa"].iter().any(|p| rest.starts_with(p));
    if small && native_romaji_readable(&lower[i..j - 1], false) { 300 } else { 0 }
}

/// 2026-10-13: t で終わる英単語のすぐ後ろが ch + 母音 (mat + ちゃ、it + ち、bot + ち) なら割高に (1/100 単位)。
/// tch と打つのは英単語の一部 (match) か っち (matcha・botchi) で、t で終わる語 + ち と割ることはまずない
fn splits_tch(lower: &str, j: usize) -> i64 {
    let b = lower.as_bytes();
    if j < 1 || b[j - 1] != b't' { return 0; }
    let rest = &lower[j..];
    // 打っている途中で ch・c で止まっている (metch) ときも同じ (母音が来る前に met を英字にしない)
    let pending = RUN_AT_END.get() && !FINISHING.get() && (rest == "ch" || rest == "c");
    if pending || (rest.starts_with("ch") && b.get(j + 2).is_some_and(|&c| is_vowel(c))) { 200 } else { 0 }
}

/// 2026-10-31: j から後ろが日本語として読めるか。並びの終わりまでローマ字として読めるか (suffixes[j])、
/// 読めるローマ字の並びのすぐ後ろに表の英単語 (3 字以上) が来るなら読めるとみなす (並びの終わりまでだけを見ると、
/// 同じ入力の後ろに英単語があるときは、いつも読めないことになっていた)。見るのは 40 字先まで
fn japanese_until_english(lower: &str, j: usize, suffixes: &[bool]) -> bool {
    if suffixes[j] { return true; }
    let n = lower.len();
    ((j + 1)..n.min(j + 40)).any(|p| lower.is_char_boundary(p) && native_romaji_readable(&lower[j..p], false)
        && ((p + 3)..=n.min(p + 30)).any(|q| lower.is_char_boundary(q) && is_lexicon_word(&lower[p..q])))
}

fn cap_boundary(s: &str) -> bool {
    CAP_BOUNDARY.iter().any(|b| s.starts_with(b))
}

/// 2026-10-13: 区切りの計算で英語にする 2 文字の語。ローマ字として読めないものだけが効く (o+r の r が読めない文字として残るより安く)
const TWO_LETTER_DP: &[&str] = &["or", "at", "if", "of", "up", "us", "id", "ok", "by", "my", "it", "is", "as", "am"];

/// 英文の途中で英語として扱う 2 文字の語 (ふだんは短すぎて英語と判定しない)
const TWO_LETTER_WORDS: &[&str] = &[
    "am", "an", "as", "at", "be", "by", "do", "go", "he", "if", "in", "is", "it", "me", "my", "no", "of", "ok", "on", "or", "so",
    "to", "up", "us", "we",
];

thread_local! {
    /// 2026-10-13: 今区切っている英字の連続が入力の末尾か (英単語の頭 = 打っている途中の語、は末尾でだけ認める。Dde-ta の Dde を英字にしない)
    static RUN_AT_END: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

thread_local! {
    /// 2026-10-13: 今区切っている英字の連続のすぐ後ろが数字か (Qwen3.6 の Qwen は読める続きでも 1 語に。USB-Cke- の - の前ではしない)
    static RUN_BEFORE_DIGIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    /// Space を押した時点の区切りか (末尾の n 1 つの扱いだけが変わる)
    static FINISHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// ん を nn と打つ人か (設定の kunrei_nn、音ごとの推定で n 1 つに加点がある人)
fn nn_typist(style: Style) -> bool {
    style.kunrei_nn || (style.adaptive.is_some() && style.penalty("n") > 0)
}

/// lower[i..j] が n で終わる英単語で、すぐ後ろが n + 母音 (mainni = main + に) のとき、nn と打つ人には英語と読むか。
/// ローマ字としては ま・い・nn・い とも読めるので、まいんい が日本語の語にならないときだけ (english_n_split.txt)
fn n_split_english(lower: &str, i: usize, j: usize, style: Style) -> bool {
    let b = lower.as_bytes();
    if !nn_typist(style) || j + 1 >= b.len() || b[j] != b'n' || !is_vowel(b[j + 1]) {
        return false;
    }
    let w = &lower[i..j];
    words().n_split.get(w).is_some_and(|v| v.as_bytes().contains(&b[j + 1])) && !n_straddles_japanese_head(lower, i, j)
}

/// 2026-11-23: ん をまたぐ読みが日本語の語の頭としてよく出るときは、n_split を当てない (日本語の Wikipedia の語の頭の 3 音の回数、japanese_heads.txt)。
/// 英単語の最後の音 + ん + 後ろの母音 (titan + に の た・ん・い = 単位、taken + に の け・ん・い = 権威) は、英単語の頭から読んだ読み (ちたんい) とは別に
/// 英単語の途中から始まる日本語の語になりうる。english_n_split.txt は英単語の頭から読んだ読みだけを JMdict で確かめているので、ここで見る
/// (訓令式＋nn で 町単位で = matitannide が、ま + titan + に + で になっていた。titan は英語以外の言語の表の語)。
/// 線 (N_SPLIT_HEAD_FLOOR) は 200・500・1,000 を比べ、日本語の文 (Wikipedia の節 4 万) で直る数が同じで、英単語 + に・の の組で英字でなくなる語が最も少ない 1,000
/// (main + に の い・ん・い 121 は英語のまま)
const N_SPLIT_HEAD_FLOOR: u32 = 1000;

fn n_split_head_floor() -> u32 {
    static FLOOR: OnceLock<u32> = OnceLock::new();
    *FLOOR.get_or_init(|| test_env("AZK_N_SPLIT_HEAD").and_then(|s| s.trim().parse().ok()).unwrap_or(N_SPLIT_HEAD_FLOOR))
}

/// lower[i..j] (n で終わる英単語) の最後の音 (n の前の 1 音) から 3 音 (その音・ん・後ろの音) が、日本語の語の頭として線より多く出るか
fn n_straddles_japanese_head(lower: &str, i: usize, j: usize) -> bool {
    let floor = n_split_head_floor();
    if floor == 0 || j < i + 2 { return false; }
    let end = j - 1;
    let table = romaji_kana_table();
    let Some(start) = (end.saturating_sub(4).max(i)..end).find(|&k| lower.get(k..end).is_some_and(|key| table.contains_key(key))) else { return false };
    let morae = kana_morae(lower, start, 3);
    morae.len() == 3 && morae[1] == "ん" && japanese_head_count(&morae.concat()) > floor
}

/// 末尾の n 1 つを ん と読むときの加点 (1/100 単位)。nn と打つ人だけ。英単語 (加点 100) より高くする
fn finishing_n_penalty(style: Style) -> i64 {
    if style.kunrei_nn {
        200
    } else if nn_typist(style) {
        200 + style.penalty("n")
    } else {
        0
    }
}

/// Space を押した時点の区切り (末尾の n 1 つを、nn と打つ人には英語のしるしとして見る)。
/// 2026-11-08 (仕事 12): 空白なしで打った英語の句 (seeyoutomorrow) は、英語の語ごとの英字の区切りにする (english_phrases)
pub fn segment_finishing(raw: &str) -> Vec<Token> {
    segment_finishing_with_spaces(raw).0
}

/// 打ち終えたときの区切りと、英語の句の語の切れ目 (打った列のバイトの位置。変換サーバーへ入れるとき、ここに半角の空白を入れる)
pub fn segment_finishing_with_spaces(raw: &str) -> (Vec<Token>, Vec<usize>) {
    FINISHING.set(true);
    let tokens = segment(raw);
    FINISHING.set(false);
    let tokens = keep_trailing_stray(raw, tokens);
    // 2026-11-11: 製品名 (aftereffects → After Effects) があれば、それを使う (英語の句の分けはしない)
    let (tokens, spaces, matched, _) = product_phrases(raw, tokens);
    if matched { return (tokens, spaces); }
    english_phrases(raw, tokens)
}

/// 2026-11-19: 打ち終えたときの区切りで当てた製品名の (打った綴り, 表記) (aftereffects, After Effects)。確定の直後の Backspace で戻す形と、
/// この IME の間は当てない綴りに使う (undo_for_spaces)
pub fn finishing_products(raw: &str) -> Vec<(String, String)> {
    FINISHING.set(true);
    let tokens = segment(raw);
    FINISHING.set(false);
    product_phrases(raw, keep_trailing_stray(raw, tokens)).3
}

/// 2026-11-19 (まとめ役の rc-next 234b9b9 の見直しの 3): 空白を入れて確定した直後の Backspace で戻す形と、この IME の間は当てない並び。
/// 英語の句は、空白を抜いた形と、空白を含む英字の並び (phrase_runs)。製品名 (products が空でない) は、表記を打った綴り (小文字) に戻し
/// (After Effects → aftereffects。前は空白を抜くだけで AfterEffects に戻していた)、当てた綴りそのもの (aftereffects・windows11) を覚える
/// (前は空白の前後の英字の並びを覚えていて、aftereffectsde・windows のように当てる綴りと違い、次も当たっていた)
pub fn undo_for_spaces(text: &str, raw: &str, spaces: &[usize], products: &[(String, String)]) -> (String, Vec<String>) {
    if products.is_empty() {
        return (text.replace(' ', ""), phrase_runs(raw, spaces));
    }
    let mut plain = text.replace(' ', "");
    let mut runs: Vec<String> = vec![];
    for (piece, form) in products {
        let joined = form.replace(' ', "");
        if let Some(at) = plain.find(&joined) {
            plain.replace_range(at..at + joined.len(), piece);
        }
        if !runs.contains(piece) { runs.push(piece.clone()); }
    }
    (plain, runs)
}

/// 2026-11-11: 打っている途中の区切り (segment) に、製品名の表の表記を当てたもの (打ち終える前にも After Effects と見せ、Enter でもその表記で確定する)
pub fn segment_with_products(raw: &str) -> (Vec<Token>, Vec<usize>) {
    let (tokens, spaces, _, _) = product_phrases(raw, segment(raw));
    (tokens, spaces)
}

/// 2026-11-11 (After Effects などが入らない件・まとめ役 09:42): ソフト・アプリ・サービスの製品名の表 (english_products.txt。
/// tools/core_products) の、空白を含む表記 (After Effects・DaVinci Resolve・Windows 11)。打つ綴り (小文字の英数字だけ: aftereffects) → 表記。
/// 表記は英数字の語を空白 1 つで並べたものだけ (記号の入る表記は字数が変わるので入れない)。1 語の表記は表記直しの表 (canonical_case) で扱う
fn parse_product_table(text: &str) -> HashMap<String, String> {
    text.lines().map(str::trim)
        .filter(|l| !l.starts_with('#') && l.contains(' ') && !l.contains("  ") && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b' ')
            && l.as_bytes()[0].is_ascii_alphabetic())
        .map(|l| (l.replace(' ', "").to_ascii_lowercase(), l.to_string()))
        .filter(|(key, _)| key.len() <= PRODUCT_MAX_LETTERS)
        .collect()
}

/// 製品名の打つ綴りの字数の上限
const PRODUCT_MAX_LETTERS: usize = 40;

thread_local! {
    /// 試験で使う製品名の表 (set_test_products)
    static TEST_PRODUCTS: std::cell::RefCell<Option<HashMap<String, String>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn set_test_products(forms: &[&str]) {
    TEST_PRODUCTS.with(|t| *t.borrow_mut() = Some(parse_product_table(&forms.join("\n"))));
}

fn product_form(spelling: &str) -> Option<String> {
    if cfg!(test) {
        if let Some(found) = TEST_PRODUCTS.with(|t| t.borrow().as_ref().map(|table| table.get(spelling).cloned())) { return found; }
    }
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let dir = test_env("AZK_WORDS_DIR").map(std::path::PathBuf::from).or_else(dll_dir);
        parse_product_table(&dir.and_then(|d| std::fs::read_to_string(d.join("english_products.txt")).ok()).unwrap_or_default())
    }).get(spelling).cloned()
}

/// 打った列のうち、製品名の表の打つ綴りと同じ所を、表記の語ごとの英字の区切りにする (語は表記の大文字小文字。打った字と大文字小文字だけ違い、字数は同じ)。
/// 見るのは、英数字の並びの頭か、かな (ローマ字) の語のすぐ後ろから始まり、区切りの境目で終わる所 (後ろは並びの終わり・英数字でない字・かなの語)。
/// いちばん長い綴りを採る。小文字と数字だけで打った所だけ (大文字を交ぜて打った綴りはそのまま)。表記直しの設定 (english_case_fix) がオフなら何もしない。
/// 確定の直後の Backspace で空白なしに戻した並びは、この IME の間は当てない (英語の句と同じ DECLINED_PHRASES)。
/// 返すもの: 区切り・空白を入れる所 (打った列のバイトの位置)・当てたか・当てた (打った綴り, 表記) (2026-11-19)
fn product_phrases(raw: &str, tokens: Vec<Token>) -> (Vec<Token>, Vec<usize>, bool, Vec<(String, String)>) {
    if !case_fix_enabled() || !raw.is_ascii() || tokens.is_empty() {
        return (tokens, vec![], false, vec![]);
    }
    let mut starts = Vec::with_capacity(tokens.len() + 1);
    let mut at = 0;
    for t in &tokens {
        starts.push(at);
        at += t.raw.len();
    }
    starts.push(at);
    if at != raw.len() {
        return (tokens, vec![], false, vec![]);
    }
    let bytes = raw.as_bytes();
    let (mut out, mut spaces, mut copied, mut k) = (Vec::with_capacity(tokens.len()), vec![], 0, 0);
    let mut pieces: Vec<(String, String)> = vec![];
    while k < tokens.len() {
        let p = starts[k];
        let word_start = p == 0 || !bytes[p - 1].is_ascii_alphanumeric() || tokens[k - 1].kind == TokenKind::Romaji;
        let mut found = None;
        if word_start && bytes[p].is_ascii_lowercase() {
            for e in (k + 1..=tokens.len()).rev() {
                let end = starts[e];
                let piece = &raw[p..end];
                if piece.len() > PRODUCT_MAX_LETTERS || !piece.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) { continue; }
                if !(end == raw.len() || !bytes[end].is_ascii_alphanumeric() || tokens[e].kind == TokenKind::Romaji) { continue; }
                if DECLINED_PHRASES.with(|d| d.borrow().contains(piece)) { continue; }
                if let Some(form) = product_form(piece) {
                    found = Some((e, form));
                    break;
                }
            }
        }
        match found {
            Some((e, form)) => {
                pieces.push((raw[p..starts[e]].to_string(), form.to_string()));
                out.extend(tokens[copied..k].iter().cloned());
                let words: Vec<&str> = form.split(' ').collect();
                let mut q = p;
                for (n, w) in words.iter().enumerate() {
                    out.push(Token { raw: w.to_string(), kind: TokenKind::English });
                    q += w.len();
                    if n + 1 < words.len() { spaces.push(q); }
                }
                copied = e;
                k = e;
            }
            None => k += 1,
        }
    }
    if copied == 0 {
        return (tokens, vec![], false, vec![]);
    }
    out.extend(tokens[copied..].iter().cloned());
    (out, spaces, true, pieces)
}

/// 2026-11-08 (仕事 12): 空白なしで打った英語の句を分けるのは、小文字だけの英字の並び (英字以外の字で区切った 1 続き) がこの字数以上のとき
const PHRASE_MIN_LETTERS: usize = 6;
/// 切り方の点の幅 (-ln P の 1000 倍)。2 番目の切り方・並び全体を 1 語とみた点より、これだけよいときだけ分ける
const PHRASE_MARGIN: i64 = 2000;
/// めったに出ない語 (-ln P(w) の 1000 倍がこれより大きい。表の 4 字以上の英単語は除く) を含む切り方は採らない
const PHRASE_RARE_WORD: u32 = 13000;
/// 今の区切りで表の英単語になっている語のうち、中で切らない (まとめる) 語の字数。つないだ綴りが表 (社名の表を除く) にあればまとめる字数も同じ
const PHRASE_KEEP_LETTERS: usize = 7;

/// 2026-11-08 (仕事 12): 設定 input.english_phrase_spaces (初期値オン)。試験のビルドではいつもオン (次の語の表を読ませた試しのときだけ効く)
fn phrase_spaces_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| cfg!(test) || shared::AppConfig::read_or_default().input.english_phrase_spaces)
}

thread_local! {
    /// 2026-11-08 (仕事 12): 空白を入れた直後の Backspace で空白なしに戻した並び (小文字)。この IME の間は、同じ並びを分けない
    static DECLINED_PHRASES: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());
}

/// 空白を入れた英語の句を、確定の直後の Backspace で空白なしに戻した (composition.rs の UndoAutocorrect)
pub fn decline_phrase(run: &str) {
    DECLINED_PHRASES.with(|d| { d.borrow_mut().insert(run.to_ascii_lowercase()); });
}

/// 2026-11-08 (仕事 12): 空白を入れた所 (segment_finishing_with_spaces の 2 つめ) を含む英字の並び (小文字)
pub fn phrase_runs(raw: &str, spaces: &[usize]) -> Vec<String> {
    let bytes = raw.as_bytes();
    let mut runs: Vec<String> = vec![];
    for &at in spaces {
        let (mut i, mut j) = (at.min(bytes.len()), at.min(bytes.len()));
        while i > 0 && bytes[i - 1].is_ascii_alphabetic() { i -= 1; }
        while j < bytes.len() && bytes[j].is_ascii_alphabetic() { j += 1; }
        let run = raw[i..j].to_ascii_lowercase();
        if !runs.contains(&run) { runs.push(run); }
    }
    runs
}

/// 英語の句を切るのに使う、英語の次の語の表 (next_en.hwn) の引き方。試験では小さな表に差し替える
trait PhraseTable {
    fn find(&self, word: &str) -> Option<usize>;
    fn uni(&self, id: usize) -> u32;
    fn mult(&self, id: usize) -> u32;
    fn successors(&self, id: usize) -> HashMap<usize, u32>;
}

impl PhraseTable for super::next_word::Table {
    fn find(&self, word: &str) -> Option<usize> { self.spell_find(word) }
    fn uni(&self, id: usize) -> u32 { self.spell_uni(id) }
    fn mult(&self, id: usize) -> u32 { self.spell_mult(id) }
    fn successors(&self, id: usize) -> HashMap<usize, u32> { self.spell_successors(id) }
}

/// 打ち終えたときの区切りのうち、空白なしで打った英語の句の並びを英語の語ごとの英字の区切りにし、語の切れ目を返す。
/// 見るのは、英字以外の字で区切った小文字だけの英字の並び (6 字以上。大文字を含む並びは今も英字の判定に入る) で、区切りがその並びの端で切れているもの
fn english_phrases(raw: &str, tokens: Vec<Token>) -> (Vec<Token>, Vec<usize>) {
    if !phrase_spaces_enabled() || !raw.is_ascii() || raw.len() < PHRASE_MIN_LETTERS {
        return (tokens, vec![]);
    }
    let Some(table) = super::next_word::spell_tables().iter().find(|t| t.spell_language() == "en") else { return (tokens, vec![]) };
    let mut starts = Vec::with_capacity(tokens.len() + 1);
    let mut at = 0;
    for t in &tokens {
        starts.push(at);
        at += t.raw.len();
    }
    starts.push(at);
    if at != raw.len() {
        return (tokens, vec![]);
    }
    let bytes = raw.as_bytes();
    let (mut out, mut spaces, mut copied) = (Vec::with_capacity(tokens.len()), vec![], 0);
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
            j += 1;
        }
        let run = &raw[i..j];
        let ends = (starts.iter().position(|&s| s == i), starts.iter().position(|&s| s == j));
        if let (true, (Some(a), Some(b))) = (run.len() >= PHRASE_MIN_LETTERS && run.bytes().all(|c| c.is_ascii_lowercase()), ends) {
            let declined = DECLINED_PHRASES.with(|d| d.borrow().contains(run));
            if let Some(words) = (!declined && a >= copied && a < b).then(|| phrase_words(run, &tokens[a..b], table, &is_word, &is_lexicon_word)).flatten() {
                out.extend(tokens[copied..a].iter().cloned());
                let mut p = i;
                for (n, w) in words.iter().enumerate() {
                    out.push(Token { raw: raw[p..p + w.len()].to_string(), kind: TokenKind::English });
                    p += w.len();
                    if n + 1 < words.len() {
                        spaces.push(p);
                    }
                }
                copied = b;
            }
        }
        i = j;
    }
    if spaces.is_empty() {
        return (tokens, vec![]);
    }
    out.extend(tokens[copied..].iter().cloned());
    (out, spaces)
}

/// 1 つの並びを英語の句として分けた語 (2 語以上)。分けないなら None。決め方は t12/eval12.py と同じ (dev 2,984 句で決め、test 3,021 句で
/// 正しく分けた 848・違う分け方 12・日本語のローマ字の文 (ヘボン式・訓令式・一般の文 3,675・仕事 20 の打鍵) を英語にした 0):
/// 1. 今の区切りが、表に無い小文字の英字も読めない子音 1 字も無く、かなと英字が混ざる (so|no|[bag] = その bag) なら分けない
/// 2. 今の区切りが全部表の英単語 ([level][seven]) なら、その切れ目のまま (どの 2 語の並びも表に出ているときだけ)
/// 3. 崩れているなら、次の語の表で点 (-ln P(頭の語) - ln P(語|前の語)) のいちばん小さい切り方を探す。表に無い 2 語の並びを含む切り方・
///    2 番目の切り方や並び全体を 1 語とみた点との差が小さい切り方は採らない
/// 4. 割らない: 今の区切りで 7 字以上の表の英単語の中は切らない。つないだ綴りが表 (社名の表は除く) の 7 字以上の語か、
///    つないだ 1 語の点が 2 語に切った点以下 (weekend・keyboard・cannot) ならまとめる
/// 5. めったに出ない語を含めば分けない
fn phrase_words(run: &str, toks: &[Token], table: &dyn PhraseTable, known: &dyn Fn(&str) -> bool, lexicon: &dyn Fn(&str) -> bool) -> Option<Vec<String>> {
    // 崩れ: 表に無い英字の語か、読めない子音 1 字 (英字の語のすぐ前の子音 1 字 (abou|t|[the]) も。っ になる重ねはローマ字の前だけ)
    let broken = toks.iter().enumerate().any(|(j, t)| match t.kind {
        TokenKind::English => !known(&t.raw.to_ascii_lowercase()),
        TokenKind::Romaji => stray_consonant(toks, j) || (t.raw.len() == 1 && t.raw.bytes().all(|c| c.is_ascii_lowercase() && !is_vowel(c) && c != b'n')
            && toks.get(j + 1).is_some_and(|next| next.kind == TokenKind::English)),
    });
    let mut successors: HashMap<usize, HashMap<usize, u32>> = HashMap::new();
    let mut pair = |a: usize, b: usize| successors.entry(a).or_insert_with(|| table.successors(a)).get(&b).copied();
    if !broken {
        if !toks.iter().all(|t| t.kind == TokenKind::English) {
            return None;
        }
        let words: Vec<String> = toks.iter().map(|t| t.raw.to_ascii_lowercase()).collect();
        let ids: Vec<usize> = words.iter().map(|w| table.find(w)).collect::<Option<_>>()?;
        if ids.windows(2).any(|p| pair(p[0], p[1]).is_none()) {
            return None;
        }
        return merged_phrase(words, &HashSet::new(), table, &mut pair, known, lexicon);
    }
    // 動的計画法: beams[i] = 頭から i バイトまでの切り方の上位 2 つ (点, 語の終わりの位置, 語の番号, 表に無い並びの数)
    #[derive(Clone)]
    struct Path { cost: i64, ends: Vec<usize>, ids: Vec<usize>, unseen: u32 }
    let n = run.len();
    let mut beams: Vec<Vec<Path>> = vec![vec![]; n + 1];
    beams[0].push(Path { cost: 0, ends: vec![], ids: vec![], unseen: 0 });
    for i in 0..n {
        if beams[i].is_empty() {
            continue;
        }
        let from = beams[i].clone();
        for j in i + 1..=(i + 20).min(n) {
            let w = &run[i..j];
            if w.len() < 2 && w != "a" && w != "i" {
                continue;
            }
            let Some(id) = table.find(w) else { continue };
            for p in &from {
                let (step, unseen) = match p.ids.last() {
                    None => (table.uni(id), 0),
                    Some(&prev) => match pair(prev, id) {
                        Some(score) => (score, 0),
                        None => (table.uni(id) + table.mult(prev), 1),
                    },
                };
                let mut next = p.clone();
                next.cost += step as i64;
                next.ends.push(j);
                next.ids.push(id);
                next.unseen += unseen;
                beams[j].push(next);
            }
            beams[j].sort_by_key(|p| p.cost);
            beams[j].truncate(2);
        }
    }
    let best = beams[n].first()?;
    if best.unseen > 0 || beams[n].get(1).is_some_and(|second| second.cost - best.cost < PHRASE_MARGIN) {
        return None;
    }
    if let Some(whole) = table.find(run).filter(|_| n >= 2) {
        if table.uni(whole) as i64 - best.cost < PHRASE_MARGIN {
            return None;
        }
    }
    // 今の区切りで 7 字以上の表の英単語になっている語の中の位置
    let mut forbidden = HashSet::new();
    let mut at = 0;
    for t in toks {
        if t.kind == TokenKind::English && t.raw.len() >= PHRASE_KEEP_LETTERS && known(&t.raw.to_ascii_lowercase()) {
            forbidden.extend(at + 1..at + t.raw.len());
        }
        at += t.raw.len();
    }
    let mut start = 0;
    let words = best.ends.iter().map(|&end| { let w = run[start..end].to_string(); start = end; w }).collect();
    merged_phrase(words, &forbidden, table, &mut pair, known, lexicon)
}

/// 隣り合う 2 語をまとめる所 (割らない語) をまとめ、2 語以上残って、めったに出ない語を含まなければその語
fn merged_phrase(mut words: Vec<String>, forbidden: &HashSet<usize>, table: &dyn PhraseTable, pair: &mut dyn FnMut(usize, usize) -> Option<u32>,
                 known: &dyn Fn(&str) -> bool, lexicon: &dyn Fn(&str) -> bool) -> Option<Vec<String>> {
    let mut changed = true;
    while changed && words.len() >= 2 {
        changed = false;
        let mut at = 0;
        for i in 0..words.len() - 1 {
            at += words[i].len();
            let joined = format!("{}{}", words[i], words[i + 1]);
            let one = table.find(&joined).map(|id| table.uni(id) as i64);
            let two = match (table.find(&words[i]), table.find(&words[i + 1])) {
                (Some(a), Some(b)) => Some(table.uni(a) as i64 + pair(a, b).unwrap_or_else(|| table.uni(b) + table.mult(a)) as i64),
                _ => None,
            };
            if forbidden.contains(&at) || (joined.len() >= PHRASE_KEEP_LETTERS && lexicon(&joined))
                || matches!((one, two), (Some(one), Some(two)) if one <= two) {
                words[i] = joined;
                words.remove(i + 1);
                changed = true;
                break;
            }
        }
    }
    if words.len() < 2 {
        return None;
    }
    let rare = |w: &String| !(known(w) && w.len() >= 4) && table.find(w).map_or(u32::MAX, |id| table.uni(id)) > PHRASE_RARE_WORD;
    if words.iter().any(rare) {
        return None;
    }
    Some(words)
}

/// 2026-10-30: 打っている途中は末尾の子音 1 字が打ちかけ (たべ + t) だった入力で、打ち終えたときの区切りが、その子音といっしょに
/// 前のかなを英字の語へ取り込んだら、打っている途中の区切りに戻す (末尾は読めない子音 1 字のまま。母音の抜けなら変換するときの直しが拾う)。
/// 例文の測りで、文の終わりの母音を落とした入力の 66% (244 件中 161 件) が、Space の区切り直しでこうなっていた。戻すのは次の 2 つ:
/// ① 打っている途中に表の英単語 (4 字以上) だった所から、後ろのかなまで 1 語に伸ばした (stock + kiretetay・pi + ckupsitekurerutotasukar)
/// ② 取り込んだ語の末尾の子音より前がローマ字として読め (bet・rut・kar)、語の切れ目 (並びの頭・英単語・助詞の直後) から始まらず、
///    4 字以上の表の語でもない (ta + [bet]・si + te + ku + re + [rut]・ta + su + [kar])。
/// 子音の重なる名前 (boxcryptor・traefik)・助詞の後ろの短い名前 (これは + nec)・today・my は今までどおり英字
fn keep_trailing_stray(raw: &str, finishing: Vec<Token>) -> Vec<Token> {
    let Some(last) = finishing.last().filter(|t| t.kind == TokenKind::English && t.raw.len() >= 2) else { return finishing };
    if !raw.ends_with(last.raw.as_str()) || !raw.is_ascii() {
        return finishing;
    }
    let typing = segment(raw);
    let stray = typing.last().is_some_and(|t| t.kind == TokenKind::Romaji && t.raw.len() == 1
        && t.raw.as_bytes()[0].is_ascii_lowercase() && !is_vowel(t.raw.as_bytes()[0]) && t.raw != "n");
    if !stray {
        return finishing;
    }
    // 打っている途中の区切りで、取り込んだ所 (末尾の子音より前) に、かなの音・表の英単語があったか
    let start = raw.len() - last.raw.len();
    let (mut at, mut absorbed_kana, mut known_english, mut merged) = (0, false, false, false);
    for token in &typing[..typing.len() - 1] {
        let end = at + token.raw.len();
        if end > start {
            match token.kind {
                // 社名の表だけの語 (hetz) と 3 字以下の語 (hat) は数えない (korehatraefik の [hat] のように、打っている途中の区切りが外れていることがある)
                TokenKind::English => known_english |= token.raw.len() >= 4 && is_lexicon_word(&token.raw.to_ascii_lowercase()),
                TokenKind::Romaji if token.raw.bytes().any(is_vowel) => {
                    absorbed_kana = true;
                    merged |= known_english;
                }
                TokenKind::Romaji => {}
            }
        }
        at = end;
    }
    if !absorbed_kana {
        return finishing;
    }
    let word = last.raw.to_ascii_lowercase();
    let left_boundary = finishing.len() == 1 || finishing.get(finishing.len().wrapping_sub(2)).is_some_and(|before| {
        before.kind == TokenKind::English || SHORT_TERM_BOUNDARY.contains(&before.raw.to_ascii_lowercase().as_str())
    });
    let known = is_word(&word) || words().context.contains(&word);
    if known && (word.len() >= 4 || left_boundary) {
        return finishing;
    }
    if merged {
        return typing;
    }
    if !native_romaji_readable(&word[..word.len() - 1], false) || left_boundary {
        return finishing;
    }
    typing
}

thread_local! {
    // Bounded context evidence, not a probability. Explicit choices outweigh automatic detection.
    static SENTENCE: std::cell::Cell<i8> = const { std::cell::Cell::new(0) };
}

pub fn set_english_sentence(on: bool) {
    SENTENCE.set(if on { 2 } else { 0 });
}

pub fn in_english_sentence() -> bool {
    SENTENCE.get() >= 2
}

pub fn note_sentence_commit(latin: bool, explicit: bool, spaced: bool) {
    let previous = SENTENCE.get();
    let next = if !latin {
        -4
    } else if explicit {
        (previous.max(0) + 4).min(6)
    } else if spaced {
        (previous.max(0) + 2).min(6)
    } else if previous >= 2 {
        (previous + 1).min(6)
    } else {
        0
    };
    SENTENCE.set(next);
}

fn sentence_lexeme_score(word: &str, pending: bool) -> i8 {
    let lower = word.to_ascii_lowercase();
    let known = |w: &str| !learned_kana(w)
        && (is_word(w) || words().context.contains(w) || TWO_LETTER_WORDS.contains(&w) || matches!(w, "i" | "a"));
    if learned_kana(&lower) { return -8; }
    if known(&lower) {
        return if native_romaji_readable(&lower, false) { 1 } else { 2 };
    }
    if let Some((stem, suffix)) = lower.split_once('\'') {
        let base = if suffix == "t" { stem.strip_suffix('n').unwrap_or(stem) } else { stem };
        let suffixes = ["m", "s", "re", "ve", "ll", "d", "t"];
        if (known(stem) || known(base)) && (suffixes.contains(&suffix)
            || (pending && suffixes.iter().any(|s| s.starts_with(suffix)))) { return 2; }
    }
    if lower.contains('-') && lower.split('-').all(|part| known(part)) { return 2; }
    // A Japanese long-vowel mark is not evidence for an unknown English compound.
    if lower.contains('-') && lower.split('-').filter(|part| !part.is_empty())
        .all(|part| sentence_romaji_readable(part, pending)) { return -8; }
    if pending && (is_word_prefix(&lower) || context_word_prefix(&lower)) { return 0; }
    if !sentence_romaji_readable(&lower, true) {
        let tokens = segment_plain(&lower, style());
        let mut roman = String::new();
        let japanese = |run: &str| run.len() >= 4 && sentence_romaji_readable(run, false);
        for token in tokens {
            if token.kind == TokenKind::Romaji {
                roman += &token.raw;
            } else {
                if japanese(&roman) { return -8; }
                roman.clear();
            }
        }
        // Strong English context may retain an unlisted word, but not a readable Japanese run.
        if !japanese(&roman) && !japanese_particle_suffix(&roman) { return -1; }
    }
    -8
}

fn sentence_romaji_readable(word: &str, pending: bool) -> bool {
    if native_romaji_readable(word, pending) { return true; }
    let units = super::romaji_style::simulate(word);
    let mut suffix = String::new();
    for unit in units {
        if unit.kana.chars().all(|c| ('ぁ'..='ゖ').contains(&c)) && !unit.pending {
            if !suffix.is_empty() { return false; }
        } else if unit.pending && unit.kana.chars().all(|c| c.is_ascii_lowercase()) {
            suffix += &unit.kana;
        } else { return false; }
    }
    suffix.is_empty() || suffix == "n" || (pending
        && super::romaji_table::ROMAJI_TABLE.iter().any(|(keys, _)| keys.starts_with(&suffix)))
}

fn sentence_word(raw: &str, _style: Style) -> bool {
    if !in_english_sentence() || raw.is_empty()
        || !raw.chars().all(|c| c.is_ascii() && (!c.is_ascii_control() || c == ' ')) { return false; }
    let mut score = 2;
    let mut has_content = false;
    let bytes = raw.as_bytes();
    let mut end = 0;
    while end < bytes.len() {
        if !bytes[end].is_ascii_alphabetic() { end += 1; continue; }
        let start = end;
        end += 1;
        while end < bytes.len() && (bytes[end].is_ascii_alphabetic() || matches!(bytes[end], b'\'' | b'-')) { end += 1; }
        let word = raw[start..end].trim_end_matches('\'');
        let pending = end == raw.len() && word.len() == end - start && !FINISHING.get();
        score = score.min(sentence_lexeme_score(word, pending));
        has_content = true;
    }
    (has_content || raw.chars().any(|c| c.is_ascii_digit() || c.is_ascii_punctuation()))
        && SENTENCE.get() >= 2
        // Every lexical chunk must fit; punctuation never rescues a rejected Japanese word.
        && score + SENTENCE.get() >= 3
}

fn segment_with(raw: &str, style: Style) -> Vec<Token> {
    if let Some((start, end)) = literal_notation_span(raw, style) {
        let mut tokens = segment_with(&raw[..start], style);
        tokens.push(Token { raw: raw[start..end].into(), kind: TokenKind::English });
        let suffix = &raw[end..];
        let previous = SENTENCE.get();
        if japanese_particle_suffix(suffix) { SENTENCE.set(0); }
        tokens.extend(segment_with(suffix, style));
        SENTENCE.set(previous);
        return tokens;
    }
    if let Some((start, end)) = chemical_formula_span(raw) {
        let previous = RUN_AT_END.replace(false);
        let mut tokens = segment_with(&raw[..start], style);
        RUN_AT_END.set(previous);
        // 数字は今までどおりローマ字の側 (変換サーバーが元素記号と数字を見て下付きにする)、英字は記号ごとの並び
        let mut at = start;
        for (k, c) in raw[start..end].char_indices() {
            let boundary = start + k;
            if c.is_ascii_digit() {
                if at < boundary { tokens.push(Token { raw: raw[at..boundary].into(), kind: TokenKind::English }); }
                tokens.push(Token { raw: c.to_string(), kind: TokenKind::Romaji });
                at = boundary + 1;
            }
        }
        if at < end { tokens.push(Token { raw: raw[at..end].into(), kind: TokenKind::English }); }
        tokens.extend(segment_with(&raw[end..], style));
        return tokens;
    }
    if let Some((start, end, function)) = math_function_span(raw, style) {
        // 関数名の手前は入力の終わりではない (kan + sinx の n を打ちかけにしない)
        let previous = RUN_AT_END.replace(false);
        let mut tokens = segment_with(&raw[..start], style);
        RUN_AT_END.set(previous);
        tokens.extend(function);
        tokens.extend(segment_with(&raw[end..], style));
        return tokens;
    }
    // 2026-11-03 (仕事 10): 数式 (2e+1/t・3x+1=0・1/2) は英字・半角の記号のまま。関数名 (上) のあとで見る (sinx の x を変数とみて sin を しn にしない)
    if let Some((start, end)) = math_expression_span(raw) {
        let previous = RUN_AT_END.replace(false);
        let mut tokens = segment_with(&raw[..start], style);
        RUN_AT_END.set(previous);
        tokens.push(Token { raw: raw[start..end].into(), kind: TokenKind::English });
        tokens.extend(segment_with(&raw[end..], style));
        return tokens;
    }
    // 2026-11-04 (仕事 19 の 3a): 子音の字＋語を小文字で打った所 (jri-gu = Jリーグ・bkyuu = B級・xsenn = X線) は、字だけを英字に。
    // 数式 (上) のあとで見る (数学か物理をよく打つ人の式の中のかけ算は割らない)。字の形 (J・x) は変換するときに表から (initial_word_case)
    if let Some(at) = initial_word_span(raw) {
        let previous = RUN_AT_END.replace(false);
        let mut tokens = segment_with(&raw[..at], style);
        RUN_AT_END.set(previous);
        tokens.push(Token { raw: raw[at..at + 1].into(), kind: TokenKind::English });
        tokens.extend(segment_with(&raw[at + 1..], style));
        return tokens;
    }
    if sentence_word(raw, style) {
        return vec![Token { raw: raw.to_string(), kind: TokenKind::English }];
    }
    // アクセント付きの文字を含む語 (España・café) はローマ字ではありえないので、語全体を英語にする
    if let Some(pos) = raw.find(is_accented_latin) {
        let is_word_char = |c: char| c.is_ascii_alphabetic() || is_accented_latin(c);
        let mut start = raw[..pos].rfind(|c: char| !is_word_char(c)).map_or(0, |i| i + raw[i..].chars().next().unwrap().len_utf8());
        let mut end = raw[pos..].find(|c: char| !is_word_char(c)).map_or(raw.len(), |i| pos + i);
        if !words().latin.is_empty() {
            let positions: Vec<usize> = raw[start..end].char_indices().map(|(i, _)| start + i).chain(std::iter::once(end)).collect();
            'word: for candidate in positions.iter().copied().filter(|&i| i <= pos && pos - i <= 60) {
                for word_end in positions.iter().rev().copied().filter(|&i| i > pos && i - candidate <= 60) {
                    if !words().latin.contains(&raw[candidate..word_end].to_lowercase()) { continue; }
                    let suffix = &raw[word_end..end];
                    if !suffix.is_empty() && !PARTICLES.iter().any(|particle| suffix.starts_with(particle)) { continue; }
                    let prefix = segment_plain(&raw[start..candidate], style);
                    if candidate == start || prefix.last().is_some_and(|t| t.kind == TokenKind::English || PARTICLES.contains(&t.raw.as_str())) {
                        start = candidate;
                        end = word_end;
                        break 'word;
                    }
                }
            }
        }
        let mut tokens = segment_with(&raw[..start], style);
        tokens.push(Token { raw: raw[start..end].to_string(), kind: TokenKind::English });
        tokens.extend(segment_with(&raw[end..], style));
        return tokens;
    }
    // 略語を先に切り出す (ピリオドを打った時点で英字にまとめる)
    ABBREVIATION_STYLE.set(Some(style));
    for (end, c) in raw.char_indices().rev() {
        if c != '.' {
            continue;
        }
        let end = end + 1;
        if let Some(start) = abbreviation_start(raw, end) {
            // 略語のすぐ後ろのピリオドも英字のまま (etc... = etc...、etc.。。 にしない)
            let end = end + raw[end..].bytes().take_while(|&b| b == b'.').count();
            let mut tokens = segment_plain(&raw[..start], style);
            tokens.push(Token { raw: raw[start..end].to_string(), kind: TokenKind::English });
            tokens.extend(segment_with(&raw[end..], style));
            return tokens;
        }
    }
    segment_plain(raw, style)
}

fn japanese_particle_suffix(raw: &str) -> bool {
    raw.len() >= 2 && sentence_romaji_readable(raw, false)
        && PARTICLES.iter().any(|particle| raw.starts_with(particle))
}

/// 2026-10-31: 1 字の元素記号 (ELEMENT_SYMBOLS は 2 字の記号だけ)
const SINGLE_ELEMENTS: &[&str] = &["H", "B", "C", "N", "O", "F", "P", "S", "K", "V", "Y", "I", "W", "U"];

thread_local! {
    /// 試験で chemical_formula_focus を切り替える (None なら設定を読む)
    static CHEM_FOCUS_FOR_TEST: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// 2026-10-31: 化学・生物・地学のどれかをよく打つ分野に選んだ人か (設定 input.domains_focus)。小文字で打った化学式を英字にする (chemical_formula_span)。
/// 試験のビルドでは AZK_CHEM_FORMULAS (0・1) で選ぶ
fn chemical_formula_focus() -> bool {
    if let Some(on) = CHEM_FOCUS_FOR_TEST.get() { return on; }
    static FOCUS: OnceLock<bool> = OnceLock::new();
    *FOCUS.get_or_init(|| match test_env("AZK_CHEM_FORMULAS") {
        Some(on) => on == "1",
        None => !cfg!(test) && shared::AppConfig::read_or_default().input.domains_focus.iter()
            .any(|d| matches!(d.as_str(), "chemistry" | "biology" | "earth")),
    })
}

/// 英小文字と数字の並びが、元素記号 (大文字小文字を問わない) と数の並びに分けられるか (co2 = C O 2・naoh = Na O H・h2so4)。数は記号のすぐ後ろだけ
fn formula_parts(lower: &str) -> bool {
    let b = lower.as_bytes();
    let mut ok = vec![false; b.len() + 1];
    ok[0] = true;
    for i in 0..b.len() {
        if !ok[i] { continue; }
        if b[i].is_ascii_digit() {
            if i > 0 { ok[i + 1] = true; }
            continue;
        }
        if SINGLE_ELEMENTS.iter().any(|e| e.eq_ignore_ascii_case(&lower[i..i + 1])) { ok[i + 1] = true; }
        if i + 2 <= b.len() && b[i + 1].is_ascii_lowercase() && ELEMENT_SYMBOLS.iter().any(|e| e.eq_ignore_ascii_case(&lower[i..i + 2])) { ok[i + 2] = true; }
    }
    ok[b.len()]
}

/// 2026-10-31: 化学・生物・地学を選んだ人の、小文字で打った化学式 (co2・h2o・h2so4・caco3・kmno4)。英字にする所を返す。
/// 前は c|o|2 = こ2、h|2|o = h2お になっていた。化学式とみるのは、元素記号と数の並びに分けられて、数を含み、頭の固まりがローマ字として読めない
/// (co2・h2o・h2so4・ch4・caco3・kmno4。c 行の音だけの固まりは日本語の綴りとみない) もの。数を含まない並び (naoh・nacl・hcl) は、略語 (gps = Ga P S・
/// cpu・usb・os) と見分けられないので当てない。頭が読める式 (fe2o3・na2co3・so2) も、日本語の語の途中 (yaku80km の ku80k) と見分けられないので当てない
/// もの。読める固まり 1 つと数 (no2 = の2・o2・so2・ba2) は日本語と見分けられないので当てない。英字の固まりは 5 字まで。
/// 手前は入力の頭か、助詞で終わる読める並び (taikichuuno + co2。語の途中の denkibunkai の nkib・taikichuu の ikic は当てない)。
/// 後ろはローマ字として読める並び (co2noudo・naohwo) か英字でない字で、子音で終わって後ろが母音か続く音節がある (unkai の unk・c + hu) なら
/// その字は次の音の頭。終わりの固まりが助詞 (hcl + ga) か、数のあとの 3 字以上の読める固まり (co2 + nou) なら入れない。
/// 当たる並びのうち長いもの (h2o・h2so4) を選ぶ
fn chemical_formula_span(raw: &str) -> Option<(usize, usize)> {
    if !chemical_formula_focus() || !raw.is_ascii() { return None; }
    let lower = raw.to_ascii_lowercase();
    let bytes = raw.as_bytes();
    // c 行の音だけでできた固まり (co・caco) は日本語の綴りとみない。c 行とほかの音が交ざる固まり (cono = この) は日本語
    let c_row_only = |g: &str| g.len() % 2 == 0 && g.as_bytes().chunks(2).all(|p| p[0] == b'c' && is_vowel(p[1]));
    let japanese_like = |letters: &str| native_romaji_readable(letters, false) && !c_row_only(letters);
    let alnum_end = |s: usize| s + bytes[s..].iter().take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit()).count();
    for s in 0..raw.len() {
        if !bytes[s].is_ascii_lowercase() || (s > 0 && bytes[s - 1].is_ascii_digit()) { continue; }
        let run_start = raw[..s].rfind(|c: char| !c.is_ascii_alphabetic()).map_or(0, |i| i + 1);
        // 手前は入力の頭 (英字でない字のあと) か、ローマ字として読み切れて助詞で終わる並び (taikichuuno + co2・sono + co2)
        let prefix = &lower[run_start..s];
        let after_particle = native_romaji_readable(prefix, false) && PAIR_PARTICLES.iter().any(|p| prefix.ends_with(p));
        if !prefix.is_empty() && !after_particle { continue; }
        let limit = alnum_end(s).min(s + 20);
        let valid = |e: usize| {
            let span = &lower[s..e];
            if !formula_parts(span) { return false; }
            let groups: Vec<&str> = span.split(|c: char| c.is_ascii_digit()).filter(|g| !g.is_empty()).collect();
            if groups.iter().any(|g| g.len() > 5) { return false; }
            // 数を含まない並び (naoh・hcl) は当てない: 測った文では略語 (gps = Ga P S・cpu・usb) ばかりで、それを崩した
            // 頭の固まりはローマ字として読めないもの (co・h・caco・kmno)。読める頭 (ku80k・ra11m・ikibo4k・fe2o3) は日本語の語の途中と見分けられない。
            // 数は 2 けたまで (80km・200m は数と単位)。数の前の固まりが助詞で終わる (phga7 = pH が 7) なら入れない (の は kmno4 のため除く)
            if !span.bytes().any(|b| b.is_ascii_digit()) || japanese_like(groups[0]) { return false; }
            if span.split(|c: char| !c.is_ascii_digit()).any(|d| d.len() > 2) { return false; }
            let mut pieces = span.split_inclusive(|c: char| c.is_ascii_digit()).filter(|p| p.len() > 1);
            if pieces.any(|p| ["ga", "wo", "ha", "de", "to", "mo", "ni", "he", "ya", "ka"].iter().any(|q| p.trim_end_matches(|c: char| c.is_ascii_digit()).ends_with(q))) { return false; }
            let last = span.as_bytes()[span.len() - 1];
            // 終わりの英字の固まりが助詞 (co2 + no・hcl + ga) か、数のあとの 2 字以上の読める固まり (co2 + nou・h2 + owo・sp2 + ko) なら入れない
            let tail = *groups.last().unwrap();
            if last.is_ascii_lowercase() && (PAIR_PARTICLES.iter().any(|p| tail.ends_with(p))
                || (!span.starts_with(tail) && tail.len() >= 2 && native_romaji_readable(tail, false))) { return false; }
            // 数の途中では切らない (heikacu180 の cu1)
            if e < raw.len() && bytes[e].is_ascii_digit() && last.is_ascii_digit() { return false; }
            // 後ろ: 入力の終わり・英字でない字・ローマ字として読める並び。子音で終わって後ろが母音か、終わりの字から音節が続く (c + hu = ちゅ) なら、
            // その字は次の音の頭
            if e == raw.len() || !bytes[e].is_ascii_alphabetic() { return true; }
            if last.is_ascii_lowercase() && !is_vowel(last) && (is_vowel(lower.as_bytes()[e])
                || SYLLABLES.iter().any(|syl| syl.len() >= 2 && lower[e - 1..].starts_with(syl))) { return false; }
            // 後ろはローマ字の音で始まればよい (h2so4 + to + naoh の naoh は読めなくても、式の切れ目は to の手前)
            let alpha = e + bytes[e..].iter().take_while(|b| b.is_ascii_alphabetic()).count();
            SYLLABLES.iter().any(|syl| lower[e..alpha].starts_with(syl)) || (!FINISHING.get() && alpha == raw.len() && is_syllable_prefix(&lower[e..alpha]))
        };
        if let Some(e) = ((s + 1)..=limit).rev().find(|&e| valid(e)) { return Some((s, e)); }
    }
    None
}

/// 2026-10-31: 数学の関数名。LaTeX の演算子の名前 (TeX Live の latex.ltx と amsmath の amsopn.sty が定義する \sin・\log・\lim など。
/// mod は \bmod・\pmod)。大文字で始まる \Pr と、2 語の \injlim・\projlim は除く。長い名前を先に試す (sinh を sin より先)
const MATH_FUNCTIONS: &[&str] = &[
    "arccos", "arcsin", "arctan", "liminf", "limsup", "cosh", "coth", "sinh", "tanh",
    "arg", "cos", "cot", "csc", "deg", "det", "dim", "exp", "gcd", "hom", "inf", "ker", "lim", "log", "max", "min", "mod",
    "sec", "sin", "sup", "tan", "lg", "ln",
];

/// 2026-10-31: 関数名 + 助詞 のあとに来る数学の語の読み (standalone_math_function)。数学の目印の語 (分野辞書の domain_math_signals.txt) と、
/// 関数名といっしょに使う語 (グラフ・値・公式・周期・定義・合成・倍角 など) の読み。読みは Mozc の辞書 (dictionary0*.txt) で確かめた。
/// 熟語は頭の語で当てる (かほう = 加法・加法定理、せきぶん = 積分・積分する)
const MATH_WORD_READINGS: &[&str] = &[
    "かんすう", "ほうていしき", "ふとうしき", "びぶん", "せきぶん", "どうかんすう", "ていせきぶん", "ふていせきぶん", "すうれつ", "ぜんかしき",
    "さんかくかんすう", "しすうかんすう", "たいすうかんすう", "ぎゃくかんすう", "ぐらふ", "あたい", "こうしき", "しゅうき", "ていぎ", "ごうせい",
    "ばいかく", "はんかく", "にじょう", "さんじょう", "さいだいち", "さいしょうち", "きょくだいち", "きょくしょうち", "きょくげん", "ぞうげん",
    "ていぎいき", "ちいき", "せっせん", "ぜんきんせん", "かほう", "ていり", "せいげんていり", "よげんていり", "こどほう", "しんぷく", "いそう",
    "ぎゃくすう", "ふごう", "はんい", "けいさん", "きんじ", "てんかい", "せいしつ",
];

/// 2026-10-31: 数学を選んだ人の、単独で打った関数名 (sin と cos・tan の加法定理・sin の値)。関数名のすぐ後ろが助詞 (の・を・が・は・と・で・や・も・へ) で、
/// - 助詞が と・や で、その後ろも関数名 (sintocos・sinyacostotan)
/// - 助詞の後ろが数学の語 (tannokahouteiri = tan の加法定理・sinnoatai = sin の値)
/// のとき。どちらも関数名の手前が入力の頭・英字でない字・助詞・数学の語 (kansuusinnoatai = 関数 sin の値) のときだけ
/// (tyuusinnoteigi = 中心の定義・botanto = ボタンと の しん・たん は当てない)
/// 助詞のすぐ後ろが u (sintou = 浸透・tantou = 担当) は、のばす音の語の途中とみて除く。ん を nn と打つ人でも、ときどき n 1 つで打つ
/// (中心と・神殿) ので、打ち方で決めつけない。英字にする所 (関数名だけ。と・や でつないだ関数名は、つないだ所までまとめて区切る) を返す
fn standalone_math_function(raw: &str, lower: &str, s: usize, e: usize) -> Option<(usize, Vec<Token>)> {
    const PARTICLES: &[&str] = &["no", "wo", "ga", "ha", "to", "de", "ya", "mo", "he"];
    let english = |a: usize, b: usize| Token { raw: raw[a..b].into(), kind: TokenKind::English };
    let particle = PARTICLES.iter().find(|p| lower[e..].starts_with(**p))?;
    let q = e + particle.len();
    // 助詞のすぐ後ろの u は、のばす音の語の途中 (sintou = 浸透・tantou = 担当・sinnou = 震央)
    if lower.as_bytes().get(q) == Some(&b'u') { return None; }
    // 関数名の手前: 入力の頭・英字でない字・助詞・数学の語 (botanto の tan = ボタン、tyuusinno の sin = 中心 は当てない)
    let kana = |text: &str| -> String { super::romaji_style::simulate(text).iter().filter(|u| !u.pending).map(|u| u.kana.as_str()).collect() };
    let run_start = lower[..s].rfind(|c: char| !c.is_ascii_alphabetic()).map_or(0, |i| i + lower[i..].chars().next().unwrap().len_utf8());
    let before = kana(&lower[run_start..s]);
    let left_fits = before.is_empty() || ["の", "を", "が", "は", "と", "で", "や", "も", "に", "へ"].iter().any(|p| before.ends_with(p))
        || MATH_WORD_READINGS.iter().any(|w| before.ends_with(w));
    if !left_fits { return None; }
    // と・や でつながる関数名 (後ろが入力の終わり・英字でない字・助詞・次の と・や)
    let mut tokens = vec![english(s, e)];
    let (mut at, mut chained) = (e, false);
    while let Some(link) = ["to", "ya"].iter().find(|p| lower[at..].starts_with(**p)) {
        let g = at + link.len();
        // 打っている途中の末尾の関数名 (botantodet の det = 出てくる の打ちかけ) は決めない
        let Some(next) = MATH_FUNCTIONS.iter().filter(|f| lower[g..].starts_with(**f)).find(|f| {
            let h = g + f.len();
            (h == raw.len() && FINISHING.get()) || (h < raw.len() && (!raw.as_bytes()[h].is_ascii_alphabetic() || PAIR_PARTICLES.iter().any(|p| lower[h..].starts_with(p))))
        }) else { break };
        tokens.push(Token { raw: raw[at..g].into(), kind: TokenKind::Romaji });
        tokens.push(english(g, g + next.len()));
        at = g + next.len();
        chained = true;
    }
    if chained { return Some((at, tokens)); }
    let after = kana(&lower[q..]);
    MATH_WORD_READINGS.iter().any(|w| after.starts_with(w)).then_some((e, tokens))
}

/// 2026-10-31: 関数名のすぐ後ろに打つギリシャ文字の読み (sinしーた = sinθ、cosあるふぁ)。ぱい (π) は しんぱい (心配) とぶつかるので入れない
const GREEK_READINGS: &[&str] = &["si-ta", "shi-ta", "arufa", "be-ta", "ganma", "deruta", "ramuda", "omega", "fai"];

thread_local! {
    /// 試験で math_function_level を切り替える (None なら設定を読む)
    static MATH_LEVEL_FOR_TEST: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

/// 2026-10-31: 数学の関数名を英字にする度合い (math_function_span)。数学か物理 (2026-11-01) をよく打つ分野に選んだ人 (設定 input.domains_focus) は 2、
/// ほかは 1 (^・(・数字 + 変数 の形だけ。日本語の組で区切りが 1 行も変わらない形なので、分野を選ばずに教科書の式を打つ人にも効かせる)。
/// 試験のビルドでは AZK_MATH_FUNCTIONS (0・1・2) で選ぶ
fn math_function_level() -> u8 {
    if let Some(level) = MATH_LEVEL_FOR_TEST.get() { return level; }
    static LEVEL: OnceLock<u8> = OnceLock::new();
    *LEVEL.get_or_init(|| match test_env("AZK_MATH_FUNCTIONS") {
        Some(level) => level.parse().unwrap_or(0),
        None if cfg!(test) => 1,
        // 2026-11-05 (仕事 28 の 6): 数学の式の直し (input.math_expressions)。書いていなければ、よく打つ分野に数学か物理があればオン (前と同じ)
        None => if shared::AppConfig::read_or_default().input.math_expressions_on() { 2 } else { 1 },
    })
}

/// 2026-11-01: よく打つ分野が数学か物理 (sinωt・cosθ の式が多い) なら 2、ほかは 1
fn math_function_level_for(domains_focus: &[String]) -> u8 {
    if domains_focus.iter().any(|d| d == "math" || d == "physics") { 2 } else { 1 }
}

/// 2026-10-31: 引数の形が続く数学の関数名 (sin2x・cos^2x・tan(x)・sinxno・detA)。英字にする所の (始まり, 終わり, 区切り) を返す。
/// - 1: 関数名のすぐ後ろが ^・(・{・_・ギリシャ文字 (θ)、または数字 + (^・(・ギリシャ文字・変数) のとき。数字 + 変数 は 関数名|数字|変数 と区切る
/// - 2: さらに、すぐ後ろが変数のとき (関数名と変数で 1 語)・ギリシャ文字の読み (sinsi-ta) のとき・数字 + ど (sin30do = sin30度) のとき
///
/// 変数は、大文字の並び・ギリシャ文字 1 字か、母音でない小文字 1 字で、その字から変換のローマ字の綴りが始まらず、後ろが英字でないか綴りの頭のもの。
/// ん + 子音 の日本語 (sinkansen の k・tanchou の c・sintsuu の t) は、その字から綴りが始まるので当たらない。n で終わる名前 (sin・tan・min) の
/// 後ろの n は変数にしない (sinn = しん)。打っている途中の末尾の小文字 (sink = しんか の打ちかけ) は、x のほかは決めない。
/// 打っている途中は、変数 x の後ろが綴りの打ちかけ (sinxn の n) でもよい (ほかの字は決めない: tansh = たんしゅ の打ちかけ)。
/// 関数名を含む英単語 (tanks・becoming・catalogs)、手前がローマ字として読み切れない並び (shin の hin)・n 1 字 (nmodp) は対象外
fn math_function_span(raw: &str, style: Style) -> Option<(usize, usize, Vec<Token>)> {
    let level = math_function_level();
    if level == 0 || !raw.bytes().any(|b| b.is_ascii_alphabetic()) { return None; }
    let lower = raw.to_ascii_lowercase();
    let bytes = raw.as_bytes();
    let greek = |c: char| !c.is_ascii() && c.is_alphabetic() && !japanese_char(c);
    // この位置から変換のローマ字表の綴り (か・っ・ゃ・→) が始まるか。同じ子音の重ねは促音
    let spelling_at = |p: usize| {
        let rest = &lower[p..];
        table_romaji_keys().0.iter().any(|keys| rest.starts_with(keys))
            || (rest.len() >= 2 && rest.as_bytes()[0] == rest.as_bytes()[1] && !is_vowel(rest.as_bytes()[0]))
    };
    let variable_end = |p: usize, after_n: bool| -> Option<usize> {
        let c = raw[p..].chars().next()?;
        if c.is_ascii_uppercase() { return Some(p + raw[p..].bytes().take_while(u8::is_ascii_uppercase).count()); }
        if greek(c) { return Some(p + c.len_utf8()); }
        if !c.is_ascii_lowercase() || is_vowel(c as u8) || (after_n && c == 'n') || spelling_at(p) { return None; }
        let q = p + 1;
        if q == raw.len() { return (FINISHING.get() || c == 'x').then_some(q); }
        (!bytes[q].is_ascii_alphabetic() || spelling_at(q) || (!FINISHING.get() && c == 'x' && table_romaji_keys().1.contains(&lower[q..]))).then_some(q)
    };
    let english = |a: usize, b: usize| Token { raw: raw[a..b].into(), kind: TokenKind::English };
    // かなの語 (度・しーた) が入力の終わり・英字でない字・助詞の前で終わるか
    let kana_word_at = |p: usize, words: &[&str]| words.iter().any(|w| {
        lower[p..].starts_with(w) && (p + w.len() == raw.len() || !bytes[p + w.len()].is_ascii_alphabetic()
            || PAIR_PARTICLES.iter().any(|particle| lower[p + w.len()..].starts_with(particle)))
    });
    for s in 0..raw.len() {
        if !raw.is_char_boundary(s) || !bytes[s].is_ascii_lowercase() { continue; }
        let run_start = raw[..s].rfind(|c: char| !c.is_ascii_alphabetic()).map_or(0, |i| i + raw[i..].chars().next().unwrap().len_utf8());
        let prefix = &lower[run_start..s];
        let lone_n = prefix.ends_with('n') && !prefix[..prefix.len() - 1].ends_with(|c: char| is_vowel(c as u8) || c == 'n');
        if !prefix.is_empty() && (lone_n || !native_romaji_readable(prefix, false)) { continue; }
        let run_end = s + raw[s..].bytes().take_while(u8::is_ascii_alphabetic).count();
        for name in MATH_FUNCTIONS.iter().filter(|name| raw[s..].starts_with(**name)) {
            let e = s + name.len();
            let found = match raw[e..].chars().next() {
                Some('^' | '(' | '{' | '_') => Some((e, vec![english(s, e)])),
                // 2026-11-01: ^ で上付きにした字 (sin²x・cos⁻¹x)。打った列に上付きの字がそのまま入る
                Some(c) if super::scripts::superscript_char(c) => Some((e, vec![english(s, e)])),
                Some(c) if greek(c) => Some((e, vec![english(s, e)])),
                Some(c) if c.is_ascii_digit() => {
                    let d = e + raw[e..].bytes().take_while(u8::is_ascii_digit).count();
                    if matches!(raw[d..].chars().next(), Some('^' | '(')) || (level >= 2 && kana_word_at(d, &["do"])) {
                        Some((e, vec![english(s, e)]))
                    } else {
                        variable_end(d, false).map(|v| {
                            let mut tokens = vec![english(s, e)];
                            tokens.extend(raw[e..d].chars().map(|c| Token { raw: c.to_string(), kind: TokenKind::Romaji }));
                            tokens.push(english(d, v));
                            (v, tokens)
                        })
                    }
                }
                Some(_) if level >= 2 && kana_word_at(e, GREEK_READINGS) => Some((e, vec![english(s, e)])),
                Some(_) if level >= 2 => variable_end(e, name.ends_with('n')).map(|v| (v, vec![english(s, v)]))
                    .or_else(|| standalone_math_function(raw, &lower, s, e)),
                _ => None,
            };
            let Some((end, tokens)) = found else { continue };
            // 今の区切りで、関数名を含んで前後に延びる表の英単語になっている所 (tanks・becoming・causing・catalogs・cosplay) は今までどおり
            let alpha_end = end.min(run_end);
            let mut at = run_start;
            let inside_word = segment_run(&raw[run_start..run_end], style).iter().any(|t| {
                let (a, b) = (at, at + t.raw.len());
                at = b;
                let w = &lower[a..b];
                t.kind == TokenKind::English && a < alpha_end && s < b && (a < s || b > alpha_end) && (is_word(w) || words().context.contains(w))
            });
            if inside_word { continue; }
            return Some((s, end, tokens));
        }
    }
    None
}

/// 2026-11-03 (仕事 10): 数学か物理をよく打つ分野に選んだ人 (math_function_level が 2) の、数式の並びの (始まり, 終わり)。
/// 数式は、数 (0-9。数字の間の . も)・1 字の変数・括弧を、演算子 (+ - * / = ^ < >) でつないだもので、演算子の両側に数か変数があるもの
/// (2e+1/t・x²+1/x・1/2・3x+1=0。演算子の無い 2en = 2円 は数式にしない)。数のすぐ後ろの変数はかけ算 (2e・3x)。上付き・下付きの字 (² ₙ) は前の数・変数の続き。
/// 式の中は英字・半角の記号のまま入れる (/ を ・、- を ー にしない)。式のあとのローマ字は今までどおり かな。
/// 変数の英字は、すぐ後ろが英字でないか、その字から変換のローマ字の綴りが始まらず、後ろの字から綴りが始まるもの (tkana の t。打っている途中は綴りの頭でもよい)。
/// x は、小さいかなの綴り (xa・xtu) の頭のときだけ綴りとみる (xnituite の x は変数)。母音の字は後ろが英字なら変数にしない (2en の e)。
/// 大文字は、すぐ後ろが小文字でなければ変数 (Ax+B)。頭の変数は、手前の英字がローマ字として読み切れ、その字まで入れると読めないときだけ
/// (kotaehax+1 の x。ka+1 の a は か の一部、nedann+500 の n は ん の一部)
/// 2026-11-04 (仕事 10b): 次の 3 つを足した。
/// - すぐ前が母音で 1 字で音になる母音 (kotaehaa=3 の 2 つめの a) が = + * ^ < > の前に来た所は、前が日本語でも変数 (かれ=3 の e のように子音と組む母音はかなのまま)
/// - 式の中と数のすぐ後ろの、ローマ字として読めない 2〜3 字の小文字の並び (ax・bx・abc) は変数のかけ算。後ろに続く字がローマ字として読み切れるならそこまで (axnoshiki の ax)
/// - 入力の終わりが、変数・数のすぐ後ろの = + * < > の所 (a=・2x+) は、右の数・変数を打つ前でも式 (- と / は ー と ・、^ は顔文字の ^^ のことが多いので含めない)
fn math_expression_span(raw: &str) -> Option<(usize, usize)> {
    if math_function_level() < 2 || !raw.bytes().any(|b| b"+-*/=^<>".contains(&b)) { return None; }
    let chars: Vec<(usize, char)> = raw.char_indices().collect();
    let n = chars.len();
    let lower = raw.to_ascii_lowercase();
    let at_end = |i: usize| i >= n;
    let byte = |i: usize| if i < n { chars[i].0 } else { raw.len() };
    let script_mark = |c: char| super::scripts::superscript_char(c) || "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓ".contains(c);
    let greek = |c: char| !c.is_ascii() && c.is_alphabetic() && !japanese_char(c) && !script_mark(c);
    // この字の位置から、変換のローマ字の綴りが始まるか (同じ子音の重ねは促音)。x は小さいかなの綴りだけ
    let spelling_at = |i: usize| -> bool {
        if at_end(i) || !chars[i].1.is_ascii_lowercase() { return false; }
        let rest = &lower[byte(i)..];
        if chars[i].1 == 'x' { return SMALL_KANA.iter().any(|k| k.starts_with('x') && rest.starts_with(k)); }
        table_romaji_keys().0.iter().any(|k| rest.starts_with(k))
            || (rest.len() >= 2 && rest.as_bytes()[0] == rest.as_bytes()[1] && !is_vowel(rest.as_bytes()[0]))
            || (RUN_AT_END.get() && !FINISHING.get() && table_romaji_keys().1.contains(rest))
    };
    // i の字が変数なら、その後ろ (上付き・下付きの字も含めて)
    let variable = |i: usize| -> Option<usize> {
        let c = chars.get(i)?.1;
        let next = chars.get(i + 1).map(|x| x.1);
        let ok = if greek(c) {
            true
        } else if c.is_ascii_uppercase() {
            !next.is_some_and(|d| d.is_ascii_lowercase())
        } else if c.is_ascii_lowercase() {
            match next {
                Some(d) if d.is_ascii_alphabetic() => !is_vowel(c as u8) && !spelling_at(i) && spelling_at(i + 1),
                _ => true,
            }
        } else {
            false
        };
        ok.then(|| (i + 1..=n).find(|&k| k == n || !script_mark(chars[k].1)).unwrap_or(n))
    };
    let number = |i: usize| -> Option<usize> {
        if !chars.get(i)?.1.is_ascii_digit() { return None; }
        let mut k = i;
        while k < n && (chars[k].1.is_ascii_digit() || (chars[k].1 == '.' && k + 1 < n && chars[k + 1].1.is_ascii_digit() && k > i)) { k += 1; }
        while k < n && script_mark(chars[k].1) { k += 1; }
        Some(k)
    };
    let operator = |c: char| "+-*/=^<>".contains(c);
    // - と / を除いた演算子 (かなの ー・・ にならないもの)
    let math_operator = |c: char| "+*=^<>".contains(c);
    let letter_run_end = |i: usize| (i..n).find(|&k| !chars[k].1.is_ascii_lowercase()).unwrap_or(n);
    // i から始まる、ローマ字として読めない 2〜3 字の小文字の並び (ax・bx・abc) を変数のかけ算とみたときの、その後ろ (上付き・下付きの字も含めて)。
    // 並びの全体が読める (oosaka・akaruku) ならかけ算にしない。後ろに字が続くなら、続く字がローマ字の綴りで始まって読み切れるとき (axnoshiki の ax)。
    // 綴りの頭の字 (xtu の x・mod の mo) からは始めない (母音は、次の字が綴りの頭でなければよい: ax)
    let product = |i: usize| -> Option<usize> {
        let j = letter_run_end(i);
        let pending = j == n && RUN_AT_END.get() && !FINISHING.get();
        if j - i < 2 || native_romaji_readable(&lower[byte(i)..byte(j)], pending) { return None; }
        if spelling_at(i) && !(is_vowel(chars[i].1 as u8) && !spelling_at(i + 1)) { return None; }
        let k = (2..=3).map(|len| i + len).filter(|&k| k <= j).find(|&k| !native_romaji_readable(&lower[byte(i)..byte(k)], false)
            && (k == j || (spelling_at(k) && native_romaji_readable(&lower[byte(k)..byte(j)], pending))))?;
        Some((k..=n).find(|&m| m == n || !script_mark(chars[m].1)).unwrap_or(n))
    };
    // 頭の変数の手前の英字がローマ字として読み切れ、その字までは読めないか (ねだんn+500 の n・きょうu+1 の u は ローマ字の続き)
    let start_ok = |i: usize| -> bool {
        if i == 0 || !chars[i - 1].1.is_ascii_alphabetic() { return true; }
        if chars[i].1.is_ascii_digit() || chars[i].1 == '(' { return true; }
        // すぐ前が母音で、1 字で音になる母音 (kotaehaa=3・kotaehaax+b の a) が演算子 (- / を除く) の前に来た変数・かけ算は、前が日本語でも
        let vowel = |c: char| c.is_ascii_lowercase() && is_vowel(c as u8);
        if vowel(chars[i].1) && vowel(chars[i - 1].1)
            && variable(i).or_else(|| product(i)).is_some_and(|k| chars.get(k).is_some_and(|c| math_operator(c.1))) { return true; }
        let run_start = (0..i).rev().take_while(|&k| chars[k].1.is_ascii_alphabetic()).last().unwrap_or(i);
        native_romaji_readable(&lower[byte(run_start)..byte(i)], false) && !native_romaji_readable(&lower[byte(run_start)..byte(i + 1)], false)
    };
    for s in 0..n {
        let c = chars[s].1;
        // 頭のかけ算 (ax+b) は、手前が英字でないか、綴りの頭でない字 (bx の b) か、すぐ前も母音の母音 (kotaehaax の 2 つめの a) から
        // (kotaehax の hax は は + x。ha を巻き込まない)
        let product_start = || s == 0 || !chars[s - 1].1.is_ascii_alphabetic() || !spelling_at(s)
            || (is_vowel(c.to_ascii_lowercase() as u8) && is_vowel(chars[s - 1].1.to_ascii_lowercase() as u8));
        if !(c.is_ascii_digit() || c == '(' || variable(s).is_some() || (product(s).is_some() && product_start())) || !start_ok(s) { continue; }
        // 数・変数の前に数字・英字が続く所からは始めない (12+3 の 2 から、ab+c の b から始めない)
        if s > 0 && chars[s - 1].1.is_ascii_digit() { continue; }
        let (mut i, mut depth, mut operators, mut end, mut used) = (s, 0usize, 0usize, None, 0usize);
        let mut expect_operand = true;
        while i < n {
            let c = chars[i].1;
            if expect_operand {
                if c == '(' { depth += 1; i += 1; continue; }
                // 演算子のあとの符号 (x=-1・2*(-3))
                if (c == '-' || c == '+') && operators > 0 && i + 1 < n
                    && (chars[i + 1].1.is_ascii_digit() || chars[i + 1].1 == '(' || variable(i + 1).is_some()) { i += 1; continue; }
                // 変数のかけ算 (ax) は、式の頭か演算子のあと
                let Some(mut k) = number(i).or_else(|| variable(i)).or_else(|| if i == s || operators > 0 { product(i) } else { None }) else { break };
                // 数のすぐ後ろの変数はかけ算 (2e・3x・2ax)
                if chars[i].1.is_ascii_digit() {
                    while let Some(v) = variable(k) { k = v; }
                    if let Some(v) = product(k) { k = v; }
                }
                i = k;
                expect_operand = false;
                end = Some(i);
                used = operators;
            } else if c == ')' && depth > 0 {
                depth -= 1;
                i += 1;
                end = Some(i);
            } else if c == '(' {
                // 括弧のかけ算 ((x+1)(x-1)・2(x+1))
                depth += 1;
                i += 1;
                expect_operand = true;
            } else if operator(c) {
                operators += 1;
                i += 1;
                expect_operand = true;
            } else {
                break;
            }
        }
        // 入力の終わりが演算子 (- / ^ を除く) の所 (a=・2x+)。右の数・変数はまだ (sugoi^ は顔文字の打ちかけ)
        if end.is_some() && expect_operand && i == n && operators > used && RUN_AT_END.get() && "=+*<>".contains(chars[n - 1].1) {
            return Some((byte(s), raw.len()));
        }
        if let Some(e) = end.filter(|_| used >= 1) {
            return Some((byte(s), byte(e)));
        }
    }
    None
}

/// 2026-11-04 (仕事 19 の 3a): 英字 1 字＋語 (Jリーグ・B級・X線) を小文字で打った綴りの表 (initial_words.txt。tools/initials/make_initial_words.py)。
/// (打つ綴り, 字の形, 合計, 多い形の割合) を綴りの順に。子音の字だけ。試験のビルドでは AZK_INITIAL_WORDS で別の表を読める (off で読まない)
fn initial_words() -> &'static Vec<(String, char, u32, f32)> {
    static TABLE: OnceLock<Vec<(String, char, u32, f32)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let path = match test_env("AZK_INITIAL_WORDS") {
            Some(off) if off == "off" => None,
            Some(path) => Some(std::path::PathBuf::from(path)),
            None => test_env("AZK_WORDS_DIR").map(std::path::PathBuf::from).or_else(dll_dir).map(|d| d.join("initial_words.txt")),
        };
        let text = path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let mut table: Vec<(String, char, u32, f32)> = text.lines().filter(|l| !l.starts_with('#')).filter_map(|l| {
            let mut f = l.split('\t');
            let key = f.next()?.trim();
            let form = f.next()?.trim().chars().next()?;
            let (total, ratio) = (f.next()?.trim().parse().ok()?, f.next()?.trim().parse().ok()?);
            (key.len() >= 2 && key.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') && form.is_ascii_alphabetic()).then(|| (key.to_string(), form, total, ratio))
        }).collect();
        table.sort_by(|a, b| a.0.cmp(&b.0));
        table
    })
}

/// 英字 1 字＋語の表の語を使う線 (合計の回数・多い形の割合)。試験のビルドでは AZK_INITIAL_MIN_COUNT・AZK_INITIAL_MIN_RATIO で替える。
/// 2026-11-04 の測り (Wikipedia の文の英字 1 字＋語を小文字にした組の dev 2,337 行) で 50 回・0.9 を選んだ: 直る 1,008・字の形が違う 2・区切りを違う形にする 0
/// (test 2,366 行で直る 995・0)。日本語の組 (誤判定の 3,660 行×2・一般の文 3,675×3・12 分野) で壊す行は、一番ゆるい線 (20 回・0) でも 0
fn initial_word_lines() -> (u32, f32) {
    static LINES: OnceLock<(u32, f32)> = OnceLock::new();
    *LINES.get_or_init(|| (test_env("AZK_INITIAL_MIN_COUNT").and_then(|v| v.parse().ok()).unwrap_or(50),
        test_env("AZK_INITIAL_MIN_RATIO").and_then(|v| v.parse().ok()).unwrap_or(0.9)))
}

thread_local! {
    /// 試験で英字 1 字＋語の表を差し替える (None なら表のファイルを読む)
    static INITIAL_WORDS_FOR_TEST: std::cell::RefCell<Option<Vec<(String, char, u32, f32)>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn set_test_initial_words(list: &[(&str, char, u32, f32)]) {
    let mut table: Vec<(String, char, u32, f32)> = list.iter().map(|&(k, f, t, r)| (k.to_string(), f, t, r)).collect();
    table.sort_by(|a, b| a.0.cmp(&b.0));
    INITIAL_WORDS_FOR_TEST.with(|t| *t.borrow_mut() = Some(table));
}

fn with_initial_words<T>(f: impl FnOnce(&[(String, char, u32, f32)]) -> T) -> T {
    INITIAL_WORDS_FOR_TEST.with(|table| match table.borrow().as_ref() {
        Some(table) => f(table),
        None => f(initial_words()),
    })
}

/// raw の i から始まる、表の綴り (字＋語の部分) のうち一番長いものの (終わり, 字の形)。線より少ない語は見ない
fn initial_word_at(raw: &str, i: usize) -> Option<(usize, char)> {
    with_initial_words(|table| initial_word_in(table, raw, i))
}

fn initial_word_in(table: &[(String, char, u32, f32)], raw: &str, i: usize) -> Option<(usize, char)> {
    let (min_count, min_ratio) = initial_word_lines();
    let rest = raw.get(i..)?;
    let n = rest.bytes().take_while(|b| b.is_ascii_lowercase() || *b == b'-').count().min(24);
    (2..=n).rev().find_map(|len| {
        let found = table.binary_search_by(|e| e.0.as_str().cmp(&rest[..len])).ok()?;
        let (_, form, total, ratio) = &table[found];
        (*total >= min_count && *ratio >= min_ratio).then_some((i + len, *form))
    })
}

/// 2026-11-04 (仕事 19 の 3a): 子音の字＋語を小文字で打った所 (jri-gu・bkyuugurume・xsenn) の、字の位置 (バイト)。
/// 字のすぐ後ろが表の語の部分の綴りで、その後ろが並びの終わり・英字でない字・ローマ字として読み切れる並び (jri-gunosennshu の no…) のとき。
/// 次の所は見ない: 母音の字 (全部ローマ字として読める: imo-do・akyuu)、n (後ろの子音と ん になる)、数字のすぐ後ろ (10mizyou の m は単位の決まり)、
/// 英字のすぐ後ろ (前が大文字・読み切れない並び)、助詞でない かな のすぐ後ろ (yukkuristai は母音の抜け)、字が後ろとローマ字の音を作る所
/// (tsyatu の ts・同じ子音の重ねの っ)
fn initial_word_span(raw: &str) -> Option<usize> {
    if !initial_letter_words_enabled() || !raw.is_ascii() || with_initial_words(|table| table.is_empty()) { return None; }
    let bytes = raw.as_bytes();
    let n = bytes.len();
    let (keys, prefixes) = table_romaji_keys();
    for i in 0..n {
        let c = bytes[i];
        if !c.is_ascii_lowercase() || is_vowel(c) { continue; }
        // 同じ子音の重ねの 2 つ目 (menn の 2 つ目の n は ん、mattya の 2 つ目の t は っ の続き) からも始めない
        if i > 0 && (bytes[i - 1].is_ascii_digit() || bytes[i - 1].is_ascii_uppercase() || bytes[i - 1] == c) { continue; }
        // 前の小文字の並びは、ローマ字として読み切れて助詞で終わること (kotosiha + jri-gu)。英字の語のすぐ後ろ (koreheabsence の a + b)・
        // 助詞でない かな のすぐ後ろ (yukkuristai は し の母音の抜け) は見ない
        let run_start = (0..i).rev().take_while(|&k| bytes[k].is_ascii_lowercase()).last().unwrap_or(i);
        // (助詞の前までもローマ字として読み切れること。cache の he は助詞でない)
        const PARTICLES: &[&str] = &["ha", "ga", "no", "wo", "ni", "de", "to", "mo", "he", "ya"];
        let before = &raw[run_start..i];
        let after_particle = PARTICLES.iter().any(|p| before.strip_suffix(p).is_some_and(|head| head.is_empty() || native_romaji_readable(head, false)));
        if run_start < i && !after_particle { continue; }
        // 字が後ろとローマ字の音を作る所
        // (字と次の字が綴りかその頭: tsya の ts・sh・th。n は、後ろの子音と ん になる打ち方があるので見ない: Nゲージ は Shift で)
        if c == b'n' || (2..=4).any(|l| raw.get(i..i + l).is_some_and(|k| keys.contains(k) || prefixes.contains(k))) || bytes.get(i + 1) == Some(&c)
            || (c == b't' && raw[i + 1..].starts_with("ch")) { continue; }
        // 字から始まる並びが英単語の表の語 (korehegrace の grace・sonoskin の skin) なら見ない
        let letters_end = (i..n).find(|&k| !bytes[k].is_ascii_lowercase()).unwrap_or(n);
        if (i + 3..=letters_end).any(|k| is_word(&raw[i..k])) { continue; }
        let Some((end, _)) = initial_word_at(raw, i) else { continue };
        let rest_end = (end..n).find(|&k| !bytes[k].is_ascii_lowercase()).unwrap_or(n);
        if end < n && bytes[end].is_ascii_uppercase() { continue; }
        let pending = rest_end == n && RUN_AT_END.get() && !FINISHING.get();
        if rest_end > end && !native_romaji_readable(&raw[end..rest_end], pending) { continue; }
        return Some(i);
    }
    None
}

/// 2026-11-04 (仕事 19 の 3a): 英字 1 字の語 (letter) のすぐ後ろのローマ字 (after) が、表の語の部分の綴りで始まるときの字の形 (jri-gu の j → J・xziku の x → x)
pub fn initial_word_case(letter: &str, after: &str) -> Option<&'static str> {
    const FORMS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    if !initial_letter_words_enabled() || letter.len() != 1 || !letter.bytes().all(|b| b.is_ascii_lowercase()) { return None; }
    let (_, form) = initial_word_at(&format!("{letter}{after}"), 0)?;
    let at = FORMS.find(form)?;
    Some(&FORMS[at..at + 1])
}

// Deliberate punctuation is stronger evidence than a readable lowercase syllable.
fn literal_notation_span(raw: &str, style: Style) -> Option<(usize, usize)> {
    // 2026-10-13: メールアドレス (tanaka@example.com・yamada.taro@example.co.jp)。@ の前後が英数字で、@ の後ろに . で区切った語がある
    // (打っている途中は . の前でもよい)。こうしないと たなか＠example。com になる
    if let Some(at) = raw.find('@') {
        let local_char = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
        let start = raw[..at].rfind(|c: char| !local_char(c)).map_or(0, |i| i + 1);
        let domain_len = raw[at + 1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-')).unwrap_or(raw.len() - at - 1);
        let domain = raw[at + 1..at + 1 + domain_len].trim_end_matches('.');
        let local = &raw[start..at];
        let tld_ok = domain.rsplit_once('.').is_some_and(|(head, tld)| !head.is_empty() && tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic()));
        let typing = !FINISHING.get() && at + 1 + domain_len == raw.len() && !domain.is_empty();
        if !local.is_empty() && local.bytes().any(|b| b.is_ascii_alphabetic()) && (tld_ok || typing) {
            return Some((start, at + 1 + domain.len()));
        }
    }
    const HYPHENATED: &[&str] = &["lo-fi", "sci-fi", "hi-fi", "wi-fi", "e-mail"];
    let extensions = file_extensions();
    let boundary = |suffix: &str| suffix.is_empty() || suffix.starts_with(|c: char| !c.is_ascii_alphabetic())
        || PARTICLES.iter().any(|p| suffix.starts_with(p));
    let lower = raw.to_ascii_lowercase();
    for compound in HYPHENATED {
        for (start, _) in lower.match_indices(compound) {
            let end = start + compound.len();
            let component_start = start + compound.rfind('-').unwrap() + 1;
            let component_end = raw[component_start..].find(|c: char| !c.is_ascii_alphabetic())
                .map_or(raw.len(), |length| component_start + length);
            let component = &lower[component_start..component_end];
            if component_end > end && (is_word(component) || words().context.contains(component)) { continue; }
            let suffix = &raw[end..];
            if !suffix.is_empty() && !suffix.starts_with(|c: char| !c.is_ascii_alphabetic())
                && !japanese_particle_suffix(suffix) { continue; }
            let before = segment_plain(&raw[..start], style);
            if start == 0 || before.last().is_some_and(|t| !t.raw.chars().all(|c| c.is_ascii_alphabetic())
                || PARTICLES.contains(&t.raw.as_str())) { return Some((start, end)); }
        }
    }
    for (dot, ch) in raw.char_indices() {
        if ch != '.' || dot == 0 { continue; }
        let tail = &lower[dot + 1..];
        let extension = extensions.iter().filter(|ext| tail.starts_with(**ext) && boundary(&tail[ext.len()..]))
            .map(|ext| ext.len()).max()
            .or_else(|| (!FINISHING.get() && !tail.is_empty() && extensions.iter().any(|ext| ext.starts_with(tail))).then_some(tail.len()));
        let Some(length) = extension else { continue; };
        let run_start = raw[..dot].rfind(|c: char| !c.is_ascii_alphabetic()).map_or(0, |i| i + raw[i..].chars().next().unwrap().len_utf8());
        let stem = &raw[run_start..dot];
        // 2026-11-05 (仕事 22 の 1): 数字のすぐ後ろの拡張子 (IMG_0001.JPG・report2.pdf・01.mp3) も、. と拡張子を英字のまま (数字はそのまま)
        if period_auto() && stem.is_empty() && raw[..dot].ends_with(|c: char| c.is_ascii_digit()) { return Some((dot, dot + 1 + length)); }
        if stem.is_empty() { continue; }
        let tokens = segment_run(stem, style);
        let mut start = dot;
        for token in tokens.iter().rev() {
            if token.kind != TokenKind::English { break; }
            start -= token.raw.len();
        }
        if start == dot && is_word(&stem.to_ascii_lowercase()) && !learned_kana(&stem.to_ascii_lowercase()) {
            start = run_start;
        }
        // 語の途中の英単語 (ローマ字の語 + 助詞 + 英単語) を探すのは、前からの 23 個の拡張子のときだけ (2026-11-05 に足した mp3 で、
        // mounerune.mp3 = もうねるね。mp3 の rune を英単語とみて rune.mp3 にしていた)
        let old_extension = file_extensions_all()[..23].iter().any(|ext| ext.len() == length && tail.starts_with(*ext));
        if start == dot && (old_extension || !period_auto()) {
            for candidate in run_start..dot {
                let word = &lower[candidate..dot];
                if word.len() < 3 || !is_word(word) || learned_kana(word) { continue; }
                let before = segment_run(&raw[run_start..candidate], style);
                if before.last().is_some_and(|t| PARTICLES.contains(&t.raw.as_str())) {
                    start = candidate;
                    break;
                }
            }
        }
        if start < dot { return Some((start, dot + 1 + length)); }
    }
    // 2026-11-05 (仕事 22 の 2): かなのすぐ後ろの . に続く、小文字で打った拡張子 (siryou.pdfha)。拡張子だけを英字の語にし、. は 。 のまま。
    // 資料.pdf (名詞のあと) か 〜だよ。pdf (文の終わりのあと) かは、変換サーバーが変換するときに品詞で決める (slash_dot.swift)
    for (dot, ch) in raw.char_indices() {
        if !period_auto() { break; }
        if ch != '.' || dot == 0 || !raw[..dot].ends_with(|c: char| c.is_ascii_lowercase()) { continue; }
        if let Some(length) = dot_extension_at(&raw[dot + 1..]) { return Some((dot + 1, dot + 1 + length)); }
    }
    // Initials such as B.o.B are not Japanese sentence punctuation.
    let bytes = raw.as_bytes();
    for start in 0..bytes.len() {
        if !bytes[start].is_ascii_uppercase() { continue; }
        if start > 0 && bytes[start - 1].is_ascii_alphabetic() {
            let before = segment_plain(&raw[..start], style);
            if !before.last().is_some_and(|t| t.kind == TokenKind::Romaji && PARTICLES.contains(&t.raw.as_str())) { continue; }
        }
        let mut end = start + 1;
        let mut dots = 0;
        while end + 1 < bytes.len() && bytes[end] == b'.' && bytes[end + 1].is_ascii_alphabetic() {
            end += 2;
            dots += 1;
        }
        if dots >= 2 && boundary(&raw[end..]) { return Some((start, end)); }
    }
    None
}

/// 2026-11-05 (仕事 22 の 2): かなのすぐ後ろの . に続くと、資料.pdf か 〜だよ。pdf かを変換サーバーが決める拡張子 (小文字で打ったものだけ)。
/// 変換サーバーの dotExtensionForms (slash_dot.swift) と同じ並び。ローマ字として読み切れる綴り (go・ini・iso・java) は入れない
pub fn dot_extensions() -> &'static [&'static str] {
    &["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "heic", "tiff",
      "txt", "md", "csv", "zip", "exe", "mp3", "mp4", "wav", "mov", "html", "json"]
}

/// 打った列 tail の頭が、小文字で打った dot_extensions の拡張子で、その後ろが 打ち終わり・英字でも数字でもない字・助詞のとき、拡張子の長さ
fn dot_extension_at(tail: &str) -> Option<usize> {
    let after = |rest: &str| rest.is_empty() || rest.starts_with(|c: char| !c.is_ascii_alphanumeric())
        || (["ha", "ga", "wo", "ni", "de", "to", "mo", "no", "he", "ya", "kara", "made", "yori"].iter().any(|p| rest.starts_with(p))
            && native_romaji_readable(rest, !FINISHING.get()));
    dot_extensions().iter().filter(|ext| tail.starts_with(**ext) && after(&tail[ext.len()..])).map(|ext| ext.len()).max()
}

/// 2026-11-05 (仕事 22 の 2): 英字の語 word が、かなのすぐ後ろの . (前のローマ字の並び before の終わり) に続く、小文字で打った拡張子か。
/// そうなら表記直し (pdf → PDF) をせずに変換サーバーへ渡す (。 のままなら変換サーバーが表の形にする)
pub fn kana_dot_extension(before: &str, word: &str) -> bool {
    period_auto() && before.strip_suffix('.').is_some_and(|b| b.ends_with(|c: char| c.is_ascii_lowercase())) && dot_extensions().contains(&word)
}

/// 2026-11-05 (仕事 22): ファイルの拡張子 (小文字)。英字・数字のすぐ後ろの . に続くと、. と拡張子を英字のまま入れる (literal_notation_span)。
/// 前は dll・json・md・pdf … の 23 個。画像・文書・音・動画・圧縮の拡張子 (png・docx・mp3・zip) を足した
pub fn file_extensions() -> &'static [&'static str] {
    if !period_auto() { return &file_extensions_all()[..23]; }
    file_extensions_all()
}

fn file_extensions_all() -> &'static [&'static str] {
    &["dll", "exe", "config", "json", "jsonl", "js", "jsx", "ts", "tsx", "md", "txt", "py", "rs", "go", "toml", "yaml", "yml", "ini", "csv", "log",
      "pdf", "html", "css",
      "doc", "docx", "xls", "xlsx", "ppt", "pptx", "png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "heic", "tif", "tiff",
      "zip", "mp3", "mp4", "wav", "mov", "avi", "mkv", "m4a", "xml", "tex", "bat", "ps1", "sh", "cpp", "hpp", "java", "kt", "swift", "rb", "php",
      "htm", "apk", "iso", "tsv", "epub", "psd", "dmg", "msi"]
}

const QUANTITY_UNITS: &[&str] = &[
    "mol", "mmol", "umol", "nmol", "pmol", "M", "mM", "uM", "nM", "pM",
    "L", "l", "mL", "ml", "uL", "ul", "nL", "g", "mg", "kg", "ug", "ng", "pg",
    "m", "mm", "cm", "um", "nm", "km", "s", "ms", "min", "h",
    "Hz", "kHz", "MHz", "GHz", "V", "mV", "A", "mA", "W", "mW",
    "J", "kJ", "Pa", "kPa", "MPa", "K", "N", "Da", "kDa", "bp", "kbp", "Mbp",
];

/// 2026-11-01: よく打つ分野 (設定 input.domains_focus) の英字の略語 (分野辞書の %APPDATA%\HaniWord\domain\domain_<名前>_abbrev.txt。
/// mRNA・NSAIDs・DBMS など。小文字で打つとローマ字として読める略語と、英字の判定の表にもうある語は、表を作るときに除いてある)。
/// (小文字, 表記) の組。文脈用の表 (語の切れ目に収まるときだけ英字。3 字以上) と表記直しの表に足す。利用者専用の表と同じく、
/// 読んでよい場所 (data_dir) からだけ読む。
/// 試験のビルドでは AZK_DOMAINS_FOCUS (, 区切り) で分野を選ぶ
fn domain_abbreviations() -> &'static Vec<(String, String)> {
    static LIST: OnceLock<Vec<(String, String)>> = OnceLock::new();
    LIST.get_or_init(|| {
        let Some(dir) = data_dir() else { return vec![] };
        let focus: Vec<String> = match test_env("AZK_DOMAINS_FOCUS") {
            Some(names) => names.split(',').filter(|name| !name.is_empty()).map(str::to_string).collect(),
            None if cfg!(test) => vec![],
            None => shared::AppConfig::read_or_default().input.domains_focus,
        };
        let mut out = vec![];
        for name in focus {
            // 名前の決まり (英小文字・数字・_、32 字まで) に合わないものはパスに使わない
            if name.is_empty() || name.len() > 32 || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') { continue; }
            let path = dir.join("domain").join(format!("domain_{name}_abbrev.txt"));
            if std::fs::metadata(&path).map_or(true, |meta| !meta.is_file() || meta.len() > 256 << 10) { continue; }
            if let Ok(text) = std::fs::read_to_string(&path) { out.extend(parse_abbreviations(&text)); }
        }
        crate::debug_log::write(&format!("auto_english: {} domain abbreviations", out.len()));
        out
    })
}

/// 2026-11-01: よく打つ分野の略語の表にしか無い語 (英字の判定の表にも文脈用の表にも無い語) の表記 (小文字 → 表記)。load_words が入れる
static DOMAIN_ONLY: OnceLock<HashMap<String, String>> = OnceLock::new();

fn domain_abbreviation_form(lower: &str) -> Option<&'static str> {
    DOMAIN_ONLY.get().and_then(|forms| forms.get(lower)).map(String::as_str)
}

/// 略語の表の 1 行 1 語 (表記そのまま。# は説明)。英字だけの 2〜30 字の語 (数字を含む HbA1c・IPv4 は英字の並びに収まらないので使わない)
fn parse_abbreviations(text: &str) -> Vec<(String, String)> {
    text.lines().map(|line| line.trim_start_matches('\u{feff}').trim())
        .filter(|w| (2..=30).contains(&w.len()) && !w.starts_with('#') && w.bytes().all(|b| b.is_ascii_alphabetic()))
        .map(|w| (w.to_ascii_lowercase(), w.to_string())).collect()
}

/// 2026-10-31: 2 字以上でローマ字として読めない単位・記号 (kg・km・mL・Hz・hPa・mmHg・pH) を判定の語に入れる。かなのすぐ後ろ (sonnnakgdesu) で
/// 前の音を巻き込んで表に無い語 (nakg) にしていた。読める単位 (pa = ぱ・min = みん・da = だ) は入れない
fn add_unit_words(set: &mut HashSet<String>) {
    for unit in QUANTITY_UNITS.iter().chain(LETTER_UNITS) {
        let lower = unit.to_ascii_lowercase();
        if lower.len() >= 2 && unit_word_shape(&lower) { set.insert(lower); }
    }
}

/// かなの並びの中で単位として読んでよい形か。ローマ字として読めず、母音で始まらず・終わらず (kda = く + だ の母音の抜け・mpa は compare の中)、
/// n で始まらず (ng・nl・nm は ん + 子音)、音節の打ちかけでない (gy = ぎゃ の打ちかけ)
fn unit_word_shape(lower: &str) -> bool {
    let b = lower.as_bytes();
    !native_romaji_readable(lower, false) && !is_vowel(b[0]) && !is_vowel(b[b.len() - 1]) && b[0] != b'n' && !is_syllable_prefix(lower)
}

/// 2026-10-31: QUANTITY_UNITS に無い単位・記号 (分野ごとの英字の並びの洗い出しで、和文の中で崩れていたもの)
const LETTER_UNITS: &[&str] = &[
    "hPa", "mmHg", "kcal", "ppm", "ppb", "atm", "bpm", "dB", "eV", "keV", "MeV", "GeV", "Bq", "Gy", "Sv", "lx",
    "bps", "kbps", "Mbps", "Gbps", "KB", "MB", "GB", "TB", "pH",
];

/// 2026-10-31: 数字のあとの 2 字以上の単位 (大文字小文字を問わない) の長さ。単位そのものはローマ字として読めず (pa = ぱ・min = みん は除く)、
/// 後ろが無いか、ローマ字として読める並び (5kgizyou = 5kg以上・100kmueni・60hzmataha) のもの。前は 5|k|gi|じょう・[hzmata] になっていた。
/// 1 字短い単位 (打ったとおりの字で比べる。kg の k は K = ケルビン ではない) + 残りでも読めるとき (10mmade = 10m + まで) は決めない
fn unit_before_kana(run: &str) -> Option<usize> {
    let lower = run.to_ascii_lowercase();
    let unit = |u: &str| QUANTITY_UNITS.iter().chain(LETTER_UNITS).any(|x| *x == u);
    // 打った字は小文字だけか、表の表記どおり (2Gbai の Gb は GB にしない)
    QUANTITY_UNITS.iter().chain(LETTER_UNITS)
        .filter(|u| u.len() <= run.len() && (&run[..u.len()] == **u || (run[..u.len()].bytes().all(|b| b.is_ascii_lowercase()) && run[..u.len()].eq_ignore_ascii_case(u))))
        .map(|u| u.to_ascii_lowercase())
        .filter(|u| u.len() >= 2 && !native_romaji_readable(u, false))
        .filter(|u| {
            let rest = &lower[u.len()..];
            // 単位のあとによく来る語 (以上・以下・以内・上・内) が続くなら長いほう (30bpmika = 30bpm 以下。bp + みか にしない)
            let after_unit = ["izyou", "ijou", "ika", "inai", "ue", "uti", "uchi"].iter().any(|w| rest.starts_with(w));
            (rest.is_empty() || native_romaji_readable(rest, !FINISHING.get()))
                && (after_unit || !(unit(&run[..u.len() - 1]) && native_romaji_readable(&lower[u.len() - 1..], !FINISHING.get())))
        })
        .map(|u| u.len()).max()
}

fn quantity_before(tokens: &[Token]) -> bool {
    if let [.., unit, slash] = tokens {
        if slash.kind == TokenKind::English && slash.raw == "/" && QUANTITY_UNITS.contains(&unit.raw.as_str()) {
            return true;
        }
    }
    tokens.iter().rev().find(|t| !t.raw.chars().all(|c| c == ' ' || c == '\t'))
        .is_some_and(|t| t.raw.chars().last().is_some_and(|c| c.is_ascii_digit() || ('\u{ff10}'..='\u{ff19}').contains(&c)))
}

/// 2026-10-20: 英字の語 (GPT・Pixel・iPhone・GPT-) のすぐ後ろが数字で終わっているか (GPT4o の o の手前)
fn latin_before_digits(tokens: &[Token]) -> bool {
    let mut k = tokens.len();
    while k > 0 && !tokens[k - 1].raw.is_empty() && tokens[k - 1].raw.chars().all(|c| c.is_ascii_digit()) { k -= 1; }
    k < tokens.len() && k > 0 && tokens[k - 1].kind == TokenKind::English
        && tokens[k - 1].raw.chars().last().is_some_and(|c| c.is_ascii_alphabetic() || c == '-')
}

fn segment_quantity_run(run: &str, style: Style, after_number: bool, after_long_vowel: bool, after_latin_number: bool) -> Vec<Token> {
    // 2026-10-20: 英字の語 + 数字 のすぐ後ろの小文字 1 字 (GPT4o・Pixel8a・iPhone16e) は型番の一部として英字。
    // 入力の終わり (Space・Enter を押した時点)・記号や空白の前か、後ろが を・で のときだけ (母音 1 字 + を・で で始まる日本語の語はまず無い)。
    // ほかの助詞の前 (o + to = 音、a + no = あの) は日本語の語の頭と見分けられないので、迷ったら日本語
    if after_latin_number && run.as_bytes()[0].is_ascii_lowercase() {
        let rest = &run[1..];
        let at_end = rest.is_empty() && (FINISHING.get() || !RUN_AT_END.get());
        let before_particle = (rest.starts_with("wo") || rest.starts_with("de")) && native_romaji_readable(rest, !FINISHING.get());
        if at_end || before_particle {
            let mut tokens = vec![Token { raw: run[..1].into(), kind: TokenKind::English }];
            if !rest.is_empty() { tokens.extend(segment_run(rest, style)); }
            return tokens;
        }
    }
    if after_long_vowel && run.bytes().all(|c| c.is_ascii_lowercase())
        && !is_word(run) && !words().context.contains(run)
        && native_romaji_readable(run, !FINISHING.get()) {
        return super::romaji_style::simulate(run).into_iter()
            .map(|unit| Token { raw: unit.keys, kind: TokenKind::Romaji }).collect();
    }
    if after_number {
        let length = QUANTITY_UNITS.iter().filter(|unit| run.starts_with(**unit))
            .filter(|unit| {
                let rest = &run[unit.len()..];
                rest.is_empty() || ["ha", "ga", "wo", "no", "ni", "de", "to", "mo", "he", "ya"].iter().any(|p| rest.starts_with(p))
                    || (!FINISHING.get() && (is_syllable_prefix(rest) || rest == "n"))
            })
            .map(|unit| unit.len()).max()
            .or_else(|| unit_before_kana(run));
        if let Some(length) = length {
            let mut tokens = vec![Token { raw: run[..length].into(), kind: TokenKind::English }];
            if length < run.len() { tokens.extend(segment_run(&run[length..], style)); }
            return tokens;
        }
        if !FINISHING.get() && QUANTITY_UNITS.iter().any(|unit| unit.starts_with(run)) {
            return vec![Token { raw: run.into(), kind: TokenKind::English }];
        }
    }
    segment_run(run, style)
}

fn segment_plain(raw: &str, style: Style) -> Vec<Token> {
    let mut tokens = vec![];
    let mut run = String::new();
    // 英字の連続ごとの (始まりの token, 終わりの token)。空白で区切った英単語の並びを見るため
    let mut runs: Vec<(usize, usize)> = vec![];
    let all: Vec<char> = raw.chars().collect();
    for (k, &c) in all.iter().enumerate() {
        // 数字にはさまれた . , は小数点・桁区切り (2.0, 1,000)。英字のまま入れる。
        // , は後ろがちょうど 3 桁のときだけ (iOS と同じ。1,00 は 1、00)。打っている途中 (末尾が 3 桁未満) は , のまま
        let group = all[k + 1..].iter().take_while(|n| n.is_ascii_digit()).count();
        let comma_ok = c != ',' || group == 3 || (group < 3 && k + 1 + group == all.len());
        if number_punctuation() && (c == '.' || c == ',') && comma_ok && run.is_empty() && k > 0 && all[k - 1].is_ascii_digit() && group > 0 {
            tokens.push(Token { raw: c.to_string(), kind: TokenKind::English });
            continue;
        }
        if c.is_ascii_alphabetic() {
            run.push(c);
        } else {
            // 2026-11-06 (まとめ役 09:57): z 1 字 (前が英字でない) のすぐ後ろの . , / - [ ] は、Mozc と同じ z ＋記号の近道 (… ‥ ・ 〜 『 』)。
            // z も記号もローマ字の側に入れて、変換サーバーのローマ字の表で字にする (/ と . の打ち分け・英字の後ろのハイフンの決まりにかけない)
            // ローマ字のすぐ後ろ (korez. = これ…) も同じ。並び全体が英単語の表の語 (jazz・quiz・fizz) のときと、z の前がローマ字として読めないときは今までどおり
            let z_shortcut = ".,/-[]".contains(c) && run.ends_with('z')
                && (run == "z" || (!is_word(&run.to_ascii_lowercase()) && native_romaji_readable(&run[..run.len() - 1], false)));
            if z_shortcut {
                let head = &run[..run.len() - 1];
                if !head.is_empty() {
                    let after_long_vowel = tokens.last().is_some_and(|t: &Token| t.kind == TokenKind::Romaji && t.raw == "-");
                    let first = tokens.len();
                    tokens.extend(segment_plain_run_after_slash(head, style, &tokens, after_long_vowel));
                    runs.push((first, tokens.len()));
                }
                tokens.push(Token { raw: "z".into(), kind: TokenKind::Romaji });
                tokens.push(Token { raw: c.to_string(), kind: TokenKind::Romaji });
                run.clear();
                continue;
            }
            if !run.is_empty() {
                let after_long_vowel = tokens.last().is_some_and(|t: &Token| t.kind == TokenKind::Romaji && t.raw == "-");
                let previous = RUN_AT_END.replace(false);
                let previous_digit = RUN_BEFORE_DIGIT.replace(c.is_ascii_digit());
                let first = tokens.len();
                tokens.extend(segment_plain_run_after_slash(&run, style, &tokens, after_long_vowel));
                runs.push((first, tokens.len()));
                RUN_AT_END.set(previous);
                RUN_BEFORE_DIGIT.set(previous_digit);
                run.clear();
            }
            // 英単語のすぐ後ろの - は長音 (ー) ではなくハイフン (e-mail)
            let after_english = tokens.last().is_some_and(|t| t.kind == TokenKind::English);
            let unit_slash = c == '/' && tokens.last().is_some_and(|t| t.kind == TokenKind::English && QUANTITY_UNITS.contains(&t.raw.as_str()));
            // 2026-11-05 (仕事 22 の 1、まとめ役 12:28): 数字のすぐ後ろと、小文字の英字の語のすぐ後ろの / は、・ にせず半角の / (1/2・11/5・and/or・
            // github/main・w/o)。打った時点で左の字で決める。Wikipedia 日本語版で / が 9 割を越える形だけ (小文字の語 0.928・小文字 1 字 0.920)。
            // 略語 (CT・DVD 0.532)・頭だけ大文字 (Youtube 0.758)・混ざる語 (iPhone 0.639)・大文字 1 字 (ジョン・F・ケネディ 0.278) と、
            // 名前の中 (前が ・: フォン・der) は ・ のまま (tools/slash_dot/scan_latin_pairs.py)。略語どうしの決まった組 (TCP/IP) と、
            // 数字にはさまれた / で後ろの語が ・ の形のもの (1・2年生) は、変換するときに変換サーバーが決め直す (slash_dot.swift)
            let lower_word = after_english && tokens.last().is_some_and(|t| !t.raw.is_empty() && t.raw.bytes().all(|b| b.is_ascii_lowercase()))
                && !(tokens.len() >= 2 && tokens[tokens.len() - 2].kind == TokenKind::Romaji && tokens[tokens.len() - 2].raw == "/");
            let latin_slash = c == '/' && slash_auto() && ((k > 0 && all[k - 1].is_ascii_digit()) || lower_word);
            // 2026-11-05 (仕事 28 の 1): いつも半角の / か . にする設定なら、かなの入力中もいつも英字の側 (半角)
            let always_latin = (c == '/' && slash_key() == KeyForm::Latin) || (c == '.' && period_key() == KeyForm::Latin);
            let kind = if (c == '-' && after_english) || unit_slash || latin_slash || always_latin { TokenKind::English } else { TokenKind::Romaji };
            tokens.push(Token { raw: c.to_string(), kind });
        }
    }
    if !run.is_empty() {
        let after_long_vowel = tokens.last().is_some_and(|t| t.kind == TokenKind::Romaji && t.raw == "-");
        let first = tokens.len();
        tokens.extend(segment_plain_run_after_slash(&run, style, &tokens, after_long_vowel));
        runs.push((first, tokens.len()));
    }
    english_phrase_runs(numeronym_runs(tokens, &runs), &runs)
}

/// 2026-11-05 (仕事 22 の 1): 小文字の英字の語のすぐ後ろの半角の / (and/or・src/main・input/output) に続く並びは、全体が英単語の表の語なら英字の語に
fn segment_plain_run_after_slash(run: &str, style: Style, before: &[Token], after_long_vowel: bool) -> Vec<Token> {
    let latin_slash = matches!(before, [.., word, slash] if slash.kind == TokenKind::English && slash.raw == "/"
        && word.kind == TokenKind::English && word.raw.bytes().all(|b| b.is_ascii_lowercase()));
    let lower = run.to_ascii_lowercase();
    if slash_auto() && latin_slash && (is_word(&lower) || TWO_LETTER_WORDS.contains(&lower.as_str())) && !learned_kana(&lower) {
        return vec![Token { raw: run.to_string(), kind: TokenKind::English }];
    }
    segment_plain_run(run, style, before, after_long_vowel)
}

/// segment_plain の英字の並び 1 つ (前に記号・空白・数字があるか、入力の頭)。
/// 2026-10-31: 並びの頭の zh・zj・zk・zl は矢印 (← ↓ ↑ →) の綴りのまま、後ろを打ち進めても英字の語に入れない
/// (前は zhkousin が [zh] + こうしん になり、打った時点で見えていた ← が zh に戻っていた)。並びが表の英単語で始まるとき (zlib) は今までどおり。
/// 語の途中 (英字・かなのあと: xzk・korezh) は今までどおり
fn segment_plain_run(run: &str, style: Style, before: &[Token], after_long_vowel: bool) -> Vec<Token> {
    let lower = run.to_ascii_lowercase();
    let arrow = run.len() > 2 && matches!(&run.as_bytes()[..2], b"zh" | b"zj" | b"zk" | b"zl")
        && !(3..=lower.len().min(30)).any(|end| is_word(&lower[..end]) || words().context.contains(&lower[..end]));
    if arrow {
        let mut tokens: Vec<Token> = run[..2].chars().map(|c| Token { raw: c.to_string(), kind: TokenKind::Romaji }).collect();
        tokens.extend(segment_quantity_run(&run[2..], style, false, false, false));
        return tokens;
    }
    segment_quantity_run(run, style, quantity_before(before), after_long_vowel, latin_before_digits(before))
}

/// 2026-10-13: 数字をはさんだ短い小文字の英字 (i18n・l10n・k8s・h2o・b2b・p2p) は全体を英字に。両側が 1〜2 字で、
/// どちらかがローマ字で読めない字か、数字が 2 桁以上のときだけ (dai1kai = 第1回 は両側が読めて長いので対象外)。トークンの数は変えない
fn numeronym_runs(mut tokens: Vec<Token>, runs: &[(usize, usize)]) -> Vec<Token> {
    for pair in runs.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        let between = &tokens[left.1..right.0];
        if between.is_empty() || !between.iter().all(|t| t.raw.chars().all(|c| c.is_ascii_digit())) { continue; }
        let text = |(a, b): (usize, usize), tokens: &[Token]| tokens[a..b].iter().map(|t| t.raw.as_str()).collect::<String>();
        let (l, r) = (text(left, &tokens), text(right, &tokens));
        // 大文字を含むもの (H2O・H2SO4・CuSO4 のような化学式) は対象外。変換サーバーが元素記号と数字を見て下付きにする
        if l.bytes().chain(r.bytes()).any(|b| b.is_ascii_uppercase()) { continue; }
        let digits: usize = between.iter().map(|t| t.raw.len()).sum();
        if l.len() > 2 || r.len() > 2 { continue; }
        // 後ろがかな 1 字に読める 2 文字 (x86 + no、m3 + ka) は助詞なので巻き込まない
        if r.len() == 2 && native_romaji_readable(&r, false) { continue; }
        if !(digits >= 2 || !native_romaji_readable(&l, false) || !native_romaji_readable(&r, false)) { continue; }
        for t in tokens[left.0..right.1].iter_mut() { t.kind = TokenKind::English; }
    }
    tokens
}

/// 2026-10-13: 空白 1 つで区切った英単語の並び (thank you・this is a pen・open the file)。2 語以上がどれも英単語で、
/// うち 1 語がもう英字なら、並び全体を英字にする (you・a・the を よう・あ にしない)。日本語の語 (github no settei の settei) が
/// 混ざる並びはそこで切れるので、の は日本語のまま。英語の文を崩しやすい型
fn english_phrase_runs(tokens: Vec<Token>, runs: &[(usize, usize)]) -> Vec<Token> {
    if runs.len() < 2 {
        return tokens;
    }
    let text = |&(a, b): &(usize, usize)| tokens[a..b].iter().map(|t| t.raw.as_str()).collect::<String>();
    let known = |w: &str| {
        let l = w.to_ascii_lowercase();
        (words().set.contains(&l) || words().context.contains(&l) || TWO_LETTER_WORDS.contains(&l.as_str()) || l == "a" || l == "i")
            && !learned_kana(&l)
    };
    // もう英字になっている語か、ローマ字として読み切れない英単語 (tomorrow) を、英語の並びの手がかりにする
    let english = |&(a, b): &(usize, usize)| tokens[a..b].iter().all(|t| t.kind == TokenKind::English)
        || !native_romaji_readable(&text(&(a, b)).to_ascii_lowercase(), false);
    // 固有名詞 (大文字を含む語・表記直しの表にある語) のすぐ後ろの助詞は日本語 (GitHub no repo の no、Google to Apple の to)。
    // 英語として不自然な並び (github no)
    let proper = |w: &str| (w.len() > 1 && w.chars().any(|c| c.is_ascii_uppercase())) || canonical_case(&w.to_ascii_lowercase()).is_some();
    let spaced = |left: &(usize, usize), right: &(usize, usize)| right.0 == left.1 + 1 && tokens[left.1].raw == " ";
    let mut convert = vec![false; runs.len()];
    let mut i = 0;
    while i < runs.len() {
        if !known(&text(&runs[i])) { i += 1; continue; }
        // 固有名詞の直後の助詞は、次の並びの頭にもしない (Google to Apple の to)
        if i > 0 && spaced(&runs[i - 1], &runs[i]) && PARTICLES.contains(&text(&runs[i]).to_ascii_lowercase().as_str())
            && proper(&text(&runs[i - 1])) { i += 1; continue; }
        let mut j = i + 1;
        while j < runs.len() && spaced(&runs[j - 1], &runs[j]) && known(&text(&runs[j]))
            && !(PARTICLES.contains(&text(&runs[j]).to_ascii_lowercase().as_str()) && proper(&text(&runs[j - 1]))) { j += 1; }
        let next = j;
        // 並びの端の助詞が、空白をはさんで日本語の語に続くなら日本語 (github no settei の no、settei no github の no)
        let particle = |k: usize| PARTICLES.contains(&text(&runs[k]).to_ascii_lowercase().as_str());
        let mut start = i;
        let mut end = j;
        while end - start >= 2 && particle(end - 1) && end < runs.len() && spaced(&runs[end - 1], &runs[end]) && !known(&text(&runs[end])) { end -= 1; }
        while end - start >= 2 && particle(start) && start > 0 && spaced(&runs[start - 1], &runs[start]) && !known(&text(&runs[start - 1])) { start += 1; }
        if end - start >= 2 && runs[start..end].iter().any(english) {
            for k in start..end { convert[k] = true; }
        }
        i = next;
    }
    if !convert.iter().any(|c| *c) {
        return tokens;
    }
    let mut out = Vec::with_capacity(tokens.len());
    let mut at = 0;
    for (k, range) in runs.iter().enumerate() {
        if !convert[k] { continue; }
        out.extend(tokens[at..range.0].iter().cloned());
        out.push(Token { raw: text(range), kind: TokenKind::English });
        at = range.1;
    }
    out.extend(tokens[at..].iter().cloned());
    out
}

pub fn has_english(tokens: &[Token]) -> bool {
    tokens.iter().any(|t| t.kind == TokenKind::English)
}

/// 連続する同じ種類をまとめる (サーバーへ入れるとき用)
pub fn runs(tokens: &[Token]) -> Vec<(String, TokenKind)> {
    let mut out: Vec<(String, TokenKind)> = vec![];
    for t in tokens {
        match out.last_mut() {
            Some((text, kind)) if *kind == t.kind && t.kind == TokenKind::Romaji => text.push_str(&t.raw),
            _ => out.push((t.raw.clone(), t.kind.clone())),
        }
    }
    out
}

/// 2026-10-28 (試験中): ローマ字の母音の抜け (souiuktodesu の k = そういうことです) を直す候補。
/// 打った文字の列に母音を補う位置 (バイト) と母音の組を返す。どれを選ぶか (選ばないか) は変換サーバーの採点 (ScoreReadings) で決める (composition.rs の repair_typos。測りは vowel_repair_probe)。
/// 直すのはローマ字として読めない子音 1 字 (so|u|i|u|k|to の k) だけ。英字にした語 (英単語の表の kid・表に無い語の規則の語) は、
/// 打ったとおりが英単語や名前のことがあるので直さない (英語の自動判別とぶつけない。10-28 の決定)。打っている途中の末尾の子音は打ちかけなので直さない。
/// 前後はかな (読めるローマ字) か並びの端のときだけ (読めない字の並び dmm・npm は名前や略語とみて直さない)。補う所が 3 か所以上ある入力も直さない
pub fn vowel_repairs(typed: &str, finishing: bool) -> Vec<(usize, char)> {
    let tokens = if finishing { segment_finishing(typed) } else { segment(typed) };
    let stray = |j: usize| stray_consonant(&tokens, j);
    let kana = |j: usize| tokens[j].kind == TokenKind::Romaji && !stray(j);
    let mut out: Vec<(usize, char)> = vec![];
    let mut at = 0;
    for (k, t) in tokens.iter().enumerate() {
        let last = k + 1 == tokens.len();
        if stray(k) && (k == 0 || kana(k - 1)) && (last || kana(k + 1)) && (finishing || !last) {
            for v in ['a', 'i', 'u', 'e', 'o'] {
                if native_romaji_readable(&format!("{}{v}", t.raw), false) { out.push((at + 1, v)); }
            }
        }
        at += t.raw.len();
    }
    let mut spots: Vec<usize> = out.iter().map(|&(p, _)| p).collect();
    spots.dedup();
    if spots.len() > 2 { vec![] } else { out }
}

/// 読めない子音 1 字のローマ字 (っ になる重ね kk・tch の t は読める)
fn stray_consonant(tokens: &[Token], j: usize) -> bool {
    let consonant = |c: u8| c.is_ascii_lowercase() && !is_vowel(c) && c != b'n';
    let (b, next) = (tokens[j].raw.as_bytes(), tokens.get(j + 1).map_or("", |t| t.raw.as_str()));
    tokens[j].kind == TokenKind::Romaji && b.len() == 1 && consonant(b[0])
        && next.as_bytes().first() != Some(&b[0]) && !(b[0] == b't' && next.starts_with("ch"))
}

/// 2026-11-04 (仕事 20、試験の切り替え AZK_VOWEL_IN_ENGLISH=0 で切る): 英字の区切りの中の母音の抜けも直す候補にするか
pub fn vowel_in_english_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| test_env("AZK_VOWEL_IN_ENGLISH").as_deref() != Some("0"))
}

/// 2026-11-04 (仕事 20): 英字の区切りの中の母音の抜けを直すのに要る、読みだけの AI の点数の差 (試験では AZK_VOWEL_IN_ENGLISH_MARGIN で替える)。
/// 2026-11-05 の測り (例文の打鍵、母音を 1 つ抜いた dev 1,972 件と正しく打った dev 1,972 件) で 10 を選んだ: 打鍵が正しく直る 462 (前の形 222)・
/// 違う形に直す 6・正しく打った行を変える 0 (差 8 で 577・6・2、6 で 701・8・4)。test 2,016 件で 414 (前の形 189)・5・4 (4 は前の形と同じ)
pub fn english_repair_margin() -> f32 {
    static MARGIN: OnceLock<f32> = OnceLock::new();
    *MARGIN.get_or_init(|| test_env("AZK_VOWEL_IN_ENGLISH_MARGIN").and_then(|v| v.parse().ok()).unwrap_or(10.0))
}

/// 2026-11-04 (仕事 20): 母音の抜けで、抜けた所の子音が隣の英字とつながって英字の区切りになった所 (watsh + iha = わたしは・
/// lobbym + dottara = lobby もどったら・thumbnailchottotenukikam = thumbnail ちょっと手抜きかも)。vowel_repairs は読めない子音 1 字の
/// ローマ字しか見ず、10-30 の例文の測りでは母音の抜けの 66% がこの形で直らなかった (英語の判定と母音の直しが、読めない子音の並びという同じ手がかりを取り合う)。
/// 小文字だけで英単語の表のどれにも無い英字の区切りごとに、中の子音のすぐ後ろ (次が子音か区切りの終わり) に母音を 1 字足して打ち終えたときの区切りをやり直し、
/// その区切りの字が全部読めるローマ字か表の英単語になる足し方のうち、表に無い英字の字数がいちばん少ないもの (同じ数なら全部) を 1 組にする。
/// 区切り直すのは前後のローマ字の区切りまでで、回数は 1 回の Space で 120 回まで (超えたら、そこまでの組だけ)
/// (組の中は (打った列の中の位置 (バイト。母音を入れる所), 母音))。2 字以上足さないと読めない並び (kbctl・npm) は組にならない。
/// 大文字を含む区切り (Shift を押した) と表の語 (英単語・社名・表記直しの表・覚えた語) は英語のまま。足すと表の英単語が区切りから消える足し方も採らない。
/// どれを選ぶか (選ばないか) は変換サーバーの採点で決める (composition.rs の repair_typos。差は english_repair_margin)
pub fn vowel_repairs_in_english(typed: &str) -> Vec<Vec<(usize, char)>> {
    let lower = |w: &str| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase());
    let known = |w: &str| is_word(w) || words().context.contains(w) || canonical_case(w).is_some() || japanese_context_case(w).is_some();
    // 表に無い小文字の英字と、読めない子音 1 字のローマ字の字数 (足したあと、子音がローマ字の側に 1 字で残る足し方 (tnki → t + niki) を数えるため)
    let unknown = |tokens: &[Token]| tokens.iter().enumerate()
        .filter(|&(j, t)| (t.kind == TokenKind::English && lower(&t.raw) && !known(&t.raw)) || stray_consonant(tokens, j)).map(|(_, t)| t.raw.len()).sum::<usize>();
    let english = |tokens: &[Token]| {
        let mut words: Vec<String> = tokens.iter().filter(|t| t.kind == TokenKind::English && known(&t.raw)).map(|t| t.raw.to_ascii_lowercase()).collect();
        words.sort_unstable();
        words
    };
    let tokens = segment_finishing(typed);
    let mut starts = Vec::with_capacity(tokens.len() + 1);
    let mut at = 0;
    for t in &tokens {
        starts.push(at);
        at += t.raw.len();
    }
    starts.push(at);
    let mut groups = vec![];
    // 区切り直す回数の上限 (長い英字の区切りで Space が遅くならないように)
    let mut tries = 0;
    'tokens: for (k, t) in tokens.iter().enumerate() {
        if t.kind != TokenKind::English || t.raw.len() < 2 || !lower(&t.raw) || known(&t.raw) { continue; }
        // 括弧・かぎ括弧・引用符で挟まれた英字の並び (「crista」・[cristae]) は、ことさらに打った英字の語とみて直さない
        let bytes = typed.as_bytes();
        let run_lo = (0..starts[k]).rev().take_while(|&j| bytes[j].is_ascii_alphabetic()).last().unwrap_or(starts[k]);
        let run_hi = (starts[k]..bytes.len()).find(|&j| !bytes[j].is_ascii_alphabetic()).unwrap_or(bytes.len());
        let quoted = |open: &str, close: &str| typed[..run_lo].ends_with(open) && typed[run_hi..].starts_with(close);
        if [("「", "」"), ("『", "』"), ("[", "]"), ("(", ")"), ("（", "）"), ("\"", "\""), ("'", "'")].iter().any(|(o, c)| quoted(o, c)) { continue; }
        // 区切り直すのは、この区切りと前後のローマ字の区切り (隣の英字の区切り・記号・数字の手前まで) だけ (行の全体を何十回も区切り直すと長い文で遅い)
        let mut lo = k;
        while lo > 0 && tokens[lo - 1].kind == TokenKind::Romaji && lower(&tokens[lo - 1].raw) { lo -= 1; }
        let mut hi = k + 1;
        while hi < tokens.len() && tokens[hi].kind == TokenKind::Romaji && lower(&tokens[hi].raw) { hi += 1; }
        let window = &typed[starts[lo]..starts[hi]];
        let base = segment_finishing(window);
        let (before, kept) = (unknown(&base), english(&base));
        let offset = starts[k] - starts[lo];
        let raw = t.raw.as_bytes();
        let (mut fewest, mut group) = (before, vec![]);
        for p in 1..=raw.len() {
            // 子音のすぐ後ろで、次が子音か区切りの終わり (母音の前には足さない)
            if is_vowel(raw[p - 1]) || (p < raw.len() && is_vowel(raw[p])) { continue; }
            for v in ['a', 'i', 'u', 'e', 'o'] {
                tries += 1;
                if tries > 120 { break 'tokens; }
                let mut candidate = window.to_string();
                candidate.insert(offset + p, v);
                let after = segment_finishing(&candidate);
                let left = unknown(&after);
                let mut now = english(&after);
                if left + t.raw.len() > before || left > fewest || kept.iter().any(|w| match now.iter().position(|x| x == w) { Some(i) => { now.remove(i); false } None => true }) { continue; }
                if left < fewest { (fewest, group) = (left, vec![]); }
                group.push((starts[k] + p, v));
            }
        }
        if !group.is_empty() { groups.push(group); }
    }
    groups
}

/// 2026-10-30: 母音を 1 つ多く打った所の候補 (motsuu = もつう の 2 つめの u)。打ち終えたときの区切りで、母音 1 字だけのローマ字の音のうち、
/// すぐ前がローマ字の音で母音で終わるもの (英字の語の中・直後と、並びの頭は除く)。打った列の中の位置 (バイト。その母音)。
/// どれを抜くか (抜かないか) は変換サーバーの採点で決める (composition.rs の repair_typos)
pub fn excess_vowel_spots(typed: &str) -> Vec<usize> {
    let tokens = segment_finishing(typed);
    let mut out = vec![];
    let mut at = 0;
    for (k, token) in tokens.iter().enumerate() {
        let vowel = token.kind == TokenKind::Romaji && token.raw.len() == 1 && is_vowel(token.raw.as_bytes()[0]);
        let after_kana = k > 0 && tokens[k - 1].kind == TokenKind::Romaji
            && tokens[k - 1].raw.bytes().all(|b| b.is_ascii_lowercase()) && tokens[k - 1].raw.bytes().last().is_some_and(is_vowel);
        if vowel && after_kana {
            out.push(at);
        }
        at += token.raw.len();
    }
    out
}

/// 2026-10-28: 母音の抜けを直すか (設定 input.typo_repair、試験中・初期値オフ)
pub fn typo_repair_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| !cfg!(test) && shared::AppConfig::read_or_default().input.typo_repair)
}

/// 2026-10-28: 直した所の綴り (補った母音の前の子音と、その前後 2 字ずつ。souiuktodesu の k なら iukto)。
/// Shift+BackSpace で打ったとおりに戻したら、この綴りは次から直さない (ATOK の「区切り直して変換すると学習して次から直さない」と同じ考え)。
/// at は vowel_repairs の位置 (子音の直後)
pub fn typo_repair_key(typed: &str, at: usize) -> String {
    let (start, end) = (at.saturating_sub(3), (at + 2).min(typed.len()));
    typed.get(start..end).unwrap_or(typed).to_ascii_lowercase()
}

/// 直さない綴りの置き場 (%APPDATA%\HaniWord\typo_repair_ng.txt に 1 行 1 つ。打った文章は残さず、綴りだけ)
fn typo_repair_ng_path() -> Option<std::path::PathBuf> {
    if cfg!(test) { return None; }
    Some(data_dir()?.join("typo_repair_ng.txt"))
}

pub fn typo_repair_declined(key: &str) -> bool {
    typo_repair_ng_path().and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|text| text.lines().any(|l| l.trim() == key))
}

/// Shift+BackSpace で戻した直しの綴りを覚える (同じ綴りは 1 回だけ。5,000 行を超えたら古い半分を捨てる)
pub fn decline_typo_repair(keys: &[String]) {
    let Some(path) = typo_repair_ng_path() else { return };
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    for key in keys {
        if !key.is_empty() && key.bytes().all(|b| b.is_ascii_lowercase()) && !lines.contains(&key.as_str()) { lines.push(key); }
    }
    if lines.len() > 5_000 { lines.drain(..lines.len() - 2_500); }
    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
    let _ = std::fs::write(&path, lines.join("\n") + "\n");
}

/// 2026-10-28: 子音の超過 (matttya の ttt = 抹茶)。母音か ん の後ろの同じ子音の 3 つ重ねで、後ろが母音・y・h のとき、
/// 1 つ多い子音の位置 (バイト) を返す。区切り (segment_run_raw) は 3 つ重ねを促音の打ち間違いとして読むので、英単語を取っていない
pub fn sokuon_repairs(typed: &str) -> Vec<usize> {
    let b = typed.as_bytes();
    let lower = typed.to_ascii_lowercase();
    (2..b.len().saturating_sub(2)).filter(|&i| {
        let c = b[i];
        c.is_ascii_lowercase() && !is_vowel(c) && c != b'n' && c != b'w' && b[i - 1] == c && b[i + 1] == c
            && (is_vowel(b[i - 2]) || b[i - 2] == b'n') && (is_vowel(b[i + 2]) || b[i + 2] == b'y' || b[i + 2] == b'h')
            && !english_then_sokuon(&lower, i - 1)
    }).collect()
}

/// 2026-10-29: 子音の 3 つ重ねのうち、英単語 + 促音 (chat + って・git + った・shop + っぽい) の形。1 つめの子音 (first) で終わる表の英単語があり、
/// 3 つめの子音のあとが、英単語の後ろによく来る って・った・っと・っぽ のとき。打ち間違い (matttya = 抹茶) とみないで、英単語の読めない字のしるしとして残す
/// (chattteiu が ちゃっってい・7909d52 では ちゃって になっていた)
fn english_then_sokuon(lower: &str, first: usize) -> bool {
    let b = lower.as_bytes();
    let after = (b.get(first + 2).copied(), b.get(first + 3).copied());
    let common = matches!(after, (Some(b't'), Some(b'e' | b'a' | b'o')) | (Some(b'p'), Some(b'o')));
    // 英単語は 3 字以上で、並びの頭か母音・ん の後ろから始まるもの (yattteru の at・itttann の it のような語の途中・2 字の語は数えない)
    common && (first.saturating_sub(15)..first.saturating_sub(1)).any(|k| (k == 0 || is_vowel(b[k - 1]) || b[k - 1] == b'n')
        && is_lexicon_word(&lower[k..first + 1]))
}

#[cfg(test)]
pub fn set_test_words(list: &[&str]) {
    let mut set: HashSet<String> = list.iter().map(|s| s.to_string()).collect();
    add_unit_words(&mut set);
    let mut sorted: Vec<String> = set.iter().cloned().collect();
    sorted.sort();
    let priority = set.iter().filter(|w| w.len() >= 5 && *w != "sushi").cloned().collect();
    // anki は利用者専用の語 (english_terms_personal.txt) として扱う (訓令式の規則の例外にしない)
    let personal = ["anki".to_string()].into_iter().collect();
    // ken + n + 母音 は、ん をまたぐ け・ん・い (権威) が日本語の語の頭としてよく出るので、表にあっても英語と読まない例 (n_straddles_japanese_head)
    let n_split = [("main".to_string(), "aiueo".to_string()), ("ken".to_string(), "aiueo".to_string())].into_iter().collect();
    // bot は文脈用の表にしか無い語として扱う (context_only_word)
    let context: HashSet<String> = ["bot".to_string()].into_iter().collect();
    let context_sorted = context.iter().cloned().collect();
    // ableton は社名・製品名の表にしか無い語として扱う (判定の語には入れ、語の頭の表 sorted には入れない。本物の表の読み込みと同じ)
    let brands: HashSet<String> = ["ableton".to_string()].into_iter().collect();
    let mut set = set;
    set.extend(brands.iter().cloned());
    // release・commit・delete・save はサ変の形のすぐ前なら英字にまとめる語として扱う (english_suru_words.txt。判定の語の表には入れない)
    let suru: HashSet<String> = ["release", "commit", "delete", "save"].into_iter().map(str::to_string).collect();
    let _ = WORDS.set(Words { set, sorted, priority, short_priority: HashSet::new(), boundary: HashSet::new(), edge_priority: HashSet::new(), personal, n_split, latin: HashSet::new(), context, context_sorted, brands, suru });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-03 (仕事 11): 英単語のすぐ後ろの読みの頭。たげ (日本語の Wikipedia で英字の語のすぐ後ろに 2 回)・たちお (3 回) は来にくい、
    /// では・して・の・から・ばん は来やすい。打ちかけで 2 音そろわないときは来にくいとしない
    #[test]
    fn rare_kana_after_english() {
        assert_eq!(kana_morae("tagede", 0, 3), vec!["た", "げ", "で"]);
        assert_eq!(kana_morae("kyoutto", 0, 3), vec!["きょ", "う", "っ"]);
        assert_eq!(kana_morae("shinbun", 0, 3), vec!["し", "ん", "ぶ"]);
        assert_eq!(kana_morae("powerstage", 6, 3), vec!["た", "げ"]);
        assert_eq!(kana_morae("tan", 0, 3), vec!["た"]);
        // 表は組み込んだ文字のまま二分探索で引く (頭の # の行は飛ばす。無い並びは 0)
        let table = "# 見出し\n# 2 行目\nあい\t3\nかき\t12\nかきく\t5\nさ\t40\n";
        for (key, count) in [("あい", 3), ("かき", 12), ("かきく", 5), ("さ", 40), ("あ", 0), ("かく", 0), ("ん", 0), ("", 0)] {
            assert_eq!(sorted_table_count(table, key), count, "{key}");
        }
        assert_eq!(sorted_table_count("# だけ\n", "あ"), 0);
        let floors = AFTER_LATIN_FLOORS;
        for rare in ["tage", "tagede", "tation"] {
            assert!(rare_after_latin(rare, 0, floors), "{rare}");
        }
        for common in ["deha", "shite", "nosettei", "kara", "bannga", "ta", "tati"] {
            assert!(!rare_after_latin(common, 0, floors), "{common}");
        }
    }

    /// 2026-11-04 (仕事 11): 英単語 + 来にくい かな (powers + たげ) は、表の英単語 2 つにちょうど分けられるときだけ分ける
    #[test]
    fn rare_kana_after_english_splits_into_two_words() {
        let table = ["power", "powers", "stage", "sound", "sounds", "station", "plan", "patch", "kite"];
        let tok = |raw: &str, english: bool| Token { raw: raw.into(), kind: if english { TokenKind::English } else { TokenKind::Romaji } };
        let show = |tokens: Vec<Token>| tokens.iter().map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect::<Vec<_>>().join("|");
        let split = |run: &str, tokens: Vec<Token>| {
            let lower = run.to_ascii_lowercase();
            show(split_rare_after_english(run, tokens, AFTER_LATIN_FLOORS, |w| table.contains(&w),
                |e| ["de", "no", "wo"].iter().any(|p| lower[e..].starts_with(*p))))
        };
        assert_eq!(split("powerstage", vec![tok("powers", true), tok("ta", false), tok("ge", false)]), "[power]|[stage]");
        assert_eq!(split("soundstagede", vec![tok("sounds", true), tok("ta", false), tok("ge", false), tok("de", false)]), "[sound]|[stage]|de");
        // たちお は日本語の語の頭として 99 回 (立ち往生など) なので分けない (語の頭の線 40)
        assert_eq!(split("powerstation", vec![tok("powers", true), tok("ta", false), tok("ti", false), tok("o", false), tok("n", false)]), "[powers]|ta|ti|o|n");
        // 英単語のあとに助詞なしで続けた和文 (patch + 来てから・きてか は語の頭として 44 回) は分けない
        assert_eq!(split("patchkitekara", vec![tok("patch", true), tok("ki", false), tok("te", false), tok("ka", false), tok("ra", false)]), "[patch]|ki|te|ka|ra");
        // 英単語 2 つに分けられなければ何もしない (plan + まだき…)。大文字を交ぜて打った語もそのまま
        assert_eq!(split("planmadaki", vec![tok("plan", true), tok("ma", false), tok("da", false), tok("ki", false)]), "[plan]|ma|da|ki");
        assert_eq!(split("Powerstage", vec![tok("Powers", true), tok("ta", false), tok("ge", false)]), "[Powers]|ta|ge");
        // 来やすい かな (stage + で) は見ない
        assert_eq!(split("stagede", vec![tok("stage", true), tok("de", false)]), "[stage]|de");
    }

    #[test]
    fn typing_backspace_latency_probe() {
        let Ok(directory) = std::env::var("AZK_BACKSPACE_LATENCY_DIR") else { return };
        let root = std::path::PathBuf::from(directory);
        assert!(root.join("isolated_latency_fixture.json").is_file());
        let appdata = std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap());
        assert_eq!(appdata.canonicalize().unwrap(), root.join("appdata").canonicalize().unwrap());
        let words = std::path::PathBuf::from(std::env::var_os("AZK_WORDS_DIR").unwrap());
        assert_eq!(words.canonicalize().unwrap(), root.join("words").canonicalize().unwrap());
        let fixtures: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("cases.json")).unwrap()).unwrap();
        let mut rows = Vec::new();
        for case in fixtures["cases"].as_array().unwrap() {
            let raw = case["raw"].as_str().unwrap();
            let mut typed = String::new();
            for character in raw.chars() {
                typed.push(character);
                LEARNED_REFRESH_QUERIES.set(0);
                let start = std::time::Instant::now();
                let tokens = segment(&typed);
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                assert_eq!(tokens.iter().map(|t| t.raw.as_str()).collect::<String>(), typed);
                rows.push(serde_json::json!({"case":case["id"],"phase":"typing", "length":typed.len(),
                    "milliseconds":elapsed, "learned_refresh_queries":LEARNED_REFRESH_QUERIES.get(),
                    "english_active":has_english(&tokens)}));
            }
            for _ in 0..12 {
                if typed.is_empty() { break; }
                LEARNED_REFRESH_QUERIES.set(0);
                let start = std::time::Instant::now();
                let tokens = segment(&typed);
                let english = has_english(&tokens);
                if let Some(last) = tokens.last() {
                    let remove = if english && last.kind == TokenKind::English {
                        last.raw.chars().last().unwrap().len_utf8()
                    } else { last.raw.len() };
                    typed.truncate(typed.len() - remove);
                }
                if english {
                    let next = segment(&typed);
                    let _ = runs(&next);
                }
                rows.push(serde_json::json!({"case":case["id"],"phase":"backspace", "length":typed.len(),
                    "milliseconds":start.elapsed().as_secs_f64()*1000.0,
                    "learned_refresh_queries":LEARNED_REFRESH_QUERIES.get(),
                    "english_active":english}));
            }
        }
        let out = root.join("classifier.json");
        assert!(!out.exists());
        std::fs::write(out, serde_json::to_vec_pretty(&serde_json::json!({"status":"measured", "rows":rows,
            "scope":"Current source classifier and Backspace token decisions; excludes RPC/TSF/display",
            "installed_client_measured":false,"production_pipe_used":false,"user_data_used":false})).unwrap()).unwrap();
    }

    #[test]
    fn classification_reuses_one_learning_snapshot_and_releases_it() {
        assert!(SEGMENT_LEARNING.with_borrow(Option::is_none));
        LEARNED_REFRESH_QUERIES.set(0);
        segment("mikakuteityuunoBackspacemokakuninnsimasu");
        assert_eq!(LEARNED_REFRESH_QUERIES.get(), 1);
        assert!(SEGMENT_LEARNING.with_borrow(Option::is_none));
        segment("gyakuyunyuugaga");
        assert_eq!(LEARNED_REFRESH_QUERIES.get(), 2);
    }

    #[test]
    fn learning_snapshot_nesting_and_unwinding_release_the_owner() {
        LEARNED_REFRESH_QUERIES.set(0);
        let result = std::panic::catch_unwind(|| {
            let _outer = SegmentLearning::new();
            {
                let _nested = SegmentLearning::new();
                assert_eq!(LEARNED_REFRESH_QUERIES.get(), 1);
            }
            assert!(SEGMENT_LEARNING.with_borrow(Option::is_some));
            panic!("synthetic snapshot unwind");
        });
        assert!(result.is_err());
        assert!(SEGMENT_LEARNING.with_borrow(Option::is_none));
    }

    #[test]
    fn learning_cache_does_not_reuse_data_for_a_denied_or_different_path() {
        let timestamp = Some(std::time::UNIX_EPOCH);
        let path = Some(std::path::PathBuf::from("synthetic-learning.txt"));
        let cache = Learned { path: path.clone(), modified: timestamp, ..Default::default() };
        assert!(cache.matches(&path, timestamp));
        assert!(!cache.matches(&None, timestamp));
        assert!(!cache.matches(&Some(std::path::PathBuf::from("another-fixture.txt")), timestamp));
        assert!(!cache.matches(&path, None));
    }

    #[test]
    fn literal_suffixes_distinguish_particles_from_english() {
        for suffix in ["de", "ni", "no", "wo", "nisite", "wotukau", "tokumiawaseru"] {
            assert!(japanese_particle_suffix(suffix), "{suffix}");
        }
        for suffix in ["", "d", "r", "word", "download", "network", " no", ".", "in", "and"] {
            assert!(!japanese_particle_suffix(suffix), "{suffix}");
        }
    }

    #[test]
    fn literal_notation_does_not_capture_a_partial_english_component() {
        for raw in ["lo-fidelity", "wi-finish", "koreha.iidesu", "nihongo.configdehanai"] {
            assert_eq!(literal_notation_span(raw, STANDARD), None, "{raw}");
        }
        if is_word("finance") || words().context.contains("finance") {
            assert_eq!(literal_notation_span("hi-finance", STANDARD), None);
        }
        assert_eq!(literal_notation_span("B.o.Bni", STANDARD), Some((0, 5)));
        assert_eq!(literal_notation_span("korehaB.o.B", STANDARD), Some((6, 11)));
    }

    #[test]
    fn mixed_language_usage_probe() {
        let (Ok(input), Ok(output)) = (std::env::var("AZK_MIXED_CASES"), std::env::var("AZK_MIXED_OUT")) else { return };
        let fixture: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(input).unwrap()).unwrap();
        let mut cases = fixture["cases"].as_array().unwrap().clone();
        for word in fixture["it_terms"].as_array().unwrap() {
            let name = word.as_str().unwrap();
            for spelling in [name.to_string(), name.to_ascii_lowercase()] {
                for template in fixture["it_templates"].as_array().unwrap() {
                    let template = template.as_str().unwrap();
                    cases.push(serde_json::json!({"family":"it_usage", "raw":template.replace("{word}", &spelling),
                        "expected":template.replace("{word}", &format!("[{spelling}]"))}));
                }
            }
        }
        let annotate = |tokens: &[Token]| {
            let mut text = String::new();
            let mut latin = false;
            for token in tokens {
                let next = token.kind == TokenKind::English;
                if latin != next { text.push(if next { '[' } else { ']' }); }
                text += &token.raw;
                latin = next;
            }
            if latin { text.push(']'); }
            text
        };
        let mut rows = Vec::new();
        for case in cases {
            let raw = case["raw"].as_str().unwrap();
            let expected = case["expected"].as_str().unwrap();
            assert_eq!(expected.replace(['[', ']'], ""), raw);
            for style in [STANDARD, Style { kunrei_nn: true, adaptive: None }] {
                for context in [false, true] {
                    set_english_sentence(false);
                    if context { note_sentence_commit(true, true, true); }
                    for finishing in [false, true] {
                        FINISHING.set(finishing);
                        let tokens = segment_with(raw, style);
                        let shown = annotate(&tokens);
                        let mut reading: String = runs(&tokens).iter().map(|(text, kind)| if *kind == TokenKind::English {
                            text.clone()
                        } else { super::super::romaji_style::simulate(&super::super::full_width::to_fullwidth(text, false))
                            .iter().map(|unit| unit.kana.as_str()).collect() }).collect();
                        if finishing && reading.ends_with('n') && tokens.last().is_some_and(|t| t.kind == TokenKind::Romaji) {
                            reading.pop();
                            reading.push('\u{3093}');
                        }
                        let operations: Vec<_> = runs(&tokens).iter().map(|(text, kind)| serde_json::json!([
                            if *kind == TokenKind::English { text.clone() } else { super::super::full_width::to_fullwidth(text, false) },
                            *kind == TokenKind::English])).collect();
                        rows.push(serde_json::json!({"family":case["family"], "raw":raw, "expected":expected,
                            "style":if style.kunrei_nn {"kunrei_nn"} else {"standard"}, "context":context,
                            "finishing":finishing, "shown":shown, "reading":reading, "pass":shown == expected,
                            "operations":operations,
                            "diagnostic_only":case["diagnostic_only"].as_bool().unwrap_or(false)
                                || (case["nn_finishing_ambiguous"] == true && style.kunrei_nn && finishing)}));
                    }
                    FINISHING.set(false);
                    if case["stable_prefix"].as_bool().unwrap_or(false) {
                        for (position, ch) in raw.char_indices() {
                            let prefix = &raw[..position + ch.len_utf8()];
                            let shown = annotate(&segment_with(prefix, style));
                            rows.push(serde_json::json!({"family":"japanese_prefix", "raw":prefix, "word":raw,
                                "style":if style.kunrei_nn {"kunrei_nn"} else {"standard"}, "context":context,
                                "finishing":false, "expected":prefix, "shown":shown, "pass":shown == prefix,
                                "diagnostic_only":context}));
                        }
                    }
                }
            }
        }
        FINISHING.set(false);
        set_english_sentence(false);
        let failed = rows.iter().filter(|row| row["pass"] == false && row["diagnostic_only"] != true).count();
        std::fs::write(output, serde_json::to_string_pretty(&serde_json::json!({"rows":rows, "failed":failed})).unwrap()).unwrap();
        if std::env::var_os("AZK_MIXED_CHECK").is_some() { assert_eq!(failed, 0, "See mixed language report"); }
    }

    /// 2026-10-15: ローマ字の判定をまとめて見る (AZK_ROMAJI_PROBE=入力ファイル・AZK_ROMAJI_PROBE_OUT=出力ファイルのときだけ)。
    /// 1 行 = 打った文字列<TAB>打っている途中 (ふつう)<TAB>Space・Enter の時点<TAB>同じく nn と打つ人<TAB>英語の文脈で打っている途中<TAB>英語の文脈で Space・Enter
    #[test]
    fn romaji_judgement_probe() {
        let (Ok(input), Ok(output)) = (std::env::var("AZK_ROMAJI_PROBE"), std::env::var("AZK_ROMAJI_PROBE_OUT")) else { return };
        let mut out = String::new();
        set_english_sentence(false);
        for raw in std::fs::read_to_string(input).unwrap().lines().filter(|l| !l.is_empty()) {
            let typing = show_with(raw, STANDARD);
            FINISHING.set(true);
            let finishing = show_with(raw, STANDARD);
            let finishing_nn = show_with(raw, Style { kunrei_nn: true, adaptive: None });
            FINISHING.set(false);
            // 英語の文脈 (直前に英字を確定した) のとき
            set_english_sentence(true);
            let typing_context = show_with(raw, STANDARD);
            FINISHING.set(true);
            let finishing_context = show_with(raw, STANDARD);
            FINISHING.set(false);
            // 訓令式・nn の人の英語の文脈
            let kunrei = Style { kunrei_nn: true, adaptive: None };
            let typing_context_nn = show_with(raw, kunrei);
            FINISHING.set(true);
            let finishing_context_nn = show_with(raw, kunrei);
            FINISHING.set(false);
            set_english_sentence(false);
            // 設定の打ち方 (AZK_ROMAJI_STYLE。公開版の初期値は auto) で Space を押した時点
            FINISHING.set(true);
            let finishing_configured = show_with(raw, style());
            FINISHING.set(false);
            out += &format!("{raw}\t{typing}\t{finishing}\t{finishing_nn}\t{typing_context}\t{finishing_context}\t{typing_context_nn}\t{finishing_context_nn}\t{finishing_configured}\n");
        }
        std::fs::write(output, out).unwrap();
    }

    #[test]
    fn small_kana_dictionary_probe() {
        let Ok(output) = std::env::var("AZK_SMALL_KANA_PROBE") else { return };
        let mut rows = Vec::new();
        for style in [STANDARD, Style { kunrei_nn: true, adaptive: None }] {
            for raw in ["sekyuriteximennde", "sekyuritelimennde", "sekyurithimennde", "texisyatu", "dexisupurei",
                        "sekyuritexisisutemu", "sekyuritelisisutemu", "fuxairu", "fulairu", "vuxiniru", "vuliniru", "texime", "TeXIME",
                        "korehataxide", "text", "file", "indexing", "axial", "github", "supermario"] {
                rows.push(serde_json::json!({"style": if style.kunrei_nn { "kunrei_nn" } else { "standard" },
                    "raw": raw, "shown": show_with(raw, style),
                    "kana": super::super::romaji_style::simulate(raw).iter().map(|unit| unit.kana.as_str()).collect::<String>()}));
            }
        }
        std::fs::write(output, serde_json::to_string_pretty(&rows).unwrap()).unwrap();
    }

    #[test]
    fn command_names_keep_their_final_consonant_before_particles() {
        if std::env::var_os("AZK_COMMANDS_CHECK").is_none() { return; }
        for style in [STANDARD, Style { kunrei_nn: true, adaptive: None }] {
            set_english_sentence(false);
            for command in ["pwd", "cwd", "gdb", "gcc", "awk", "sed", "grep", "chmod", "chown", "mkdir", "rmdir", "ssh", "npm"] {
                assert!(!learned_english(command), "Regression must not depend on learning: {command}");
                for (prefix, suffix) in [("", "wo"), ("", "wonyuuryoku"), ("", "de"), ("", "ni"), ("mazu", "wotukau")] {
                    let raw = format!("{prefix}{command}{suffix}");
                    assert_eq!(show_with(&raw, style), format!("{prefix}[{command}]{suffix}"), "{raw}");
                }
            }
            for raw in ["dwo", "dwonn", "dwu", "sedai", "sedainosa", "gitaa", "mazuhakakunin"] {
                assert_eq!(show_with(raw, style), raw, "Japanese must survive: {raw}");
            }
            assert_eq!(show_with("issh", style), "issh");
            assert_eq!(show_with("isshoni", style), "isshoni");
            assert_eq!(show_with("hassh", style), "hassh");
            assert_eq!(show_with("hasshin", style), "hasshin");
            FINISHING.set(true);
            assert_eq!(show_with("ssh", style), "[ssh]");
            FINISHING.set(false);
        }
    }

    #[test]
    fn uncommon_combined_kana_yields_to_words_but_explicit_small_kana_survives() {
        if std::env::var_os("AZK_COMMANDS_CHECK").is_none() { return; }
        for keys in UNCOMMON_SMALL_KANA {
            assert!(super::super::romaji_table::ROMAJI_TABLE.iter().any(|(input, _)| input == keys), "{keys}");
        }
        for style in [STANDARD, Style { kunrei_nn: true, adaptive: None }] {
            set_english_sentence(false);
            for word in ["two", "the", "sweat", "switch", "dwell", "dwarf", "twice", "who", "deli"] {
                assert_eq!(show_with(word, style), format!("[{word}]"), "{word}");
            }
            for raw in ["doxo", "dolo", "toxo", "tolo", "texi", "teli", "dexi", "delisupurei", "suxa", "sula",
                        "sha", "cha", "fa", "dwo", "dwonn", "dwuxa", "dwula"] {
                assert_eq!(show_with(raw, style), raw, "{raw}");
            }
        }
    }

    #[test]
    fn native_suffix_evidence_matches_forward_readability() {
        for raw in ["", "iretai", "windowsproiretahougaii", "hiraiterubyuu", "nodexi", "ravi", "killing",
            "sy", "n", "nn", "kanpai", "kitte", "kotchi", "Google", "xyz", "v"] {
            for pending in [false, true] {
                let suffixes = native_romaji_suffixes(raw, pending);
                for index in 0..=raw.len() {
                    assert_eq!(suffixes[index], native_romaji_readable(&raw[index..], pending), "{raw}, {index}, {pending}");
                }
            }
        }
    }

    #[test]
    fn mixed_loanword_boundaries_regression() {
        if std::env::var_os("AZK_MIXED_LOANWORD_CHECK").is_none() { return; }
        let mut failures = Vec::new();
        for style in [STANDARD, Style { kunrei_nn: true, adaptive: None }] {
            for context in [0, 6] {
                SENTENCE.set(context);
                for finishing in [false, true] {
                    FINISHING.set(finishing);
                    for (raw, expected) in [
                        ("wikipedia", "[wikipedia]"),
                        ("wikimedia", "[wikimedia]"),
                        ("azookey", "[azookey]"),
                        ("azookeywotukau", "[azookey]wotukau"),
                        ("wikipediawoyomu", "[wikipedia]woyomu"),
                        ("wikimedianokiji", "[wikimedia]nokiji"),
                        ("toyotanodexi-ra-", "toyotanodexi-ra-"),
                        ("toyotanodeli-ra-", "toyotanodeli-ra-"),
                        ("nodexisupurei", "nodexisupurei"),
                        ("nodexi-ra-", "nodexi-ra-"),
                        ("nodeli-ra-", "nodeli-ra-"),
                        ("do-ravi-ra-", "do-ravi-ra-"),
                        ("windowsproiretahougaii", "[windows][pro]iretahougaii"),
                        ("githubiretai", "[github]iretai"),
                        ("googleerabu", "[google]erabu"),
                        ("microsoft", "[microsoft]"),
                        ("video", "[video]"),
                        ("indexing", "[indexing]"),
                        ("node.js", "[node.js]"),
                        ("deli", "[deli]"),
                        ("hanadi", "hanadi"),
                        ("tidimu", "tidimu"),
                        ("korehataxide", "koreha[taxi]de"),
                        ("hiraiterubyuu", "hiraiterubyuu"),
                    ] {
                        let actual = show_with(raw, style);
                        if actual != expected { failures.push(format!("{raw}: {actual} != {expected}, kunrei={} context={context} finishing={finishing}", style.kunrei_nn)); }
                    }
                    if context > 0 {
                        for raw in ["home-game", "state-of-the-art", "wikipedia-based", "node", "my", "in", "killing", "ire"] {
                            assert_eq!(show_with(raw, style), format!("[{raw}]"), "{raw}");
                        }
                    }
                }
            }
        }
        SENTENCE.set(0);
        FINISHING.set(false);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn explicit_small_kana_units_have_indivisible_english_boundaries() {
        for raw in ["sekyuritexime", "sekyuritelime", "dexisupurei", "fuxairu", "fulairu", "vuxiniru", "vuliniru"] {
            let (joined, protected) = small_kana_boundaries(raw);
            assert!(joined.iter().any(|flag| *flag), "{raw}");
            assert!(protected.iter().any(|flag| *flag), "{raw}");
        }
        for raw in ["taxi", "text", "TeXIME", "TeX", "lime", "axial"] {
            assert!(small_kana_boundaries(raw).0.iter().all(|flag| !flag), "{raw}");
        }
    }

    #[test]
    fn automatic_adjacent_english_tokens_do_not_become_manual_learning() {
        let english = |raw: &str| Token { raw: raw.into(), kind: TokenKind::English };
        let kana = |raw: &str| Token { raw: raw.into(), kind: TokenKind::Romaji };
        assert!(automatically_detected_english(&[english("tex"), english("ime")], "texime"));
        assert!(automatically_detected_english(&[kana("koreha"), english("TeX"), english("IME"), kana("de")], "texime"));
        assert!(!automatically_detected_english(&[english("tex"), kana("i"), english("me")], "texime"));
        assert!(!automatically_detected_english(&[english("tex"), english(" "), english("ime")], "texime"));
        assert!(!automatically_detected_english(&[english("tex"), english("ime")], "tex"));
    }

    #[test]
    fn small_kana_dictionary_regressions() {
        if std::env::var_os("AZK_SMALL_KANA_CHECK").is_none() { return; }
        for style in [STANDARD, Style { kunrei_nn: true, adaptive: None }] {
            for raw in ["sekyuriteximennde", "sekyuritelimennde", "sekyurithimennde", "texisyatu", "telisyatu",
                        "dexisupurei", "delisupurei", "fuxairu", "fulairu", "vuxiniru", "vuliniru",
                        "sekyuritexisisutemu", "sekyuritelisisutemu"] {
                for context in [false, true] {
                    set_english_sentence(context);
                    for finishing in [false, true] {
                        FINISHING.set(finishing);
                        assert_eq!(show_with(raw, style), raw, "{raw} finishing={finishing} context={context}");
                    }
                    FINISHING.set(false);
                    if let Some(start) = raw.find("texi").or_else(|| raw.find("teli")) {
                        for end in start + 4..=raw.len() {
                            assert_eq!(show_with(&raw[..end], style), raw[..end], "{raw} prefix={end} context={context}");
                        }
                    }
                }
            }
            set_english_sentence(false);
            for raw in ["text", "file", "indexing", "github", "supermario"] {
                assert_eq!(show_with(raw, style), format!("[{raw}]"));
            }
            assert_eq!(show_with("korehataxide", style), "koreha[taxi]de");
            assert!(segment_with("TeXIME", style).iter().all(|token| token.kind == TokenKind::English));
            // 2026-10-15: 同じ母音を小さい文字で伸ばす形 (えぇ・まぁ・さぁ・ねぇ) は日本語。英単語の表にある語 (gala・solo・delete) は英語のまま
            for raw in ["exe", "ele", "mala", "malane", "salate", "sala", "inele", "urusexe", "nexe", "hexe", "malamala"] {
                for finishing in [false, true] {
                    FINISHING.set(finishing);
                    assert_eq!(show_with(raw, style), raw, "{raw} finishing={finishing}");
                }
                FINISHING.set(false);
            }
            for raw in ["gala", "solo", "delete", "select", "balance", "element", "hole"] {
                assert_eq!(show_with(raw, style), format!("[{raw}]"), "{raw}");
            }
            // 2026-10-15: 元素記号を助詞でつなぐ形と、大文字 1 字＋助詞＋日本語
            for (raw, expected) in [("HtoO", "[H]to[O]"), ("NatoCl", "[Na]to[Cl]"), ("FetoCu", "[Fe]to[Cu]"), ("HmoO", "[H]mo[O]"),
                                    ("HtoOnohannou", "[H]to[O]nohannou"), ("HtoOdeha", "[H]to[O]deha"), ("MotoGP", "[Moto][GP]"),
                                    ("Nagasakini", "[Nagasaki]ni"), ("Codeni", "[Code]ni"), ("Segade", "[Sega]de")] {
                assert_eq!(show_with(raw, style), expected, "{raw}");
            }
        }
    }

    #[test]
    fn context_dictionary_parser_filters_comments_and_invalid_lexemes() {
        let parsed = parse_context_words("# comment\nBrowser\nbrowser\ncan't\nself-hosted\n12\n--\nwrong123\nniño\n");
        assert_eq!(parsed, ["browser", "can't", "self-hosted"].into_iter().map(str::to_string).collect());
    }

    #[test]
    fn sentence_japanese_guard_includes_small_kana_and_unfinished_syllables() {
        for word in ["texisyatu", "telyuubunn", "vuxiniru", "gyakuyunyuugo", "sinnbunn", "kyouha", "kotchi", "metcha"] {
            assert!(sentence_romaji_readable(word, false), "{word}");
        }
        assert!(sentence_romaji_readable("texisyat", true));
        assert!(!sentence_romaji_readable("texisyat", false));
        assert!(!sentence_romaji_readable("browser", false));
        assert!(!sentence_romaji_readable("zxqw", true));
    }

    #[test]
    fn japanese_returns_after_strong_english_context() {
        let (Ok(_), Ok(path), Ok(out)) = (std::env::var("AZK_WORDS_DIR"), std::env::var("AZK_ROMAJI_TEST"), std::env::var("AZK_CONTEXT_JAPANESE_OUT")) else { return };
        let mut changed = vec![];
        let text = std::fs::read_to_string(path).unwrap();
        for (index, raw) in text.lines().enumerate() {
            set_english_sentence(false);
            let neutral = segment_finishing(raw);
            note_sentence_commit(true, true, true);
            note_sentence_commit(true, true, true);
            let contextual = segment_finishing(raw);
            if contextual != neutral { changed.push(format!("line={} input={raw} neutral={neutral:?} contextual={contextual:?}", index + 1)); }
        }
        set_english_sentence(false);
        std::fs::write(out, format!("lines={} context_changes={}\n{}", text.lines().count(), changed.len(), changed.join("\n"))).unwrap();
        assert!(changed.is_empty(), "Japanese return regressions: {}", changed.join(" / "));
    }

    #[test]
    fn selected_language_words_do_not_replace_readable_japanese() {
        for word in ["vono", "vino", "von", "netto", "desayuno"] { assert!(native_romaji_readable(word, false), "{word}"); }
        for word in ["leche", "queso", "milch", "ekmek", "bonjour"] { assert!(!native_romaji_readable(word, false), "{word}"); }
        assert!(native_romaji_readable("bonjour", true));
    }

    #[test]
    fn latin_dictionaries_are_selected_not_globally_enabled() {
        let dir = std::env::temp_dir().join(format!("azk-latin-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(dir.join("latin_words")).unwrap();
        std::fs::write(dir.join("latin_words/es.txt"), "# comment\nLeche\nque\ny\nniño\nwrong123\n").unwrap();
        std::fs::write(dir.join("latin_words/fr.txt"), "bonjour\n").unwrap();
        let mut set = HashSet::new();
        add_latin_words(&mut set, &dir, &[]);
        assert!(set.is_empty());
        add_latin_words(&mut set, &dir, &["es".into(), "../fr".into(), "unknown".into()]);
        assert_eq!(set, ["leche", "que", "y", "niño"].into_iter().map(str::to_string).collect());
        add_latin_words(&mut set, &dir, &["fr".into()]);
        assert!(set.contains("bonjour"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn latin_word_merging_respects_japanese_boundaries() {
        let english = |raw: &str| Token { raw: raw.into(), kind: TokenKind::English };
        let kana = |raw: &str| Token { raw: raw.into(), kind: TokenKind::Romaji };
        let known = |word: &str| word == "queso";
        assert_eq!(merge_latin_words(vec![english("que"), kana("so"), kana("wo")], known), vec![english("queso"), kana("wo")]);
        assert_eq!(merge_latin_words(vec![kana("ko"), kana("re"), kana("ha"), english("que"), kana("so")], known), vec![kana("ko"), kana("re"), kana("ha"), english("queso")]);
        let all_kana = vec![kana("que"), kana("so")];
        assert_eq!(merge_latin_words(all_kana.clone(), known), all_kana);
        let pending_word = vec![kana("bo"), kana("n"), kana("jou"), kana("r")];
        assert_eq!(merge_latin_words(pending_word.clone(), |w| w == "bonjour"), pending_word);
        FINISHING.set(true);
        let finished = merge_latin_words(pending_word, |w| w == "bonjour");
        FINISHING.set(false);
        assert_eq!(finished, vec![english("bonjour")]);
        let geminate = vec![kana("ne"), kana("t"), kana("to")];
        assert_eq!(merge_latin_words(geminate.clone(), |w| w == "net"), geminate);
        let inside_word = vec![kana("ka"), english("que"), kana("so"), kana("ru")];
        assert_eq!(merge_latin_words(inside_word.clone(), known), inside_word);
    }

    /// 2026-10-28: 母音の抜け (souiuktodesu の k) を直す候補。英字にした語・読めない字の並び・打ちかけの末尾は直さない
    #[test]
    fn vowel_repairs_skip_english_and_clusters() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        let spots = |raw: &str, finishing: bool| vowel_repairs(raw, finishing).iter().map(|&(p, v)| format!("{p}{v}")).collect::<Vec<_>>().join(" ");
        assert_eq!(spots("souiuktodesu", true), "6a 6i 6u 6e 6o");
        // 打っている途中の末尾は打ちかけ。打ち終えたら直す候補
        assert_eq!(spots("souiuk", false), "");
        // っ・ん・tch は読める
        for raw in ["mattya", "kanji", "kotchi", "sinnbunn"] { assert_eq!(spots(raw, true), "", "{raw}"); }
        // 英字にした語は直さない: 知っている語 (test)・文脈用の表の語 (bot)・語の途中に埋まった語 (tori + [shop] + ri)
        assert_eq!(spots("korehatestdesu", true), "");
        assert_eq!(spots("kinoutestwokatta", true), "");
        assert_eq!(spots("botde", true), "");
        assert_eq!(show("torishopri"), "tori[shop]ri");
        assert_eq!(spots("torishopri", true), "");
        // 読めない字の並び (略語・名前) は直さない
        assert_eq!(spots("dmmde", true), "");
        // 子音の超過: 3 つ重ねは促音の打ち間違いとして読み (shop + ppu にしない)、1 つ多い子音を返す
        assert_eq!(show("shopppu"), "shopppu");
        assert_eq!(show("shoppu"), "shoppu");
        assert_eq!(sokuon_repairs("shopppu"), vec![4]);
        assert_eq!(sokuon_repairs("matttya"), vec![3]);
        // 直した所の綴り (子音と前後 2 字ずつ)。試験では覚える先が無いので、直さない綴りは無い
        assert_eq!(typo_repair_key("souiuktodesu", 6), "iukto");
        assert_eq!(typo_repair_key("ktodesu", 1), "kto");
        assert!(!typo_repair_declined("iukto"));
        for raw in ["shoppu", "kitte", "www", "kusawwwa", "nnn"] { assert!(sokuon_repairs(raw).is_empty(), "{raw}"); }
    }

    /// 2026-11-04 (仕事 20): 英字の区切りになった母音の抜け (watsh + iha = watashiha) を直す候補。表の語・大文字を含む語・
    /// 1 字足しても読めない並び・足すと表の英単語が消える足し方は出さない
    #[test]
    fn vowel_repairs_inside_unknown_lowercase_english() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        let groups = |raw: &str| vowel_repairs_in_english(raw).iter()
            .map(|g| g.iter().map(|&(p, v)| format!("{p}{v}")).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>();
        let finishing = |raw: &str| { FINISHING.set(true); let shown = show(raw); FINISHING.set(false); shown };
        // t のあとに母音を足すと全部読める (watashiha・watishiha …)。どれにするかは採点で決める
        assert_eq!(finishing("watshiha"), "[wat]shiha");
        assert_eq!(groups("watshiha"), vec!["3a 3i 3u 3e 3o"]);
        // n のあとに足すと t が読めない子音 1 字のローマ字で残る (tniki) ので、t のあとだけ
        assert_eq!(groups("kyouhatnkidesu"), vec!["7a 7i 7u 7e 7o"]);
        // 表の英単語 (github) はそのままで、後ろの表に無い区切り (mad) だけ
        assert_eq!(finishing("githubmadtoretenai"), "[github][mad]toretenai");
        assert_eq!(groups("githubmadtoretenai"), vec!["9a 9i 9u 9e 9o"]);
        // 表の語・大文字を含む語・1 字足しても読めない並び・読めない子音 1 字のローマ字 (vowel_repairs が見る) は組にしない
        // 括弧で挟まれた英字の並び (「crista」・[cristae]) も直さない
        for raw in ["korehatestdesu", "Watshiha", "kbctlwotsukau", "souiuktodesu", "kono「crista」toha", "kono[cristae]toha"] {
            assert!(groups(raw).is_empty(), "{raw}: {:?}", groups(raw));
        }
    }

    /// 2026-10-31: 数学をよく打つ分野に選んだ人 (度合い 2) は、引数の形が続く関数名 (sin2x・sin^2x・tan(x)・sinxno・detA) を英字に。
    /// ん + 子音 の日本語 (しんかんせん・たんちょう・しんつう・みんな) と、手前が n 1 字の並び (nmodp) は今までどおり
    #[test]
    fn math_function_names_when_math_is_focused() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        let finishing = |raw: &str| { FINISHING.set(true); let shown = show(raw); FINISHING.set(false); shown };
        let japanese = ["sinkansenninoru", "tanchoudearu", "sintsuugasuru", "minnadeiku", "sinbunwoyomu", "tannidekaku", "kensin2kaime", "kousinsita", "nmodpnokeisan"];
        MATH_LEVEL_FOR_TEST.set(Some(0));
        let before: Vec<String> = japanese.iter().map(|raw| finishing(raw)).collect();
        let sinx_before = finishing("kansuusinxnosyuuki");
        assert_ne!(finishing("sin2x"), "[sin]2[x]");
        MATH_LEVEL_FOR_TEST.set(Some(2));
        assert_eq!(finishing("sin2x"), "[sin]2[x]");
        assert_eq!(show("sin2x"), "[sin]2[x]");
        assert_eq!(finishing("sin^2x"), "[sin]^2x");
        assert_eq!(finishing("y=tan(x)nogurafu"), "y=[tan](x)nogurafu");
        assert_eq!(finishing("kansuusinxnosyuuki"), "kansuu[sinx]nosyuuki");
        assert_eq!(finishing("sinxtocosxnosekiwomotomeru"), "[sinx]to[cosx]nosekiwomotomeru");
        assert_eq!(finishing("detA=0"), "[detA]=0");
        for (raw, kana) in japanese.iter().zip(&before) { assert_eq!(&finishing(raw), kana, "{raw}"); }
        // 単独の関数名: と・や でつなぐ関数名どうし、助詞 + 数学の語 (手前が入力の頭・助詞・数学の語のとき)
        assert_eq!(finishing("sintocosnokankei"), "[sin]to[cos]nokankei");
        assert_eq!(finishing("sinyacostotan"), "[sin]ya[cos]to[tan]");
        assert_eq!(finishing("tannokahouteiri"), "[tan]nokahouteiri");
        assert_eq!(finishing("sinnoatai"), "[sin]noatai");
        let nn = Style { kunrei_nn: true, adaptive: None };
        let finishing_nn = |raw: &str| { FINISHING.set(true); let shown = show_with(raw, nn); FINISHING.set(false); shown };
        assert_eq!(finishing("kansuusinnoatai"), "kansuu[sin]noatai");
        assert_eq!(finishing_nn("tannohugou"), "[tan]nohugou");
        assert_eq!(finishing_nn("sintotan"), "[sin]to[tan]");
        let standalone_japanese = ["sintoukaisuru", "tantousya", "tannnohanasi", "minnadeiku", "sinnnoatai", "tanniwosoroeru", "sinnou",
            "tyuusinnoteigi", "tyuusintonaru", "sindendukuri", "zensinnokinniku"];
        MATH_LEVEL_FOR_TEST.set(Some(0));
        let standalone_before: Vec<(String, String)> = standalone_japanese.iter().map(|raw| (finishing(raw), finishing_nn(raw))).collect();
        MATH_LEVEL_FOR_TEST.set(Some(2));
        for (raw, (n, nn)) in standalone_japanese.iter().zip(&standalone_before) {
            assert_eq!(&finishing(raw), n, "{raw}");
            assert_eq!(&finishing_nn(raw), nn, "nn {raw}");
        }
        // 度合い 1 (^・(・数字 + 変数 の形だけ) では、関数名 + 変数 1 字 は今までどおり
        MATH_LEVEL_FOR_TEST.set(Some(1));
        assert_eq!(finishing("sin2x"), "[sin]2[x]");
        assert_eq!(finishing("kansuusinxnosyuuki"), sinx_before);
        MATH_LEVEL_FOR_TEST.set(None);
        // 物理を選んだ人も度合い 2 (sinωt・cosθ)
        assert_eq!(math_function_level_for(&["physics".to_string()]), 2);
        assert_eq!(math_function_level_for(&["law".to_string(), "math".to_string()]), 2);
        assert_eq!(math_function_level_for(&["chemistry".to_string()]), 1);
        assert_eq!(math_function_level_for(&[]), 1);
    }

    /// 2026-11-04 (仕事 19 の 2): 英語の固有名詞の表は、英字の語の字の形 (einstein → Einstein) と Shift で打った英字の守り (Einstein + の) だけに使い、
    /// 区切りの判定には入れない (handoverdekiru の区切りは表の有り無しで同じ)
    #[test]
    fn english_proper_names_only_for_case_and_shift() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        ENGLISH_NAMES_FOR_TEST.with(|n| *n.borrow_mut() = Some(vec![]));
        let without = show("handoverdekiru");
        set_test_english_names(&["Einstein", "Dover", "Schrodinger"]);
        assert_eq!(show("handoverdekiru"), without);
        assert_eq!(english_name_case("einstein"), Some("Einstein".to_string()));
        assert_eq!(english_name_case("Einstein"), None);
        assert_eq!(english_name_case("eins"), None);
        // Shift で打った名前 + 助詞は名前の後ろで かな に。名前の打ちかけ (Einst) は決めない
        assert_eq!(temp_ascii_return_with("Einsteinno", TEMP_ASCII_RULES), Some(8));
        assert_eq!(temp_ascii_return_with("Einst", TEMP_ASCII_RULES), None);
        ENGLISH_NAMES_FOR_TEST.with(|n| *n.borrow_mut() = None);
    }

    /// 2026-11-04 (仕事 19 の 3a): 子音の字＋語を小文字で打った所 (jri-gu = Jリーグ・bkyuu = B級) は字だけを英字にし、字の形は表の多い形に。
    /// 母音の字・数字のすぐ後ろ・字が後ろと音を作る所 (tsyatu)・線より少ない語は今のまま
    #[test]
    fn lowercase_initial_letter_words() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        set_test_initial_words(&[("jri-gu", 'J', 29325, 1.0), ("bkyuu", 'B', 4397, 0.999), ("xsenn", 'X', 5893, 1.0), ("xziku", 'x', 311, 0.6),
            ("tsyatu", 'T', 6576, 1.0), ("gmenn", 'G', 40, 1.0), ("amenn", 'A', 9646, 1.0)]);
        let finishing = |raw: &str| { FINISHING.set(true); let shown = show(raw); FINISHING.set(false); shown };
        assert_eq!(finishing("jri-gunosennshu"), "[j]ri-gunosennshu");
        assert_eq!(finishing("bkyuugurume"), "[b]kyuugurume");
        assert_eq!(finishing("kotosihaxsenndesu"), "kotosiha[x]senndesu");
        // 助詞でない かな のすぐ後ろ (yukkurixsenn)・助詞に見える綴りの英字のすぐ後ろ (cache の he)・tch の っ は見ない
        set_test_initial_words(&[("jri-gu", 'J', 29325, 1.0), ("bkyuu", 'B', 4397, 0.999), ("xsenn", 'X', 5893, 1.0), ("tchi", 'T', 500, 1.0)]);
        for raw in ["yukkurixsenndesu", "cachebkyuu", "itchiwosagasu"] {
            let shown = finishing(raw);
            assert!(!shown.contains("[x]") && !shown.contains("[b]") && !shown.contains("[t]"), "{raw}: {shown}");
        }
        assert_eq!(initial_word_case("j", "ri-gunosennshu"), Some("J"));
        assert_eq!(initial_word_case("x", "senn"), Some("X"));
        // 多い形の割合・回数が線より下の語 (xziku・gmenn)、母音の字 (amenn)、字が後ろと音を作る所 (tsyatu の ts)、数字のすぐ後ろは区切らない
        assert_eq!(initial_word_case("x", "ziku"), None);
        // nn の 2 つ目の n から語 (N + きょく) を探さない (bmennkyoku = B面 + きょく)
        set_test_initial_words(&[("jri-gu", 'J', 29325, 1.0), ("bkyuu", 'B', 4397, 0.999), ("xsenn", 'X', 5893, 1.0), ("xziku", 'x', 311, 0.6),
            ("tsyatu", 'T', 6576, 1.0), ("gmenn", 'G', 40, 1.0), ("amenn", 'A', 9646, 1.0), ("bmenn", 'B', 9510, 1.0), ("nkyoku", 'N', 150, 1.0)]);
        assert_eq!(finishing("bmennkyoku"), "[b]mennkyoku");
        for raw in ["xzikudesu", "gmenndesu", "amenndesu", "tsyatuwokau", "10bkyuu", "amennkyoku"] {
            assert!(!finishing(raw).contains(&format!("[{}]", raw.trim_start_matches(char::is_numeric).chars().next().unwrap())), "{raw}: {}", finishing(raw));
        }
        INITIAL_WORDS_FOR_TEST.with(|t| *t.borrow_mut() = None);
    }

    /// 2026-11-03 (仕事 10): 数学か物理をよく打つ分野に選んだ人は、数式を英字・半角の記号のまま入れる (式のあとのローマ字は かな)。ほかの人は今のまま
    #[test]
    fn math_expressions_stay_literal_for_math_typists() {
        MATH_LEVEL_FOR_TEST.set(Some(2));
        for (raw, expected) in [
            ("2e+1/tkana", "[2e+1/t]kana"),
            ("3x+1=0wotoku", "[3x+1=0]wotoku"),
            ("1/2wokeisansuru", "[1/2]wokeisansuru"),
            ("x=-1notoki", "[x=-1]notoki"),
            ("kotaehax+1desu", "kotaeha[x+1]desu"),
            ("(x+1)(x-1)wotenkai", "[(x+1)(x-1)]wotenkai"),
            ("x²+1/xnituite", "[x²+1/x]nituite"),
            // 演算子の無い並び・ローマ字の長音・語の間の / は数式にしない
            ("2ennkasegu", "2ennkasegu"),
            ("ra-mennwotaberu", "ra-mennwotaberu"),
            ("toukyou/oosaka", "toukyou/oosaka"),
            ("nedann+500enn", "nedann+500enn"),
            // 2026-11-04 (仕事 10b): 1 字で音になる母音のすぐ後ろが演算子なら、前が日本語でも変数。子音と組む母音 (kare の e) はかなのまま
            ("kotaehaa=3desu", "kotaeha[a=3]desu"),
            ("a=1notoki", "[a=1]notoki"),
            ("kare=3", "kare=3"),
            // 式の中と数のすぐ後ろの、読めない 2〜3 字の並びは変数のかけ算。読み切れる並び (akarui・oosaka) はかな
            ("f(x)=axtosuru", "[f(x)=ax]tosuru"),
            ("y=ax^2+bx+cnogurafu", "[y=ax^2+bx+c]nogurafu"),
            ("2ax+1=0", "[2ax+1=0]"),
            ("kotaehaax+bdesu", "kotaeha[ax+b]desu"),
            // 入力の終わりの演算子 (打っている途中の a=)。- と / は ー と ・ のことが多いので式にしない
            ("a=", "[a=]"),
            ("kotaehax=", "kotaeha[x=]"),
            ("f(x)=", "[f(x)=]"),
            ("ra-", "ra-"),
            ("toukyou/", "toukyou/"),
            ("sugoi^", "sugoi^"),
            // 綴りの頭の字 (xtu・mod) からかけ算にしない
            ("i/xtu/po/nn", "i/xtu/po/nn"),
            ("2^10mod7", "[2^10][mod]7"),
        ] {
            assert_eq!(show(raw), expected, "{raw}");
        }
        assert!(!show("x+akarui").contains("[x+a"), "{}", show("x+akarui"));
        // 入力の終わりでない並び (後ろにほかの並びが続く所) は、演算子で終わっても式にしない
        RUN_AT_END.set(false);
        assert!(!show("a=").contains("[a="), "{}", show("a="));
        RUN_AT_END.set(true);
        // 関数名の今の動きは同じ (関数名を先に見る)
        assert!(show("sin2x+1").starts_with("[sin]2[x]"), "{}", show("sin2x+1"));
        MATH_LEVEL_FOR_TEST.set(Some(1));
        assert!(!show("2e+1/tkana").contains("[2e"), "{}", show("2e+1/tkana"));
        MATH_LEVEL_FOR_TEST.set(None);
    }

    #[test]
    fn z_symbol_shortcuts_stay_romaji() {
        // 2026-11-06: z ＋ . , / - [ ] (Mozc と同じ … ‥ ・ 〜 『 』)。z も記号もローマ字の側 (変換サーバーのローマ字の表で字にする)
        let shape = |raw: &str| segment_finishing(raw).iter().map(|t| format!("{}{}", if t.kind == TokenKind::English { "E:" } else { "R:" }, t.raw)).collect::<Vec<_>>().join(" ");
        for (raw, expected) in [("z.", "R:z R:."), ("z,", "R:z R:,"), ("z/", "R:z R:/"), ("z-", "R:z R:-"), ("z[", "R:z R:["), ("z]", "R:z R:]")] {
            assert_eq!(shape(raw), expected, "{raw}");
        }
        assert!(shape("korez.").ends_with("R:z R:."), "{}", shape("korez."));
        assert!(shape("sorede z.").ends_with("R:z R:."));
        // z で始まるローマ字・英単語と、z の前がローマ字として読めない英単語 (jazz・fizz) は今までどおり
        // (quiz のように z の前が読める英単語は、本物の表では英単語の表の語なので今までどおり。試験の表には無い)
        for raw in ["zu", "zehi", "zero", "jazz.", "fizz,"] {
            assert!(!shape(raw).contains("R:z R:"), "{raw}: {}", shape(raw));
        }
        assert_eq!(shape("zh"), "R:z R:h");
    }

    /// 2026-11-05 (仕事 22 の 1): 数字・英字の語のすぐ後ろの / は半角の / (英字の並び)。かなのあと・頭の / は今までどおり ・ (ローマ字の側)。
    /// 数字のすぐ後ろの . に拡張子が続けば (IMG_0001.JPG)、. と拡張子は英字のまま
    #[test]
    fn slash_and_period_after_digits_and_latin_stay_half_width() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        for (raw, expected) in [
            ("1/2", "1[/]2"),
            ("11/5nikaigi", "11[/]5nikaigi"),
            ("24/7", "24[/]7"),
            ("1/2nennsei", "1[/]2nennsei"),
            // 略語・大文字 1 字のあとは ・ のまま (TCP/IP は変換サーバーが組の表で / に戻す)
            ("TCP/IP", "[TCP]/[IP]"),
            ("jonn/F/kenedhi", "jonn/[F]/kenedhi"),
            ("github/main", "[github][/][main]"),
            ("toukyou/oosaka", "toukyou/oosaka"),
            ("kokugo/suugaku/eigo", "kokugo/suugaku/eigo"),
            ("/", "/"),
        ] {
            assert_eq!(show(raw), expected, "{raw}");
        }
        assert!(show("IMG_0001.JPG").ends_with("0001[.JPG]"), "{}", show("IMG_0001.JPG"));
        assert!(show("syasinn0001.jpgwookuru").contains("0001[.jpg]wookuru"), "{}", show("syasinn0001.jpgwookuru"));
        // 数字のあとの . に数字が続けば今までどおり小数点。文の終わりの 。 (100.) は 。 のまま
        assert_eq!(show("2.5"), "2[.]5");
        assert_eq!(show("goukeiha100."), "goukeiha100.");
        // 足した拡張子 (mp3) では、ローマ字の語の中の英単語 (mounerune の rune) をファイル名の頭にしない
        assert!(!show("mounerune.mp3haasukiku").contains("rune.mp3"), "{}", show("mounerune.mp3haasukiku"));
        // (仕事 22 の 2) かなのすぐ後ろの . に続く小文字の拡張子は、拡張子だけ英字 (. は 。 のまま。資料.pdf か 〜だよ。PDF かは変換サーバーが決める)
        assert_eq!(show("siryou.pdfha"), "siryou.[pdf]ha");
        assert_eq!(show("koregamatomedayo.pdfhakonoforuda"), "koregamatomedayo.[pdf]hakonoforuda");
        assert_eq!(show("gijiroku.docx"), "gijiroku.[docx]");
        assert!(kana_dot_extension("siryou.", "pdf") && !kana_dot_extension("siryou.", "PDF") && !kana_dot_extension("siryou", "pdf"));
        // 読める並び (sorede.gohann) は今までどおり
        assert_eq!(show("sorede.gohannwotaberu"), "sorede.gohannwotaberu");
    }

    /// 2026-11-05 (仕事 28 の 1): / と . の打ち分けの設定。いつも ・ か 。 (22 の前の動き)・いつも半角の / か .
    #[test]
    fn slash_and_period_key_settings() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        KEY_FORMS_FOR_TEST.set(Some((KeyForm::Japanese, KeyForm::Japanese)));
        assert_eq!(show("1/2"), "1/2");
        assert_eq!(show("github/main").matches("[/]").count(), 0, "{}", show("github/main"));
        assert!(!show("syasinn0001.jpgwookuru").contains("[.jpg]"), "{}", show("syasinn0001.jpgwookuru"));
        assert_eq!(show("2.5"), "2[.]5");
        KEY_FORMS_FOR_TEST.set(Some((KeyForm::Latin, KeyForm::Latin)));
        assert_eq!(show("kokugo/suugaku"), "kokugo[/]suugaku");
        assert_eq!(show("sounanda."), "sounanda[.]");
        KEY_FORMS_FOR_TEST.set(Some((KeyForm::Auto, KeyForm::Auto)));
        assert_eq!(show("1/2"), "1[/]2");
        assert_eq!(show("kokugo/suugaku"), "kokugo/suugaku");
        KEY_FORMS_FOR_TEST.set(None);
    }

    /// 2026-10-31: 数字のあとの 2 字以上の単位は、後ろが母音で始まる語でも単位 (5kgizyou = 5kg以上)。かなのすぐ後ろの単位は前の音を巻き込まない
    /// (sonnnakgdesu = そんな kg です)。読める単位 (5pa-sento の pa = ぱ) と、1 字短い単位でも読める並び (10mmade = 10m まで) は今までどおり
    #[test]
    fn units_after_numbers_and_kana() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        let finishing = |raw: &str| { FINISHING.set(true); let shown = show(raw); FINISHING.set(false); shown };
        assert_eq!(finishing("5kgizyou"), "5[kg]izyou");
        assert_eq!(finishing("30bpmika"), "30[bpm]ika");
        assert_eq!(finishing("2mmizyou"), "2[mm]izyou");
        assert_eq!(finishing("100kmuenitobu"), "100[km]uenitobu");
        assert_eq!(finishing("1000hpaika"), "1000[hpa]ika");
        assert_eq!(finishing("60hzmataha50hz"), "60[hz]mataha50[hz]");
        assert_eq!(finishing("sonnnakgdesu"), "sonnna[kg]desu");
        assert_eq!(finishing("saitekiphdesu"), "saiteki[ph]desu");
        assert_ne!(finishing("10mmade"), "10[mm]ade");
        assert!(!finishing("5pa-sento").contains("[pa]"));
        // 表に無い略語の一部の 2 字 (nmr の nm・ims の ms・aml の ml)、母音の抜け (kdasai の kda)・綴りの途中 (compare の mpa・mshita の ms・
        // kigengire の ng) は単位にしない
        for (raw, unit) in [("okuseihanmrwomotiiru", "[nm]"), ("imshakaisou", "[ms]"), ("amldehatyuu", "[ml]"), ("kinnkyuumrigaokonaeru", "[um]"),
            ("mitekdasai", "[kda]"), ("ottemshita", "[ms]"), ("kigengirere", "[ng]"), ("korewocompareshite", "[mpa]"), ("tekban", "[kb]"), ("2Gbai", "[Gb]")] {
            assert!(!finishing(raw).contains(unit), "{raw}: {}", finishing(raw));
        }
    }

    /// 2026-10-31: 化学・生物・地学を選んだ人は、小文字で打った数を含む化学式 (co2・h2o・caco3) を英字に。読める固まり 1 つと数
    /// (no2 = の2・ba2)、単位 (80km)、日本語は今までどおり。選んでいない人は変わらない
    #[test]
    fn chemical_formulas_when_chemistry_is_focused() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        let finishing = |raw: &str| { FINISHING.set(true); let shown = show(raw); FINISHING.set(false); shown };
        let cases = ["taikichuunoco2noudo", "h2owodenkibunkaisuru", "caco3wokanetusuru",
            "dai1no2", "ba2kai", "sonohonhaomosiroi", "korehaatarasii", "kakuninnsite", "tanaka", "80kmmade", "11mnoitikara", "hagpsgaugokanai"];
        CHEM_FOCUS_FOR_TEST.set(Some(false));
        let before: Vec<String> = cases.iter().map(|raw| finishing(raw)).collect();
        CHEM_FOCUS_FOR_TEST.set(Some(true));
        assert_eq!(finishing("taikichuunoco2noudo"), "taikichuuno[co]2noudo");
        assert_eq!(finishing("h2owodenkibunkaisuru"), "[h]2[o]wodenkibunkaisuru");
        assert_eq!(finishing("caco3wokanetusuru"), "[caco]3wokanetusuru");
        for (raw, b) in cases.iter().zip(&before).skip(3) { assert_eq!(&finishing(raw), b, "{raw}"); }
        CHEM_FOCUS_FOR_TEST.set(None);
    }

    /// 2026-11-01: 分野の略語の表は 1 行 1 語。説明 (#)・空行・数字を含む語・1 字の語は読まない
    #[test]
    fn domain_abbreviation_lines() {
        let parsed = parse_abbreviations("\u{feff}# 医学の略語\r\nmRNA\r\n\r\nNSAIDs\r\nHbA1c\r\nX\r\n  DBMS  \r\n");
        assert_eq!(parsed, [("mrna".to_string(), "mRNA".to_string()), ("nsaids".into(), "NSAIDs".into()), ("dbms".into(), "DBMS".into())]);
    }

    /// 2026-10-31: 入力の頭・記号や数字のあとで打った zh・zl などは矢印のまま。後ろを打ち進めても [zh] に戻さない (← こうしん)。
    /// 語の途中 (英字・かなのあと) の z + h は今までどおり
    #[test]
    fn arrows_at_the_head_stay_arrows() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        for raw in ["zhkousin", "zlsusumu", "zjsita", "a,zhkousin", "1zl2", "zhnoatai"] {
            let tokens = segment_with(raw, STANDARD);
            assert!(tokens.iter().all(|t| t.kind == TokenKind::Romaji), "{raw}: {tokens:?}");
        }
        assert_eq!(show("zh"), "zh");
        assert_eq!(show("korezhkousin"), "kore[zh]kousin");
        assert_eq!(show("xzk"), "xzk");
    }

    /// 2026-10-25: 「botで」がうまく打てない。bot は文脈用の表にしか無く、bot + を が ぼ + two になっていた。
    /// 入力の終わりで同点なら、前から始まる英単語 + 助詞 (chat + を) を、かなのあとの英単語 (ちゃ + two) より選ぶ
    #[test]
    fn context_words_and_particles_after_english() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        assert_eq!(show("chatwo"), "[chat]wo");
        // 2026-10-28: 社名・製品名の表にしか無い語は、語の切れ目に収まれば 1 語 (頭の英単語 able + ローマ字 ton と同点だった)。語の途中では使わない
        assert_eq!(show("ableton"), "[ableton]");
        assert_eq!(show("korehaabletondesu"), "koreha[ableton]desu");
        assert_eq!(show("abletonnopasokon"), "[ableton]nopasokon");
        // 2026-10-29: 英単語のすぐ後ろの促音は、その子音を足すと表の語になるとき (test + s = tests) だけ 1 点高く。test + って は今のまま
        assert_eq!(show("testssitekara"), "[tests]sitekara");
        assert_eq!(show("testtte"), "[test]tte");
        assert_eq!(show("chattteiu"), "[chat]tteiu");
        assert_eq!(sokuon_repairs("chattteiu"), Vec::<usize>::new());
        assert_eq!(show("shopppoi"), "[shop]ppoi");
        assert_eq!(sokuon_repairs("shopppoi"), Vec::<usize>::new());
        // 2026-10-29: 守っている l・x の小書きから始まる表の英単語が語の切れ目まで届くなら英単語 (の + ぉ + ぎん にしない)。
        // わざと打った のぉ・どぉ・はぁ (文の終わり・次の音が続く・助詞の前でも英単語に届かない) は守る
        assert_eq!(show("konosaitonologingamenn"), "konosaitono[login]gamenn");
        assert_eq!(show("nologin"), "no[login]");
        for raw in ["sounanolo", "dolo", "sorehala", "nolodesu", "dolonaka", "halano"] { assert_eq!(show(raw), raw, "{raw}"); }
        // 2026-10-31: 切れ目の後ろは、次の表の英単語の手前まで読めればよい (同じ入力の後ろに英単語があると、login・linux が
        // の + ぉ + ぎん・ぃ + ぬ + x のままだった。報告の文: …nilinuxnozeizyakuseidecriticalna…)。わざと打った のぉ は今までどおり
        assert_eq!(show("konosaitonologingamenndetestsuru"), "konosaitono[login]gamennde[test]suru");
        assert_eq!(show("nologinnotestwosuru"), "no[login]no[test]wosuru");
        assert_eq!(show("sounanolodetest"), "sounanolode[test]");
        if is_lexicon_word("linux") && is_lexicon_word("critical") {
            assert_eq!(show("nilinuxnozeizyakuseidecriticalnayatu"), "ni[linux]nozeizyakuseide[critical]nayatu");
        }
        assert_eq!(show("chatwosuru"), "[chat]wosuru");
        assert_eq!(show("botwo"), "[bot]wo");
        assert_eq!(show("botwotukuru"), "[bot]wotukuru");
        assert_eq!(show("korehabotwo"), "koreha[bot]wo");
        assert_eq!(show("botde"), "[bot]de");
        // 打っている途中の bot の t は打ちかけ。日本語の ぼたん・没・ぼっち・ボトル・ぼっと は崩さない
        for raw in ["bot", "botan", "botsu", "botu", "botti", "botoru", "bottomu", "botto", "bottosuru"] {
            assert_eq!(show(raw), raw, "{raw}");
        }
        // 英単語のすぐ後ろ・語の途中では使わない
        assert_eq!(show("twobot"), "[two]bot");
    }

    #[test]
    fn unreadable_joins_the_english_word() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        assert_eq!(show("keltype"), "[keltype]");
        assert_eq!(show("korehakeltype"), "koreha[keltype]");
        assert_eq!(show("keltypette"), "[keltype]tte");
    }

    #[test]
    fn latin_mode_words() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        assert!(reads_as_romaji("sekai"));
        assert!(!reads_as_romaji("keltype"));
        assert!(!note_latin_word("sekai"));
        assert!(note_latin_word("zxqw"));
        assert!(!session_english("sekai"));
        assert_eq!(show("zxqwnituite"), "[zxqw]nituite");
    }

    #[test]
    fn abbreviations() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        assert_eq!(show("ex."), "[ex.]");
        assert_eq!(show("korehaex.wsl"), "koreha[ex.]wsl");
        assert_eq!(show("e.g.kore"), "[e.g.]kore");
        assert_eq!(show("suimin."), "suimin.");
        assert_eq!(show("sorehano."), "sorehano.");
        assert_eq!(show("etc."), "[etc.]");
    }

    #[test]
    fn accented_words() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        assert_eq!(show("España"), "[España]");
        assert_eq!(show("kafe.café"), "kafe.[café]");
        assert_eq!(show("caféde"), "[caféde]");
    }

    #[test]
    fn english_sentence_words() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        set_english_sentence(true);
        assert_eq!(show("by"), "[by]");
        assert_eq!(show("we"), "[we]");
        assert_eq!(show("check"), "[check]");
        assert_eq!(show("sarerunoha"), "sarerunoha");
        assert_eq!(show("checkhadouiuimi"), "[check]hadouiuimi");
        set_english_sentence(false);
        assert_eq!(show("we"), "we");
    }

    #[test]
    fn sentence_bias_scores_choices_and_resets_on_japanese() {
        set_english_sentence(false);
        assert_eq!(SENTENCE.get(), 0);
        note_sentence_commit(true, true, false);
        assert_eq!(SENTENCE.get(), 4);
        note_sentence_commit(true, false, true);
        assert_eq!(SENTENCE.get(), 6);
        note_sentence_commit(false, false, false);
        assert_eq!(SENTENCE.get(), -4);
        assert!(!in_english_sentence());
        note_sentence_commit(true, false, true);
        assert_eq!(SENTENCE.get(), 2);
        set_english_sentence(false);
        assert!(!in_english_sentence());
    }

    #[test]
    fn sentence_context_handles_short_words_contractions_and_punctuation() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        note_sentence_commit(true, true, false);
        for raw in ["in", "to", "no", "on", "I'm", "we're", "he's", "it's", "we'll", "I'd", "isn't", "home.", "hello!", "home-game", "home 2.5", "home,", "(home)"] {
            assert!(sentence_word(raw, STANDARD), "{raw}");
            assert_eq!(segment(raw), vec![Token { raw: raw.into(), kind: TokenKind::English }], "{raw}");
        }
        for raw in ["sarerunoha", "koreha", "checkhadouiuimi", "home.nihongo", "nihongo!", "'nihongo'", "日本語"] {
            assert!(!sentence_word(raw, STANDARD), "{raw}");
        }
        assert!(sentence_word("hom", STANDARD));
        assert!(sentence_word("qxjv", STANDARD));
        set_english_sentence(true);
        assert!(!sentence_word("qxjv", STANDARD));
        note_sentence_commit(true, true, false);
        FINISHING.set(true);
        assert!(!sentence_word("hom", STANDARD));
        FINISHING.set(false);
        set_english_sentence(false);
        assert_eq!(show("home"), "home");
    }

    #[test]
    fn comma_and_period_after_english_follow_the_sentence_language() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        let shown = |raw: &str| segment(raw).iter().map(|t| match t.kind {
            TokenKind::English => format!("[{}]", t.raw),
            TokenKind::Romaji => t.raw.clone(),
        }).collect::<String>();
        note_commit(None);
        set_text_before(Some(("", true)));
        assert_eq!(shown("github, youtube."), "[github][,] [youtube][.]");
        // すぐ後ろに字が続く , は日本語の 、 の打ち方 (英語ならコンマの後ろに空白を入れる)
        assert_eq!(shown("github,youtube."), "[github],[youtube].");
        // 同じ文に日本語があれば 、 。 のまま (明日はhtml、)。後ろに打ち足した日本語でも
        assert_eq!(shown("ashitahagithub,"), "ashitaha[github],");
        assert_eq!(shown("github,youtubegaii."), "[github],[youtube]gaii.");
        // 文が変われば別に決める (日本語の文のあとの英語の文)
        assert_eq!(shown("koreha.hello, github."), "koreha.[hello][,] [github][.]");
        // 前の文字列: 文の始まりまでに日本語があれば日本語の文 (今日はGitHub Actions、)
        set_text_before(Some(("今日はGitHub ", false)));
        assert_eq!(shown("github,"), "[github],");
        set_text_before(Some(("晴れ。", false)));
        assert_eq!(shown("github,"), "[github][,]");
        set_text_before(Some(("I like GitHub ", true)));
        assert_eq!(shown("github,"), "[github][,]");
        // 読めない・届かないときは、確定した文字列から見た状態で
        note_commit(Some("今日は"));
        note_commit(Some("GitHub"));
        set_text_before(None);
        assert_eq!(shown("github,"), "[github],");
        set_text_before(Some(("GitHub ", false)));
        assert_eq!(shown("github,"), "[github],");
        note_commit(Some("です。"));
        set_text_before(None);
        assert_eq!(shown("github,"), "[github][,]");
        note_commit(None);
        set_text_before(None);
        assert_eq!(shown("github,"), "[github][,]");
        // 英語の文に混ざるローマ字で読める語・日本語の語では日本語の文としない
        set_text_before(Some(("", true)));
        assert!(shown("think python, ramen, ken.").starts_with("[think] [python][,] ramen"), "{}", shown("think python, ramen, ken."));
        // 「 」 の中は別の文。英字の語にはさまれた . はピリオド
        assert_eq!(shown("kareha[Hello, github.]toitta."), "kareha[[Hello][,] [github][.]]toitta.");
        assert_eq!(shown("saitohagithub.youtubedesu."), "saitoha[github][.][youtube]desu.");
        // 日本語の文に空白を入れて打っても、は で終わる語があれば日本語の文
        assert_eq!(shown("kyouha github,youtube wo miru."), "kyouha [github],[youtube] wo miru.");
        // メモ: のあとは新しい文
        set_text_before(Some(("メモ: ", true)));
        assert_eq!(shown("github,"), "[github][,]");
        // かなの後ろは今までどおり
        set_text_before(Some(("", true)));
        assert_eq!(shown("sushi,"), "sushi,");
    }

    #[test]
    fn hyphen_after_english() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        assert_eq!(show("test-desu"), "[test][-]desu");
        assert_eq!(show("ra-men"), "ra-men");
    }

    #[test]
    fn decimal_point() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        assert_eq!(show("python3.12"), "[python]3[.]12");
        assert_eq!(show("kotaeha2."), "kotaeha2.");
        assert_eq!(show("1,000enn"), "1[,]000enn");
        assert_eq!(show("1,00enn"), "1,00enn");
    }

    #[test]
    fn decimal_quantity_units_keep_latin_and_particle_boundaries() {
        for style in [STANDARD, Style { kunrei_nn: true, adaptive: None }] {
            for (raw, expected) in [
                ("2.5molno", "2[.]5[mol]no"), ("2.5mmolwo", "2[.]5[mmol]wo"),
                ("0.25umolni", "0[.]25[umol]ni"), ("3nmolde", "3[nmol]de"),
                ("2.5mLno", "2[.]5[mL]no"), ("2.5mlno", "2[.]5[ml]no"),
                ("5mgwo", "5[mg]wo"), ("10gno", "10[g]no"), ("2kgni", "2[kg]ni"),
                ("5mMno", "5[mM]no"), ("10mmno", "10[mm]no"), ("3cmno", "3[cm]no"),
                ("5 molno", "5 [mol]no"), ("3minna", "3minna"),
                ("2.5mol/Lno", "2[.]5[mol][/][L]no"), ("2.5mol/lno", "2[.]5[mol][/][l]no"),
                ("5mg/mLno", "5[mg][/][mL]no"),
                ("2motto", "2motto"), ("1monowo", "1monowo"),
            ] {
                assert_eq!(show_with(raw, style), expected, "{raw}");
            }
        }
    }

    #[test]
    fn pending_unit_does_not_force_an_incomplete_name_when_finishing() {
        assert_eq!(show("2.5mo"), "2[.]5[mo]");
        FINISHING.set(true);
        let finished = show("2.5mo");
        FINISHING.set(false);
        assert_eq!(finished, "2[.]5mo");
    }

    #[test]
    fn learned_lines() {
        let learned = parse_learned("+keltype\n-shine\n+shine\n-api\n");
        assert!(learned.english.contains("keltype"));
        assert!(!learned.english.contains("shine") && learned.kana.contains("shine"));
        assert!(learned.kana.contains("api"));
    }

    #[test]
    fn old_latin_learning_never_overrides_readable_japanese() {
        for word in ["takai", "yasui", "miso", "teriyaki", "sushi", "kirei", "kawaii", "kanji", "sekai", "in", "home", "game"] {
            let learned = parse_learned(&format!("+{word}\n?{word}\n?{word}\n?{word}\n"));
            assert!(!learned.english.contains(word), "{word}");
            assert!(learning_updates(word, word, true).is_empty(), "{word}");
            assert!(learning_updates(word, word, false).is_empty(), "{word}");
        }
    }

    #[test]
    fn automatic_or_candidate_commits_do_not_teach_language_preferences() {
        for word in ["qzxqzx", "kiroqzx"] {
            assert!(learning_updates(word, word, false).is_empty());
            assert_eq!(learning_updates(word, word, true), vec![format!("?{word}")]);
        }
        // 2026-10-24: 頭に読めない並びがある表に無い語は、もともと英字と判定するので覚えない
        assert!(learning_updates("zqxbrand", "zqxbrand", true).is_empty());
        let once = parse_learned("?keltype\n");
        assert!(!once.english.contains("keltype"));
        let twice = parse_learned("?keltype\n?keltype\n");
        assert!(!twice.english.contains("keltype"));
        let three = parse_learned("?keltype\n?keltype\n?keltype\n");
        assert!(three.english.contains("keltype"));
        let corrected = parse_learned("?keltype\n?keltype\n?keltype\n-keltype\n?keltype\n");
        assert!(!corrected.english.contains("keltype"));
        assert!(corrected.kana.contains("keltype"));
    }

    #[test]
    fn japanese_readings_survive_a_legacy_english_learning_file() {
        if std::env::var_os("AZK_TAKAI_FIXTURE").is_none() { return; }
        for word in ["takai", "yasui", "kirei", "miso", "teriyaki", "takainoha"] {
            assert!(segment_finishing(word).iter().all(|token| token.kind == TokenKind::Romaji), "{word}");
        }
        assert!(learned_english("qzxqzx"));
    }

    #[test]
    fn case_table() {
        let table = parse_case_table("# comment\nGitHub\niPhone\nWi-Fi\n");
        assert_eq!(table.get("github").map(String::as_str), Some("GitHub"));
        assert_eq!(table.get("iphone").map(String::as_str), Some("iPhone"));
        assert!(!table.contains_key("wi-fi"));
    }

    #[test]
    fn compound_case_entries() {
        let table = parse_case_table("SuperMario\nMarioKart\nMinecraft\nPowerShell\nAnki\n");
        assert_eq!(table.get("supermario").map(String::as_str), Some("SuperMario"));
        assert!(case_entry_priority("SuperMario"));
        assert!(case_entry_priority("MarioKart"));
        assert!(case_entry_priority("PowerShell"));
        assert!(!case_entry_priority("Anki"));
        assert!(!case_entry_priority("Minecraft"));
        assert!(!case_entry_priority("TeX"));
    }

    #[test]
    fn compound_names_keep_japanese_boundaries() {
        let table = parse_case_table("GoogleChrome\nGoogleChromeTools\nSuperMario\n");
        let tokens = vec![
            Token { raw: "koreha".into(), kind: TokenKind::Romaji },
            Token { raw: "google".into(), kind: TokenKind::English },
            Token { raw: "chrome".into(), kind: TokenKind::English },
            Token { raw: "tools".into(), kind: TokenKind::English },
            Token { raw: "wo".into(), kind: TokenKind::Romaji },
            Token { raw: "super".into(), kind: TokenKind::English },
            Token { raw: "mario".into(), kind: TokenKind::Romaji },
        ];
        let merged = merge_english_names(tokens, |name| table.contains_key(name));
        assert_eq!(merged.iter().map(|t| t.raw.as_str()).collect::<Vec<_>>(),
            ["koreha", "googlechrometools", "wo", "super", "mario"]);
        assert_eq!(merged[4].kind, TokenKind::Romaji);
    }

    #[test]
    fn case_dictionary_probe() {
        let Ok(probes) = std::env::var("AZK_CASE_TEST") else { return };
        for probe in probes.split('|') {
            let (raw, expected) = probe.split_once('=').expect("raw=Canonical probe");
            assert_eq!(canonical_case(raw), Some(expected), "case correction for {raw}");
            let english = segment_finishing(raw);
            assert_eq!(english, vec![Token { raw: raw.into(), kind: TokenKind::English }],
                "name must remain one English token: {raw}");
            assert!(canonical_case(expected).is_none(), "explicit capitals must be preserved");
        }
        // Existing clients can also recognize the registered units without the new numeric rule.
        for unit in ["mol", "mmol", "umol", "nmol", "pmol"] {
            let tokens = segment_run(&format!("{unit}no"), style());
            assert_eq!(tokens.first(), Some(&Token { raw: unit.into(), kind: TokenKind::English }));
            assert!(tokens[1..].iter().all(|t| t.kind == TokenKind::Romaji));
            assert_eq!(tokens[1..].iter().map(|t| t.raw.as_str()).collect::<String>(), "no");
        }
    }

    /// 2026-11-06 (英単語＋する、利用者の報告 releasesuru): サ変の形のすぐ前の english_suru_words.txt の語 (試験では release・commit・delete・save) を英字に。
    /// t で終わる語 (commit) は、つ を t と す に分ける。サ変の形が続かない所と、表に無い語 (tame・made・mat) は今のまま
    #[test]
    fn english_words_before_suru_forms() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        let finishing = |raw: &str| { FINISHING.set(true); let shown = show(raw); FINISHING.set(false); shown };
        for (raw, expected) in [
            ("releasesuru", "[release]suru"), ("releasesitakedo", "[release]sitakedo"), ("releasesareta", "[release]sareta"),
            ("releasesurugaumakuikanakattakamo", "[release]surugaumakuikanakattakamo"), ("commitsuru", "[commit]suru"),
            ("commitsureba", "[commit]sureba"), ("deletesimasu", "[delete]simasu"), ("saveshite", "[save]shite"),
            ("kinoureleasesita", "kinou[release]sita"), ("releasesitecommitsuru", "[release]site[commit]suru"),
            // 表に無くても、語＋する の全体がローマ字として読めない英単語の表の語 (試験の表の check・think) は英字に
            ("checksuru", "[check]suru"),
            // 全体がふつうの音節で読める語 (commit + suru = こっみつる) は、左が助詞なら
            ("korewocommitsuru", "korewo[commit]suru"),
        ] {
            assert_eq!(finishing(raw), expected, "{raw}");
            assert_eq!(show(raw), expected, "{raw} (打っている途中)");
        }
        // サ変の形の打ちかけ (releasesu) では、まだまとめない
        assert!(!show("releasesu").contains('['), "{}", show("releasesu"));
        // 全体が読める表の語 (home = ほめ) は、english_suru_words.txt に無ければ替えない (ほめする)
        // 全体がふつうの音節で読める語は、左が切れ目でなければ (和文の語の途中かもしれない) まとめない。全体が読めない語 (release の le) は、どこでも
        assert!(!finishing("kacommitsuru").contains("[commit]"), "{}", finishing("kacommitsuru"));
        for raw in ["releaseno", "tamesita", "madesita", "matsuru", "kesuru", "Releasesuru", "homesuru"] {
            let shown = finishing(raw);
            assert!(["[release]", "[tame]", "[made]", "[mat]", "[home]"].iter().all(|w| !shown.contains(w)), "{raw}: {shown}");
        }
        // 語＋する の全体が読めない表の語は、区切りの途中で英字が割れていても (re|star|tsu|ru の形) 1 語に
        let romaji = |raw: &str| Token { raw: raw.into(), kind: TokenKind::Romaji };
        let english = |raw: &str| Token { raw: raw.into(), kind: TokenKind::English };
        assert_eq!(promote_before_suru("checksuru", vec![romaji("che"), romaji("c"), romaji("k"), romaji("su"), romaji("ru")]),
            vec![english("check"), romaji("su"), romaji("ru")]);
        assert_eq!(promote_before_suru("chectsuru", vec![romaji("che"), romaji("c"), romaji("tsu"), romaji("ru")]),
            vec![romaji("che"), romaji("c"), romaji("tsu"), romaji("ru")]);
    }

    fn show(raw: &str) -> String {
        show_with(raw, STANDARD)
    }

    fn show_with(raw: &str, style: Style) -> String {
        segment_with(raw, style)
            .iter()
            .map(|t| match t.kind {
                TokenKind::English => format!("[{}]", t.raw),
                TokenKind::Romaji => t.raw.clone(),
            })
            .collect()
    }

    /// 2026-10-20: c 行 (ca cu co ce ci) は ふつうの音節。ふだん c 行を使わない人 (音ごとの推定) と訓令式の人にだけ英語のしるし
    #[test]
    fn c_row_spellings_are_japanese_unless_the_typist_never_uses_them() {
        set_english_sentence(false);
        for raw in ["cansokusareteiru", "conoyouni", "shianobacuteria", "syougicomawomotiiru", "simoyocotyou", "sincansen", "kaitaco-do", "tsucawareteiru", "nicennteisareteiru", "fucegu"] {
            assert!(!has_english(&segment_with(raw, STANDARD)), "{raw}: {}", show_with(raw, STANDARD));
            // 入れたて (回数なし) の音ごとの推定でも日本語
            let fresh = Style { kunrei_nn: false, adaptive: Some(adaptive_table(&crate::engine::romaji_style::Stats::default())) };
            assert!(!has_english(&segment_with(raw, fresh)), "{raw}: {}", show_with(raw, fresh));
        }
        // か く こ せ を k・s で打ち続けた人には、c 行は英語のしるし (code・cute は英字のまま)
        let k_typist = crate::engine::romaji_style::count(&"kakukosesikakukose".repeat(30));
        let adaptive = Style { kunrei_nn: false, adaptive: Some(adaptive_table(&k_typist)) };
        for word in ["code", "cute", "come"] {
            if !is_word(word) { continue; }
            assert_eq!(show_with(&format!("korehe{word}desu"), adaptive), format!("korehe[{word}]desu"));
        }
        // 大文字で打った語は、ローマ字として読めても英字 (Ken + を。候補にも打ったとおりの形が先に出る)
        assert_eq!(show_with("Kenwomita", STANDARD), "[Ken]womita");
        // 訓令式の人は今までどおり (c は英語のしるし)
        if is_word("code") {
            assert_eq!(show_with("codewokaku", Style { kunrei_nn: true, adaptive: None }), "[code]wokaku");
        }
    }

    /// 2026-10-20: 英単語のすぐ後ろに空白なしで英単語が続く区切りと、英単語 + 助詞 + 英単語 が同点なら、助詞をはさむほう
    #[test]
    fn particle_between_english_words_wins_a_tie_with_a_longer_word() {
        set_english_sentence(false);
        if is_word("detest") && is_word("test") && is_word("haniword") {
            for style in [STANDARD, Style { kunrei_nn: true, adaptive: None }] {
                FINISHING.set(true);
                assert_eq!(show_with("haniworddetestsita", style), "[haniword]de[test]sita");
                FINISHING.set(false);
            }
        }
        // 英単語が 1 つだけのとき (detest + する) は長い語のまま
        if is_word("detest") {
            assert_eq!(show_with("detestsuru", STANDARD), "[detest]suru");
        }
    }

    /// 2026-10-20: 英字の語 + 数字 のすぐ後ろの小文字 1 字 (GPT4o・Pixel8a) は、入力の終わり・を・で の前でだけ英字
    #[test]
    fn single_letter_after_a_model_number() {
        set_english_sentence(false);
        FINISHING.set(true);
        assert_eq!(show_with("GPT4o", STANDARD), "[GPT]4[o]");
        assert_eq!(show_with("GPT-4o", STANDARD), "[GPT][-]4[o]");
        FINISHING.set(false);
        assert_eq!(show_with("GPT4odekaku", STANDARD), "[GPT]4[o]dekaku");
        assert_eq!(show_with("Pixel8awokatta", STANDARD), "[Pixel]8[a]wokatta");
        // 日本語の語の頭と見分けられないもの・数字のあとの日本語は日本語のまま
        assert_eq!(show_with("GPT4otowokiku", STANDARD), "[GPT]4otowokiku");
        assert_eq!(show_with("AI2tsunodannkai", STANDARD), "[AI]2tsunodannkai");
        // 大文字で終わる表の語 (iOS) は和文の中でも 1 語
        if canonical_case("ios") == Some("iOS") {
            assert_eq!(show_with("sorehaiOSnoapuri", STANDARD), "soreha[iOS]noapuri");
            assert_eq!(show_with("kyougaiOSwo", STANDARD), "kyouga[iOS]wo");
        }
        assert_eq!(show_with("CO2nokyuushuu", STANDARD), "[CO]2nokyuushuu");
        assert_eq!(show_with("dai1kai", STANDARD), "dai1kai");
        // 打っている途中の末尾の 1 字は まだ決めない (お の打ちかけかもしれない)
        assert_eq!(show_with("GPT4o", STANDARD), "[GPT]4o");
    }

    /// 2026-10-30: 母音を 1 つ多く打った所の候補は、かなの後ろの母音 1 字の音だけ (英字の語の中・直後・並びの頭は除く)
    #[test]
    fn excess_vowel_spots_are_vowels_after_kana() {
        set_english_sentence(false);
        assert_eq!(excess_vowel_spots("motsuuutsukushii"), vec![5, 6, 15]);
        assert_eq!(excess_vowel_spots("aoi"), vec![1, 2]);
        assert_eq!(excess_vowel_spots("kakikukeko"), Vec::<usize>::new());
        if is_word("github") {
            assert!(excess_vowel_spots("githubeiriguchi").iter().all(|&at| at > 6), "{:?}", excess_vowel_spots("githubeiriguchi"));
        }
    }

    /// 2026-10-30: 英語の文の途中 (it's + 空白 で確定したあと) の correct. は、すぐ後ろに字が続いても . のまま。日本語の文の中の 1 語のあとは 。
    #[test]
    fn a_period_ends_an_english_sentence_even_without_a_space() {
        set_english_sentence(false);
        if !is_word("correct") { return; }
        let shown = |raw: &str| segment(raw).iter()
            .map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect::<Vec<_>>().join("|");
        // 英語の文脈が無いとき (今までどおり): すぐ後ろに字が続く . は 。 の打ち方
        assert!(!shown("correct.tokanozyohaitta.").starts_with("[correct]|[.]"), "{}", shown("correct.tokanozyohaitta."));
        note_sentence_commit(true, false, true);
        assert!(shown("correct.tokanozyohaitta.").starts_with("[correct]|[.]"), "{}", shown("correct.tokanozyohaitta."));
        assert!(shown("correct.").ends_with("[.]") || shown("correct.") == "[correct.]", "{}", shown("correct."));
        set_english_sentence(false);
        // 日本語の文の中の英単語のあと
        assert!(!shown("korehagithub.").ends_with("[.]"), "{}", shown("korehagithub."));
    }

    /// 2026-10-30: 英語の短縮形は 1 つの英字の語 (Space で確定と空白、英語の文の文脈も続く)。n't の n を ん にしない
    #[test]
    fn contractions_are_one_english_word() {
        set_english_sentence(false);
        if !is_word("it") || !is_word("do") || !is_word("you") { return; }
        let show = |raw: &str| segment_finishing(raw).iter()
            .map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect::<Vec<_>>().join("|");
        for word in ["it's", "I'm", "don't", "can't", "you're", "we'll", "I've", "she'd", "doesn't", "won't", "That's", "let's"] {
            assert_eq!(show(word), format!("[{word}]"), "{word}");
        }
        assert_eq!(show("koreit's"), "ko|re|[it's]");
        assert_eq!(show("don'tdesu"), "[don't]|de|su");
        // 打っている途中 (' のあとをまだ打っていない) は今までどおり
        assert!(segment("don'").iter().all(|t| t.raw != "don'"));
        // 短縮形でないもの: 知らない語・' だけ
        assert!(!show("xyz'q").contains("[xyz'q]"));
        assert!(!show("'s").contains("['s]"));
    }

    /// 2026-10-30: 文の終わりの母音を落とした入力 (たべ + t・して + t) で Space を押しても、前のかなを英字の語に取り込まない
    /// (打っている途中の区切りのまま、末尾は読めない子音 1 字)。表の 4 字以上の語・並びの頭や助詞の後ろの語・子音の重なる名前は今までどおり英字
    #[test]
    fn a_trailing_stray_consonant_does_not_pull_kana_into_english() {
        set_english_sentence(false);
        if !is_word("bet") || !is_word("stock") || !is_word("pickup") || !is_word("today") { return; }
        let show = |raw: &str| segment_finishing(raw).iter()
            .map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect::<String>();
        assert_eq!(show("tabet"), "tabet");
        assert_eq!(show("sitekurerut"), "sitekurerut");
        assert_eq!(show("tasukar"), "tasukar");
        assert_eq!(show("gyuunyuunostockkiretetay"), "gyuunyuuno[stock]kiretetay");
        assert_eq!(show("ekidepickupsitekurerutotasukar"), "ekide[pickup]sitekurerutotasukar");
        assert_eq!(show("today"), "[today]");
        assert_eq!(show("my"), "[my]");
        assert_eq!(show("watasihamy"), "watasiha[my]");
        assert_eq!(show("korehanec"), "koreha[nec]");
        assert_eq!(show("boxcryptor"), "[boxcryptor]");
        if is_word("github") { assert_eq!(show("korehagithub"), "koreha[github]"); }
        // 末尾が打ちかけでない入力・n で終わる入力は変えない
        assert_eq!(show("tabete"), "tabete");
        assert_eq!(show("pickup"), "[pickup]");
    }

    /// 2026-10-21: かなが日本語の語にならない英単語 (english_boundary_words.txt) は、語の切れ目に収まっているときだけ英語
    #[test]
    fn readable_english_words_at_word_boundaries() {
        set_english_sentence(false);
        if !words().boundary.contains("issue") || !words().boundary.contains("see") { return; }
        for style in [STANDARD, Style { kunrei_nn: true, adaptive: None }] {
            assert_eq!(show_with("githubnoissuewomiru", style), "[github]no[issue]womiru");
            assert_eq!(show_with("githubnoissue", style), "[github]noissue", "打っている途中の末尾はまだ決めない");
            FINISHING.set(true);
            assert_eq!(show_with("githubnoissue", style), "[github]no[issue]");
            // 前が日本語の語の中の音節 (その・二度目 の の・に) なら英字にしない
            if words().boundary.contains("gore") { assert_eq!(show_with("sonogorenobo", style), "sonogorenobo"); }
            if words().boundary.contains("dome") { assert_eq!(show_with("nidomeno", style), "nidomeno"); }
            assert_eq!(show_with("see", style), "[see]");
            FINISHING.set(false);
            assert_eq!(show_with("see", style), "see");
            assert_eq!(show_with("see,", style), "[see],");
            // 語の途中 (前か後ろが助詞でない) は日本語のまま
            assert_eq!(show_with("isseeni", style), "isseeni");
        }
    }

    /// 2026-10-20: english_case.txt の ~ の行 (普通の英単語と同じ綴りの名前) は別の表。いつも直す表には入れない
    #[test]
    fn japanese_context_case_table_is_separate() {
        let text = "# 説明\n~Windows\nGitHub\n~Zoom\n~two words\n";
        let context = parse_japanese_context_case(text);
        assert_eq!(context.get("windows").map(String::as_str), Some("Windows"));
        assert_eq!(context.get("zoom").map(String::as_str), Some("Zoom"));
        assert!(!context.contains_key("github"));
        let always = parse_case_table(text);
        assert_eq!(always.get("github").map(String::as_str), Some("GitHub"));
        assert!(!always.contains_key("windows"));
    }

    /// 日本語の文をローマ字に直したもので、1 文字ずつ打った途中の状態も含めて
    /// 英語と誤判定される箇所を数える (AZK_WORDS_DIR と AZK_ROMAJI_TEST を指定したときだけ)
    #[test]
    fn false_positives_on_prompts() {
        let (Ok(_), Ok(path)) = (std::env::var("AZK_WORDS_DIR"), std::env::var("AZK_ROMAJI_TEST")) else { return };
        let text = std::fs::read_to_string(path).unwrap();
        let (mut lines, mut final_hits, mut prefix_hits, mut finished_hits) = (0, 0, 0, 0);
        let mut examples = vec![];
        for line in text.lines() {
            lines += 1;
            let tokens = segment(line);
            FINISHING.set(true);
            let finished = segment(line);
            FINISHING.set(false);
            if has_english(&finished) { finished_hits += 1; }
            if has_english(&tokens) {
                final_hits += 1;
                if examples.len() < 40 {
                    let shown: String = tokens.iter().map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect();
                    examples.push(shown);
                }
            }
            for k in 1..line.len() {
                let prefix = segment(&line[..k]);
                if has_english(&prefix) {
                    prefix_hits += 1;
                    if examples.len() < 40 {
                        let words: Vec<&str> = prefix.iter().filter(|t| t.kind == TokenKind::English).map(|t| t.raw.as_str()).collect();
                        examples.push(format!("prefix line={lines} end={k} words={words:?}"));
                    }
                    break;
                }
            }
        }
        let report = format!("lines={lines} final_english={final_hits} any_prefix_english={prefix_hits} finished_english={finished_hits}\n{}", examples.join("\n"));
        let out = std::env::var("AZK_FP_OUT").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("auto_english_fp.txt"));
        std::fs::write(out, report).unwrap();
        // 打ち終えた文が英語と判定されたら失敗 (途中の誤判定は数だけ報告する)
        assert_eq!(final_hits, 0, "英語と誤判定した文がある: {}", examples.join(" / "));
        assert_eq!(finished_hits, 0, "確定時に英語と誤判定した文がある");
    }

    /// 2026-10-14: 英単語の後ろの , . の判定を、Sonnet に作らせた判定に困りそうな文で試す (AZK_PUNCT_CASES を指定したときだけ)。
    /// 入力は 1 行 1 件の「番号\t前の文字列\t打つ列」。前を読めるアプリ (readable) と読めないアプリ (terminal: 前の文字列を
    /// 確定した履歴として渡す) の 2 通りで区切り、英字の語を \u{1}…\u{2} で囲んだ形を書き出す
    #[test]
    fn dev_punctuation_cases() {
        let (Ok(_), Ok(path), Ok(out)) = (std::env::var("AZK_WORDS_DIR"), std::env::var("AZK_PUNCT_CASES"), std::env::var("AZK_PUNCT_OUT")) else { return };
        let text = std::fs::read_to_string(path).unwrap();
        let mut report = String::new();
        for line in text.lines() {
            let mut fields = line.split('\t');
            let (Some(id), Some(before), Some(raw)) = (fields.next(), fields.next(), fields.next()) else { continue };
            for mode in ["readable", "terminal"] {
                note_commit(None);
                if mode == "readable" {
                    set_text_before(Some((before, true)));
                } else {
                    if !before.is_empty() { note_commit(Some(before)); }
                    set_text_before(None);
                }
                // 英字の語は \u{1}…\u{2} で囲む ([ ] は 「 」 を打つ列に出てくるため)
                let shown: String = segment(raw).iter().map(|t| if t.kind == TokenKind::English { format!("\u{1}{}\u{2}", t.raw) } else { t.raw.clone() }).collect();
                report.push_str(&format!("{id}\t{mode}\t{shown}\n"));
            }
        }
        note_commit(None);
        set_text_before(Some(("", true)));
        std::fs::write(out, report).unwrap();
    }

    /// 音ごとの推定 (romaji_style = auto) と今までの方式を比べる (AZK_WORDS_DIR と AZK_REPLAY_DIR を指定したときだけ)。
    /// 日本語の文を 4 通りの打ち方 (訓令式＋nn・ヘボン式＋n・混ぜた 2 通り) に直し、1 行ずつ「判定してから数える」順に流す。
    /// 誤検出 (打ち終えた文・打っている途中) と、最後の回数での英単語の検出率を、方式ごとに書き出す
    #[test]
    fn adaptive_replay() {
        use crate::engine::romaji_style::{count, simulate, Stats};
        let (Ok(_), Ok(dir)) = (std::env::var("AZK_WORDS_DIR"), std::env::var("AZK_REPLAY_DIR")) else { return };
        let dir = std::path::Path::new(&dir);
        let base = std::fs::read_to_string(dir.join("romaji_test_kunrei.txt")).unwrap();
        // 打ち方: 音 → 綴り (sokuon は次の音の頭の子音を重ねる)
        let profiles: Vec<(&str, Vec<(&str, &str)>)> = vec![
            ("kunrei_nn", vec![]),
            ("hepburn_n", vec![("si", "shi"), ("ti", "chi"), ("tu", "tsu"), ("zi", "ji"), ("hu", "fu"), ("sya", "sh"), ("tya", "ch"), ("zya", "j"), ("n", "n")]),
            ("mix_si_chi_fu_ja_n", vec![("ti", "chi"), ("zi", "ji"), ("hu", "fu"), ("tya", "ch"), ("zya", "j"), ("n", "n")]),
            ("mix_shi_ti_tsu_nn", vec![("si", "shi"), ("tu", "tsu"), ("sya", "sh")]),
            // 前半は訓令式＋nn、後半はヘボン式＋n (打ち方を変えた人・家族で使う PC)。忘却で追いつけるか
            ("switch_kunrei_to_hepburn", vec![("si", "shi"), ("ti", "chi"), ("tu", "tsu"), ("zi", "ji"), ("hu", "fu"), ("sya", "sh"), ("tya", "ch"), ("zya", "j"), ("n", "n")]),
            // 2026-10-20: か く こ せ し を c で打つ人 (ca cu co ce ci)。半分だけ c で打つ人 (写経の文の組のように混ぜる人) も
            ("c_row", vec![("ka", "ca"), ("ku", "cu"), ("ko", "co"), ("se", "ce"), ("si", "ci")]),
            ("c_row_half", vec![("ka", "ca|ka"), ("ku", "cu|ku"), ("ko", "co|ko"), ("se", "ce|se"), ("si", "ci|si")]),
        ];
        let reromanize = |line: &str, choice: &[(&str, &str)]| -> String {
            let units = simulate(line);
            let mut keys: Vec<String> = units.iter().map(|u| u.keys.clone()).collect();
            // "ca|ka" は 1 回ごとに交互に使う
            let mut turn = 0usize;
            for (k, unit) in units.iter().enumerate() {
                let mut pick = |g: &str| choice.iter().find(|(c, _)| *c == g).map(|(_, v)| {
                    turn += 1;
                    let options: Vec<&str> = v.split('|').collect();
                    options[turn % options.len()]
                });
                let new = match unit.kana.as_str() {
                    "か" => pick("ka").map(|v| v.to_string()),
                    "く" => pick("ku").map(|v| v.to_string()),
                    "こ" => pick("ko").map(|v| v.to_string()),
                    "せ" => pick("se").map(|v| v.to_string()),
                    "し" => pick("si").map(|v| v.to_string()),
                    "ち" => pick("ti").map(|v| v.to_string()),
                    "つ" => pick("tu").map(|v| v.to_string()),
                    "じ" => pick("zi").map(|v| v.to_string()),
                    "ふ" => pick("hu").map(|v| v.to_string()),
                    "しゃ" | "しゅ" | "しょ" => pick("sya").map(|v| format!("{v}{}", &unit.keys[unit.keys.len() - 1..])),
                    "ちゃ" | "ちゅ" | "ちょ" => pick("tya").map(|v| format!("{v}{}", &unit.keys[unit.keys.len() - 1..])),
                    "じゃ" | "じゅ" | "じょ" => pick("zya").map(|v| format!("{v}{}", &unit.keys[unit.keys.len() - 1..])),
                    "ん" if unit.keys == "nn" => {
                        let next_consonant = units.get(k + 1).and_then(|u| u.keys.chars().next()).is_some_and(|c| c.is_ascii_alphabetic() && !"aiueoyn".contains(c));
                        pick("n").filter(|_| next_consonant).map(|v| v.to_string())
                    }
                    _ => None,
                };
                if let Some(new) = new {
                    keys[k] = new;
                }
            }
            // 促音の重ねは次の音の頭に合わせる (っち = tchi)
            for k in 0..units.len() {
                if units[k].kana == "っ" && units[k].keys.len() == 1 && k + 1 < units.len() {
                    let first = keys[k + 1].chars().next().unwrap_or('t');
                    keys[k] = if keys[k + 1].starts_with("ch") { "t".into() } else { first.to_string() };
                }
            }
            keys.concat()
        };
        let scowl = std::fs::read_to_string(std::path::Path::new(&std::env::var("AZK_WORDS_DIR").unwrap()).join("english_words_scowl.txt")).unwrap();
        let probes: Vec<String> = scowl
            .lines()
            .map(|w| w.trim())
            .filter(|w| (4..=9).contains(&w.len()) && w.chars().all(|c| c.is_ascii_lowercase()))
            .filter(|w| ["shi", "chi", "tsu", "ji", "sh", "ch", "j", "f"].iter().any(|s| w.contains(s)) || w.as_bytes().windows(2).any(|p| p[0] == b'n' && !b"aiueoyn".contains(&p[1])))
            .map(|w| w.to_string())
            .collect();
        let english_rate = |style: Style| -> f64 {
            let hit = probes.iter().filter(|w| has_english(&segment_with(&format!("korehe{w}"), style))).count();
            hit as f64 / probes.len() as f64
        };
        // 2026-10-20: c で始まる音 (ca co cu ce ci。ch・ck は除く) を含む英単語 (code・cute・cake)。c 行の綴りを日本語として読むようにしたため
        let c_probes: Vec<String> = scowl.lines().map(|w| w.trim())
            .filter(|w| (3..=9).contains(&w.len()) && w.chars().all(|c| c.is_ascii_lowercase()))
            .filter(|w| w.as_bytes().windows(2).any(|p| p[0] == b'c' && b"aiueo".contains(&p[1])))
            .map(|w| w.to_string()).collect();
        let c_rate = |style: Style| -> f64 {
            let hit = c_probes.iter().filter(|w| has_english(&segment_with(&format!("korehe{w}"), style))).count();
            hit as f64 / c_probes.len() as f64
        };
        let mut report = format!("probes={} (shi chi tsu j sh ch f や n+子音 を含む英単語 4〜9 文字、korehe+語)、c_probes={} (ca co cu ce ci を含む英単語 3〜9 文字)\n", probes.len(), c_probes.len());
        // AZK_REPLAY_LIMITS: 例外を当てる語の長さの上限を、方式によらず固定して比べる (カンマ区切り。0 = ふだんどおり)
        let limits: Vec<usize> = std::env::var("AZK_REPLAY_LIMITS").map(|s| s.split(',').map(|x| x.parse().unwrap()).collect()).unwrap_or(vec![0]);
        for limit in limits {
        EXCEPTION_MAX_LEN.set((limit > 0).then_some(limit));
        report += &format!("\n######## 例外を当てる語の長さの上限 = {}\n", if limit == 0 { "ふだんどおり (auto は 3、ほかはなし)".to_string() } else { limit.to_string() });
        // 2026-10-21: AZK_REPLAY_EVERY=N で N 行に 1 行だけ判定する (回数はすべての行で数える。全部だと数時間かかる)。AZK_REPLAY_PROFILES で打ち方を選ぶ (カンマ区切り)
        let every: usize = std::env::var("AZK_REPLAY_EVERY").ok().and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
        let selected: Option<Vec<String>> = std::env::var("AZK_REPLAY_PROFILES").ok().filter(|s| !s.is_empty()).map(|s| s.split(',').map(str::to_string).collect());
        report += &format!("判定する行: {every} 行に 1 行\n");
        for (name, choice) in &profiles {
            if selected.as_ref().is_some_and(|names| !names.iter().any(|n| n == name)) { continue; }
            let started = std::time::Instant::now();
            let half = base.lines().count() / 2;
            let lines: Vec<String> = base
                .lines()
                .enumerate()
                .map(|(k, l)| if name.starts_with("switch") && k < half { l.to_string() } else { reromanize(l, choice) })
                .collect();
            let (mut fin_late, mut late_examples) = (0usize, vec![]);
            let mut stats = Stats::default();
            // 1 日に 20 行打つとみなす
            let mut stats_day = 0.0;
            let (mut fin, mut pre) = (vec![0usize; 3], vec![0usize; 3]);
            let mut examples = vec![];
            let mut pre_examples: Vec<String> = vec![];
            // 2026-10-25: AZK_REPLAY_FP_DUMP を指定したら、打ち終えた文の誤検出を方式ごとにすべて書き出す (fp_<打ち方>.txt。どの語で外れたかを見る)
            let mut fp_dump = String::new();
            for (index, line) in lines.iter().enumerate() {
                let styles = [STANDARD, Style { kunrei_nn: true, adaptive: None }, Style { kunrei_nn: false, adaptive: Some(adaptive_table(&stats)) }];
                for (s, style) in styles.iter().enumerate() {
                    if index % every != 0 { break; }
                    let tokens = segment_with(line, *style);
                    if has_english(&tokens) {
                        fin[s] += 1;
                        if std::env::var("AZK_REPLAY_FP_DUMP").is_ok() {
                            fp_dump += &format!("{}\t{}\n", ["standard", "kunrei_nn", "auto"][s], tokens.iter().map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect::<String>());
                        }
                        if s == 2 && name.starts_with("switch") && index >= half + 300 {
                            fin_late += 1;
                            if late_examples.len() < 5 {
                                late_examples.push(line.clone());
                            }
                        }
                        if s == 2 && examples.len() < 15 {
                            examples.push(tokens.iter().map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect::<String>());
                        }
                    }
                    if let Some(k) = (1..line.len()).find(|&k| has_english(&segment_with(&line[..k], *style))) {
                        pre[s] += 1;
                        if s == 2 && pre_examples.len() < 10 {
                            pre_examples.push(segment_with(&line[..k], *style).iter().map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect::<String>());
                        }
                    }
                }
                // 1 行 = 1 日とみなして忘却も効かせる (本番の record と同じ足し方)
                stats.add_with_decay(&count(line), stats_day);
                stats_day += 1.0 / 20.0;
            }
            if name.starts_with("switch") {
                report += &format!("\n(切り替えから 300 行より後の auto の誤検出: {fin_late} {:?})", late_examples);
            }
            let table = adaptive_table(&stats);
            let penalties: Vec<String> = ADAPTIVE.iter().zip(table.iter()).filter(|(_, p)| **p > 0).map(|((s, _, _), p)| format!("{s}={:.2}", *p as f64 / 100.0)).collect();
            let adaptive = Style { kunrei_nn: false, adaptive: Some(table) };
            if let Ok(dump) = std::env::var("AZK_REPLAY_DUMP") {
                let missed: Vec<String> = probes.iter().filter(|w| !has_english(&segment_with(&format!("korehe{w}"), adaptive))).map(|w| {
                    let shown: String = segment_with(&format!("korehe{w}"), adaptive).iter().map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect();
                    format!("{w}\t{shown}")
                }).collect();
                std::fs::write(format!("{dump}_{name}.txt"), missed.join("\n")).unwrap();
            }
            let shown_examples: Vec<String> = examples.iter().cloned().chain(pre_examples.iter().map(|e| format!("(途中) {e}"))).collect();
            report += &format!(
                "\n== {name} ({} 行、{:.0} 秒)\n誤検出 打ち終えた文: standard={} kunrei_nn={} auto={}\n誤検出 途中で一度でも: standard={} kunrei_nn={} auto={}\n英単語の検出率: standard={:.3} kunrei_nn={:.3} auto={:.3}\nc を含む英単語の検出率: standard={:.3} kunrei_nn={:.3} auto={:.3}\n最後の加点: {}\n{}\n",
                lines.len(), started.elapsed().as_secs_f64(), fin[0], fin[1], fin[2], pre[0], pre[1], pre[2],
                english_rate(STANDARD), english_rate(Style { kunrei_nn: true, adaptive: None }), english_rate(adaptive),
                c_rate(STANDARD), c_rate(Style { kunrei_nn: true, adaptive: None }), c_rate(adaptive),
                penalties.join(" "), shown_examples.join("\n")
            );
            let cases = ["anpanmantyoko", "tenpura", "sinbun", "shinbun", "shinbunwoyomu", "sinnbunnwoyomu", "kanpeki", "sanpo", "senpai", "konbini", "tanpopo", "shinkansen", "chanto", "kinjo", "jinja", "shopni", "chromedekensaku", "ankiwosuru", "fontwokaeru", "shellde", "pushsita", "branchwokiru", "jsondekaku", "functionwokaku", "checkshite"];
            let shown: Vec<String> = cases.iter().map(|c| segment_with(c, adaptive).iter().map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect::<String>()).collect();
            let shown_k: Vec<String> = cases.iter().map(|c| segment_with(c, Style { kunrei_nn: true, adaptive: None }).iter().map(|t| if t.kind == TokenKind::English { format!("[{}]", t.raw) } else { t.raw.clone() }).collect::<String>()).collect();
            report += &format!("例 auto: {}\n例 kunrei_nn: {}\n", shown.join(" "), shown_k.join(" "));
            // 2026-10-24: 打ち方 1 つごとに書き出す (PC が眠って止まっても、終わった打ち方の結果は残す)
            std::fs::write(dir.join("adaptive_replay.txt"), &report).unwrap();
            if std::env::var("AZK_REPLAY_FP_DUMP").is_ok() {
                std::fs::write(dir.join(format!("fp_{name}.txt")), &fp_dump).unwrap();
            }
        }
        }
        EXCEPTION_MAX_LEN.set(None);
        std::fs::write(dir.join("adaptive_replay.txt"), report).unwrap();
    }

    /// 単語リスト (1 行 1 語、タブの後ろは無視) のうち、ローマ字として最後まで読める語を書き出す (AZK_READABLE_IN / AZK_READABLE_OUT)
    #[test]
    fn dump_readable() {
        let (Ok(input), Ok(output)) = (std::env::var("AZK_READABLE_IN"), std::env::var("AZK_READABLE_OUT")) else { return };
        let style = STANDARD;
        let text = std::fs::read_to_string(input).unwrap();
        let mut out = String::new();
        for line in text.lines() {
            let word = line.split('\t').next().unwrap_or("");
            let (tokens, flags) = segment_run_raw(word, style);
            if !word.is_empty() && tokens.iter().all(|t| t.kind == TokenKind::Romaji) && !flags.iter().any(|f| *f) {
                out += line;
                out.push('\n');
            }
        }
        std::fs::write(output, out).unwrap();
    }

    #[test]
    fn debug_adaptive() {
        let (Ok(_), Ok(text)) = (std::env::var("AZK_WORDS_DIR"), std::env::var("AZK_DEBUG_ADAPTIVE")) else { return };
        let kunrei = crate::engine::romaji_style::count(&"sitatukatteziyuunisyabettannmonohutatu".repeat(400));
        let a = Style { kunrei_nn: false, adaptive: Some(adaptive_table(&kunrei)) };
        let mut out = String::new();
        for line in text.split('|') {
            let (raw, flags) = segment_run_raw(line, a);
            out += &format!("{line}: final={} raw={:?} flags={:?}\n", show_with(line, a), raw.iter().map(|t| format!("{}{}", if t.kind == TokenKind::English { "E:" } else { "" }, t.raw)).collect::<Vec<_>>(), flags);
        }
        std::fs::write(std::env::temp_dir().join("debug_adaptive.txt"), out).unwrap();
    }

    #[test]
    fn debug_sentence() {
        let Ok(text) = std::env::var("AZK_DEBUG_SENTENCE") else { return };
        let k = Style { kunrei_nn: std::env::var("AZK_DEBUG_STANDARD").is_err(), adaptive: None };
        let mut out = String::new();
        for line in text.split('|') {
            for (index, c) in line.char_indices() {
                let len = index + c.len_utf8();
                out += &format!("{}\n", show_with(&line[..len], k));
            }
            if std::env::var_os("AZK_DEBUG_FINISHING").is_some() {
                FINISHING.set(true);
                let shown = show_with(line, k);
                FINISHING.set(false);
                out += &format!("finish: {shown}\n");
            }
        }
        std::fs::write(std::env::temp_dir().join("debug_sentence.txt"), out).unwrap();
    }

    /// 2026-10-24: 単語表に無い語で、頭にローマ字として読めない並びがあるもの (dynabook の dy) は、語の切れ目までを 1 語の英字に
    #[test]
    fn unknown_words_with_unreadable_head() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        assert_eq!(show("dynabook"), "[dynabook]");
        // 2026-10-25: 入力の頭の って・っそ の打ちかけは、重ねで始まる語 (ttedit) の頭でも英字にしない。重ねのあとが読めないもの (ffm・llv) は別
        for w in ["tte", "tted", "sso", "jja", "ppoi"] { assert!(leading_sokuon_kana(w), "{w}"); }
        for w in ["tt", "ffm", "llv", "nna", "aat", "ttx"] { assert!(!leading_sokuon_kana(w), "{w}"); }
        assert_eq!(show("korehadynabookdesu"), "koreha[dynabook]desu"); // これ + had + yな にしない
        assert_eq!(show("kinoudynabookwokatta"), "kinou[dynabook]wokatta"); // kino + udynabook にしない (母音の音節は含めない)
        assert_eq!(show("kinoukarapytorch"), "kinoukara[pytorch]"); // 読めない並び pyt が英単語の頭 (python) になるので ra は含めない
        assert_eq!(show("dynabooksaikou"), "[dynabook]saikou"); // 読めない文字のすぐ後ろで残りが日本語として読める所で切る
        assert_eq!(show("korehadyn"), "koreha[dyn]"); // 打っている途中も英字
        assert_eq!(show("korehady"), "korehady"); // dy はまだ dya などの打ちかけ
        assert_eq!(show("kyndrylnokaisya"), "[kyndryl]nokaisya");
        assert_eq!(show("lynx"), "[lynx]");
        assert_eq!(show("mozcwotsukau"), "[mozc]wotsukau"); // 並びの頭なら読める音節 mo も含める
        // 読めない文字を含まない日本語・母音の無い並び・助詞が続く子音の並びは今までどおり
        assert_eq!(show("kyouhaiitenkidesune"), "kyouhaiitenkidesune");
        assert_eq!(show("tsmc"), "tsmc");
        assert_eq!(show("dmmade"), "[dm]made");
    }

    #[test]
    fn detection() {
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        assert_eq!(show("ashitahagoogledesagasu"), "ashitaha[google]desagasu");
        assert_eq!(show("testdesu"), "[test]desu");
        assert_eq!(show("hello"), "[hello]");
        assert_eq!(show("windowsnokoushin"), "[windows]nokoushin");
        assert_eq!(show("home"), "home"); // ローマ字として読めるので日本語 (ほめ)
        assert_eq!(show("sushi"), "sushi");
        assert_eq!(show("game"), "game");
        assert_eq!(show("goog"), "goog"); // まだローマ字として読める (ごおg)
        assert_eq!(show("googl"), "[googl]"); // 読めなくなったので英語の打ちかけ
        assert_eq!(show("kensaku"), "kensaku");
        assert_eq!(show("python"), "[python]");
        assert_eq!(show("gakkou"), "gakkou");
        assert_eq!(show("konnnichiha"), "konnnichiha");
        assert_eq!(show("xtu"), "xtu"); // 英単語でなければローマ字に回す
        assert_eq!(show("metcha"), "metcha"); // めっちゃ
        assert_eq!(show("PCha"), "[PC]ha");
        assert_eq!(show("github"), "[github]"); // gi・thu・b より英語
        assert_eq!(show("githubnopush"), "[github]nopush");
        assert_eq!(show("youtubedemiru"), "[youtube]demiru"); // ようつべ ではなく
        assert_eq!(show("youtu"), "youtu"); // 大文字は英字のまま
        assert_eq!(show("LINEmo"), "[LINE]mo");
        assert_eq!(show("Googleha"), "[Google]ha");
        // 訓令式・ん=nn の人
        let k = Style { kunrei_nn: true, adaptive: None };
        assert_eq!(show_with("annki", k), "annki"); // 暗記
        assert_eq!(show_with("anki", k), "[anki]"); // Anki
        assert_eq!(show_with("ankiwohiraku", k), "[anki]wohiraku");
        assert_eq!(show_with("shopni", k), "[shop]ni");
        assert_eq!(show_with("sitteiru", k), "sitteiru");
        assert_eq!(show_with("home", k), "home");
        assert_eq!(show("anki"), "anki"); // 標準の人には暗記
        // Space を押した時点で n が 1 つだけ残っていたら英語のしるし (nn と打つ人だけ)
        // (単語表は 1 回しか設定できないので、表にある ken で確かめる)
        let finishing = |raw: &str, style: Style| {
            FINISHING.set(true);
            let shown = show_with(raw, style);
            FINISHING.set(false);
            shown
        };
        assert_eq!(show_with("ken", k), "ken"); // 打っている途中は けn
        assert_eq!(finishing("ken", k), "[ken]");
        assert_eq!(finishing("kenn", k), "kenn"); // けん
        assert_eq!(finishing("kon", k), "kon"); // 英単語でなければ こん
        assert_eq!(finishing("korehaken", k), "koreha[ken]");
        assert_eq!(finishing("ken", STANDARD), "ken"); // n 1 つで ん と打つ人には けん
        // n で終わる英単語 + n + 母音 (nn と打つ人の mainni = main + に。まいんい は日本語の語にならない)
        assert_eq!(show_with("mainni", k), "[main]ni");
        assert_eq!(show_with("mainnni", k), "mainnni"); // まいんに
        assert_eq!(show_with("mainni", STANDARD), "mainni"); // n 1 つで ん と打つ人には まいんに
        // 2026-11-23: ん をまたぐ 3 音 (英単語の最後の音 + ん + 後ろの母音) が日本語の語の頭としてよく出るなら、表にあっても n_split を当てない
        // (ken + に の け・ん・い = 権威、titan + に の た・ん・い = 単位。main + に の い・ん・い は少ないので英語のまま)
        assert!(n_straddles_japanese_head("korehakenni", 6, 9));
        assert_eq!(show_with("korehakenni", k), "korehakenni");
        assert!(n_straddles_japanese_head("matitannide", 2, 7)); // 町単位で (ま + titan + に + で にしない)
        assert!(!n_straddles_japanese_head("korehemainni", 6, 10));
        assert!(!n_straddles_japanese_head("mainni", 0, 4));
        // 略語の後ろのピリオドは英字のまま
        assert_eq!(show("etc..."), "[etc...]");
        assert_eq!(show("etc."), "[etc.]");
        // 音ごとの推定 (auto): まだ打っていない人は標準と同じ
        let fresh = Style { kunrei_nn: false, adaptive: Some(adaptive_table(&crate::engine::romaji_style::Stats::default())) };
        assert_eq!(show_with("shinbun", fresh), "shinbun");
        assert_eq!(show_with("anki", fresh), "anki");
        // 訓令式・nn だけを 200 回ずつ打った人
        let kunrei = crate::engine::romaji_style::count(&"sitatukatteziyuunisyabettannmonohutatu".repeat(40));
        let a = Style { kunrei_nn: false, adaptive: Some(adaptive_table(&kunrei)) };
        assert_eq!(show_with("annki", a), "annki");
        assert_eq!(show_with("shopni", a), "[shop]ni");
        assert_eq!(show_with("sitteiru", a), "sitteiru");
        assert_eq!(show_with("tenpura", a), "tenpura"); // ten + pura (3 文字以下は例外で日本語)
        // ヘボン式・n 1 つの人には、同じ綴りは日本語
        let hepburn = crate::engine::romaji_style::count(&"shitatsukatteshinbunwojiyuunishabettanmonofutatsu".repeat(40));
        let h = Style { kunrei_nn: false, adaptive: Some(adaptive_table(&hepburn)) };
        assert_eq!(show_with("shinbunwoyomu", h), "shinbunwoyomu");
        assert_eq!(show_with("chanto", h), "chanto");
    }
}

#[cfg(test)]
mod phrase_tests {
    use super::*;

    /// 小さな英語の次の語の表 (語, -ln P * 1000) と組 (前, 後, 点)。組が無いときの係数は 3000
    struct Fake { words: Vec<(&'static str, u32)>, pairs: Vec<(&'static str, &'static str, u32)> }

    impl PhraseTable for Fake {
        fn find(&self, word: &str) -> Option<usize> { self.words.iter().position(|(w, _)| *w == word) }
        fn uni(&self, id: usize) -> u32 { self.words[id].1 }
        fn mult(&self, _: usize) -> u32 { 3000 }
        fn successors(&self, id: usize) -> HashMap<usize, u32> {
            self.pairs.iter().filter(|(a, _, _)| *a == self.words[id].0).filter_map(|(_, b, c)| Some((self.find(b)?, *c))).collect()
        }
    }

    fn table() -> Fake {
        Fake {
            words: vec![("see", 8000), ("you", 6000), ("tomorrow", 9000), ("so", 6000), ("no", 7000), ("bag", 11000), ("level", 10000), ("seven", 10000),
                        ("week", 10000), ("end", 9000), ("weekend", 9000), ("plans", 11000), ("xs", 13108), ("peed", 13150), ("about", 9000), ("the", 3000)],
            pairs: vec![("see", "you", 3000), ("you", "tomorrow", 5000), ("level", "seven", 6000), ("week", "end", 6000), ("weekend", "plans", 4000),
                        ("end", "plans", 9000), ("xs", "peed", 1000), ("about", "the", 2000)],
        }
    }

    fn tokens(shape: &str) -> Vec<Token> {
        shape.split('|').map(|t| match t.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            Some(w) => Token { raw: w.into(), kind: TokenKind::English },
            None => Token { raw: t.into(), kind: TokenKind::Romaji },
        }).collect()
    }

    fn words(run: &str, shape: &str) -> Option<Vec<String>> {
        let known = |w: &str| ["see", "you", "tomorrow", "bag", "level", "seven", "week", "end", "weekend", "plans", "about", "the"].contains(&w);
        phrase_words(run, &tokens(shape), &table(), &known, &known)
    }

    #[test]
    fn splits_a_run_whose_current_segmentation_is_broken() {
        // 今の区切りに表に無い英字 [w] がある → 次の語の表で see you tomorrow
        assert_eq!(words("seeyoutomorrow", "se|e|yo|u|to|mo|r|ro|[w]"), Some(vec!["see".into(), "you".into(), "tomorrow".into()]));
        // 英字の語のすぐ前の子音 1 字 (a|bo|u|t|[the] の t) も崩れ (っ になる重ねはローマ字の前だけ)
        assert_eq!(words("aboutthe", "a|bo|u|t|[the]"), Some(vec!["about".into(), "the".into()]));
    }

    #[test]
    fn leaves_runs_that_mix_kana_and_english_words() {
        // その bag (so|no|[bag]) は崩れていないので分けない (仕事 20 の打鍵の組で日本語を英語にしていた形)
        assert_eq!(words("sonobag", "so|no|[bag]"), None);
    }

    #[test]
    fn keeps_the_current_boundaries_of_english_words() {
        // 全部表の英単語なら切り直さない (level|seven を levels|even にしない)。並びが表に無ければ空白を入れない
        assert_eq!(words("levelseven", "[level]|[seven]"), Some(vec!["level".into(), "seven".into()]));
        assert_eq!(words("sevenlevel", "[seven]|[level]"), None);
    }

    #[test]
    fn keeps_compounds_together() {
        // つないだ 1 語の点が 2 語に切った点以下なら 1 語 (weekend)
        let fake = table();
        let mut pair = |a: usize, b: usize| fake.successors(a).get(&b).copied();
        let known = |_: &str| false;
        let merged = merged_phrase(vec!["week".into(), "end".into(), "plans".into()], &HashSet::new(), &fake, &mut pair, &known, &known);
        assert_eq!(merged, Some(vec!["weekend".into(), "plans".into()]));
        // 今の区切りで 7 字以上の表の英単語の中 (ここでは位置 3) は切らない
        let seeyou = |w: &str| w == "seeyou";
        let merged = merged_phrase(vec!["see".into(), "you".into(), "tomorrow".into()], &HashSet::from([3]), &fake, &mut pair, &seeyou, &known);
        assert_eq!(merged, Some(vec!["seeyou".into(), "tomorrow".into()]));
    }

    #[test]
    fn rejects_rare_words_and_unseen_pairs() {
        // めったに出ない語 (xs・peed) を含む切り方は採らない
        assert_eq!(words("xspeed", "[xspeed]"), None);
        // 表に無い 2 語の並び (tomorrow see) を含む切り方は採らない
        assert_eq!(words("tomorrowsee", "to|mo|r|ro|[wsee]"), None);
    }

    /// 2026-11-11: 製品名の表の綴り (aftereffects) は、語ごとの英字の区切りにして表記の大文字小文字に (After|Effects、空白は 5)。
    /// かなの語のすぐ後ろ・前でも当て、語の途中・大文字を交ぜた綴り・表の語の一部だけ・Backspace で戻した並びには当てない
    #[test]
    fn product_names_get_their_spaces_and_case() {
        set_test_products(&["After Effects", "DaVinci Resolve", "Windows 11", "Premiere Pro"]);
        let tok = |parts: &[(&str, bool)]| parts.iter().map(|&(r, e)| Token { raw: r.into(), kind: if e { TokenKind::English } else { TokenKind::Romaji } }).collect::<Vec<_>>();
        let shown = |(t, s, m, _): (Vec<Token>, Vec<usize>, bool, Vec<(String, String)>)| (t.iter().map(|t| t.raw.clone()).collect::<Vec<_>>().join("|"), s, m);
        assert_eq!(shown(product_phrases("aftereffects", tok(&[("aftereffects", true)]))), ("After|Effects".into(), vec![5], true));
        // かなの語に続く・かなの語が続く (aftereffectsde = After Effects + で、wa + davinciresolve)
        assert_eq!(shown(product_phrases("aftereffectsde", tok(&[("aftereffects", true), ("de", false)]))), ("After|Effects|de".into(), vec![5], true));
        assert_eq!(shown(product_phrases("wadavinciresolve", tok(&[("wa", false), ("davinciresolve", true)]))), ("wa|DaVinci|Resolve".into(), vec![9], true));
        // 数字の入る版の名 (windows11)
        assert_eq!(shown(product_phrases("windows11", tok(&[("windows", true), ("11", false)]))), ("Windows|11".into(), vec![7], true));
        // 表の語の一部だけ (premiere)・語の途中から (xaftereffects)・大文字を交ぜた綴り (AfterEffects) には当てない
        assert!(!product_phrases("premiere", tok(&[("premiere", true)])).2);
        assert!(!product_phrases("xaftereffects", tok(&[("x", true), ("aftereffects", true)])).2);
        assert!(!product_phrases("AfterEffects", tok(&[("AfterEffects", true)])).2);
        // 確定の直後の Backspace で戻した並びには当てない
        decline_phrase("aftereffects");
        assert!(!product_phrases("aftereffects", tok(&[("aftereffects", true)])).2);
        DECLINED_PHRASES.with(|d| d.borrow_mut().clear());
        // 2026-11-19: Backspace で戻す形は打った綴り (小文字)、覚えるのは当てた綴りそのもの (かなが続く aftereffectsde・数字の入る windows11 も)
        let pieces = product_phrases("aftereffectsde", tok(&[("aftereffects", true), ("de", false)])).3;
        assert_eq!(pieces, vec![("aftereffects".to_string(), "After Effects".to_string())]);
        assert_eq!(undo_for_spaces("After Effectsで", "aftereffectsde", &[5], &pieces), ("aftereffectsで".to_string(), vec!["aftereffects".to_string()]));
        let pieces = product_phrases("windows11", tok(&[("windows", true), ("11", false)])).3;
        assert_eq!(undo_for_spaces("Windows 11", "windows11", &[7], &pieces), ("windows11".to_string(), vec!["windows11".to_string()]));
        for (piece, _) in &pieces { decline_phrase(piece); }
        assert!(!product_phrases("windows11", tok(&[("windows", true), ("11", false)])).2);
        DECLINED_PHRASES.with(|d| d.borrow_mut().clear());
        // 英語の句 (製品名でない) は前のまま: 空白を抜いた形と、空白を含む英字の並び
        assert_eq!(undo_for_spaces("see you tomorrow", "seeyoutomorrow", &[3, 6], &[]), ("seeyoutomorrow".to_string(), vec!["seeyoutomorrow".to_string()]));
        TEST_PRODUCTS.with(|t| *t.borrow_mut() = None);
    }

    #[test]
    fn phrase_runs_are_the_letter_runs_around_the_spaces() {
        assert_eq!(phrase_runs("seeyoutomorrow,matane", &[3, 6]), vec!["seeyoutomorrow".to_string()]);
        assert_eq!(phrase_runs("ab", &[]), Vec::<String>::new());
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;

    /// 2026-11-08 (仕事 12): 英語の句の分け方を、配布物の表 (AZK_WORDS_DIR・AZK_NEXT_WORDS_DIR) で書き出す (AZK_PHRASE_PROBE を指定したときだけ。
    /// 1 行 1 入力、結果は <path>.out.tsv に 入力<TAB>分けた語 (空白区切り。分けなければ -))。t12/eval12.py の決め方と同じになるかを確かめる
    #[test]
    fn phrase_probe() {
        let Ok(path) = std::env::var("AZK_PHRASE_PROBE") else { return };
        let mut out = String::new();
        for raw in std::fs::read_to_string(&path).unwrap().lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (tokens, spaces) = segment_finishing_with_spaces(raw);
            let shown = if spaces.is_empty() {
                "-".to_string()
            } else {
                let mut text = String::new();
                for (at, c) in raw.char_indices() {
                    if spaces.contains(&at) { text.push(' '); }
                    text.push(c);
                }
                text
            };
            let _ = tokens;
            out.push_str(&format!("{raw}\t{shown}\n"));
        }
        std::fs::write(format!("{path}.out.tsv"), out).unwrap();
    }

    /// 2026-10-20: 試験用の打鍵の組を、配布物の表 (AZK_WORDS_DIR) で区切って書き出す (AZK_SEGMENT_PROBE を指定したときだけ。1 行 1 入力)
    #[test]
    fn segmentation_probe() {
        let Ok(path) = std::env::var("AZK_SEGMENT_PROBE") else { return };
        let show = |tokens: &[Token]| tokens.iter().map(|t| match t.kind {
            TokenKind::English => format!("[{}]", t.raw),
            TokenKind::Romaji => t.raw.clone(),
        }).collect::<Vec<_>>().join("|");
        let mut out = String::new();
        for raw in std::fs::read_to_string(&path).unwrap().lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (raw, expected) = raw.split_once('\t').unwrap_or((raw, ""));
            let typing = show(&segment(raw));
            let finishing = show(&segment_finishing(raw));
            let ok = expected.is_empty() || finishing == expected;
            out.push_str(&format!("{}\t{raw}\ttyping={typing}\tfinishing={finishing}\texpected={expected}\n", if ok { "OK" } else { "NG" }));
        }
        std::fs::write(format!("{path}.out.tsv"), out).unwrap();
    }

    /// 2026-10-28: 母音の抜けを直す候補 (vowel_repairs) を書き出す (AZK_REPAIR_PROBE に 1 行 1 入力。結果は <path>.out.tsv に 入力<TAB>位置:母音 …)
    /// 2026-11-04 (仕事 20): 3 列目に英字の区切りの中の候補 (vowel_repairs_in_english。組は ; で区切る)、4 列目にその時間 (マイクロ秒)
    #[test]
    fn vowel_repair_probe() {
        let Ok(path) = std::env::var("AZK_REPAIR_PROBE") else { return };
        let mut out = String::new();
        for raw in std::fs::read_to_string(&path).unwrap().lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let raw = raw.split('\t').next().unwrap();
            let repairs: Vec<String> = vowel_repairs(raw, true).iter().map(|(p, v)| format!("{p}:{v}")).collect();
            let started = std::time::Instant::now();
            let groups = vowel_repairs_in_english(raw);
            let micros = started.elapsed().as_micros();
            let groups: Vec<String> = groups.iter().map(|g| g.iter().map(|(p, v)| format!("{p}:{v}")).collect::<Vec<_>>().join(" ")).collect();
            out.push_str(&format!("{raw}\t{}\t{}\t{micros}\n", repairs.join(" "), groups.join(";")));
        }
        std::fs::write(format!("{path}.out.tsv"), out).unwrap();
    }

    /// 2026-10-24: 打鍵ごとの区切りの時間 (AZK_SEGMENT_TIMING に 1 行 1 入力。1 字打つたびに頭からその字までを segment で区切り、最後に segment_finishing。
    /// 結果は <path>.timing<AZK_TIMING_LABEL>.tsv。AZK_NO_UNKNOWN_RULE で表に無い語の規則の前と比べる)
    #[test]
    fn segmentation_timing() {
        let Ok(path) = std::env::var("AZK_SEGMENT_TIMING") else { return };
        let label = std::env::var("AZK_TIMING_LABEL").unwrap_or_default();
        let _ = segment_finishing("warmupdesu");
        let micros = |f: &dyn Fn()| { let t = std::time::Instant::now(); f(); t.elapsed().as_secs_f64() * 1e6 };
        let (mut out, mut keys, mut finishing) = (String::new(), vec![], vec![]);
        for raw in std::fs::read_to_string(&path).unwrap().lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let times: Vec<f64> = raw.char_indices().map(|(at, c)| micros(&|| { std::hint::black_box(segment(&raw[..at + c.len_utf8()])); })).collect();
            let fin = micros(&|| { std::hint::black_box(segment_finishing(raw)); });
            let max = times.iter().cloned().fold(0.0, f64::max);
            out += &format!("{}\t{:.0}\t{:.0}\t{:.0}\t{raw}\n", raw.len(), times.iter().sum::<f64>() / times.len() as f64, max, fin);
            keys.extend(times);
            finishing.push(fin);
        }
        let summary = |v: &mut Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let at = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
            format!("n={} mean={:.0} p50={:.0} p90={:.0} p99={:.0} max={:.0}", v.len(), v.iter().sum::<f64>() / v.len() as f64, at(0.5), at(0.9), at(0.99), at(1.0))
        };
        let head = format!("# 規則={} 打鍵ごと(µs): {}\n# 打ち終え(µs): {}\n# 長さ\t平均\t最大\t打ち終え\t入力\n",
            if unknown_word_rule() { "あり" } else { "なし" }, summary(&mut keys), summary(&mut finishing));
        std::fs::write(format!("{path}.timing{label}.tsv"), head + &out).unwrap();
    }
}
