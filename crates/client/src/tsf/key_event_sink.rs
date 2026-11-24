use windows::{
    core::GUID,
    Win32::{
        Foundation::{BOOL, LPARAM, WPARAM},
        UI::TextServices::{ITfContext, ITfKeyEventSink_Impl},
    },
};

use anyhow::Result;

use super::factory::TextServiceFactory_Impl;
use std::sync::atomic::{AtomicBool, Ordering};

/// Shift だけを押して離したか (Google 日本語入力と同じく、一時的な英数入力をかなに戻すのに使う)
static SHIFT_ALONE: AtomicBool = AtomicBool::new(false);

fn note_key_down(wparam: WPARAM) {
    // VK_SHIFT = 0x10, VK_LSHIFT = 0xA0, VK_RSHIFT = 0xA1
    SHIFT_ALONE.store(matches!(wparam.0, 0x10 | 0xA0 | 0xA1), Ordering::Relaxed);
}

impl TextServiceFactory_Impl {
    fn note_key_up(&self, wparam: WPARAM) {
        crate::engine::diacritics::note_key_up(wparam.0);
        if matches!(wparam.0, 0x10 | 0xA0 | 0xA1) && SHIFT_ALONE.swap(false, Ordering::Relaxed) {
            if let Ok(text_service) = self.borrow() {
                if let Ok(mut composition) = text_service.borrow_mut_composition() {
                    composition.temp_ascii = false;
                    composition.shifted_count = 0;
                }
            }
        }
    }
}

// sink (aka event listener) for key events
impl ITfKeyEventSink_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    #[tracing::instrument]
    fn OnTestKeyDown(
        &self,
        pic: Option<&ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Result<BOOL> {
        // this function checks if the key event will be handled by "OnKeyUp" function
        // so we need to return TRUE if we want to handle the key event
        note_key_down(wparam);
        crate::engine::diacritics::note_key(wparam.0, lparam.0);
        // 2026-10-29: つなぎ直したものは、キーを受けるかどうかを決める前に受け取る (OnKeyDown と同じ状態で決める。レビュー L5)
        crate::engine::ipc_service::reconnect_if_needed();
        let result = self.process_key(pic, wparam)?.is_some();
        // 2026-10-29: 文字のキーを字のままアプリへ渡したか (入力中でないときの長押しで、アプリの字を置き換えてよいかを決める)
        crate::engine::diacritics::note_passed(wparam.0, result);

        Ok(result.into())
    }

    #[macros::anyhow]
    #[tracing::instrument]
    fn OnKeyDown(&self, pic: Option<&ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        // this function is called when a key is pressed
        // we can handle key events here
        note_key_down(wparam);
        crate::engine::diacritics::note_key(wparam.0, lparam.0);
        let result = self.handle_key(pic, wparam)?;

        Ok(result.into())
    }

    #[macros::anyhow]
    fn OnTestKeyUp(
        &self,
        _pic: Option<&ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        // same as OnTestKeyDown
        self.note_key_up(wparam);
        Ok(false.into())
    }

    #[macros::anyhow]
    fn OnKeyUp(&self, _pic: Option<&ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        self.note_key_up(wparam);
        // this function is called when a key is released
        // but we handle key events in OnKeyDown function
        // so just return S_OK
        Ok(false.into())
    }

    #[macros::anyhow]
    fn OnPreservedKey(&self, pic: Option<&ITfContext>, rguid: *const GUID) -> Result<BOOL> {
        // 2026-11-08 (仕事 53): 入力モードを切り替える保ったキー (Alt+`・VK_KANJI)。半角/全角 と同じ道で切り替える (打ちかけがあるときも同じ扱い)。
        // Alt+` は、今の配列で VK_OEM_3 が 1 の左の位置 (0x29) にあるときだけ。JIS 配列の Alt+@ は食べずにアプリへ
        use crate::engine::mode_keys;
        let guid = unsafe { rguid.as_ref() }.copied().unwrap_or_default();
        let toggles = guid == crate::globals::GUID_PRESERVED_KEY_KANJI
            || (guid == crate::globals::GUID_PRESERVED_KEY_BACKQUOTE && mode_keys::backquote_toggles_now());
        if !toggles {
            return Ok(false.into());
        }
        let eaten = mode_keys::with_forced_toggle(|| self.handle_key(pic, WPARAM(mode_keys::VK_HALF_FULL)))?;
        Ok(eaten.into())
    }

    #[macros::anyhow]
    fn OnSetFocus(&self, _fforeground: BOOL) -> Result<()> {
        Ok(())
    }
}
