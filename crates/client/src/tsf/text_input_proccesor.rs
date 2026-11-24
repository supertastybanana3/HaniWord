use std::collections::HashMap;

use crate::{
    engine::{client_action::ClientAction, composition::CompositionState, ipc_service, state::IMEState},
    globals::{
        DllModule, GUID_DISPLAY_ATTRIBUTE, GUID_DISPLAY_ATTRIBUTE_CONVERTED,
        GUID_DISPLAY_ATTRIBUTE_INPUT, GUID_DISPLAY_ATTRIBUTE_LIVE_DRAFT,
    },
};

use super::factory::TextServiceFactory_Impl;
use windows::{
    core::Interface as _,
    Win32::{
        Foundation::BOOL,
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::TextServices::{
            CLSID_TF_CategoryMgr, ITfCategoryMgr, ITfCompartmentEventSink, ITfCompartmentMgr, ITfKeyEventSink, ITfKeystrokeMgr,
            GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
            ITfLangBarItemButton, ITfLangBarItemMgr, ITfSource, ITfTextInputProcessorEx_Impl,
            ITfTextInputProcessor_Impl, ITfThreadMgr, ITfThreadMgrEventSink,
        },
    },
};

use anyhow::{Context, Result};

impl ITfTextInputProcessor_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    #[tracing::instrument]
    fn Activate(&self, ptim: Option<&ITfThreadMgr>, tid: u32) -> Result<()> {
        tracing::debug!("Activated with tid: {tid}");
        // 2026-10-29: つなぐ・最初のやり取りで待つ時間の合計は EVENT_BUDGET まで (超えたら、つなぎ直しは裏で)
        let _budget = ipc_service::EventBudget::start();

        // add reference to the dll instance to prevent it from being unloaded
        let mut dll_instance = DllModule::get()?;
        dll_instance.add_ref();

        // initialize ipc_service
        // 2026-10-28: つなげなくても (サーバー・候補窓がまだ受け付けていない) Activate は失敗させず、下の受け口はいつも登録する。
        // 前はここで抜けていたので、キーの受け口が無いまま、そのアプリでは最後まで英字だけが入っていた。つながるまでは、サーバーが返事をしない間と
        // 同じ扱いで IME 本体だけでひらがなを入れ、キーが来たら (フォーカスが移ったら) つなぎ直す (ipc_service::reconnect_if_needed)
        // (Activate を失敗させると、前の IME のアイコンが残って紛らわしい)
        if let Some(mut ipc_service) = ipc_service::connect_at_activation() {
            // 2026-10-14: サーバーの返事が遅くても Activate は失敗させない (失敗すると前の IME のアイコンが残って紛らわしい)
            if let Err(error) = ipc_service.append_text("".to_string()) {
                tracing::error!("append_text at activation failed: {error:?}");
            }
            // 入力モードの表示 (あ / A) をこのアプリのモードに合わせる
            // 2026-10-29: IME の状態を取れなくても (ほかが持っている) ここで抜けない。つないだものは次のキーで受け取る (レビュー L2)
            let kana = IMEState::get().map_or(true, |state| state.input_mode == crate::engine::input_mode::InputMode::Kana);
            let _ = ipc_service.set_input_mode(if kana { "あ" } else { "A" });
            ipc_service::install(ipc_service);
        } else {
            tracing::error!("Failed to initialize IPC service");
        }

        let mut text_service = self.borrow_mut()?;

        text_service.tid = tid;
        let thread_mgr = ptim.context("Thread manager is null")?;
        text_service.thread_mgr = Some(thread_mgr.clone());

        // initialize key event sink
        tracing::debug!("AdviseKeyEventSink");

        unsafe {
            thread_mgr.cast::<ITfKeystrokeMgr>()?.AdviseKeyEventSink(
                tid,
                &text_service.this::<ITfKeyEventSink>()?,
                BOOL::from(true),
            )?;
        };

        // 2026-11-08 (仕事 53): 入力モードを切り替える保ったキー (Alt+`・VK_KANJI)。登録できなくても続ける (今までのキーはそのまま)
        unsafe {
            let keystroke = thread_mgr.cast::<ITfKeystrokeMgr>()?;
            let description: Vec<u16> = "HaniWord".encode_utf16().collect();
            for (guid, key) in crate::globals::preserved_mode_keys() {
                if let Err(error) = keystroke.PreserveKey(tid, &guid, &key, &description) {
                    crate::debug_log::write(&format!("PreserveKey failed: {error:?}"));
                }
            }
        }

        // 2026-11-08 (仕事 53 の案 1、見るだけ): IME の開け閉めの区画が外から替わったら、印だけを debug_log に書く (区画には書かない)
        let openclose = (|| -> Result<u32> { unsafe {
            let compartment = thread_mgr.cast::<ITfCompartmentMgr>()?.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE)?;
            Ok(compartment.cast::<ITfSource>()?.AdviseSink(&ITfCompartmentEventSink::IID, &text_service.this::<ITfCompartmentEventSink>()?)?)
        } })();
        match openclose {
            Ok(cookie) => text_service.openclose_sink_cookie = Some(cookie),
            Err(error) => crate::debug_log::write(&format!("openclose sink failed: {error:?}")),
        }

        // initialize thread manager event sink
        tracing::debug!("AdviseThreadMgrEventSink");
        unsafe {
            let cookie = thread_mgr.cast::<ITfSource>()?.AdviseSink(
                &ITfThreadMgrEventSink::IID,
                &text_service.this::<ITfThreadMgrEventSink>()?,
            )?;
            text_service.thread_mgr_sink_cookie = Some(cookie);
        };

        // initialize text layout sink
        tracing::debug!("AdviseTextLayoutSink");
        let doc_mgr = unsafe { thread_mgr.GetFocus() };
        if let Ok(doc_mgr) = doc_mgr {
            text_service.advise_text_layout_sink(doc_mgr)?;
        }

        // initialize display attribute
        tracing::debug!("Initialize display attribute");
        let atom_map = unsafe {
            let mut map = HashMap::new();
            let category_mgr: ITfCategoryMgr =
                CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;

            let atom = category_mgr.RegisterGUID(&GUID_DISPLAY_ATTRIBUTE)?;
            map.insert(GUID_DISPLAY_ATTRIBUTE, atom);
            for guid in [
                GUID_DISPLAY_ATTRIBUTE_INPUT,
                GUID_DISPLAY_ATTRIBUTE_LIVE_DRAFT,
                GUID_DISPLAY_ATTRIBUTE_CONVERTED,
            ] {
                let atom = category_mgr.RegisterGUID(&guid)?;
                map.insert(guid, atom);
            }
            map
        };

        text_service.display_attribute_atom = atom_map;

        // initialize langbar
        tracing::debug!("Initialize langbar");
        unsafe {
            thread_mgr
                .cast::<ITfLangBarItemMgr>()?
                .AddItem(&text_service.this::<ITfLangBarItemButton>()?)?;
        };

        tracing::debug!("Activate success");
        crate::engine::terminal_choices::start();

        Ok(())
    }

    #[macros::anyhow]
    #[tracing::instrument]
    fn Deactivate(&self) -> Result<()> {
        tracing::debug!("Deactivated");
        let _budget = ipc_service::EventBudget::start();

        // remove reference to the dll instance
        let mut dll_instance = DllModule::get()?;
        dll_instance.release();

        // 後片付けは 1 つ失敗しても残りを続ける (途中で抜けるとイベントの登録が残り、終わった実体にイベントが届き続ける)
        let note = |step: &str, result: Result<()>| {
            if let Err(error) = result {
                crate::debug_log::write(&format!("Deactivate: {step} failed: {error:?}"));
            }
        };
        // 2026-10-20: 候補を出したまま別のキーボードに切り替えると、候補窓が残っていた。
        // フォーカスが移ったときと同じく、見えているものを確定して候補窓を閉じる
        note("commit", self.handle_action(&[ClientAction::EndComposition], CompositionState::None));
        crate::engine::composition::hide_latin_suggestions();
        if let Ok(state) = IMEState::get() {
            if let Some(mut ipc) = state.ipc_service.clone() {
                drop(state);
                let _ = ipc.hide_window();
            }
        }
        note("end composition", self.end_composition());
        {
            let text_service = self.borrow()?;
            if let Some(thread_mgr) = text_service.thread_mgr.clone() {
                note("unadvise key event sink", (|| unsafe {
                    thread_mgr.cast::<ITfKeystrokeMgr>()?.UnadviseKeyEventSink(text_service.tid)?;
                    Ok(())
                })());
                // 2026-11-08 (仕事 53): 保ったキーを外す
                for (guid, key) in crate::globals::preserved_mode_keys() {
                    note("unpreserve key", (|| unsafe {
                        thread_mgr.cast::<ITfKeystrokeMgr>()?.UnpreserveKey(&guid, &key)?;
                        Ok(())
                    })());
                }
                note("remove langbar", (|| unsafe {
                    thread_mgr
                        .cast::<ITfLangBarItemMgr>()?
                        .RemoveItem(&text_service.this::<ITfLangBarItemButton>()?)?;
                    Ok(())
                })());
            }
        }

        let mut text_service = self.borrow_mut()?;
        if let (Some(thread_mgr), Some(cookie)) = (text_service.thread_mgr.clone(), text_service.openclose_sink_cookie.take()) {
            note("unadvise openclose sink", (|| unsafe {
                let compartment = thread_mgr.cast::<ITfCompartmentMgr>()?.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE)?;
                compartment.cast::<ITfSource>()?.UnadviseSink(cookie)?;
                Ok(())
            })());
        }
        if let (Some(thread_mgr), Some(cookie)) = (text_service.thread_mgr.clone(), text_service.thread_mgr_sink_cookie.take()) {
            note("unadvise thread manager event sink", (|| unsafe {
                thread_mgr.cast::<ITfSource>()?.UnadviseSink(cookie)?;
                Ok(())
            })());
        }
        note("unadvise text layout sink", text_service.unadvise_text_layout_sink());

        // clear display attribute
        text_service.display_attribute_atom.clear();

        text_service.tid = 0;
        text_service.thread_mgr = None;

        tracing::debug!("Deactivate success");

        Ok(())
    }
}

impl ITfTextInputProcessorEx_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    fn ActivateEx(&self, ptim: Option<&ITfThreadMgr>, tid: u32, _dwflags: u32) -> Result<()> {
        // called when the text service is activated
        // if this function is implemented, the Activate() function won't be called
        // so we need to call the Activate function manually
        tracing::debug!("Activated(Ex) with tid: {tid}");
        self.Activate(ptim, tid)?;
        Ok(())
    }
}
