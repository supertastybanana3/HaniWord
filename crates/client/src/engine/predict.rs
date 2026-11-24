//! Phrase completions from local history, followed by Windows predictions.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const HISTORY_LIMIT: usize = 20_000;
const PREDICTION_LIMIT: usize = 6;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Phrase {
    reading: String,
    text: String,
    count: u64,
    last_used: u64,
    first_order: u64, // Zero for legacy entries whose original creation order is unknown.
}

struct History {
    path: PathBuf,
    phrases: Vec<Phrase>,
}

impl History {
    fn load(path: PathBuf) -> io::Result<Self> {
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        Ok(Self {
            path,
            phrases: parse_phrases(&contents),
        })
    }

    fn remember(&mut self, reading: &str, text: &str, now: u64) -> io::Result<()> {
        record_phrase(&mut self.phrases, reading, text, now);
        save_phrases(&self.path, &self.phrases)
    }

    /// 読みに reading、表記に text を含む控えを消す。消したものがあれば書き直して true
    fn forget(&mut self, reading: &str, text: &str) -> io::Result<bool> {
        let before = self.phrases.len();
        self.phrases.retain(|phrase| !(phrase.reading.contains(reading) && phrase.text.contains(text)));
        if self.phrases.len() == before {
            return Ok(false);
        }
        save_phrases(&self.path, &self.phrases)?;
        Ok(true)
    }
}

thread_local! {
    static HISTORY: RefCell<Option<History>> = const { RefCell::new(None) };
    /// 2026-11-08: 予測の候補を読んだときの、学習をまっさらにした印 (auto_english::learning_reset_mark)。変わっていたら読み直す
    /// (読み直さないと、まっさらにする前の候補をメモリから書き戻していた)
    static HISTORY_RESET_MARK: std::cell::Cell<Option<Option<SystemTime>>> = const { std::cell::Cell::new(None) };
}

fn with_history<T>(use_history: impl FnOnce(&mut History) -> T) -> Option<T> {
    if !shared::winsec::private_data_allowed() { return None; }
    let mark = super::auto_english::learning_reset_mark();
    HISTORY.with(|cached| {
        let mut cached = cached.borrow_mut();
        if HISTORY_RESET_MARK.with(|loaded| loaded.get()) != Some(mark) {
            *cached = None;
        }
        if cached.is_none() {
            let path = super::auto_english::data_dir()?.join("phrases.tsv");
            match History::load(path) {
                Ok(history) => {
                    *cached = Some(history);
                    HISTORY_RESET_MARK.with(|loaded| loaded.set(Some(mark)));
                }
                Err(error) => {
                    log::warn!("Cannot read prediction history: {error}");
                    return None;
                }
            }
        }
        cached.as_mut().map(use_history)
    })
}

fn valid_reading(reading: &str, minimum: usize) -> bool {
    reading.chars().count() >= minimum
        && reading
            .chars()
            .all(|ch| ('\u{3041}'..='\u{3096}').contains(&ch) || ch == '\u{30fc}')
}

fn memorable(reading: &str, text: &str) -> bool {
    valid_reading(reading, 3)
        && text
            .chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch) || ch == '\u{3005}')
        && !text.contains(['\t', '\n', '\r'])
}

pub fn remember(reading: &str, text: &str) {
    // 2026-11-05 (仕事 27): 予測候補を使わない設定 (初期値) では、打った言い回しを履歴に残さない
    if !enabled() || !memorable(reading, text) {
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    with_history(|history| {
        if let Err(error) = history.remember(reading, text, now) {
            log::warn!("Cannot save prediction history: {error}");
        }
    });
}

/// 2026-10-29: 候補の一覧で忘れた学習の語 (Ctrl+Delete) を含む控えも消す (予測の候補に残らないように)。
/// 試験のビルドでは何もしない (控えの置き場が本物の APPDATA か、試験の表の置き場になるため)
pub fn forget(reading: &str, text: &str) {
    if cfg!(test) || reading.is_empty() || text.is_empty() {
        return;
    }
    with_history(|history| {
        if let Err(error) = history.forget(reading, text) {
            log::warn!("Cannot save prediction history: {error}");
        }
    });
}

/// 2026-11-05 (仕事 27): 打っている間に日本語の予測候補を出すか (設定 input.japanese_predictions、初期値はオフ)。
/// 前はライブ変換でない入力でいつも出していた。試験の中では出す (今までの試験のため)
pub fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| cfg!(test) || shared::AppConfig::read_or_default().input.japanese_predictions)
}

pub fn predict(reading: &str, conversions: &[String]) -> Vec<String> {
    if !enabled() {
        return vec![];
    }
    if !valid_reading(reading, 2) {
        return Vec::new();
    }
    let learned =
        with_history(|history| learned_candidates(reading, &history.phrases)).unwrap_or_default();
    complete_predictions(
        reading,
        conversions,
        learned,
        super::windows_text::predictions,
    )
}

fn parse_phrases(contents: &str) -> Vec<Phrase> {
    let mut unique = HashMap::new();
    for line in contents.lines() {
        let mut columns = line.split('\t');
        let (Some(reading), Some(text), Some(count), Some(last_used)) = (
            columns.next(),
            columns.next(),
            columns.next(),
            columns.next(),
        ) else {
            continue;
        };
        if !memorable(reading, text) { continue; }
        unique.insert(
            (reading.to_owned(), text.to_owned()),
            Phrase {
                reading: reading.to_owned(),
                text: text.to_owned(),
                count: count.parse().unwrap_or(1),
                last_used: last_used.parse().unwrap_or(0),
                first_order: columns.next().and_then(|value| value.parse().ok()).unwrap_or(0),
            },
        );
    }
    unique.into_values().collect()
}

fn record_phrase(phrases: &mut Vec<Phrase>, reading: &str, text: &str, now: u64) {
    if let Some(phrase) = phrases
        .iter_mut()
        .find(|phrase| phrase.reading == reading && phrase.text == text)
    {
        phrase.count = phrase.count.saturating_add(1);
        phrase.last_used = now;
    } else {
        let first_order = phrases.iter().map(|phrase| phrase.first_order).max().unwrap_or(0).saturating_add(1);
        phrases.push(Phrase {
            reading: reading.to_owned(),
            text: text.to_owned(),
            count: 1,
            last_used: now,
            first_order,
        });
    }
    phrases.sort_unstable_by(|left, right| {
        right
            .last_used
            .cmp(&left.last_used)
            .then_with(|| left.reading.cmp(&right.reading))
            .then_with(|| left.text.cmp(&right.text))
    });
    phrases.truncate(HISTORY_LIMIT);
}

fn save_phrases(path: &Path, phrases: &[Phrase]) -> io::Result<()> {
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "History needs a parent directory",
        )
    })?;
    fs::create_dir_all(directory)?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!("phrases.{}.{}.tmp", std::process::id(), sequence));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| -> io::Result<()> {
        for phrase in phrases {
            writeln!(
                file,
                "{}\t{}\t{}\t{}\t{}",
                phrase.reading, phrase.text, phrase.count, phrase.last_used, phrase.first_order
            )?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn learned_candidates(reading: &str, phrases: &[Phrase]) -> Vec<String> {
    let mut matches: Vec<_> = phrases
        .iter()
        .filter(|phrase| phrase.reading.starts_with(reading) && phrase.reading != reading)
        .collect();
    matches.sort_unstable_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.first_order.cmp(&right.first_order))
            .then_with(|| left.text.cmp(&right.text))
            .then_with(|| left.reading.cmp(&right.reading))
    });
    matches
        .into_iter()
        .map(|phrase| phrase.text.clone())
        .collect()
}

fn complete_predictions(
    reading: &str,
    conversions: &[String],
    learned: Vec<String>,
    windows_predictions: impl FnOnce(&str) -> Vec<String>,
) -> Vec<String> {
    if !valid_reading(reading, 2) {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for text in learned {
        if seen.insert(text.clone()) {
            result.push(text);
            if result.len() == PREDICTION_LIMIT {
                return result;
            }
        }
    }
    for text in windows_predictions(reading) {
        if text.chars().count() < 2 || text == reading || conversions.contains(&text) {
            continue;
        }
        if seen.insert(text.clone()) {
            result.push(text);
            if result.len() == PREDICTION_LIMIT {
                break;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn phrase(reading: &str, text: &str, count: u64, last_used: u64) -> Phrase {
        Phrase {
            reading: reading.to_owned(),
            text: text.to_owned(),
            count,
            last_used,
            first_order: 0,
        }
    }

    #[test]
    fn remember_requires_kana_reading_kanji_and_one_line() {
        assert!(memorable("おつかれ", "お疲れ様"));
        assert!(memorable("さーびす", "奉仕"));
        assert!(memorable("ひびの", "日々の"));
        assert!(memorable("のまの", "々"));
        for (reading, text) in [
            ("ひび", "日々"),
            ("abc", "漢字"),
            ("おつk", "お疲れ"),
            ("カタカナ", "漢字"),
            ("ゝああ", "漢字"),
            ("おつかれ", "おつかれ"),
            ("おつかれ", "abc"),
            ("おつかれ", "漢\t字"),
            ("おつかれ", "漢\n字"),
            ("おつかれ", "漢\r字"),
        ] {
            assert!(!memorable(reading, text), "{reading:?} {text:?}");
        }
    }

    #[test]
    fn learned_candidates_rank_frequency_then_first_learning_and_require_completion() {
        let mut phrases = vec![
            phrase("おつかれ", "low", 1, 100),
            phrase("おつかれさま", "recent", 3, 50),
            phrase("おつかれさまです", "frequent", 4, 1),
            phrase("おつかれでした", "older", 3, 20),
            phrase("おつ", "exact", 100, 100),
            phrase("よろしい", "unrelated", 100, 100),
        ];
        phrases[1].first_order = 2;
        phrases[3].first_order = 1;
        assert_eq!(
            learned_candidates("おつ", &phrases),
            strings(&["frequent", "older", "recent", "low"])
        );
    }

    #[test]
    fn first_learning_order_survives_reuse_and_serialization() {
        let mut phrases = Vec::new();
        record_phrase(&mut phrases, "かんじです", "漢字です", 10);
        record_phrase(&mut phrases, "かんじです", "感じです", 20);
        assert_eq!(learned_candidates("かん", &phrases), strings(&["漢字です", "感じです"]));
        record_phrase(&mut phrases, "かんじです", "感じです", 30);
        assert_eq!(learned_candidates("かん", &phrases)[0], "感じです");
        record_phrase(&mut phrases, "かんじです", "漢字です", 40);
        assert_eq!(learned_candidates("かん", &phrases)[0], "漢字です");
        let encoded = phrases.iter().map(|phrase| format!("{}\t{}\t{}\t{}\t{}\n",
            phrase.reading, phrase.text, phrase.count, phrase.last_used, phrase.first_order)).collect::<String>();
        assert_eq!(learned_candidates("かん", &parse_phrases(&encoded))[0], "漢字です");
    }

    #[test]
    fn history_precedes_windows_and_duplicates_are_removed() {
        let result = complete_predictions(
            "よろ",
            &[],
            strings(&["よろしく", "よろしく"]),
            |_| strings(&["よろしく", "宜しく", "宜しく", "よろしくお願いします"]),
        );
        assert_eq!(
            result,
            strings(&["よろしく", "宜しく", "よろしくお願いします"])
        );
    }

    #[test]
    fn windows_candidates_exclude_conversions_single_chars_and_reading() {
        let result = complete_predictions("よろ", &strings(&["寄ろ"]), Vec::new(), |_| {
            strings(&["寄ろ", "字", "", "よろ", "よろしく", "宜しく"])
        });
        assert_eq!(result, strings(&["よろしく", "宜しく"]));
    }

    #[test]
    fn predictions_are_limited_to_six() {
        let result = complete_predictions("よろ", &[], strings(&["one"]), |_| {
            strings(&["two", "three", "four", "five", "six", "seven"])
        });
        assert_eq!(
            result,
            strings(&["one", "two", "three", "four", "five", "six"])
        );
        let result = complete_predictions(
            "よろ",
            &[],
            strings(&["1", "2", "3", "4", "5", "6", "7"]),
            |_| panic!("History already supplies six predictions"),
        );
        assert_eq!(result, strings(&["1", "2", "3", "4", "5", "6"]));
    }

    #[test]
    fn invalid_prefix_does_not_call_windows() {
        for reading in ["", "あ", "abc", "あk", "カナ", "ゝあ"] {
            assert!(
                complete_predictions(reading, &[], Vec::new(), |_| panic!("Invalid prefix"))
                    .is_empty()
            );
        }
    }

    #[test]
    fn reads_legacy_tsv_and_defaults_malformed_numbers() {
        let phrases = parse_phrases("short\trow\nおつかれ\tお疲れ様\t3\t100\r\nよろしい\t宜しい\tbad\tbad\nおつかれ\tお疲れ様\t4\t101\textra\n");
        assert_eq!(phrases.len(), 2);
        assert!(phrases.contains(&phrase("おつかれ", "お疲れ様", 4, 101)));
        assert!(phrases.contains(&phrase("よろしい", "宜しい", 1, 0)));
    }

    #[test]
    fn remembers_pair_counts_safely_and_retains_most_recent_entries() {
        let mut phrases = vec![phrase("おつかれ", "お疲れ様", u64::MAX, 1)];
        record_phrase(&mut phrases, "おつかれ", "お疲れ様", 10);
        assert_eq!(phrases, vec![phrase("おつかれ", "お疲れ様", u64::MAX, 10)]);
        record_phrase(&mut phrases, "おつかれ", "お疲れ", 11);
        record_phrase(&mut phrases, "おつかれ", "お疲れ", 12);
        let mut expected = phrase("おつかれ", "お疲れ", 2, 12);
        expected.first_order = 1;
        assert_eq!(phrases[0], expected);
        let mut many: Vec<_> = (0..HISTORY_LIMIT)
            .map(|index| phrase("よみ", &index.to_string(), 1, index as u64))
            .collect();
        record_phrase(&mut many, "あたらしい", "新しい", HISTORY_LIMIT as u64);
        assert_eq!(many.len(), HISTORY_LIMIT);
        assert_eq!(many[0].text, "新しい");
        assert!(!many.iter().any(|phrase| phrase.text == "0"));
    }

    #[test]
    fn saves_and_replaces_existing_tsv_without_touching_user_history() {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "azk-cleanroom-predict-{}-{sequence}",
            std::process::id()
        ));
        let path = directory.join("phrases.tsv");
        let mut history = History::load(path.clone()).unwrap();
        history.remember("おつかれ", "お疲れ様", 10).unwrap();
        history.remember("おつかれ", "お疲れ様", 11).unwrap();
        history.remember("よろしい", "宜しい", 12).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "よろしい\t宜しい\t1\t12\t2\nおつかれ\tお疲れ様\t2\t11\t1\n"
        );
        assert_eq!(History::load(path.clone()).unwrap().phrases.len(), 2);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    /// 2026-10-29: 忘れた学習の語を含む控え (読みと表記の両方に含む) だけを消して書き直す
    #[test]
    fn forgets_phrases_containing_a_forgotten_word() {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!("azk-cleanroom-predict-{}-{sequence}", std::process::id()));
        let path = directory.join("phrases.tsv");
        let mut history = History::load(path.clone()).unwrap();
        history.remember("そらねこのもんだい", "空ネコの問題", 10).unwrap();
        history.remember("そらねこのもんだい", "空猫の問題", 11).unwrap();
        history.remember("おつかれさま", "お疲れ様", 12).unwrap();
        assert!(history.forget("そらねこ", "空ネコ").unwrap());
        assert!(!history.forget("そらねこ", "空ネコ").unwrap());
        let saved = History::load(path.clone()).unwrap();
        let mut texts: Vec<&str> = saved.phrases.iter().map(|p| p.text.as_str()).collect();
        texts.sort();
        assert_eq!(texts, vec!["お疲れ様", "空猫の問題"]);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn failed_load_and_failed_replacement_preserve_existing_data() {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "azk-cleanroom-predict-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let invalid = directory.join("invalid.tsv");
        fs::write(&invalid, [0xff]).unwrap();
        assert!(History::load(invalid.clone()).is_err());
        assert_eq!(fs::read(&invalid).unwrap(), [0xff]);
        let blocked = directory.join("phrases.tsv");
        fs::create_dir(&blocked).unwrap();
        assert!(save_phrases(&blocked, &[phrase("よみ", "字", 1, 0)]).is_err());
        assert!(blocked.is_dir());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
        fs::remove_file(invalid).unwrap();
        fs::remove_dir(blocked).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn windows_prediction_api() {
        let Ok(readings) = std::env::var("AZK_PREDICT_TEST") else {
            return;
        };
        let mut report = String::new();
        for reading in readings.split(',') {
            let started = std::time::Instant::now();
            let windows = super::super::windows_text::predictions(reading);
            let elapsed = started.elapsed();
            let converted = super::super::windows_text::conversions(reading);
            let predicted =
                complete_predictions(reading, &converted, Vec::new(), |_| windows.clone());
            report.push_str(&format!(
                "{reading}\t{}ms\tWindows={windows:?}\tPredictions={predicted:?}\n",
                elapsed.as_millis()
            ));
            assert!(
                !windows.is_empty(),
                "Windows returned no predictions for {reading:?}"
            );
        }
        let output = std::env::var_os("AZK_PREDICT_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../logs/predict_test.txt"));
        fs::write(output, report).unwrap();
    }
}
