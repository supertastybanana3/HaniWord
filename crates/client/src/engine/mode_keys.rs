//! 2026-11-08 (仕事 53): 入力モード (あ／A) を切り替えるキー。半角/全角 の無い配列 (US 配列) の人向けに足す。
//! - Alt+` (1 の左のキー、走査コード 0x29): Alt を押したキーは key event sink では受けないので、TSF の保ったキー (PreserveKey) で受ける。
//!   今の配列で VK_OEM_3 がその位置にあるときだけ切り替える。JIS 配列では VK_OEM_3 は @ のキー (走査コード 0x1A) なので、Alt+@ は食べずにアプリへ
//!   (JIS の 0x29 は 半角/全角 で、もともと切り替え)。配列を選ばせなくても、JIS・US の両方で同じ設定のまま動く
//! - VK_KANJI (0x19): Windows が Alt+` や Alt+半角/全角 をこのキーにして届けることがある。修飾キーによらず切り替え (これも保ったキー)
//! - VK_IME_ON (0x16) → あ、VK_IME_OFF (0x1A) → A: キーを割り当て直す道具で、左右の Alt などに割り当てる人向け。すでにそのモードなら食べるだけ
//! - Ctrl+Space: 設定 input.ctrl_space_toggle がオンのときだけ (初期値オフ。VS Code の補完などとぶつかるため)。オフなら今までどおり
//!   (入力中は確定して空白、入力中でなければアプリへ)
//! どれも process_key で 半角/全角 (0xF3) に読み替えるので、打ちかけがあるときの扱いは 半角/全角 と同じ
use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use super::input_mode::InputMode;

pub const VK_IME_ON: usize = 0x16;
pub const VK_KANJI: usize = 0x19;
pub const VK_IME_OFF: usize = 0x1A;
pub const VK_OEM_3: usize = 0xC0;
/// 半角/全角 キーの仮想キー (切り替えのキーはみなこれに読み替える)
pub const VK_HALF_FULL: usize = 0xF3;
/// 1 の左のキーの走査コード (US 配列の `、JIS 配列の 半角/全角)
pub const SCAN_LEFT_OF_1: u32 = 0x29;

/// 押されたキーが入力モードのキーなら、何をするか
#[derive(Debug, Clone, PartialEq)]
pub enum ModeKey {
    /// 半角/全角 と同じ切り替え
    Toggle,
    /// このモードにする (今と同じなら食べるだけ)
    To(InputMode),
    /// 保ったキーで切り替えた同じ打鍵が、キーの受け口にも届いた (食べるだけ)
    AlreadyHandled,
}

thread_local! {
    /// 保ったキーから、半角/全角 と同じ切り替えを呼んでいる間だけ立てる
    static FORCED_TOGGLE: Cell<bool> = const { Cell::new(false) };
    /// 保ったキーで切り替えた時刻 (ミリ秒)。同じ打鍵の VK_KANJI がキーの受け口にも届いたときに、2 回切り替えないように
    static PRESERVED_AT: Cell<Option<u64>> = const { Cell::new(None) };
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// 保ったキー (Alt+`・VK_KANJI) で、半角/全角 と同じ切り替えを呼ぶ
pub fn with_forced_toggle<T>(f: impl FnOnce() -> T) -> T {
    FORCED_TOGGLE.with(|c| c.set(true));
    PRESERVED_AT.with(|c| c.set(Some(now_ms())));
    let result = f();
    FORCED_TOGGLE.with(|c| c.set(false));
    result
}

/// 押されたキーが入力モードのキーか (process_key の頭で、Alt・Ctrl を見る前に決める)
pub fn mode_key(vk: usize, ctrl: bool, shift: bool) -> Option<ModeKey> {
    if FORCED_TOGGLE.with(Cell::get) {
        return Some(ModeKey::Toggle);
    }
    let recently_preserved = PRESERVED_AT.with(Cell::get).is_some_and(|at| now_ms().saturating_sub(at) < 300);
    decide(vk, ctrl, shift, ctrl_space_toggle_enabled(), recently_preserved)
}

fn decide(vk: usize, ctrl: bool, shift: bool, ctrl_space: bool, recently_preserved: bool) -> Option<ModeKey> {
    match vk {
        VK_KANJI if recently_preserved => Some(ModeKey::AlreadyHandled),
        VK_KANJI => Some(ModeKey::Toggle),
        VK_IME_ON => Some(ModeKey::To(InputMode::Kana)),
        VK_IME_OFF => Some(ModeKey::To(InputMode::Latin)),
        0x20 if ctrl && !shift && ctrl_space => Some(ModeKey::Toggle),
        _ => None,
    }
}

/// Alt+` の保ったキーで切り替えるか: 今の配列で VK_OEM_3 の走査コードが 1 の左の位置 (0x29) のとき。JIS 配列では @ のキー (0x1A) なので切り替えない
pub fn backquote_toggles(scan_of_oem3: u32) -> bool {
    scan_of_oem3 == SCAN_LEFT_OF_1
}

pub fn backquote_toggles_now() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC};
    backquote_toggles(unsafe { MapVirtualKeyW(VK_OEM_3 as u32, MAPVK_VK_TO_VSC) })
}

/// 設定 input.ctrl_space_toggle (初期値オフ)。試験のビルドではオフ (今の Ctrl+Space の試験のまま)
pub fn ctrl_space_toggle_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| !cfg!(test) && shared::AppConfig::read_or_default().input.ctrl_space_toggle)
}

/// 2026-11-08 (仕事 53 の案 1、見るだけ): 最後にフォーカスが移った時刻 (ミリ秒)
static LAST_FOCUS_MS: AtomicU64 = AtomicU64::new(0);

pub fn note_focus() {
    LAST_FOCUS_MS.store(now_ms(), Ordering::Relaxed);
}

/// IME の開け閉め (GUID_COMPARTMENT_KEYBOARD_OPENCLOSE) が外から替わったときの印。HaniWord は今この区画を書かないので、替わったのはみな外から。
/// debug_log (debug_log_path.txt があるときだけ書く) に、開いた/閉じた と、直前にフォーカスが移ってからの時間だけを書く (字は残さない)
pub fn note_open_close(open: Option<bool>) {
    let last = LAST_FOCUS_MS.load(Ordering::Relaxed);
    crate::debug_log::write(&open_close_line(open, (last != 0).then(|| now_ms().saturating_sub(last))));
}

fn open_close_line(open: Option<bool>, since_focus_ms: Option<u64>) -> String {
    let state = match open { Some(true) => "open", Some(false) => "closed", None => "unknown" };
    let focus = match since_focus_ms { Some(ms) => format!("{ms}ms after a focus change"), None => "no focus change yet".to_string() };
    format!("openclose changed from outside: {state} ({focus})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_keys_toggle_or_set_the_mode() {
        assert_eq!(decide(VK_KANJI, false, false, false, false), Some(ModeKey::Toggle));
        // Alt+半角/全角 などで届いた VK_KANJI は、修飾キーによらず切り替え
        assert_eq!(decide(VK_KANJI, true, true, false, false), Some(ModeKey::Toggle));
        assert_eq!(decide(VK_IME_ON, false, false, false, false), Some(ModeKey::To(InputMode::Kana)));
        assert_eq!(decide(VK_IME_OFF, false, false, false, false), Some(ModeKey::To(InputMode::Latin)));
        // 保ったキーで切り替えた同じ打鍵が受け口にも届いたら、食べるだけ
        assert_eq!(decide(VK_KANJI, false, false, false, true), Some(ModeKey::AlreadyHandled));
        // ほかのキーは関わらない (JIS の @ = VK_OEM_3 も)
        assert_eq!(decide(VK_OEM_3, false, false, false, false), None);
        assert_eq!(decide(0x41, true, false, true, false), None);
    }

    #[test]
    fn ctrl_space_toggles_only_when_the_setting_is_on() {
        assert_eq!(decide(0x20, true, false, true, false), Some(ModeKey::Toggle));
        assert_eq!(decide(0x20, true, false, false, false), None);
        // Ctrl+Shift+Space・Ctrl なしの Space は切り替えない
        assert_eq!(decide(0x20, true, true, true, false), None);
        assert_eq!(decide(0x20, false, false, true, false), None);
    }

    #[test]
    fn backquote_toggles_only_at_the_key_left_of_1() {
        // US 配列: VK_OEM_3 (`) は 1 の左 (0x29) → 切り替え
        assert!(backquote_toggles(0x29));
        // JIS 配列: VK_OEM_3 は @ のキー (0x1A) → Alt+@ は食べない
        assert!(!backquote_toggles(0x1A));
        assert!(!backquote_toggles(0));
    }

    #[test]
    fn open_close_lines_carry_no_text() {
        assert_eq!(open_close_line(Some(false), Some(12)), "openclose changed from outside: closed (12ms after a focus change)");
        assert_eq!(open_close_line(Some(true), None), "openclose changed from outside: open (no focus change yet)");
    }
}
