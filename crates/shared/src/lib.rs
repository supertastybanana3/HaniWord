use serde::{Deserialize, Serialize};

pub mod winsec;
/// 2026-11-04: 単語登録 (%APPDATA%\HaniWord\user_dictionary.tsv) の読み書き
pub mod user_dictionary;
use std::path::PathBuf;
use std::io::{self, Read};

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/azookey.rs"));
    include!(concat!(env!("OUT_DIR"), "/window.rs"));
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("azookey_service_descriptor");
}

/// 利用者ごとのデータのフォルダ名 (%APPDATA%\HaniWord)。2026-10-20 に Azookey から改名
pub const APP_DIR: &str = "HaniWord";

/// 2026-10-29: この版の番号 (新しい版の知らせで、GitHub の最新のリリースと比べる。設定画面にも出す)。
/// 版を上げるときは、ここと installer\compile.ps1 の -Version をそろえる (compile.ps1 が食い違いを止める)
pub const APP_VERSION: &str = "0.1.1";

/// 2026-10-29: リリースを置く GitHub のリポジトリ (所有者/名前)。新しい版の知らせが問い合わせる先
pub const RELEASE_REPOSITORY: &str = "supertastybanana3/HaniWord";

/// 利用者のデータの置き場 (%APPDATA%\HaniWord: 設定・学習・予測の履歴・ログ)。APPDATA が無ければ None。読み書きする所はみなここから引く。
/// 2026-11-10: 試験のビルド (shared 自身の試験と、機能 test-data-root を入れた IME 本体などの試験) では、本物の %APPDATA% を使わない。
/// APPDATA が一時フォルダの中 (試験が作った置き場) を指していればそこ、そうでなければプロセスごとの一時フォルダ (中は空から始まる)。
/// 前は IME 本体の試験が、この PC の利用者の settings.json を読み、予測の履歴 (phrases.tsv)・直さない綴り・ログに書きうる道があった
pub fn app_data_dir() -> Option<PathBuf> {
    let appdata = PathBuf::from(std::env::var_os("APPDATA")?);
    #[cfg(any(test, feature = "test-data-root"))]
    {
        let temp = std::env::temp_dir();
        if !appdata.starts_with(&temp) {
            return Some(temp.join(format!("haniword_test_appdata_{}", std::process::id())).join(APP_DIR));
        }
    }
    Some(appdata.join(APP_DIR))
}

fn get_config_root() -> PathBuf {
    app_data_dir().unwrap_or_else(|| PathBuf::from(APP_DIR))
}

const SETTINGS_FILENAME: &str = "settings.json";
const MAX_CLIENT_SETTINGS_BYTES: u64 = 65536;

fn client_settings_path() -> PathBuf {
    get_config_root().join("client").join(SETTINGS_FILENAME)
}

fn read_client_settings(path: &std::path::Path) -> Option<AppConfig> {
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(MAX_CLIENT_SETTINGS_BYTES + 1).read_to_string(&mut text).ok()?;
    if text.len() as u64 > MAX_CLIENT_SETTINGS_BYTES { return None; }
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let mut config = AppConfig::default();
    config.input = serde_json::from_value(value.get("input")?.clone()).ok()?;
    config.input = public_input(&config.input);
    Some(config)
}

fn public_input(input: &InputConfig) -> InputConfig {
    let mut input = input.clone();
    // Only known non-sensitive client preferences cross the sandbox boundary.
    input.extra.clear();
    if !["auto", "standard", "kunrei_nn"].contains(&input.romaji_style.as_str()) { input.romaji_style = "auto".into(); }
    if !["auto", "on", "off"].contains(&input.tex_mode.as_str()) { input.tex_mode = "auto".into(); }
    input.direct_prefixes = input.direct_prefixes.chars().filter(|c| c.is_ascii_punctuation()).take(16).collect();
    input.latin_languages.retain(|code| LATIN_LANGUAGES.contains(&code.as_str()));
    input.latin_languages.sort();
    input.latin_languages.dedup();
    let mut symbols: Vec<char> = input.fullwidth_symbols.chars().filter(|c| CONFIGURABLE_SYMBOLS.contains(*c)).collect();
    symbols.sort();
    symbols.dedup();
    input.fullwidth_symbols = symbols.into_iter().collect();
    if !["yen", "backslash"].contains(&input.yen_key.as_str()) { input.yen_key = default_yen_key(); }
    input.mode_indicator = input.mode_indicator_style().to_string();
    if !["dictionary", "preview"].contains(&input.ai_pause_order.as_str()) { input.ai_pause_order = default_ai_pause_order(); }
    for names in [&mut input.domains_off, &mut input.domains_focus, &mut input.dialects] {
        names.retain(|name| valid_domain_name(name));
        names.sort();
        names.dedup();
    }
    input
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ZenzaiConfig {
    pub enable: bool,
    pub profile: String,
    pub backend: String,
    /// インストール先に置いた zenz モデルのファイル名
    #[serde(default = "default_model")]
    pub model: String,
    /// モデルのプロンプト形式 ("v2" / "v3")
    #[serde(default = "default_model_version")]
    pub model_version: String,
    /// 1 回の変換で Zenzai が推論する回数の上限
    #[serde(default = "default_inference_limit")]
    pub inference_limit: i32,
    /// 2026-11-19: 利用者が AI の入り切りを設定画面で自分で替えたか。偽なら初めの値のままとみなし、遅い CPU (AVX 無しで GPU も使えない) の PC では
    /// launcher が AI を切る (apply_slow_cpu_default)
    #[serde(default)]
    pub enable_chosen: bool,
    /// launcher が AI を切った理由 ("slow_cpu")。空なら切っていない。設定画面はこれを見て知らせを出す
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub auto_off: String,
    /// このプログラムが知らない項目。書き戻すときに消さない
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// launcher が遅い CPU のために AI を切ったときの印 (ZenzaiConfig.auto_off)
pub const AI_AUTO_OFF_SLOW_CPU: &str = "slow_cpu";

/// 2026-11-19: launcher が変換サーバーを起動する前に毎回書く、実際に使う AI の版と、設定から下げたわけ (app_data_dir の下の ai_backend.json)。
/// 設定画面 (「HaniWord について」) が読む。reasons: gpu_failed_before・no_avx2・avx2_failed_before・gpu_missing・avx2_missing・quick_exits (起きた順)
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct AiBackendStatus {
    /// 実際に使う版 (llama_vulkan・llama_cuda・llama_cpu_avx2・llama_cpu)
    pub llama: String,
    /// 設定の裏方 (vulkan・cuda・cpu)
    pub setting: String,
    #[serde(default)]
    pub reasons: Vec<String>,
    /// launcher が AI を切ったなら "slow_cpu"
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ai_off: String,
    /// 書いた時刻 (UNIX 秒)
    #[serde(default)]
    pub updated: u64,
    /// このプログラムが知らない項目。書き戻すときに消さない
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

const AI_BACKEND_STATUS_FILENAME: &str = "ai_backend.json";

impl AiBackendStatus {
    pub fn path() -> Option<PathBuf> {
        app_data_dir().map(|dir| dir.join(AI_BACKEND_STATUS_FILENAME))
    }

    /// 無い・大きすぎる (64KB 超)・読めないときは None
    pub fn read() -> Option<Self> {
        Self::read_from(&Self::path()?)
    }

    fn read_from(path: &std::path::Path) -> Option<Self> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        if !meta.is_file() || meta.len() > 65536 {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    /// 一時ファイルに書いてから置き換える。利用者の権限でないとき (管理者の起動) は書かない
    pub fn write(&self) -> io::Result<()> {
        if !winsec::private_data_allowed() {
            return Ok(());
        }
        let path = Self::path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "APPDATA is unavailable"))?;
        self.write_to(&path)
    }

    fn write_to(&self, path: &std::path::Path) -> io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        write_atomic(path, serde_json::to_string_pretty(self)?.as_bytes())
    }
}

impl ZenzaiConfig {
    /// 2026-11-19: 遅い CPU (slow: launcher が AVX 無しの CPU の版を選んだ) の PC で、AI の入り切りが初めの値のままなら切る。
    /// 速い版に戻ったら、launcher が切った分だけ戻す。利用者が自分で替えていれば何もしない。替えたら真
    pub fn apply_slow_cpu_default(&mut self, slow: bool) -> bool {
        if self.enable_chosen {
            return false;
        }
        if slow && self.enable {
            self.enable = false;
            self.auto_off = AI_AUTO_OFF_SLOW_CPU.to_string();
            return true;
        }
        if !slow && !self.enable && self.auto_off == AI_AUTO_OFF_SLOW_CPU {
            self.enable = true;
            self.auto_off.clear();
            return true;
        }
        false
    }
}

fn default_model() -> String {
    "zenz-v3.2-small.gguf".to_string()
}

fn default_model_version() -> String {
    "v3".to_string()
}

fn default_inference_limit() -> i32 {
    3
}

/// 入力の設定
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct InputConfig {
    /// 英語と日本語を自動で打ち分ける
    #[serde(default = "default_true")]
    pub auto_english: bool,
    /// ローマ字の打ち方。"standard" = ヘボン式も訓令式も日本語として読む。
    /// "kunrei_nn" = 訓令式で ん は nn と打つ人向け (shi chi tsu ji sha cha や n+子音 は英語のしるしにする)
    #[serde(default = "default_romaji_style")]
    pub romaji_style: String,
    /// 英字で打った語の後ろに日本語が続くとき半角スペースを自動で入れる (Windows でコンピューター)。初期値はオフ
    #[serde(default)]
    pub space_after_latin: bool,
    /// 行頭か空白の直後に打つと、空白まで変換せずにそのままアプリへ渡す記号 (/command・$skill・@ファイル名)。空なら使わない
    #[serde(default = "default_direct_prefixes")]
    pub direct_prefixes: String,
    /// 英字のまま入れた英単語を決まった表記に直す (github → GitHub, pdf → PDF)。初期値はオン
    #[serde(default = "default_true")]
    pub english_case_fix: bool,
    /// 2026-11-24: Flutter で作られたアプリの窓 (FLUTTERVIEW) でも学習する。初期値はオフ (Flutter は欄がパスワードかを IME に伝えないので、
    /// オフのあいだは学習・予測の履歴・前後の文の取得・診断ログ・打ち方の数えをしない。client/src/engine/direct_field.rs の 5)
    #[serde(default)]
    pub learn_in_flutter: bool,
    /// 端末でシェルの入力待ち (PowerShell・コマンド プロンプト・bash) のときは英字にする
    #[serde(default = "default_true")]
    pub latin_at_shell_prompt: bool,
    /// Visible terminal confirmation choices pass their advertised shortcut keys without changing input mode.
    #[serde(default = "default_true")]
    pub latin_at_terminal_choices: bool,
    /// 大文字を打ったら確定まで英字のまま (オフでも大文字で始まる語はその語だけ英字)。
    /// 2026-10-21: 初期値はオフ。オンだと英語の判定が止まり、H2Odearu・Googledekensaku が確定まで英字のままになるため
    #[serde(default)]
    pub shift_ascii_mode: bool,
    /// アプリを開いたときの入力モードを「あ」にする
    #[serde(default = "default_true")]
    pub start_in_kana: bool,
    /// ^ と _ で上付き・下付き (x^2 → x²、H_2O → H₂O)
    #[serde(default = "default_true")]
    pub caret_scripts: bool,
    /// ギリシャ文字の名前を文字に (アルファ → α。プラスアルファは変えない)
    #[serde(default = "default_true")]
    pub greek_letters: bool,
    /// 化学式の数字を下付きに (H2O → H₂O)
    #[serde(default = "default_true")]
    pub chem_subscript: bool,
    /// TeX 版: "auto" = .tex を開いているときだけ / "on" = いつも / "off" = 使わない (TeX 版を入れたときだけ効く)
    #[serde(default = "default_tex_mode")]
    pub tex_mode: String,
    /// 長押しで出すアクセント付きの文字の言語 (fr de es it pt nordic pl cs tr pinyin)。空なら使わない
    #[serde(default)]
    pub latin_languages: Vec<String>,
    /// 日本語入力 (あ) のときも長押しでアクセント付きの文字を出す (オフなら英数 (A) のときだけ)
    #[serde(default = "default_true")]
    pub accents_in_kana: bool,
    /// 2026-11-05: 長押しのアクセントを使うとき (latin_languages が空でない)、英字のキーを押しっぱなしにしたら同じ字を続けて入れる
    /// (初期値オフ = Mac の長押しと同じく続けて入れない。数字・記号・Backspace・矢印はいつも続けて入る。端末・コードエディタでも続けて入る)
    #[serde(default)]
    pub letter_key_repeat: bool,
    /// 数字のあとの . , を小数点・桁区切りにする (2.0、1,000。iOS と同じ。, は後ろがちょうど 3 桁のときだけ)
    #[serde(default = "default_true")]
    pub number_punctuation: bool,
    /// 英単語を打っている途中に、前の語と打ちかけから次の語の候補を出す (my f → father。Tab で補う)。
    /// 英語と、latin_languages で選んだ言語の表を使う (2026-10-19)。
    /// 2026-11-05 (仕事 27): 日本語の予測候補と同じく選ぶ形にし、初期値をオフにした。前の項目 next_word_suggestions (初期値オン) は読まない
    /// (保存のたびに全項目が書かれるので、書いてある true が選んだものか見分けられない。全員オフから始める。ファイルに残っていても extra に残るだけ)
    #[serde(default)]
    pub english_predictions: bool,
    /// 2026-11-05 (仕事 27): 打っている間 (ライブ変換の中でも) に、日本語の予測候補 (確定した言い回しの履歴・Windows の予測) を出す。初期値はオフ
    #[serde(default)]
    pub japanese_predictions: bool,
    /// 2026-10-20: 日本語入力 (あ) で打った記号のうち、全角にするもの (半角の字で並べる。ここに無い記号は半角)。
    /// 初期値は全部 (Google 日本語入力の初期設定と同じ)。、。・「」ー になる , . / [ ] - はローマ字表の記号なので対象外
    #[serde(default = "default_fullwidth_symbols")]
    pub fullwidth_symbols: String,
    /// ¥ キー (とろキーの \) で入る字: "yen" = ¥ / "backslash" = \。全角にするかは fullwidth_symbols の \ で決める
    #[serde(default = "default_yen_key")]
    pub yen_key: String,
    /// 2026-11-05 (仕事 23): 日本語入力 (あ) で打った数字の形。"off" = 半角 (初期値)、"numbers" = 英字・記号にくっついていない数字だけ全角
    /// (5kg・GPT4o・mp3 は半角)、"all" = 英字にくっついた数字も全角 (５kg・GPT４o。式 3x+1=0 と URL・メール・パスは半角のまま)。
    /// 見せる所と確定する所だけ全角にし、数字のあとの判定 (小数点・桁区切り・単位・化学式・数式・/ と .) は半角のときと同じに効かせる。
    /// 前の版の true / false は "numbers" / "off" として読む
    #[serde(default = "default_fullwidth_digits", deserialize_with = "fullwidth_digits_setting")]
    pub fullwidth_digits: String,
    /// 2026-10-20: 使わない分野辞書の名前 (変換サーバーの domain\domains.txt に並ぶもの。例 "law")。
    /// 配布する分野辞書は初期値で全部オン (標準の語より低い重みで足すだけ)。変換サーバーが読む
    #[serde(default)]
    pub domains_off: Vec<String>,
    /// 2026-10-30: よく打つ分野 (インストーラーの専門分野の画面・設定アプリで選ぶ)。選んだ分野は、文脈に分野の目印の語が無くても
    /// 同音語の優遇 (再建 → 債権・2じょう → 2乗) を効かせる。選ばない分野は今のまま (オンなら目印があるときだけ)。変換サーバーが読む
    #[serde(default)]
    pub domains_focus: Vec<String>,
    /// 2026-11-06 (仕事 39): 方言の言い方の辞書の名前 (変換サーバーの domain\dialects.txt に並ぶもの。今は "kansai" だけ)。
    /// 選んだ方言の文の終わり・助動詞のまとまり (へん・はる・ねん・やん) をかなのまま出す。初期値は空 (使わない)。変換サーバーが読む
    #[serde(default)]
    pub dialects: Vec<String>,
    /// 2026-11-07 (仕事 43): ネットスラングの辞書 (変換サーバーの domain\slang.txt の internet。オワコン・ぴえん・親ガチャ など) を使う。初期値はオフ。変換サーバーが読む
    #[serde(default)]
    pub internet_slang: bool,
    /// 2026-10-22 (M3.6 誤用): 候補窓に「本来は〜」の注記を出す (的を得る → 本来は 的を射る)。初期値はオフ
    #[serde(default)]
    pub misuse_notes: bool,
    /// 2026-10-22 (M3.6 誤用): 候補窓に異字同訓の使い分けの注記を出す (会う・合う・遭う)。初期値はオフ
    #[serde(default)]
    pub homophone_notes: bool,
    /// 2026-11-06 (仕事 40): 候補窓で、差別・強い侮辱の語とされる語に「注意」の注記を出す (候補はそのまま。チャットやゲームで弾かれやすい語)。初期値はオフ
    #[serde(default)]
    pub caution_slurs: bool,
    /// 2026-11-06 (仕事 40): 候補窓で、放送・新聞などで言い換えられることがある語に「注意」の注記を出す。初期値はオフ
    #[serde(default)]
    pub caution_broadcast: bool,
    /// 2026-10-28 (試験中): 。、？！ を打ったら、そこまでを変換する (ATOK の句読点変換。確定は次の字か Enter、直後の BackSpace で入力中に戻す)。初期値はオフ
    #[serde(default)]
    pub punctuation_convert: bool,
    /// 2026-10-28 (試験中): ローマ字の母音の抜け (souiuktodesu の k = そういうことです) を、変換するときに直す。読めない子音 1 字だけで、英字にした語は直さない。
    /// 打ったとおりも候補に残し、直した直後の Shift+BackSpace で打ったとおりに戻す (戻した綴りは次から直さない)。初期値はオフ
    #[serde(default)]
    pub typo_repair: bool,
    /// 2026-10-29: ライブ変換で、AI が清書した前のほう (最後の文節を除く) を、次の打鍵で辞書の結果に戻さない (後ろだけを辞書で変換して見せる)。
    /// 2026-10-30 から初期値はオン (試験中から外した。前の版の設定で項目が無ければオン、書いてある値はそのまま)
    #[serde(default = "default_true")]
    pub live_keep_refined: bool,
    /// 2026-10-29: ライブ変換で、短い入力は辞書の変換だけを見せ (手が止まっても AI で変換し直さない)、AI の変換は Space の候補の一覧に混ぜる
    /// (一番上は見えていたもののまま)。2026-10-30 から初期値はオン (live_keep_refined と同じ)
    #[serde(default = "default_true")]
    pub live_short_dictionary: bool,
    /// 2026-11-08 (仕事 45): 変換するとき、カーソルの右の字 (入力中の字の後ろ、最初の改行まで・20 字まで) も見る。
    /// 辞書だけの変換 (ライブ変換の表示など) で、組み合わせの数の表の外側の文脈に使う。オフなら IME 本体は右の字を取らずに空を送る。
    /// 初期値はオン (前の版の設定で項目が無ければオン、書いてある値はそのまま)。IME 本体と変換サーバーが読む
    #[serde(default = "default_true")]
    pub right_context: bool,
    /// 2026-11-08 (仕事 12): かな入力中に空白なしで打った英語の句 (seeyoutomorrow) を、Space・確定のときに英語の語に分けて半角の空白を入れる
    /// (see you tomorrow)。確実なときだけ (今の区切りが崩れているか全部英単語で、英語の次の語の表で分け方がはっきりしているとき)。
    /// 直後の Backspace で空白なしに戻す。初期値はオン (前の版の設定で項目が無ければオン、書いてある値はそのまま)。IME 本体が読む
    #[serde(default = "default_true")]
    pub english_phrase_spaces: bool,
    /// 2026-11-08 (仕事 53): Ctrl+Space で あ／A を切り替える (US 配列の人向け。初期値オフ: VS Code の補完などとぶつかるため)。
    /// オフなら今までどおり (入力中は確定して空白、入力中でなければアプリへ)。IME 本体が読む
    #[serde(default)]
    pub ctrl_space_toggle: bool,
    /// 2026-11-08 (仕事 52): AI の変換がふだんよりずっと遅いとき (ほかのアプリの負荷など)、変換サーバーが自動で AI を休み、辞書だけの変換にする。
    /// 速さが戻ったら AI に戻す。変換サーバーが読む。
    /// 2026-11-19: 初期値をオフにした (0.1.0。負荷の無いときの Vulkan で、ふだんの速さのばらつきを重いと誤って休むことがあったため。設定でオンにできる)
    #[serde(default)]
    pub ai_auto_pause: bool,
    /// 2026-11-08 (仕事 52): AI を休んでいる間の辞書の並べ方。"dictionary" = 辞書だけ向け (AI を切った設定と同じ直し。初期値)、
    /// "preview" = AI ありのライブ変換の仮表示と同じ (休む前後で見え方が変わらない)。知らない値は dictionary。変換サーバーが読む
    #[serde(default = "default_ai_pause_order")]
    pub ai_pause_order: String,
    /// 2026-11-08 (仕事 52): AI を休んでいる間、候補の窓の下に「負荷がかかっているためAIがオフになっています」を出す。初期値オン。IME 本体が読む
    #[serde(default = "default_true")]
    pub ai_pause_notice: bool,
    /// 2026-11-05 (仕事 28): ・ と / の打ち分け。"auto" (前後で自動。今の動き)・"dot" (いつも ・)・"slash" (いつも /)。初期値は auto
    #[serde(default = "default_auto")]
    pub slash_key: String,
    /// 2026-11-05 (仕事 28): 。 と . の打ち分け。"auto" (前後で自動。今の動き)・"kuten" (いつも 。)・"period" (いつも .)。初期値は auto
    #[serde(default = "default_auto")]
    pub period_key: String,
    /// 2026-11-05 (仕事 28): 難読語の同音語に置き換える (当局の商用 → 当局の慫慂。変換サーバーの homophones.swift)。初期値はオン
    #[serde(default = "default_true")]
    pub homophone_replace: bool,
    /// 2026-11-05 (仕事 28): 小文字で打った英字 1 字＋語を決まった形にする (jri-gu → Jリーグ。initial_words.txt)。初期値はオン
    #[serde(default = "default_true")]
    pub initial_letter_words: bool,
    /// 2026-11-05 (仕事 28): 数学の式 (a=・ax) を英字のまま入れる。分野の辞書 (domains_focus) と切り離した。書いていない前の版の設定では、
    /// 数学か物理をよく打つ分野に選んでいればオン (math_expressions_on)。設定アプリは選び直したときに値を書く
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub math_expressions: Option<bool>,
    /// 2026-11-06 (仕事 32): ギリシャ文字の大文字・小文字の順 (字の小文字 → "lower" か "upper"。例 {"σ": "upper"})。書いていない字は
    /// 変換サーバーの既定 (ウィキペディアの文で数えて決めた。今はどの字も小文字が先)。候補の並びと、カタカナの名前の自動の置き換え (シグマ → Σ) に効く
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub greek_case: std::collections::BTreeMap<String, String>,
    /// 2026-11-03: 表記ゆれ (かなでも漢字でもよい所・送り仮名のあるなし) で、利用者が確定した形の向きを覚えて、次の変換と候補の順に使う
    /// (変換サーバーの hyouki.swift。記録は memory\hyouki_learned.txt)。初期値はオン
    #[serde(default = "default_true")]
    pub hyouki_learning: bool,
    /// 2026-11-05 (仕事 22 の 3): 候補の一覧で ・ / ／ と 。 . を選び直したら、同じ前後の語の組で次からその形にする (初期値オン)。
    /// 記録は変換サーバーの memory\slash_dot_learned.txt (個人のデータ。配らない)。学習データを使ってはいけない接続では読み書きしない
    #[serde(default = "default_true")]
    pub slash_learning: bool,
    /// 2026-11-05 (仕事 24): ラテン文字のつづりの指摘・自動修正。"off"・"suggest" (次の語の候補の一番上に直した形を出す)・
    /// "autocorrect" (Space で確定するとき、確実な所だけ直す。直後の BackSpace で戻す)。初期値は off。
    /// かなの入力中に英字の並びと判定した英語の文の語だけを見る (半角英数の直接入力・日本語に混ざった英語・端末・コードの欄は見ない)
    #[serde(default = "default_latin_spell")]
    pub latin_spell: String,
    /// 2026-11-06 (まとめ役 09:57): 「。。。」「・・・」「...」(同じ点が 3 つ・6 つ) を、変換で自動で … ・ …… にする (初期値オフ)。
    /// オフでも候補の一覧の 2 番目に … を出す (変換サーバーの ellipsis.swift)
    #[serde(default)]
    pub ellipsis_auto: bool,
    /// 2026-11-06 (まとめ役 09:57・10:02、iOS のスマート句読点と同じ): 半角英数 (A) で - を 2 回続けたら — (em dash) にする (初期値オフ)。
    /// オフのときは、半角英数は今までどおり素通し。端末・コードを書くアプリでは働かせない
    #[serde(default)]
    pub smart_punctuation: bool,
    /// 2026-11-06 (仕事 34、iOS と同じ入力の補助): 半角英数で、英字か数字のあとの空白に続けて Space を押したら「. 」にする (初期値オフ)
    #[serde(default)]
    pub period_shortcut: bool,
    /// 2026-11-06 (仕事 34): 半角英数で、文の終わり (. ? ! と空白のあと・改行のあと) に打った小文字の英字を大文字にする (初期値オフ)
    #[serde(default)]
    pub auto_capitalize: bool,
    /// 2026-11-06 (仕事 34): 1+2= のように式のあとに = を打ったら、候補に答え (3・1+2=3) を出す (初期値オフ。変換サーバー)
    #[serde(default)]
    pub calc_candidates: bool,
    /// 2026-11-06 (仕事 35): 入力モード (あ・A) を切り替えたときにカーソルのそばに出す表示。表示の窓 (ui.exe) が読む。
    /// "badge" = 札 (あ と A を並べて今のほうを塗る。Caps Lock がオンの間も出す。初期値)、
    /// "compact" = 小さな吹き出し (今のモードの字だけ。2.5 秒で消える。Caps Lock がオンの間は矢印を出す)、
    /// "off" = 出さない (タスクバーの右下の表示だけ。Caps Lock の表示も出さない)。知らない値は badge
    #[serde(default = "default_mode_indicator")]
    pub mode_indicator: String,
    /// 2026-11-06 (仕事 36): かなで打った数詞＋助数詞 (いっかい・ふたり・にじゅうさんにん) の出し方。"arabic" = 算用数字の形 (1回) が先 (初期値、今のまま)、
    /// "kanji" = 漢数字の形 (一回・二人・二十三人) を 1 番目、算用数字の形を 2 番目に。数字のキーで打った数字はどちらでも変えない。知らない値は arabic。変換サーバーが読む
    #[serde(default = "default_counter_numerals", deserialize_with = "counter_numerals_setting")]
    pub counter_numerals: String,
    /// このプログラムが知らない入力の設定 (新しい版が足したもの)。書き戻すときに消さない
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_true() -> bool {
    true
}

fn default_ai_pause_order() -> String {
    "dictionary".to_string()
}

fn default_fullwidth_digits() -> String {
    "off".to_string()
}

/// 2026-11-05 (仕事 23): fullwidth_digits は前の版の bool (true = numbers) も読む。知らない値は off
fn fullwidth_digits_setting<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Value { Flag(bool), Name(String) }
    Ok(match Value::deserialize(deserializer)? {
        Value::Flag(true) => "numbers".to_string(),
        Value::Flag(false) => "off".to_string(),
        Value::Name(name) if ["off", "numbers", "all"].contains(&name.as_str()) => name,
        Value::Name(_) => "off".to_string(),
    })
}

fn default_counter_numerals() -> String {
    "arabic".to_string()
}

fn counter_numerals_setting<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    // 文字列でない値 (数・null) でも設定全体を読み落とさない
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(if value.as_str() == Some("kanji") { "kanji".to_string() } else { default_counter_numerals() })
}

fn default_latin_spell() -> String {
    "off".to_string()
}

fn default_auto() -> String {
    "auto".to_string()
}

/// 2026-11-06 (仕事 35): input.mode_indicator の値
pub const MODE_INDICATOR_STYLES: [&str; 3] = ["badge", "compact", "off"];

fn default_mode_indicator() -> String {
    "badge".to_string()
}

impl InputConfig {
    /// 2026-11-05 (仕事 28): 数学の式を英字のまま入れるか。書いてあればその値、無ければ (前の版の設定) 数学か物理をよく打つ分野に選んでいるか
    pub fn math_expressions_on(&self) -> bool {
        self.math_expressions.unwrap_or_else(|| self.domains_focus.iter().any(|d| d == "math" || d == "physics"))
    }

    /// 2026-11-06 (仕事 35): 入力モードの表示 (MODE_INDICATOR_STYLES のどれか。知らない値は "badge")
    pub fn mode_indicator_style(&self) -> &'static str {
        MODE_INDICATOR_STYLES.iter().copied().find(|style| *style == self.mode_indicator).unwrap_or("badge")
    }
}

/// 長押しのアクセント付きの文字と次の語の候補で選べる言語 (input.latin_languages の値)
pub const LATIN_LANGUAGES: [&str; 10] = ["fr", "de", "es", "it", "pt", "nordic", "pl", "cs", "tr", "pinyin"];
/// 2026-10-30: 配布する分野辞書の名前 (変換サーバーの domain\domains.txt と同じ。インストーラーの専門分野の画面の並び)
/// 2026-10-31: 知っている分野の全部。実際に配るかどうかは一式の domain\domains.txt で決まり、インストーラーと設定画面はそこにある分野だけを出す
pub const DOMAIN_NAMES: [&str; 11] = ["law", "medicine", "biology", "chemistry", "it", "geography", "history", "earth", "architecture", "religion", "math"];

/// 2026-11-06 (仕事 39): 知っている方言の言い方の辞書の名前 (変換サーバーの domain\dialects.txt と同じ)。設定画面は、入っているものだけを出す
pub const DIALECT_NAMES: [&str; 1] = ["kansai"];

/// 分野辞書の名前の決まり (domains.txt・dialects.txt・input.domains_off・domains_focus・dialects): 英小文字・数字・_ だけ、32 字まで
fn valid_domain_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// 2026-11-06: 次の語の候補の表 (IME 本体の隣の next_words\next_<名前>.hwn) と、設定の言語 (input.latin_languages) の対応。
/// IME 本体 (next_word.rs) と設定アプリとインストーラー (haniword.iss の LanguageChosen) で同じ。英語 (next_en.hwn) はいつも入れる
pub const NEXT_WORD_LANGUAGE_FILES: &[(&str, &[&str])] = &[
    ("fr", &["fr"]), ("de", &["de"]), ("es", &["es"]), ("it", &["it"]), ("pt", &["pt"]),
    ("pl", &["pl"]), ("cs", &["cs"]), ("tr", &["tr"]), ("nordic", &["da", "sv", "nb", "is", "fi"]),
];

/// 2026-11-06: 次の語の候補の表が入っていない言語 (dir に next_<名前>.hwn が 1 つでも無い言語)。インストーラーは英語の表だけ必ず入れ、
/// ほかの言語は選んだものだけ入れるので、設定アプリがあとから選んだ言語に一言出すのに使う。dir (next_words) が無ければ None
pub fn missing_next_word_languages(dir: &std::path::Path) -> Option<Vec<String>> {
    if !dir.is_dir() {
        return None;
    }
    Some(NEXT_WORD_LANGUAGE_FILES.iter()
        .filter(|(_, names)| names.iter().any(|n| !dir.join(format!("next_{n}.hwn")).is_file()))
        .map(|(code, _)| code.to_string())
        .collect())
}

/// 2026-10-31: 入っている分野辞書の名前 (%APPDATA%\HaniWord\domain\domains.txt。変換サーバーが読むのと同じ表、並びはファイルのまま)。
/// 設定画面は、ここにある分野だけを出す (配る一式に分野辞書を入れる・外すだけで、画面も付いてくる)。
/// 読めない・リンク・64KB を超えるときは None。# の行 (説明) と、名前の決まりに合わない行は読まない
pub fn installed_domain_names() -> Option<Vec<String>> {
    domain_names_in(&get_config_root())
}

/// 2026-11-06 (仕事 39): 入っている方言の言い方の辞書の名前 (%APPDATA%\HaniWord\domain\dialects.txt)。読み方は installed_domain_names と同じ
pub fn installed_dialect_names() -> Option<Vec<String>> {
    names_in(&get_config_root(), "dialects.txt")
}

/// 2026-11-07 (仕事 43): 入っているくだけた言葉の辞書の名前 (%APPDATA%\HaniWord\domain\slang.txt。今は "internet" だけ)。読み方は installed_domain_names と同じ
pub fn installed_slang_names() -> Option<Vec<String>> {
    names_in(&get_config_root(), "slang.txt")
}

fn domain_names_in(root: &std::path::Path) -> Option<Vec<String>> {
    names_in(root, "domains.txt")
}

fn names_in(root: &std::path::Path, file: &str) -> Option<Vec<String>> {
    let path = root.join("domain").join(file);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > 64 << 10 {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    Some(text.lines().map(|line| line.trim_start_matches('\u{feff}').trim()).filter(|name| valid_domain_name(name)).map(String::from).collect())
}

/// 設定で全角・半角を選べる記号 (日本語入力のときに打てる半角の記号のうち、ローマ字表で 、。・「」ー にならないもの)
pub const CONFIGURABLE_SYMBOLS: &str = "!\"#$%&'()*+:;<=>?@\\^_`{|}~";

fn default_fullwidth_symbols() -> String {
    CONFIGURABLE_SYMBOLS.to_string()
}

fn default_yen_key() -> String {
    "yen".to_string()
}

fn default_tex_mode() -> String {
    "auto".to_string()
}

fn default_direct_prefixes() -> String {
    "/$@".to_string()
}

fn default_romaji_style() -> String {
    // 打った綴りから判断する (訓令式＋ん は nn の人と分かるまでは、ヘボン式も読む)
    "auto".to_string()
}

impl Default for InputConfig {
    fn default() -> Self {
        InputConfig {
            auto_english: true,
            romaji_style: default_romaji_style(),
            space_after_latin: false,
            direct_prefixes: default_direct_prefixes(),
            english_case_fix: true,
            learn_in_flutter: false,
            latin_at_shell_prompt: true,
            latin_at_terminal_choices: true,
            shift_ascii_mode: false,
            start_in_kana: true,
            caret_scripts: true,
            greek_letters: true,
            chem_subscript: true,
            tex_mode: default_tex_mode(),
            latin_languages: vec![],
            accents_in_kana: true,
            letter_key_repeat: false,
            number_punctuation: true,
            english_predictions: false,
            japanese_predictions: false,
            fullwidth_symbols: default_fullwidth_symbols(),
            yen_key: default_yen_key(),
            domains_off: vec![],
            domains_focus: vec![],
            dialects: vec![],
            internet_slang: false,
            misuse_notes: false,
            homophone_notes: false,
            caution_slurs: false,
            caution_broadcast: false,
            punctuation_convert: false,
            typo_repair: false,
            live_keep_refined: true,
            live_short_dictionary: true,
            right_context: true,
            english_phrase_spaces: true,
            ctrl_space_toggle: false,
            ai_auto_pause: false,
            ai_pause_order: default_ai_pause_order(),
            ai_pause_notice: true,
            slash_key: default_auto(),
            period_key: default_auto(),
            homophone_replace: true,
            initial_letter_words: true,
            math_expressions: None,
            greek_case: Default::default(),
            hyouki_learning: true,
            slash_learning: true,
            latin_spell: default_latin_spell(),
            ellipsis_auto: false,
            smart_punctuation: false,
            period_shortcut: false,
            auto_capitalize: false,
            calc_candidates: false,
            mode_indicator: default_mode_indicator(),
            counter_numerals: default_counter_numerals(),
            fullwidth_digits: default_fullwidth_digits(),
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod config_tests {
    /// 2026-11-19: 遅い CPU の PC では、AI の入り切りが初めの値のままなら切り、速い版に戻ったら戻す。利用者が自分で替えていれば何もしない
    #[test]
    fn slow_cpu_turns_ai_off_only_when_untouched() {
        let mut zenzai = super::AppConfig::default().zenzai;
        assert!(zenzai.enable && !zenzai.enable_chosen && zenzai.auto_off.is_empty());
        assert!(!zenzai.apply_slow_cpu_default(false));
        assert!(zenzai.apply_slow_cpu_default(true));
        assert!(!zenzai.enable);
        assert_eq!(zenzai.auto_off, super::AI_AUTO_OFF_SLOW_CPU);
        assert!(!zenzai.apply_slow_cpu_default(true));
        // 速い版に戻ったら戻す
        assert!(zenzai.apply_slow_cpu_default(false));
        assert!(zenzai.enable && zenzai.auto_off.is_empty());
        // 自分でオンにした人・自分で切った人はそのまま
        let mut on = super::AppConfig::default().zenzai;
        on.enable_chosen = true;
        assert!(!on.apply_slow_cpu_default(true));
        assert!(on.enable);
        let mut off = super::AppConfig::default().zenzai;
        off.enable = false;
        off.enable_chosen = true;
        assert!(!off.apply_slow_cpu_default(false));
        assert!(!off.enable);
        // 前の版の settings.json (項目が無い) は初めの値のまま扱い。切った印は空なら書かない
        let old: super::ZenzaiConfig = serde_json::from_str(r#"{"enable":true,"profile":"","backend":"vulkan"}"#).unwrap();
        assert!(!old.enable_chosen && old.auto_off.is_empty());
        let written = serde_json::to_value(&old).unwrap();
        assert!(written.get("auto_off").is_none());
        assert_eq!(written["enable_chosen"], false);
    }

    /// 2026-11-19: 実際に使う AI の版のファイル (試験では一時フォルダの下)。知らない項目は書き戻しても残る。
    /// 読み書きは試験だけの決まった場所で見る (並べて流すほかの試験が APPDATA を替えるので、置き場を引き直さない)
    #[test]
    fn ai_backend_status_round_trips() {
        let status = super::AiBackendStatus {
            llama: "llama_cpu".into(),
            setting: "vulkan".into(),
            reasons: vec!["no_avx2".into()],
            ai_off: super::AI_AUTO_OFF_SLOW_CPU.into(),
            updated: 1,
            extra: serde_json::from_str(r#"{"model_error":"load_failed"}"#).unwrap(),
        };
        assert!(super::AiBackendStatus::path().unwrap().starts_with(std::env::temp_dir()));
        let dir = std::env::temp_dir().join(format!("haniword_ai_backend_test_{}", std::process::id()));
        let path = dir.join("ai_backend.json");
        status.write_to(&path).unwrap();
        assert_eq!(super::AiBackendStatus::read_from(&path), Some(status.clone()));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("model_error") && text.contains("no_avx2"));
        let plain = super::AiBackendStatus { ai_off: String::new(), extra: serde_json::Map::new(), ..status };
        plain.write_to(&path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("ai_off"));
        let _ = std::fs::remove_file(&path);
        assert_eq!(super::AiBackendStatus::read_from(&path), None);
        let _ = std::fs::remove_dir(&dir);
    }

    use super::*;

    #[test]
    fn missing_next_word_languages_lists_languages_without_tables() {
        let dir = std::env::temp_dir().join(format!("haniword_next_words_test_{}", std::process::id()));
        assert_eq!(missing_next_word_languages(&dir), None);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["en", "fr", "da", "sv", "nb", "is"] {
            std::fs::write(dir.join(format!("next_{name}.hwn")), b"x").unwrap();
        }
        let missing = missing_next_word_languages(&dir).unwrap();
        // fr は入っている。北欧は fi が無いので入っていない扱い
        assert!(!missing.contains(&"fr".to_string()));
        assert!(missing.contains(&"nordic".to_string()) && missing.contains(&"de".to_string()));
        assert_eq!(missing.len(), NEXT_WORD_LANGUAGE_FILES.len() - 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn appearance_defaults_to_the_current_look_and_is_sanitized() {
        // 2026-11-06 (仕事 37): 書いていない前の版の設定は今のまま。知らない値・危ない字は今のままに戻す
        let config: AppConfig = serde_json::from_value(serde_json::json!({"version": "0.1.0", "zenzai": {"enable": true, "profile": "", "backend": "vulkan"}})).unwrap();
        assert_eq!(config.appearance, AppearanceConfig::default());
        assert_eq!(config.appearance.font_size, 14);
        assert_eq!(config.appearance.fallback, None);
        let odd: AppearanceConfig = serde_json::from_value(serde_json::json!({
            "font": "Yu Gothic UI\"; } body { color: red", "latin_font": "Consolas", "font_size": 99, "theme": "neon",
            "selected_color": "javascript:alert(1)", "layout": "grid",
            "fallback": ["Meiryo UI", "x\"); }", "Meiryo UI", "BIZ UDPゴシック", "@notdef", "MS UI Gothic"]})).unwrap();
        let s = odd.sanitized();
        assert_eq!(s.font, "");
        assert_eq!(s.latin_font, "Consolas");
        assert_eq!(s.font_size, 28);
        assert_eq!(s.theme, "light");
        assert_eq!(s.selected_color, "");
        assert_eq!(s.layout, "standard");
        // 危ない名前と重なりを除き、止めの印より後ろは捨てる
        assert_eq!(s.fallback, Some(vec!["Meiryo UI".to_string(), "BIZ UDPゴシック".to_string(), FALLBACK_STOP.to_string()]));
        let ok: AppearanceConfig = serde_json::from_value(serde_json::json!({"font": "BIZ UDPゴシック", "selected_color": "#AABBCC", "theme": "dark"})).unwrap();
        assert_eq!(ok.sanitized().font, "BIZ UDPゴシック");
        assert_eq!(ok.sanitized().selected_color, "#aabbcc");
    }

    #[test]
    fn mode_indicator_style_defaults_to_badge() {
        // 2026-11-06 (仕事 35): 書いていない前の版の設定・知らない値は札 (今の表示)。3 つの値はそのまま
        let config: InputConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.mode_indicator_style(), "badge");
        for style in MODE_INDICATOR_STYLES {
            let config: InputConfig = serde_json::from_value(serde_json::json!({"mode_indicator": style})).unwrap();
            assert_eq!(config.mode_indicator_style(), style);
        }
        let config: InputConfig = serde_json::from_str(r#"{"mode_indicator": "balloon"}"#).unwrap();
        assert_eq!(config.mode_indicator_style(), "badge");
        assert_eq!(public_input(&config).mode_indicator, "badge");
    }

    #[test]
    fn installed_domains_are_read_from_the_domains_file() {
        // 変換サーバーと同じ domain\domains.txt から、説明の行と名前の決まりに合わない行を除いて読む。ファイルが無い・大きすぎるときは None
        let root = std::env::temp_dir().join(format!("haniword_domains_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("domain")).unwrap();
        assert_eq!(domain_names_in(&root), None);
        std::fs::write(root.join("domain").join("domains.txt"), "\u{feff}# 説明\r\nlaw\r\n religion \r\n\r\nLaw\r\n../x\r\nmath\r\n").unwrap();
        assert_eq!(domain_names_in(&root), Some(vec!["law".to_string(), "religion".to_string(), "math".to_string()]));
        std::fs::write(root.join("domain").join("domains.txt"), "law\n".repeat(20000)).unwrap();
        assert_eq!(domain_names_in(&root), None);
        // 方言の一覧 (dialects.txt) も同じ読み方。分野の一覧とは混ぜない
        assert_eq!(names_in(&root, "dialects.txt"), None);
        std::fs::write(root.join("domain").join("dialects.txt"), "# 説明\nkansai\n../x\n").unwrap();
        assert_eq!(names_in(&root, "dialects.txt"), Some(vec!["kansai".to_string()]));
        assert_eq!(domain_names_in(&root), None);
        // 2026-11-07 (仕事 43): くだけた言葉の一覧 (slang.txt) も同じ読み方で、方言の一覧とは混ぜない
        assert_eq!(names_in(&root, "slang.txt"), None);
        std::fs::write(root.join("domain").join("slang.txt"), "# 説明\ninternet\n").unwrap();
        assert_eq!(names_in(&root, "slang.txt"), Some(vec!["internet".to_string()]));
        assert_eq!(names_in(&root, "dialects.txt"), Some(vec!["kansai".to_string()]));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_choices_are_applied_once_and_validated() {
        let root = std::env::temp_dir().join(format!("haniword_choices_{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut config = AppConfig::default();
        std::fs::write(root.join("install_choices.json"), r#"{"fullwidth_symbols": "?!~x", "yen_key": "backslash"}"#).unwrap();
        assert!(apply_install_choices(&mut config, &root));
        assert_eq!(config.input.fullwidth_symbols, "!?~");
        assert_eq!(config.input.yen_key, "backslash");
        assert!(!root.join("install_choices.json").exists());
        assert!(!apply_install_choices(&mut config, &root));
        std::fs::write(root.join("install_choices.json"), r#"{"yen_key": "dollar"}"#).unwrap();
        assert!(!apply_install_choices(&mut config, &root));
        assert_eq!(config.input.yen_key, "backslash");
        std::fs::write(root.join("install_choices.json"), r#"{"latin_languages": ["es", "fr", "../x", "es", 3]}"#).unwrap();
        assert!(apply_install_choices(&mut config, &root));
        assert_eq!(config.input.latin_languages, ["fr", "es"]);
        // 専門分野: 知っている名前だけを画面の並びで残す
        assert!(config.input.domains_focus.is_empty());
        std::fs::write(root.join("install_choices.json"), r#"{"domains_focus": ["math", "law", "../x", "Law", "math", 1]}"#).unwrap();
        assert!(apply_install_choices(&mut config, &root));
        assert_eq!(config.input.domains_focus, ["law", "math"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn learn_in_flutter_defaults_off_for_new_and_legacy_configs() {
        assert!(!InputConfig::default().learn_in_flutter);
        assert!(!AppConfig::default().input.learn_in_flutter);
        let empty: InputConfig = serde_json::from_str("{}").unwrap();
        assert!(!empty.learn_in_flutter);
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["input"].as_object_mut().unwrap().remove("learn_in_flutter");
        let legacy: AppConfig = serde_json::from_value(value).unwrap();
        assert!(!legacy.input.learn_in_flutter);
    }

    #[test]
    fn english_case_fix_defaults_on_for_new_and_legacy_configs() {
        assert!(InputConfig::default().english_case_fix);
        assert!(AppConfig::default().input.english_case_fix);
        let empty: InputConfig = serde_json::from_str("{}").unwrap();
        assert!(empty.english_case_fix);
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("input");
        let legacy: AppConfig = serde_json::from_value(value).unwrap();
        assert!(legacy.input.english_case_fix);
        assert!(!legacy.input.shift_ascii_mode);
        assert!(legacy.input.latin_languages.is_empty());
        assert!(legacy.zenzai.profile.is_empty());
        // 2026-11-05 (仕事 23): 数字の全角は初期値 off (前の版の設定でも)。前の版の true は numbers、false は off、知らない値は off
        assert!(InputConfig::default().fullwidth_digits == "off" && empty.fullwidth_digits == "off" && legacy.input.fullwidth_digits == "off");
        for (json, expected) in [(r#"{"fullwidth_digits": true}"#, "numbers"), (r#"{"fullwidth_digits": false}"#, "off"),
                                 (r#"{"fullwidth_digits": "all"}"#, "all"), (r#"{"fullwidth_digits": "numbers"}"#, "numbers"), (r#"{"fullwidth_digits": "wide"}"#, "off")] {
            let read: InputConfig = serde_json::from_str(json).unwrap();
            assert_eq!(read.fullwidth_digits, expected, "{json}");
        }
    }

    /// 2026-11-03: 表記ゆれの学習は初期値オン。前の版の設定 (項目が無い) でもオン、書いてあるオフはそのまま
    #[test]
    fn hyouki_learning_defaults_on_and_keeps_written_value() {
        assert!(InputConfig::default().hyouki_learning);
        let empty: InputConfig = serde_json::from_str("{}").unwrap();
        assert!(empty.hyouki_learning);
        let off: InputConfig = serde_json::from_str(r#"{"hyouki_learning": false}"#).unwrap();
        assert!(!off.hyouki_learning);
        assert_eq!(serde_json::to_value(&off).unwrap()["hyouki_learning"], serde_json::Value::Bool(false));
        // 2026-11-05 (仕事 22 の 3): / と . の選び直しの学習も初期値オン
        assert!(InputConfig::default().slash_learning && empty.slash_learning);
        // 2026-11-06: … の自動・スマート句読点は初期値オフ
        assert!(!InputConfig::default().ellipsis_auto && !empty.ellipsis_auto);
        assert!(!InputConfig::default().smart_punctuation && !empty.smart_punctuation);
        // 仕事 34 (iOS と同じ入力の補助) も初期値オフ
        assert!(!empty.period_shortcut && !empty.auto_capitalize && !empty.calc_candidates);
        // 仕事 36: 数詞は算用数字の形が先 (今のまま)。"kanji" だけを読み、知らない値は arabic
        assert_eq!((InputConfig::default().counter_numerals.as_str(), empty.counter_numerals.as_str()), ("arabic", "arabic"));
        let kanji: InputConfig = serde_json::from_str(r#"{"counter_numerals": "kanji"}"#).unwrap();
        assert_eq!(kanji.counter_numerals, "kanji");
        let unknown: InputConfig = serde_json::from_str(r#"{"counter_numerals": "roman"}"#).unwrap();
        assert_eq!(unknown.counter_numerals, "arabic");
        let number: InputConfig = serde_json::from_str(r#"{"counter_numerals": 1, "calc_candidates": true}"#).unwrap();
        assert!(number.counter_numerals == "arabic" && number.calc_candidates);
    }

    /// 2026-10-30: ライブ変換の 2 つ (清書した前のほうを戻さない・短い入力は辞書の変換のまま) は初期値オン。前の版の設定 (項目が無い) でもオン、
    /// 2026-11-05 (仕事 27): 打っている間の候補は日本語・英語とも初期値オフ。前の項目 next_word_suggestions が true で残っていても読まず、
    /// 読み書きで落ちない (知らない項目として残る)
    #[test]
    fn typing_predictions_default_off_and_old_key_is_ignored() {
        let input = InputConfig::default();
        assert!(!input.english_predictions && !input.japanese_predictions);
        let old: InputConfig = serde_json::from_str(r#"{"next_word_suggestions": true}"#).unwrap();
        assert!(!old.english_predictions && !old.japanese_predictions);
        let written = serde_json::to_value(&old).unwrap();
        assert_eq!(written["next_word_suggestions"], serde_json::Value::Bool(true));
        assert_eq!(written["english_predictions"], serde_json::Value::Bool(false));
        let on: InputConfig = serde_json::from_str(r#"{"english_predictions": true, "japanese_predictions": true}"#).unwrap();
        assert!(on.english_predictions && on.japanese_predictions);
    }

    /// 2026-11-05 (仕事 28): 新しい切り替えは、項目の無い前の版の設定では今の動きのまま (打ち分けは自動・同音語と英字 1 字＋語はオン)。
    /// 数学の式は、書いていなければ数学か物理をよく打つ分野に選んでいるかで決まり、書いた値はそれより先。書いていない値は書き戻さない
    #[test]
    fn settings28_defaults_keep_current_behavior() {
        let old: InputConfig = serde_json::from_str(r#"{"domains_focus": ["law"]}"#).unwrap();
        assert_eq!((old.slash_key.as_str(), old.period_key.as_str()), ("auto", "auto"));
        assert!(old.homophone_replace && old.initial_letter_words && old.hyouki_learning && old.slash_learning && !old.space_after_latin);
        assert_eq!(old.math_expressions, None);
        assert!(!old.math_expressions_on());
        assert!(serde_json::to_value(&old).unwrap().get("math_expressions").is_none());
        let focus: InputConfig = serde_json::from_str(r#"{"domains_focus": ["math"]}"#).unwrap();
        assert!(focus.math_expressions_on());
        let physics: InputConfig = serde_json::from_str(r#"{"domains_focus": ["physics"]}"#).unwrap();
        assert!(physics.math_expressions_on());
        let off: InputConfig = serde_json::from_str(r#"{"domains_focus": ["math"], "math_expressions": false}"#).unwrap();
        assert!(!off.math_expressions_on());
        let on: InputConfig = serde_json::from_str(r#"{"math_expressions": true}"#).unwrap();
        assert!(on.math_expressions_on());
        assert_eq!(serde_json::to_value(&on).unwrap()["math_expressions"], serde_json::Value::Bool(true));
    }

    /// 2026-11-06 (仕事 32): ギリシャ文字の大文字・小文字の順。書いていなければ空 (変換サーバーの既定) で、書き出さない
    #[test]
    fn greek_case_defaults_empty_and_keeps_written_letters() {
        let old: InputConfig = serde_json::from_str("{}").unwrap();
        assert!(old.greek_case.is_empty());
        assert!(serde_json::to_value(&old).unwrap().get("greek_case").is_none());
        let chosen: InputConfig = serde_json::from_str(r#"{"greek_case": {"σ": "upper", "ω": "lower", "δ": "upper"}}"#).unwrap();
        assert_eq!(chosen.greek_case.get("σ").map(String::as_str), Some("upper"));
        assert_eq!(chosen.greek_case.len(), 3);
        let back = serde_json::to_value(&chosen).unwrap();
        assert_eq!(back["greek_case"]["δ"], serde_json::Value::String("upper".into()));
    }

    /// 書いてある値 (オフ・オン) はそのまま。句読点での変換・母音の直しは試験中で初期値オフのまま
    #[test]
    fn live_options_default_on_and_keep_written_values() {
        let input = InputConfig::default();
        assert!(input.live_keep_refined && input.live_short_dictionary);
        assert!(!input.punctuation_convert && !input.typo_repair);
        let empty: InputConfig = serde_json::from_str("{}").unwrap();
        assert!(empty.live_keep_refined && empty.live_short_dictionary);
        // 2026-11-08 (仕事 45): カーソルの右の字も見る変換は初期値オン。書いてあるオフはそのまま、IME 本体へ渡す設定にも
        assert!(input.right_context && empty.right_context);
        // 2026-11-08 (仕事 12): 英語の句に空白を入れるのも初期値オン。書いてあるオフはそのまま、IME 本体へ渡す設定にも
        assert!(input.english_phrase_spaces && empty.english_phrase_spaces);
        let phrase_off: InputConfig = serde_json::from_str(r#"{"english_phrase_spaces": false}"#).unwrap();
        assert!(!phrase_off.english_phrase_spaces && !public_input(&phrase_off).english_phrase_spaces);
        // 2026-11-08 (仕事 53): Ctrl+Space の切り替えは初期値オフ。書いてあるオンはそのまま、IME 本体へ渡す設定にも
        assert!(!input.ctrl_space_toggle && !empty.ctrl_space_toggle);
        let ctrl_space: InputConfig = serde_json::from_str(r#"{"ctrl_space_toggle": true}"#).unwrap();
        assert!(ctrl_space.ctrl_space_toggle && public_input(&ctrl_space).ctrl_space_toggle);
        // 2026-11-08 (仕事 52): 休んでいる間の知らせは初期値オン。並べ方は辞書だけ向け。2026-11-19: 重いときに AI を休むのは初期値オフ。書いてあるオンはそのまま
        assert!(!input.ai_auto_pause && !empty.ai_auto_pause && input.ai_pause_notice && empty.ai_pause_notice);
        let paused_on: InputConfig = serde_json::from_str(r#"{"ai_auto_pause": true}"#).unwrap();
        assert!(paused_on.ai_auto_pause);
        assert_eq!((input.ai_pause_order.as_str(), empty.ai_pause_order.as_str()), ("dictionary", "dictionary"));
        let paused_off: InputConfig = serde_json::from_str(r#"{"ai_auto_pause": false, "ai_pause_order": "preview", "ai_pause_notice": false}"#).unwrap();
        assert!(!paused_off.ai_auto_pause && !paused_off.ai_pause_notice && paused_off.ai_pause_order == "preview");
        assert!(!public_input(&paused_off).ai_pause_notice);
        let off: InputConfig = serde_json::from_str(r#"{"right_context": false}"#).unwrap();
        assert!(!off.right_context && !public_input(&off).right_context);
        assert_eq!(serde_json::to_value(&off).unwrap()["right_context"], serde_json::Value::Bool(false));
        assert!(!empty.punctuation_convert && !empty.typo_repair);
        // 2026-11-05: つづりの指摘・自動修正も初期値はオフ (項目が無い前の版の設定でも)
        assert_eq!(input.latin_spell, "off");
        assert_eq!(empty.latin_spell, "off");
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        let fields = value["input"].as_object_mut().unwrap();
        fields.remove("live_keep_refined");
        fields.remove("live_short_dictionary");
        let legacy: AppConfig = serde_json::from_value(value.clone()).unwrap();
        assert!(legacy.input.live_keep_refined && legacy.input.live_short_dictionary);
        for (keep, short) in [(false, false), (false, true), (true, false), (true, true)] {
            value["input"]["live_keep_refined"] = serde_json::json!(keep);
            value["input"]["live_short_dictionary"] = serde_json::json!(short);
            let written: AppConfig = serde_json::from_value(value.clone()).unwrap();
            assert_eq!((written.input.live_keep_refined, written.input.live_short_dictionary), (keep, short));
            // IME 本体へ渡す設定にもそのまま
            let public = serde_json::json!({"input": public_input(&written.input)});
            assert_eq!((public["input"]["live_keep_refined"].as_bool(), public["input"]["live_short_dictionary"].as_bool()), (Some(keep), Some(short)));
        }
    }

    /// 2026-10-29: 新しい版の知らせは初期値オン。前の版の settings.json (項目が無い) でもオン、切った人はオフのまま
    #[test]
    fn update_notices_default_on_and_keep_an_explicit_off() {
        assert!(AppConfig::default().notify_updates);
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("notify_updates");
        assert!(serde_json::from_value::<AppConfig>(value.clone()).unwrap().notify_updates);
        value["notify_updates"] = serde_json::json!(false);
        let off: AppConfig = serde_json::from_value(value).unwrap();
        assert!(!off.notify_updates);
        assert_eq!(serde_json::to_value(&off).unwrap()["notify_updates"], false);
        assert!(!off.extra.contains_key("notify_updates"));
        // 設定画面の外 (IME 本体) へは渡さない
        assert!(!serde_json::json!({"input": public_input(&off.input)}).to_string().contains("notify_updates"));
    }

    #[test]
    fn explicit_case_fix_off_and_unrelated_settings_are_preserved() {
        let config: InputConfig = serde_json::from_value(serde_json::json!({
            "english_case_fix": false, "latin_languages": ["es"], "extra_setting": 42
        })).unwrap();
        assert!(!config.english_case_fix);
        assert_eq!(config.latin_languages, ["es"]);
        assert_eq!(serde_json::to_value(config).unwrap()["extra_setting"], 42);
    }

    #[test]
    fn public_preferences_exclude_profile_and_unknown_free_text() {
        let mut config = AppConfig::default();
        config.zenzai.profile = "private profile".into();
        config.extra.insert("secret".into(), serde_json::json!("private extra"));
        config.input.extra.insert("secret".into(), serde_json::json!("private input extra"));
        config.input.english_case_fix = false;
        config.input.latin_languages = vec!["es".into(), "es".into(), "unknown private language".into()];
        let value = serde_json::json!({"input": public_input(&config.input)});
        let text = value.to_string();
        assert!(!text.contains("private"));
        assert!(value.get("zenzai").is_none());
        assert!(value.get("secret").is_none());
        assert_eq!(value["input"]["latin_languages"], serde_json::json!(["es"]));
        assert_eq!(value["input"]["english_case_fix"], false);
    }

    #[test]
    fn public_preferences_reject_unknown_modes_and_limit_prefixes() {
        let mut input = InputConfig::default();
        input.romaji_style = "private arbitrary text".into();
        input.tex_mode = "private arbitrary text".into();
        input.direct_prefixes = "password/$@################################".into();
        let public = public_input(&input);
        assert_eq!(public.romaji_style, "auto");
        assert_eq!(public.tex_mode, "auto");
        assert_eq!(public.direct_prefixes.len(), 16);
        assert!(public.direct_prefixes.chars().all(|c| c.is_ascii_punctuation()));
    }

    #[test]
    fn domains_off_keeps_only_file_name_safe_names() {
        let mut input = InputConfig::default();
        assert!(input.domains_off.is_empty());
        input.domains_off = vec!["law".into(), "../x".into(), "Law".into(), "law".into(), "it_terms".into(), "".into()];
        assert_eq!(public_input(&input).domains_off, vec!["it_terms".to_string(), "law".to_string()]);
        assert!(input.domains_focus.is_empty());
        input.domains_focus = vec!["math".into(), "../x".into(), "math".into()];
        assert_eq!(public_input(&input).domains_focus, vec!["math".to_string()]);
        // 2026-11-06 (仕事 39): 方言は初期値で使わない。名前の決まりは分野と同じ
        assert!(input.dialects.is_empty());
        input.dialects = vec!["kansai".into(), "../x".into(), "Kansai".into(), "kansai".into()];
        assert_eq!(public_input(&input).dialects, vec!["kansai".to_string()]);
        let old: InputConfig = serde_json::from_str(r#"{"domains_off": ["law"]}"#).unwrap();
        assert!(old.dialects.is_empty());
        // 2026-11-07 (仕事 43): ネットスラングは初期値でオフ。前の版の設定 (書いていない) もオフ
        assert!(!input.internet_slang);
        assert!(!old.internet_slang);
        let on: InputConfig = serde_json::from_str(r#"{"internet_slang": true}"#).unwrap();
        assert!(on.internet_slang && public_input(&on).internet_slang);
    }

    /// 2026-11-10: 試験のビルドでは、利用者のデータの置き場は本物の %APPDATA% にならない (一時フォルダの中を指す APPDATA だけはそのまま)
    #[test]
    fn tests_never_use_the_real_app_data() {
        let dir = app_data_dir().expect("APPDATA");
        assert!(dir.starts_with(std::env::temp_dir()), "{}", dir.display());
        if let Some(real) = std::env::var_os("APPDATA").map(PathBuf::from).filter(|p| !p.starts_with(std::env::temp_dir())) {
            assert!(!dir.starts_with(&real), "{}", dir.display());
        }
    }

    #[test]
    fn opening_existing_settings_does_not_rewrite_explicit_off() {
        struct Fixture { root: PathBuf, original: Option<std::ffi::OsString> }
        impl Drop for Fixture {
            fn drop(&mut self) {
                match &self.original {
                    Some(value) => std::env::set_var("APPDATA", value),
                    None => std::env::remove_var("APPDATA"),
                }
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }
        let fixture = Fixture {
            root: std::env::temp_dir().join(format!("azookey_config_fixture_{}", std::process::id())),
            original: std::env::var_os("APPDATA"),
        };
        std::fs::create_dir_all(fixture.root.join(APP_DIR)).unwrap();
        let mut config = AppConfig::default();
        config.input.english_case_fix = false;
        let original = serde_json::to_vec_pretty(&config).unwrap();
        let path = fixture.root.join(APP_DIR).join("settings.json");
        std::fs::write(&path, &original).unwrap();
        std::env::set_var("APPDATA", &fixture.root);
        assert!(!AppConfig::new().input.english_case_fix);
        assert_eq!(std::fs::read(&path).unwrap(), original);

        // 2026-10-30: 新しく入れたとき (インストーラーの「おすすめの設定ですぐに使う」は install_choices.json を書かない) は、
        // 書き出す settings.json と IME 本体へ渡す設定で、ライブ変換の 2 つがオン・新しい版の知らせがオン
        std::fs::remove_file(&path).unwrap();
        let fresh = AppConfig::new();
        assert!(fresh.input.live_keep_refined && fresh.input.live_short_dictionary && fresh.notify_updates);
        let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["input"]["live_keep_refined"], true);
        assert_eq!(written["input"]["live_short_dictionary"], true);
        assert_eq!(written["notify_updates"], true);
        let client: serde_json::Value = serde_json::from_slice(&std::fs::read(client_settings_path()).unwrap()).unwrap();
        assert_eq!(client["input"]["live_keep_refined"], true);
        assert_eq!(client["input"]["live_short_dictionary"], true);
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppConfig {
    pub version: String,
    pub zenzai: ZenzaiConfig,
    #[serde(default)]
    pub input: InputConfig,
    /// 2026-10-29: 新しい版を知らせる (1 日 1 回 GitHub の最新のリリースを確かめ、新しければ Windows の通知。launcher が確かめる)
    #[serde(default = "default_true")]
    pub notify_updates: bool,
    /// 2026-11-06 (仕事 37): 候補の窓の見た目 (設定アプリの「外観」)。どれも初期値は今のまま。表示の窓 (ui.exe) が読む
    #[serde(default)]
    pub appearance: AppearanceConfig,
    /// このプログラムが知らない項目 (新しい版が足したもの)。書き戻すときに消さない
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// 2026-11-06 (仕事 37): 候補の窓の見た目。表示の窓は `sanitized` を通したものだけを使う (フォントの名前は CSS に入るので、字を限る)
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct AppearanceConfig {
    /// 候補のフォント (日本語)。空なら今のまま (Yu Gothic UI、無ければ Meiryo UI)
    #[serde(default)]
    pub font: String,
    /// 英字 (と数字・記号) だけ別のフォントにする。空なら日本語のフォントと同じ
    #[serde(default)]
    pub latin_font: String,
    /// 候補の字の大きさ (px)。初期値 14 (今のまま)。CANDIDATE_FONT_SIZES の範囲
    #[serde(default = "default_candidate_font_size")]
    pub font_size: u32,
    /// 色の組: "light" (今のまま)・"dark"・"system" (Windows のアプリのモードに合わせる)
    #[serde(default = "default_candidate_theme")]
    pub theme: String,
    /// 選んだ候補の色: "" (今のまま、薄い青)・"accent" (Windows のアクセントの色)・"#rrggbb"
    #[serde(default)]
    pub selected_color: String,
    /// 並べ方: "standard" (今の形)・"flat" (番号の列に色を付けず、選んだ行を塗る)・"compact" (行の間を詰める)
    #[serde(default = "default_candidate_layout")]
    pub layout: String,
    /// 2026-11-06 (仕事 37 の足し): 候補のフォントに無い字を補うフォントの並び。書いていなければ今のまま (候補のフォントが今のままなら Meiryo UI、
    /// そのあと Windows がほかのフォントで補う)。並びを書けばその順に補い、FALLBACK_STOP ("@notdef") を置くとそこで止めて、無い字を □ で出す
    /// (それより後ろは設定に残っていても使わない。設定アプリは印を動かせるように残す)。止めないときは並びのあとに Windows が補う。絵文字は止めても Segoe UI Emoji で出す
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Vec<String>>,
    /// このプログラムが知らない項目。書き戻すときに消さない
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// 候補の字の大きさの範囲 (px)
pub const CANDIDATE_FONT_SIZES: std::ops::RangeInclusive<u32> = 10..=28;
pub const CANDIDATE_THEMES: [&str; 3] = ["light", "dark", "system"];
pub const CANDIDATE_LAYOUTS: [&str; 3] = ["standard", "flat", "compact"];
/// 補うフォントの並びで「ここで止めて □ で出す」印 (フォントの名前には @ を入れられないので重ならない)
pub const FALLBACK_STOP: &str = "@notdef";

fn default_candidate_font_size() -> u32 {
    14
}

fn default_candidate_theme() -> String {
    "light".to_string()
}

fn default_candidate_layout() -> String {
    "standard".to_string()
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        AppearanceConfig {
            font: String::new(),
            latin_font: String::new(),
            font_size: default_candidate_font_size(),
            theme: default_candidate_theme(),
            selected_color: String::new(),
            layout: default_candidate_layout(),
            fallback: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// フォントの名前として CSS に入れてよいか (64 字まで。引用符・バックスラッシュ・区切りの記号・制御文字を含まない)
fn safe_font_name(name: &str) -> bool {
    name.chars().count() <= 64 && !name.trim().is_empty()
        && name.chars().all(|c| !c.is_control() && !"\"'\\;:{}<>()[],@/*`".contains(c))
}

impl AppearanceConfig {
    /// 表示の窓が使う形: 知らない値は今のままに戻し、大きさは範囲に収め、フォントの名前は安全な字だけのものを残す
    pub fn sanitized(&self) -> AppearanceConfig {
        let font = |f: &str| if safe_font_name(f) { f.trim().to_string() } else { String::new() };
        let color = &self.selected_color;
        let hex = color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit());
        AppearanceConfig {
            font: font(&self.font),
            latin_font: font(&self.latin_font),
            font_size: self.font_size.clamp(*CANDIDATE_FONT_SIZES.start(), *CANDIDATE_FONT_SIZES.end()),
            theme: if CANDIDATE_THEMES.contains(&self.theme.as_str()) { self.theme.clone() } else { default_candidate_theme() },
            selected_color: if color == "accent" || hex { color.to_ascii_lowercase() } else { String::new() },
            layout: if CANDIDATE_LAYOUTS.contains(&self.layout.as_str()) { self.layout.clone() } else { default_candidate_layout() },
            // 補うフォントの並び: 安全な名前と止めの印だけを、重ねず 8 つまで。止めの印より後ろは捨てる
            fallback: self.fallback.as_ref().map(|list| {
                let mut out: Vec<String> = Vec::new();
                for f in list {
                    if f == FALLBACK_STOP {
                        out.push(FALLBACK_STOP.to_string());
                        break;
                    }
                    let f = f.trim();
                    if safe_font_name(f) && !out.iter().any(|x| x == f) && out.len() < 8 {
                        out.push(f.to_string());
                    }
                }
                out
            }),
            extra: serde_json::Map::new(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            version: "0.1.0".to_string(),
            zenzai: ZenzaiConfig {
                enable: true,
                profile: "".to_string(),
                backend: "vulkan".to_string(),
                model: default_model(),
                model_version: default_model_version(),
                inference_limit: default_inference_limit(),
                enable_chosen: false,
                auto_off: String::new(),
                extra: serde_json::Map::new(),
            },
            input: InputConfig::default(),
            notify_updates: true,
            appearance: AppearanceConfig::default(),
            extra: serde_json::Map::new(),
        }
    }
}

/// 2026-10-20: インストーラーで選んだ設定 (install_choices.json の fullwidth_symbols・yen_key・latin_languages) を一度だけ取り込み、ファイルを消す。
/// インストーラーは利用者の権限でこのファイルだけを書く (完全な settings.json の既定値を別に持たないため)。取り込んだら true
fn apply_install_choices(config: &mut AppConfig, root: &std::path::Path) -> bool {
    let path = root.join("install_choices.json");
    let Ok(meta) = std::fs::symlink_metadata(&path) else { return false };
    let text = if meta.is_file() && meta.len() <= 4096 { std::fs::read_to_string(&path).ok() } else { None };
    let _ = std::fs::remove_file(&path);
    let Some(value) = text.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return false };
    let mut applied = false;
    if let Some(symbols) = value.get("fullwidth_symbols").and_then(|v| v.as_str()) {
        config.input.fullwidth_symbols = CONFIGURABLE_SYMBOLS.chars().filter(|c| symbols.contains(*c)).collect();
        applied = true;
    }
    if let Some(key) = value.get("yen_key").and_then(|v| v.as_str()).filter(|k| ["yen", "backslash"].contains(k)) {
        config.input.yen_key = key.to_string();
        applied = true;
    }
    if let Some(languages) = value.get("latin_languages").and_then(|v| v.as_array()) {
        config.input.latin_languages = LATIN_LANGUAGES.iter()
            .filter(|code| languages.iter().any(|v| v.as_str() == Some(**code)))
            .map(|code| code.to_string()).collect();
        applied = true;
    }
    // 2026-10-30: インストーラーの専門分野の画面で選んだ分野 (知っている名前だけ。選ばなければ鍵が無く、今の初期値のまま)
    if let Some(domains) = value.get("domains_focus").and_then(|v| v.as_array()) {
        config.input.domains_focus = DOMAIN_NAMES.iter()
            .filter(|name| domains.iter().any(|v| v.as_str() == Some(**name)))
            .map(|name| name.to_string()).collect();
        applied = true;
    }
    applied
}

/// 同じフォルダの一時ファイルに書いてから置き換える (Windows の rename は既存のファイルを置き換える)
fn write_atomic(path: &std::path::Path, contents: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, contents)?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

impl AppConfig {
    /// settings.json の場所 (変更を見張るため)
    pub fn path() -> PathBuf {
        if winsec::private_data_allowed() { get_config_root().join(SETTINGS_FILENAME) }
        else { client_settings_path() }
    }

    /// 2026-10-19 (security review #8): 一時ファイルに書いてから置き換える (書きかけで落ちても settings.json が壊れない)。
    /// 失敗しても落ちずに知らせるだけ
    pub fn write(&self) {
        if let Err(error) = self.try_write() { eprintln!("Cannot write settings: {error}"); }
    }

    pub fn try_write(&self) -> io::Result<()> {
        if !winsec::private_data_allowed() { return Ok(()); }
        let config_path = get_config_root().join(SETTINGS_FILENAME);
        write_atomic(&config_path, serde_json::to_string_pretty(self)?.as_bytes())?;
        if let Err(error) = self.publish_client_settings() { eprintln!("Cannot publish client preferences: {error}"); }
        Ok(())
    }

    pub fn publish_client_settings(&self) -> io::Result<()> {
        if !winsec::private_data_allowed() { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Private settings require an unelevated desktop token")); }
        let path = client_settings_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let contents = serde_json::to_vec_pretty(&serde_json::json!({"input": public_input(&self.input)}))?;
        write_atomic(&path, &contents)
    }

    pub fn read() -> Self {
        if !winsec::private_data_allowed() { return Self::read_or_default(); }
        let config_path = get_config_root().join(SETTINGS_FILENAME);
        if !config_path.exists() {
            return AppConfig::default();
        }
        // 読めない・壊れた設定でも落ちずに既定値で動く (ファイルはそのまま残し、上書きは設定を保存したときだけ)
        match std::fs::read_to_string(&config_path).map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string())) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("Cannot read settings ({}): {error}", config_path.display());
                AppConfig::default()
            }
        }
    }

    /// 失敗しても落ちずに既定値を返す読み込み (アプリの中で動く IME 本体から使う)
    pub fn read_or_default() -> Self {
        if !winsec::private_data_allowed() { return read_client_settings(&client_settings_path()).unwrap_or_default(); }
        let config_path = get_config_root().join(SETTINGS_FILENAME);
        std::fs::read_to_string(config_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn new() -> Self {
        if !winsec::private_data_allowed() { return Self::read_or_default(); }
        let config_path = get_config_root();
        if !config_path.exists() {
            if let Err(error) = std::fs::create_dir_all(&config_path) {
                eprintln!("Cannot create the settings folder: {error}");
                return AppConfig::default();
            }
        }
        // 設定ファイルがあるときは書き戻さない (古い版のプログラムが、知らない項目を消してしまうため)。
        // ただしインストール時に選んだ設定 (install_choices.json) があれば、取り込んで書き戻す
        let exists = get_config_root().join(SETTINGS_FILENAME).exists();
        let mut config = AppConfig::read();
        let chosen = apply_install_choices(&mut config, &get_config_root());
        if !exists || chosen {
            config.write();
        } else if let Err(error) = config.publish_client_settings() {
            eprintln!("Cannot publish client preferences: {error}");
        }
        config
    }
}
