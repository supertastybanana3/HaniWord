use macros::anyhow;
use windows::{
    core::{implement, AsImpl, Interface, IUnknown, GUID, VARIANT},
    Win32::{
        Foundation::RECT,
        System::Com::CoTaskMemFree,
        UI::TextServices::{
            ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfEditSession,
            ITfEditSession_Impl, ITfInputScope, ITfInsertAtSelection, ITfRange, InputScope, GUID_PROP_ATTRIBUTE,
            GUID_PROP_COMPOSING, GUID_PROP_INPUTSCOPE, TF_AE_NONE,
            TF_ANCHOR_END, TF_ANCHOR_START, TF_ES_READ, TF_ES_READWRITE, TF_ES_SYNC, TF_IAS_QUERYONLY, TF_SELECTION,
            TF_SELECTIONSTYLE, TF_ST_CORRECTION, TF_TF_MOVESTART,
        },
    },
};

use std::{cell::Cell, collections::HashMap, mem::ManuallyDrop, rc::Rc, time::Instant};

use anyhow::{Context, Result};

use crate::{
    engine::state::IMEState,
    extension::StringExt as _,
    globals::{
        GUID_DISPLAY_ATTRIBUTE, GUID_DISPLAY_ATTRIBUTE_CONVERTED, GUID_DISPLAY_ATTRIBUTE_INPUT,
        GUID_DISPLAY_ATTRIBUTE_LIVE_DRAFT,
    },
};

/// 下線の種類
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpanKind {
    /// 入力中 (ひらがなのまま): 波線
    Input,
    /// ライブ変換の仮表示: 点線
    LiveDraft,
    /// 変換済み (ほかの文節・ライブ変換の清書後): 細い実線
    Converted,
    /// 変換中の注目文節: 太い実線
    Focused,
}

/// 未確定文字列の一部分と、その表示のしかた
#[derive(Debug, Clone)]
pub struct PreeditSpan {
    pub start: i32,
    pub len: i32,
    pub kind: SpanKind,
}

use super::factory::TextServiceFactory;

// 2026-11-06 (端末で未確定の途中の形が積み重なった件): 未確定の範囲に字を書いたか (TIP_WRITTEN) と、未確定の範囲を失ったか (TIP_LOST)。
// Windows Terminal は、コンポジションの印の無い字を確定とみなしてシェルに送り、文書から消す (src/tsf/Implementation.cpp の _doCompositionUpdate)。
// IME 本体のコンポジションが字を覆えていないまま「範囲が空なら選択の場所に字を入れる」を続けると、打つたびに途中の形がまるごと確定されていった。
// 編集のセッションはあとで (非同期で) 走ることがあるので、印はセッションの中から立てる。IME 本体の実体はスレッドに 1 つなので、スレッドごとに持つ
thread_local! {
    static TIP_WRITTEN: Cell<bool> = const { Cell::new(false) };
    static TIP_LOST: Cell<bool> = const { Cell::new(false) };
    // 2026-11-08 (メモ帳で確定した文が消えた件): 未確定の範囲のうち、IME 本体のものとみなす字数 (UTF-16)。最後に書いた字数。
    // 打ち始めは、始めたときの範囲の字数 (利用者が選んでいた字。打つと置き換わる。今までどおり)
    static TIP_OWNED: Cell<usize> = const { Cell::new(0) };
    // 2026-11-08 (同じ件): 編集のセッションの中にいるか (深さ)。アプリがその場でコンポジションを終えた知らせ (OnCompositionTerminated) を見分ける
    static IN_SESSION: Cell<u32> = const { Cell::new(0) };
}

/// 2026-11-08 (メモ帳で確定した文が消えた件): 未確定の範囲に書く (置き換える・消す) かの決まり。
/// 範囲が、自分が書いた字数 (打ち始めは利用者が選んでいた字数) より長ければ、書かない・消さない。読めないアプリ (None) は今のまま書く
pub(crate) fn may_write_over(range_len: Option<usize>, owned: usize) -> bool {
    range_len.map_or(true, |len| len <= owned)
}

/// 打ち始めの範囲 (選択) の字数を数える上限。これより長い選択の上に打ち始めたら、最初の書き込みは確かめない (今までどおり置き換える)
const START_LIMIT: usize = 1 << 20;

/// 範囲の字数 (UTF-16) を limit 字まで数える (limit より長ければ limit)。読めなければ None
unsafe fn range_len_upto(cookie: u32, range: &ITfRange, limit: usize) -> Option<usize> {
    let probe = range.Clone().ok()?;
    let mut buffer = [0u16; 4096];
    let mut total = 0usize;
    while total < limit {
        let want = (limit - total).min(buffer.len());
        let mut fetched: u32 = 0;
        probe.GetText(cookie, TF_TF_MOVESTART, &mut buffer[..want], &mut fetched).ok()?;
        total += fetched as usize;
        if (fetched as usize) < want {
            break;
        }
    }
    Some(total.min(limit))
}

/// 未確定の範囲 (range) に書いてよいかを確かめる (may_write_over)。範囲が自分のものより長ければ (自分が書いていない字、
/// たとえば前に確定した文を覆っている)、書かずにコンポジションを終え (字には触らない)、次のキーでカーソルの所から始め直す。
/// ログは字数だけ。(書いてよいか, 範囲の字数 (owned+1 字まで。読めなければ None))
unsafe fn guard_write(cookie: u32, composition: &ITfComposition, range: &ITfRange, what: &str) -> (bool, Option<usize>) {
    let owned = TIP_OWNED.with(Cell::get);
    if owned >= START_LIMIT {
        return (true, None);
    }
    let len = range_len_upto(cookie, range, owned + 1);
    if may_write_over(len, owned) {
        return (true, len);
    }
    lose_composition(cookie, composition, &format!(
        "{what}: the composition range is longer than the text written (range>={}, written={owned}); not writing", len.unwrap_or(0)));
    (false, len)
}

/// 2026-11-08 (メモ帳で確定した文が消えた件): IME 本体の編集のセッションの中から呼ばれたか
/// (アプリがその場でコンポジションを終えると、OnCompositionTerminated がセッションの中から呼ばれる)
pub fn in_edit_session() -> bool {
    IN_SESSION.with(Cell::get) > 0
}

/// 自分の編集の中でアプリがコンポジションを終えたとき: 借りに行かず、印だけ立てる (片付けは次のキーの頭の take_composition_lost から)
pub fn mark_composition_lost(why: &str) {
    TIP_LOST.with(|lost| lost.set(true));
    TIP_WRITTEN.with(|written| written.set(false));
    TIP_OWNED.with(|owned| owned.set(0));
    crate::debug_log::write(&format!("preedit: {why}; the input starts over at the next key"));
}

/// 編集のセッションの深さを数える (抜けるとき、失敗しても戻す)
struct SessionDepth;

impl SessionDepth {
    fn enter() -> Self {
        IN_SESSION.with(|depth| depth.set(depth.get() + 1));
        SessionDepth
    }
}

impl Drop for SessionDepth {
    fn drop(&mut self) {
        IN_SESSION.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// 未確定の範囲を失った (前のキーで書いた字が範囲から消えていた・書いた字を範囲が覆わなかった) か
pub fn composition_lost() -> bool {
    TIP_LOST.with(Cell::get)
}

/// composition_lost を読んで印を下ろす (次のキーで未確定の状態を空にするとき)
pub fn take_composition_lost() -> bool {
    TIP_LOST.with(|lost| lost.replace(false))
}

/// 書いた字 (expected 字、UTF-16) を、未確定の範囲 (composition の範囲) が覆っているか。範囲が空のときだけ偽
/// (字数を切るアプリ・字を読めないアプリでは、短くても読めなくても覆っているとみる。直すのは、範囲の外に字を書いてしまう形だけ)
unsafe fn composition_covers(cookie: u32, composition: &ITfComposition, expected: usize) -> bool {
    if expected == 0 { return true; }
    let Ok(range) = composition.GetRange() else { return false };
    range.IsEmpty(cookie).map_or(true, |empty| !empty.as_bool())
}

/// 2026-11-06 (端末の件、ログだけ): 未確定の範囲の字に、TSF のコンポジションの印 (GUID_PROP_COMPOSING) が付いているか。読めなければ None。
/// Windows Terminal はこの印の無い字を確定として送るので、印が無いのに範囲が字を覆っている形が起きているかを見る
unsafe fn composing_mark(cookie: u32, context: &ITfContext, composition: &ITfComposition) -> Option<bool> {
    let range = composition.GetRange().ok()?;
    let property = context.GetProperty(&GUID_PROP_COMPOSING).ok()?;
    let value = property.GetValue(cookie, &range).ok()?;
    Some(i32::try_from(&value).is_ok_and(|n| n != 0))
}

/// 未確定の範囲を失ったとみて、コンポジションを終え (終えられなくてもよい)、次のキーで始め直す印を立てる
unsafe fn lose_composition(cookie: u32, composition: &ITfComposition, why: &str) {
    let ended = composition.EndComposition(cookie).is_ok();
    TIP_LOST.with(|lost| lost.set(true));
    TIP_WRITTEN.with(|written| written.set(false));
    TIP_OWNED.with(|owned| owned.set(0));
    crate::debug_log::write(&format!("preedit: {why} (ended={ended}); the input starts over at the next key"));
}

#[implement(ITfEditSession)]
struct EditSession<'a, T> {
    callback: Rc<dyn Fn(u32) -> anyhow::Result<T>>,
    pub result: Cell<Option<T>>,
    phantom: std::marker::PhantomData<&'a T>,
}

// edit action will be performed within this function
pub fn edit_session<T>(
    client_id: u32,
    context: ITfContext,
    callback: Rc<dyn Fn(u32) -> anyhow::Result<T>>,
) -> Result<Option<T>> {
    edit_session_with(client_id, context, callback, TF_ES_READWRITE)
}

/// 読むだけの編集セッションをその場で (同期で) 行う。アプリが許さなければ Err
pub fn read_session_sync<T>(
    client_id: u32,
    context: ITfContext,
    callback: Rc<dyn Fn(u32) -> anyhow::Result<T>>,
) -> Result<Option<T>> {
    edit_session_with(client_id, context, callback, TF_ES_READ | TF_ES_SYNC)
}

pub fn write_session_sync<T>(
    client_id: u32,
    context: ITfContext,
    callback: Rc<dyn Fn(u32) -> anyhow::Result<T>>,
) -> Result<Option<T>> {
    edit_session_with(client_id, context, callback, TF_ES_READWRITE | TF_ES_SYNC)
}

fn edit_session_with<T>(
    client_id: u32,
    context: ITfContext,
    callback: Rc<dyn Fn(u32) -> anyhow::Result<T>>,
    flags: windows::Win32::UI::TextServices::TF_CONTEXT_EDIT_CONTEXT_FLAGS,
) -> Result<Option<T>> {
    let session: ITfEditSession = EditSession {
        callback,
        result: Cell::new(None),
        phantom: std::marker::PhantomData,
    }
    .into();
    // 拒否されたとき (同期で取れない等) は Ok(None)
    let result = unsafe { context.RequestEditSession(client_id, &session, flags) };

    match result {
        Ok(result) => {
            result.ok()?;
            let session = unsafe { session.as_impl() };
            Ok(session.result.take())
        }
        Err(e) => Err(anyhow::Error::new(e)),
    }
}

impl<'a, T> ITfEditSession_Impl for EditSession_Impl<'a, T> {
    #[anyhow]
    fn DoEditSession(&self, cookie: u32) -> Result<()> {
        let _depth = SessionDepth::enter();
        let result = (self.callback)(cookie)?;
        self.result.set(Some(result));
        Ok(())
    }
}

impl TextServiceFactory {
    #[tracing::instrument]
    pub fn start_composition(&self) -> Result<()> {
        tracing::debug!("start_composition");

        // 2026-11-08 (メモ帳で確定した文が消えた件): 編集のセッションを頼む前に借りを手放す (セッションの中から、アプリの知らせで
        // IME 本体が呼ばれることがある。前は借りたまま頼み、知らせの側の確定が「RefCell already borrowed」で失敗した)
        let (tid, context, context_composition, sink, insert, tip_exists) = {
            let text_service = self.borrow()?;
            let tip_exists = text_service.borrow_composition()?.tip_composition.is_some();
            (
                text_service.tid,
                text_service.context::<ITfContext>()?,
                text_service.context::<ITfContextComposition>()?,
                text_service.this::<ITfCompositionSink>()?,
                text_service.context::<ITfInsertAtSelection>()?,
                tip_exists,
            )
        };

        if tip_exists {
            self.end_composition()?;
            return Ok(());
        }

        let composition = edit_session::<ITfComposition>(
            tid,
            context,
            Rc::new({
                move |cookie| unsafe {
                    let range = insert.InsertTextAtSelection(cookie, TF_IAS_QUERYONLY, &[])?;
                    // 2026-11-08: 始めた範囲の字数 (選択に字があれば、最初の書き込みでその字を置き換える。今までどおり)。読めなければ 0
                    let selected = range_len_upto(cookie, &range, START_LIMIT).unwrap_or(0);
                    let composition =
                        context_composition.StartComposition(cookie, &range, &sink)?;
                    TIP_OWNED.with(|owned| owned.set(selected));
                    if selected > 0 {
                        crate::debug_log::write(&format!("start_composition: started over the selection ({selected} chars)"));
                    }

                    Ok(composition)
                }
            }),
        )?;

        tracing::debug!("Composition started {composition:?}");
        if composition.is_none() {
            crate::debug_log::write("start_composition: the app did not start the composition right away");
        }
        TIP_WRITTEN.with(|written| written.set(false));
        TIP_LOST.with(|lost| lost.set(false));
        self.borrow()?.borrow_mut_composition()?.tip_composition = composition;

        Ok(())
    }

    /// 2026-11-08 (メモ帳で確定した文が消えた件): 書き込みの編集のセッションに要るもの (tid・文書・コンポジション・表示の属性)。
    /// コンポジションが無ければ None (文書は取りに行かない。今までどおり)。借りはここで手放す (セッションの中から、アプリの知らせで IME 本体が呼ばれることがある)
    fn session_parts(&self) -> Result<Option<(u32, ITfContext, ITfComposition, HashMap<GUID, u32>)>> {
        let text_service = self.borrow()?;
        let Some(tip) = text_service.borrow_composition()?.tip_composition.clone() else {
            return Ok(None);
        };
        Ok(Some((text_service.tid, text_service.context::<ITfContext>()?, tip, text_service.display_attribute_atom.clone())))
    }

    /// キャレットの直前の 1 文字 (スマート全角スペース用)。取れなければ None
    pub fn preceding_char(&self) -> Option<char> {
        let text_service = self.borrow().ok()?;
        let context = text_service.context::<ITfContext>().ok()?;
        let insert = text_service.context::<ITfInsertAtSelection>().ok()?;
        let result = edit_session::<Option<char>>(
            text_service.tid,
            context,
            Rc::new(move |cookie| unsafe {
                let range = insert.InsertTextAtSelection(cookie, TF_IAS_QUERYONLY, &[])?;
                let previous = range.Clone()?;
                previous.Collapse(cookie, TF_ANCHOR_START)?;
                let mut shifted: i32 = 0;
                previous.ShiftStart(cookie, -2, &mut shifted, std::ptr::null())?;
                if shifted == 0 {
                    return Ok(None);
                }
                let mut buffer = [0u16; 4];
                let mut fetched: u32 = 0;
                previous.GetText(cookie, 0, &mut buffer, &mut fetched)?;
                let text = String::from_utf16_lossy(&buffer[..fetched as usize]);
                Ok(text.chars().last())
            }),
        )
        .ok()??;
        result
    }

    /// 素通しの判定用: キャレットの直前の文字を同期の読み取りで調べる (キーの判定中に呼ぶ)
    pub fn preceding_in(&self, context: &ITfContext) -> crate::engine::prefix_direct::Preceding {
        use crate::engine::prefix_direct::Preceding;
        let Ok(text_service) = self.borrow() else { return Preceding::Unknown };
        let Ok(insert) = context.cast::<ITfInsertAtSelection>() else { return Preceding::Unknown };
        let tid = text_service.tid;
        drop(text_service);
        let result = read_session_sync::<Preceding>(
            tid,
            context.clone(),
            Rc::new(move |cookie| unsafe {
                let range = insert.InsertTextAtSelection(cookie, TF_IAS_QUERYONLY, &[])?;
                let previous = range.Clone()?;
                previous.Collapse(cookie, TF_ANCHOR_START)?;
                let mut shifted: i32 = 0;
                previous.ShiftStart(cookie, -2, &mut shifted, std::ptr::null())?;
                if shifted == 0 {
                    return Ok(Preceding::Start);
                }
                let mut buffer = [0u16; 4];
                let mut fetched: u32 = 0;
                previous.GetText(cookie, 0, &mut buffer, &mut fetched)?;
                let text = String::from_utf16_lossy(&buffer[..fetched as usize]);
                Ok(text.chars().last().map_or(Preceding::Start, Preceding::Char))
            }),
        );
        match result {
            Ok(Some(preceding)) => preceding,
            _ => Preceding::Unknown,
        }
    }

    /// 2026-10-14: キャレットの前の文字列を、同期の読み取りで最大 max 字まで。(文字列, 入力欄の先頭まで届いたか)。読めなければ None
    pub fn preceding_text_in(&self, context: &ITfContext, max: i32) -> Option<(String, bool)> {
        let text_service = self.borrow().ok()?;
        let insert = context.cast::<ITfInsertAtSelection>().ok()?;
        let tid = text_service.tid;
        drop(text_service);
        let result = read_session_sync::<(String, bool)>(
            tid,
            context.clone(),
            Rc::new(move |cookie| unsafe {
                let range = insert.InsertTextAtSelection(cookie, TF_IAS_QUERYONLY, &[])?;
                let previous = range.Clone()?;
                previous.Collapse(cookie, TF_ANCHOR_START)?;
                let mut shifted: i32 = 0;
                previous.ShiftStart(cookie, -max, &mut shifted, std::ptr::null())?;
                let mut buffer = vec![0u16; max.max(0) as usize + 2];
                let mut fetched: u32 = 0;
                if shifted != 0 {
                    previous.GetText(cookie, 0, &mut buffer, &mut fetched)?;
                }
                let reached_start = shifted.unsigned_abs() < max.unsigned_abs();
                Ok((String::from_utf16_lossy(&buffer[..fetched as usize]), reached_start))
            }),
        );
        result.ok().flatten()
    }

    /// 2026-10-14: 入力欄の種類 (InputScope、direct_field.rs)。指定が無ければ空、読めなければ None。
    /// 読み方は Mozc の TipRangeUtil::GetInputScopes と同じ (キャレットの範囲の GUID_PROP_INPUTSCOPE)
    pub fn input_scopes_in(&self, context: &ITfContext) -> Option<Vec<i32>> {
        let text_service = self.borrow().ok()?;
        let insert = context.cast::<ITfInsertAtSelection>().ok()?;
        let tid = text_service.tid;
        drop(text_service);
        let owner = context.clone();
        let result = read_session_sync::<Vec<i32>>(
            tid,
            context.clone(),
            Rc::new(move |cookie| unsafe {
                let range = insert.InsertTextAtSelection(cookie, TF_IAS_QUERYONLY, &[])?;
                let property = owner.GetAppProperty(&GUID_PROP_INPUTSCOPE)?;
                let value = property.GetValue(cookie, &range)?;
                let Ok(unknown) = IUnknown::try_from(&value) else {
                    return Ok(Vec::new());
                };
                let scope: ITfInputScope = unknown.cast()?;
                let mut buffer: *mut InputScope = std::ptr::null_mut();
                let mut count: u32 = 0;
                scope.GetInputScopes(&mut buffer, &mut count)?;
                if buffer.is_null() {
                    return Ok(Vec::new());
                }
                let scopes = std::slice::from_raw_parts(buffer, count.min(64) as usize).iter().map(|s| s.0).collect();
                CoTaskMemFree(Some(buffer as *const _));
                Ok(scopes)
            }),
        );
        result.ok().flatten()
    }

    /// 未確定の文字列が無いときに、候補窓をキャレットの位置に出す (長押しの候補)。読めなければシステムのキャレットの位置
    pub fn update_pos_at_selection(&self) {
        let rect = (|| -> Option<RECT> {
            let text_service = self.borrow().ok()?;
            let context = text_service.context::<ITfContext>().ok()?;
            let insert = text_service.context::<ITfInsertAtSelection>().ok()?;
            let tid = text_service.tid;
            drop(text_service);
            let view_context = context.clone();
            edit_session::<RECT>(
                tid,
                context,
                Rc::new(move |cookie| unsafe {
                    let range = insert.InsertTextAtSelection(cookie, TF_IAS_QUERYONLY, &[])?;
                    let view = view_context.GetActiveView()?;
                    let mut rect = RECT::default();
                    let mut clipped = false.into();
                    view.GetTextExt(cookie, &range, &mut rect, &mut clipped)?;
                    Ok(rect)
                }),
            )
            .ok()?
            .filter(|r| r.bottom > r.top || r.right > r.left)
        })()
        .or_else(|| unsafe {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::Graphics::Gdi::ClientToScreen;
            use windows::Win32::System::Threading::GetCurrentThreadId;
            use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO};
            let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
            GetGUIThreadInfo(GetCurrentThreadId(), &mut info).ok()?;
            if info.hwndCaret.is_invalid() {
                return None;
            }
            let mut top_left = POINT { x: info.rcCaret.left, y: info.rcCaret.top };
            let mut bottom_right = POINT { x: info.rcCaret.right, y: info.rcCaret.bottom };
            let _ = ClientToScreen(info.hwndCaret, &mut top_left);
            let _ = ClientToScreen(info.hwndCaret, &mut bottom_right);
            Some(RECT { left: top_left.x, top: top_left.y, right: bottom_right.x, bottom: bottom_right.y })
        });
        if let (Some(rect), Ok(state)) = (rect, IMEState::get()) {
            if let Some(mut ipc) = state.ipc_service.clone() {
                drop(state);
                let _ = ipc.set_window_position(rect.top, rect.left, rect.bottom, rect.right);
            }
        }
    }

    /// 確定の取り消し用: キャレットの直前にある `text` (直前に確定したもの) を未確定文字列に戻す。
    /// 直前の文字列が `text` と一致しないとき (カーソルを動かした等) は何もせず false
    #[tracing::instrument]
    pub fn start_composition_over_previous(&self, text: &str) -> Result<bool> {
        // 2026-11-08 (メモ帳で確定した文が消えた件): 編集のセッションを頼む前に借りを手放す (start_composition と同じ)
        let (tid, context, context_composition, sink, insert) = {
            let text_service = self.borrow()?;
            if text_service.borrow_composition()?.tip_composition.is_some() {
                return Ok(false);
            }
            (
                text_service.tid,
                text_service.context::<ITfContext>()?,
                text_service.context::<ITfContextComposition>()?,
                text_service.this::<ITfCompositionSink>()?,
                text_service.context::<ITfInsertAtSelection>()?,
            )
        };
        let expected: Vec<u16> = text.encode_utf16().collect();
        let expected_empty = expected.is_empty();
        // 2026-11-05 (仕事 23): 数字を全角で見せていたら、アプリの字は全角の数字 (長さは同じ。確定したときの形はどちらの全角の形か分からないので両方と比べる)
        let shown: Vec<u16> = crate::engine::full_width::fullwidth_digits(text).encode_utf16().collect();
        let shown_all: Vec<u16> = crate::engine::full_width::fullwidth_digits_all(text).encode_utf16().collect();

        // 2026-10-20 (Codex の再調査 L3): 同期の編集セッションで行う。非同期で受け付けられると、あとで始まった未確定文字列を
        // ここで受け取れず、IME が追跡しない未確定文字列がアプリに残るため。同期で取れなければ何もしない (Tab・やり直しはアプリへ)
        let composition = write_session_sync::<Option<ITfComposition>>(
            tid,
            context,
            Rc::new({
                move |cookie| unsafe {
                    let range = insert.InsertTextAtSelection(cookie, TF_IAS_QUERYONLY, &[])?;
                    let previous = range.Clone()?;
                    previous.Collapse(cookie, TF_ANCHOR_START)?;
                    let mut shifted: i32 = 0;
                    previous.ShiftStart(cookie, -(expected.len() as i32), &mut shifted, std::ptr::null())?;
                    if shifted != -(expected.len() as i32) {
                        return Ok(None);
                    }
                    let mut buffer = vec![0u16; expected.len() + 8];
                    let mut fetched: u32 = 0;
                    previous.GetText(cookie, 0, &mut buffer, &mut fetched)?;
                    let got = &buffer[..fetched as usize];
                    if got != &expected[..] && got != &shown[..] && got != &shown_all[..] {
                        return Ok(None);
                    }
                    let composition = context_composition.StartComposition(cookie, &previous, &sink)?;
                    // 2026-11-08: 前の字 (自分が確定した字) の上に始めたので、その字数を IME 本体のものとみなす
                    TIP_OWNED.with(|owned| owned.set(expected.len()));
                    Ok(Some(composition))
                }
            }),
        )
        .ok()
        .flatten()
        .flatten();

        match composition {
            Some(composition) => {
                // 前の字の上に始めたので、範囲にはもう字がある
                TIP_WRITTEN.with(|written| written.set(!expected_empty));
                TIP_LOST.with(|lost| lost.set(false));
                self.borrow()?.borrow_mut_composition()?.tip_composition = Some(composition);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    #[tracing::instrument]
    pub fn end_composition(&self) -> Result<()> {
        tracing::debug!("end_composition");
        // 2026-11-08 (メモ帳で確定した文が消えた件): 編集のセッションを頼む前に借りを手放す。前は借りたまま頼み、セッションの中で
        // アプリがコンポジションを終えると、その知らせの側の確定が「RefCell already borrowed」で失敗した (Cancel のとき)
        if let Some((tid, context, composition, _)) = self.session_parts()? {
            let result = edit_session(
                tid,
                context.clone(),
                Rc::new({
                    move |cookie| unsafe {
                        // 2026-11-08: このキーで未確定の範囲を失っていたら (コンポジションはもう終えた)、字には触らない
                        if TIP_LOST.with(Cell::get) {
                            return Ok(());
                        }
                        // clear display attribute first
                        let range: ITfRange = composition.GetRange()?;

                        // set existing text to the composition
                        let mut text = vec![0; 1024];
                        let mut text_len = 1024;

                        let range_new = range.Clone()?;
                        range_new.GetText(cookie, TF_TF_MOVESTART, &mut text, &mut text_len)?;

                        text = text[..text_len as usize].to_vec();
                        range.SetText(cookie, TF_ST_CORRECTION, &text)?;

                        let prop = context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
                        prop.Clear(cookie, &range)?;

                        // shift the start of the composition
                        range.Collapse(cookie, TF_ANCHOR_END)?;
                        let selection = TF_SELECTION {
                            range: ManuallyDrop::new(Some(range.clone())),
                            style: TF_SELECTIONSTYLE {
                                ase: TF_AE_NONE,
                                fInterimChar: false.into(),
                            },
                        };

                        context.SetSelection(cookie, &[selection])?;

                        composition.EndComposition(cookie)?;
                        Ok(())
                    }
                }),
            );
            // 2026-11-06 (c): 終えられなくても (アプリが入力を終えていた・編集を断られた)、IME 本体はこのコンポジションを手放して先へ進む。
            // 前はここで抜けて、IME 本体が終わったコンポジションを持ったまま「入力中」に残り、フォーカスを移しても戻らなかった
            if let Err(error) = result {
                crate::debug_log::write(&format!("end_composition failed: {error:?}"));
            }
        } else {
            tracing::warn!("Composition is not started");
        }

        TIP_WRITTEN.with(|written| written.set(false));
        TIP_OWNED.with(|owned| owned.set(0));
        self.borrow()?.borrow_mut_composition()?.tip_composition = None;

        Ok(())
    }

    #[tracing::instrument]
    pub fn set_text(&self, text: &str, subtext: &str) -> Result<()> {
        // 2026-11-08 (メモ帳で確定した文が消えた件): 編集のセッションを頼む前に借りを手放す
        if let Some((tid, context, composition, display_attribute_atom)) = self.session_parts()? {
            edit_session(
                tid,
                context.clone(),
                Rc::new({
                    let text_len = text.chars().count() as i32;

                    // unpadded is all you need!
                    let text = format!("{text}{subtext}").as_str().to_wide_16_unpadded();

                    move |cookie| unsafe {
                        if TIP_LOST.with(Cell::get) {
                            return Ok(());
                        }
                        let Ok(range) = composition.GetRange() else {
                            lose_composition(cookie, &composition, "set_text: no composition range");
                            return Ok(());
                        };
                        // 2026-11-08: 範囲が自分のものより長ければ (自分が書いていない字を覆っている)、書かない・消さない
                        if !guard_write(cookie, &composition, &range, "set_text").0 {
                            return Ok(());
                        }
                        if let Err(error) = range.SetText(cookie, TF_ST_CORRECTION, &text) {
                            lose_composition(cookie, &composition, &format!("set_text: SetText failed: {error:?}"));
                            return Ok(());
                        }
                        // 2026-11-06 (b): 書いた字を未確定の範囲が覆わなければ (入力が終わっている)、書いた字を消す (端末なら確定としてシェルに送られる)
                        if !composition_covers(cookie, &composition, text.len()) {
                            let _ = range.SetText(cookie, 0, &[]);
                            lose_composition(cookie, &composition, "set_text: the composition does not cover the text");
                            return Ok(());
                        }
                        TIP_WRITTEN.with(|written| written.set(!text.is_empty()));
                        TIP_OWNED.with(|owned| owned.set(text.len()));

                        // first, set the display attribute to the "text" part
                        let text_range = range.Clone()?;
                        text_range.Collapse(cookie, TF_ANCHOR_START)?;
                        let mut shifted: i32 = 0;
                        text_range.ShiftEnd(cookie, text_len, &mut shifted, std::ptr::null())?;
                        let display_attribute = display_attribute_atom.get(&GUID_DISPLAY_ATTRIBUTE);
                        if let Some(display_attribute) = display_attribute {
                            let pvar = VARIANT::from(*display_attribute as i32);
                            let prop = context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
                            prop.SetValue(cookie, &text_range, &pvar)?;
                        }

                        range.Collapse(cookie, TF_ANCHOR_END)?;
                        let selection = TF_SELECTION {
                            range: ManuallyDrop::new(Some(range.clone())),
                            style: TF_SELECTIONSTYLE {
                                ase: TF_AE_NONE,
                                fInterimChar: false.into(),
                            },
                        };

                        context.SetSelection(cookie, &[selection])?;

                        Ok(())
                    }
                }),
            )?;
        } else {
            tracing::warn!("Composition is not started");
        }

        Ok(())
    }

    /// 未確定文字列を置き換え、部分ごとに下線を付け、キャレットを置く (位置は UTF-16 単位)
    #[tracing::instrument]
    pub fn set_preedit(&self, text: &str, spans: &[PreeditSpan], caret: i32) -> Result<()> {
        // 2026-11-05 (仕事 23): 設定で数字を全角にする人には、見せる・確定する所だけ全角に (UTF-16 の長さは変わらない)
        let fullwidth;
        let text = if crate::engine::full_width::digit_display() != crate::engine::full_width::DigitForm::Off {
            fullwidth = crate::engine::full_width::displayed_digits(text);
            fullwidth.as_str()
        } else {
            text
        };

        // 2026-11-08 (メモ帳で確定した文が消えた件): 編集のセッションを頼む前に借りを手放す
        if let Some((tid, context, composition, display_attribute_atom)) = self.session_parts()? {
            edit_session(
                tid,
                context.clone(),
                Rc::new({
                    let text = text.to_wide_16_unpadded();
                    let spans = spans.to_vec();

                    move |cookie| unsafe {
                        // 2026-11-06: このキーの前の書き込みで未確定の範囲を失っていたら、もう書かない (次のキーで始め直す)
                        if TIP_LOST.with(Cell::get) {
                            return Ok(());
                        }
                        let Ok(range) = composition.GetRange() else {
                            lose_composition(cookie, &composition, "set_preedit: no composition range");
                            return Ok(());
                        };
                        let empty = range.IsEmpty(cookie).map(|empty| empty.as_bool()).unwrap_or(false);
                        // 2026-11-06 (a): 前に字を書いたのに未確定の範囲が空なら、アプリが未確定の字を確定して消している
                        // (Windows Terminal はコンポジションの印の無い字をシェルに送って消す)。字を入れ直さない (入れ直すと、打つたびに途中の形が確定されていく)
                        if empty && !text.is_empty() && TIP_WRITTEN.with(Cell::get) {
                            lose_composition(cookie, &composition, "set_preedit: the composition lost the text written before");
                            return Ok(());
                        }
                        // 2026-11-08 (メモ帳で確定した文が消えた件): 打ち始めも、選択の場所に字を入れず、未確定の範囲に書く。
                        // 10-30 からは選択の場所に入れて未確定の範囲をそこに合わせ、11-06 からは入れた字にコンポジションの印が無ければ
                        // (Chrome・メモ帳・端末で毎回無かった) アプリが返した範囲を丸ごと消して書き直していた。アプリが返す範囲が入れた字だけとは限らず、
                        // 選択が残っていればそこも置き換わる (メモ帳で、句読点の変換の直後の次の字で、前に確定した文が消えた)。
                        // 書く前に、範囲が自分のものより長くないかを確かめる (長ければ書かない・消さない。Cancel の空の書き込みも同じ)
                        let owned_before = TIP_OWNED.with(Cell::get);
                        let (writable, range_before) = guard_write(cookie, &composition, &range, "set_preedit");
                        if !writable {
                            return Ok(());
                        }
                        if let Err(error) = range.SetText(cookie, TF_ST_CORRECTION, &text) {
                            lose_composition(cookie, &composition, &format!("set_preedit: SetText failed: {error:?}"));
                            return Ok(());
                        }
                        // 2026-11-06 (b): 書いた字を未確定の範囲が覆わなければ (入力が終わっている)、書いた字を消して始め直す
                        if !composition_covers(cookie, &composition, text.len()) {
                            let _ = range.SetText(cookie, 0, &[]);
                            lose_composition(cookie, &composition, "set_preedit: the composition does not cover the text");
                            return Ok(());
                        }
                        if !text.is_empty() && empty && crate::debug_log::enabled() {
                            crate::debug_log::write(&format!("set_preedit: wrote into the composition range (range before={range_before:?}, owned before={owned_before}, written={}, composing mark={:?})",
                                text.len(), composing_mark(cookie, &context, &composition)));
                        }
                        TIP_WRITTEN.with(|written| written.set(!text.is_empty()));
                        TIP_OWNED.with(|owned| owned.set(text.len()));
                        // 2026-11-06 (端末の件、ログだけ): 書いた字にコンポジションの印が無ければ書いておく (動作確認のログがオンのときだけ調べる)
                        if !text.is_empty() && crate::debug_log::enabled() && composing_mark(cookie, &context, &composition) == Some(false) {
                            crate::debug_log::write(&format!("set_preedit: the written text has no composing mark (first write={empty})"));
                        }

                        let prop = context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
                        prop.Clear(cookie, &range)?;

                        for span in spans.iter() {
                            if span.len <= 0 {
                                continue;
                            }
                            let guid = match span.kind {
                                SpanKind::Input => &GUID_DISPLAY_ATTRIBUTE_INPUT,
                                SpanKind::LiveDraft => &GUID_DISPLAY_ATTRIBUTE_LIVE_DRAFT,
                                SpanKind::Converted => &GUID_DISPLAY_ATTRIBUTE_CONVERTED,
                                SpanKind::Focused => &GUID_DISPLAY_ATTRIBUTE,
                            };
                            let Some(atom) = display_attribute_atom.get(guid) else {
                                continue;
                            };
                            let span_range = range.Clone()?;
                            span_range.Collapse(cookie, TF_ANCHOR_START)?;
                            let mut shifted: i32 = 0;
                            span_range.ShiftEnd(cookie, span.start + span.len, &mut shifted, std::ptr::null())?;
                            span_range.ShiftStart(cookie, span.start, &mut shifted, std::ptr::null())?;
                            let pvar = VARIANT::from(*atom as i32);
                            prop.SetValue(cookie, &span_range, &pvar)?;
                        }

                        let caret_range = range.Clone()?;
                        caret_range.Collapse(cookie, TF_ANCHOR_START)?;
                        let mut shifted: i32 = 0;
                        caret_range.ShiftStart(cookie, caret, &mut shifted, std::ptr::null())?;
                        caret_range.Collapse(cookie, TF_ANCHOR_START)?;
                        let selection = TF_SELECTION {
                            range: ManuallyDrop::new(Some(caret_range)),
                            style: TF_SELECTIONSTYLE {
                                ase: TF_AE_NONE,
                                fInterimChar: false.into(),
                            },
                        };

                        context.SetSelection(cookie, &[selection])?;

                        Ok(())
                    }
                }),
            )?;
        } else {
            tracing::warn!("Composition is not started");
        }

        Ok(())
    }

    #[tracing::instrument]
    pub fn shift_start(&self, text: &str, subtext: &str) -> Result<()> {
        // 2026-11-08 (メモ帳で確定した文が消えた件): 編集のセッションを頼む前に借りを手放す
        if let Some((tid, context, composition, display_attribute_atom)) = self.session_parts()? {
            edit_session(
                tid,
                context.clone(),
                Rc::new({
                    let text_len = text.chars().count() as i32;
                    let subtext = subtext.to_wide_16_unpadded();

                    move |cookie| unsafe {
                        // 2026-11-08: このキーで未確定の範囲を失っていたら (コンポジションはもう終えた)、字には触らない
                        if TIP_LOST.with(Cell::get) {
                            return Ok(());
                        }
                        // first, shift the start of the composition
                        let range = composition.GetRange()?;
                        let mut shifted: i32 = 0;

                        // and clear the display attribute
                        let prop = context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
                        prop.Clear(cookie, &range)?;

                        range.Collapse(cookie, TF_ANCHOR_START)?;
                        range.ShiftStart(cookie, text_len, &mut shifted, std::ptr::null())?;

                        composition.ShiftStart(cookie, &range)?;

                        // then, set the display attribute
                        let range = composition.GetRange()?;

                        // 2026-11-08: 頭を確定して縮めた範囲が、自分のものより長ければ (自分が書いていない字を覆っている)、書かない・消さない
                        if !guard_write(cookie, &composition, &range, "shift_start").0 {
                            return Ok(());
                        }
                        range.SetText(cookie, TF_ST_CORRECTION, &subtext)?;
                        TIP_WRITTEN.with(|written| written.set(!subtext.is_empty()));
                        TIP_OWNED.with(|owned| owned.set(subtext.len()));

                        let display_attribute = display_attribute_atom.get(&GUID_DISPLAY_ATTRIBUTE);
                        if let Some(display_attribute) = display_attribute {
                            let pvar = VARIANT::from(*display_attribute as i32);
                            let prop = context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
                            prop.SetValue(cookie, &range, &pvar)?;
                        }

                        range.Collapse(cookie, TF_ANCHOR_END)?;
                        let selection = TF_SELECTION {
                            range: ManuallyDrop::new(Some(range)),
                            style: TF_SELECTIONSTYLE {
                                ase: TF_AE_NONE,
                                fInterimChar: false.into(),
                            },
                        };

                        context.SetSelection(cookie, &[selection])?;

                        Ok(())
                    }
                }),
            )?;
        } else {
            tracing::warn!("Composition is not started");
        }

        Ok(())
    }

    #[tracing::instrument]
    pub fn update_pos(&self) -> Result<()> {
        {
            let mut text_service = match self.borrow_mut() {
                Ok(text_service) => text_service,
                Err(error) => {
                    tracing::warn!("Skip update_pos due to borrow conflict: {error:?}");
                    return Ok(());
                }
            };

            if !text_service
                .update_pos_state
                .try_begin_update(Instant::now())
            {
                tracing::debug!("Skip re-entrant update_pos call");
                return Ok(());
            }
        }

        let result: Result<()> = (|| {
            let (tid, context, tip_composition, anchor) = {
                let text_service = self.borrow()?;
                let composition = text_service.borrow_composition()?;
                (
                    text_service.tid,
                    text_service.context::<ITfContext>()?,
                    composition.tip_composition.clone(),
                    text_service.candidate_anchor,
                )
            };

            if let Some(tip_composition) = tip_composition {
                edit_session(
                    tid,
                    context.clone(),
                    Rc::new({
                        let context = context.clone();

                        move |cookie| unsafe {
                            let view = context.GetActiveView()?;
                            let range = tip_composition.GetRange()?;
                            // 変換中は注目文節の先頭の 1 字に合わせる (Mozc の tip_ui_handler_conventional.cc の GetTargetPos と同じ)。
                            // 文節を ←→ で移っても、窓が未確定文字列の頭に残っていた
                            let anchor = anchor.filter(|&(offset, _)| offset > 0);
                            if let Some((offset, _)) = anchor {
                                range.Collapse(cookie, TF_ANCHOR_START)?;
                                let mut shifted: i32 = 0;
                                range.ShiftEnd(cookie, offset + 1, &mut shifted, std::ptr::null())?;
                                range.ShiftStart(cookie, offset, &mut shifted, std::ptr::null())?;
                            }

                            let Some(mut ipc_service) = IMEState::get()?.ipc_service.clone() else {
                                return Ok(());
                            };

                            let mut rect = RECT::default();
                            let mut clipped = false.into();
                            view.GetTextExt(cookie, &range, &mut rect, &mut clipped)?;

                            // 端末は範囲を見ずに、未確定文字列を書き始めたカーソルの 1 マスを返す (Windows Terminal の
                            // TermControl.cpp の TsfDataProvider::GetCursorPosition)。頭の 1 字と同じ矩形が返ってきたら、
                            // 前の文節のマス数だけ右へずらす
                            if let Some((_, cells)) = anchor.filter(|_| crate::engine::prefix_direct::host_is_terminal()) {
                                let head = tip_composition.GetRange()?;
                                head.Collapse(cookie, TF_ANCHOR_START)?;
                                let mut shifted: i32 = 0;
                                head.ShiftEnd(cookie, 1, &mut shifted, std::ptr::null())?;
                                let mut head_rect = RECT::default();
                                view.GetTextExt(cookie, &head, &mut head_rect, &mut clipped)?;
                                let cell = rect.right - rect.left;
                                if head_rect == rect && cell > 0 {
                                    rect.left += cells * cell;
                                    rect.right += cells * cell;
                                }
                            }

                            ipc_service.set_window_position(
                                rect.top,
                                rect.left,
                                rect.bottom,
                                rect.right,
                            )?;

                            Ok(())
                        }
                    }),
                )?;
            }

            Ok(())
        })();

        match self.borrow_mut() {
            Ok(mut text_service) => {
                text_service.update_pos_state.finish_update(Instant::now());
            }
            Err(error) => {
                tracing::warn!("Failed to reset update_pos guard: {error:?}");
            }
        }

        if let Err(error) = result {
            tracing::warn!("Failed to update composition window position: {error:?}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod guard_tests {
    use super::{in_edit_session, may_write_over, SessionDepth};

    /// 2026-11-08 (メモ帳で確定した文が消えた件): 未確定の範囲に書く・消すかの決まり。
    /// 範囲が、自分が書いた字数 (打ち始めは利用者が選んでいた字数) より長ければ、書かない・消さない。読めないアプリは今のまま
    #[test]
    fn writes_only_over_what_the_ime_owns() {
        // 打ち始め: 空の範囲には書く。字のある範囲 (前に確定した文を覆っている) には書かない
        assert!(may_write_over(Some(0), 0));
        assert!(!may_write_over(Some(1), 0));
        assert!(!may_write_over(Some(6), 0));
        // 利用者が選んでいた字の上に打ち始めたら、その字数までは置き換える (今までどおり)
        assert!(may_write_over(Some(4), 4));
        assert!(!may_write_over(Some(5), 4));
        // 打っている途中: 前に書いた字数までなら書く。字数を切るアプリ (短い) も書く。長ければ書かない
        assert!(may_write_over(Some(5), 5));
        assert!(may_write_over(Some(3), 5));
        assert!(!may_write_over(Some(6), 5));
        // Cancel の空の書き込みも同じ決まり (自分が書いた字だけを消す)
        assert!(!may_write_over(Some(11), 5));
        // 読めないアプリは今のまま書く
        assert!(may_write_over(None, 0));
        assert!(may_write_over(None, 5));
    }

    /// 編集のセッションの中か (OnCompositionTerminated がセッションの中から呼ばれたかを見分ける)。入れ子でも、抜けたら戻る
    #[test]
    fn edit_session_depth_is_counted_and_restored() {
        assert!(!in_edit_session());
        {
            let _outer = SessionDepth::enter();
            assert!(in_edit_session());
            {
                let _inner = SessionDepth::enter();
                assert!(in_edit_session());
            }
            assert!(in_edit_session());
        }
        assert!(!in_edit_session());
        // 中で失敗して抜けても戻る
        let result: Result<(), ()> = (|| {
            let _depth = SessionDepth::enter();
            Err(())
        })();
        assert!(result.is_err());
        assert!(!in_edit_session());
    }
}
