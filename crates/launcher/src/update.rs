//! 2026-10-29: 新しい版を知らせる (settings.json の notify_updates、初期値オン)。
//! 1 日 1 回、GitHub の API で最新のリリース (releases/latest) の tag を読み、入っている版 (shared::APP_VERSION) より新しければ
//! Windows の通知 (トースト) で知らせる。押すとリリースのページが開く。落とす・入れるはしない (知らせるだけ)。
//! 送るのは HTTPS の GET だけ (User-Agent は HaniWord/<版>)。打った文字・学習・設定・PC の情報は送らない。
//! 返事があって失敗 (回数制限・GitHub が落ちている・読めない返事) なら黙って次の日に、つながらなければ 1 時間後にまた試す。
//! 同じ版は 1 回だけ知らせる (通知を出せた版を覚えておく)

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const HOUR: u64 = 60 * 60;
const DAY: u64 = 24 * HOUR;
/// 次に確かめる時刻と知らせた版 (%APPDATA%\HaniWord に置く。settings.json とは別: 設定画面が書き戻しても消えないように)
const STATE_FILE: &str = "update_check.json";
/// 通知の差出人 (HKCU\Software\Classes\AppUserModelId に名前を登録する。アンインストーラーが消す)
const APP_ID: &str = "HaniWord.Updates";
/// 返事の本文の上限 (リリースの説明が長くても 1 回分は数十 KB)
const MAX_BODY: usize = 1024 * 1024;
/// 本文を受け取る時間の合計の上限 (少しずつ送られても、ここで打ち切る)
const READ_LIMIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u64, u64, u64);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// tag の版 (v0.1.1・0.2・1.0.0+build)。前の v と + から後ろ (ビルドの印) は見ない。
/// 試作の版 (0.2.0-rc.1・0.2.0-beta など - の付くもの) と読めないものは None (数えない)
pub fn parse_version(tag: &str) -> Option<Version> {
    let tag = tag.trim();
    let tag = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    let core = tag.split('+').next().unwrap_or_default();
    if core.contains('-') { return None; }
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() > 3 || parts.iter().any(|part| part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let number = |index: usize| parts.get(index).map_or(Some(0), |part| part.parse().ok());
    Some(Version(number(0)?, number(1)?, number(2)?))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// 次に確かめる時刻 (UNIX 時刻の秒)。返事があれば 1 日あと、つながらなければ 1 時間あと
    pub next_check: u64,
    /// 通知を出せた版 (同じ版は 2 回知らせない)
    pub notified: String,
}

impl State {
    fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else { return Self::default() };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return Self::default() };
        State {
            next_check: value["next_check"].as_u64().unwrap_or(0),
            notified: value["notified"].as_str().unwrap_or_default().chars().take(32).collect(),
        }
    }

    fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(&serde_json::json!({"next_check": self.next_check, "notified": self.notified}))?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, path).inspect_err(|_| { let _ = std::fs::remove_file(&temporary); })
    }
}

/// 確かめる時か (次に確かめる時刻になった。時計が 1 日より前に戻ったときも確かめ直す)
pub fn due(state: &State, now: u64) -> bool {
    now >= state.next_check || state.next_check > now + DAY
}

/// 問い合わせの結果
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// つながらない・時間内に受け取れなかった (1 時間後にまた試す)
    Unreachable,
    /// 返事があった (状態コード, 本文。本文が上限を超えたら空)
    Status(u32, Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub version: Version,
    /// 押したときに開くページ。返事の html_url は使わず、tag から作る (このリポジトリのリリースのページだけ)
    pub url: String,
}

/// GitHub の releases/latest の返事から、知らせる版とページを読む。下書き・試作 (prerelease) は数えない
fn parse_release(body: &[u8], repository: &str) -> Option<Notice> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    if value["draft"].as_bool() == Some(true) || value["prerelease"].as_bool() == Some(true) { return None; }
    let tag = value["tag_name"].as_str()?;
    let version = parse_version(tag)?;
    // tag が頭の v と数字と . だけならその tag のページ、ほか (+ のビルドの印・空白) は最新のリリースのページ
    let plain = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    let url = if !plain.is_empty() && plain.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        format!("https://github.com/{repository}/releases/tag/{tag}")
    } else {
        format!("https://github.com/{repository}/releases/latest")
    };
    Some(Notice { version, url })
}

/// 1 回分の確かめ。fetch は最新のリリースを問い合わせる (試験では偽の返事)。知らせることがあれば返す。
/// 次に確かめる時刻だけを書き換え、知らせた版は呼ぶ側が通知を出せてから書く。保存も呼ぶ側
pub fn check(state: &mut State, now: u64, current: Version, repository: &str, fetch: impl FnOnce() -> Reply) -> Option<Notice> {
    if !due(state, now) { return None; }
    let (status, body) = match fetch() {
        Reply::Unreachable => {
            state.next_check = now + HOUR;
            return None;
        }
        Reply::Status(status, body) => (status, body),
    };
    state.next_check = now + DAY;
    if status != 200 { return None; }
    let notice = parse_release(&body, repository)?;
    if notice.version <= current || state.notified == notice.version.to_string() { return None; }
    Some(notice)
}

/// settings.json の notify_updates。ファイルが無ければ初期値 (オン)、項目が無ければオン (前の版の設定)。
/// ファイルはあるのに読めない・壊れている・型が違うときはオフ (切った人の設定を読めないまま問い合わせない)
fn notifications_enabled(settings: &Path) -> bool {
    match std::fs::read(settings) {
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(serde_json::Value::Object(map)) => match map.get("notify_updates") {
                None => true,
                Some(value) => value.as_bool() == Some(true),
            },
            _ => false,
        },
    }
}

/// 状態はメモリに持ち、ファイルは残すためだけに使う (残せなくても、メモリの状態で次の時刻まで待つ)
pub struct Updater {
    settings: PathBuf,
    path: PathBuf,
    state: State,
}

impl Updater {
    pub fn new(app: &Path) -> Self {
        let path = app.join(STATE_FILE);
        Updater { settings: app.join("settings.json"), state: State::load(&path), path }
    }

    /// 1 回分: 設定がオンで確かめる時なら確かめ、知らせる。show は通知を出せたら true
    fn tick(&mut self, now: u64, current: Version, repository: &str, fetch: impl FnOnce() -> Reply,
        show: impl FnOnce(&Notice) -> bool) {
        if !notifications_enabled(&self.settings) { return; }
        let before = self.state.clone();
        if let Some(notice) = check(&mut self.state, now, current, repository, fetch) {
            if show(&notice) { self.state.notified = notice.version.to_string(); }
        }
        if self.state != before {
            if let Err(error) = self.state.save(&self.path) { eprintln!("[launcher]: cannot save {}: {error}", self.path.display()); }
        }
    }
}

/// 常駐している launcher の裏で、1 時間ごとに「確かめる時か」を見る (起動の直後は 1 分待つ)。設定は毎回読み直す
pub fn spawn(app: PathBuf) {
    // 利用者の設定を読めない起動 (管理者・サンドボックス) では確かめない
    if !shared::winsec::private_data_allowed() { return; }
    let Some(current) = parse_version(shared::APP_VERSION) else { return };
    std::thread::spawn(move || {
        let mut updater = Updater::new(&app);
        std::thread::sleep(Duration::from_secs(60));
        loop {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            updater.tick(now, current, shared::RELEASE_REPOSITORY, || fetch_latest(shared::RELEASE_REPOSITORY), |notice| {
                show(notice).inspect_err(|error| eprintln!("[launcher]: cannot show the update notice: {error:?}")).is_ok()
            });
            std::thread::sleep(Duration::from_secs(HOUR));
        }
    });
}

/// HTTPS の GET を 1 回 (WinHTTP。クッキーは使わない・HTTPS から HTTP への転送はしない (WinHTTP の既定)・各段階 15 秒、受け取りは合計 60 秒まで)
fn fetch_latest(repository: &str) -> Reply {
    fetch_from("api.github.com", windows::Win32::Networking::WinHttp::INTERNET_DEFAULT_HTTPS_PORT,
        &format!("/repos/{repository}/releases/latest"), true, READ_LIMIT)
}

/// GET を 1 回。試験では手元の偽のサーバー (secure = false) に向ける。limit は受け取りの合計の時間
fn fetch_from(host: &str, port: u16, path: &str, secure: bool, limit: Duration) -> Reply {
    fetch_inner(host, port, path, secure, limit).unwrap_or(Reply::Unreachable)
}

fn fetch_inner(host: &str, port: u16, path: &str, secure: bool, limit: Duration) -> Option<Reply> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;

    struct Handle(*mut core::ffi::c_void);
    impl Drop for Handle {
        fn drop(&mut self) { unsafe { let _ = WinHttpCloseHandle(self.0); } }
    }
    let open = |handle: *mut core::ffi::c_void| (!handle.is_null()).then_some(Handle(handle));
    let agent = HSTRING::from(format!("HaniWord/{}", shared::APP_VERSION));
    let (host, path) = (HSTRING::from(host), HSTRING::from(path));
    let started = Instant::now();
    unsafe {
        let session = open(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0))?;
        WinHttpSetTimeouts(session.0, 15_000, 15_000, 15_000, 15_000).ok()?;
        let connect = open(WinHttpConnect(session.0, &host, port, 0))?;
        let flags = if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let request = open(WinHttpOpenRequest(connect.0, w!("GET"), &path, PCWSTR::null(), PCWSTR::null(),
            std::ptr::null(), flags))?;
        WinHttpSetOption(Some(request.0), WINHTTP_OPTION_DISABLE_FEATURE, Some(&WINHTTP_DISABLE_COOKIES.to_ne_bytes())).ok()?;
        let headers: Vec<u16> = "Accept: application/vnd.github+json\r\n".encode_utf16().collect();
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).ok()?;
        let mut status = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut core::ffi::c_void), &mut size, std::ptr::null_mut()).ok()?;
        let mut body = Vec::new();
        let mut buffer = [0u8; 16 * 1024];
        loop {
            // 届いている分だけを読む (同期の WinHttpReadData は頼んだ量がそろうまで待つので、合計の時間を見られない)
            if started.elapsed() > limit { return None; }
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available).ok()?;
            if available == 0 { break; }
            let mut read = 0u32;
            WinHttpReadData(request.0, buffer.as_mut_ptr() as *mut core::ffi::c_void, available.min(buffer.len() as u32), &mut read).ok()?;
            if read == 0 { break; }
            body.extend_from_slice(&buffer[..read as usize]);
            if body.len() > MAX_BODY { return Some(Reply::Status(status, Vec::new())); }
        }
        Some(Reply::Status(status, body))
    }
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// 通知の中身。押すとページを開く (protocol の起動なので、launcher は待たなくてよい)
fn toast_xml(notice: &Notice, test: bool) -> String {
    let title = if test { format!("HaniWord の通知の試験です ({})", notice.version) } else { format!("HaniWord の新しい版があります ({})", notice.version) };
    format!(r#"<toast activationType="protocol" launch="{}"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        escape_xml(&notice.url), escape_xml(&title), escape_xml("押すとリリースのページを開きます。自動では入れ替えません"))
}

/// 通知の差出人の名前を登録する (パッケージでないアプリの通知には要る。HKCU の自分の分だけ)
fn register_app_id(app_id: &str) -> windows::core::Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::*;
    let key_path = HSTRING::from(format!(r"Software\Classes\AppUserModelId\{app_id}"));
    let name: Vec<u16> = "HaniWord".encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(name.as_ptr() as *const u8, name.len() * 2) };
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(HKEY_CURRENT_USER, &key_path, 0, windows::core::PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None).ok()?;
        let result = RegSetValueExW(key, windows::core::w!("DisplayName"), 0, REG_SZ, Some(bytes)).ok();
        let _ = RegCloseKey(key);
        result
    }
}

fn show(notice: &Notice) -> windows::core::Result<()> {
    show_with(notice, APP_ID, false, false).map(|_| ())
}

/// test: 試験の文で、画面には出さず通知センターにだけ入れる (SuppressPopup)。入ったか (通知の履歴にあるか) を返し、履歴からは消す
fn show_with(notice: &Notice, app_id: &str, test: bool, quiet: bool) -> windows::core::Result<bool> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    unsafe { let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED); }
    register_app_id(app_id)?;
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(toast_xml(notice, test)))?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    let (tag, group) = (HSTRING::from("update"), HSTRING::from("haniword"));
    toast.SetTag(&tag)?;
    toast.SetGroup(&group)?;
    if quiet { toast.SetSuppressPopup(true)?; }
    let id = HSTRING::from(app_id);
    ToastNotificationManager::CreateToastNotifierWithId(&id)?.Show(&toast)?;
    if !test { return Ok(true); }
    std::thread::sleep(Duration::from_millis(500));
    let history = ToastNotificationManager::History()?;
    let shown = history.GetHistoryWithId(&id)?.Size()? > 0;
    let _ = history.RemoveGroupedTagWithId(&tag, &group, &id);
    Ok(shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "supertastybanana3/HaniWord";
    const NOW: u64 = 1_761_700_000;
    const CURRENT: Version = Version(0, 1, 0);

    fn release(tag: &str) -> Reply {
        Reply::Status(200, serde_json::to_vec(&serde_json::json!({
            "tag_name": tag, "html_url": format!("https://github.com/{REPO}/releases/tag/{tag}"), "draft": false, "prerelease": false,
        })).unwrap())
    }

    /// 試験ごとの %APPDATA%\HaniWord の代わり
    struct Folder(PathBuf);
    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("haniword_update_{name}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Folder(path)
        }
        fn settings(&self, text: &str) { std::fs::write(self.0.join("settings.json"), text).unwrap(); }
    }
    impl Drop for Folder {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn versions_ignore_the_v_and_build_metadata_and_skip_prereleases() {
        assert_eq!(parse_version("v0.1.1"), Some(Version(0, 1, 1)));
        assert_eq!(parse_version("V1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(parse_version("0.2"), Some(Version(0, 2, 0)));
        assert_eq!(parse_version("1"), Some(Version(1, 0, 0)));
        assert_eq!(parse_version(" v0.1.10+build.7 "), Some(Version(0, 1, 10)));
        for tag in ["v0.2.0-rc.1", "0.2.0-beta", "v1.0.0-alpha+5", "", "v", "0.1.x", "0..1", "1.2.3.4", "v-1.0", "１.0.0", "9999999999.0.0",
            "v0.1.1/../x", "0.1.1/"] {
            assert_eq!(parse_version(tag), None, "{tag:?}");
        }
        // 数として比べる (0.1.10 は 0.1.9 より新しい)
        assert!(parse_version("0.1.10").unwrap() > parse_version("v0.1.9").unwrap());
        assert!(parse_version("0.2.0").unwrap() > parse_version("0.1.99").unwrap());
        assert!(parse_version("v0.1.1").unwrap() == parse_version(shared::APP_VERSION).unwrap());
    }

    #[test]
    fn checks_once_a_day_and_again_an_hour_after_no_connection() {
        let mut state = State::default();
        let mut asked = 0;
        assert!(check(&mut state, NOW, CURRENT, REPO, || { asked += 1; release("v0.1.0") }).is_none());
        assert_eq!((asked, state.next_check), (1, NOW + DAY));
        // 1 日たつまでは問い合わせない
        assert!(check(&mut state, NOW + DAY - 1, CURRENT, REPO, || { asked += 1; release("v0.1.1") }).is_none());
        assert_eq!(asked, 1);
        let notice = check(&mut state, NOW + DAY, CURRENT, REPO, || { asked += 1; release("v0.1.1") }).unwrap();
        assert_eq!((asked, notice.version), (2, Version(0, 1, 1)));
        assert_eq!(notice.url, format!("https://github.com/{REPO}/releases/tag/v0.1.1"));
        // つながらなければ 1 時間後にまた試す (起動の直後にたまたまオフラインだったとき)
        let mut offline = State::default();
        assert!(check(&mut offline, NOW, CURRENT, REPO, || Reply::Unreachable).is_none());
        assert_eq!(offline.next_check, NOW + HOUR);
        assert!(check(&mut offline, NOW + HOUR - 1, CURRENT, REPO, || panic!("asked within the hour")).is_none());
        assert!(check(&mut offline, NOW + HOUR, CURRENT, REPO, || release("v0.1.1")).is_some());
        // 時計が 1 日より前に戻ったら確かめ直す
        assert!(due(&State { next_check: NOW + DAY + 10, ..Default::default() }, NOW));
        assert!(!due(&State { next_check: NOW + DAY, ..Default::default() }, NOW));
    }

    #[test]
    fn answered_failures_are_silent_and_wait_a_day() {
        let failures: Vec<Reply> = vec![
            Reply::Status(403, br#"{"message":"API rate limit exceeded"}"#.to_vec()),
            Reply::Status(404, br#"{"message":"Not Found"}"#.to_vec()),         // まだリリースが無い
            Reply::Status(503, Vec::new()),                                     // GitHub が落ちている
            Reply::Status(200, b"<html>rate limited</html>".to_vec()),
            Reply::Status(200, Vec::new()),                                     // 本文が上限を超えた
            Reply::Status(200, br#"{"tag_name": 5}"#.to_vec()),
            Reply::Status(200, br#"{"tag_name": "v0.2.0", "prerelease": true}"#.to_vec()),
            Reply::Status(200, br#"{"tag_name": "v0.2.0", "draft": true}"#.to_vec()),
            Reply::Status(200, br#"{"tag_name": "v0.2.0-rc.1"}"#.to_vec()),
        ];
        for (index, failure) in failures.into_iter().enumerate() {
            let mut state = State::default();
            assert!(check(&mut state, NOW, CURRENT, REPO, || failure).is_none(), "{index}");
            assert_eq!(state.next_check, NOW + DAY, "{index}");
            assert!(state.notified.is_empty(), "{index}");
            assert!(check(&mut state, NOW + HOUR, CURRENT, REPO, || panic!("asked again")).is_none());
        }
    }

    #[test]
    fn the_notice_opens_a_page_made_from_the_tag() {
        // 返事の html_url は使わない (別のリポジトリ・.. でたどる先・%2e%2e)
        for url in ["https://example.com/releases/tag/v0.1.1", "https://github.com/someone/HaniWord/releases/tag/v0.1.1",
            "https://github.com/supertastybanana3/HaniWord/releases/../../someone/x/releases/download/v1/setup.exe",
            "https://github.com/supertastybanana3/HaniWord/releases/%2e%2e/%2e%2e/someone/x"] {
            let body = serde_json::to_vec(&serde_json::json!({"tag_name": "v0.1.1", "html_url": url})).unwrap();
            let notice = check(&mut State::default(), NOW, CURRENT, REPO, || Reply::Status(200, body)).unwrap();
            assert_eq!(notice.url, format!("https://github.com/{REPO}/releases/tag/v0.1.1"), "{url}");
        }
        // tag に数字・. ・頭の v 以外があれば、最新のリリースのページ
        for tag in ["0.1.1+build.7", " v0.1.1"] {
            let notice = check(&mut State::default(), NOW, CURRENT, REPO, || release(tag)).unwrap();
            assert_eq!(notice.url, format!("https://github.com/{REPO}/releases/latest"), "{tag:?}");
        }
        assert_eq!(check(&mut State::default(), NOW, CURRENT, REPO, || release("0.1.1")).unwrap().url,
            format!("https://github.com/{REPO}/releases/tag/0.1.1"));
        // 通知の XML は字をエスケープする
        let xml = toast_xml(&Notice { version: Version(0, 1, 1), url: "https://github.com/a/b/releases/tag/x?a=1&b=2".into() }, false);
        assert!(xml.contains("launch=\"https://github.com/a/b/releases/tag/x?a=1&amp;b=2\""));
        assert!(xml.contains("HaniWord の新しい版があります (0.1.1)"));
    }

    #[test]
    fn the_setting_is_off_when_the_file_cannot_be_read() {
        let folder = Folder::new("settings");
        let path = folder.0.join("settings.json");
        assert!(notifications_enabled(&path), "no settings yet: the default (on)");
        for (text, expected) in [
            (r#"{"version":"0.1.0","input":{}}"#, true),                    // 前の版の設定 (項目が無い)
            (r#"{"notify_updates": true}"#, true),
            (r#"{"notify_updates": false}"#, false),
            (r#"{"notify_updates": "yes"}"#, false),                        // 型が違う
            (r#"{"notify_updates": false"#, false),                         // 壊れている
            ("", false),
            ("[]", false),
        ] {
            folder.settings(text);
            assert_eq!(notifications_enabled(&path), expected, "{text:?}");
        }
        // 読めない (ファイルの代わりにフォルダ)
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(!notifications_enabled(&path));
        // オフなら問い合わせない
        let mut updater = Updater::new(&folder.0);
        updater.tick(NOW, CURRENT, REPO, || panic!("asked while the setting could not be read"), |_| panic!("shown"));
        std::fs::remove_dir(&path).unwrap();
        folder.settings(r#"{"notify_updates": false}"#);
        updater.tick(NOW, CURRENT, REPO, || panic!("asked while off"), |_| panic!("shown"));
        assert_eq!(updater.state, State::default());
    }

    #[test]
    fn the_same_version_is_announced_once_across_restarts() {
        let folder = Folder::new("restart");
        let mut shown = vec![];
        let mut updater = Updater::new(&folder.0);
        updater.tick(NOW, CURRENT, REPO, || release("v0.1.1"), |notice| { shown.push(notice.version); true });
        assert_eq!(shown, [Version(0, 1, 1)]);
        // launcher を起動し直しても (ファイルから読む) 同じ版は知らせない・次の日まで問い合わせない
        let mut restarted = Updater::new(&folder.0);
        assert_eq!(restarted.state, updater.state);
        assert_eq!(restarted.state, State { next_check: NOW + DAY, notified: "0.1.1".into() });
        restarted.tick(NOW + HOUR, CURRENT, REPO, || panic!("asked within the day"), |_| panic!("shown"));
        restarted.tick(NOW + DAY, CURRENT, REPO, || release("v0.1.1"), |_| panic!("shown twice"));
        restarted.tick(NOW + 2 * DAY, CURRENT, REPO, || release("0.1.1+rebuild"), |_| panic!("shown twice"));
        // もっと新しい版はまた知らせる
        restarted.tick(NOW + 3 * DAY, CURRENT, REPO, || release("v0.2.0"), |notice| { shown.push(notice.version); true });
        assert_eq!(shown, [Version(0, 1, 1), Version(0, 2, 0)]);
        // 入っている版と同じ・古い版は知らせない
        let other = Folder::new("current");
        let mut current = Updater::new(&other.0);
        current.tick(NOW, Version(0, 2, 0), REPO, || release("v0.2.0"), |_| panic!("the installed version"));
        current.tick(NOW + DAY, Version(0, 2, 0), REPO, || release("v0.1.9"), |_| panic!("an older version"));
        assert!(current.state.notified.is_empty());
    }

    #[test]
    fn a_notice_that_could_not_be_shown_is_tried_again_the_next_day() {
        let folder = Folder::new("show");
        let mut updater = Updater::new(&folder.0);
        updater.tick(NOW, CURRENT, REPO, || release("v0.1.1"), |_| false);
        assert_eq!(updater.state, State { next_check: NOW + DAY, notified: String::new() });
        // ファイルにも「知らせた」とは残らない (通知を出してから保存する)
        assert_eq!(State::load(&folder.0.join(STATE_FILE)), updater.state);
        let mut shown = 0;
        updater.tick(NOW + DAY, CURRENT, REPO, || release("v0.1.1"), |_| { shown += 1; true });
        assert_eq!((shown, updater.state.notified.as_str()), (1, "0.1.1"));
    }

    #[test]
    fn an_unsaved_state_still_waits_a_day() {
        let folder = Folder::new("unsaved");
        // 状態のファイルを書けない (同じ名前のフォルダがある)
        std::fs::create_dir(folder.0.join(STATE_FILE)).unwrap();
        let mut updater = Updater::new(&folder.0);
        let mut asked = 0;
        let mut shown = 0;
        for hour in 0..30 {
            updater.tick(NOW + hour * HOUR, CURRENT, REPO, || { asked += 1; release("v0.1.1") }, |_| { shown += 1; true });
        }
        // 毎時ではなく 1 日 1 回 (30 時間で 2 回)、知らせるのは 1 回
        assert_eq!((asked, shown), (2, 1));
    }

    /// 手元だけの偽のサーバー (127.0.0.1) に WinHTTP で問い合わせる。送るのは GET と User-Agent・Accept だけ (クッキー・ほかの情報は無い)。
    /// 状態コードと本文を読み、大きすぎる本文は捨て、少しずつしか送られなければ打ち切る
    #[test]
    fn fetch_sends_only_a_plain_get() {
        use std::io::{Read, Write};
        let serve = |status: &'static str, body: Vec<u8>, pause: Duration| {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = stream.read(&mut buffer).unwrap();
                    if n == 0 { break; }
                    request.extend_from_slice(&buffer[..n]);
                }
                let header = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(header.as_bytes());
                if pause.is_zero() {
                    let _ = stream.write_all(&body);
                } else {
                    for byte in &body {
                        if stream.write_all(&[*byte]).and_then(|_| stream.flush()).is_err() { break; }
                        std::thread::sleep(pause);
                    }
                }
                String::from_utf8(request).unwrap()
            });
            (port, server)
        };
        let limit = Duration::from_secs(60);
        let body = br#"{"tag_name":"v0.1.1"}"#.to_vec();
        let (port, server) = serve("200 OK", body.clone(), Duration::ZERO);
        let got = fetch_from("127.0.0.1", port, &format!("/repos/{REPO}/releases/latest"), false, limit);
        let request = server.join().unwrap();
        assert_eq!(got, Reply::Status(200, body));
        let lines: Vec<&str> = request.split("\r\n").filter(|line| !line.is_empty()).collect();
        assert_eq!(lines[0], format!("GET /repos/{REPO}/releases/latest HTTP/1.1"));
        let mut names: Vec<String> = lines[1..].iter().map(|line| line.split(':').next().unwrap().to_ascii_lowercase()).collect();
        names.sort();
        assert_eq!(names, ["accept", "connection", "host", "user-agent"], "{request}");
        assert!(lines.contains(&format!("User-Agent: HaniWord/{}", shared::APP_VERSION).as_str()), "{request}");
        assert!(lines.contains(&"Accept: application/vnd.github+json"), "{request}");
        // 回数制限 (403) も状態コードごと返す (check が黙って次の日に)
        let (port, server) = serve("403 Forbidden", br#"{"message":"API rate limit exceeded"}"#.to_vec(), Duration::ZERO);
        assert!(matches!(fetch_from("127.0.0.1", port, "/x", false, limit), Reply::Status(403, _)));
        server.join().unwrap();
        // 大きすぎる本文は捨てる (返事はあったので次の日に)
        let (port, server) = serve("200 OK", vec![b' '; MAX_BODY + 10], Duration::ZERO);
        assert_eq!(fetch_from("127.0.0.1", port, "/x", false, limit), Reply::Status(200, Vec::new()));
        let _ = server.join();
        // 1 バイトずつしか送られなければ、受け取りの合計の上限で打ち切る (上限 1 秒の試験)
        let (port, server) = serve("200 OK", vec![b' '; 40], Duration::from_millis(100));
        let started = Instant::now();
        assert_eq!(fetch_from("127.0.0.1", port, "/x", false, Duration::from_secs(1)), Reply::Unreachable);
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
        let _ = server.join();
        // つながらない (誰も受けていないポート)
        let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert_eq!(fetch_from("127.0.0.1", unused, "/x", false, limit), Reply::Unreachable);
    }

    /// 確かめ用 (AZK_TOAST_PROBE を指定したときだけ): 試験の差出人で、画面には出さず通知センターにだけ通知を入れ、入ったかを見て消す。
    /// 差出人の登録 (HKCU\Software\Classes\AppUserModelId\HaniWord.UpdateTest) も最後に消す
    #[test]
    fn toast_probe() {
        if std::env::var_os("AZK_TOAST_PROBE").is_none() { return; }
        let app_id = "HaniWord.UpdateTest";
        let notice = Notice { version: Version(0, 0, 0), url: format!("https://github.com/{REPO}/releases") };
        let result = show_with(&notice, app_id, true, true);
        unsafe {
            let _ = windows::Win32::System::Registry::RegDeleteTreeW(windows::Win32::System::Registry::HKEY_CURRENT_USER,
                &windows::core::HSTRING::from(format!(r"Software\Classes\AppUserModelId\{app_id}")));
        }
        assert_eq!(result.map_err(|error| format!("{error:?}")), Ok(true));
    }
}
