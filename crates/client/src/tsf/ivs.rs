//! IVS replacement of one explicitly selected ideograph, without clipboard or key injection.

use std::{mem::ManuallyDrop, rc::Rc};

use anyhow::Result;
use windows::{core::Interface, Win32::UI::TextServices::{ITfContext, ITfContextComposition, ITfCompositionSink, ITfRange, TF_DEFAULT_SELECTION,
    TF_SELECTION, TF_TF_MOVESTART}};

use crate::engine::{ivs, client_action::ClientAction, composition::{CompositionState, Segment}};
use super::{edit_session::{read_session_sync, write_session_sync}, factory::TextServiceFactory};

fn selected_target(context: &ITfContext, cookie: u32) -> Result<Option<(ITfRange, String)>> {
    unsafe {
        let mut selections = [TF_SELECTION::default()];
        let mut fetched = 0;
        let result = context.GetSelection(cookie, TF_DEFAULT_SELECTION, &mut selections, &mut fetched);
        // GetSelection transfers a reference; release it even on a failed/empty read.
        let range = ManuallyDrop::take(&mut selections[0].range);
        result?;
        let Some(range) = range.filter(|_| fetched == 1) else { return Ok(None) };
        let read_range = range.Clone()?;
        let mut buffer = [0u16; 5]; // A base plus IVS takes at most four UTF-16 units.
        let mut count = 0;
        read_range.GetText(cookie, TF_TF_MOVESTART, &mut buffer, &mut count)?;
        if count as usize > buffer.len() || !read_range.IsEmpty(cookie)?.as_bool() { return Ok(None); }
        let Ok(text) = String::from_utf16(&buffer[..count as usize]) else { return Ok(None) };
        if !ivs::can_cycle(&text) { return Ok(None); }
        Ok(Some((range, text)))
    }
}

impl TextServiceFactory {
    pub fn has_selected_ivs_target(&self, context: &ITfContext) -> bool {
        let Ok(service) = self.borrow() else { return false };
        let tid = service.tid;
        drop(service);
        let read_context = context.clone();
        read_session_sync(tid, context.clone(), Rc::new(move |cookie| {
            Ok(selected_target(&read_context, cookie)?.is_some())
        })).ok().flatten().unwrap_or(false)
    }

    pub fn cycle_selected_ivs(&self, context: &ITfContext) -> Result<bool> {
        let (tid, sink) = {
            let service = self.borrow()?;
            (service.tid, service.this::<ITfCompositionSink>()?)
        };
        let edit_context = context.clone();
        let selected = write_session_sync(tid, context.clone(), Rc::new(move |cookie| {
            // Start over the actual selected range, not an empty insertion/query range.
            let Some((range, text)) = selected_target(&edit_context, cookie)? else { return Ok(None) };
            let composition_context: ITfContextComposition = edit_context.cast()?;
            let composition = unsafe { composition_context.StartComposition(cookie, &range, &sink)? };
            Ok(Some((composition, text)))
        }))?.flatten();
        let Some((composition, text)) = selected else { return Ok(false) };
        {
            let service = self.borrow()?;
            let mut comp = service.borrow_mut_composition()?;
            comp.tip_composition = Some(composition);
            comp.hiragana = text.clone();
            comp.caret = text.chars().count() as i32;
            comp.segments = vec![Segment { reading: text.clone(), text: text.clone(), ..Default::default() }];
            comp.ivs_original = Some(text);
            comp.focus = 0;
            comp.typed.clear();
            comp.typed_valid = false;
            comp.from_prediction = true; // The selected glyph has no recoverable phonetic reading.
            comp.latin_selected = false;
            comp.latin_learning_intent = false;
            comp.live = false;
            comp.state = CompositionState::Converting;
        }
        if let Err(error) = self.handle_action(&[ClientAction::CycleIvs], CompositionState::Converting) {
            log::warn!("IVS candidate picker failed: {error:?}");
            let _ = self.handle_action(&[ClientAction::Cancel], CompositionState::None);
            // The editor accepted the composition; never execute its shortcut a second time.
        }
        Ok(true)
    }
}
