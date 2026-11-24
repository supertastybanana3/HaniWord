use std::cmp::max;
use std::sync::Arc;

use anyhow::Context as _;
use azookey_server::TonicNamedPipeServer;
use ipc::{WindowAction, WindowController, WindowService};
use shared::proto::window_service_server::WindowServiceServer;
use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::platform::windows::{EventLoopBuilderExtWindows, WindowExtWindows};
use tao::{
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
};
use tokio::sync::mpsc;
use tonic::transport::Server;
use uiaccess::prepare_uiaccess_token;
use utils::get_candidate_window_position;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE},
};

pub mod candidate;
pub mod caret;
pub mod indicator;
pub mod ipc;
/// 2026-11-23 (点検 Astra 02): 候補の窓の持ち主の確かめ
pub mod owner;
pub mod uiaccess;
pub mod utils;
mod navigation;


#[derive(Debug)]
pub enum UserEvent {
    UpdateSize(i32, i32),
    /// 候補 (JSON の配列) と、候補の窓の下に添える文言 (JSON の文字列。2026-11-08 仕事 52)
    UpdateCandidates(String, String),
    UpdateSelection(i32),
    WindowAction(WindowAction),
    /// 2026-10-13: 入力モードの表示が消え終わった (表示側から。seq が最新のときだけ窓を隠す)
    IndicatorHidden(u64),
    /// Caps Lock を見る間隔 (CaretCue と同じく 60ms ごとに GetKeyState)
    CapsTick,
    /// Caps Lock がオンの間に探したテキストカーソルの位置
    CaretAt(Option<caret::CaretRect>),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // obtain uiaccess token
    if let Err(error) = prepare_uiaccess_token() {
        // UIAccess が取れなくても候補ウィンドウは出せる (スタートメニュー上などでは隠れる)
        eprintln!("Failed to obtain UIAccess: {error:?}");
        uiaccess::note_status(&format!("off: {error:?}"));
    }
    if shared::winsec::elevation_status()? { anyhow::bail!("Candidate UI must not continue elevated"); }
    shared::winsec::allow_pipe_identity_queries()?;

    // WebView2 の作業用のデータは、指定がないと実行ファイルの隣 (ui.exe.WebView2) に作られる。
    // ui.exe はユーザーが書き換えられない Program Files に置くので、%APPDATA%\HaniWord (以前と同じ場所) にする
    if std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_none() {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", std::path::PathBuf::from(appdata).join(shared::APP_DIR).join("ui.exe.WebView2"));
        }
    }

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
        .with_any_thread(true)
        .build();

    // initialize window controller
    let (tx, mut rx) = mpsc::channel(32);
    let window_controller = WindowController::new(tx.clone());
    let grpc_service = WindowService {
        controller: window_controller.clone(),
        // 2026-11-23 (点検 Astra 02): 表示を変える要求は、今の入力先の処理からだけ受ける (owner.rs)
        gate: std::sync::Arc::new(owner::Gate::new()),
    };

    #[cfg(not(feature = "security-probe"))]
    let pipe_base = "haniword_ui".to_owned();
    #[cfg(feature = "security-probe")]
    let pipe_base = {
        let base = std::env::var("AZK_TEST_UI_PIPE")?;
        let executable = std::env::current_exe()?;
        if !base.starts_with("haniword_ui_test_") || base.len() > 80
            || !base.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || !executable.parent().is_some_and(|parent| parent.join("isolated_security_fixture.json").is_file()) {
            anyhow::bail!("UI probe requires an isolated fixture and a test-only pipe");
        }
        base
    };

    // start grpc server
    tokio::spawn(async move {
        println!("WindowServer listening");
        Server::builder()
            .max_concurrent_streams(4)
            .concurrency_limit_per_connection(4)
            .add_service(WindowServiceServer::new(grpc_service).max_decoding_message_size(65536))
            .serve_with_incoming(TonicNamedPipeServer::new(&pipe_base))
            .await
            .unwrap_or_else(|error| {
                // 2026-10-20 (Fable の再調査 L6): 待ち受けられなければ、黙って何もしない候補窓のまま残らずに終わる
                eprintln!("WindowServer failed: {error}");
                std::process::exit(1);
            });
    });

    let event_loop_proxy = event_loop.create_proxy();

    let proxy_clone = event_loop_proxy.clone();
    let candidate_window = candidate::create_candidate_window(&event_loop)?;
    // 2026-11-06 (仕事 37): 候補の窓の外観 (設定アプリの「外観」)。作るときに今の値を埋め、候補の窓を出すたびに settings.json の更新の時刻を見て、
    // 変わっていれば読み直して当て直す (開き直さなくても次の候補から効く)。アクセントの色を使うときは、Windows の色が変わったときも当て直す
    let settings_path = shared::AppConfig::path();
    let settings_stamp = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut appearance_stamp = settings_stamp(&settings_path);
    let mut appearance = shared::AppConfig::read_or_default().appearance.sanitized();
    let mut appearance_accent = indicator::accent_color();
    let candidate_webview_builder = candidate::create_candidate_webview(&candidate::appearance_json(&appearance, &appearance_accent))?;
    let candidate_webview = candidate_webview_builder
        // 2026-11-22 (公開前の点検): 開発者ツール (右クリックの検査・F12) は開発用のビルドだけ。wry は Windows の配る版でもこの値をそのまま WebView2 に渡す
        .with_devtools(cfg!(debug_assertions))
        .with_ipc_handler(move |message| {
            if let Ok(message) = serde_json::from_str::<serde_json::Value>(message.body()) {
                if let Some(type_value) = message.get("type") {
                    if type_value == "resize" {
                        let width = message.get("width").and_then(|v| v.as_f64());
                        let height = message.get("height").and_then(|v| v.as_f64());
                        if let (Some(width), Some(height)) = (width, height) {
                            if !(1.0..=16384.0).contains(&width) || !(1.0..=16384.0).contains(&height) { return; }
                            proxy_clone
                                .send_event(UserEvent::UpdateSize(width as i32, height as i32))
                                .ok();
                        }
                    }
                }
            }
        })
        .build(&candidate_window)?;

    let indicator_window = indicator::create_indicator_window(&event_loop)?;
    let indicator_webview = indicator::create_indicator_webview(&indicator_window, event_loop_proxy.clone())?;
    // 2026-11-06 (仕事 35): 表示の形 (札・小さく・出さない)。モードの知らせのたびと、Caps Lock を見る間に約 1 秒ごとに設定を読み直す
    let mut indicator_style = indicator::Style::current();
    indicator_window.set_inner_size(indicator_style.window_size());
    let mut style_ticks: u32 = 0;

    // 2026-10-13: Caps Lock は CaretCue と同じく GetKeyState を 60ms ごとに見る (イベントループのスレッドで)。
    // オンの間は別のスレッドでテキストカーソルを探して (UI オートメーション → Win32)、表示を付いていかせる
    let caps_tracking = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let proxy = event_loop_proxy.clone();
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(std::time::Duration::from_millis(60));
            loop {
                ticks.tick().await;
                if proxy.send_event(UserEvent::CapsTick).is_err() { break; }
            }
        });
        let proxy = event_loop_proxy.clone();
        let tracking = caps_tracking.clone();
        std::thread::spawn(move || {
            let locator = caret::CaretLocator::new();
            loop {
                if tracking.load(std::sync::atomic::Ordering::Relaxed) {
                    if proxy.send_event(UserEvent::CaretAt(locator.locate())).is_err() { break; }
                    std::thread::sleep(std::time::Duration::from_millis(120));
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(60));
                }
            }
        });
    }
    let mut indicator_seq: u64 = 0;
    let mut indicator_visible = false;
    let mut indicator_caps_shown = false;
    let mut caps_on = false;
    let mut candidate_shown = false;
    let mut last_ime_rect: Option<caret::CaretRect> = None;

    // handle window actions
    // 2026-10-19 (security review #11): 表示へ送れない・描けないとき (終了の途中など) は、落ちずにその 1 回を捨てる
    let proxy_clone = event_loop_proxy.clone();
    tokio::spawn(async move {
        while let Some(action) = rx.recv().await {
            match action {
                WindowAction::Show => {
                    proxy_clone
                        .send_event(UserEvent::WindowAction(WindowAction::Show))
                        .ok();
                }
                WindowAction::Hide => {
                    proxy_clone
                        .send_event(UserEvent::WindowAction(WindowAction::Hide))
                        .ok();
                }
                WindowAction::SetPosition {
                    top,
                    left,
                    bottom,
                    right,
                } => {
                    proxy_clone
                        .send_event(UserEvent::WindowAction(WindowAction::SetPosition {
                            top,
                            left,
                            bottom,
                            right,
                        }))
                        .ok();
                }
                WindowAction::SetCandidate { candidates, note } => {
                    proxy_clone
                        .send_event(UserEvent::WindowAction(WindowAction::SetCandidate {
                            candidates,
                            note,
                        }))
                        .ok();
                }
                WindowAction::SetSelection { index } => {
                    proxy_clone
                        .send_event(UserEvent::WindowAction(WindowAction::SetSelection {
                            index,
                        }))
                        .ok();
                }
                WindowAction::SetInputMode(input_method, ai_paused) => {
                    proxy_clone
                        .send_event(UserEvent::WindowAction(WindowAction::SetInputMode(
                            input_method,
                            ai_paused,
                        )))
                        .ok();
                }
            }
        }
    });

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        let indicator_hwnd = indicator_window.hwnd();

        match event {
            Event::NewEvents(StartCause::Init) => {}
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            Event::UserEvent(script) => match script {
                UserEvent::UpdateCandidates(candidates, note) => {
                    candidate_webview
                        .evaluate_script(&format!("updateCandidates({}, {})", candidates, note))
                        .ok();
                }
                UserEvent::UpdateSelection(index) => {
                    candidate_webview
                        .evaluate_script(&format!("updateSelection({})", index))
                        .ok();
                }
                UserEvent::IndicatorHidden(seq) => {
                    if seq == indicator_seq {
                        let _ = unsafe { ShowWindow(HWND(indicator_hwnd as *mut std::ffi::c_void), SW_HIDE) };
                        indicator_visible = false;
                        indicator_caps_shown = false;
                    }
                }
                UserEvent::CapsTick => {
                    style_ticks += 1;
                    if style_ticks >= 16 {
                        style_ticks = 0;
                        restyle(indicator::Style::current(), &mut indicator_style, &indicator_window, &indicator_webview,
                            &mut indicator_seq, &mut indicator_visible, &mut indicator_caps_shown);
                    }
                    let now = unsafe { (GetKeyState(VK_CAPITAL.0 as i32) & 1) != 0 };
                    if now != caps_on {
                        caps_on = now;
                        if !now && indicator_visible {
                            // 消える動きのあとで表示側から知らせが来て窓を隠す
                            indicator_seq += 1;
                            let script = indicator_style.script(&indicator::message_font())
                                + &indicator::caps_script(false, &indicator::accent_color(), indicator_seq);
                            let _ = indicator_webview.evaluate_script(&script);
                            indicator_caps_shown = false;
                        }
                    }
                    // 出さない形では、Caps Lock の表示も出さない (テキストカーソルも探さない)
                    caps_tracking.store(caps_on && indicator_style != indicator::Style::Off, std::sync::atomic::Ordering::Relaxed);
                }
                UserEvent::CaretAt(found) => {
                    if caps_on && !candidate_shown && indicator_style != indicator::Style::Off {
                        if let Some(rect) = found.or(if indicator_caps_shown { None } else { last_ime_rect }) {
                            indicator_window.set_outer_position(indicator_style.origin(rect, indicator_window.scale_factor()));
                        }
                        if !indicator_caps_shown && (found.is_some() || last_ime_rect.is_some()) {
                            indicator_seq += 1;
                            let _ = unsafe { ShowWindow(HWND(indicator_hwnd as *mut std::ffi::c_void), SW_SHOWNOACTIVATE) };
                            let script = indicator_style.script(&indicator::message_font())
                                + &indicator::caps_script(true, &indicator::accent_color(), indicator_seq);
                            let _ = indicator_webview.evaluate_script(&script);
                            indicator_visible = true;
                            indicator_caps_shown = true;
                        }
                    }
                }
                UserEvent::UpdateSize(width, height) => {
                    candidate_window.set_inner_size(LogicalSize::new(width, height));
                }
                UserEvent::WindowAction(action) => {
                    match action {
                        WindowAction::Show => {
                            let stamp = settings_stamp(&settings_path);
                            let accent = indicator::accent_color();
                            if stamp != appearance_stamp || (appearance.selected_color == "accent" && accent != appearance_accent) {
                                appearance_stamp = stamp;
                                let next = shared::AppConfig::read_or_default().appearance.sanitized();
                                if next != appearance || accent != appearance_accent {
                                    appearance = next;
                                    appearance_accent = accent;
                                    let _ = candidate_webview.evaluate_script(&candidate::appearance_script(
                                        &candidate::appearance_json(&appearance, &appearance_accent)));
                                }
                            }
                            // 候補ウィンドウを出すときは、入力モードの表示 (Caps Lock も) をすぐ隠す。候補を閉じたら Caps Lock は戻る
                            candidate_shown = true;
                            if indicator_visible {
                                indicator_seq += 1;
                                let _ = indicator_webview.evaluate_script("hideNow()");
                                let _ = unsafe { ShowWindow(HWND(indicator_hwnd as *mut std::ffi::c_void), SW_HIDE) };
                                indicator_visible = false;
                                indicator_caps_shown = false;
                            }

                            let _ = unsafe {
                                ShowWindow(
                                    HWND(candidate_window.hwnd() as *mut std::ffi::c_void),
                                    SW_SHOWNOACTIVATE,
                                )
                            };
                        }
                        WindowAction::Hide => {
                            candidate_shown = false;
                            let _ = unsafe {
                                ShowWindow(
                                    HWND(candidate_window.hwnd() as *mut std::ffi::c_void),
                                    SW_HIDE,
                                )
                            };
                        }
                        WindowAction::SetPosition {
                            top,
                            left,
                            bottom,
                            right,
                        } => {
                            let (x, y) = get_candidate_window_position(
                                top,
                                left,
                                bottom,
                                right,
                                &candidate_window,
                            );

                            unsafe {
                                let _ = SetWindowPos(
                                    HWND(candidate_window.hwnd() as *mut std::ffi::c_void),
                                    HWND_TOPMOST,
                                    0,
                                    0,
                                    0,
                                    0,
                                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                                );

                                let _ = SetWindowPos(
                                    HWND(indicator_hwnd as *mut std::ffi::c_void),
                                    HWND_TOPMOST,
                                    0,
                                    0,
                                    0,
                                    0,
                                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                                );
                            }
                            candidate_window
                                .set_outer_position(PhysicalPosition::new(x as f64, y as f64));
                            // 入力モードの表示は IME から届いたカーソルの真下 (Caps Lock を出しっぱなしの間は、探した位置に任せる)
                            last_ime_rect = Some((top, left, bottom, right));
                            if !indicator_caps_shown && indicator_style != indicator::Style::Off {
                                indicator_window.set_outer_position(indicator_style.origin(
                                    (top, left, bottom, right),
                                    indicator_window.scale_factor(),
                                ));
                            }
                        }
                        WindowAction::SetCandidate { candidates, note } => {
                            let candidates = serde_json::to_string(&candidates)
                                .context("Failed to serialize candidates")
                                .unwrap_or_default();
                            // 文言も JSON の文字列にして渡す (スクリプトの中で字がそのまま文字列になるように)
                            let note = serde_json::to_string(&note).unwrap_or_else(|_| "\"\"".to_string());

                            event_loop_proxy
                                .send_event(UserEvent::UpdateCandidates(candidates, note))
                                .ok();
                        }
                        WindowAction::SetSelection { index } => {
                            event_loop_proxy
                                .send_event(UserEvent::UpdateSelection(index))
                                .ok();
                        }
                        WindowAction::SetInputMode(input_method, ai_paused) => {
                            // 札: CaretCue と同じ動き (出る 0.18 秒 → 0.47 秒止まる → 0.15 秒で消える)。小さく: 2.5 秒止まって 0.13 秒で消える
                            // (動きは indicator.html)。消え終わったら表示側から IndicatorHidden が来て窓を隠す。Caps Lock がオンならその表示に戻る。
                            // 出さない: 何もしない (タスクバーの右下の表示だけ)
                            restyle(indicator::Style::current(), &mut indicator_style, &indicator_window, &indicator_webview,
                                &mut indicator_seq, &mut indicator_visible, &mut indicator_caps_shown);
                            caps_tracking.store(caps_on && indicator_style != indicator::Style::Off, std::sync::atomic::Ordering::Relaxed);
                            if !candidate_shown && indicator_style != indicator::Style::Off {
                                if let Some(rect) = last_ime_rect {
                                    indicator_window.set_outer_position(indicator_style.origin(rect, indicator_window.scale_factor()));
                                }
                                indicator_seq += 1;
                                let _ = unsafe { ShowWindow(HWND(indicator_hwnd as *mut std::ffi::c_void), SW_SHOWNOACTIVATE) };
                                let script = indicator_style.script(&indicator::message_font())
                                    + &indicator::mode_script(&input_method, &indicator::accent_color(), indicator_seq, ai_paused);
                                let _ = indicator_webview.evaluate_script(&script);
                                indicator_visible = true;
                                indicator_caps_shown = false;
                            }
                        }
                    }
                }
            },
            _ => (),
        }
    });
}

/// 2026-11-06 (仕事 35): 設定の表示の形が変わったら、出ている表示をすぐ隠して、窓をその形の大きさにする
fn restyle(
    next: indicator::Style,
    style: &mut indicator::Style,
    window: &tao::window::Window,
    webview: &wry::WebView,
    seq: &mut u64,
    visible: &mut bool,
    caps_shown: &mut bool,
) {
    if next == *style { return; }
    *style = next;
    if *visible {
        *seq += 1;
        let _ = webview.evaluate_script("hideNow()");
        let _ = unsafe { ShowWindow(HWND(window.hwnd() as *mut std::ffi::c_void), SW_HIDE) };
        *visible = false;
        *caps_shown = false;
    }
    window.set_inner_size(next.window_size());
}

#[cfg(test)]
mod tests {
    #[test]
    fn appearance_is_passed_as_json_not_as_source() {
        // 2026-11-06 (仕事 37): フォントの名前は sanitized で字を限ったうえ、JSON の文字列として渡す
        let a: shared::AppearanceConfig = serde_json::from_value(serde_json::json!({
            "font": "Meiryo UI", "latin_font": "x\");alert(1);//", "selected_color": "accent", "font_size": 18})).unwrap();
        let json = super::candidate::appearance_json(&a, "#0078d4");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["font"], "Meiryo UI");
        assert_eq!(v["latinFont"], "");
        assert_eq!(v["fontSize"], 18);
        assert_eq!(v["selectedColor"], "accent");
        assert_eq!(v["accent"], "#0078d4");
        assert_eq!(super::candidate::appearance_script(&json), format!("applyAppearance({json})"));
        assert_eq!(v["fallback"], serde_json::Value::Null);
        let b: shared::AppearanceConfig = serde_json::from_value(serde_json::json!({"fallback": ["BIZ UDPゴシック", "@notdef", "Meiryo UI"]})).unwrap();
        let v: serde_json::Value = serde_json::from_str(&super::candidate::appearance_json(&b, "#0078d4")).unwrap();
        assert_eq!(v["fallback"], serde_json::json!(["BIZ UDPゴシック", "@notdef"]));
    }

    /// 2026-11-08 (仕事 52): 重いので AI を休んでいる間だけ、札の右下に点を出す命令を足す (true/false の字だけ。ほかの字は入らない)
    #[test]
    fn ai_paused_mark_goes_before_the_mode() {
        assert!(super::indicator::mode_script("あ", "#0078d4", 3, true).contains("setAIPaused(true);updateInputMethod("));
        assert!(super::indicator::mode_script("A", "#0078d4", 3, false).contains("setAIPaused(false);updateInputMethod("));
    }

    #[test]
    fn input_mode_is_a_json_argument_not_executable_source() {
        for mode in ["A", "あ", "\");globalThis.auditMarker=true;//", "\\\"\n\0"] {
            let script = super::indicator::mode_script(mode, "#0078d4", 7, false);
            let call = script.split_once(";updateInputMethod(").unwrap().1;
            let argument = call.strip_suffix(",7)").unwrap();
            assert_eq!(serde_json::from_str::<String>(argument).unwrap(), mode);
        }
    }

    #[test]
    fn indicator_style_follows_the_setting() {
        use super::indicator::Style;
        assert_eq!(Style::from_setting("badge"), Style::Badge);
        assert_eq!(Style::from_setting("compact"), Style::Compact);
        assert_eq!(Style::from_setting("off"), Style::Off);
        assert_eq!(Style::from_setting("balloon"), Style::Badge);
        // フォントの名前は JSON の文字列として渡す
        let script = Style::Compact.script("\");alert(1);//");
        assert_eq!(script, "setStyle(\"compact\",\"\\\");alert(1);//\");");
    }

    #[test]
    fn compact_tail_points_at_the_caret() {
        // 細いカーソル (左端 100、行の下端 220) の画素の中央に尖りの先 (窓の中の (31.5, 9)) が来る
        let origin = super::indicator::compact_origin((200, 100, 220, 101), 1.0);
        assert_eq!((origin.x, origin.y), (69.0, 211.0));
        // 150%: 尖りの先は窓の中の (47.25, 13.5) 物理ピクセル
        let origin = super::indicator::compact_origin((200, 100, 220, 101), 1.5);
        assert_eq!((origin.x, origin.y), (53.0, 207.0));
        // 幅のある矩形 (変換中の文字列) は右端
        let origin = super::indicator::compact_origin((200, 100, 220, 300), 1.0);
        assert_eq!(origin.x, 269.0);
    }
}
