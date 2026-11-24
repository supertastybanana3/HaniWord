// 端末でシェルの入力待ち (PowerShell・コマンド プロンプト・bash のプロンプト) のときは、英数 (A) に切り替える。
// wsl・claude --resume のようなコマンドを日本語で打つことはまず無いため。
//
// Windows Terminal はどのタブも同じプロセスなので、ウィンドウのタイトル (= いま見ているタブのタイトル) で見分ける。
// Claude Code などのプログラムを動かしている間はタイトルが変わるので、日本語入力はいつもどおり。
// 2026-10-13: シェルの入力待ちでは、まず A にする。半角/全角でいつもどおり あ に切り替えられるように:
// 前は「あ」のまま裏で英字を通していた。いまは本当に A に切り替え (表示も A)、半角/全角 でいつもどおり あ に戻せる
// (戻したらそのタイトルの間はもう切り替えない)。入力待ちでなくなったら (タイトルが変わったら) 自動で あ に戻す。

use std::cell::RefCell;
use std::sync::OnceLock;

use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

use super::prefix_direct::host_is_terminal;

thread_local! {
    /// 半角/全角 で解除したときのタイトル。2026-11-22: 解除は、そのタイトルのまま打っている間だけ。違うタイトルで打ったら忘れる (forget_release_unless)
    static RELEASED: RefCell<Option<String>> = const { RefCell::new(None) };
    /// 自動で A にしたときのタイトル (入力待ちでなくなったら あ に戻すため)
    static AUTO: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| shared::AppConfig::read_or_default().input.latin_at_shell_prompt)
}

pub fn foreground_title() -> String {
    unsafe {
        let window = GetForegroundWindow();
        let mut buffer = [0u16; 512];
        let length = GetWindowTextW(window, &mut buffer).max(0) as usize;
        String::from_utf16_lossy(&buffer[..length])
    }
}

/// シェルが入力を待っているときのタイトルか。
/// コマンド プロンプトはコマンドを実行中「コマンド プロンプト - claude」になるので、完全一致だけを見る
pub fn is_prompt_title(title: &str) -> bool {
    let title = title.trim();
    let title = ["管理者: ", "Administrator: "]
        .iter()
        .find_map(|p| title.strip_prefix(p))
        .unwrap_or(title);
    const EXACT: &[&str] = &[
        "Windows PowerShell",
        "PowerShell",
        "コマンド プロンプト",
        "Command Prompt",
        "Developer PowerShell for VS 2022",
        "Developer PowerShell for VS 18",
        "Developer Command Prompt for VS 2022",
        "Developer Command Prompt for VS 18",
    ];
    if EXACT.contains(&title) {
        return true;
    }
    // 2026-11-22: 名前の後ろに版の番号 (空白＋数字と . の並び) が付いたタイトルも入力待ち (Windows Terminal の既定の端末の受け渡しで
    // 開いた窓が「Windows PowerShell 5.1」、PowerShell 7 の「PowerShell 7.5.4」)。後ろにコマンドが続く形 (PowerShell 7.5.4 - claude) は当たらない
    const VERSIONED: &[&str] = &[
        "Windows PowerShell",
        "PowerShell",
        "Developer PowerShell for VS 2022",
        "Developer PowerShell for VS 18",
    ];
    if VERSIONED.iter().any(|name| title.strip_prefix(name).and_then(|rest| rest.strip_prefix(' ')).is_some_and(is_version_number)) {
        return true;
    }
    // シェルの exe のパスがそのままタイトルになっていることがある
    let lower = title.to_ascii_lowercase();
    if ["\\cmd.exe", "\\powershell.exe", "\\pwsh.exe"].iter().any(|exe| lower.ends_with(exe)) {
        return true;
    }
    // bash のプロンプト (Ubuntu の既定 PS1 は「ユーザー@ホスト: ディレクトリ」をタイトルにする)
    if let Some((user_host, _dir)) = title.split_once(": ") {
        if let Some((user, host)) = user_host.split_once('@') {
            let name_like = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
            return name_like(user) && name_like(host);
        }
    }
    false
}

/// 2026-11-22: 解除したタイトルと違うタイトルで打ったら、解除を忘れる。前は一度解除すると忘れず、Windows Terminal の
/// PowerShell のタブはどれも同じタイトル (Windows PowerShell) なので、コマンドを動かして入力待ちに戻っても・ほかの PowerShell のタブでも、
/// その IME のスレッドが続く間はもう A に切り替わらなかった。違うタイトルが見えるのは、そのタイトルのときに文字のキーを打ったときだけ
/// (すぐ終わるコマンドの間に打たなければ、解除は残る)。同じタイトルの別のタブは見分けられない
fn forget_release_unless(title: &str) {
    RELEASED.with_borrow_mut(|released| {
        if released.as_deref().is_some_and(|r| r != title) {
            crate::debug_log::write(&format!("shell prompt: release forgotten ({:?} -> {title:?})", released.as_deref().unwrap_or_default()));
            *released = None;
        }
    });
}

/// 版の番号 (5.1・7.5.4・7)。数字と . の並びで、. の前後に数字がある
fn is_version_number(text: &str) -> bool {
    !text.is_empty() && text.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// いま英字を強制するか
pub fn forced_latin() -> bool {
    if !enabled() || !host_is_terminal() {
        return false;
    }
    forced_latin_at(&foreground_title())
}

/// 前景のタイトルが title のとき、英字を強制するか (解除したタイトルと違えば、解除を忘れてから見る)
fn forced_latin_at(title: &str) -> bool {
    forget_release_unless(title);
    if !is_prompt_title(title) {
        return false;
    }
    !RELEASED.with_borrow(|released| released.as_deref() == Some(title))
}

/// 強制英字の最中に 半角/全角 を押した: このタイトルの間は日本語を打てるようにする
pub fn release() {
    let title = foreground_title();
    crate::debug_log::write(&format!("shell prompt: released for {title:?}"));
    RELEASED.with_borrow_mut(|released| *released = Some(title));
}

/// 文字を打つ前に呼ぶ。切り替えるモードを返す: シェルの入力待ちになった (かなのとき) → Some(true) = A にする、
/// 自動で A にしたあと入力待ちでなくなった → Some(false) = あ に戻す
pub fn auto_switch(kana: bool) -> Option<bool> {
    if !enabled() || !host_is_terminal() {
        return None;
    }
    auto_switch_at(kana, &foreground_title())
}

/// 前景のタイトルが title のときの auto_switch
fn auto_switch_at(kana: bool, title: &str) -> Option<bool> {
    if kana {
        if !forced_latin_at(title) {
            return None;
        }
        crate::debug_log::write(&format!("shell prompt: switch to latin for {title:?}"));
        AUTO.with_borrow_mut(|auto| *auto = Some(title.to_string()));
        Some(true)
    } else {
        forget_release_unless(title);
        if AUTO.with_borrow(|auto| auto.is_none()) || is_prompt_title(title) {
            return None;
        }
        AUTO.with_borrow_mut(|auto| *auto = None);
        Some(false)
    }
}

/// 利用者が 半角/全角 でモードを切り替えた。自動で A にしていたなら、そのタイトルの間はもう切り替えない
pub fn note_manual_toggle() {
    if let Some(title) = AUTO.with_borrow_mut(|auto| auto.take()) {
        crate::debug_log::write(&format!("shell prompt: manual toggle, released for {title:?}"));
        RELEASED.with_borrow_mut(|released| *released = Some(title));
    }
}

#[cfg(test)]
mod tests {
    use super::{auto_switch_at, is_prompt_title, note_manual_toggle, AUTO, RELEASED};

    fn reset() {
        RELEASED.with_borrow_mut(|released| *released = None);
        AUTO.with_borrow_mut(|auto| *auto = None);
    }

    const PS: &str = "Windows PowerShell";

    /// 2026-11-22: 解除 (半角/全角) は、そのタイトルのまま打っている間だけ。別のタイトルで打ってから入力待ちに戻ったら、また A にする
    #[test]
    fn release_lasts_only_while_the_title_stays() {
        reset();
        // 入力待ちで かな を打つ → A に
        assert_eq!(auto_switch_at(true, PS), Some(true));
        // 半角/全角 で あ に戻す → 解除
        note_manual_toggle();
        // 同じタブで解除したまま打つ → 切り替えない (何回でも)
        assert_eq!(auto_switch_at(true, PS), None);
        assert_eq!(auto_switch_at(true, PS), None);
        // コマンドを動かしている間 (別のタイトル) に打つ → そのまま あ
        assert_eq!(auto_switch_at(true, "✳ Claude Code"), None);
        // 入力待ちに戻って打つ → また A に
        assert_eq!(auto_switch_at(true, PS), Some(true));
    }

    /// 別のタイトルで打ったのが A のとき (英字を打っていた) も、解除を忘れる
    #[test]
    fn release_is_forgotten_after_typing_latin_elsewhere() {
        reset();
        assert_eq!(auto_switch_at(true, PS), Some(true));
        note_manual_toggle();
        assert_eq!(auto_switch_at(true, PS), None);
        // 別のアプリの題 (A のまま英字を打った)
        assert_eq!(auto_switch_at(false, "vim README.md"), None);
        assert_eq!(auto_switch_at(true, PS), Some(true));
    }

    /// 自動で A にしたあと、入力待ちでなくなったら あ に戻す (前と同じ)
    #[test]
    fn auto_latin_returns_to_kana_after_the_prompt() {
        reset();
        assert_eq!(auto_switch_at(true, PS), Some(true));
        assert_eq!(auto_switch_at(false, PS), None);
        assert_eq!(auto_switch_at(false, "✳ Claude Code"), Some(false));
        assert_eq!(auto_switch_at(false, "✳ Claude Code"), None);
    }

    #[test]
    fn prompt_titles() {
        assert!(is_prompt_title("Windows PowerShell"));
        assert!(is_prompt_title("管理者: Windows PowerShell"));
        assert!(is_prompt_title("コマンド プロンプト"));
        assert!(is_prompt_title("C:\\WINDOWS\\system32\\cmd.exe"));
        assert!(is_prompt_title("user@laptop: ~/project"));
        assert!(!is_prompt_title("コマンド プロンプト - claude --resume"));
        assert!(!is_prompt_title("mathview"));
        assert!(!is_prompt_title("✳ 仕組み: 入力方式の調査"));
        assert!(!is_prompt_title("メモ: 今日の予定"));
    }

    /// 2026-11-22: 版の番号の付いたタイトル (既定の端末の受け渡しで開いた Windows Terminal の窓・PowerShell 7)
    #[test]
    fn prompt_titles_with_versions() {
        assert!(is_prompt_title("Windows PowerShell 5.1"));
        assert!(is_prompt_title("管理者: Windows PowerShell 5.1"));
        assert!(is_prompt_title("Administrator: Windows PowerShell 5.1"));
        assert!(is_prompt_title("PowerShell 7.5.4"));
        assert!(is_prompt_title("管理者: PowerShell 7.5.4"));
        assert!(is_prompt_title("PowerShell 7"));
        assert!(is_prompt_title("Developer PowerShell for VS 2022 17.14"));
        // 後ろにコマンドが続く形・版でない字が続く形は当たらない
        assert!(!is_prompt_title("PowerShell 7.5.4 - claude"));
        assert!(!is_prompt_title("Windows PowerShell 5.1 - vim"));
        assert!(!is_prompt_title("PowerShell 7."));
        assert!(!is_prompt_title("PowerShell .5"));
        assert!(!is_prompt_title("PowerShell 7.5.4a"));
        assert!(!is_prompt_title("PowerShell ISE"));
        assert!(!is_prompt_title("Windows PowerShell5.1"));
        assert!(!is_prompt_title("PowerShell 7.5.4 (x64)"));
        assert!(!is_prompt_title("✳ PowerShell 7.5.4"));
    }
}
