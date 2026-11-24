// 行頭や空白の後に `/` `$` `@` を打つと、その語は変換せずにアプリへ直接渡す (コマンド名・ファイル名の入力。
// アプリの補完を使える)。空白で普通の入力に戻る (/review kyouha → /review 今日は)。
// 2026-10-13: / のあとは、知っているコマンド・英単語の頭・ローマ字で読めない並びの間だけ素通しにする。
// 日本語として読めるようになったら (/wo → を)、渡した文字を Backspace で消して打ち直し、日本語の入力に戻す (slash_commands.rs)
//
// キー 1 回につき OnTestKeyDown と OnKeyDown の 2 回呼ばれるので、同じキーの判定は 1 回だけ行い結果を使い回す。

use std::cell::Cell;
use std::sync::OnceLock;

use super::user_action::UserAction;

/// 前の文字が分かったか
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Preceding {
    /// 入力欄・行の先頭
    Start,
    Char(char),
    /// アプリが文字を読ませてくれない
    Unknown,
}

/// / のあとに覚えておく語の長さの上限 (これより長い語は日本語に戻さない)
pub const WORD_MAX: usize = 32;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct State {
    /// 素通し中か
    pub active: bool,
    /// 素通しで打った文字数 (Backspace で記号まで消したら素通しをやめる)
    pub len: u32,
    /// / で始めたか (日本語に戻すのは / のときだけ。$skill・@ファイル名 はいつも素通し)
    pub slash: bool,
    /// / のあとに打った小文字の英字とハイフン (日本語に戻すときに打ち直す)
    pub word: [u8; WORD_MAX],
    pub word_len: u8,
    /// 大文字・数字・記号を打った (コマンドかパスなので日本語には戻さない)
    pub settled: bool,
}

impl State {
    fn word(&self) -> &str {
        std::str::from_utf8(&self.word[..self.word_len as usize]).unwrap_or("")
    }
}

/// このキーをどう扱うか
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decision {
    /// アプリへそのまま渡す
    Pass,
    /// いつもどおり IME で扱う
    Normal,
    /// 素通しをやめて日本語に戻す: 渡した delete 文字を Backspace で消し、retype を打ち直す (このキーは飲み込む)
    Retype { delete: u8, retype: [u8; WORD_MAX], len: u8 },
}

impl Decision {
    pub fn retype_text(&self) -> Option<(usize, String)> {
        match self {
            Decision::Retype { delete, retype, len } => {
                Some((*delete as usize, String::from_utf8_lossy(&retype[..*len as usize]).into_owned()))
            }
            _ => None,
        }
    }
}

/// 素通しを始める記号 (設定 input.direct_prefixes。空なら使わない)
pub fn prefixes() -> &'static str {
    static PREFIXES: OnceLock<String> = OnceLock::new();
    PREFIXES.get_or_init(|| shared::AppConfig::read_or_default().input.direct_prefixes)
}

fn is_boundary(preceding: Preceding, fallback: bool) -> bool {
    match preceding {
        Preceding::Start => true,
        Preceding::Char(c) => c.is_whitespace(),
        Preceding::Unknown => fallback,
    }
}

/// 未確定文字列がなく、かな入力のときに 1 キー分進める。
/// `preceding` は記号を打ったときだけ呼ぶ (アプリへの問い合わせは重いので)
pub fn step(
    state: &mut State,
    action: &UserAction,
    prefixes: &str,
    preceding: impl FnOnce() -> Preceding,
    fallback: bool,
    stays_latin: impl Fn(&str) -> bool,
) -> Decision {
    match action {
        // Shift などの修飾キーだけの押下は素通しの状態を変えない
        UserAction::Unknown => {
            if state.active {
                Decision::Pass
            } else {
                Decision::Normal
            }
        }
        UserAction::Input(c) if !state.active && prefixes.contains(*c) => {
            if is_boundary(preceding(), fallback) {
                *state = State { active: true, len: 1, slash: *c == '/', ..State::default() };
                Decision::Pass
            } else {
                Decision::Normal
            }
        }
        // ハイフンも語の一部として見る (同じキーが ー なので、pr-comments は英字のまま、ra-menn は ラーメン に戻す)
        UserAction::Input(c) if state.active && state.slash && !state.settled && (c.is_ascii_lowercase() || (*c == '-' && state.word_len > 0))
            && (state.word_len as usize) < WORD_MAX - 1 => {
            let mut word = state.word;
            word[state.word_len as usize] = *c as u8;
            let len = state.word_len + 1;
            let text = std::str::from_utf8(&word[..len as usize]).unwrap_or("");
            if stays_latin(text) {
                state.word = word;
                state.word_len = len;
                state.len += 1;
                Decision::Pass
            } else {
                let delete = state.word_len;
                *state = State::default();
                Decision::Retype { delete, retype: word, len }
            }
        }
        UserAction::Input(_) | UserAction::Number(_) if state.active => {
            state.len += 1;
            // 大文字・数字・記号 (ハイフン以外) のあとは日本語に戻さない (/Model・/v2・/mnt/c)
            state.settled = true;
            Decision::Pass
        }
        UserAction::Backspace if state.active => {
            state.len = state.len.saturating_sub(1);
            if state.word_len > 0 && !state.settled {
                state.word_len -= 1;
            }
            if state.len == 0 {
                state.active = false;
            }
            Decision::Pass
        }
        // 空白は半角のままアプリへ。ここで素通しは終わり
        UserAction::Space if state.active => {
            *state = State::default();
            Decision::Pass
        }
        _ => {
            *state = State::default();
            Decision::Normal
        }
    }
}

/// IME が読み込まれているアプリが端末か (行頭が分からなくても行頭とみなしてよい)
pub fn host_is_terminal() -> bool {
    static TERMINAL: OnceLock<bool> = OnceLock::new();
    *TERMINAL.get_or_init(|| {
        let exe = host_exe().to_ascii_lowercase();
        [
            "windowsterminal.exe",
            "openconsole.exe",
            "conhost.exe",
            "wezterm-gui.exe",
            "alacritty.exe",
            "mintty.exe",
            "tabby.exe",
            "hyper.exe",
            "warp.exe",
        ]
        .contains(&exe.as_str())
    })
}

/// IME の DLL を読み込んでいるアプリの exe 名
pub fn host_exe() -> &'static str {
    static EXE: OnceLock<String> = OnceLock::new();
    EXE.get_or_init(|| {
        use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
        let mut buffer = [0u16; 1024];
        let length = unsafe { GetModuleFileNameW(None, &mut buffer) } as usize;
        let path = String::from_utf16_lossy(&buffer[..length]);
        path.rsplit(['\\', '/']).next().unwrap_or_default().to_string()
    })
}

thread_local! {
    static STATE: Cell<State> = const { Cell::new(State { active: false, len: 0, slash: false, word: [0; WORD_MAX], word_len: 0, settled: false }) };
    /// 直前に判定したキー (メッセージの時刻, 仮想キー) とその結果
    static LAST: Cell<Option<(i32, usize, Decision)>> = const { Cell::new(None) };
}

/// 同じキーの 2 回目の呼び出しなら前の結果を返す。初回なら `decide` で決めて覚える
pub fn decide_once(wparam: usize, decide: impl FnOnce(&mut State) -> Decision) -> Decision {
    let time = unsafe { windows::Win32::UI::WindowsAndMessaging::GetMessageTime() };
    if let Some((t, w, decision)) = LAST.get() {
        if t == time && w == wparam {
            return decision;
        }
    }
    let mut state = STATE.get();
    let decision = decide(&mut state);
    STATE.set(state);
    LAST.set(Some((time, wparam, decision)));
    decision
}

/// 未確定文字列があるときなど、素通しの対象外になったら呼ぶ
pub fn reset() {
    STATE.set(State::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(keys: &[UserAction], preceding: Preceding) -> Vec<Decision> {
        let mut state = State::default();
        keys.iter().map(|k| step(&mut state, k, "/$@", || preceding, false, |_| true)).collect()
    }

    use Decision::{Normal as N, Pass as P};
    use UserAction as K;

    #[test]
    fn command_then_space_then_japanese() {
        let keys = [K::Input('/'), K::Input('r'), K::Input('e'), K::Space, K::Input('k')];
        assert_eq!(run(&keys, Preceding::Start), [P, P, P, P, N]);
    }

    #[test]
    fn email_at_is_not_a_prefix() {
        assert_eq!(run(&[K::Input('@'), K::Input('a')], Preceding::Char('o')), [N, N]);
        assert_eq!(run(&[K::Input('@'), K::Input('a')], Preceding::Char(' ')), [P, P]);
        assert_eq!(run(&[K::Input('$')], Preceding::Char('\n')), [P]);
    }

    #[test]
    fn backspace_over_the_prefix_ends_it() {
        let keys = [K::Input('@'), K::Input('s'), K::Backspace, K::Backspace, K::Input('a')];
        assert_eq!(run(&keys, Preceding::Start), [P, P, P, P, N]);
    }

    #[test]
    fn shift_alone_keeps_it_and_enter_ends_it() {
        let keys = [K::Input('/'), K::Unknown, K::Input('A'), K::Enter, K::Input('a')];
        assert_eq!(run(&keys, Preceding::Start), [P, P, P, N, N]);
    }

    #[test]
    fn unknown_preceding_uses_fallback() {
        let mut state = State::default();
        assert_eq!(step(&mut state, &K::Input('/'), "/$@", || Preceding::Unknown, true, |_| true), P);
        let mut state = State::default();
        assert_eq!(step(&mut state, &K::Input('/'), "/$@", || Preceding::Unknown, false, |_| true), N);
        let mut state = State::default();
        assert_eq!(step(&mut state, &K::Input('/'), "", || Preceding::Start, true, |_| true), N);
    }

    /// 2026-10-13: / のあとで日本語として読めるようになったら打ち直す。コマンドの頭・英単語の頭のうちは素通し
    #[test]
    fn slash_returns_to_japanese_when_the_word_is_not_a_command() {
        let stays = |w: &str| ["model", "mcp", "clear"].iter().any(|c| c.starts_with(w));
        let mut state = State::default();
        let keys = [K::Input('/'), K::Input('m'), K::Input('o'), K::Input('d')];
        let got: Vec<_> = keys.iter().map(|k| step(&mut state, k, "/$@", || Preceding::Start, false, stays)).collect();
        assert_eq!(got, [P, P, P, P]);
        let mut state = State::default();
        let keys = [K::Input('/'), K::Input('m'), K::Input('a')];
        let got: Vec<_> = keys.iter().map(|k| step(&mut state, k, "/$@", || Preceding::Start, false, stays)).collect();
        assert_eq!(got[..2], [P, P]);
        assert_eq!(got[2].retype_text(), Some((1, "ma".to_string())));
        assert!(!state.active);
        // 大文字・数字・記号を打ったあとは戻さない。$ と @ はいつも素通し
        let mut state = State::default();
        let keys = [K::Input('/'), K::Input('M'), K::Input('a')];
        let got: Vec<_> = keys.iter().map(|k| step(&mut state, k, "/$@", || Preceding::Start, false, |_| false)).collect();
        assert_eq!(got, [P, P, P]);
        let mut state = State::default();
        let keys = [K::Input('@'), K::Input('a')];
        let got: Vec<_> = keys.iter().map(|k| step(&mut state, k, "/$@", || Preceding::Start, false, |_| false)).collect();
        assert_eq!(got, [P, P]);
    }
}
