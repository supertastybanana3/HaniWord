//! 2026-11-08 (仕事 53 の案 1、見るだけ): IME の開け閉め (GUID_COMPARTMENT_KEYBOARD_OPENCLOSE) の区画が外から替わったときの印を、
//! debug_log (debug_log_path.txt があるときだけ書く) に残す。HaniWord はこの区画を書かず、入力モードも替えない
//! (外の道具の IME_SET・Excel のセルごとの入力モード・Windows が自分で替える場面が、どれだけあるかを数えてから決める)
use windows::{
    core::{Interface, GUID},
    Win32::UI::TextServices::{ITfCompartmentEventSink_Impl, ITfCompartmentMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE},
};

use anyhow::Result;

use super::factory::TextServiceFactory_Impl;

impl ITfCompartmentEventSink_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    fn OnChange(&self, rguid: *const GUID) -> Result<()> {
        let guid = unsafe { rguid.as_ref() }.copied().unwrap_or_default();
        if guid != GUID_COMPARTMENT_KEYBOARD_OPENCLOSE {
            return Ok(());
        }
        let open = (|| -> Option<bool> {
            let thread_mgr = self.borrow().ok()?.thread_mgr.clone()?;
            let compartment = unsafe { thread_mgr.cast::<ITfCompartmentMgr>().ok()?.GetCompartment(&guid).ok()? };
            let value = unsafe { compartment.GetValue().ok()? };
            i32::try_from(&value).ok().map(|n| n != 0)
        })();
        crate::engine::mode_keys::note_open_close(open);
        Ok(())
    }
}
