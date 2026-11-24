// ローマ字の打ち方を、打った綴りから判断する (設定 input.romaji_style = "auto" のとき)。
//
// 日本語として確定した入力について、打った綴りと、それが変換サーバーでどのかなになったかを 1 音ずつ対応づけ、
// 音ごとに「どの綴りを選んだか」の回数だけを %APPDATA%\HaniWord\romaji_style_stats.json に残す (打った文章は残さない)。
// 対応づけは変換サーバーと同じローマ字表 (romaji_table.rs) で打鍵をかなに直し、サーバーの読みと一致したときだけ数える。
//
// 使い方は 2 通り:
// - 2 値の判断 (is_kunrei_nn): 訓令式の綴りがほとんどで、ん もほぼ nn の人を kunrei_nn とみなす (以前からの判断)
// - 音ごとの推定 (penalty): 音ごとにディリクレ分布 (2 択ならベータ分布) で「この人がこの綴りを使う割合」を推定し、
//   99% の上限でも十分に小さい綴り (= この人はまず使わない) だけを、英語の判定で「英語のしるし」として加点する
//   (2026-10-11 の 6astra の提案。review/astra_romaji_out.txt)
use super::romaji_table::ROMAJI_TABLE;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stats {
    /// ヘボン式だけの綴り (shi chi tsu ji sha … ja)。ふ (hu/fu) は混ぜて使う人が多いので数えない
    pub hepburn: u32,
    /// 訓令式だけの綴り (si ti tu zi sya … zya)
    pub kunrei: u32,
    /// ん を nn と打った回数 (後ろが子音のときだけ。母音・y の前の nn は誰でも打つので数えない)
    pub nn: u32,
    /// ん を n 1 つ (+子音) で打った回数
    pub n_single: u32,
    /// 音ごとの綴りの回数: 音 → 綴り → 回数 (例: "si" → {"si": 120, "shi": 3})。古い回数は少しずつ減らすので小数
    #[serde(default)]
    pub groups: BTreeMap<String, BTreeMap<String, f64>>,
    /// 音ごとの、最後に回数を足した日 (1970-01-01 からの日数)。日数による忘却に使う
    #[serde(default)]
    pub updated: BTreeMap<String, f64>,
}

/// 2 値の判断に要る最低の回数 (綴り・ん それぞれ)
const MIN_SAMPLES: u32 = 30;
/// 忘却の半減期: その音を新しく何回打ったら古い回数が半分になるか、何日で半分になるか (6astra の開始案。測って決めた値ではない)
const HALF_LIFE_COUNT: f64 = 200.0;
const HALF_LIFE_DAYS: f64 = 180.0;

impl Stats {
    /// 訓令式＋ん は nn の人か
    pub fn is_kunrei_nn(&self) -> bool {
        let spell = self.hepburn + self.kunrei;
        let n = self.nn + self.n_single;
        spell >= MIN_SAMPLES
            && n >= MIN_SAMPLES
            && self.kunrei as f64 >= spell as f64 * 0.95
            && self.nn as f64 >= n as f64 * 0.95
    }

    /// 回数を足す (忘却なし。ベンチ・テスト用)
    pub fn add(&mut self, other: &Stats) {
        self.hepburn += other.hepburn;
        self.kunrei += other.kunrei;
        self.nn += other.nn;
        self.n_single += other.n_single;
        for (group, variants) in &other.groups {
            let mine = self.groups.entry(group.clone()).or_default();
            for (variant, count) in variants {
                *mine.entry(variant.clone()).or_default() += count;
            }
        }
    }

    /// 回数を足す。音ごとの回数は、足す前に古い分を減らす: その音を新しく打った回数 k と、前に足してからの日数 d で
    /// 2^(-k/HALF_LIFE_COUNT - d/HALF_LIFE_DAYS) 倍 (打ち方を変えた人・家族で使う PC に追いつくため。事前の割合は減らさない)
    pub fn add_with_decay(&mut self, other: &Stats, today: f64) {
        self.hepburn += other.hepburn;
        self.kunrei += other.kunrei;
        self.nn += other.nn;
        self.n_single += other.n_single;
        for (group, variants) in &other.groups {
            let new: f64 = variants.values().sum();
            let days = self.updated.get(group).map_or(0.0, |last| (today - last).max(0.0));
            let factor = (2f64).powf(-new / HALF_LIFE_COUNT - days / HALF_LIFE_DAYS);
            let mine = self.groups.entry(group.clone()).or_default();
            for count in mine.values_mut() {
                *count *= factor;
            }
            for (variant, count) in variants {
                *mine.entry(variant.clone()).or_default() += count;
            }
            self.updated.insert(group.clone(), today);
        }
    }

    fn note(&mut self, group: &str, variant: &str) {
        *self.groups.entry(group.to_string()).or_default().entry(variant.to_string()).or_default() += 1.0;
    }

    fn is_empty(&self) -> bool {
        *self == Stats::default()
    }
}

// ---- 打鍵をかなに直す (変換サーバーの Roman2Kana.toHiragana と同じ手順) ----

/// かなの 1 まとまり (または、まだかなになっていない英字 1 字) と、それを作った打鍵
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub kana: String,
    pub keys: String,
    /// まだかなになっていない英字 (打ちかけ、またはローマ字表にない文字)
    pub pending: bool,
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| ROMAJI_TABLE.iter().copied().collect())
}

/// 打った文字の列を、変換サーバーと同じ規則でかなに直す (打鍵とかなの対応つき)
pub fn simulate(typed: &str) -> Vec<Unit> {
    let mut units: Vec<Unit> = vec![];
    for added in typed.chars().map(|c| c.to_ascii_lowercase()) {
        // 末尾のまだかなになっていない英字 (最大 3 字) + 今の 1 字 がローマ字表にあればかなにする (長いほうから)
        let trailing = units.iter().rev().take_while(|u| u.pending && u.kana.is_ascii()).count();
        let mut done = false;
        for k in (1..=trailing.min(3)).rev() {
            let start = units.len() - k;
            let key: String = units[start..].iter().map(|u| u.kana.as_str()).chain(std::iter::once(added.encode_utf8(&mut [0; 4]) as &str)).collect();
            if let Some(kana) = table().get(key.as_str()) {
                let keys: String = units[start..].iter().map(|u| u.keys.as_str()).collect::<String>() + &added.to_string();
                units.truncate(start);
                units.push(Unit { kana: kana.to_string(), keys, pending: false });
                done = true;
                break;
            }
        }
        if done {
            continue;
        }
        let last = units.last().filter(|u| u.pending).map(|u| u.kana.clone());
        // t + ch は っ + ch (matcha = まっちゃ。変換サーバーの Roman2Kana と同じ。2026-10-13)
        if added == 'h' && last.as_deref() == Some("c") && units.len() >= 2
            && units[units.len() - 2].pending && units[units.len() - 2].kana == "t" {
            let at = units.len() - 2;
            units[at] = Unit { kana: "っ".into(), keys: "t".into(), pending: false };
            units.push(Unit { kana: added.to_string(), keys: added.to_string(), pending: true });
            continue;
        }
        // 同じ子音の重ね (kk) は っ
        if last.as_deref() == Some(added.to_string().as_str()) && added.is_ascii_alphabetic() {
            let unit = units.last_mut().unwrap();
            *unit = Unit { kana: "っ".into(), keys: added.to_string(), pending: false };
            units.push(Unit { kana: added.to_string(), keys: added.to_string(), pending: true });
            continue;
        }
        // n + (y 以外) は ん
        if last.as_deref() == Some("n") && added != 'y' {
            let unit = units.last_mut().unwrap();
            *unit = Unit { kana: "ん".into(), keys: "n".into(), pending: false };
            units.push(Unit { kana: added.to_string(), keys: added.to_string(), pending: true });
            continue;
        }
        match table().get(added.to_string().as_str()) {
            Some(kana) => units.push(Unit { kana: kana.to_string(), keys: added.to_string(), pending: false }),
            None => units.push(Unit { kana: added.to_string(), keys: added.to_string(), pending: true }),
        }
    }
    units
}

/// 読みを比べるための骨組み: ひらがな (カタカナはひらがなに) と英字 (小文字) だけ
fn skeleton(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            'ぁ'..='ゖ' => Some(c),
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60),
            'a'..='z' => Some(c),
            'A'..='Z' => Some(c.to_ascii_lowercase()),
            'ａ'..='ｚ' | 'Ａ'..='Ｚ' => char::from_u32(c as u32 - 0xFEE0).map(|c| c.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

// ---- 音ごとの綴りの分類 ----

const SMALL: &str = "ぁぃぅぇぉゃゅょっゎゕゖ";

/// かな 1 まとまりと打鍵から (音, 綴り)。綴りの選び方に意味のない音は None
fn classify(kana: &str, keys: &str) -> Option<(&'static str, &'static str)> {
    let pick = |pairs: &[(&'static str, &'static str)]| pairs.iter().find(|(k, _)| keys == *k).map(|(_, v)| *v);
    let group_and = |group: &'static str, variant: Option<&'static str>| variant.map(|v| (group, v));
    match kana {
        "し" => group_and("si", pick(&[("si", "si"), ("shi", "shi"), ("ci", "ci")])),
        // 2026-10-20: c 行 (か く こ せ)。英語の判定で、ふだん c 行を使わない人の ca cu co ce を英語のしるしにする
        "か" => group_and("ka", pick(&[("ka", "ka"), ("ca", "ca")])),
        "く" => group_and("ku", pick(&[("ku", "ku"), ("cu", "cu")])),
        "こ" => group_and("ko", pick(&[("ko", "ko"), ("co", "co")])),
        "せ" => group_and("se", pick(&[("se", "se"), ("ce", "ce")])),
        "ち" => group_and("ti", pick(&[("ti", "ti"), ("chi", "chi")])),
        "つ" => group_and("tu", pick(&[("tu", "tu"), ("tsu", "tsu")])),
        "じ" => group_and("zi", pick(&[("zi", "zi"), ("ji", "ji")])),
        "ふ" => group_and("hu", pick(&[("hu", "hu"), ("fu", "fu")])),
        "しゃ" | "しゅ" | "しょ" => group_and("sya", prefix_variant(keys, &[("sy", "sy"), ("sh", "sh")])),
        "しぇ" => group_and("sye", prefix_variant(keys, &[("sy", "sy"), ("sh", "sh")])),
        "ちゃ" | "ちゅ" | "ちょ" => group_and("tya", prefix_variant(keys, &[("ty", "ty"), ("ch", "ch"), ("cy", "cy")])),
        "ちぇ" => group_and("tye", prefix_variant(keys, &[("ty", "ty"), ("ch", "ch"), ("cy", "cy")])),
        "じゃ" | "じゅ" | "じょ" => group_and("zya", prefix_variant(keys, &[("zy", "zy"), ("jy", "jy"), ("j", "j")])),
        "じぇ" => group_and("zye", prefix_variant(keys, &[("zy", "zy"), ("jy", "jy"), ("j", "j")])),
        "ふぁ" | "ふぃ" | "ふぇ" | "ふぉ" => group_and("fa", prefix_variant(keys, &[("f", "f"), ("hw", "hw")])),
        _ if SMALL.contains(kana) && kana.chars().count() == 1 => {
            group_and("small", prefix_variant(keys, &[("x", "x"), ("l", "l")]))
        }
        _ => None,
    }
}

fn prefix_variant(keys: &str, pairs: &[(&'static str, &'static str)]) -> Option<&'static str> {
    pairs.iter().find(|(p, _)| keys.starts_with(p)).map(|(_, v)| *v)
}

/// 大きいかな + 小さいかな に分けて打った音 (si + xya = しゃ) の音
fn split_group(big: &str, small: &str) -> Option<&'static str> {
    match (big, small) {
        ("し", "ゃ" | "ゅ" | "ょ") => Some("sya"),
        ("し", "ぇ") => Some("sye"),
        ("ち", "ゃ" | "ゅ" | "ょ") => Some("tya"),
        ("ち", "ぇ") => Some("tye"),
        ("じ", "ゃ" | "ゅ" | "ょ") => Some("zya"),
        ("じ", "ぇ") => Some("zye"),
        ("ふ", "ぁ" | "ぃ" | "ぇ" | "ぉ") => Some("fa"),
        _ => None,
    }
}

/// かなに直した列から、音ごとの綴りの回数を数える
fn count_units(units: &[Unit]) -> Stats {
    let mut stats = Stats::default();
    for (i, unit) in units.iter().enumerate() {
        if unit.pending {
            continue;
        }
        let next = units.get(i + 1);
        if unit.kana == "ん" {
            // n 1 つ (n + 子音) と、子音の前の nn だけが選んだ綴り。母音・y の前は nn が要る。最後の ん (n で止めて確定) は数えない
            let next_consonant = next
                .and_then(|u| u.keys.chars().next())
                .is_some_and(|c| c.is_ascii_alphabetic() && !"aiueoyn".contains(c));
            if next_consonant {
                match unit.keys.as_str() {
                    "n" => {
                        stats.n_single += 1;
                        stats.note("n", "n");
                    }
                    "nn" => {
                        stats.nn += 1;
                        stats.note("n", "nn");
                    }
                    _ => {}
                }
            }
            continue;
        }
        // 重ねの っ (kk の k) は選んだ綴りではない
        if unit.kana == "っ" && unit.keys.len() == 1 {
            continue;
        }
        if let Some((group, variant)) = classify(&unit.kana, &unit.keys) {
            stats.note(group, variant);
            let legacy_hepburn = matches!((group, variant), ("si", "shi") | ("ti", "chi") | ("tu", "tsu") | ("zi", "ji"))
                || (matches!(group, "sya" | "sye") && variant == "sh")
                || (matches!(group, "tya" | "tye") && variant == "ch")
                || (matches!(group, "zya" | "zye") && variant == "j");
            let legacy_kunrei = matches!((group, variant), ("si", "si") | ("ti", "ti") | ("tu", "tu") | ("zi", "zi"))
                || (matches!(group, "sya" | "sye") && variant == "sy")
                || (matches!(group, "tya" | "tye") && variant == "ty")
                || (matches!(group, "zya" | "zye") && variant == "zy");
            if legacy_hepburn {
                stats.hepburn += 1;
            }
            if legacy_kunrei {
                stats.kunrei += 1;
            }
        }
        // 分けて打った拗音 (si + xya)
        if let Some(group) = next.filter(|u| !u.pending).and_then(|u| split_group(&unit.kana, &u.kana)) {
            stats.note(group, "split");
        }
    }
    stats
}

/// 打った文字の列をすべてローマ字として数える (かなとの対応は確かめない。テスト・ベンチ用)
pub fn count(typed: &str) -> Stats {
    count_units(&simulate(typed))
}

/// 確定した入力の回数を数える。`typed` は打った文字の列、`reading` はサーバーの読み (英語と判定した部分は英字のまま)。
/// `english` は英語と判定した区切り (英語の部分は数えない)。打鍵から作ったかなが読みと一致しないときは数えない
pub fn count_commit(typed: &str, reading: &str, english: Option<&[super::auto_english::Token]>) -> Stats {
    use super::auto_english::TokenKind;
    let target = skeleton(reading);
    // 1. 英語と判定した区切りのとおり (英語の部分は英字のまま)
    if let Some(tokens) = english {
        let mut expected = String::new();
        let mut stats = Stats::default();
        let mut romaji = String::new();
        let flush = |romaji: &mut String, expected: &mut String, stats: &mut Stats| {
            if !romaji.is_empty() {
                let units = simulate(romaji);
                expected.push_str(&skeleton(&units.iter().map(|u| u.kana.as_str()).collect::<String>()));
                stats.add(&count_units(&units));
                romaji.clear();
            }
        };
        for token in tokens {
            match token.kind {
                TokenKind::Romaji => romaji.push_str(&token.raw),
                TokenKind::English => {
                    flush(&mut romaji, &mut expected, &mut stats);
                    expected.push_str(&skeleton(&token.raw));
                }
            }
        }
        flush(&mut romaji, &mut expected, &mut stats);
        if expected == target {
            return stats;
        }
    }
    // 2. すべてローマ字 (英語と判定しなかった入力、または利用者が英語の判定をかなに直した入力)
    let units = simulate(typed);
    if skeleton(&units.iter().map(|u| u.kana.as_str()).collect::<String>()) == target {
        return count_units(&units);
    }
    Stats::default()
}

// ---- 保存 ----

static CACHE: Mutex<Option<(Option<std::time::SystemTime>, Stats)>> = Mutex::new(None);

fn path() -> Option<std::path::PathBuf> {
    if !shared::winsec::private_data_allowed() { return None; }
    if cfg!(test) {
        return None;
    }
    Some(shared::app_data_dir()?.join("romaji_style_stats.json"))
}

fn read_file(path: &std::path::Path) -> Stats {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// 今までの回数 (ほかのアプリで増えた分も読むため、ファイルが変わっていたら読み直す)
pub fn load() -> Stats {
    let Some(path) = path() else { return Stats::default() };
    let modified = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((m, stats)) = guard.as_ref() {
        if *m == modified {
            return stats.clone();
        }
    }
    let stats = read_file(&path);
    *guard = Some((modified, stats.clone()));
    stats
}

/// 同じファイルをいくつものアプリ (IME はアプリごとに読み込まれる) が同時に書き換えないための名前つきミューテックス
struct FileLock(windows::Win32::Foundation::HANDLE);

impl FileLock {
    fn acquire() -> Option<FileLock> {
        use windows::core::w;
        use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
        let handle = unsafe { CreateMutexW(None, false, w!("Local\\azookey_romaji_style_stats")) }.ok()?;
        // WAIT_OBJECT_0 = 0、WAIT_ABANDONED = 0x80 (前の持ち主が落ちた。中身は読み直すので使ってよい)
        let result = unsafe { WaitForSingleObject(handle, 500) }.0;
        if result == 0 || result == 0x80 {
            Some(FileLock(handle))
        } else {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
            None
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Threading::ReleaseMutex(self.0);
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// 確定した入力の回数を足す (count_commit の結果)
pub fn record(add: &Stats) {
    if add.is_empty() {
        return;
    }
    let Some(path) = path() else { return };
    let Some(_lock) = FileLock::acquire() else { return };
    let mut stats = read_file(&path);
    let today = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() / 86400.0);
    stats.add_with_decay(add, today);
    if let Ok(text) = serde_json::to_string(&stats) {
        // 書きかけのファイルを読まれないよう、別名に書いてから置き換える
        let temp = path.with_extension("json.tmp");
        if std::fs::write(&temp, text).is_ok() {
            let _ = std::fs::rename(&temp, &path);
        }
    }
}

// ---- 音ごとの推定 ----

/// 音ごとの、まだ打っていない人の初期の割合 (綴り, 割合)。全国の利用率ではなく、弱い出発点 (擬似回数は KAPPA 回分)。
/// 佐田 2017 (大学生 150 人) の向きだけを弱く入れた 6astra の提案値。表にない綴り (ci・hw・split など) は SMALL_PRIOR を分ける
const PRIORS: &[(&str, &[(&str, f64)])] = &[
    ("si", &[("si", 0.5), ("shi", 0.5)]),
    ("ti", &[("ti", 0.8), ("chi", 0.2)]),
    ("tu", &[("tu", 0.8), ("tsu", 0.2)]),
    ("zi", &[("zi", 0.5), ("ji", 0.5)]),
    ("hu", &[("hu", 0.5), ("fu", 0.5)]),
    ("sya", &[("sy", 0.5), ("sh", 0.5)]),
    ("sye", &[("sy", 0.5), ("sh", 0.5)]),
    ("tya", &[("ty", 0.6), ("ch", 0.3), ("cy", 0.1)]),
    ("tye", &[("ty", 0.6), ("ch", 0.3), ("cy", 0.1)]),
    ("zya", &[("j", 0.45), ("zy", 0.35), ("jy", 0.2)]),
    ("zye", &[("j", 0.45), ("zy", 0.35), ("jy", 0.2)]),
    ("n", &[("n", 0.5), ("nn", 0.5)]),
    ("small", &[("x", 0.5), ("l", 0.5)]),
    ("fa", &[("f", 0.8), ("split", 0.2)]),
    // 2026-10-20: c 行。初めは ka ku ko se だけを置き、ca cu co ce は表にない綴りと同じ擬似回数 (SMALL_PRIOR)。
    // 打っていない人には加点しない (入れたての人の cansoku = かんそく)。ka を 20 回ほど打つと ca の加点が 1 を超える
    ("ka", &[("ka", 1.0)]),
    ("ku", &[("ku", 1.0)]),
    ("ko", &[("ko", 1.0)]),
    ("se", &[("se", 1.0)]),
];
/// 初期の割合の重み (擬似回数)。20 回打てば初期値の影響は 1 割を切る
const KAPPA: f64 = 2.0;
/// 表にない綴りの擬似回数
const SMALL_PRIOR: f64 = 0.1;
/// 使う割合の 99% 上限がこれ以上の綴りには加点しない
const TAU: f64 = 0.20;
/// 加点の強さと上限 (英単語のコストが 1)
const LAMBDA: f64 = 1.0;
const MAX_PENALTY: f64 = 2.0;

/// この人が、音 `group` を綴り `variant` で打つ割合の 99% 上限 (事後分布はベータ分布で近似)
pub fn upper_rate(stats: &Stats, group: &str, variant: &str) -> f64 {
    let prior: &[(&str, f64)] = PRIORS.iter().find(|(g, _)| *g == group).map(|(_, p)| *p).unwrap_or(&[]);
    let counts = stats.groups.get(group);
    let mut names: Vec<&str> = prior.iter().map(|(v, _)| *v).collect();
    if let Some(c) = counts {
        for v in c.keys() {
            if !names.contains(&v.as_str()) {
                names.push(v);
            }
        }
    }
    if !names.contains(&variant) {
        names.push(variant);
    }
    let alpha_of = |v: &str| {
        let p = prior.iter().find(|(name, _)| *name == v).map(|(_, p)| KAPPA * p).unwrap_or(SMALL_PRIOR);
        p + counts.and_then(|c| c.get(v)).copied().unwrap_or(0.0)
    };
    let a = alpha_of(variant);
    let b: f64 = names.iter().filter(|v| **v != variant).map(|v| alpha_of(v)).sum();
    beta_quantile(0.99, a, b)
}

/// 英語の判定で、音 `group` を綴り `variant` でローマ字として読むときに足すコスト (0 〜 MAX_PENALTY)
pub fn penalty(stats: &Stats, group: &str, variant: &str) -> f64 {
    let upper = upper_rate(stats, group, variant);
    (LAMBDA * (TAU / upper).ln()).clamp(0.0, MAX_PENALTY)
}

/// 読みをローマ字に戻すとき (崩れた読みの英字候補など) の綴り。音ごとに訓令式かヘボン式か、ん を nn にするか
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spelling {
    /// 訓令式で書く音 (si ti tu zi hu sya sye tya tye zya zye)。入っていない音はヘボン式
    pub kunrei: Vec<&'static str>,
    /// ん をいつも nn と書く (と見せる)
    pub nn: bool,
}

/// 表示を選ぶ音: (音, 訓令式の綴り, ヘボン式の綴り, 回数が少ないときに借りる音)
const DISPLAY_GROUPS: &[(&str, &str, &str, &str)] = &[
    ("si", "si", "shi", ""), ("ti", "ti", "chi", ""), ("tu", "tu", "tsu", ""), ("zi", "zi", "ji", ""), ("hu", "hu", "fu", ""),
    ("sya", "sy", "sh", ""), ("sye", "sy", "sh", "sya"), ("tya", "ty", "ch", ""), ("tye", "ty", "ch", "tya"),
    ("zya", "zy", "j", ""), ("zye", "zy", "j", "zya"),
];

impl Spelling {
    /// どの音も同じ式 (設定で kunrei_nn / standard を選んだ人)
    pub fn uniform(kunrei_nn: bool) -> Spelling {
        Spelling { kunrei: if kunrei_nn { DISPLAY_GROUPS.iter().map(|g| g.0).collect() } else { vec![] }, nn: kunrei_nn }
    }

    pub fn is_kunrei(&self, group: &str) -> bool {
        self.kunrei.contains(&group)
    }
}

/// 回数から、音ごとの表示の綴りを選ぶ。初めはヘボン式・ん は n で、その人がもう一方をよく使う (使う割合が半分を超える) と
/// 90% 以上の確からしさで言えたら切り替える (6astra の開始案)
pub fn spelling(stats: &Stats) -> Spelling {
    let count = |group: &str, variant: &str| stats.groups.get(group).and_then(|c| c.get(variant)).copied().unwrap_or(0.0);
    let total = |group: &str| stats.groups.get(group).map_or(0.0, |c| c.values().sum());
    // a 回と b 回のとき、a の側の割合が半分を超える確からしさ (一様な事前分布)
    let more_likely = |a: f64, b: f64| 1.0 - incomplete_beta(0.5, a + 1.0, b + 1.0);
    let mut kunrei = vec![];
    for (group, k, h, borrow) in DISPLAY_GROUPS {
        let source = if total(group) < 5.0 && !borrow.is_empty() { borrow } else { group };
        if more_likely(count(source, k), count(source, h)) > 0.9 {
            kunrei.push(*group);
        }
    }
    Spelling { kunrei, nn: more_likely(count("n", "nn"), count("n", "n")) > 0.9 }
}

/// ベータ分布の下側 p 点 (二分法)
fn beta_quantile(p: f64, a: f64, b: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if incomplete_beta(mid, a, b) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

/// 正則化不完全ベータ関数 I_x(a, b) (連分数展開)
fn incomplete_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let ln_front = ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln();
    if x < (a + 1.0) / (a + b + 2.0) {
        ln_front.exp() * beta_fraction(x, a, b) / a
    } else {
        1.0 - ln_front.exp() * beta_fraction(1.0 - x, b, a) / b
    }
}

fn beta_fraction(x: f64, a: f64, b: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..300 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-12 {
            break;
        }
    }
    h
}

/// ガンマ関数の対数 (Lanczos 近似)
fn ln_gamma(x: f64) -> f64 {
    const G: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        return (std::f64::consts::PI / (std::f64::consts::PI * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut sum = G[0];
    for (i, g) in G.iter().enumerate().skip(1) {
        sum += g / (x + i as f64);
    }
    let t = x + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + sum.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kana(typed: &str) -> String {
        simulate(typed).iter().map(|u| u.kana.as_str()).collect()
    }

    #[test]
    fn simulates_like_the_server() {
        assert_eq!(kana("konnnichiha"), "こんにちは");
        assert_eq!(kana("sinbun"), "しんぶn");
        assert_eq!(kana("kitte"), "きって");
        assert_eq!(kana("mittsu"), "みっつ");
        assert_eq!(kana("xtsu"), "っ");
        assert_eq!(kana("kya"), "きゃ");
        assert_eq!(kana("sixya"), "しゃ"); // xya が先に ゃ になり、si と合わさらない
        assert_eq!(kana("tenpura"), "てんぷら");
    }

    #[test]
    fn counts() {
        let s = count("sinnbunnwoyomu");
        assert_eq!((s.kunrei, s.nn, s.n_single), (1, 2, 0));
        let s = count("shinbunwoyomu");
        assert_eq!((s.hepburn, s.nn, s.n_single), (1, 0, 2));
        let s = count("tukatte");
        assert_eq!(s.kunrei, 1);
        // 小さい っ は つ の綴りではない
        let s = count("xtsu");
        assert_eq!((s.hepburn, s.kunrei), (0, 0));
        assert_eq!(s.groups["small"]["x"], 1.0);
        // っ の重ね (ttsu) は つ = tsu の 1 回
        let s = count("mittsu");
        assert_eq!(s.groups["tu"]["tsu"], 1.0);
        // jya は じゃ行の jy
        let s = count("jyanai");
        assert_eq!(s.groups["zya"]["jy"], 1.0);
        // ふ は 2 値の判断には数えない
        let s = count("fune");
        assert_eq!((s.hepburn, s.kunrei, s.groups["hu"]["fu"]), (0, 0, 1.0));
        // 母音の前の nn は数えない
        let s = count("konnnichiha");
        assert_eq!((s.nn, s.n_single), (0, 0));
        // 分けて打った拗音
        let s = count("sixyakai");
        assert_eq!(s.groups["sya"]["split"], 1.0);
    }

    #[test]
    fn commit_must_match_reading() {
        use crate::engine::auto_english::{Token, TokenKind};
        assert_eq!(count_commit("sitteiru", "しっている", None).kunrei, 1);
        // 読みと合わなければ数えない
        assert!(count_commit("sitteiru", "しっていr", None).is_empty());
        // 英語の部分は数えず、日本語の部分だけ数える
        let tokens = vec![
            Token { raw: "anki".into(), kind: TokenKind::English },
            Token { raw: "wositeiru".into(), kind: TokenKind::Romaji },
        ];
        let s = count_commit("ankiwositeiru", "ankiをしている", Some(&tokens));
        assert_eq!((s.kunrei, s.n_single), (1, 0));
        // 英語の判定をかなに直した (読みがすべてかな)
        let s = count_commit("ankiwositeiru", "あんきをしている", Some(&tokens));
        assert_eq!((s.kunrei, s.n_single), (1, 1));
    }

    #[test]
    fn estimates() {
        // 6astra の表: 一様な初期値で si を 30 回 → shi の 99% 上限は約 13.8%、加点は約 0.37
        let mut s = Stats::default();
        s.groups.entry("si".into()).or_default().insert("si".into(), 30.0);
        let u = upper_rate(&s, "si", "shi");
        assert!((u - 0.138).abs() < 0.002, "{u}");
        let p = penalty(&s, "si", "shi");
        assert!((p - 0.37).abs() < 0.02, "{p}");
        assert_eq!(penalty(&s, "si", "si"), 0.0);
        // まだ打っていない人には加点しない
        assert_eq!(penalty(&Stats::default(), "si", "shi"), 0.0);
        assert_eq!(penalty(&Stats::default(), "ti", "chi"), 0.0);
        s.groups.get_mut("si").unwrap().insert("si".into(), 200.0);
        assert!((penalty(&s, "si", "shi") - 2.0).abs() < 1e-9);
    }

    #[test]
    fn decay() {
        let mut s = Stats::default();
        let one = count("si");
        // 同じ日に 200 回打つと、最初の回数は約半分
        s.add_with_decay(&count("ti"), 100.0);
        for _ in 0..200 {
            s.add_with_decay(&count("ti"), 100.0);
        }
        assert!(s.groups["ti"]["ti"] < 200.0 && s.groups["ti"]["ti"] > 140.0, "{}", s.groups["ti"]["ti"]);
        // 180 日あけると半分になってから足される
        let mut s = Stats::default();
        s.add_with_decay(&one, 0.0);
        s.add_with_decay(&one, 180.0);
        assert!((s.groups["si"]["si"] - (0.5 * 2f64.powf(-1.0 / 200.0) + 1.0)).abs() < 1e-9);
        // ほかの音を打っても、この音の回数は減らない
        s.add_with_decay(&count("tu".repeat(500).as_str()), 180.0);
        assert!(s.groups["si"]["si"] > 1.4);
    }

    #[test]
    fn display_spelling() {
        assert_eq!(spelling(&Stats::default()), Spelling { kunrei: vec![], nn: false });
        let k = count(&"sitatukatteziyuunisyabettannmonohutatu".repeat(10));
        let sp = spelling(&k);
        assert!(sp.nn && sp.is_kunrei("si") && sp.is_kunrei("tu") && sp.is_kunrei("sye") && sp.is_kunrei("hu") && !sp.is_kunrei("ti"));
        // si と chi を混ぜる人
        let m = count(&"sitachikaranisinbun".repeat(10));
        let sp = spelling(&m);
        assert!(sp.is_kunrei("si") && !sp.is_kunrei("ti") && !sp.nn);
    }

    #[test]
    fn decision() {
        let mut s = Stats { kunrei: 40, hepburn: 0, nn: 40, n_single: 1, ..Default::default() };
        assert!(s.is_kunrei_nn());
        s.hepburn = 10;
        assert!(!s.is_kunrei_nn());
        assert!(!Stats { kunrei: 5, hepburn: 0, nn: 5, n_single: 0, ..Default::default() }.is_kunrei_nn());
    }
}
