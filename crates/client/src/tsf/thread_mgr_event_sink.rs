use windows::Win32::UI::TextServices::{ITfContext, ITfDocumentMgr, ITfThreadMgrEventSink_Impl};

use anyhow::Result;

use crate::engine::{client_action::ClientAction, composition::CompositionState};

use super::factory::TextServiceFactory_Impl;

impl ITfThreadMgrEventSink_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    fn OnInitDocumentMgr(&self, _pdim: Option<&ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    #[macros::anyhow]
    fn OnUninitDocumentMgr(&self, _pdim: Option<&ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    #[macros::anyhow]
    fn OnSetFocus(
        &self,
        focus: Option<&ITfDocumentMgr>,
        _prevfocus: Option<&ITfDocumentMgr>,
    ) -> Result<()> {
        // 終わった (Deactivate 後の) 実体に届いたイベントは無視する
        if self.borrow()?.thread_mgr.is_none() {
            return Ok(());
        }
        // 2026-11-08 (仕事 53 の案 1): 開け閉めの区画の替わりが、フォーカスの移りの直後かを見るため
        crate::engine::mode_keys::note_focus();
        // 2026-10-28: IME が有効になったときにつなげていなければ、ここでもつなぎ直す (裏のスレッドで。2026-10-29)。
        // フォーカスの移りで待つ時間の合計も EVENT_BUDGET まで
        let _budget = crate::engine::ipc_service::EventBudget::start();
        crate::engine::ipc_service::reconnect_if_needed();
        // 2026-11-06 (仕事 25): 移った先がそのまま渡す欄 (パスワード欄・入力欄の種類が数字・メールなど) なら、言語バーを A の絵に
        // (保存しているモードは変えない。読めなければ あ のまま)
        let shown = focus.and_then(|focus| unsafe { focus.GetTop() }.ok()).map_or(false, |context| {
            let (field, field_id) = self.field_kind(&context, "focus");
            crate::engine::direct_field::shows_latin(field, crate::engine::direct_field::enabled_here(field_id))
        });
        crate::engine::direct_field::set_shown_latin(shown);
        self.update_lang_bar()?;

        // if focus is changed, the text layout sink should be updated
        if let Some(focus) = focus {
            self.borrow_mut()?.advise_text_layout_sink(focus.clone())?;
        }

        let actions = vec![ClientAction::EndComposition];
        // 2026-10-29: 確定に失敗しても (サーバーが返事をしない)、下の後片付けは続ける (レビュー L6)
        if let Err(error) = self.handle_action(&actions, CompositionState::None) {
            crate::debug_log::write(&format!("focus change: commit failed: {error:?}"));
        }
        crate::engine::latin_learn::focus_changed();
        crate::engine::tex::reset();
        crate::engine::auto_english::set_english_sentence(false);
        crate::engine::auto_english::note_commit(None);
        crate::engine::next_word::note_commit(None);
        crate::engine::composition::hide_latin_suggestions();

        Ok(())
    }

    #[macros::anyhow]
    fn OnPushContext(&self, _pic: Option<&ITfContext>) -> Result<()> {
        Ok(())
    }

    #[macros::anyhow]
    fn OnPopContext(&self, _pic: Option<&ITfContext>) -> Result<()> {
        Ok(())
    }
}
