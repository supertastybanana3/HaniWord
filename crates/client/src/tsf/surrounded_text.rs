// reference to the original code:
// https://github.com/google/mozc/blob/master/src/win32/tip/tip_surrounding_text.cc

use std::{mem::ManuallyDrop, rc::Rc};

use anyhow::Result;
use windows::{
    core::{IUnknown, Interface},
    Win32::UI::TextServices::{
        ITfCompartmentMgr, ITfContext, ITfDocumentMgr, GUID_COMPARTMENT_TRANSITORYEXTENSION_PARENT,
        TF_ANCHOR_END, TF_ANCHOR_START, TF_DEFAULT_SELECTION, TF_HALTCOND, TF_HF_OBJECT, TF_SELECTION,
        ITfRange, TF_TF_MOVESTART, TS_SS_TRANSITORY,
    },
};

use crate::engine::state::IMEState;

use super::{edit_session::edit_session, factory::TextServiceFactory};

impl TextServiceFactory {
    fn to_parent_document_if_exists(
        &self,
        document_manager: Option<ITfDocumentMgr>,
    ) -> Result<ITfDocumentMgr> {
        let document_manager = match document_manager {
            Some(doc_mgr) => doc_mgr,
            None => return Err(anyhow::anyhow!("Document manager is null")),
        };

        unsafe {
            // Get top context
            let context = match document_manager.GetTop() {
                Ok(ctx) => ctx,
                Err(_) => return Ok(document_manager),
            };

            // Get status
            let status = match context.GetStatus() {
                Ok(s) => s,
                Err(_) => return Ok(document_manager),
            };

            // Check if context is transitory
            if (status.dwStaticFlags & TS_SS_TRANSITORY) != TS_SS_TRANSITORY {
                return Ok(document_manager);
            }

            // Get compartment manager
            let compartment_mgr = match document_manager.cast::<ITfCompartmentMgr>() {
                Ok(mgr) => mgr,
                Err(_) => return Ok(document_manager),
            };

            // Get compartment
            let compartment = match compartment_mgr
                .GetCompartment(&GUID_COMPARTMENT_TRANSITORYEXTENSION_PARENT)
            {
                Ok(comp) => comp,
                Err(_) => return Ok(document_manager),
            };

            // Get value
            let variant = match compartment.GetValue() {
                Ok(var) => var,
                Err(_) => return Ok(document_manager),
            };

            // Use a cloned IUnknown from VARIANT to avoid invalid reference-count handling.
            // If this is not VT_UNKNOWN (or null), treat it as "parent not available".
            let variant_unk = match IUnknown::try_from(&variant) {
                Ok(unk) => unk,
                Err(_) => return Ok(document_manager),
            };

            match variant_unk.cast::<ITfDocumentMgr>() {
                Ok(parent_doc_mgr) => Ok(parent_doc_mgr),
                Err(_) => Ok(document_manager),
            }
        }
    }

    fn to_parent_context_if_exists(&self, context: Option<ITfContext>) -> Result<ITfContext> {
        let context = match context {
            Some(ctx) => ctx,
            None => return Err(anyhow::anyhow!("Context is null")),
        };

        unsafe {
            // Get document manager
            let document_mgr = match context.GetDocumentMgr() {
                Ok(doc_mgr) => doc_mgr,
                Err(_) => return Ok(context),
            };

            // Get parent document
            let parent_doc_mgr = self.to_parent_document_if_exists(Some(document_mgr))?;

            // Get top context from parent document
            let parent_context = match parent_doc_mgr.GetTop() {
                Ok(ctx) => ctx,
                Err(_) => return Ok(context),
            };

            Ok(parent_context)
        }
    }

    /// 変換サーバーに渡す前の文脈 (AI の変換の左の文脈)。
    /// 2026-10-22: ふつうの欄では、入力中の範囲の頭から前へ 30 字を取る (入力していなければ選択の頭から)。
    /// 前は「カーソルの前 30 字から、入力中の表示の字数を引いた分」で、表示が 30 字以上だと空になり、
    /// カーソルが入力の途中にあるとずれていた (写経の 914 文で、文脈が渡っていたのは平均 3 字ほど)。
    /// 仮の文書 (TS_SS_TRANSITORY。入力中の字は親の文書に無い) の欄は今までどおり
    /// 2026-10-31: fallback は直前に確定した字 (Composition.char_before)。前の字を読めない・空の欄では、これを左の文脈にする (context_to_send)
    pub fn update_context(&self, preview: &str, fallback: Option<char>) -> Result<()> {
        // 2026-11-23 (Astra の点検 37): 覚えない欄 (IS_PRIVATE・パスワード・PIN) では前後の文を読まず、変換サーバーの左の文脈を空にする
        if crate::engine::direct_field::private_field() {
            if let Some(mut ipc_service) = IMEState::get()?.ipc_service.clone() {
                if let Err(error) = ipc_service.set_context(String::new(), String::new()) {
                    tracing::warn!("Failed to clear the context in a private field: {error:?}");
                }
            }
            return Ok(());
        }
        let result: Result<()> = (|| unsafe {
            let text_service = self.borrow()?;

            let context = text_service.context::<ITfContext>()?;
            let transitory = context.GetStatus()
                .is_ok_and(|status| (status.dwStaticFlags & TS_SS_TRANSITORY) == TS_SS_TRANSITORY);
            let composition = if transitory { None } else { text_service.borrow_composition()?.tip_composition.clone() };
            let parent_context = self.to_parent_context_if_exists(Some(context))?;

            // 2026-11-08 (仕事 45): カーソルの右の字も同じ編集セッションで取る (設定 input.right_context がオンのときだけ)
            let want_right = right_context_enabled();
            let texts = edit_session::<(String, String)>(
                text_service.tid,
                parent_context.clone(),
                Rc::new({
                    let preview_count = preview.chars().count() as i32;

                    move |cookie| {
                        let halt_cond = TF_HALTCOND {
                            pHaltRange: ManuallyDrop::new(None),
                            aHaltPos: TF_ANCHOR_START,
                            dwFlags: TF_HF_OBJECT,
                        };
                        // 範囲の頭から前へ 30 字 (終わりを end_shift 字だけ前へずらす) の字
                        let preceding = |range: &ITfRange, end_shift: i32| -> windows::core::Result<String> {
                            let mut preceding_range_shifted = 0;
                            let preceding_range = range.Clone()?;
                            preceding_range.Collapse(cookie, TF_ANCHOR_START)?;
                            preceding_range.ShiftStart(
                                cookie,
                                -30,
                                &mut preceding_range_shifted,
                                &halt_cond,
                            )?;
                            if end_shift != 0 {
                                preceding_range.ShiftEnd(
                                    cookie,
                                    -end_shift,
                                    &mut preceding_range_shifted,
                                    &halt_cond,
                                )?;
                            }

                            let mut pchtext = [0u16; 64];
                            let mut pcch = 0;
                            preceding_range.GetText(
                                cookie,
                                TF_TF_MOVESTART,
                                &mut pchtext,
                                &mut pcch,
                            )?;
                            Ok(String::from_utf16_lossy(&pchtext[..pcch as usize]))
                        };

                        // 2026-11-08 (仕事 45): 範囲の終わりから後ろへ 20 字 (カーソルの右の字。入力中の字・選んだ字の後ろ)。取れなければ空 (今と同じ動き)
                        let following = |range: &ITfRange| -> windows::core::Result<String> {
                            let following_range = range.Clone()?;
                            following_range.Collapse(cookie, TF_ANCHOR_END)?;
                            let mut following_range_shifted = 0;
                            following_range.ShiftEnd(cookie, RIGHT_CONTEXT_CHARS, &mut following_range_shifted, &halt_cond)?;
                            let mut pchtext = [0u16; 64];
                            let mut pcch = 0;
                            following_range.GetText(cookie, TF_TF_MOVESTART, &mut pchtext, &mut pcch)?;
                            Ok(String::from_utf16_lossy(&pchtext[..pcch as usize]))
                        };
                        let right_of = |range: &ITfRange| if want_right { following(range).unwrap_or_default() } else { String::new() };

                        // 1. ふつうの欄で入力中なら、入力中の範囲の頭から (うまくいかなければ 2. の今までの取り方)
                        if let Some(range) = composition.as_ref().and_then(|composition| composition.GetRange().ok()) {
                            if let Ok(text) = preceding(&range, 0) {
                                return Ok((text, right_of(&range)));
                            }
                        }

                        // 2. Get the selection from the parent context.
                        let mut pselection: [TF_SELECTION; 1] = [TF_SELECTION::default()];
                        let mut pfetched = 0;
                        parent_context.GetSelection(
                            cookie,
                            TF_DEFAULT_SELECTION,
                            &mut pselection,
                            &mut pfetched,
                        )?;

                        if pfetched == 0 {
                            return Ok((String::new(), String::new()));
                        }

                        let range = match pselection[0].range.as_ref() {
                            Some(range) => range.Clone()?,
                            None => return Ok((String::new(), String::new())),
                        };

                        Ok((preceding(&range, preview_count)?, right_of(&range)))
                    }
                }),
            )?;

            let (preceding_text, following_text) = match texts {
                Some((left, right)) => (Some(left), right),
                None => (None, String::new()),
            };
            let Some(preceding_text) = context_to_send(preceding_text, fallback) else {
                return Ok(());
            };

            let Some(mut ipc_service) = IMEState::get()?.ipc_service.clone() else {
                return Ok(());
            };

            ipc_service.set_context(preceding_text, right_context_to_send(&following_text))?;

            Ok(())
        })();

        if let Err(error) = result {
            tracing::warn!("Failed to update surrounded text context: {error:?}");
        }

        Ok(())
    }
}

/// 2026-10-31: 変換サーバーに送る左の文脈。読めた字があればそれ。読めない (None)・空なら、直前に確定した字。
/// どちらも無ければ、読めたときは空 (文の頭)、読めないときは送らない (今までどおり)。
/// 変換サーバーは左の文脈が空なら文の頭とみなし、頭の語が助詞の AI の候補を下げる (も|ビール → モビール)。
/// 前の字を読めない欄で、確定した 私 のすぐ後ろに打った もびーる (私もビール) を文の頭と取り違えないように
fn context_to_send(read: Option<String>, fallback: Option<char>) -> Option<String> {
    match (read, fallback) {
        (Some(text), _) if !text.is_empty() => Some(text),
        (_, Some(c)) => Some(c.to_string()),
        (read, None) => read,
    }
}

/// 2026-11-08 (仕事 45): カーソルの右の字を何字まで取るか (変換サーバーも 20 字・最初の改行までに切る)
const RIGHT_CONTEXT_CHARS: i32 = 20;

/// 2026-11-08 (仕事 45): カーソルの右の字も見て変換するか (設定 input.right_context、初期値オン)。オフなら右の字は取らずに空を送る
fn right_context_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| !cfg!(test) && shared::AppConfig::read_or_default().input.right_context)
}

/// 変換サーバーに送る右の字: 最初の改行より前の、頭から 20 字まで (改行で始まる・取れないときは空。今と同じ動き)
fn right_context_to_send(read: &str) -> String {
    read.split(['\r', '\n']).next().unwrap_or_default().chars().take(RIGHT_CONTEXT_CHARS as usize).collect()
}

#[cfg(test)]
mod tests {
    use super::{context_to_send, right_context_to_send};

    #[test]
    fn right_context_stops_at_the_first_line_break() {
        assert_eq!(right_context_to_send("、できるようになった。"), "、できるようになった。");
        assert_eq!(right_context_to_send("を操作する\r\n次の行"), "を操作する");
        assert_eq!(right_context_to_send("\r\n次の行"), "");
        assert_eq!(right_context_to_send(""), "");
        assert_eq!(right_context_to_send(&"あ".repeat(30)).chars().count(), 20);
    }

    #[test]
    fn unreadable_or_empty_text_falls_back_to_the_last_committed_char() {
        assert_eq!(context_to_send(Some("前の文。".into()), Some('私')).as_deref(), Some("前の文。"));
        assert_eq!(context_to_send(Some(String::new()), Some('私')).as_deref(), Some("私"));
        assert_eq!(context_to_send(None, Some('私')).as_deref(), Some("私"));
        assert_eq!(context_to_send(Some(String::new()), None).as_deref(), Some(""));
        assert_eq!(context_to_send(None, None), None);
    }
}
