// TeX 版 (追加インストール。tex_commands.txt があれば入っている)。
//
// TeX を書いているとき:
//   ・\frac のようなコマンドは英字のまま
//   ・$…$ $$…$$ \(…\) \[…\] の中は、打ったキーをそのままアプリへ渡す (英字)
//   ・^ _ の上付き・下付き、ギリシャ文字、化学式の下付きはしない (TeX の記法そのものなので)
//
// いつ TeX を書いているとみなすかは設定 input.tex_mode (タスクバーの「あ」の右クリックで切り替え):
//   auto = 開いているファイルが .tex など (ウィンドウのタイトルで判定) か、TeX 専用のエディタのとき / on = いつも / off = 使わない

use std::cell::RefCell;
use std::sync::Mutex;

use super::user_action::UserAction;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TexMode {
    Auto,
    On,
    Off,
}

impl TexMode {
    pub fn as_str(self) -> &'static str {
        match self {
            TexMode::Auto => "auto",
            TexMode::On => "on",
            TexMode::Off => "off",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "on" => TexMode::On,
            "off" => TexMode::Off,
            _ => TexMode::Auto,
        }
    }
}

/// TeX 版が入っているか
pub fn installed() -> bool {
    static INSTALLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *INSTALLED.get_or_init(|| {
        super::auto_english::dll_dir().is_some_and(|dir| dir.join("tex_commands.txt").exists())
    })
}

/// 設定は右クリックのメニューでいつでも変わるので、settings.json が変わったら読み直す
static MODE: Mutex<Option<(Option<std::time::SystemTime>, TexMode)>> = Mutex::new(None);

pub fn mode() -> TexMode {
    let path = shared::AppConfig::path();
    let modified = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
    let mut guard = MODE.lock().unwrap_or_else(|e| e.into_inner());
    match *guard {
        Some((m, mode)) if m == modified => mode,
        _ => {
            let mode = TexMode::parse(&shared::AppConfig::read_or_default().input.tex_mode);
            *guard = Some((modified, mode));
            mode
        }
    }
}

pub fn set_mode(mode: TexMode) {
    let mut config = shared::AppConfig::read_or_default();
    config.input.tex_mode = mode.as_str().to_string();
    config.write();
    crate::debug_log::write(&format!("tex: mode -> {}", mode.as_str()));
}

/// TeX のファイルを開いているように見えるか (ウィンドウのタイトルとアプリ)
fn looks_like_tex() -> bool {
    let exe = super::prefix_direct::host_exe().to_ascii_lowercase();
    if ["texworks.exe", "texstudio.exe", "texmaker.exe", "winedt.exe", "lyx.exe", "tex2img.exe"].contains(&exe.as_str()) {
        return true;
    }
    let title = super::shell_prompt::foreground_title().to_ascii_lowercase();
    [".tex", ".sty", ".cls", ".bib", ".ltx", ".dtx"].iter().any(|ext| {
        title.match_indices(ext).any(|(i, _)| {
            // 拡張子の直後が英数字でない (.text や .texture は除く)
            title[i + ext.len()..].chars().next().map_or(true, |c| !c.is_ascii_alphanumeric())
        })
    })
}

/// いま TeX を書いているか
pub fn active() -> bool {
    if !installed() {
        return false;
    }
    match mode() {
        TexMode::On => true,
        TexMode::Off => false,
        TexMode::Auto => looks_like_tex(),
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum Math {
    #[default]
    Off,
    /// $ … $
    Inline,
    /// $$ … $$
    Display,
    /// \( … \) か \[ … \]
    Bracket,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct State {
    pub math: Math,
    /// 数式に入ってから打った文字数 (Backspace で開きまで消したら抜ける)
    len: u32,
    /// 直前が \ (\( \[ \) \] を見分ける。数式の外ではコマンドの途中)
    backslash: bool,
    /// \ に続けてコマンドの英字を打っている途中 (数式の外。英字のままアプリへ渡す)
    command: bool,
    /// $$ の閉じの 1 つ目の $ を打った
    closing: bool,
    /// $ を打った直後 (続けて $ なら $$)
    just_opened: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decision {
    Pass,
    Normal,
}

/// 未確定文字列がないときの 1 キー
pub fn step(state: &mut State, action: &UserAction) -> Decision {
    use Decision::{Normal, Pass};
    let in_math = state.math != Math::Off;
    let was_backslash = std::mem::take(&mut state.backslash);
    let just_opened = std::mem::take(&mut state.just_opened);
    let closing = std::mem::take(&mut state.closing);
    match action {
        UserAction::Unknown => {
            state.backslash = was_backslash;
            state.just_opened = just_opened;
            state.closing = closing;
            if in_math || state.command { Pass } else { Normal }
        }
        UserAction::Input('$') if !was_backslash => {
            match state.math {
                Math::Off => {
                    *state = State { math: Math::Inline, len: 1, just_opened: true, ..State::default() };
                }
                Math::Inline if just_opened => {
                    state.math = Math::Display;
                    state.len += 1;
                }
                Math::Inline => *state = State::default(),
                Math::Display if closing => *state = State::default(),
                Math::Display => {
                    state.closing = true;
                    state.len += 1;
                }
                Math::Bracket => state.len += 1,
            }
            Pass
        }
        UserAction::Input('\\') => {
            state.backslash = !was_backslash;
            if in_math {
                state.len += 1;
            } else {
                state.command = true;
            }
            Pass
        }
        UserAction::Input(c) if was_backslash && (*c == '(' || *c == '[') && !in_math => {
            *state = State { math: Math::Bracket, len: 2, ..State::default() };
            Pass
        }
        UserAction::Input(c) if was_backslash && (*c == ')' || *c == ']') && state.math == Math::Bracket => {
            *state = State::default();
            Pass
        }
        UserAction::Input(c) if state.command && !in_math => {
            if c.is_ascii_alphabetic() || (was_backslash && !c.is_alphanumeric()) {
                // \frac の英字、\\ \{ \% のような記号 1 字のコマンド
                if !c.is_ascii_alphabetic() {
                    state.command = false;
                }
                Pass
            } else {
                state.command = false;
                Normal
            }
        }
        UserAction::Input(_) | UserAction::Number(_) | UserAction::Space | UserAction::Tab if in_math => {
            state.len += 1;
            Pass
        }
        UserAction::Backspace if in_math => {
            state.len = state.len.saturating_sub(1);
            if state.len == 0 {
                *state = State::default();
            }
            Pass
        }
        UserAction::Backspace if state.command => {
            state.command = false;
            Pass
        }
        // 行内の $ は行をまたがない
        UserAction::Enter if state.math == Math::Inline => {
            *state = State::default();
            Normal
        }
        UserAction::Enter if in_math => Pass,
        UserAction::Navigation(_) | UserAction::Home | UserAction::End if in_math => Pass,
        _ => {
            *state = State::default();
            Normal
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static LAST: RefCell<Option<(i32, usize, Decision)>> = const { RefCell::new(None) };
}

/// 同じキーの 2 回目 (OnTestKeyDown → OnKeyDown) は前の結果を返す
pub fn decide_once(wparam: usize, action: &UserAction) -> Decision {
    let time = unsafe { windows::Win32::UI::WindowsAndMessaging::GetMessageTime() };
    if let Some((t, w, d)) = LAST.with_borrow(|l| *l) {
        if t == time && w == wparam {
            return d;
        }
    }
    let decision = STATE.with_borrow_mut(|state| step(state, action));
    LAST.with_borrow_mut(|l| *l = Some((time, wparam, decision)));
    decision
}

/// 日本語を打っている途中で $ を打った: 日本語を確定して数式に入ったことにする
pub fn enter_inline_math() {
    STATE.with_borrow_mut(|state| *state = State { math: Math::Inline, len: 1, just_opened: true, ..State::default() });
}

pub fn reset() {
    STATE.with_borrow_mut(|state| *state = State::default());
}

pub fn in_math() -> bool {
    STATE.with_borrow(|state| state.math != Math::Off)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Decision::{Normal as N, Pass as P};
    use UserAction as K;

    fn run(keys: &[UserAction]) -> Vec<Decision> {
        let mut state = State::default();
        keys.iter().map(|k| step(&mut state, k)).collect()
    }

    #[test]
    fn inline_math() {
        let keys = [K::Input('$'), K::Input('x'), K::Input('^'), K::Number(2), K::Input('$'), K::Input('k')];
        assert_eq!(run(&keys), [P, P, P, P, P, N]);
    }

    #[test]
    fn display_math_with_spaces_and_newlines() {
        let keys = [K::Input('$'), K::Input('$'), K::Input('a'), K::Space, K::Enter, K::Input('b'), K::Input('$'), K::Input('$'), K::Input('k')];
        assert_eq!(run(&keys), [P, P, P, P, P, P, P, P, N]);
    }

    #[test]
    fn brackets() {
        let keys = [K::Input('\\'), K::Input('['), K::Input('x'), K::Input('\\'), K::Input(']'), K::Input('k')];
        assert_eq!(run(&keys), [P, P, P, P, P, N]);
    }

    #[test]
    fn command_then_braces() {
        let keys = [K::Input('\\'), K::Input('t'), K::Input('e'), K::Input('x'), K::Input('{'), K::Input('n')];
        assert_eq!(run(&keys), [P, P, P, P, N, N]);
    }

    #[test]
    fn backspace_out_of_math_and_enter_ends_inline() {
        assert_eq!(run(&[K::Input('$'), K::Input('x'), K::Backspace, K::Backspace, K::Input('k')]), [P, P, P, P, N]);
        assert_eq!(run(&[K::Input('$'), K::Input('x'), K::Enter, K::Input('k')]), [P, P, N, N]);
    }

    #[test]
    fn escaped_dollar_is_not_math() {
        // \$ は記号 1 字のコマンド (数式ではない)
        assert_eq!(run(&[K::Input('\\'), K::Input('$'), K::Input('k')]), [P, P, N]);
    }
}
