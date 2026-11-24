//! 2026-11-23 (公開前のセキュリティの点検 Astra 02): 候補の窓の持ち主の確かめ。
//!
//! 候補の窓 (ui.exe) へのパイプは、同じ利用者・同じセッションの処理なら、低い整合性や隔離されたアプリ (AppContainer) からも開ける
//! (どのアプリの中の IME からも使うため)。そのため、前面と関係のない処理が、表示中の候補を隠したり、別の字を出したりできた。
//! 候補の窓は UIAccess で管理者の窓の上にも出るので、管理者のアプリで打っている人に偽の候補を見せうる。
//!
//! ここでは、表示を変える要求を「今の入力先」の処理からの接続だけに通す。今の入力先は、要求のたびに ui.exe が自分で Win32 から確かめる
//! (アプリの名前では見ない)。次のどれかの窓の処理の番号が、パイプの相手の番号と同じなら持ち主とみる:
//!   1. 前面の窓 (GetForegroundWindow)
//!   2. 前面の窓のスレッドの GetGUIThreadInfo のフォーカス・活きている窓・カーソルの窓 (別の処理の子の窓に入力があるとき)
//!   3. 前面の窓の子の窓 (UWP。前面は ApplicationFrameHost.exe の枠で、入力は中のアプリの処理の CoreWindow が受ける)
//!
//! 隠す要求は、持ち主か、最後に窓を出した接続から通す (前面が移ったあとに、前のアプリが自分の候補を閉じられるように)。
//! 外れた要求は捨てる。呼んだ側には Ok を返す (前面が移る間際の正しい要求で、IME の側を崩さないため)。拒んだ数だけを数え、中身と番号はどこにも書かない。
use std::sync::{atomic::{AtomicU64, Ordering}, Mutex};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
};

/// 要求の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 窓を出す
    Show,
    /// 窓を隠す
    Hide,
    /// 候補・選んでいる所・位置・入力モードの札を変える
    Change,
}

/// パイプの相手 (接続の番号と、相手の処理の番号)。番号が取れなかった接続は None
pub type Peer = Option<(u64, u32)>;

pub struct Gate {
    owns: Box<dyn Fn(u32) -> bool + Send + Sync>,
    last_shown: Mutex<Option<u64>>,
    rejected: AtomicU64,
}

impl Gate {
    /// 本物の確かめ (Win32 の前面・フォーカスの窓) を使う
    pub fn new() -> Self {
        Self::with_check(Box::new(input_target_is))
    }

    /// 確かめ方を差し替える (試験用)
    pub fn with_check(owns: Box<dyn Fn(u32) -> bool + Send + Sync>) -> Self {
        Self { owns, last_shown: Mutex::new(None), rejected: AtomicU64::new(0) }
    }

    /// この要求を通すか。通さないときは拒んだ数を 1 つ足す
    pub fn allow(&self, peer: Peer, kind: Kind) -> bool {
        let allowed = match peer {
            None => false,
            Some((connection, pid)) => match kind {
                Kind::Show => {
                    let owner = (self.owns)(pid);
                    if owner { *self.last_shown.lock().unwrap_or_else(|e| e.into_inner()) = Some(connection); }
                    owner
                }
                Kind::Change => (self.owns)(pid),
                Kind::Hide => (self.owns)(pid) || *self.last_shown.lock().unwrap_or_else(|e| e.into_inner()) == Some(connection),
            },
        };
        if !allowed {
            let count = self.rejected.fetch_add(1, Ordering::Relaxed) + 1;
            // 数だけを、1・2・4・8… 回目に書く (誰から何が来たかは書かない)
            if count.is_power_of_two() {
                eprintln!("candidate window: refused {count} request(s) from a client that is not the current input target");
            }
        }
        allowed
    }

    /// 拒んだ数
    #[allow(dead_code)]
    pub fn rejected(&self) -> u64 {
        self.rejected.load(Ordering::Relaxed)
    }
}

fn window_pid(hwnd: HWND) -> u32 {
    if hwnd.0.is_null() {
        return 0;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

struct ChildSearch {
    pid: u32,
    found: bool,
}

unsafe extern "system" fn child_has_pid(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam.0 as *mut ChildSearch);
    if window_pid(hwnd) == search.pid {
        search.found = true;
        return BOOL(0);
    }
    BOOL(1)
}

/// 今の入力先の処理が `pid` か (上の 1〜3)
pub fn input_target_is(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            return false;
        }
        let mut foreground_pid = 0u32;
        let thread = GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        if foreground_pid == pid {
            return true;
        }
        if thread != 0 {
            let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
            if GetGUIThreadInfo(thread, &mut info).is_ok()
                && [info.hwndFocus, info.hwndActive, info.hwndCaret].into_iter().any(|hwnd| window_pid(hwnd) == pid)
            {
                return true;
            }
        }
        let mut search = ChildSearch { pid, found: false };
        let _ = EnumChildWindows(foreground, Some(child_has_pid), LPARAM(&mut search as *mut ChildSearch as isize));
        search.found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicU32, Arc};

    /// 前面の処理の番号を差し替えられる確かめ
    fn gate_with_foreground() -> (Gate, Arc<AtomicU32>) {
        let foreground = Arc::new(AtomicU32::new(100));
        let seen = foreground.clone();
        (Gate::with_check(Box::new(move |pid| pid == seen.load(Ordering::Relaxed))), foreground)
    }

    #[test]
    fn owner_may_show_change_and_hide() {
        let (gate, _) = gate_with_foreground();
        let owner = Some((1, 100));
        assert!(gate.allow(owner, Kind::Show));
        assert!(gate.allow(owner, Kind::Change));
        assert!(gate.allow(owner, Kind::Hide));
        assert_eq!(gate.rejected(), 0);
    }

    #[test]
    fn other_clients_cannot_show_change_or_hide() {
        let (gate, _) = gate_with_foreground();
        assert!(gate.allow(Some((1, 100)), Kind::Show));
        let other = Some((2, 200));
        assert!(!gate.allow(other, Kind::Show), "a background client showed the window");
        assert!(!gate.allow(other, Kind::Change), "a background client changed candidates, position or mode");
        assert!(!gate.allow(other, Kind::Hide), "a background client hid the owner's window");
        assert_eq!(gate.rejected(), 3);
    }

    #[test]
    fn clients_without_a_process_number_are_refused() {
        let (gate, _) = gate_with_foreground();
        for kind in [Kind::Show, Kind::Hide, Kind::Change] {
            assert!(!gate.allow(None, kind));
        }
    }

    #[test]
    fn last_shower_may_hide_after_focus_moves_but_not_change() {
        let (gate, foreground) = gate_with_foreground();
        let first = Some((1, 100));
        assert!(gate.allow(first, Kind::Show));
        foreground.store(300, Ordering::Relaxed);
        assert!(gate.allow(first, Kind::Hide), "the previous app could not close its own window");
        assert!(!gate.allow(first, Kind::Change), "the previous app kept changing the window after focus moved");
        assert!(!gate.allow(first, Kind::Show), "the previous app showed the window after focus moved");
    }

    #[test]
    fn showing_moves_the_right_to_hide_to_the_new_owner() {
        let (gate, foreground) = gate_with_foreground();
        let first = Some((1, 100));
        let second = Some((2, 300));
        assert!(gate.allow(first, Kind::Show));
        foreground.store(300, Ordering::Relaxed);
        assert!(gate.allow(second, Kind::Show));
        foreground.store(500, Ordering::Relaxed);
        assert!(!gate.allow(first, Kind::Hide), "an older client hid the window that a newer owner showed");
        assert!(gate.allow(second, Kind::Hide));
    }

    #[test]
    fn real_check_refuses_process_zero_and_a_process_that_does_not_exist() {
        assert!(!input_target_is(0));
        // 番号 1 の処理は無い (System Idle は 0、System は 4)。前面の窓の処理になることもない
        assert!(!input_target_is(1));
    }
}
