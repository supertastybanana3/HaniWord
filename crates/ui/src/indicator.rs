use anyhow::{Context as _, Result};
use tao::{
    dpi::{LogicalSize, PhysicalPosition},
    event_loop::{EventLoop, EventLoopProxy},
    platform::windows::{WindowBuilderExtWindows, WindowExtWindows},
    window::{Window, WindowBuilder},
};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        SetWindowLongW, GWL_EXSTYLE, GWL_STYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP,
    },
};
use wry::{WebView, WebViewBuilder};

use crate::UserEvent;

pub fn create_indicator_window(event_loop: &EventLoop<UserEvent>) -> Result<Window> {
    let window = WindowBuilder::new()
        .with_decorations(false)
        .with_title("Indicator")
        .with_focused(false)
        // .with_visible(false)
        .with_undecorated_shadow(false)
        .with_transparent(true)
        .build(&event_loop)
        .context("Failed to create window")?;

    // 2026-10-13: CaretCue と同じ札 (幅 70・高さ 28) に、上 8px と影のぶんの余白をつけた大きさ (DPI に合わせて広がる)
    window.set_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));

    let hwnd = window.hwnd() as *mut std::ffi::c_void;

    // set extended window style
    // https://docs.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles
    // https://docs.microsoft.com/en-us/windows/win32/winmsg/window-styles
    unsafe {
        let exnewstyle = WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOPMOST.0;
        SetWindowLongW(HWND(hwnd), GWL_EXSTYLE, exnewstyle as i32);

        let style = WS_POPUP.0;
        SetWindowLongW(HWND(hwnd), GWL_STYLE, style as i32);
    };

    Ok(window)
}

/// 表示ウィンドウの大きさ (論理ピクセル) と、札の上端までの余白・カーソルとの間 (CaretCue の CaretGap)
pub const WINDOW_WIDTH: f64 = 100.0;
pub const WINDOW_HEIGHT: f64 = 52.0;
const PILL_TOP: f64 = 8.0;
const PILL_HEIGHT: f64 = 28.0;
const CARET_GAP: f64 = 3.0;

/// 表示ウィンドウの左上 (物理ピクセル)。CaretCue の Position と同じく、カーソルの真下に中央をそろえ、
/// 下に入りきらなければカーソルの上に出す。画面の作業領域からははみ出さない
pub fn indicator_origin(caret: (i32, i32, i32, i32), scale: f64) -> PhysicalPosition<f64> {
    let (top, left, bottom, right) = caret;
    // 幅のある矩形 (変換中の文字列) なら右端 (打っている位置)、細いならその中央
    let center = if right - left > 8 { right as f64 } else { (left + right) as f64 / 2.0 };
    let (w, h) = (WINDOW_WIDTH * scale, WINDOW_HEIGHT * scale);
    let work = work_area(center as i32, bottom);
    let mut x = center - w / 2.0;
    let mut y = bottom as f64 + (CARET_GAP - PILL_TOP) * scale;
    if let Some((wl, wt, wr, wb)) = work {
        x = x.clamp(wl as f64, (wr as f64 - w).max(wl as f64));
        if y + (PILL_TOP + PILL_HEIGHT) * scale > wb as f64 {
            y = (top as f64 - CARET_GAP * scale - (PILL_TOP + PILL_HEIGHT) * scale).max(wt as f64 - PILL_TOP * scale);
        }
    }
    let _ = h;
    PhysicalPosition::new(x.round(), y.round())
}

/// 2026-11-06 (仕事 35): 入力モードの表示の形 (設定 input.mode_indicator)。
/// 札 (CaretCue に合わせた今の表示)・小さく (小さな吹き出し、crates/ui/THIRD_PARTY_Mozc_indicator.md)・出さない (Caps Lock の表示も出さない)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style { Badge, Compact, Off }

impl Style {
    pub fn from_setting(value: &str) -> Self {
        match value { "compact" => Style::Compact, "off" => Style::Off, _ => Style::Badge }
    }

    /// 設定ファイル (settings.json) から読む。読めない・知らない値なら札
    pub fn current() -> Self {
        Self::from_setting(shared::AppConfig::read_or_default().input.mode_indicator_style())
    }

    pub fn window_size(self) -> LogicalSize<f64> {
        match self {
            Style::Compact => LogicalSize::new(COMPACT_WIDTH, COMPACT_HEIGHT),
            _ => LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    pub fn origin(self, caret: (i32, i32, i32, i32), scale: f64) -> PhysicalPosition<f64> {
        match self {
            Style::Compact => compact_origin(caret, scale),
            _ => indicator_origin(caret, scale),
        }
    }

    /// 表示側に形とフォントを伝える命令 (出す命令の前に付ける)
    pub fn script(self, font: &str) -> String {
        let name = if self == Style::Compact { "compact" } else { "badge" };
        format!("setStyle({},{});", serde_json::to_string(name).unwrap(), serde_json::to_string(font).unwrap())
    }
}

/// 「小さく」の窓の大きさ (論理ピクセル)。吹き出し 45×45 と上の尖り 5 に、まわりのぼかし (σ 3 の 3 倍 = 9) を足す。
/// 窓の中の吹き出しの位置は indicator.html の #compact と同じ (左上 (9, 14)、尖りの先 (31.5, 9))
pub const COMPACT_WIDTH: f64 = 63.0;
pub const COMPACT_HEIGHT: f64 = 68.0;
const COMPACT_TAIL_X: f64 = 31.5;
const COMPACT_TAIL_Y: f64 = 9.0;

/// 「小さく」の窓の左上 (物理ピクセル)。尖りの先を、カーソルの位置 (幅のある矩形なら右端 = 打っている位置) の画素の中央と、
/// 行の下端に合わせる。画面の端に近くても動かさない (THIRD_PARTY_Mozc_indicator.md の元の表示と同じ)
pub fn compact_origin(caret: (i32, i32, i32, i32), scale: f64) -> PhysicalPosition<f64> {
    let (_top, left, bottom, right) = caret;
    let x = if right - left > 8 { right } else { left } as f64 + 0.5;
    PhysicalPosition::new((x - COMPACT_TAIL_X * scale).round(), (bottom as f64 - COMPACT_TAIL_Y * scale).round())
}

/// 「小さく」の字のフォント: Windows のメッセージ ボックスのフォント (日本語の Windows では Yu Gothic UI)。取れなければ Yu Gothic UI
pub fn message_font() -> String {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut metrics = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, metrics.cbSize, Some(&mut metrics as *mut _ as *mut std::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    }.is_ok();
    let face = &metrics.lfMessageFont.lfFaceName;
    let name = String::from_utf16_lossy(&face[..face.iter().position(|&c| c == 0).unwrap_or(face.len())]);
    if ok && !name.trim().is_empty() { name } else { "Yu Gothic UI".into() }
}

fn work_area(x: i32, y: i32) -> Option<(i32, i32, i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        GetMonitorInfoW(monitor, &mut info).as_bool()
            .then_some((info.rcWork.left, info.rcWork.top, info.rcWork.right, info.rcWork.bottom))
    }
}

/// 札を塗る色。CaretCue の WindowsAccentColorProvider と同じく DwmGetColorizationColor の RGB (取れなければ #0078d4)
pub fn accent_color() -> String {
    let mut argb = 0u32;
    let mut opaque = windows::Win32::Foundation::BOOL(0);
    let ok = unsafe { windows::Win32::Graphics::Dwm::DwmGetColorizationColor(&mut argb, &mut opaque) }.is_ok();
    if !ok { return "#0078d4".into(); }
    format!("#{:02x}{:02x}{:02x}", (argb >> 16) & 0xff, (argb >> 8) & 0xff, argb & 0xff)
}

/// 中身を出す命令。seq は窓を隠すときの照合用 (古い「消えた」の知らせで新しい表示を隠さない)
/// 2026-11-08 (仕事 52): ai_paused なら札の右下に小さな点 (重いので AI を休んでいる間)
pub fn mode_script(mode: &str, accent: &str, seq: u64, ai_paused: bool) -> String {
    format!("setAccent({});setAIPaused({ai_paused});updateInputMethod({},{seq})", serde_json::to_string(accent).unwrap(), serde_json::to_string(mode).unwrap())
}

pub fn caps_script(on: bool, accent: &str, seq: u64) -> String {
    format!("setAccent({});setCapsLock({on},{seq})", serde_json::to_string(accent).unwrap())
}

pub fn create_indicator_webview(window: &Window, proxy: EventLoopProxy<UserEvent>) -> Result<WebView> {
    // 字は作者が描いた あ / A (タスクバーのアイコンと同じ原図)
    let html = include_str!("indicator.html")
        .replace("{{KANA_SVG}}", include_str!("../../client/res/mode_kana.svg"))
        .replace("{{LATIN_SVG}}", include_str!("../../client/res/mode_latin.svg"));
    let webview = WebViewBuilder::new()
        .with_transparent(true)
        // 消える動きが終わったら知らせてもらい、窓を隠す ({"type":"hidden","seq":N} だけ受け付ける)
        .with_ipc_handler(move |message| {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(message.body()) else { return };
            if value.get("type").and_then(|v| v.as_str()) == Some("hidden") {
                if let Some(seq) = value.get("seq").and_then(|v| v.as_u64()) {
                    let _ = proxy.send_event(UserEvent::IndicatorHidden(seq));
                }
            }
        })
        .with_navigation_handler(crate::navigation::embedded_html_navigation(&html))
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_html(&html)
        .build(&window)
        .context("Failed to create webview")?;

    Ok(webview)
}
