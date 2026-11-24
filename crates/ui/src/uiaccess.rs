// 候補ウィンドウを、管理者として動くアプリやスタートメニューの上にも出すための UIAccess。
//
// 署名つきのアプリでないと UIAccess は manifest では得られないので、次の手順で自分を起動し直す
// (Win32 の公開されている API の組み合わせ。2026-10-11 に書き直した独自の実装):
//   1. 自分がすでに UIAccess を持っていれば何もしない
//   2. 同じセッションで SYSTEM として動く winlogon.exe のトークンを借りて (要: 管理者権限)、
//      このスレッドだけ SeTcbPrivilege を持つ状態にする
//   3. 自分のトークンを複製し、TokenUIAccess を 1 にする (SeTcbPrivilege が要る)
//   4. そのトークンで自分自身を同じコマンドラインで起動し直し、今のプロセスは終わる
// どこかで失敗したら Err を返し、呼び出し側は UIAccess なしで続ける。

use anyhow::{bail, Context, Result};
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{
            DuplicateTokenEx, GetTokenInformation, SecurityImpersonation, SetTokenInformation, TokenImpersonation,
            TokenPrimary, TokenSessionId, TokenUIAccess, TOKEN_ACCESS_MASK, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY,
            TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY,
        },
        System::{
            Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS},
            Environment::GetCommandLineW,
            Threading::{
                CreateProcessAsUserW, ExitProcess, GetCurrentProcess, GetStartupInfoW, OpenProcess, OpenProcessToken,
                SetThreadToken, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION,
                PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
            },
        },
    },
};

/// 閉じ忘れないためのハンドルの包み
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn token_u32(token: HANDLE, class: windows::Win32::Security::TOKEN_INFORMATION_CLASS) -> Result<u32> {
    let mut value: u32 = 0;
    let mut returned: u32 = 0;
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(&mut value as *mut u32 as *mut _),
            std::mem::size_of::<u32>() as u32,
            &mut returned,
        )?;
    }
    Ok(value)
}

fn own_token(access: TOKEN_ACCESS_MASK) -> Result<Owned> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token)? };
    Ok(Owned(token))
}

/// 自分と同じセッションで動く winlogon.exe のトークン (複製用)
fn winlogon_token_in_my_session() -> Result<Owned> {
    let my_session = {
        let token = own_token(TOKEN_QUERY)?;
        token_u32(token.0, TokenSessionId)?
    };
    let snapshot = Owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? });
    let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_ok();
    while more {
        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
        if name.eq_ignore_ascii_case("winlogon.exe") {
            if let Ok(process) = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, entry.th32ProcessID) } {
                let process = Owned(process);
                let mut token = HANDLE::default();
                if unsafe { OpenProcessToken(process.0, TOKEN_QUERY | TOKEN_DUPLICATE, &mut token) }.is_ok() {
                    let token = Owned(token);
                    if token_u32(token.0, TokenSessionId).ok() == Some(my_session) {
                        return Ok(token);
                    }
                }
            }
        }
        more = unsafe { Process32NextW(snapshot.0, &mut entry) }.is_ok();
    }
    bail!("winlogon.exe not found in session {my_session}")
}

/// winlogon のトークンを複製して、このスレッドに当てる (SeTcbPrivilege を一時的に得る)
fn impersonate_winlogon() -> Result<Owned> {
    let system_token = winlogon_token_in_my_session()?;
    let mut impersonation = HANDLE::default();
    unsafe {
        DuplicateTokenEx(
            system_token.0,
            TOKEN_IMPERSONATE | TOKEN_QUERY,
            None,
            SecurityImpersonation,
            TokenImpersonation,
            &mut impersonation,
        )?;
        SetThreadToken(None, impersonation).context("SetThreadToken")?;
    }
    Ok(Owned(impersonation))
}

/// UIAccess を付けた主トークンを作る。土台は、ふだんのユーザーの権限 (エクスプローラーと同じトークン。管理者権限を持たない) が
/// 取れればそれ、取れなければ自分のトークン。2 つめの値は、ふだんのユーザーの権限を土台にしたか
fn token_with_uiaccess() -> Result<(Owned, bool)> {
    // ログオン直後はエクスプローラーがまだ無いことがある。管理者のまま動き続けないよう、少し待つ
    let mut desktop_token = shared::winsec::desktop_user_token();
    for _ in 0..60 {
        if desktop_token.is_some() || !shared::winsec::is_elevated() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
        desktop_token = shared::winsec::desktop_user_token();
    }
    let (copy, desktop) = match desktop_token {
        Some(token) => {
            let handle = token.0;
            std::mem::forget(token);
            (Owned(handle), true)
        }
        None => {
            let mine = own_token(TOKEN_DUPLICATE | TOKEN_QUERY)?;
            let mut copy = HANDLE::default();
            unsafe {
                DuplicateTokenEx(
                    mine.0,
                    TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
                    None,
                    SecurityImpersonation,
                    TokenPrimary,
                    &mut copy,
                )?;
            }
            (Owned(copy), false)
        }
    };
    let on: u32 = 1;
    unsafe {
        SetTokenInformation(copy.0, TokenUIAccess, &on as *const u32 as *const _, std::mem::size_of::<u32>() as u32)
            .context("SetTokenInformation(TokenUIAccess)")?;
    }
    Ok((copy, desktop))
}

fn relaunch_with(token: HANDLE) -> Result<()> {
    let mut startup = STARTUPINFOW::default();
    unsafe { GetStartupInfoW(&mut startup) };
    let mut info = PROCESS_INFORMATION::default();
    // CreateProcessAsUserW はコマンドラインの文字列を書き換えることがあるので、書ける場所に写す
    let mut command_line: Vec<u16> = unsafe {
        let p = GetCommandLineW();
        let mut len = 0;
        while *p.0.add(len) != 0 {
            len += 1;
        }
        std::slice::from_raw_parts(p.0, len).to_vec()
    };
    command_line.push(0);
    unsafe {
        CreateProcessAsUserW(
            token,
            None,
            PWSTR(command_line.as_mut_ptr()),
            None,
            None,
            false,
            PROCESS_CREATION_FLAGS(0),
            None,
            None,
            &startup,
            &mut info,
        )
        .context("CreateProcessAsUserW")?;
        let _ = CloseHandle(info.hThread);
        let _ = CloseHandle(info.hProcess);
    }
    Ok(())
}

/// UIAccess の状態を一時フォルダに書く (動作確認用。haniword_uiaccess.txt)
pub fn note_status(status: &str) {
    // 管理者権限のときは、ユーザーが書き換えられる一時フォルダに書かない (置かれたリンクの先を書き換えてしまうため)
    if shared::winsec::is_elevated() {
        return;
    }
    let _ = std::fs::write(std::env::temp_dir().join("haniword_uiaccess.txt"), status);
}

/// UIAccess を持っていなければ、UIAccess を付けて起動し直す (成功したら戻らない)
pub fn prepare_uiaccess_token() -> Result<()> {
    {
        let token = own_token(TOKEN_QUERY)?;
        if token_u32(token.0, TokenUIAccess)? != 0 {
            note_status(if shared::winsec::is_elevated() { "on (elevated)" } else { "on (not elevated)" });
            return Ok(());
        }
    }
    let impersonation = impersonate_winlogon()?;
    let result = token_with_uiaccess();
    // 借りたトークンはすぐ外す (以後の処理はふつうの権限で)
    if unsafe { SetThreadToken(None, HANDLE::default()) }.is_err() { std::process::abort(); }
    drop(impersonation);
    let (token, desktop) = result?;
    if desktop {
        // ふだんのユーザーの権限で起動し直す (管理者のまま動き続けない)
        let exe = std::env::current_exe()?;
        let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        shared::winsec::spawn_with_token(token.0, &exe, "", &dir).context("CreateProcessWithTokenW")?;
    } else {
        relaunch_with(token.0)?;
    }
    unsafe { ExitProcess(0) }
}
