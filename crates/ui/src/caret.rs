//! 2026-10-13: Caps Lock の表示を出しっぱなしにするあいだ、テキストカーソルの位置を探す。
//! CaretCue (https://github.com/GoldAnd07/CaretCue, Apache License 2.0) の CaretLocator を移植した:
//! UI オートメーションの選択範囲を始点に縮めた矩形 → Win32 のキャレット (GetGUIThreadInfo) の順。
//! 変更点: マウス位置への切り替えはしない (見つからなければ None。呼ぶ側が IME から届いた位置を使う)
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, SAFEARRAY};
use windows::Win32::System::Ole::{SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound, SafeArrayUnaccessData};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
    UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO};

/// (top, left, bottom, right)。物理ピクセル
pub type CaretRect = (i32, i32, i32, i32);

pub struct CaretLocator {
    automation: Option<IUIAutomation>,
}

impl CaretLocator {
    /// 呼んだスレッドで COM (MTA) を始める。UI オートメーションが使えなくても Win32 のキャレットは探す
    pub fn new() -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let automation = unsafe { CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.ok();
        Self { automation }
    }

    pub fn locate(&self) -> Option<CaretRect> {
        self.from_automation().or_else(win32_caret)
    }

    fn from_automation(&self) -> Option<CaretRect> {
        let automation = self.automation.as_ref()?;
        unsafe {
            let focused = automation.GetFocusedElement().ok()?;
            let pattern: IUIAutomationTextPattern = focused.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
            let selection = pattern.GetSelection().ok()?;
            if selection.Length().ok()? <= 0 {
                return None;
            }
            let range = selection.GetElement(0).ok()?.Clone().ok()?;
            range.MoveEndpointByRange(TextPatternRangeEndpoint_End, &range, TextPatternRangeEndpoint_Start).ok()?;
            let rects = range.GetBoundingRectangles().ok()?;
            first_useful_rect(rects)
        }
    }
}

/// GetBoundingRectangles の [left, top, width, height, ...] から、高さのある最初の矩形
unsafe fn first_useful_rect(array: *mut SAFEARRAY) -> Option<CaretRect> {
    if array.is_null() {
        return None;
    }
    let mut found = None;
    if let (Ok(lower), Ok(upper)) = (SafeArrayGetLBound(array, 1), SafeArrayGetUBound(array, 1)) {
        let count = (upper - lower + 1).max(0) as usize;
        let mut data: *mut std::ffi::c_void = std::ptr::null_mut();
        if count >= 4 && SafeArrayAccessData(array, &mut data).is_ok() && !data.is_null() {
            let values = std::slice::from_raw_parts(data as *const f64, count);
            for rect in values.chunks_exact(4) {
                let (x, y, w, h) = (rect[0], rect[1], rect[2], rect[3]);
                if x.is_finite() && y.is_finite() && h.is_finite() && h > 1.0 {
                    found = Some((y as i32, x as i32, (y + h) as i32, (x + w.max(1.0)) as i32));
                    break;
                }
            }
            let _ = SafeArrayUnaccessData(array);
        }
    }
    let _ = SafeArrayDestroy(array);
    found
}

fn win32_caret() -> Option<CaretRect> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            return None;
        }
        let thread = GetWindowThreadProcessId(foreground, None);
        let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(thread, &mut info).ok()?;
        if info.hwndCaret.0.is_null() {
            return None;
        }
        let mut top_left = POINT { x: info.rcCaret.left, y: info.rcCaret.top };
        let mut bottom_right = POINT { x: info.rcCaret.right, y: info.rcCaret.bottom };
        if !ClientToScreen(info.hwndCaret, &mut top_left).as_bool() || !ClientToScreen(info.hwndCaret, &mut bottom_right).as_bool() {
            return None;
        }
        Some((top_left.y, top_left.x, bottom_right.y.max(top_left.y + 2), bottom_right.x.max(top_left.x + 1)))
    }
}
