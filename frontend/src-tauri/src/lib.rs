mod ipc;

#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]
compile_error!("Release settings must embed the frontend; enable custom-protocol.");

use serde::{Deserialize, Serialize};
use shared::AppConfig;
use std::{path::PathBuf, sync::Mutex};

#[derive(Debug)]
pub struct AppState {
    settings: Mutex<AppConfig>,
    ipc: ipc::IPCService,
}

impl AppState {
    fn new() -> Self {
        AppState {
            settings: Mutex::new(AppConfig::new()),
            ipc: ipc::IPCService::new().unwrap(),
        }
    }
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> AppConfig {
    let config = state.settings.lock().unwrap();
    config.clone()
}

#[tauri::command]
fn update_config(state: tauri::State<AppState>, new_config: AppConfig) -> Result<(), String> {
    if !shared::winsec::private_data_allowed() { return Err("Settings require an unelevated desktop process".into()); }
    let mut config = state.settings.lock().unwrap();
    *config = new_config;
    // 2026-10-19 (security review #8): 書けなかったら画面に知らせる (落ちない)
    config.try_write().map_err(|error| format!("設定を保存できませんでした: {error}"))?;

    state.ipc.clone().update_config().map_err(|error| error.to_string())?;
    Ok(())
}

/// 2026-11-08: 学習をまっさらにする。include_tables が偽なら変換の学習 (azooKey の memory.*) だけ、真なら HaniWord の学習の表も。
/// 消す前に変換サーバーが %APPDATA%\HaniWord\memory_backup_<日時> へ控えを取り、その場所を返す (控えを取れなければ何も消さず、エラー)
#[tauri::command]
fn reset_learning(state: tauri::State<AppState>, include_tables: bool) -> Result<String, String> {
    if !shared::winsec::private_data_allowed() { return Err("Settings require an unelevated desktop process".into()); }
    state.ipc.clone().reset_learning(include_tables)
        .map_err(|error| format!("学習を消せませんでした (変換サーバーにつながらないか、控えを取れませんでした。何も消していません): {error}"))
}

/// 2026-10-29: この版の番号 (設定画面の「バージョンと更新プログラム」に出す。shared::APP_VERSION の 1 か所で持つ)
#[tauri::command]
fn app_version() -> &'static str {
    shared::APP_VERSION
}

/// 2026-11-19: launcher が変換サーバーを起動する前に書いた、実際に使う AI の版と設定から下げたわけ (%APPDATA%\HaniWord\ai_backend.json)。
/// 「HaniWord について」と Zenzai の画面に出す。無い (まだ一度も起動していない・古い launcher)・読めないなら None
#[tauri::command]
fn ai_backend_status() -> Option<shared::AiBackendStatus> {
    shared::AiBackendStatus::read()
}

/// 2026-10-31: 入っている分野辞書の名前 (%APPDATA%\HaniWord\domain\domains.txt)。設定画面の専門用語の欄は、ここにある分野だけを出す。
/// 読めなければ None で、画面は知っている分野を全部出す
#[tauri::command]
fn installed_domains() -> Option<Vec<String>> {
    shared::installed_domain_names()
}

/// 2026-11-06 (仕事 39): 入っている方言の言い方の辞書の名前 (%APPDATA%\HaniWord\domain\dialects.txt)。設定画面の「方言の語尾」は、ここにある方言だけを出す。
/// 読めなければ None で、画面は知っている方言 (shared の DIALECT_NAMES) を出す
#[tauri::command]
fn installed_dialects() -> Option<Vec<String>> {
    shared::installed_dialect_names()
}

/// 2026-11-07 (仕事 43): 入っているくだけた言葉の辞書の名前 (%APPDATA%\HaniWord\domain\slang.txt)。設定画面の「ネットスラング」は、internet があるときだけ出す。
/// 読めなければ None で、画面は出す
#[tauri::command]
fn installed_slang() -> Option<Vec<String>> {
    shared::installed_slang_names()
}

/// 2026-11-06: 次の語の候補の表が入っていない言語 (この設定アプリの隣の next_words に表が無い言語)。インストーラーは英語の表だけ
/// 必ず入れ、ほかの言語は選んだものだけ入れる。next_words が無ければ (組み立てたまま動かしているとき) None で、画面は何も出さない
#[tauri::command]
fn missing_next_words() -> Option<Vec<String>> {
    let dir = std::env::current_exe().ok()?.parent()?.join("next_words");
    shared::missing_next_word_languages(&dir)
}

/// 2026-11-06 (仕事 37): Windows に入っているフォントの名前 (外観の候補のフォントを選ぶため)。日本語の字を持つフォント (SHIFTJIS_CHARSET でも
/// 数えられるもの) には japanese を付ける。縦書き用 (@ で始まる名前) は除く
#[derive(serde::Serialize)]
struct FontFamily {
    name: String,
    japanese: bool,
}

#[tauri::command]
fn system_fonts() -> Vec<FontFamily> {
    use std::collections::BTreeMap;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::Graphics::Gdi::{EnumFontFamiliesExW, GetDC, ReleaseDC, DEFAULT_CHARSET, LOGFONTW, SHIFTJIS_CHARSET, TEXTMETRICW};
    unsafe extern "system" fn each(font: *const LOGFONTW, _metric: *const TEXTMETRICW, _kind: u32, data: LPARAM) -> i32 {
        let (Some(font), Some(map)) = (font.as_ref(), (data.0 as *mut BTreeMap<String, bool>).as_mut()) else { return 1 };
        let len = font.lfFaceName.iter().position(|&c| c == 0).unwrap_or(font.lfFaceName.len());
        let name = String::from_utf16_lossy(&font.lfFaceName[..len]);
        if !name.is_empty() && !name.starts_with('@') {
            *map.entry(name).or_insert(false) |= font.lfCharSet == SHIFTJIS_CHARSET;
        }
        1
    }
    let mut map: BTreeMap<String, bool> = BTreeMap::new();
    unsafe {
        let dc = GetDC(HWND::default());
        let query = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
        EnumFontFamiliesExW(dc, &query, Some(each), LPARAM(&mut map as *mut _ as isize), 0);
        ReleaseDC(HWND::default(), dc);
    }
    map.into_iter().map(|(name, japanese)| FontFamily { name, japanese }).collect()
}

/// 2026-11-06 (仕事 37): Windows のアクセントの色 (外観の見本で「Windows のアクセントの色」を見せるため。表示の窓と同じく DwmGetColorizationColor、取れなければ #0078d4)
#[tauri::command]
fn accent_color() -> String {
    let mut argb = 0u32;
    let mut opaque = windows::Win32::Foundation::BOOL(0);
    if unsafe { windows::Win32::Graphics::Dwm::DwmGetColorizationColor(&mut argb, &mut opaque) }.is_err() {
        return "#0078d4".into();
    }
    format!("#{:02x}{:02x}{:02x}", (argb >> 16) & 0xff, (argb >> 8) & 0xff, argb & 0xff)
}

/// ライセンス表示 (インストール先の THIRD_PARTY_NOTICES.txt) を既定のアプリで開く
#[tauri::command]
fn open_notices() -> Result<(), String> {
    let appdata = std::env::var("APPDATA").map_err(|e| e.to_string())?;
    let path = std::path::Path::new(&appdata).join(shared::APP_DIR).join("THIRD_PARTY_NOTICES.txt");
    if !path.exists() {
        return Err("THIRD_PARTY_NOTICES.txt が見つかりません".to_string());
    }
    std::process::Command::new("notepad.exe").arg(path).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// 2026-11-04: 単語登録 (%APPDATA%\HaniWord\user_dictionary.tsv) の一覧。読めなかった行は errors に
#[tauri::command]
fn user_dictionary_list() -> shared::user_dictionary::Parsed {
    shared::user_dictionary::load()
}

/// 単語登録を保存して、変換サーバーに読み直しを知らせる (知らせられなくても、変換サーバーはファイルの更新時刻を見て読み直す)
#[tauri::command]
fn user_dictionary_save(state: tauri::State<AppState>, entries: Vec<shared::user_dictionary::Entry>) -> Result<(), String> {
    shared::user_dictionary::save(&entries)?;
    let _ = state.ipc.clone().update_config();
    Ok(())
}

/// 選んだファイル (ほかの IME の辞書・書き出した表) を読む。保存はしない (画面で今の登録に足してから保存する)
#[tauri::command]
fn user_dictionary_parse(bytes: Vec<u8>) -> Result<shared::user_dictionary::Parsed, String> {
    let text = shared::user_dictionary::decode(&bytes)?;
    Ok(shared::user_dictionary::parse_tsv(&text))
}

/// 単語登録を書き出す (ドキュメントのフォルダに HaniWord_単語登録.tsv。あれば (2)・(3)… を付ける)。置いた場所をエクスプローラーで見せ、場所を返す
#[tauri::command]
fn user_dictionary_export(app: tauri::AppHandle, entries: Vec<shared::user_dictionary::Entry>) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    use tauri::Manager;
    // ドキュメントのフォルダは OneDrive に移っていることがあるので、Windows に場所を聞く
    let folder = app.path().document_dir().or_else(|_| app.path().home_dir()).map_err(|e| e.to_string())?;
    let path = (1..1000)
        .map(|n| folder.join(if n == 1 { "HaniWord_単語登録.tsv".to_string() } else { format!("HaniWord_単語登録 ({n}).tsv") }))
        .find(|p| !p.exists())
        .ok_or("書き出す名前が見つかりません")?;
    std::fs::write(&path, shared::user_dictionary::to_tsv(&entries).as_bytes()).map_err(|e| format!("書き出せませんでした: {e}"))?;
    // explorer.exe は /select,"<場所>" の形でないと、空白のある名前を選べない
    let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
    Ok(path.display().to_string())
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Capability {
    cpu: bool,
    cuda: bool,
    vulkan: bool,
}

#[tauri::command]
fn check_capability() -> Capability {
    // cuda:
    // cudart64_12.dll
    // cublas64_12.dll

    // vulkan:
    // vulkan-1.dllの存在確認

    let mut capability = Capability {
        cpu: true,
        cuda: false,
        vulkan: false,
    };

    // Check for CUDA availability
    let cuda_files = ["cudart64_12.dll", "cublas64_12.dll"];
    let cuda_available = cuda_files.iter().all(|file| {
        // Check if the file exists in system path or in the current directory
        std::env::var("PATH")
            .unwrap_or_default()
            .split(';')
            .map(PathBuf::from)
            .chain(std::iter::once(std::env::current_dir().unwrap_or_default()))
            .any(|path| path.join(file).exists())
    });
    // 2026-10-20: HaniWord の配布物には CUDA 版の llama.cpp (llama_cuda) を入れていないので、自分で置いたときだけ選べる
    let cuda_backend = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(shared::APP_DIR).join("llama_cuda");
    capability.cuda = cuda_available && cuda_backend.join("ggml-cuda.dll").exists();

    // Check for Vulkan availability
    let vulkan_file = "vulkan-1.dll";
    let vulkan_available = std::env::var("PATH")
        .unwrap_or_default()
        .split(';')
        .map(PathBuf::from)
        .chain(std::iter::once(std::env::current_dir().unwrap_or_default()))
        .any(|path| path.join(vulkan_file).exists());
    capability.vulkan = vulkan_available;

    capability
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new();

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_opener::init())
        // 2026-11-04: 言語バーの「単語登録を開く」は --user-dictionary を付けて起動する。はじめから単語登録のページを出す
        .setup(|app| {
            use tauri::Manager;
            if std::env::args().any(|argument| argument == "--user-dictionary") {
                if let Some(window) = app.get_webview_window("main") {
                    if let Ok(mut url) = window.url() {
                        url.set_path("/user-dictionary");
                        let _ = window.navigate(url);
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_config,
            update_config,
            check_capability,
            open_notices,
            app_version,
            ai_backend_status,
            installed_domains,
            installed_dialects,
            installed_slang,
            missing_next_words,
            system_fonts,
            accent_color,
            user_dictionary_list,
            user_dictionary_save,
            user_dictionary_parse,
            user_dictionary_export,
            reset_learning
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn settings_embed_html_and_scripts_without_a_dev_server() {
        assert!(!tauri::is_dev(), "Settings must not require localhost:1420");
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let assets = context.assets();
        assert!(assets.get(&"index.html".into()).is_some(), "Missing embedded settings HTML");
        assert!(assets.iter().any(|(key, _)| key.as_ref().ends_with(".js")), "Missing embedded settings JavaScript");
    }
}
