//! 2026-10-14: IME を通さずにアプリへ渡す欄
//!
//! 1. 入力欄の種類 (InputScope) が数字・電話・パスワード・PIN・メール・時刻・半角英数だけの欄。Mozc (Google 日本語入力) の
//!    tip_input_mode_manager.cc `GetOverriddenState` と同じ考え方で、ひらがな・カタカナ・全角の指定と混ざっていたら普通に入力する。
//!    URL も入れる (2026-10-14 19:30。Chrome のアドレスバーは IS_SEARCH[50]、type=url の欄が IS_URL[1] なのを実測)
//! 2. ブラウザの拡張機能の小窓 (持ち主のある、題名の無いツール窓) での数字。iCloud パスワードの認証コード欄は 1 字ずつの欄で、
//!    keydown の keyCode が数字のときだけ受け付けて次の欄へ移る (拡張機能 3.4.0 の page_popup.js)。IME が数字を受け取ると
//!    keyCode は 229 になり、必ず失敗する。小窓の題名は空で、iCloud の小窓だけを見分けることはできない
//!
//! 3. 2026-11-06 (仕事 25): アプリが「この文脈では IME を使わない」と伝えている文脈 (GUID_COMPARTMENT_KEYBOARD_DISABLED か
//!    GUID_COMPARTMENT_EMPTYCONTEXT が 0 でない)。Chromium はパスワード欄にこの文脈を使う (ui/base/ime/win/tsf_bridge.cc の
//!    InitializeDisabledContext が 2 つとも 1 にする。入力欄の種類は IS_PRIVATE)。Mozc も、この文脈では IME を閉じた扱いにする
//!    (src/win32/tip/tip_status.cc の IsDisabledContext、tip_keyevent_handler.cc)。半角/全角を押しても普通の入力にしない
//!
//! 4. 2026-11-23 (Astra の点検 37): パスワード・PIN の指定 (IS_PASSWORD・IS_NUMERIC_PASSWORD・各種 PIN) は、ほかの指定より先に見て、3 と同じ
//!    上書きできない欄 (Field::Disabled) にする。かな・全角の指定と混ざっていても、半角/全角を押しても、日本語入力に戻さない。
//!    この欄では学習・予測の履歴・前後の文の取得・診断ログも止める (private_field。README の「パスワード欄では日本語入力を使わない」)
//!
//! どちらも、入力中でない あ のときだけ。保存している入力モードは変えない。1 の欄で半角/全角を押したら、その欄でも普通に入力する。
//! 1・3 の欄にフォーカスがある間は、言語バーを A の絵にする (shown_latin。保存しているモードは あ のまま)
use std::cell::Cell;

use windows::core::{Interface, GUID};
use windows::Win32::{
    Foundation::HWND,
    UI::{
        TextServices::{
            ITfCompartmentMgr, ITfContext, GUID_COMPARTMENT_EMPTYCONTEXT, GUID_COMPARTMENT_KEYBOARD_DISABLED,
            IS_ALPHANUMERIC_FULLWIDTH, IS_ALPHANUMERIC_HALFWIDTH, IS_ALPHANUMERIC_PIN, IS_ALPHANUMERIC_PIN_SET, IS_DIGITS,
            IS_EMAIL_SMTPEMAILADDRESS, IS_EMAIL_USERNAME, IS_HIRAGANA, IS_KATAKANA_FULLWIDTH, IS_KATAKANA_HALFWIDTH, IS_NUMBER,
            IS_NUMBER_FULLWIDTH, IS_NUMERIC_PASSWORD, IS_NUMERIC_PIN, IS_PASSWORD, IS_PRIVATE, IS_TELEPHONE_AREACODE,
            IS_TELEPHONE_COUNTRYCODE, IS_TELEPHONE_FULLTELEPHONENUMBER, IS_TELEPHONE_LOCALNUMBER, IS_TIME_FULLTIME,
            IS_TIME_HOUR, IS_TIME_MINORSEC, IS_URL,
        },
        WindowsAndMessaging::{WS_EX_TOOLWINDOW, WS_POPUP},
    },
};

use super::user_action::UserAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// 普通の欄
    Normal,
    /// 文字・数字・空白をそのまま渡す (InputScope)
    Direct,
    /// 数字だけそのまま渡す (ブラウザの拡張機能の小窓)
    DirectDigits,
    /// アプリが IME を使わない文脈と伝えている (パスワード欄)。文字・数字・空白・半角/全角をそのまま渡す
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Normal,
    /// このキーは IME を通さずアプリへ
    Pass,
    /// 半角/全角: この欄でも普通に入力する (あ にする)
    EnableKana,
}

/// IME を切る指定 (Mozc と同じ＋Windows 8 以降の数字・PIN の指定)
const DIRECT_SCOPES: [i32; 18] = [
    IS_URL.0,
    IS_EMAIL_USERNAME.0,
    IS_EMAIL_SMTPEMAILADDRESS.0,
    IS_DIGITS.0,
    IS_NUMBER.0,
    IS_PASSWORD.0,
    IS_TELEPHONE_FULLTELEPHONENUMBER.0,
    IS_TELEPHONE_COUNTRYCODE.0,
    IS_TELEPHONE_AREACODE.0,
    IS_TELEPHONE_LOCALNUMBER.0,
    IS_TIME_FULLTIME.0,
    IS_TIME_HOUR.0,
    IS_TIME_MINORSEC.0,
    IS_ALPHANUMERIC_HALFWIDTH.0,
    // Windows 8 以降の数字・PIN の指定 (Mozc の表には無い)
    IS_NUMERIC_PASSWORD.0,
    IS_NUMERIC_PIN.0,
    IS_ALPHANUMERIC_PIN.0,
    IS_ALPHANUMERIC_PIN_SET.0,
];

/// 2026-11-23 (Astra の点検 37): パスワード・PIN の指定。かな・全角の指定と混ざっていても、半角/全角を押しても IME を通さない (Field::Disabled)
const SENSITIVE_SCOPES: [i32; 5] = [
    IS_PASSWORD.0,
    IS_NUMERIC_PASSWORD.0,
    IS_NUMERIC_PIN.0,
    IS_ALPHANUMERIC_PIN.0,
    IS_ALPHANUMERIC_PIN_SET.0,
];

/// パスワード・PIN の欄か (ほかの指定と混ざっていても)
pub fn scopes_are_sensitive(scopes: &[i32]) -> bool {
    scopes.iter().any(|scope| SENSITIVE_SCOPES.contains(scope))
}

/// かな・全角の入力を求める指定。これと混ざっていたら普通に入力する
const JAPANESE_SCOPES: [i32; 5] = [
    IS_HIRAGANA.0,
    IS_KATAKANA_FULLWIDTH.0,
    IS_KATAKANA_HALFWIDTH.0,
    IS_NUMBER_FULLWIDTH.0,
    IS_ALPHANUMERIC_FULLWIDTH.0,
];

/// 入力欄の種類から、IME を通さない欄か。知らない指定 (既定・検索・IS_PRIVATE など) は見ない
pub fn scopes_are_direct(scopes: &[i32]) -> bool {
    let mut direct = false;
    for scope in scopes {
        if JAPANESE_SCOPES.contains(scope) {
            return false;
        }
        direct |= DIRECT_SCOPES.contains(scope);
    }
    direct
}

/// Chromium 系のブラウザ (拡張機能の小窓を Chrome_WidgetWin_1 のツール窓で出す)
const BROWSERS: [&str; 8] = [
    "msedge.exe",
    "chrome.exe",
    "brave.exe",
    "vivaldi.exe",
    "opera.exe",
    "chromium.exe",
    "thorium.exe",
    "arc.exe",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowFacts {
    pub class: String,
    pub has_title: bool,
    pub style: u32,
    pub ex_style: u32,
    pub owned: bool,
}

/// ブラウザの拡張機能の小窓か。Chrome for Testing 155 で実測: 本体の窓は題名あり・持ち主なし・WS_POPUP なし、
/// 拡張機能の小窓は題名が空・持ち主あり・style 0x96000000 (WS_POPUP)・exstyle 0x00200080 (WS_EX_TOOLWINDOW)
pub fn is_browser_popup(exe: &str, window: &WindowFacts) -> bool {
    BROWSERS.contains(&exe.to_ascii_lowercase().as_str())
        && window.class == "Chrome_WidgetWin_1"
        && !window.has_title
        && window.owned
        && window.style & WS_POPUP.0 != 0
        && window.ex_style & WS_EX_TOOLWINDOW.0 != 0
}

/// 一番上の窓 (GetAncestor GA_ROOT) を調べる。題名はメッセージを送らずに読む (InternalGetWindowText)
pub fn window_facts(hwnd: HWND) -> Option<WindowFacts> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetClassNameW, GetWindow, GetWindowLongW, InternalGetWindowText, GA_ROOT, GWL_EXSTYLE, GWL_STYLE,
        GW_OWNER,
    };
    if hwnd.0.is_null() {
        return None;
    }
    unsafe {
        let root = GetAncestor(hwnd, GA_ROOT);
        if root.0.is_null() {
            return None;
        }
        let mut class = [0u16; 64];
        let length = GetClassNameW(root, &mut class).max(0) as usize;
        let mut title = [0u16; 4];
        let has_title = InternalGetWindowText(root, &mut title) > 0;
        Some(WindowFacts {
            class: String::from_utf16_lossy(&class[..length.min(class.len())]),
            has_title,
            style: GetWindowLongW(root, GWL_STYLE) as u32,
            ex_style: GetWindowLongW(root, GWL_EXSTYLE) as u32,
            owned: GetWindow(root, GW_OWNER).is_ok_and(|owner| !owner.0.is_null()),
        })
    }
}

/// どのキーを読むか: IME が受け取るはずのキー (文字・数字・空白) と、半角/全角だけ
pub fn relevant(action: &UserAction) -> bool {
    matches!(action, UserAction::Input(_) | UserAction::Number(_) | UserAction::Space | UserAction::ToggleInputMode)
}

/// 入力中でないときのキーをどう扱うか。enabled_here = この欄で半角/全角を押して普通の入力に戻してある
pub fn decide(action: &UserAction, kana: bool, shift: bool, field: Field, enabled_here: bool) -> Decision {
    match (field, action) {
        (Field::Normal, _) => Decision::Normal,
        (Field::Disabled, _) => Decision::Pass,
        // 小窓の数字の扱いは半角/全角で変えない (かなの入力はもともと普通にできる)
        (Field::Direct, UserAction::ToggleInputMode) if !enabled_here => Decision::EnableKana,
        _ if !kana || enabled_here => Decision::Normal,
        (Field::Direct, UserAction::Input(_) | UserAction::Number(_) | UserAction::Space) => Decision::Pass,
        // Shift+数字は記号なので、小窓でも今までどおり
        (Field::DirectDigits, UserAction::Number(_)) if !shift => Decision::Pass,
        _ => Decision::Normal,
    }
}

/// 2026-11-06 (仕事 25): 文脈の 2 つの印 (キーボード入力を切る・空の文脈) のどちらかが 0 でないか。
/// 値は VT_I4 で入る (Chromium は 1、Mozc は VT_I4 で 0 でないときだけ切れているとみなす)。読めなければ切れていない扱い
pub fn context_disabled(context: &ITfContext) -> bool {
    let set = |guid: &GUID| -> bool {
        let Ok(manager) = context.cast::<ITfCompartmentMgr>() else { return false };
        let Ok(compartment) = (unsafe { manager.GetCompartment(guid) }) else { return false };
        let Ok(value) = (unsafe { compartment.GetValue() }) else { return false };
        i32::try_from(&value).is_ok_and(|n| n != 0)
    };
    set(&GUID_COMPARTMENT_KEYBOARD_DISABLED) || set(&GUID_COMPARTMENT_EMPTYCONTEXT)
}

/// 言語バーに A を見せるか: そのまま渡す欄 (Direct・Disabled) で、半角/全角で普通の入力に戻していないとき
pub fn shows_latin(field: Field, enabled_here: bool) -> bool {
    matches!(field, Field::Disabled) || (field == Field::Direct && !enabled_here)
}

/// 言語バーの絵に使う印を替える。変わったら true (言語バーを描き直す)
pub fn set_shown_latin(shown: bool) -> bool {
    SHOWN_LATIN.replace(shown) != shown
}

/// 今の欄では、保存しているモードが あ でも言語バーを A の絵にするか
pub fn shown_latin() -> bool {
    SHOWN_LATIN.get()
}

thread_local! {
    /// 2026-11-06 (仕事 25): フォーカスのある欄では言語バーを A の絵にするか (set_shown_latin)
    static SHOWN_LATIN: Cell<bool> = const { Cell::new(false) };
    /// 今の欄が IS_PRIVATE (61) を含むか。入力中でないときの最初のキーで読んだ値
    static PRIVATE: Cell<bool> = const { Cell::new(false) };
    /// 半角/全角で普通の入力に戻した欄 (文脈のポインタ, 窓, 入力欄の種類)。種類が変わったら戻す (Mozc も同じ種類の間だけ前のモードを使う)
    static ENABLED: Cell<Option<FieldId>> = const { Cell::new(None) };
    /// 直前に判定したキー (メッセージの時刻, 仮想キー) とその結果。OnTestKeyDown と OnKeyDown で 2 回呼ばれるため
    static LAST: Cell<Option<(i32, usize, Decision)>> = const { Cell::new(None) };
}

/// 2026-10-14 19:30: アプリが「この欄の入力を覚えないで」と伝えている欄 (IS_PRIVATE)。Chrome はパスワード欄とシークレットの窓の欄に付ける
/// (パスワード欄に IS_PASSWORD は付かない。実測)。直接入力にはしない (シークレットの窓のふつうの欄にも付くため) が、学習はすべて止める
/// 2026-11-23 (Astra の点検 37): パスワード・PIN の欄も覚えない欄にする (学習・予測の履歴・前後の文の取得・診断ログを止める)
pub fn note_scopes(scopes: Option<&[i32]>) {
    PRIVATE.set(scopes.is_some_and(|s| s.contains(&IS_PRIVATE.0) || scopes_are_sensitive(s)));
}

/// 2026-10-20 (Fable の再調査 M1): 次の語の候補を出してはいけない欄か (IS_PRIVATE・パスワード・PIN・数字・メール・URL など)。
/// 英数モードでは direct_field の判定 (かなモードのときだけ) を通らないので、候補を出す前にこれで欄を確かめる
pub fn scopes_block_suggestions(scopes: &[i32]) -> bool {
    scopes.contains(&IS_PRIVATE.0) || scopes_are_sensitive(scopes) || scopes_are_direct(scopes)
}

/// 2026-11-23 (Astra の点検 37): 欄の種類。IME を使わない文脈 (disabled) とパスワード・PIN の欄は、ほかの指定より先に Disabled
/// (上書きできない)。数字・URL などは Direct (半角/全角で普通の入力に戻せる)。小窓は DirectDigits
pub fn field_of(disabled: bool, scopes: Option<&[i32]>, popup: bool) -> Field {
    if disabled || scopes.is_some_and(scopes_are_sensitive) {
        Field::Disabled
    } else if scopes.is_some_and(scopes_are_direct) {
        Field::Direct
    } else if popup {
        Field::DirectDigits
    } else {
        Field::Normal
    }
}

pub fn private_field() -> bool {
    PRIVATE.get()
}

pub type FieldId = (usize, isize, u64);

pub fn field_id(context: usize, hwnd: isize, scopes: Option<&[i32]>) -> FieldId {
    let kind = scopes.unwrap_or(&[]).iter().fold(scopes.map_or(1, |_| 0u64), |h, s| h.wrapping_mul(131).wrapping_add(*s as u32 as u64));
    (context, hwnd, kind)
}

pub fn enabled_here(field_id: FieldId) -> bool {
    ENABLED.get() == Some(field_id)
}

pub fn enable(field_id: FieldId) {
    ENABLED.set(Some(field_id));
}

/// 同じキーの 2 回目 (OnKeyDown) は、1 回目 (OnTestKeyDown) の結果を使う
pub fn decide_once(wparam: usize, decide: impl FnOnce() -> Decision) -> Decision {
    let time = unsafe { windows::Win32::UI::WindowsAndMessaging::GetMessageTime() };
    if let Some((t, w, decision)) = LAST.get() {
        if t == time && w == wparam {
            return decision;
        }
    }
    let decision = decide();
    LAST.set(Some((time, wparam, decision)));
    decision
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_follow_mozc() {
        // Chromium: type=tel・inputmode=numeric・password・email (シークレットなら IS_PRIVATE が足される)
        assert!(scopes_are_direct(&[IS_TELEPHONE_FULLTELEPHONENUMBER.0]));
        assert!(scopes_are_direct(&[IS_DIGITS.0, 61]));
        assert!(scopes_are_direct(&[IS_PASSWORD.0]));
        assert!(scopes_are_direct(&[IS_EMAIL_SMTPEMAILADDRESS.0]));
        assert!(scopes_are_direct(&[IS_NUMERIC_PIN.0]));
        // 既定・検索・指定なしは普通に入力する
        assert!(!scopes_are_direct(&[]));
        assert!(!scopes_are_direct(&[0]));
        assert!(!scopes_are_direct(&[50]));
        // URL の欄は直接 (Chrome のアドレスバーは検索 [50] なので日本語のまま)
        assert!(scopes_are_direct(&[IS_URL.0]));
        // Chrome のパスワード欄・シークレットの窓は IS_PRIVATE だけ。直接入力にはしない (学習だけ止める)
        assert!(!scopes_are_direct(&[IS_PRIVATE.0]));
        // かなの指定と混ざっていたら普通に入力する (Mozc は複数のモードなら元のまま)
        assert!(!scopes_are_direct(&[IS_DIGITS.0, IS_HIRAGANA.0]));
    }

    fn popup() -> WindowFacts {
        WindowFacts { class: "Chrome_WidgetWin_1".into(), has_title: false, style: 0x9600_0000, ex_style: 0x0020_0080, owned: true }
    }

    #[test]
    fn only_untitled_owned_tool_windows_of_browsers_are_popups() {
        assert!(is_browser_popup("msedge.exe", &popup()));
        assert!(is_browser_popup("Chrome.exe", &popup()));
        // 本体の窓 (Chrome for Testing で実測した値)
        let main = WindowFacts { class: "Chrome_WidgetWin_1".into(), has_title: true, style: 0x16CF_0000, ex_style: 0x0020_0100, owned: false };
        assert!(!is_browser_popup("chrome.exe", &main));
        assert!(!is_browser_popup("chrome.exe", &WindowFacts { has_title: true, ..popup() }));
        assert!(!is_browser_popup("chrome.exe", &WindowFacts { owned: false, ..popup() }));
        assert!(!is_browser_popup("chrome.exe", &WindowFacts { ex_style: 0x0020_0000, ..popup() }));
        // Electron のアプリ (同じ窓の種類) は対象外
        assert!(!is_browser_popup("slack.exe", &popup()));
        assert!(!is_browser_popup("code.exe", &popup()));
    }

    /// 2026-11-23 (Astra の点検 37): パスワード・PIN の欄は、かなの指定と混ざっていても、半角/全角を押しても日本語入力に戻さない。覚えない欄にもする
    #[test]
    fn password_and_pin_fields_cannot_be_switched_to_kana() {
        use Decision::*;
        use UserAction as K;
        // パスワードだけ + 半角/全角
        let password = field_of(false, Some(&[IS_PASSWORD.0]), false);
        assert_eq!(password, Field::Disabled);
        assert_eq!(decide(&K::ToggleInputMode, true, false, password, false), Pass);
        assert_eq!(decide(&K::ToggleInputMode, false, false, password, false), Pass);
        assert_eq!(decide(&K::Input('a'), true, false, password, false), Pass);
        // パスワード + かな の指定 (前は普通の欄になっていた)
        let mixed = field_of(false, Some(&[IS_PASSWORD.0, IS_HIRAGANA.0]), false);
        assert_eq!(mixed, Field::Disabled);
        assert_eq!(decide(&K::Input('a'), true, false, mixed, false), Pass);
        assert!(scopes_block_suggestions(&[IS_PASSWORD.0, IS_HIRAGANA.0]));
        // 数字の PIN (と、ほかの PIN・数字のパスワード)
        for pin in [IS_NUMERIC_PIN.0, IS_NUMERIC_PASSWORD.0, IS_ALPHANUMERIC_PIN.0, IS_ALPHANUMERIC_PIN_SET.0] {
            let field = field_of(false, Some(&[pin]), false);
            assert_eq!(field, Field::Disabled, "{pin}");
            assert_eq!(decide(&K::Number(1), true, false, field, false), Pass);
            assert_eq!(decide(&K::ToggleInputMode, true, false, field, false), Pass);
            assert_eq!(field_of(false, Some(&[pin, IS_ALPHANUMERIC_FULLWIDTH.0]), false), Field::Disabled, "{pin}");
        }
        // 覚えない欄 (学習・予測の履歴・前後の文・診断ログを止める)
        for scopes in [&[IS_PASSWORD.0][..], &[IS_NUMERIC_PIN.0], &[IS_PASSWORD.0, IS_HIRAGANA.0]] {
            note_scopes(Some(scopes));
            assert!(private_field(), "{scopes:?}");
        }
        note_scopes(None);
        // 数字・URL は今までどおり上書きできる Direct、かなの指定と混ざれば普通の欄
        assert_eq!(field_of(false, Some(&[IS_DIGITS.0]), false), Field::Direct);
        assert_eq!(field_of(false, Some(&[IS_URL.0]), false), Field::Direct);
        assert_eq!(field_of(false, Some(&[IS_DIGITS.0, IS_HIRAGANA.0]), false), Field::Normal);
        assert_eq!(field_of(false, Some(&[0]), true), Field::DirectDigits);
        assert_eq!(field_of(true, Some(&[0]), false), Field::Disabled);
        assert_eq!(field_of(false, None, false), Field::Normal);
    }

    #[test]
    fn private_scope_is_remembered_per_field() {
        note_scopes(Some(&[IS_PRIVATE.0]));
        assert!(private_field());
        note_scopes(Some(&[0]));
        assert!(!private_field());
        note_scopes(None);
        assert!(!private_field());
    }

    #[test]
    fn decisions() {
        use Decision::*;
        use UserAction as K;
        // 数字・電話・パスワードの欄: あ のとき文字・数字・空白はそのまま
        assert_eq!(decide(&K::Number(1), true, false, Field::Direct, false), Pass);
        assert_eq!(decide(&K::Input('a'), true, false, Field::Direct, false), Pass);
        assert_eq!(decide(&K::Space, true, false, Field::Direct, false), Pass);
        // A のときは今までどおり (もともと IME を通らない)
        assert_eq!(decide(&K::Input('a'), false, false, Field::Direct, false), Normal);
        // 半角/全角: その欄でも普通に入力する。戻したあとは今までどおり
        assert_eq!(decide(&K::ToggleInputMode, true, false, Field::Direct, false), EnableKana);
        assert_eq!(decide(&K::ToggleInputMode, false, false, Field::Direct, false), EnableKana);
        assert_eq!(decide(&K::ToggleInputMode, true, false, Field::Direct, true), Normal);
        assert_eq!(decide(&K::Number(1), true, false, Field::Direct, true), Normal);
        // 拡張機能の小窓: 数字だけ。文字・Shift+数字 (記号)・半角/全角は今までどおり
        assert_eq!(decide(&K::Number(1), true, false, Field::DirectDigits, false), Pass);
        assert_eq!(decide(&K::Number(1), true, true, Field::DirectDigits, false), Normal);
        assert_eq!(decide(&K::Input('a'), true, false, Field::DirectDigits, false), Normal);
        assert_eq!(decide(&K::Space, true, false, Field::DirectDigits, false), Normal);
        assert_eq!(decide(&K::ToggleInputMode, true, false, Field::DirectDigits, false), Normal);
        // 普通の欄
        assert_eq!(decide(&K::Number(1), true, false, Field::Normal, false), Normal);
        // 2026-11-06: アプリが IME を使わないと伝えている文脈 (Chromium のパスワード欄): あ でも A でも、半角/全角でも、そのまま渡す
        for key in [K::Number(1), K::Input('a'), K::Space, K::ToggleInputMode] {
            assert_eq!(decide(&key, true, false, Field::Disabled, false), Pass);
            assert_eq!(decide(&key, true, false, Field::Disabled, true), Pass);
        }
        assert_eq!(decide(&K::ToggleInputMode, false, false, Field::Disabled, false), Pass);
    }

    #[test]
    fn latin_is_shown_only_in_passed_fields() {
        assert!(shows_latin(Field::Disabled, false));
        assert!(shows_latin(Field::Direct, false));
        // 半角/全角で普通の入力に戻した欄・普通の欄・小窓 (かなの入力はふつうにできる) は あ のまま
        assert!(!shows_latin(Field::Direct, true));
        assert!(!shows_latin(Field::Normal, false));
        assert!(!shows_latin(Field::DirectDigits, false));
        assert!(set_shown_latin(true));
        assert!(!set_shown_latin(true));
        assert!(shown_latin());
        assert!(set_shown_latin(false));
        assert!(!shown_latin());
    }
}
