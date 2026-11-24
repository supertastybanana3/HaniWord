mod security;
#[cfg(test)]
mod rpc_tests;

use azookey_server::{ConnectionState, TonicNamedPipeServer};
use tonic::{transport::Server, Request, Response, Status};
use shared::proto::azookey_service_server::{AzookeyService, AzookeyServiceServer};
use shared::proto::*;
use std::{collections::HashMap, ffi::{c_char, c_int, CStr, CString}, sync::{Arc, Weak, OnceLock, mpsc}};

const USE_ZENZAI: bool = true;
type Sessions = HashMap<u64, Weak<ConnectionState>>;
type Job = Box<dyn FnOnce(&mut Sessions) + Send>;
static JOBS: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();

fn prune_sessions(sessions: &mut Sessions) {
    sessions.retain(|id, owner| {
        if owner.upgrade().is_some() { true } else {
            unsafe { ReleaseIpcSession(*id); }
            false
        }
    });
}

// Swift stays on the main thread; edit/read/candidate operations are atomic for one connection.
async fn on_main<R: Send + 'static>(owner: Arc<ConnectionState>, f: impl FnOnce() -> Result<R, Status> + Send + 'static) -> Result<R, Status> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let job: Job = Box::new(move |sessions| {
        if tx.is_closed() { return; }
        prune_sessions(sessions);
        sessions.insert(owner.id, Arc::downgrade(&owner));
        let personal = owner.peer.get().is_some_and(|peer| peer.may_read_personal);
        unsafe { SelectIpcSession(owner.id); SetPersonalDataAllowed(personal); }
        let _ = tx.send(f());
    });
    JOBS.get().ok_or_else(|| Status::unavailable("Main queue not ready"))?
        .try_send(job).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => Status::resource_exhausted("Conversion queue full"),
            mpsc::TrySendError::Disconnected(_) => Status::unavailable("Conversion queue closed"),
        })?;
    rx.await.map_err(|_| Status::unavailable("Conversion job cancelled"))?
}

#[repr(C)]
struct FFICandidate {
    text: *mut c_char,
    subtext: *mut c_char,
    hiragana: *mut c_char,
    corresponding_count: c_int,
}

unsafe extern "C" {
    fn Initialize(path: *const c_char, use_zenzai: bool);
    fn SetContext(context: *const c_char);
    fn SetRightContext(context: *const c_char);
    fn AppendText(input: *const c_char, cursor: *mut c_int) -> *mut c_char;
    fn RemoveText(cursor: *mut c_int) -> *mut c_char;
    fn MoveCursor(offset: c_int, cursor: *mut c_int) -> *mut c_char;
    fn ShrinkText(offset: c_int) -> *mut c_char;
    fn ClearText();
    fn GetComposedText(length: *mut c_int) -> *mut *mut FFICandidate;
    fn LoadConfig();
    fn DeleteForward(cursor: *mut c_int) -> *mut c_char;
    fn AppendTextDirect(input: *const c_char, cursor: *mut c_int) -> *mut c_char;
    fn GetRawInput() -> *mut c_char;
    fn LearnSegments(readings: *const c_char, texts: *const c_char, context: *const c_char, allow_new: bool, save_now: bool) -> i32;
    fn SaveLearning();
    fn ListSingleKanji(hiragana: *const c_char, context: *const c_char, length: *mut c_int) -> *mut *mut FFICandidate;
    fn ConvertSegments(hiragana: *const c_char, context: *const c_char, fast: bool, dictionary: bool, length: *mut c_int) -> *mut *mut FFICandidate;
    fn ScoreReadings(readings: *const c_char, context: *const c_char) -> *mut c_char;
    fn ForgetLearned(reading: *const c_char, text: *const c_char) -> i32;
    fn ResetLearning(include_tables: bool) -> *mut c_char;
    fn SetPlainNotation(plain: bool);
    // 2026-11-08 (仕事 52): 次の変換が清書か (重いときに AI を休んでいる間、戻ったかの試しに使ってよい) と、直前の変換を AI を休んで辞書だけでしたか
    fn SetConvertRefine(refine: bool);
    fn LastConversionAIPaused() -> bool;
    fn SetCandidateNotes(wanted: bool);
    fn ListCandidates(hiragana: *const c_char, context: *const c_char, length: *mut c_int) -> *mut *mut FFICandidate;
    fn SelectIpcSession(connection: u64);
    fn SetPersonalDataAllowed(allowed: bool);
    fn ReleaseIpcSession(connection: u64);
    fn FreeText(text: *mut c_char);
    fn FreeCandidates(candidates: *mut *mut FFICandidate, length: c_int);
}

fn cstring(value: &str) -> Result<CString, Status> {
    CString::new(value).map_err(|_| Status::invalid_argument("NUL is not allowed"))
}

unsafe fn borrowed_string(pointer: *const c_char) -> Result<String, Status> {
    if pointer.is_null() { return Err(Status::internal("Missing native text")); }
    CStr::from_ptr(pointer).to_str().map(str::to_owned).map_err(|_| Status::internal("Invalid native UTF-8"))
}

fn owned_string(pointer: *mut c_char) -> Result<String, Status> {
    let result = unsafe { borrowed_string(pointer) };
    unsafe { FreeText(pointer); }
    result
}

struct CandidateBuffer { pointer: *mut *mut FFICandidate, length: c_int }
impl Drop for CandidateBuffer {
    fn drop(&mut self) { unsafe { FreeCandidates(self.pointer, self.length); } }
}

fn read_candidates(pointer: *mut *mut FFICandidate, length: c_int) -> Result<Vec<Suggestion>, Status> {
    if !(0..=4096).contains(&length) || (length > 0 && pointer.is_null()) { return Err(Status::internal("Invalid native candidate array")); }
    let buffer = CandidateBuffer { pointer, length };
    let mut result = Vec::with_capacity(length as usize);
    let mut bytes = 0;
    for index in 0..length as usize {
        let candidate = unsafe { buffer.pointer.add(index).read().as_ref() }.ok_or_else(|| Status::internal("Missing candidate"))?;
        let text = unsafe { borrowed_string(candidate.text)? };
        let subtext = unsafe { borrowed_string(candidate.subtext)? };
        bytes += text.len() + subtext.len();
        if bytes > 2 * 1024 * 1024 { return Err(Status::resource_exhausted("Candidate output too large")); }
        result.push(Suggestion { text, subtext, corresponding_count: candidate.corresponding_count });
    }
    Ok(result)
}

fn raw_input() -> Result<String, Status> { owned_string(unsafe { GetRawInput() }) }

fn check_append(input: &str) -> Result<(), Status> {
    let raw = raw_input()?;
    if raw.len() + input.len() > security::MAX_TEXT_BYTES || raw.chars().count() + input.chars().count() > security::MAX_INPUT_CHARS {
        return Err(Status::resource_exhausted("Composition limit exceeded"));
    }
    Ok(())
}

fn edit(op: edit_text_request::Op, text: &str, offset: i32) -> Result<(String, i32), Status> {
    let mut cursor = 0;
    let pointer = match op {
        edit_text_request::Op::Insert | edit_text_request::Op::InsertDirect => {
            check_append(text)?;
            let input = cstring(text)?;
            unsafe { if op == edit_text_request::Op::Insert { AppendText(input.as_ptr(), &mut cursor) } else { AppendTextDirect(input.as_ptr(), &mut cursor) } }
        }
        edit_text_request::Op::DeleteBackward => unsafe { RemoveText(&mut cursor) },
        edit_text_request::Op::DeleteForward => unsafe { DeleteForward(&mut cursor) },
        edit_text_request::Op::MoveCursor => unsafe { MoveCursor(offset, &mut cursor) },
    };
    Ok((owned_string(pointer)?, cursor))
}

fn suggestions() -> Result<Vec<Suggestion>, Status> {
    let mut count = 0;
    let pointer = unsafe { GetComposedText(&mut count) };
    let mut result = Vec::new();
    for candidate in read_candidates(pointer, count)? {
        if !result.iter().any(|previous: &Suggestion| previous.text == candidate.text) { result.push(candidate); }
    }
    Ok(result)
}

#[derive(Debug, Default)]
pub struct MyAzookeyService;

#[tonic::async_trait]
impl AzookeyService for MyAzookeyService {
    async fn append_text(&self, request: Request<AppendTextRequest>) -> Result<Response<AppendTextResponse>, Status> {
        let owner = security::peer(&request, false)?;
        let text = request.into_inner().text_to_append;
        security::input(&text)?;
        let composing = on_main(owner, move || {
            let (hiragana, _) = edit(edit_text_request::Op::Insert, &text, 0)?;
            Ok(ComposingText { hiragana, suggestions: suggestions()? })
        }).await?;
        Ok(Response::new(AppendTextResponse { composing_text: Some(composing) }))
    }

    async fn remove_text(&self, request: Request<RemoveTextRequest>) -> Result<Response<RemoveTextResponse>, Status> {
        let owner = security::peer(&request, false)?;
        let composing = on_main(owner, || {
            let (hiragana, _) = edit(edit_text_request::Op::DeleteBackward, "", 0)?;
            Ok(ComposingText { hiragana, suggestions: suggestions()? })
        }).await?;
        Ok(Response::new(RemoveTextResponse { composing_text: Some(composing) }))
    }

    async fn move_cursor(&self, request: Request<MoveCursorRequest>) -> Result<Response<MoveCursorResponse>, Status> {
        let owner = security::peer(&request, false)?;
        let offset = request.into_inner().offset;
        security::offset(offset, false)?;
        let composing = on_main(owner, move || {
            let (hiragana, _) = edit(edit_text_request::Op::MoveCursor, "", offset)?;
            Ok(ComposingText { hiragana, suggestions: suggestions()? })
        }).await?;
        Ok(Response::new(MoveCursorResponse { composing_text: Some(composing) }))
    }

    async fn shrink_text(&self, request: Request<ShrinkTextRequest>) -> Result<Response<ShrinkTextResponse>, Status> {
        let owner = security::peer(&request, false)?;
        let offset = request.into_inner().offset;
        security::offset(offset, true)?;
        let composing = on_main(owner, move || {
            let hiragana = owned_string(unsafe { ShrinkText(offset) })?;
            Ok(ComposingText { hiragana, suggestions: suggestions()? })
        }).await?;
        Ok(Response::new(ShrinkTextResponse { composing_text: Some(composing) }))
    }

    async fn clear_text(&self, request: Request<ClearTextRequest>) -> Result<Response<ClearTextResponse>, Status> {
        let owner = security::peer(&request, false)?;
        on_main(owner, || { unsafe { ClearText(); } Ok(()) }).await?;
        Ok(Response::new(ClearTextResponse {}))
    }

    async fn set_context(&self, request: Request<SetContextRequest>) -> Result<Response<SetContextResponse>, Status> {
        let owner = security::peer(&request, false)?;
        let request = request.into_inner();
        let context = request.context;
        security::text(&context, security::MAX_CONTEXT_BYTES)?;
        let context = cstring(context.split('\r').filter(|s| !s.is_empty()).last().unwrap_or_default())?;
        // 2026-11-08 (仕事 45): カーソルの右の字。最初の改行より前だけ (変換サーバーの DLL も 20 字・最初の改行までに切る)。空なら右の字を消す
        let right = request.right_context;
        security::text(&right, security::MAX_CONTEXT_BYTES)?;
        let right = cstring(right.split(['\r', '\n']).next().unwrap_or_default())?;
        on_main(owner, move || { unsafe { SetContext(context.as_ptr()); SetRightContext(right.as_ptr()); } Ok(()) }).await?;
        Ok(Response::new(SetContextResponse {}))
    }

    async fn edit_text(&self, request: Request<EditTextRequest>) -> Result<Response<EditTextResponse>, Status> {
        let owner = security::peer(&request, false)?;
        let request = request.into_inner();
        let op = edit_text_request::Op::try_from(request.op).map_err(|_| Status::invalid_argument("Unknown edit operation"))?;
        security::input(&request.text)?;
        security::offset(request.offset, false)?;
        let response = on_main(owner, move || {
            let (hiragana, cursor) = edit(op, &request.text, request.offset)?;
            Ok(EditTextResponse { hiragana, cursor, raw: raw_input()? })
        }).await?;
        Ok(Response::new(response))
    }

    async fn convert(&self, request: Request<ConvertRequest>) -> Result<Response<ConvertResponse>, Status> {
        let owner = security::peer(&request, false)?;
        let request = request.into_inner();
        security::input(&request.hiragana)?;
        security::text(&request.context, security::MAX_CONTEXT_BYTES)?;
        let hiragana = cstring(&request.hiragana)?;
        let context = cstring(&request.context)?;
        let (segments, ai_paused) = on_main(owner, move || {
            let mut count = 0;
            unsafe { SetPlainNotation(request.plain); SetConvertRefine(request.refine); }
            let pointer = unsafe { ConvertSegments(hiragana.as_ptr(), context.as_ptr(), request.fast, request.dictionary_only, &mut count) };
            let paused = unsafe { LastConversionAIPaused() };
            let segments: Vec<Segment> = read_candidates(pointer, count)?.into_iter().map(|candidate| Segment { text: candidate.text, length: candidate.corresponding_count }).collect();
            Ok((segments, paused))
        }).await?;
        Ok(Response::new(ConvertResponse { segments, ai_paused }))
    }

    async fn list_candidates(&self, request: Request<ListCandidatesRequest>) -> Result<Response<ListCandidatesResponse>, Status> {
        let owner = security::peer(&request, false)?;
        let request = request.into_inner();
        security::input(&request.hiragana)?;
        security::text(&request.context, security::MAX_CONTEXT_BYTES)?;
        let hiragana = cstring(&request.hiragana)?;
        let context = cstring(&request.context)?;
        let (candidates, notes, learned) = on_main(owner, move || {
            let mut count = 0;
            unsafe { SetPlainNotation(request.plain); SetCandidateNotes(request.notes && !request.single_kanji); }
            let pointer = unsafe { if request.single_kanji { ListSingleKanji(hiragana.as_ptr(), context.as_ptr(), &mut count) } else { ListCandidates(hiragana.as_ptr(), context.as_ptr(), &mut count) } };
            // subtext は候補窓の注記 (misuse.swift。「種類<TAB>注記」)。2026-11-08: corresponding_count が 1 なら学習した語を含む候補 (一覧では長さを使わない)
            let mut lists = (Vec::new(), Vec::new(), Vec::new());
            for candidate in read_candidates(pointer, count)? {
                lists.2.push(candidate.corresponding_count == 1);
                lists.0.push(candidate.text);
                lists.1.push(candidate.subtext);
            }
            Ok(lists)
        }).await?;
        Ok(Response::new(ListCandidatesResponse { candidates, notes, learned }))
    }

    async fn learn(&self, request: Request<LearnRequest>) -> Result<Response<LearnResponse>, Status> {
        let owner = security::peer(&request, true)?;
        let request = request.into_inner();
        security::learning(&request)?;
        let readings = cstring(&request.readings.join("\u{1f}"))?;
        let texts = cstring(&request.texts.join("\u{1f}"))?;
        let context = cstring(&request.context)?;
        let learned = on_main(owner, move || Ok(unsafe { LearnSegments(readings.as_ptr(), texts.as_ptr(), context.as_ptr(), true, false) })).await?;
        Ok(Response::new(LearnResponse { learned }))
    }

    /// 2026-10-29: 候補の一覧で選んでいる候補が学習した語なら忘れる (Ctrl+Delete)。辞書の語は何もしない。学習の表はその場で書き直す
    async fn forget(&self, request: Request<ForgetRequest>) -> Result<Response<ForgetResponse>, Status> {
        let owner = security::peer(&request, true)?;
        let request = request.into_inner();
        security::forgetting(&request)?;
        let reading = cstring(&request.reading)?;
        let text = cstring(&request.text)?;
        let forgotten = on_main(owner, move || Ok(unsafe { ForgetLearned(reading.as_ptr(), text.as_ptr()) })).await?;
        Ok(Response::new(ForgetResponse { forgotten }))
    }

    /// 2026-11-08: 学習をまっさらにする (設定の画面から)。消す前に控えを取る (learning_reset.swift)。呼べるのは、学習・設定を変えてよい接続だけ (Learn・Forget と同じ)
    async fn reset_learning(&self, request: Request<ResetLearningRequest>) -> Result<Response<ResetLearningResponse>, Status> {
        let owner = security::peer(&request, true)?;
        let include_tables = request.into_inner().include_tables;
        let backup = on_main(owner, move || owned_string(unsafe { ResetLearning(include_tables) })).await?;
        if backup.is_empty() {
            return Err(Status::internal("Could not back up the learning data; nothing was removed"));
        }
        Ok(Response::new(ResetLearningResponse { backup }))
    }

    /// 2026-10-28: ローマ字の母音の抜けを直すときの、補った読みの候補の採点 (typo_repair.swift)
    async fn score_readings(&self, request: Request<ScoreReadingsRequest>) -> Result<Response<ScoreReadingsResponse>, Status> {
        let owner = security::peer(&request, false)?;
        let request = request.into_inner();
        if request.readings.is_empty() || request.readings.len() > 16 { return Err(Status::invalid_argument("1 to 16 readings")); }
        for reading in &request.readings {
            security::input(reading)?;
            if reading.contains('\u{1}') { return Err(Status::invalid_argument("Invalid reading")); }
        }
        security::text(&request.context, security::MAX_CONTEXT_BYTES)?;
        let readings = cstring(&request.readings.join("\u{1}"))?;
        let context = cstring(&request.context)?;
        let count = request.readings.len();
        let rows = on_main(owner, move || owned_string(unsafe { ScoreReadings(readings.as_ptr(), context.as_ptr()) })).await?;
        let mut response = ScoreReadingsResponse::default();
        for row in rows.split('\u{1}').take(count) {
            let (dict, lm) = row.split_once('\t').unwrap_or((row, ""));
            let dict = dict.parse::<f32>().ok().filter(|v| v.is_finite());
            let lm = lm.parse::<f32>().ok().filter(|v| v.is_finite());
            response.dict.push(dict.unwrap_or(0.0));
            response.has_dict.push(dict.is_some());
            response.lm.push(lm.unwrap_or(0.0));
            response.has_lm.push(lm.is_some());
        }
        if response.dict.len() != count { return Err(Status::internal("Invalid native scores")); }
        Ok(Response::new(response))
    }

    async fn update_config(&self, request: Request<UpdateConfigRequest>) -> Result<Response<UpdateConfigResponse>, Status> {
        let owner = security::peer(&request, true)?;
        on_main(owner, || { unsafe { LoadConfig(); } Ok(()) }).await?;
        Ok(Response::new(UpdateConfigResponse {}))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 2026-11-10 (Fable の諮問 perf 4): 落ちたときに Windows エラー報告がこのプロセスのヒープ (打っている文・前後の文脈・学習) を集めないように。
    // 何も読み込む前に。設定できなくても変換は続ける
    if let Err(error) = shared::winsec::exclude_heap_from_error_reports() {
        eprintln!("Could not exclude the heap from error reports: {error:?}");
    }
    if shared::winsec::elevation_status()? { return Err("Conversion server must run unelevated".into()); }
    shared::winsec::allow_pipe_identity_queries()?;
    let current_exe = std::env::current_exe()?;
    #[cfg(not(feature = "security-probe"))]
    let pipe_base = "haniword_server".to_owned();
    #[cfg(feature = "security-probe")]
    let pipe_base = {
        let base = std::env::var("AZK_TEST_PIPE")?;
        if !base.starts_with("haniword_rpc_test_") || base.len() > 80
            || !base.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !current_exe.parent().ok_or("Missing directory")?.join("isolated_security_fixture.json").is_file() {
            return Err("Security probe requires an isolated fixture and a test-only pipe".into());
        }
        base
    };
    let path = CString::new(current_exe.parent().ok_or("Missing executable directory")?.to_str().ok_or("Invalid executable path")?)?;
    unsafe { Initialize(path.as_ptr(), USE_ZENZAI); }

    let (tx, rx) = mpsc::sync_channel::<Job>(security::MAX_JOBS);
    JOBS.set(tx).map_err(|_| "Main queue already initialized")?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.spawn(async move {
        let service = AzookeyServiceServer::new(MyAzookeyService)
            .max_decoding_message_size(security::MAX_MESSAGE_BYTES)
            .max_encoding_message_size(2 * 1024 * 1024);
        let result = Server::builder()
            .concurrency_limit_per_connection(4)
            .max_concurrent_streams(4)
            .timeout(std::time::Duration::from_secs(60))
            .add_service(service)
            .serve_with_incoming(TonicNamedPipeServer::new(&pipe_base))
            .await;
        eprintln!("HaniWord server stopped: {result:?}");
        std::process::exit(1);
    });

    let mut sessions = Sessions::new();
    let mut jobs_since_save = 0u32;
    loop {
        match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok(job) => { job(&mut sessions); jobs_since_save = jobs_since_save.saturating_add(1); }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                prune_sessions(&mut sessions);
                if jobs_since_save > 0 { unsafe { SaveLearning(); } jobs_since_save = 0; }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}
