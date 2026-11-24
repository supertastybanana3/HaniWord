use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, MutexGuard},
};

use windows::{core::GUID, Win32::UI::TextServices::ITfContext};

use super::{input_mode::InputMode, ipc_service::IPCService};

#[derive(Debug)]
pub struct IMEState {
    pub ipc_service: Option<IPCService>,
    pub input_mode: InputMode,
    pub cookies: HashMap<GUID, u32>,
    pub context: Option<ITfContext>,
}

pub static IME_STATE: LazyLock<Mutex<IMEState>> = LazyLock::new(|| {
    tracing::debug!("Creating IMEState");
    Mutex::new(IMEState {
        ipc_service: None,
        // アプリを開いたときの入力モード (設定 input.start_in_kana。初期値は「あ」)
        input_mode: if shared::AppConfig::read_or_default().input.start_in_kana {
            InputMode::Kana
        } else {
            InputMode::Latin
        },
        cookies: HashMap::new(),
        context: None,
    })
});
unsafe impl Sync for IMEState {}
unsafe impl Send for IMEState {}

impl IMEState {
    pub fn get() -> anyhow::Result<MutexGuard<'static, IMEState>> {
        try_lock_recovering(&IME_STATE)
    }
}

/// 2026-10-14: 持っている途中でパニックした (受け止めた) ロックは「壊れた」印が付き、そのままだと以後ずっと取れず、
/// そのアプリでは打てなくなる。中身はそのまま使えるので、印を外して取り直す
pub fn try_lock_recovering<T>(mutex: &'static Mutex<T>) -> anyhow::Result<MutexGuard<'static, T>> {
    match mutex.try_lock() {
        Ok(guard) => Ok(guard),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => {
            mutex.clear_poison();
            Ok(poisoned.into_inner())
        }
        Err(e) => anyhow::bail!("Failed to lock state: {:?}", e),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{LazyLock, Mutex};

    #[test]
    fn a_lock_poisoned_by_a_caught_panic_is_usable_again() {
        static STATE: LazyLock<Mutex<i32>> = LazyLock::new(|| Mutex::new(1));
        let _ = std::panic::catch_unwind(|| {
            let mut guard = STATE.lock().unwrap();
            *guard = 2;
            panic!("while holding the state");
        });
        assert!(STATE.is_poisoned());
        assert_eq!(*super::try_lock_recovering(&STATE).unwrap(), 2);
        assert!(!STATE.is_poisoned());
        let held = super::try_lock_recovering(&STATE).unwrap();
        assert!(super::try_lock_recovering(&STATE).is_err());
        drop(held);
    }
}
