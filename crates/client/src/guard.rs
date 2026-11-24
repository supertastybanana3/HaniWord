// 2026-10-14: IME の DLL はどのアプリの中でも動くので、ここでパニックするとアプリごと落ちる (Rust はパニックが
// COM の入口を越えるとプロセスを止める)。TSF の入口 (#[macros::anyhow]) とタイマーの入口でパニックを受け止め、
// そのキーはアプリへ返す。受け止めたことと、サーバーを待ちきれなかったことは compat_log.txt に残す
// (アプリ名・入口・場所だけ。パニックのメッセージは打った文字を含みうるので残さない)
use std::{
    cell::RefCell,
    io::Write as _,
    sync::atomic::{AtomicBool, Ordering},
};

static NEEDS_RESET: AtomicBool = AtomicBool::new(false);

thread_local! {
    static LAST_LOCATION: RefCell<String> = const { RefCell::new(String::new()) };
}

/// パニックの起きた場所を覚える。もとのフック (標準エラーへの表示) も呼ぶ
pub fn install_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
            let _ = LAST_LOCATION.try_with(|l| l.try_borrow_mut().map(|mut l| *l = location));
            previous(info);
        }));
    });
}

/// 入口でパニックを受け止めたとき (マクロから呼ぶ)。次に飲み込むキーで未確定の状態を片付ける
pub fn caught(entry: &str) {
    NEEDS_RESET.store(true, Ordering::SeqCst);
    let location = LAST_LOCATION
        .try_with(|l| l.try_borrow_mut().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default())
        .unwrap_or_default();
    record(&format!("panic caught in {entry} at {location}"));
    crate::debug_log::write(&format!("PANIC caught in {entry} at {location}"));
}

/// 入口ではない関数 (タイマー) の中身を、パニックを受け止めて走らせる
pub fn protect(entry: &str, body: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_err() {
        let _ = std::panic::catch_unwind(|| caught(entry));
    }
}

/// DLL の書き出し関数 (DllGetClassObject など) の中身を、パニックを受け止めて走らせる
pub fn run<T>(entry: &str, body: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(result) => result,
        Err(_) => {
            let _ = std::panic::catch_unwind(|| caught(entry));
            Err(anyhow::anyhow!("panic caught in {entry}"))
        }
    }
}

/// パニックのあと、まだ片付けていなければ true を返して片付けたことにする
pub fn take_reset() -> bool {
    NEEDS_RESET.swap(false, Ordering::SeqCst)
}

/// %APPDATA%\HaniWord\compat_log.txt に「時刻 アプリ名 pid 出来事」を 1 行足す。64KB を超えたら古いほうの半分を捨てる。
/// 管理者権限のアプリ・隔離されたアプリの中からは書かない (debug_log と同じ条件)
pub fn record(event: &str) {
    if cfg!(test) || !shared::winsec::private_data_allowed() {
        return;
    }
    let Some(dir) = shared::app_data_dir() else { return };
    let path = dir.join("compat_log.txt");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 64 * 1024) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let middle = text.len() / 2;
            let start = text.as_bytes()[middle..].iter().position(|&b| b == b'\n').map_or(text.len(), |i| middle + i + 1);
            let _ = std::fs::write(&path, &text[start..]);
        }
    }
    let exe = std::env::current_exe().ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{} {} pid={} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), exe, std::process::id(), event);
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    struct Probe;
    impl Probe {
        #[macros::anyhow]
        fn boom(&self, n: i32) -> Result<i32> {
            if n > 0 {
                panic!("boom {n}");
            }
            if n < 0 {
                anyhow::bail!("plain error");
            }
            Ok(n)
        }
    }

    #[test]
    fn panic_at_a_tsf_entry_becomes_e_fail_instead_of_aborting() {
        super::install_hook();
        let probe = Probe;
        assert_eq!(probe.boom(0).unwrap(), 0);
        assert_eq!(probe.boom(-1).unwrap_err().code(), windows::Win32::Foundation::E_FAIL);
        let _ = super::take_reset();
        assert_eq!(probe.boom(1).unwrap_err().code(), windows::Win32::Foundation::E_FAIL);
        assert!(super::take_reset());
        assert!(!super::take_reset());
        let mut ran = false;
        super::protect("timer", || { ran = true; panic!("timer boom") });
        assert!(ran && super::take_reset());
        let caught: anyhow::Result<i32> = super::run("export", || panic!("export boom"));
        assert!(caught.is_err() && super::take_reset());
        assert_eq!(super::run("export", || Ok(3)).unwrap(), 3);
    }
}
