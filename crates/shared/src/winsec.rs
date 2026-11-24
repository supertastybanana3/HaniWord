// 権限まわりの小さな道具 (サーバー・候補窓・起動用のプログラムで共通)
//   ・通信路 (名前付きパイプ) の名前を利用者 (セッション) ごとに分ける
//   ・通信路を開けるのを、自分・隔離されたアプリ・システムだけにする
//   ・管理者として動いているときに、ふだんのユーザーの権限 (エクスプローラーと同じ) で別のプログラムを起動する
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL},
        Security::{
            Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW, GetSecurityInfo, SetEntriesInAclW, SetSecurityInfo,
                EXPLICIT_ACCESS_W, TRUSTEE_W, SET_ACCESS, TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP, SE_KERNEL_OBJECT},
            ACL, PSID, PSECURITY_DESCRIPTOR, DACL_SECURITY_INFORMATION, ACE_FLAGS,
            DuplicateTokenEx, GetTokenInformation, SecurityImpersonation,
            GetSidSubAuthority, GetSidSubAuthorityCount, RevertToSelf,
            TokenElevation, TokenRestrictedSids, TokenIntegrityLevel, TokenIsAppContainer,
            TokenPrimary, TokenSessionId, TokenUser, TOKEN_GROUPS, TOKEN_MANDATORY_LABEL, TOKEN_ACCESS_MASK, TOKEN_ADJUST_DEFAULT,
            TOKEN_ADJUST_SESSIONID, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
        },
        System::{
            Pipes::{ImpersonateNamedPipeClient, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, GetNamedPipeServerSessionId},
            Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS},
            Threading::{
                CreateProcessWithTokenW, GetCurrentProcess, GetCurrentThread, OpenProcess, OpenProcessToken, OpenThreadToken, CREATE_NO_WINDOW,
                CREATE_PROCESS_LOGON_FLAGS, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
                QueryFullProcessImageNameW, PROCESS_NAME_WIN32, GetCurrentProcessId, PROCESS_ACCESS_RIGHTS,
            },
        },
        Storage::FileSystem::{CreateFileW, FILE_SHARE_MODE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
            SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT},
    },
};
use std::os::windows::io::{FromRawHandle, AsRawHandle};

/// 閉じ忘れないためのハンドルの包み
pub struct OwnedHandle(pub HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn own_token(access: TOKEN_ACCESS_MASK) -> Option<OwnedHandle> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token).ok()? };
    Some(OwnedHandle(token))
}

fn effective_token() -> Option<OwnedHandle> {
    let mut raw = HANDLE::default();
    match unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut raw) } {
        Ok(()) => Some(OwnedHandle(raw)),
        Err(error) if error.code() == windows::core::HRESULT::from_win32(1008) => own_token(TOKEN_QUERY),
        Err(_) => None,
    }
}

#[derive(Debug)]
struct Identity {
    sid: String,
    session: u32,
    container: bool,
    restricted: bool,
    integrity: u32,
    elevated: bool,
}

fn identity(token: HANDLE) -> Option<Identity> {
    let groups = token_buffer(token, TokenRestrictedSids)?;
    let label = token_buffer(token, TokenIntegrityLevel)?;
    let integrity = unsafe {
        let sid = (*(label.as_ptr() as *const TOKEN_MANDATORY_LABEL)).Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 { return None; }
        *GetSidSubAuthority(sid, u32::from(count - 1))
    };
    Some(Identity {
        sid: token_sid(token)?, session: token_u32(token, TokenSessionId)?,
        container: token_u32(token, TokenIsAppContainer)? != 0,
        restricted: unsafe { (*(groups.as_ptr() as *const TOKEN_GROUPS)).GroupCount != 0 },
        integrity, elevated: token_u32(token, TokenElevation)? != 0,
    })
}

fn private_identity(identity: &Identity) -> bool {
    !identity.container && !identity.restricted && identity.integrity >= 0x2000 && !identity.elevated
}

/// Private history/profile files are never accessed from a sandbox or elevated host.
pub fn private_data_allowed() -> bool {
    effective_token().and_then(|token| identity(token.0)).is_some_and(|id| private_identity(&id))
}

// Specific pipe rights: no FILE_CREATE_PIPE_INSTANCE, WRITE_DAC or WRITE_OWNER.
pub const PIPE_CLIENT_ACCESS: u32 = 0x0012_0183;

struct LocalAllocation(HLOCAL);
impl Drop for LocalAllocation {
    fn drop(&mut self) { unsafe { let _ = LocalFree(self.0); } }
}

fn grant_identity_query(handle: HANDLE, access: u32) -> windows::core::Result<()> {
    let mut old_acl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe { GetSecurityInfo(handle, SE_KERNEL_OBJECT, DACL_SECURITY_INFORMATION, None, None,
        Some(&mut old_acl), None, Some(&mut descriptor)).ok()?; }
    let _descriptor = LocalAllocation(HLOCAL(descriptor.0));
    if old_acl.is_null() { return Err(windows::core::Error::from_win32()); }
    let mut sids = Vec::new();
    let mut entries = Vec::new();
    for text in ["S-1-15-2-1", "S-1-15-2-2", "S-1-5-12"] {
        let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let mut sid = PSID::default();
        unsafe { ConvertStringSidToSidW(PCWSTR(text.as_ptr()), &mut sid)?; }
        sids.push(LocalAllocation(HLOCAL(sid.0)));
        entries.push(EXPLICIT_ACCESS_W {
            grfAccessPermissions: access, grfAccessMode: SET_ACCESS, grfInheritance: ACE_FLAGS(0),
            Trustee: TRUSTEE_W { TrusteeForm: TRUSTEE_IS_SID, TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
                ptstrName: PWSTR(sid.0.cast()), ..Default::default() },
        });
    }
    let mut acl: *mut ACL = std::ptr::null_mut();
    unsafe { SetEntriesInAclW(Some(&entries), Some(old_acl), &mut acl).ok()?; }
    let _acl = LocalAllocation(HLOCAL(acl.cast()));
    unsafe { SetSecurityInfo(handle, SE_KERNEL_OBJECT, DACL_SECURITY_INFORMATION, PSID::default(), PSID::default(), Some(acl), None).ok() }
}

/// Expose only identity metadata, never process memory or token duplication rights.
pub fn allow_pipe_identity_queries() -> windows::core::Result<()> {
    if !private_data_allowed() { return Err(windows::core::Error::new(windows::core::HRESULT::from_win32(5), "Server must be unelevated and unrestricted")); }
    let process = OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_ACCESS_RIGHTS(0x60000),
        false, GetCurrentProcessId())? });
    grant_identity_query(process.0, PROCESS_QUERY_LIMITED_INFORMATION.0)?;
    let token = own_token(TOKEN_QUERY | TOKEN_ACCESS_MASK(0x60000)).ok_or_else(windows::core::Error::from_win32)?;
    grant_identity_query(token.0, TOKEN_QUERY.0)
}

fn trusted_server(owner: &Identity, server: &Identity) -> bool {
    owner.sid == server.sid && owner.session == server.session && private_identity(server)
}

/// Validate the process owning this connected instance before sending any RPC data.
pub fn authenticate_pipe_server(pipe: HANDLE, expected_executable: &str) -> std::io::Result<()> {
    authenticate_pipe_server_in(pipe, expected_executable, None)
}

/// 2026-10-19 (security review #9): expected_root があれば、相手の実行ファイルがその下 (管理者しか書けない Program Files\HaniWord) に
/// あることも確かめる。同じ利用者のほかのプログラムが ui.exe という名前で通信路を先に取っても、管理者権限のアプリで打った文字を受け取れない
pub fn authenticate_pipe_server_in(pipe: HANDLE, expected_executable: &str, expected_root: Option<&std::path::Path>) -> std::io::Result<()> {
    let denied = || std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Untrusted named-pipe server");
    let owner = effective_token().and_then(|token| identity(token.0)).ok_or_else(denied)?;
    let mut pid = 0;
    let mut session = 0;
    unsafe {
        GetNamedPipeServerProcessId(pipe, &mut pid).map_err(|_| denied())?;
        GetNamedPipeServerSessionId(pipe, &mut session).map_err(|_| denied())?;
    }
    if pid == 0 || session != owner.session { return Err(denied()); }
    let process = OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(|_| denied())? });
    let mut raw = HANDLE::default();
    unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut raw).map_err(|_| denied())?; }
    let token = OwnedHandle(raw);
    let server = identity(token.0).ok_or_else(denied)?;
    if !trusted_server(&owner, &server) { return Err(denied()); }
    let mut image = vec![0u16; 32768];
    let mut length = image.len() as u32;
    unsafe { QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, PWSTR(image.as_mut_ptr()), &mut length).map_err(|_| denied())?; }
    let image = std::path::PathBuf::from(String::from_utf16_lossy(&image[..length as usize]));
    if !image.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(expected_executable)) {
        return Err(denied());
    }
    if let Some(root) = expected_root {
        if !image_under_root(&image, root) { return Err(denied()); }
    }
    let mut checked_pid = 0;
    unsafe { GetNamedPipeServerProcessId(pipe, &mut checked_pid).map_err(|_| denied())?; }
    if checked_pid != pid { return Err(denied()); }
    Ok(())
}

/// image が root の下にあるか (大文字小文字を区別しない。root そのものの名前で始まる別のフォルダ (HaniWord2) は含めない)
fn image_under_root(image: &std::path::Path, root: &std::path::Path) -> bool {
    let image = image.to_string_lossy().to_lowercase();
    let root = root.to_string_lossy().trim_end_matches('\\').to_lowercase();
    !root.is_empty() && !image.contains("\\..\\") && image.starts_with(&format!("{root}\\"))
}

/// Open with identification-only SQOS, transfer the owned overlapped handle to Tokio.
pub fn open_client_pipe(path: &str, expected_executable: &str) -> std::io::Result<std::os::windows::io::OwnedHandle> {
    open_client_pipe_in(path, expected_executable, None)
}

/// open_client_pipe と同じ。expected_root があれば、相手の実行ファイルの場所も確かめる (authenticate_pipe_server_in)
pub fn open_client_pipe_in(path: &str, expected_executable: &str, expected_root: Option<&std::path::Path>) -> std::io::Result<std::os::windows::io::OwnedHandle> {
    let name: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(PCWSTR(name.as_ptr()), PIPE_CLIENT_ACCESS, FILE_SHARE_MODE(0), None, OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT, HANDLE::default())
    }.map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xffff))?;
    let handle = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle.0) };
    authenticate_pipe_server_in(HANDLE(handle.as_raw_handle()), expected_executable, expected_root)?;
    Ok(handle)
}

fn token_u32(token: HANDLE, class: windows::Win32::Security::TOKEN_INFORMATION_CLASS) -> Option<u32> {
    let mut value: u32 = 0;
    let mut returned = 0;
    unsafe {
        GetTokenInformation(token, class, Some(&mut value as *mut u32 as *mut _), 4, &mut returned).ok()?;
    }
    Some(value)
}

/// このプロセスのセッション番号
pub fn session_id() -> u32 {
    own_token(TOKEN_QUERY).and_then(|t| token_u32(t.0, TokenSessionId)).unwrap_or(0)
}

/// 利用者 (セッション) ごとの通信路の名前 (\\.\pipe\<base>_s<セッション番号>)
pub fn pipe_name(base: &str) -> String {
    format!(r"\\.\pipe\{base}_s{}", session_id())
}

/// 以前の版の通信路の名前 (動いたままのアプリの中の古い IME がつなぎに来る。移行のあいだだけ待ち受ける)
pub fn legacy_pipe_name(base: &str) -> String {
    format!(r"\\.\pipe\{base}")
}

/// このプロセスの利用者の SID (S-1-5-21-…)
pub fn user_sid_string() -> Option<String> {
    let token = own_token(TOKEN_QUERY)?;
    token_sid(token.0)
}

fn token_buffer(token: HANDLE, class: windows::Win32::Security::TOKEN_INFORMATION_CLASS) -> Option<Vec<usize>> {
    let mut returned = 0;
    unsafe {
        let _ = GetTokenInformation(token, class, None, 0, &mut returned);
    }
    if returned == 0 || returned > 65536 { return None; }
    let mut buffer = vec![0usize; (returned as usize).div_ceil(std::mem::size_of::<usize>())];
    unsafe { GetTokenInformation(token, class, Some(buffer.as_mut_ptr() as *mut _), returned, &mut returned).ok()?; }
    Some(buffer)
}

fn token_sid(token: HANDLE) -> Option<String> {
    let buffer = token_buffer(token, TokenUser)?;
    unsafe {
        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).ok()?;
        let sid = text.to_string().ok();
        let _ = LocalFree(HLOCAL(text.0 as _));
        sid
    }
}

/// 通信路のアクセス権 (SDDL)。開けるのは 自分・隔離されたアプリ (AC: ストアアプリ・スタートメニューの検索欄など)・
/// 制限つきのトークンで動くもの (RC: ブラウザーのサンドボックスなど)・システム だけ。低い整合性レベルからも書ける (IME はそういうアプリの中でも動くため)
pub fn pipe_sddl() -> std::io::Result<String> {
    pipe_sddl_for_sid(user_sid_string())
}

fn pipe_sddl_for_sid(sid: Option<String>) -> std::io::Result<String> {
    let sid = sid.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Cannot identify the pipe owner"))?;
    Ok(format!("D:(A;;0x{PIPE_CLIENT_ACCESS:x};;;AC)(A;;0x{PIPE_CLIENT_ACCESS:x};;;RC)(A;;GA;;;SY)(A;;GA;;;{sid})S:(ML;;NW;;;LW)"))
}

#[derive(Debug, Clone)]
pub struct PipePeer {
    pub may_persist: bool,
    /// 2026-10-20 (Codex の再調査 M3): 変換に学習データ・プロフィールを使ってよいか (隔離されたアプリには使わない)
    pub may_read_personal: bool,
}

/// 2026-11-23 (点検 Astra 02): パイプの相手の処理の番号 (PID)。候補の窓 (ui.exe) が、表示を変える要求を「今の入力先」の処理からだけ受けるために使う
pub fn pipe_client_pid(pipe: HANDLE) -> Option<u32> {
    let mut pid = 0u32;
    unsafe { GetNamedPipeClientProcessId(pipe, &mut pid).ok()?; }
    (pid != 0).then_some(pid)
}

/// Identify the token of the bytes just read; never impersonate across an await.
pub fn authenticate_pipe_client(pipe: HANDLE) -> std::io::Result<PipePeer> {
    let denied = || std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Pipe client authentication failed");
    let owner = user_sid_string().ok_or_else(denied)?;
    let own = own_token(TOKEN_QUERY).ok_or_else(denied)?;
    let session = token_u32(own.0, TokenSessionId).ok_or_else(denied)?;
    unsafe { ImpersonateNamedPipeClient(pipe).map_err(|_| denied())?; }
    let result = (|| {
        let mut raw = HANDLE::default();
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut raw).map_err(|_| denied())?; }
        let token = OwnedHandle(raw);
        if token_sid(token.0).as_deref() != Some(owner.as_str()) || token_u32(token.0, TokenSessionId) != Some(session) { return Err(denied()); }
        let container = token_u32(token.0, TokenIsAppContainer).ok_or_else(denied)?;
        // UAC's ordinary desktop token also loses privileges. Only restricting SIDs
        // indicate this sandbox boundary; AppContainer/low integrity are checked separately.
        let restricted = token_buffer(token.0, TokenRestrictedSids).ok_or_else(denied)?;
        let restricted = unsafe { (*(restricted.as_ptr() as *const TOKEN_GROUPS)).GroupCount };
        let label = token_buffer(token.0, TokenIntegrityLevel).ok_or_else(denied)?;
        let level = unsafe {
            let sid = (*(label.as_ptr() as *const TOKEN_MANDATORY_LABEL)).Label.Sid;
            let count = *GetSidSubAuthorityCount(sid);
            if count == 0 { return Err(denied()); }
            *GetSidSubAuthority(sid, u32::from(count - 1))
        };
        Ok(PipePeer { may_persist: peer_may_persist(container, restricted, level), may_read_personal: peer_may_read_personal(container, restricted, level) })
    })();
    if unsafe { RevertToSelf() }.is_err() { std::process::abort(); }
    result
}

/// 2026-10-19 (security review #3, server side): learning and settings may be changed only by ordinary desktop clients:
/// no AppContainer, no restricting SIDs, and medium integrity. High integrity (an elevated administrator's app) is refused too,
/// so what is typed in elevated apps never reaches the learning files the ordinary user can read (the client also stops sending it)
fn peer_may_persist(container: u32, restricted: u32, level: u32) -> bool {
    container == 0 && restricted == 0 && (0x2000..0x3000).contains(&level)
}

/// 2026-10-20 (Codex の再調査 M3): 変換の結果に学習した語・プロフィールを使ってよい接続。AppContainer・制限トークン・低い整合性の
/// アプリ (サンドボックス) は、変換を問い合わせて学習した語を引き出せないように、学習を使わない変換にする。管理者のアプリには使ってよい
fn peer_may_read_personal(container: u32, restricted: u32, level: u32) -> bool {
    container == 0 && restricted == 0 && level >= 0x2000
}

/// DLL が見つからないなどで起動できなかったときに、Windows の「システム エラー」の窓を出さない (起動の失敗は終了コードで分かる)。
/// このモードは子のプロセス (変換サーバー) にも引き継がれる
pub fn suppress_error_dialogs() {
    use windows::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX};
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX); }
}

/// 2026-11-10 (Fable の諮問 perf 4): このプロセスが落ちたときに、Windows エラー報告 (WER) がヒープの情報を集めないようにする。
/// 変換サーバーのメモリ (ヒープ) には、打っている文・前後の文脈・学習が載っている。WerSetFlags は「現在のプロセスの WER の設定」で、
/// WER_FAULT_REPORTING_FLAG_NOHEAP は "Do not collect heap information in the event of an application crash or non-response."
/// (Microsoft Learn の werapi.h の WerSetFlags、2026-11-10 に読んだ)。落ちる前に効いていないと意味が無いので、起動の頭で呼ぶ
pub fn exclude_heap_from_error_reports() -> windows::core::Result<()> {
    use windows::Win32::System::ErrorReporting::{WerSetFlags, WER_FAULT_REPORTING_FLAG_NOHEAP};
    unsafe { WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP) }
}

/// 管理者として (昇格して) 動いているか
pub fn is_elevated() -> bool {
    elevation_status().unwrap_or(true)
}

pub fn elevation_status() -> windows::core::Result<bool> {
    let mut raw = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw)?; }
    let token = OwnedHandle(raw);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )?;
    }
    Ok(elevation.TokenIsElevated != 0)
}

#[cfg(test)]
mod tests {
    /// 2026-11-10: 起動の頭で呼ぶと、このプロセスの WER の設定に「ヒープの情報を集めない」が入る (WerGetFlags で読み戻す)
    #[test]
    fn error_reports_leave_out_the_heap() {
        use windows::Win32::System::ErrorReporting::{WerGetFlags, WER_FAULT_REPORTING_FLAG_NOHEAP};
        use windows::Win32::System::Threading::GetCurrentProcess;
        super::exclude_heap_from_error_reports().expect("WerSetFlags");
        let flags = unsafe { WerGetFlags(GetCurrentProcess()) }.expect("WerGetFlags");
        assert_ne!(flags.0 & WER_FAULT_REPORTING_FLAG_NOHEAP.0, 0, "flags = {:#x}", flags.0);
    }

    use super::*;
    #[test]
    fn pipe_servers_must_live_under_the_protected_root() {
        let root = std::path::Path::new(r"C:\Program Files\HaniWord");
        assert!(image_under_root(std::path::Path::new(r"C:\Program Files\HaniWord\Releases\0.1.0\ui.exe"), root));
        assert!(image_under_root(std::path::Path::new(r"c:\program files\haniword\ui.exe"), root));
        assert!(!image_under_root(std::path::Path::new(r"C:\Users\someone\AppData\Roaming\HaniWord\ui.exe"), root));
        assert!(!image_under_root(std::path::Path::new(r"C:\Program Files\HaniWord2\ui.exe"), root));
        assert!(!image_under_root(std::path::Path::new(r"C:\Program Files\HaniWord\..\Evil\ui.exe"), root));
    }

    #[test]
    fn sandboxed_clients_do_not_read_personal_learning() {
        assert!(peer_may_read_personal(0, 0, 0x2000));
        assert!(peer_may_read_personal(0, 0, 0x3000), "elevated apps of the same user may use the learning");
        assert!(!peer_may_read_personal(0, 0, 0x1000));
        assert!(!peer_may_read_personal(1, 0, 0x2000));
        assert!(!peer_may_read_personal(0, 2, 0x2000));
    }

    #[test]
    fn only_ordinary_desktop_clients_may_persist() {
        assert!(peer_may_persist(0, 0, 0x2000));
        assert!(peer_may_persist(0, 0, 0x2100));
        assert!(!peer_may_persist(0, 0, 0x3000), "elevated administrator apps must not write learning");
        assert!(!peer_may_persist(0, 0, 0x4000));
        assert!(!peer_may_persist(0, 0, 0x1000));
        assert!(!peer_may_persist(1, 0, 0x2000));
        assert!(!peer_may_persist(0, 2, 0x2000));
    }

    #[test]
    fn unknown_owner_does_not_widen_pipe_acl() {
        assert_eq!(pipe_sddl_for_sid(None).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        let sddl = pipe_sddl_for_sid(Some("S-1-5-21-1-2-3-1001".into())).unwrap();
        assert!(sddl.contains("S-1-5-21-1-2-3-1001"));
        assert!(!sddl.contains(";;;BU)"));
        assert!(!sddl.contains(";;;BA)"));
        assert!(!sddl.contains("GA;;;AC"));
        assert_eq!(PIPE_CLIENT_ACCESS & (0x4 | 0x40000 | 0x80000), 0);
    }
    #[test]
    fn desktop_identity_can_be_queried() {
        assert!(user_sid_string().unwrap().starts_with("S-1-"));
        assert!(elevation_status().is_ok());
    }

    #[test]
    fn private_data_policy_fails_closed_for_sandbox_and_elevated_tokens() {
        let mut id = Identity { sid: "owner".into(), session: 1, container: false, restricted: false,
            integrity: 0x2000, elevated: false };
        assert!(private_identity(&id));
        id.container = true; assert!(!private_identity(&id)); id.container = false;
        id.restricted = true; assert!(!private_identity(&id)); id.restricted = false;
        id.integrity = 0x1000; assert!(!private_identity(&id)); id.integrity = 0x2000;
        id.elevated = true; assert!(!private_identity(&id));
    }

    #[test]
    fn pipe_server_requires_owner_session_and_unrestricted_token() {
        let owner = Identity { sid: "owner".into(), session: 1, container: true, restricted: false,
            integrity: 0x1000, elevated: false };
        let mut server = Identity { sid: "owner".into(), session: 1, container: false, restricted: false,
            integrity: 0x2000, elevated: false };
        assert!(trusted_server(&owner, &server));
        server.sid = "other".into(); assert!(!trusted_server(&owner, &server)); server.sid = "owner".into();
        server.session = 2; assert!(!trusted_server(&owner, &server)); server.session = 1;
        server.container = true; assert!(!trusted_server(&owner, &server)); server.container = false;
        server.restricted = true; assert!(!trusted_server(&owner, &server)); server.restricted = false;
        server.elevated = true; assert!(!trusted_server(&owner, &server));
        assert!(authenticate_pipe_server(HANDLE::default(), "azookey-server.exe").is_err());
    }
}

/// 同じセッションで動くエクスプローラーのトークンを、新しいプロセス用に複製したもの (ふだんのユーザーの権限)
pub fn desktop_user_token() -> Option<OwnedHandle> {
    let my_session = session_id();
    let snapshot = OwnedHandle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()? });
    let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_ok();
    while more {
        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case("explorer.exe") {
            if let Ok(process) = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, entry.th32ProcessID) } {
                let process = OwnedHandle(process);
                let mut token = HANDLE::default();
                if unsafe { OpenProcessToken(process.0, TOKEN_QUERY | TOKEN_DUPLICATE, &mut token) }.is_ok() {
                    let token = OwnedHandle(token);
                    let not_elevated = {
                        let mut elevation = TOKEN_ELEVATION::default();
                        let mut returned = 0;
                        unsafe {
                            GetTokenInformation(token.0, TokenElevation, Some(&mut elevation as *mut _ as *mut _),
                                std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut returned).is_ok() && elevation.TokenIsElevated == 0
                        }
                    };
                    if token_u32(token.0, TokenSessionId) == Some(my_session) && not_elevated {
                        let mut primary = HANDLE::default();
                        unsafe {
                            DuplicateTokenEx(
                                token.0,
                                TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT | TOKEN_ADJUST_SESSIONID,
                                None,
                                SecurityImpersonation,
                                TokenPrimary,
                                &mut primary,
                            )
                            .ok()?;
                        }
                        return Some(OwnedHandle(primary));
                    }
                }
            }
        }
        more = unsafe { Process32NextW(snapshot.0, &mut entry) }.is_ok();
    }
    None
}

/// トークンを指定してプログラムを起動する (CreateProcessWithTokenW。管理者の SeImpersonatePrivilege で動く)
pub fn spawn_with_token(token: HANDLE, exe: &std::path::Path, args: &str, directory: &std::path::Path) -> windows::core::Result<()> {
    let exe_w: Vec<u16> = exe.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
    let mut command: Vec<u16> = format!("\"{}\" {}", exe.display(), args).encode_utf16().chain(Some(0)).collect();
    let dir_w: Vec<u16> = directory.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
    let startup = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessWithTokenW(
            token,
            CREATE_PROCESS_LOGON_FLAGS(0),
            PCWSTR(exe_w.as_ptr()),
            PWSTR(command.as_mut_ptr()),
            CREATE_NO_WINDOW,
            None,
            PCWSTR(dir_w.as_ptr()),
            &startup,
            &mut info,
        )?;
        let _ = CloseHandle(info.hThread);
        let _ = CloseHandle(info.hProcess);
    }
    Ok(())
}
