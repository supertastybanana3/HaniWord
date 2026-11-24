// Google 日本語入力 (Mozc) と同じ操作体系の変換エンジン。
//
//   入力中 (Composing):  ひらがなを点線の下線で表示。Space で変換、Enter でひらがなのまま確定
//   変換中 (Converting): 文節ごとに表示し、注目文節に太い下線。
//                        Space/↓ で次候補 (2 回目から候補ウィンドウ)、←→ で文節移動、
//                        Shift+←→ で文節の区切りを伸び縮み、Enter で確定、Esc/Backspace で入力中に戻る

use crate::{
    engine::user_action::UserAction,
    extension::VKeyExt as _,
    tsf::{
        edit_session::{PreeditSpan, SpanKind},
        factory::{TextServiceFactory, TextServiceFactory_Impl},
    },
};

use super::{
    client_action::{ClientAction, SetTextType},
    full_width::{to_fullwidth, to_halfwidth},
    input_mode::InputMode,
    ipc_service::IPCService,
    state::IMEState,
    text_util::{to_half_katakana, to_katakana, to_romaji},
    user_action::{Function, Navigation},
};
use shared::proto::edit_text_request::Op;
use windows::Win32::{
    Foundation::WPARAM,
    UI::{
        Input::KeyboardAndMouse::{VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN},
        TextServices::{ITfComposition, ITfCompositionSink_Impl, ITfContext},
    },
};

use anyhow::{Context, Result};

const PAGE_SIZE: i32 = 9;

/// 候補一覧の最後に置く項目。選んで Enter すると単漢字の一覧になる (MS-IME と同じ)
const SINGLE_KANJI_MARKER: &str = "単漢字…";

/// ライブ変換で、手が止まってから Zenzai で変換し直すまでの時間
const LIVE_REFINE_DELAY_MS: u32 = 200;

thread_local! {
    static LIVE_TIMER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// 時計を仕掛けた時刻と、もう数え直したか (timer_fire)
    static LIVE_ARMED: std::cell::Cell<Option<(std::time::Instant, bool)>> = const { std::cell::Cell::new(None) };
    static LIVE_TARGET: std::cell::RefCell<Option<windows::Win32::UI::TextServices::ITfTextInputProcessor>> =
        const { std::cell::RefCell::new(None) };
}

/// 2026-11-06 (仕事 29 の 2): 上限で切った清書の答えが届いたかを見る間隔 (届いて読みが同じなら仮表示を清書に替える。ipc_service::poll_late_refine)
const LATE_REFINE_POLL_MS: u32 = 100;

thread_local! {
    static LATE_TIMER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// 届いた遅れた清書 (読み, 文節ごとの (表記, 読みの文字数))。ApplyLateRefine で使い切る
    static LATE_RESULT: std::cell::RefCell<Option<(String, Vec<(String, i32)>)>> = const { std::cell::RefCell::new(None) };
}

fn arm_late_refine_timer() {
    use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};
    let old = LATE_TIMER.with(|t| t.replace(0));
    if old != 0 {
        unsafe {
            let _ = KillTimer(None, old);
        }
    }
    let id = unsafe { SetTimer(None, 0, LATE_REFINE_POLL_MS, Some(late_refine_timer_proc)) };
    LATE_TIMER.with(|t| t.set(id));
}

fn cancel_late_refine_timer() {
    let id = LATE_TIMER.with(|t| t.replace(0));
    if id != 0 {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(None, id);
        }
    }
}

extern "system" fn late_refine_timer_proc(
    _hwnd: windows::Win32::Foundation::HWND,
    _msg: u32,
    _id: usize,
    _time: u32,
) {
    use windows::core::AsImpl as _;
    crate::guard::protect("late_refine_timer_proc", || {
        match super::ipc_service::poll_late_refine() {
            super::ipc_service::LateRefineState::Pending => {}
            super::ipc_service::LateRefineState::Nothing => cancel_late_refine_timer(),
            super::ipc_service::LateRefineState::Ready(reading, converted) => {
                cancel_late_refine_timer();
                LATE_RESULT.with(|r| *r.borrow_mut() = Some((reading, converted)));
                let _budget = super::ipc_service::EventBudget::start();
                let target = LIVE_TARGET.with(|t| t.borrow().clone());
                if let Some(tip) = target {
                    let factory: &TextServiceFactory = unsafe { tip.as_impl() };
                    if let Err(error) = factory.apply_late_refine() {
                        crate::debug_log::write(&format!("apply_late_refine failed: {error:?}"));
                    }
                }
                LATE_RESULT.with(|r| r.borrow_mut().take());
            }
        }
    });
}

/// 遅れて届いた清書を、今の読みに使ってよいか: 入力中で、ライブ変換の仮表示のまま (清書していない)、キャレットが末尾、読みが同じ。
/// 2026-11-19: Esc で読みに戻している間 (live_reading) は使わない (使うと、かなが見えたまま Enter で漢字が確定し、学習もされた)
fn late_refine_applies(comp: &Composition, reading: &str) -> bool {
    comp.state == CompositionState::Composing && comp.live && !comp.refined && !comp.edit_unit && !comp.live_reading
        && comp.caret >= comp.hiragana_len() && !comp.hiragana.is_empty() && live_refinement_reading(comp, false) == reading
}

/// 清書した変換を表示に使う (時計の清書・遅れて届いた清書・Space の前の清書)
fn set_refined(comp: &mut Composition, reading: String, converted: Vec<(String, i32)>) {
    comp.segments = build_segments(&reading, converted);
    comp.segments_reading = reading;
    comp.refined = true;
    // 2026-10-29 (試験中、設定 input.live_keep_refined): 清書した前のほう (最後の文節を除く) を、次の打鍵からは辞書の結果に戻さない
    comp.live_settled = if live_options().0 { comp.segments.len().saturating_sub(1) } else { 0 };
}

/// 予測変換で、手が止まってから候補を出すまでの時間
const PREDICT_DELAY_MS: u32 = 150;

thread_local! {
    static PREDICT_TIMER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PREDICT_ARMED: std::cell::Cell<Option<(std::time::Instant, bool)>> = const { std::cell::Cell::new(None) };
}

/// 2026-10-22: 時計がこれより遅れて鳴ったら、スレッドが (アプリが長い行を並べ直すなどで) ふさがっていたとみなす。
/// ふさがっていた時間は「手が止まっていた」に数えず、鳴った時点から 1 回だけ数え直す
/// (改行の無い長い欄では並べ直しが 200ms を超え、打っている途中の毎キーに清書が割り込んで、1 キーで 2 回並べ直していた)
const TIMER_LATE_MS: u128 = 50;

#[derive(Debug, PartialEq)]
enum TimerFire {
    Run,
    Rearm,
}

/// 鳴った時計をどうするか。elapsed_ms は仕掛けてから鳴るまで、rearmed はもう 1 回数え直したか
fn timer_fire(elapsed_ms: u128, delay_ms: u32, rearmed: bool) -> TimerFire {
    if !rearmed && elapsed_ms > u128::from(delay_ms) + TIMER_LATE_MS { TimerFire::Rearm } else { TimerFire::Run }
}

/// 2026-10-22: ライブ変換の清書の時計を (掛け直して) 仕掛けるか。清書の時計は「最後のキーから」数える。
/// drafted はこのキーで辞書の仮表示を作ったか。仮表示がまだ清書されていなければ、ローマ字の打ちかけ (子音のキー) でも掛け直す
/// (前は読みが変わったときだけ仕掛けたので、子音を打った直後に清書が割り込んでいた)。
/// 時計から呼ばれた動作 (清書そのもの) では掛け直さない (清書できない読みで時計が回り続けないように)
fn arms_live_refine(drafted: bool, refined: bool, has_draft: bool, from_timer: bool) -> bool {
    drafted || (!from_timer && !refined && has_draft)
}

/// 2026-11-10: Esc で読みに戻した表示 (Composition::live_reading) を、この動きで終えるか。キーから来た動き (時計の清書・予測でない) が
/// 1 つでもあり、読みに戻す動き (ShowReading) でなければ終える。何もしないキー (入力中の ↑・PageUp) は動きが空なので、読みのまま
fn ends_reading_shown(actions: &[ClientAction], from_timer: bool) -> bool {
    !from_timer && !actions.is_empty() && !actions.iter().any(|action| matches!(action, ClientAction::ShowReading))
}

fn arm_live_timer(rearmed: bool) {
    let id = unsafe { windows::Win32::UI::WindowsAndMessaging::SetTimer(None, 0, LIVE_REFINE_DELAY_MS, Some(live_timer_proc)) };
    LIVE_TIMER.with(|t| t.set(id));
    LIVE_ARMED.with(|t| t.set(Some((std::time::Instant::now(), rearmed))));
}

fn arm_predict_timer(rearmed: bool) {
    let id = unsafe { windows::Win32::UI::WindowsAndMessaging::SetTimer(None, 0, PREDICT_DELAY_MS, Some(predict_timer_proc)) };
    PREDICT_TIMER.with(|t| t.set(id));
    PREDICT_ARMED.with(|t| t.set(Some((std::time::Instant::now(), rearmed))));
}

/// 2026-10-19: 英数モードで字を打ってから、アプリに入った字を読んで次の語の候補を出すまでの時間
const LATIN_SUGGEST_DELAY_MS: u32 = 40;

thread_local! {
    static LATIN_TIMER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn cancel_latin_suggest() {
    let id = LATIN_TIMER.with(|t| t.replace(0));
    if id != 0 {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(None, id);
        }
    }
}

extern "system" fn latin_timer_proc(
    _hwnd: windows::Win32::Foundation::HWND,
    _msg: u32,
    id: usize,
    _time: u32,
) {
    use windows::core::AsImpl as _;
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(None, id);
    }
    LATIN_TIMER.with(|t| t.set(0));
    crate::guard::protect("latin_timer_proc", || {
        // 2026-10-29: 時計から候補窓に頼むときも、待つ時間の合計は EVENT_BUDGET まで (レビュー L4)
        let _budget = super::ipc_service::EventBudget::start();
        let target = LIVE_TARGET.with(|t| t.borrow().clone());
        if let Some(tip) = target {
            let factory: &TextServiceFactory = unsafe { tip.as_impl() };
            if let Err(error) = factory.show_latin_suggestions() {
                crate::debug_log::write(&format!("show_latin_suggestions failed: {error:?}"));
            }
        }
    });
}

/// 英数モードの次の語の候補を閉じる (フォーカスが移ったとき・ほかのキー・モードの切り替え)
pub(crate) fn hide_latin_suggestions() {
    cancel_latin_suggest();
    if super::next_word::latin_state().is_some() {
        super::next_word::set_latin_state(None);
        if let Ok(state) = IMEState::get() {
            if let Some(mut ipc) = state.ipc_service.clone() {
                drop(state);
                let _ = ipc.hide_window();
            }
        }
    }
}

/// 英数モードの候補の一覧 (一番上に Tab の印。補ったあとは補った語を選んだ状態)
fn latin_candidate_list(state: &super::next_word::LatinState) -> Vec<String> {
    state.list.iter().enumerate()
        .map(|(i, w)| if i == 0 && state.pick.is_none() { format!("{w}\u{1f}Tab") } else { format!("{w}\u{1f}") })
        .collect()
}

fn cancel_predict() {
    PREDICT_ARMED.with(|t| t.set(None));
    let id = PREDICT_TIMER.with(|t| t.replace(0));
    if id != 0 {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(None, id);
        }
    }
}

extern "system" fn predict_timer_proc(
    _hwnd: windows::Win32::Foundation::HWND,
    _msg: u32,
    id: usize,
    _time: u32,
) {
    use windows::core::AsImpl as _;
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(None, id);
    }
    PREDICT_TIMER.with(|t| t.set(0));
    // アプリがふさがっていて遅れて鳴ったら、そこから数え直す (1 回だけ)
    if let Some((armed, rearmed)) = PREDICT_ARMED.with(|t| t.take()) {
        if timer_fire(armed.elapsed().as_millis(), PREDICT_DELAY_MS, rearmed) == TimerFire::Rearm {
            arm_predict_timer(true);
            return;
        }
    }
    // タイマーの入口もパニックを受け止める (越えるとアプリごと落ちる)
    crate::guard::protect("predict_timer_proc", || {
        let target = LIVE_TARGET.with(|t| t.borrow().clone());
        if let Some(tip) = target {
            let factory: &TextServiceFactory = unsafe { tip.as_impl() };
            if let Err(error) = factory.show_predictions() {
                crate::debug_log::write(&format!("show_predictions failed: {error:?}"));
            }
        }
    });
}

fn cancel_live_refine() {
    use windows::Win32::UI::WindowsAndMessaging::KillTimer;
    LIVE_ARMED.with(|t| t.set(None));
    let id = LIVE_TIMER.with(|t| t.replace(0));
    if id != 0 {
        unsafe {
            let _ = KillTimer(None, id);
        }
    }
}

extern "system" fn live_timer_proc(
    _hwnd: windows::Win32::Foundation::HWND,
    _msg: u32,
    id: usize,
    _time: u32,
) {
    use windows::core::AsImpl as _;
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(None, id);
    }
    LIVE_TIMER.with(|t| t.set(0));
    // アプリがふさがっていて遅れて鳴ったら、そこから数え直す (1 回だけ)
    if let Some((armed, rearmed)) = LIVE_ARMED.with(|t| t.take()) {
        if timer_fire(armed.elapsed().as_millis(), LIVE_REFINE_DELAY_MS, rearmed) == TimerFire::Rearm {
            arm_live_timer(true);
            return;
        }
    }
    crate::guard::protect("live_timer_proc", || {
        let target = LIVE_TARGET.with(|t| t.borrow().clone());
        if let Some(tip) = target {
            let factory: &TextServiceFactory = unsafe { tip.as_impl() };
            if let Err(error) = factory.refine_live() {
                crate::debug_log::write(&format!("refine_live failed: {error:?}"));
            }
        }
    });
}

/// 2026-10-14 (脆弱性の再調査 #3): 管理者権限・隔離されたアプリの中で確定した文は、サーバーに学習させない
/// (学習データはふだんの権限で読める %APPDATA% に残るため)。変換はそのまま使う
/// 2026-10-14 19:30: アプリが IS_PRIVATE を付けた欄 (パスワード欄・シークレットの窓) でも学習しない (direct_field.rs)
fn may_learn() -> bool {
    static ALLOWED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ALLOWED.get_or_init(shared::winsec::private_data_allowed) && !super::direct_field::private_field()
}

/// キーの動作のうち、打った文字 (英数字・記号) そのもの。サーバーが返事をしないときに、これをそのまま入れる
fn raw_typed(actions: &[ClientAction]) -> String {
    actions.iter().filter_map(|action| match action {
        ClientAction::Insert(text) if !text.is_empty() && text.chars().all(|c| c.is_ascii_graphic()) => Some(text.as_str()),
        _ => None,
    }).collect()
}

/// 2026-10-22: サーバーが返事をしない間に、IME 本体だけで打つキー
#[derive(Debug, Clone, Copy, PartialEq)]
enum LocalKey {
    Char(char),
    Enter,
    Space,
    Backspace,
    Escape,
    Other,
}

/// local_kana_step の結果: 確定して入れる字・残す打ちかけのローマ字・キーを受けるか (false ならアプリへそのまま渡す)
#[derive(Debug, PartialEq)]
struct LocalKana {
    commit: String,
    pending: String,
    eaten: bool,
}

/// 2026-10-22: サーバーが返事をしない間の 1 キー。変換はせず、サーバーと同じローマ字の表 (romaji_style::simulate) でひらがなにする。
/// かなになった字はすぐ確定し、末尾の打ちかけの英字 (k・ky・n) だけを残す。記号・数字は全角の表 (to_fullwidth) と記号の設定で入れる。
/// 打ちかけが無いときの Enter・Space・Backspace・Escape・ほかのキーは、今までどおりアプリへ渡す
fn local_kana_step(pending: &str, key: LocalKey) -> LocalKana {
    match key {
        LocalKey::Char(c) if c.is_ascii_alphabetic() => {
            // 打ちかけは 8 字まで。超えたら打ったまま確定して、この字から数え直す (かなにならない英字を打ち続けても毎キー全部を数え直さない。D の 2 回目の調べ R2-8)
            if pending.chars().count() >= 8 {
                let next = local_kana_step("", key);
                return LocalKana { commit: flush_local_romaji(pending) + &next.commit, ..next };
            }
            let units = super::romaji_style::simulate(&format!("{pending}{c}"));
            let keep = units.iter().rev().take_while(|u| u.pending && u.kana.chars().all(|k| k.is_ascii_alphabetic())).count();
            let (done, rest) = units.split_at(units.len() - keep);
            LocalKana { commit: done.iter().map(|u| u.kana.as_str()).collect(), pending: rest.iter().map(|u| u.keys.as_str()).collect(), eaten: true }
        }
        LocalKey::Char(c) => LocalKana {
            commit: flush_local_romaji(pending) + &apply_character_form(&to_fullwidth(&c.to_string(), false)),
            pending: String::new(),
            eaten: true,
        },
        _ if pending.is_empty() => LocalKana { commit: String::new(), pending: String::new(), eaten: false },
        LocalKey::Backspace => {
            let mut rest = pending.to_string();
            rest.pop();
            LocalKana { commit: String::new(), pending: rest, eaten: true }
        }
        LocalKey::Escape => LocalKana { commit: String::new(), pending: String::new(), eaten: true },
        LocalKey::Enter | LocalKey::Space | LocalKey::Other => LocalKana { commit: flush_local_romaji(pending), pending: String::new(), eaten: true },
    }
}

/// 打ちかけのローマ字を確定するときの形 (末尾の n は ん。ほかの英字は打ったまま)
fn flush_local_romaji(pending: &str) -> String {
    if pending == "n" { "ん".to_string() } else { pending.to_string() }
}

#[derive(Default, Clone, PartialEq, Debug)]
pub enum CompositionState {
    #[default]
    None,
    Composing,
    Converting,
}

#[derive(Default, Clone, Debug)]
pub struct Segment {
    pub reading: String,
    pub text: String,
    pub candidates: Vec<String>,
    pub index: i32,
    /// 無変換キーの巡回順 (最初に押したときの表記で決める。半角カタカナはいつも最後)
    pub kana_cycle: Vec<String>,
    /// 2026-10-22: 一覧を作ったとき、Windows の変換の候補が時間切れで来なかった (一覧は仮)。次に候補を動かしたときに 1 回だけ問い直す
    pub windows_pending: bool,
    /// 2026-10-22 (M3.6): 変換サーバーの注記 (候補の字, 「種類<TAB>注記」)。出すかどうかは設定 (server_note)
    pub notes: Vec<(String, String)>,
    /// 2026-10-29 (試験中、設定 input.live_short_dictionary): 短い入力の Space で見せている辞書の変換と違った、AI の変換 (候補の一覧の 2 番目に混ぜる)
    pub ai_text: Option<String>,
    /// 2026-11-02: 一覧の本当の末尾に置く、打ったとおりの英字の候補 (typed_latin_candidate)。一覧を並べ直しても末尾に戻す
    pub typed_latin: Option<String>,
    /// 2026-11-06 (仕事 20 の手 1): 一覧に足した、母音を補った形の候補 (候補の字, 補った読み)。選んで確定したら、補った読みで覚える
    pub repair_readings: Vec<(String, String)>,
    /// 2026-11-21 (恍惚上たい の案 3): 本当の変換でない表示 (句読点の変換が間に合わず、古い仮表示に増えた読みをかなで足したもの)。
    /// 候補の窓を開くときは、この字を 1 番目に置き直さず、一覧の 1 番目 (本当の変換) にする。Space では変換し直す
    pub patchwork: bool,
}

/// 2026-10-28 (試験中、設定 input.typo_repair): 変換するときに直したローマ字の母音の抜け。Shift+BackSpace で打ったとおりに戻すため
#[derive(Default, Clone, Debug)]
pub struct TypoRepair {
    /// 直す前の打った文字の列
    pub original_typed: String,
    /// 直した所の綴り (戻したら次から直さない。auto_english::typo_repair_key)
    pub keys: Vec<String>,
    /// 直した所の読みの中の位置 (字数)・補ったかな・もとの字 (候補の「打ったとおり」を作るため)
    pub spots: Vec<RepairedSpot>,
}

#[derive(Default, Clone, Debug, PartialEq)]
pub struct RepairedSpot {
    pub offset: usize,
    pub kana: String,
    /// 直す前の読みのその所の字 (子音 1 字。2026-11-04 からは英字の区切りの中の直しの英字の並び (shitno → したの) も)
    pub typed: String,
}

/// 直前に確定した内容 (Ctrl+Backspace で取り消すため)
#[derive(Default, Clone, Debug)]
pub struct LastCommit {
    pub text: String,
    pub reading: String,
    pub segments: Vec<Segment>,
}

#[derive(Clone, Debug)]
pub struct ConversionSnapshot {
    segments: Vec<Segment>,
    focus: usize,
    window_open: bool,
    refined: bool,
    segments_reading: String,
    latin_selected: bool,
    latin_learning_intent: bool,
    from_prediction: bool,
}

impl ConversionSnapshot {
    fn capture(comp: &Composition, reading: &str) -> Option<Self> {
        let complete = !comp.segments.is_empty()
            && comp.segments.iter().map(|segment| segment.reading.as_str()).collect::<String>() == reading;
        if !complete || (comp.state != CompositionState::Converting && !(comp.live && comp.refined)) {
            return None;
        }
        Some(Self { segments: comp.segments.clone(), focus: comp.focus, window_open: comp.window_open,
            refined: comp.refined, segments_reading: comp.segments_reading.clone(),
            latin_selected: comp.latin_selected, latin_learning_intent: comp.latin_learning_intent,
            from_prediction: comp.from_prediction })
    }

    /// 見えている変換をそのまま取っておく (表記だけを戻すとき。打っている途中のまだ清書していない表示も含む)
    fn capture_shown(comp: &Composition, reading: &str) -> Option<Self> {
        let complete = !comp.segments.is_empty()
            && comp.segments.iter().map(|segment| segment.reading.as_str()).collect::<String>() == reading;
        complete.then(|| Self { segments: comp.segments.clone(), focus: comp.focus, window_open: comp.window_open,
            refined: comp.refined, segments_reading: comp.segments_reading.clone(),
            latin_selected: comp.latin_selected, latin_learning_intent: comp.latin_learning_intent,
            from_prediction: comp.from_prediction })
    }

    fn restore(self, comp: &mut Composition) {
        comp.segments = self.segments;
        comp.focus = self.focus;
        comp.window_open = self.window_open;
        comp.refined = self.refined;
        comp.segments_reading = self.segments_reading;
        comp.latin_selected = self.latin_selected;
        comp.latin_learning_intent = self.latin_learning_intent;
        comp.from_prediction = self.from_prediction;
    }
}

#[derive(Clone, Debug)]
pub struct DictionaryPreview {
    reading: String,
    original: Option<ConversionSnapshot>,
    /// 2026-10-13: 2 つのスイッチ。Ctrl+変換 = 辞書だけの変換、Ctrl+無変換 = 表記の自動変換 (H₂O・α) を外す。両方オフで元へ
    dictionary_on: bool,
    plain: bool,
    /// 一度取った変換 (辞書だけ・AI) は覚えておく
    dictionary: Option<Vec<Segment>>,
    ai: Option<Vec<Segment>>,
}

impl DictionaryPreview {
    fn new(reading: String, original: Option<ConversionSnapshot>) -> Self {
        Self { reading, original, dictionary_on: false, plain: false, dictionary: None, ai: None }
    }
}

/// 2 つのスイッチに合わせた文節 (辞書だけ / AI の変換 に、表記の自動変換を外すかどうか)
fn preview_segments(plain: bool, base: Vec<Segment>) -> Vec<Segment> {
    if plain { strip_notation_segments(&base).unwrap_or(base) } else { base }
}

/// 2026-10-13: 変換サーバーが自動で掛けた表記を、打ったとおりに戻す (Ctrl+無変換)。
/// 化学式の下付き・電荷 (H₂O → H2O、SO₄²⁻ → SO42-) と、ギリシャ文字 (α → アルファ)。
/// ^ と _ で自分で打った上付き・下付きは読みに入っているので戻さない。ほかの部分の変換はそのまま
fn plain_notation_char(c: char) -> Option<char> {
    Some(match c {
        '₀'..='₉' => char::from_u32(c as u32 - '₀' as u32 + '0' as u32)?,
        '⁰' => '0', '¹' => '1', '²' => '2', '³' => '3',
        '⁴'..='⁹' => char::from_u32(c as u32 - '⁴' as u32 + '4' as u32)?,
        '⁺' => '+', '⁻' => '-', '·' => '.',
        _ => return None,
    })
}

const AUTO_GREEK: &[(char, &[(&str, &str)])] = &[
    ('ε', &[("いぷしろん", "イプシロン")]), ('ο', &[("おみくろん", "オミクロン")]), ('α', &[("あるふぁ", "アルファ")]),
    ('β', &[("べーた", "ベータ")]), ('γ', &[("がんま", "ガンマ")]), ('ζ', &[("ぜーた", "ゼータ")]),
    ('θ', &[("しーた", "シータ")]), ('ι', &[("いおた", "イオタ")]), ('λ', &[("らむだ", "ラムダ")]),
    ('μ', &[("みゅー", "ミュー")]), ('ξ', &[("ぐざい", "グザイ"), ("くしー", "クシー")]), ('σ', &[("しぐま", "シグマ")]),
    ('ψ', &[("ぷさい", "プサイ")]), ('ω', &[("おめが", "オメガ")]),
];

fn strip_notation(segment: &Segment) -> String {
    let chars: Vec<char> = segment.text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // 化学式のひと続き (英数字・括弧・下付き・電荷)。下付きの文字を含み、打ったとおりの形が読みにあるときだけ戻す
        let in_run = |c: char| c.is_ascii_alphanumeric() || matches!(c, '(' | ')' | '+' | '-' | '.') || plain_notation_char(c).is_some();
        if in_run(chars[i]) {
            let start = i;
            while i < chars.len() && in_run(chars[i]) { i += 1; }
            let run: String = chars[start..i].iter().collect();
            let plain: String = run.chars().map(|c| plain_notation_char(c).unwrap_or(c)).collect();
            out.push_str(if plain != run && segment.reading.contains(&plain) { &plain } else { &run });
            continue;
        }
        let greek = AUTO_GREEK.iter().find(|(letter, _)| *letter == chars[i])
            .and_then(|(_, names)| names.iter().find(|(reading, _)| segment.reading.contains(reading)));
        match greek {
            Some((_, katakana)) => out.push_str(katakana),
            None => out.push(chars[i]),
        }
        i += 1;
    }
    out
}

/// 表記の自動変換を戻した文節。何も変わらなければ None
fn strip_notation_segments(segments: &[Segment]) -> Option<Vec<Segment>> {
    let mut changed = false;
    let stripped = segments.iter().map(|segment| {
        let text = strip_notation(segment);
        changed |= text != segment.text;
        Segment { text, ..segment.clone() }
    }).collect();
    changed.then_some(stripped)
}

/// 2026-10-29: サーバーが返事をしない間・つながっていない間に押した、入力モードを切り替えるキー (半角/全角) なら、切り替えた先のモード
fn local_mode_toggle(wparam: WPARAM, ctrl: bool, mode: &InputMode) -> Option<InputMode> {
    (!ctrl && matches!(UserAction::try_from(wparam.0), Ok(UserAction::ToggleInputMode))).then(|| match mode {
        InputMode::Kana => InputMode::Latin,
        InputMode::Latin => InputMode::Kana,
    })
}

/// 2026-10-29: 利用者が結果を待っている動作 (Space・候補の一覧・文節の区切り直し・辞書だけの変換との切り替え・確定の取り消し…)。
/// このあいだはその場の上限 (EVENT_BUDGET) をかけず、問い合わせごとの上限まで変換を待つ (前は 3 秒で切り、遅い PC では長い文の Space が
/// 間に合わず、ひらがなのまま確定していた。レビュー H1)。打っている途中の動作 (Insert・句読点での変換・時計からの清書) は今までどおり 3 秒まで
fn awaited_by_user(action: &ClientAction) -> bool {
    matches!(action, ClientAction::Convert | ClientAction::ConvertAsOne | ClientAction::ToggleDictionaryConversion | ClientAction::ToggleNotation
        | ClientAction::NextCandidate | ClientAction::PrevCandidate | ClientAction::NextPage | ClientAction::PrevPage
        | ClientAction::SelectNumber(_) | ClientAction::MoveFocus(_) | ClientAction::FocusEdge(_) | ClientAction::Resize(_)
        | ClientAction::ConvertType(_) | ClientAction::OpenSingleKanji | ClientAction::DeleteAtCursor | ClientAction::UndoCommit
        | ClientAction::UndoTypoRepair | ClientAction::UsePrediction(_) | ClientAction::ForgetCandidate)
}

fn keeps_dictionary_preview(action: &ClientAction) -> bool {
    matches!(action, ClientAction::ToggleDictionaryConversion | ClientAction::ToggleNotation | ClientAction::MoveFocus(_)
        | ClientAction::FocusEdge(_) | ClientAction::NextCandidate | ClientAction::PrevCandidate
        | ClientAction::NextPage | ClientAction::PrevPage | ClientAction::SelectNumber(_)
        | ClientAction::OpenSingleKanji)
}

#[derive(Default, Clone, Debug)]
pub struct Composition {
    /// 2026-10-22: サーバーが返事をしない間に打った、まだかなになっていないローマ字 (local_kana_step)。空でない間は IME 本体だけで打つ
    pub local_romaji: String,
    pub hiragana: String, // 読み (ローマ字の残りを含む)
    pub caret: i32,       // 読みの中のキャレット位置 (文字数)
    pub raw_input: String,

    pub segments: Vec<Segment>,
    pub focus: usize,
    pub window_open: bool,
    /// 2026-10-14: 変換中に ←→ などでカーソル (注目文節の右端) を動かしたか。動かしていなければカーソルは全体の右端で、文字キーは確定
    pub cursor_moved: bool,
    /// 2026-10-14: 変換中のカーソルで打った文字と直後の文節を、1 つの変換の単位にして入力している間 true。
    /// サーバーの読みは「単位の読み + edit_tail の読み」で、カーソルは単位の中
    pub edit_unit: bool,
    /// edit_unit の後ろの文節。変換 (選んだ候補) をそのまま残す
    pub edit_tail: Vec<Segment>,

    pub display: String, // いま未確定文字列として表示している文字列
    pub live: bool,      // ライブ変換版のプロファイルで入力中か
    /// 2026-11-10: ライブ変換で Esc を押して、読み (かな) の表示に戻しているあいだ true (ShowReading)。次に読みを変える・動かすキーまで
    pub live_reading: bool,
    pub refined: bool,   // ライブ変換の表示が Zenzai で変換したものか (false = 辞書だけの仮表示)
    pub segments_reading: String, // segments を作ったときの読み
    /// 2026-10-29 (試験中、設定 input.live_keep_refined): ライブ変換で、AI が清書した前のほう (segments の頭からこの数の文節) を、
    /// 次の打鍵で辞書の結果に戻さずにそのまま見せる (清書したときの文節から最後の 1 つを除いた数)
    pub live_settled: usize,
    pub last_commit: Option<LastCommit>, // 確定直後だけ Some。ほかのキーを押したら None
    pub english_active: bool, // 英語と判定した語が入っているか (入っていれば打つたびに組み直す)
    pub latin_selected: bool, // 利用者が英字候補・英字変換を明示的に選んだか
    pub latin_learning_intent: bool, // F9/F10 などで英字変換したか。候補巡回だけでは学習しない
    /// 打った文字の列 (IME 側で記録)。サーバーの列は n+子音 を ん に書き換えてしまうので英語の判定には使えない
    pub typed: String,
    /// typed が読みと対応しているか (カーソルを動かして途中に打ったら崩れる)
    pub typed_valid: bool,
    /// この入力のローマ字の綴りをもう数えたか (文節ごとに確定したとき、残りを確定したときに二重に数えない)
    pub style_counted: bool,
    /// 大文字を打ったあとの一時的な英数入力 (Google 日本語入力・MS-IME の「シフトキーでの入力切替: 英数字」)。
    /// 確定するまで英字のまま。ただし大文字を 2 文字以上続けたあとに小文字を打つと、かなに戻る (NHKnoniyu-su → NHKのにゅーす)。
    /// 2026-11-03: 英字の語のあとが日本語なら、そこからも戻る (C₂ + について・Google + で。insert_temp_ascii)
    pub temp_ascii: bool,
    /// 続けて打った大文字の数 (Mozc の shifted_sequence_count_。2026-11-03 から数字では数え直さない: CO2 + について)
    pub shifted_count: u32,
    /// 2026-11-03: 一時的な英数入力を始めた位置 (読みの字の数)。ここから後ろの英字を区切りの道具で見て、かな に戻す位置を決める
    pub temp_ascii_start: Option<usize>,
    /// 予測変換の候補 (通常版のみ)。predicted_for はその候補を作ったときの読み
    pub predictions: Vec<String>,
    pub predicted_for: String,
    /// 変換中の文節が予測変換の候補から来たものか (確定しても単語として学習しない)
    pub from_prediction: bool,
    /// 2026-10-28 (試験中): 。、？！ で変換した直後か (このときの BackSpace は変換を取り消して入力中に戻す。ほかのキーを打ったら false)
    pub punctuation_converted: bool,
    /// 2026-10-28 (試験中): 変換するときに直した母音の抜け (打つ・消す・確定でなくなる)
    pub typo_repair: Option<TypoRepair>,
    /// この入力では母音の抜けをもう直さない (Shift+BackSpace で打ったとおりに戻したあと)
    pub typo_repair_declined: bool,
    /// 2026-11-06 (仕事 20 の手 1): 自動で直すほどではないが、補った読みのほうが AI の点で上だった所 (読みの中の位置・補ったかな・もとの字)。
    /// その所を含む文節の候補の一覧の 2 番目に、補った読みの形を出す
    pub typo_suggestions: Vec<RepairedSpot>,
    /// 2026-10-19: 次の語の候補 (英単語を打っている途中に、前の語と打ちかけから。next_word.rs)。next_words_for はその候補を作ったときの打ちかけ
    pub next_words: Vec<String>,
    pub next_words_for: String,
    /// Tab で補った候補の番号 (もう一度 Tab で次の候補に替える)。ほかのキーを打ったら None
    pub next_word_pick: Option<usize>,
    /// 入力を始めたときに、キャレットの前に空白で隔たってあった語 (なければ空)
    pub prev_word: String,
    /// 2026-11-05 (仕事 24): 入力を始めたときのキャレットの前の文字列 (40 字まで。読めなければ None)。英語の文のつづりの指摘・自動修正に使う
    pub text_before: Option<String>,
    /// 2026-11-05 (仕事 24): 英語の文のつづりの自動修正で確定した直後だけ Some (直した文字列, 打ったとおりの文字列, 直した綴り)。ほかのキーを押したら None
    pub last_autocorrect: Option<(String, String, Vec<String>)>,
    /// 2026-11-08 (仕事 12): last_autocorrect が、空白なしで打った英語の句に空白を入れた確定のとき、その打った並び (戻したら、この IME の間は分けない)
    pub last_phrase_runs: Vec<String>,
    /// 2026-10-24: 入力を始めたときの、キャレットの直前の字 (読めなければ、その直前に確定した文字列の最後の字)。
    /// 変換・確定した数字のあとに . , と数字を続けたとき、小数点・桁区切りに直すのに使う (number_mark_fix)
    pub char_before: Option<char>,
    /// 2026-10-28: 確定した最後の字。確定の直後のキーが入力を始める字のときだけ残し、その入力の char_before に使う (次の handle_action で捨てる)。
    /// 確定の取り消し (last_commit) は確定の直後のキーで忘れるので、それとは別に持つ (TSF で前の字を読めない欄で、2 + Enter のあとの .5 が 2。5 になっていた。B の写経 10-28)
    pub commit_tail: Option<char>,
    /// ^ _ による上付き・下付きの入力状態 (scripts.rs)
    pub script: super::scripts::ScriptState,
    /// TeX のコマンド (\ のあとの英字) を打っている途中
    pub tex_command: bool,
    /// 長押しでアクセント付きの文字の候補を出している元の文字
    pub accent: Option<char>,
    /// 長押しの候補でいま選んでいる位置 (-1 = まだ選んでいない)
    pub accent_index: i32,
    /// 候補からすでに入力した文字。選び直すときは追加せず、この文字を置き換える
    pub accent_inserted: Option<char>,
    /// 次の確定の末尾に足す文字 (英単語だけの入力を Space で確定するときの半角空白)
    pub commit_suffix: String,
    pub dictionary_preview: Option<DictionaryPreview>,
    pub ivs_original: Option<String>, // 確定済みの選択範囲。キャンセル時は消さずに戻す

    pub state: CompositionState,
    pub tip_composition: Option<ITfComposition>,
}

/// 2026-11-06 (端末の件): 未確定の状態を空にしてから次のキーを扱うか。未確定の範囲を失った (edit_session の composition_lost) か、
/// 入力中・変換中なのにコンポジションを持っていない (確定の途中で失敗して書き戻しだけが残った) とき
fn composition_needs_reset(composition: &Composition) -> bool {
    crate::tsf::edit_session::composition_lost()
        || (composition.state != CompositionState::None && composition.tip_composition.is_none())
}

impl Composition {
    fn begin_accents(&mut self, base: char, variants: &[char]) -> Option<char> {
        let first = *variants.first()?;
        self.accent = Some(base);
        self.accent_index = 0;
        self.accent_inserted = None;
        Some(first)
    }

    fn close_accents(&mut self) {
        self.accent = None;
        self.accent_index = -1;
        self.accent_inserted = None;
    }

    fn replace_typed_accent(&mut self, ch: char) -> bool {
        let Some(previous) = self.accent_inserted.or(self.accent) else { return false };
        if self.state != CompositionState::Composing || !self.typed_valid || !self.typed.ends_with(previous) {
            return false;
        }
        self.typed.pop();
        self.typed.push(ch);
        true
    }

    /// 長押しのアクセントを、いまの入力のどこに当てるか。入力中でないときは、アプリに入っている元の字を BackSpace で消して置き換える (preview_accent)。
    /// 2026-10-29: かなモードの入力中でないときは、さらに最初の押下を字のままアプリへ渡していたことを見る (accents_on_hold)
    fn at_accent_letter(&self, c: char) -> bool {
        match self.state {
            CompositionState::None => true,
            CompositionState::Composing => self.caret >= self.hiragana_len()
                && ((self.typed_valid && self.typed.ends_with(c)) || self.hiragana.ends_with(c)),
            CompositionState::Converting => false,
        }
    }

    /// 2026-10-22 (D の 2 回目の調べ R2-2): 生きている打ちかけ。打ちかけを見せている入力 (tip_composition) が閉じていたら
    /// (欄を移った・アプリか IME が入力を終えた)、無いものとみなす (次の欄では、欄の種類から確かめ直す)
    fn live_local_romaji(&self) -> &str {
        if self.tip_composition.is_some() { &self.local_romaji } else { "" }
    }

    fn reset(&mut self) {
        self.local_romaji.clear();
        self.hiragana.clear();
        self.caret = 0;
        self.raw_input.clear();
        self.segments.clear();
        self.focus = 0;
        self.window_open = false;
        self.cursor_moved = false;
        self.edit_unit = false;
        self.edit_tail.clear();
        self.display.clear();
        self.live_reading = false;
        self.refined = false;
        self.segments_reading.clear();
        self.live_settled = 0;
        self.english_active = false;
        self.latin_selected = false;
        self.latin_learning_intent = false;
        self.typed.clear();
        self.typed_valid = true;
        self.style_counted = false;
        self.temp_ascii = false;
        self.shifted_count = 0;
        self.temp_ascii_start = None;
        self.predictions.clear();
        self.predicted_for.clear();
        self.from_prediction = false;
        self.dictionary_preview = None;
        self.ivs_original = None;
        self.script = super::scripts::ScriptState::Off;
        self.tex_command = false;
        self.next_words.clear();
        self.next_words_for.clear();
        self.next_word_pick = None;
        self.prev_word.clear();
        self.char_before = None;
        self.typo_repair = None;
        self.typo_repair_declined = false;
        self.typo_suggestions.clear();
        self.close_accents();
    }

    fn hiragana_len(&self) -> i32 {
        self.hiragana.chars().count() as i32
    }

    fn converted_text(&self) -> String {
        self.segments.iter().map(|s| s.text.as_str()).collect()
    }

    /// 注目文節より前の文節の表記 (変換の文脈に使う)
    fn text_before_focus(&self) -> String {
        self.segments[..self.focus]
            .iter()
            .map(|s| s.text.as_str())
            .collect()
    }
}

/// いま選ばれているのがライブ変換版のプロファイルか
/// 大文字を打ったら確定まで英字にするか (設定 input.shift_ascii_mode。オンは Google 日本語入力と同じ。2026-10-21 から初期値はオフ = 大文字で始まる語だけ英字で、後ろは英語の判定に任せる)
/// 2026-11-05 (仕事 23): かなの入力中に打った数字を全角で見せる・確定するか (設定 input.fullwidth_digits: off・numbers・all、初期値 off)。試験ではいつも off
fn fullwidth_digit_form() -> super::full_width::DigitForm {
    if cfg!(test) {
        return super::full_width::DigitForm::Off;
    }
    static FORM: std::sync::OnceLock<super::full_width::DigitForm> = std::sync::OnceLock::new();
    *FORM.get_or_init(|| super::full_width::DigitForm::from_setting(&shared::AppConfig::read_or_default().input.fullwidth_digits))
}

/// 今の入力で見せる数字の形 (F9・F10 で英字を選んだ入力・英数モードは打ったとおり)
fn digit_form_for(comp: &Composition, mode: &InputMode) -> super::full_width::DigitForm {
    if *mode == InputMode::Kana && !comp.latin_selected { fullwidth_digit_form() } else { super::full_width::DigitForm::Off }
}

fn shift_ascii_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| shared::AppConfig::read_or_default().input.shift_ascii_mode)
}

const ACCENT_INPUT_MARKER: usize = 0x415a4b41;

/// アプリに Backspace と 1 文字を送る (英数モードで長押しの候補を選んだとき、元の文字を置き換える)
fn send_backspace_and_char(ch: char) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_BACK,
    };
    let key = |vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: ACCENT_INPUT_MARKER } },
    };
    let mut inputs = vec![key(VK_BACK, 0, KEYBD_EVENT_FLAGS(0)), key(VK_BACK, 0, KEYEVENTF_KEYUP)];
    let mut buffer = [0u16; 2];
    for unit in ch.encode_utf16(&mut buffer) {
        inputs.push(key(VIRTUAL_KEY(0), *unit, KEYEVENTF_UNICODE));
        inputs.push(key(VIRTUAL_KEY(0), *unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
    }
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

/// 2026-10-13: 行頭の / のあとで素通しにした文字を日本語の入力に戻す。Backspace (印つき = IME を素通り) で消し、
/// 同じ英字を送り直す (IME がふつうのキーとして受け取り、かなにする)。
/// 2026-10-29: 送り直す英字には打ち直しの印 (diacritics::RETYPED_KEY_MARKER) を付ける (利用者がまだ押しているキーを送るので Windows が
/// 「直前も押されていた」として届け、長押しと数えていた。端末の入力待ちでなくなったあとの最初の母音・n が BackSpace と á になった)
fn send_retype(delete: usize, text: &str) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_BACK,
    };
    let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS, extra: usize| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: extra } },
    };
    let mut inputs = vec![];
    for _ in 0..delete {
        inputs.push(key(VK_BACK, KEYBD_EVENT_FLAGS(0), ACCENT_INPUT_MARKER));
        inputs.push(key(VK_BACK, KEYEVENTF_KEYUP, ACCENT_INPUT_MARKER));
    }
    for c in text.chars().filter(|c| c.is_ascii_lowercase() || *c == '-') {
        // ハイフンは - のキー (VK_OEM_MINUS)。英字は A〜Z の仮想キー
        let vk = VIRTUAL_KEY(if c == '-' { 0xBD } else { c.to_ascii_uppercase() as u16 });
        inputs.push(key(vk, KEYBD_EVENT_FLAGS(0), super::diacritics::RETYPED_KEY_MARKER));
        inputs.push(key(vk, KEYEVENTF_KEYUP, super::diacritics::RETYPED_KEY_MARKER));
    }
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

/// 押しっぱなしの連打 (diacritics::is_repeat) の文字 c で、長押しのアクセントの候補を出すか。
/// 2026-10-29: かなモードで入力中でないときは、最初の押下を字のままアプリへ渡していたときだけ (diacritics::passed_to_app。数字の欄・端末の選択肢など)。
/// 前は入力中でなければいつも出し、サーバーの失敗の逃げ道 (見えているものを確定し、打った字をひらがなで入れ直す) のあとに押し続けると、
/// BackSpace で確定した あ を消して á を入れた (10-29 の不具合の報告「打ち始めがアクセントになる」)
/// 2026-11-05 (長押し): 押しっぱなしの英字のキーの繰り返し (repeat) を、アプリにもかなの入力にも渡さず捨てるか。
/// 長押しのアクセントを使う設定 (言語を選んでいる) で、input.letter_key_repeat がオフ (初期値)、端末・コードエディタでないとき
fn letter_repeat_suppressed(repeat: bool, vkey: usize) -> bool {
    repeat && (0x41..=0x5A).contains(&vkey) && super::diacritics::enabled() && !letter_key_repeat() && super::next_word::app_allows_latin_help()
}

/// 2026-11-05 (長押し): 設定 input.letter_key_repeat (初期値オフ = 英字のキーを押しっぱなしにしても続けて入れない)。試験ではオフ
fn letter_key_repeat() -> bool {
    if cfg!(test) { return false; }
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| shared::AppConfig::read_or_default().input.letter_key_repeat)
}

fn accents_on_hold(composition: &Composition, mode: &InputMode, c: char) -> bool {
    let allowed = *mode == InputMode::Latin || super::diacritics::in_kana();
    let start_in_kana = composition.state == CompositionState::None && *mode == InputMode::Kana;
    composition.at_accent_letter(c) && allowed && (!start_in_kana || super::diacritics::passed_to_app(c))
        && !super::diacritics::variants(c).is_empty()
}

/// 2026-10-29: ライブ変換の表示を落ち着かせる 2 つの設定 (input.live_keep_refined, input.live_short_dictionary)。2026-10-30 から初期値はどちらもオン。
/// 試験のビルドでは設定を読まず、決めた値 (LiveOptions) が無ければどちらもオフ (前からの試験の前提)
fn live_options() -> (bool, bool) {
    #[cfg(test)]
    if let Some(options) = LIVE_OPTIONS_FOR_TESTS.with(|options| options.get()) {
        return options;
    }
    static OPTIONS: std::sync::OnceLock<(bool, bool)> = std::sync::OnceLock::new();
    *OPTIONS.get_or_init(|| {
        if cfg!(test) { return (false, false); }
        let input = shared::AppConfig::read_or_default().input;
        (input.live_keep_refined, input.live_short_dictionary)
    })
}

#[cfg(test)]
thread_local! {
    static LIVE_OPTIONS_FOR_TESTS: std::cell::Cell<Option<(bool, bool)>> = const { std::cell::Cell::new(None) };
}

/// 2026-10-29 (試験中、設定 input.live_short_dictionary): これ以下の字数の読みを「短い入力」とする。正解つきの文 206 文 (文節の字と読み) の
/// 文の頭と文節 1〜2 個のかたまり 2,433 件で、一番上が正解になる割合を辞書だけと AI で比べると、6 字までは差が 3 点ほど (81.6% と 84.9%) で、
/// 7 字から 5〜16 点に開く (写経の手間を読みの長さ別に比べたデータが出たら決め直す)
const SHORT_LIVE_CHARS: usize = 6;

/// 短い入力のライブ変換か (設定がオンで、読みが SHORT_LIVE_CHARS 字以下): 辞書の変換だけを見せ、AI の変換は Space の一覧に混ぜる
fn short_live_input(comp: &Composition, reading: &str) -> bool {
    comp.live && live_options().1 && reading.chars().count() <= SHORT_LIVE_CHARS
}

/// 2026-10-29 (試験中、設定 input.live_keep_refined): AI が清書した前のほう (comp.live_settled の数の文節) と、その後ろの読み。
/// 前のほうの読みが今の読みの頭と合わなければ (前のほうを消した・直した) None
fn settled_live_prefix(comp: &Composition, reading: &str) -> Option<(Vec<Segment>, String)> {
    let kept = comp.segments.get(..comp.live_settled).filter(|kept| !kept.is_empty())?;
    let kept_reading: String = kept.iter().map(|s| s.reading.as_str()).collect();
    let rest = reading.strip_prefix(kept_reading.as_str())?;
    Some((kept.to_vec(), rest.to_string()))
}

/// 2026-10-29 (試験中、設定 input.live_short_dictionary): Space で求めた AI の変換を、見えている (辞書の) 文節の候補に混ぜる印を付ける。
/// 文節の区切りが AI と同じならその文節に、見えているのが 1 文節なら AI の変換の全体を。区切りが違えば混ぜない
fn attach_ai_alternatives(segments: &mut [Segment], ai: &[Segment]) {
    if let [only] = segments {
        let text: String = ai.iter().map(|s| s.text.as_str()).collect();
        only.ai_text = (!text.is_empty() && text != only.text).then_some(text);
        return;
    }
    if segments.len() == ai.len() && segments.iter().zip(ai).all(|(shown, ai)| shown.reading == ai.reading) {
        for (shown, ai) in segments.iter_mut().zip(ai) {
            shown.ai_text = (ai.text != shown.text).then(|| ai.text.clone());
        }
    }
}

fn is_live_profile() -> bool {
    use windows::Win32::{
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::TextServices::{
            CLSID_TF_InputProcessorProfiles, ITfInputProcessorProfileMgr, GUID_TFCAT_TIP_KEYBOARD,
            TF_INPUTPROCESSORPROFILE,
        },
    };
    unsafe {
        let Ok(mgr) = CoCreateInstance::<_, ITfInputProcessorProfileMgr>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        ) else {
            return false;
        };
        let mut profile = TF_INPUTPROCESSORPROFILE::default();
        if mgr.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile).is_err() {
            return false;
        }
        profile.guidProfile == crate::globals::GUID_PROFILE_LIVE
    }
}

/// ライブ変換で、変換済みの読みのあとに付いているローマ字の打ちかけ (例: 「きょうは」+「k」の k)
fn live_pending_suffix(comp: &Composition) -> Option<String> {
    let reading = comp.hiragana.as_str();
    let base = comp.segments_reading.as_str();
    if comp.segments.is_empty() || base.is_empty() || !reading.starts_with(base) {
        return None;
    }
    let suffix = &reading[base.len()..];
    if suffix.chars().all(|c| c.is_ascii_alphabetic()) {
        Some(suffix.to_string())
    } else {
        None
    }
}

fn live_refinement_reading(comp: &Composition, complete: bool) -> String {
    let reading = composition_reading(comp);
    // Space converts the entire input, including unfinished roman letters.
    let latin_tail = comp.english_active && (!comp.typed_valid
        || super::auto_english::segment(&comp.typed).last()
            .is_some_and(|token| token.kind == super::auto_english::TokenKind::English));
    if complete || comp.temp_ascii || latin_tail {
        reading
    } else {
        reading.trim_end_matches(|c: char| c.is_ascii_alphabetic()).to_string()
    }
}

/// 2026-11-21 (学習の効きすぎの入口、まとめ役の決め): ライブ変換で清書した前のほうの後ろの読みだけを辞書で変換すると、文頭として変換されて
/// 助詞の読みが 1 字の漢字になる (規則|が → 規則|賀。その表示を確定すると 規則賀 が覚えられて自分で強まっていた)。前のほうの最後の文節の読みを
/// 頭に付けて変換し、結果がその読みの長さで切れていれば、その後ろだけを使う (前のほうの字は清書のまま)。切れ方が違えば、今までどおり後ろだけを変換する。
/// 変換サーバーの左の文脈に前のほうの字を渡す形は、辞書だけの変換が文脈の終わりがひらがな・読点のときしか始点の品詞を引けないので効かない
/// 2026-11-23: 戻り値の 1 つ目が真なら、前のほうの最後の文節も残さない (呼ぶ側は kept の最後を外し、2 つ目で置き換える)。
/// 前のほうの最後の文節が読みのかなのまま (AI が字を替えていない と・の・が など) で、つなげた変換で切れ目をまたぐ語の字が替わるとき
/// (かなの文節と後ろの字で 1 語の漢字になる形。後ろだけを文頭として変換すると、その 1 語にならず、かなの文節 + 1 字の漢字のまま確定していた)。
/// 漢字・カタカナの文節 (AI が選んだ字) は今までどおり残す
fn convert_rest_after(ipc: &mut IPCService, kept: &[Segment], rest: &str) -> Result<(bool, Vec<Segment>)> {
    if let Some(anchor) = kept.last().filter(|s| !s.reading.is_empty()) {
        let joined = format!("{}{}", anchor.reading, rest);
        let converted = ipc.convert_with(joined.clone(), String::new(), true, true)?;
        let segments = build_segments(&joined, converted);
        let anchor_len = anchor.reading.chars().count();
        let mut covered = 0;
        let mut text_before = String::new();
        for (i, segment) in segments.iter().enumerate() {
            let len = segment.reading.chars().count();
            if covered + len == anchor_len && i + 1 < segments.len() {
                return Ok((false, segments[i + 1..].to_vec()));
            }
            if covered + len > anchor_len {
                // 2026-11-21 (文節が「名詞＋助詞」になったため): 切れ目が文節の中に落ちた (規則 + が → 規則が) ときは、
                // その文節までの字が前のほうの字で始まれば、残り (が) を 1 文節に分けて使う。始まらなければ (字が替わった) 使わない
                let joined_text = format!("{}{}", text_before, segment.text);
                if let Some(rest_text) = joined_text.strip_prefix(anchor.text.as_str()).filter(|t| !t.is_empty()) {
                    let chars: Vec<char> = joined.chars().collect();
                    let rest_reading: String = chars[anchor_len..covered + len].iter().collect();
                    let mut tail = vec![Segment { reading: rest_reading, text: rest_text.to_string(), ..Default::default() }];
                    tail.extend_from_slice(&segments[i + 1..]);
                    return Ok((false, tail));
                }
                if anchor.text == anchor.reading {
                    return Ok((true, segments));
                }
                break;
            }
            covered += len;
            text_before.push_str(&segment.text);
        }
    }
    let converted = ipc.convert_with(rest.to_string(), String::new(), true, true)?;
    Ok((false, build_segments(rest, converted)))
}

/// 2026-10-29: 。、？！ で変換するとき、AI・辞書の変換が間に合わなかった。見えている辞書の仮表示 (前のキーまでの読みを変換したもの) に、
/// その後ろに増えた読み (句読点など) をかなのまま 1 文節として足したもの。ライブ変換でない・仮表示が今の読みの頭と合わなければ None (レビュー M2・再点検 R1)
fn preview_with_tail(comp: &Composition, reading: &str) -> Option<Vec<Segment>> {
    let tail = reading.strip_prefix(comp.segments_reading.as_str())?;
    if !comp.live || comp.segments.is_empty() || comp.segments_reading.is_empty() || tail.is_empty() {
        return None;
    }
    let mut segments = comp.segments.clone();
    // 2026-11-21 (恍惚上たい の案 1c): 増えた読みが句読点だけでなく、助詞・助動詞の読みでもなく、古い仮表示の最後の文節が漢字で終わるなら、
    // その文節を外してかなで見せる (前の読みの仮表示の最後の語は、続きを打つと切れ目が変わる所 (こうこつじょう → 恍惚上 + たい) で、違う漢字を見せやすい)。
    // 増えたのが助詞のとき・最後の文節がかなで終わる (今日は) ときは、古い仮表示がたいてい正しいのでそのまま。
    // B の途中まで打った読み 4,000 か所 (間に合わなかった字数 2・3): 違う漢字を見せた所が消えた 935・1,040、正しかった表示をかなに戻した 189・77、悪 0
    let kana: String = tail.chars().filter(|c| !is_sentence_punctuation(*c)).collect();
    let mut tail = tail.to_string();
    let last_ends_kanji = segments.last().and_then(|s| s.text.chars().last()).is_some_and(|c| ('一'..='鿿').contains(&c) || c == '々');
    if !kana.is_empty() && !FUNCTION_TAILS.contains(&kana.as_str()) && last_ends_kanji {
        if let Some(last) = segments.pop() {
            tail = format!("{}{}", last.reading, tail);
        }
    }
    let patchwork = !kana.is_empty();
    segments.extend(build_segments(&tail, vec![]));
    if patchwork {
        // 2026-11-21 (案 3): 本当の変換でない表示の印
        for segment in segments.iter_mut() {
            segment.patchwork = true;
        }
    }
    Some(segments)
}

/// 2026-11-21 (案 1c): 古い仮表示をそのまま残す、増えた読み (助詞・助動詞)
const FUNCTION_TAILS: &[&str] = &["が", "は", "に", "を", "で", "と", "も", "の", "へ", "や", "か", "て", "ば", "し", "から", "まで", "より", "って",
    "けど", "ので", "のに", "だ", "です", "ます", "た", "だった", "でした", "ました", "ね", "よ", "な"];

fn is_sentence_punctuation(c: char) -> bool {
    "、。，．,.？！?!「」『』（）()…・".contains(c)
}

fn latin_spelling_candidates(text: &str, reading: &str, candidates: Vec<String>) -> Vec<String> {
    let letters = |t: &str| -> String {
        apply_character_form(t).chars().filter(|c| is_latin_letter(*c)).collect::<String>().to_lowercase()
    };
    let literal = apply_character_form(reading);
    let base = if !literal.is_empty() && literal.chars().all(is_latin_letter)
        && letters(text) != literal.to_lowercase() {
        literal
    } else {
        text.to_string()
    };
    let key = letters(&base);
    let map_runs = |f: &dyn Fn(&str) -> String| -> String {
        let mut out = String::new();
        let mut run = String::new();
        for c in base.chars() {
            if is_latin_letter(c) {
                run.push(c);
            } else {
                if !run.is_empty() {
                    out += &f(&run);
                    run.clear();
                }
                out.push(c);
            }
        }
        if !run.is_empty() { out += &f(&run); }
        out
    };
    let capitalize = |w: &str| -> String {
        let mut chars = w.chars();
        chars.next().map(|c| c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()).unwrap_or_default()
    };
    let mut variants = vec![
        base.clone(),
        map_runs(&|w| super::auto_english::canonical_case(&w.to_ascii_lowercase()).map(str::to_string).unwrap_or_else(|| w.to_string())),
        map_runs(&|w| capitalize(w)),
        map_runs(&|w| w.to_uppercase()),
        map_runs(&|w| w.to_lowercase()),
        map_runs(&|w| to_fullwidth(w, true)),
    ];
    let mut seen = std::collections::HashSet::new();
    variants.retain(|v| seen.insert(v.clone()));
    variants.extend(candidates.into_iter().filter(|c| !seen.contains(c) && letters(c) == key));
    // 2026-10-31: 小文字を含む全角の英字の候補 (ｓｉｎ・ｋｅｎｗｏ) は出さない (日本語の候補を選ぶときにじゃま、と報告)。全部大文字の全角 (ＤＮＡ) と、
    // 一覧の頭 (今の表記) は残す。全角の英字は F9 で出せる
    let head = variants.first().cloned();
    variants.retain(|v| Some(v) == head.as_ref() || !has_fullwidth_lowercase(v));
    variants
}

/// 2026-10-31: 小文字の全角の英字 (ａ〜ｚ) を含むか
fn has_fullwidth_lowercase(text: &str) -> bool {
    text.chars().any(|c| ('ａ'..='ｚ').contains(&c))
}

fn is_emoji_candidate(candidate: &str) -> bool {
    if emojis::get(candidate).is_some() { return true; }
    // The dictionary includes redundant VS16 and standalone hair/skin components,
    // which are not RGI emoji on their own. Do not rewrite the actual candidate.
    let unqualified = candidate.replace('\u{fe0f}', "");
    emojis::get(&unqualified).is_some() || matches!(unqualified.as_str(),
        "\u{1f9b0}" | "\u{1f9b1}" | "\u{1f9b2}" | "\u{1f9b3}" |
        "\u{1f3fb}" | "\u{1f3fc}" | "\u{1f3fd}" | "\u{1f3fe}" | "\u{1f3ff}")
}

fn finish_candidate_list(mut candidates: Vec<String>, reading: &str) -> Vec<String> {
    // 2026-10-31: 変換サーバー・Windows の候補に混ざった、小文字を含む全角の英字 (ｓｉｎ) も出さない。一覧の頭 (選んでいる候補。F9 で全角にした文節) は残す
    let head = candidates.first().cloned();
    candidates.retain(|c| Some(c) == head.as_ref() || !has_fullwidth_lowercase(c));
    // Keep the server's equivalent spellings together after current/Windows candidates move.
    let admission_forms = match reading {
        "りいち" => Some(["理一", "理Ⅰ"]),
        "りに" => Some(["理二", "理Ⅱ"]),
        "りさん" => Some(["理三", "理Ⅲ"]),
        "ぶんいち" => Some(["文一", "文Ⅰ"]),
        "ぶんに" => Some(["文二", "文Ⅱ"]),
        "ぶんさん" => Some(["文三", "文Ⅲ"]),
        _ => None,
    };
    if let Some([kanji, roman]) = admission_forms {
        if let (Some(a), Some(b)) = (candidates.iter().position(|c| c == kanji), candidates.iter().position(|c| c == roman)) {
            let second = candidates.remove(a.max(b));
            candidates.insert(a.min(b) + 1, second);
        }
    }
    let has_marker = candidates.iter().any(|c| c == SINGLE_KANJI_MARKER);
    candidates.retain(|c| c != SINGLE_KANJI_MARKER);
    let half = to_half_katakana(reading);
    // Latin text has no half-katakana alternative; keep its original candidate first.
    if half != reading {
        candidates.retain(|c| c != &half);
        candidates.push(half);
    }
    if has_marker { candidates.push(SINGLE_KANJI_MARKER.to_string()); }
    // Stable partition after Windows candidates and spelling forms have been added.
    let (emoji, mut ordinary): (Vec<_>, Vec<_>) = candidates.into_iter()
        .partition(|candidate| is_emoji_candidate(candidate));
    ordinary.extend(emoji);
    ordinary
}

fn finish_segment_candidates(segment: &mut Segment, candidates: Vec<String>) {
    let selected_emoji = is_emoji_candidate(&segment.text);
    segment.candidates = finish_candidate_list(candidates, &segment.reading);
    place_typed_latin_last(&mut segment.candidates, segment.typed_latin.as_deref());
    segment.index = if selected_emoji {
        segment.candidates.iter().position(|candidate| candidate == &segment.text).unwrap_or(0) as i32
    } else { 0 };
    if let Some(text) = segment.candidates.get(segment.index as usize) { segment.text = text.clone(); }
}

fn latin_kana_reading(reading: &str) -> Option<String> {
    let reading = apply_character_form(reading);
    if !reading.chars().any(|c| c.is_ascii_alphabetic())
        || !reading.chars().all(|c| c.is_ascii_alphabetic() || ('ぁ'..='ゖ').contains(&c) || c == 'ー' || c == '-') { return None; }
    let units = super::romaji_style::simulate(&to_fullwidth(&reading, false));
    let kana: String = units.iter().map(|unit| unit.kana.as_str()).collect();
    let kana = normalize_reading(&kana);
    (!kana.is_empty() && kana.chars().all(|c| ('ぁ'..='ゖ').contains(&c) || c == 'ー')).then_some(kana)
}

fn learning_reading(segment: &Segment) -> String {
    // 2026-11-06 (仕事 20 の手 1): 一覧から選んだ、母音を補った形なら、補った読みで覚える
    if let Some((_, reading)) = segment.repair_readings.iter().find(|(text, _)| text == &segment.text) {
        return reading.clone();
    }
    if segment.text.chars().any(|c| !is_latin_letter(c)) {
        if let Some(kana) = latin_kana_reading(&segment.reading) { return kana; }
    }
    segment.reading.clone()
}

fn add_japanese_recovery(candidates: &mut Vec<String>, kana: &str, recovered: Vec<String>) {
    for text in recovered.into_iter().chain([kana.to_owned(), to_katakana(kana), to_half_katakana(kana)]) {
        if !candidates.contains(&text) { candidates.push(text); }
    }
}

/// 実際のサーバーを相手に、^ _ の上付き・下付きを handle_action と同じ順 (上付き・下付き → 一時的な英数 → ふつうの入力) で打つ
#[cfg(test)]
pub(super) fn verify_script_typing(ipc: &mut IPCService) -> serde_json::Value {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    super::auto_english::set_english_sentence(false);
    for (raw, expected) in [
        ("x_n", Some("xₙ")), ("a_n", Some("aₙ")), ("S_nha", Some("Sₙは")), ("suuretua_nnoippankou", Some("すうれつaₙのいっぱんこう")),
        ("a_{n+1}", Some("aₙ₊₁")), ("_nC_r", Some("ₙCᵣ")), ("e^x", Some("eˣ")), ("a^2+b^2", Some("a²+b²")), ("H_2O", Some("H₂O")),
        ("n_k", Some("nₖ")), ("ohayou^_^", Some("おはよう^_^")), ("x_b", Some("x_b")), ("x_i+x_j", Some("xᵢ+xⱼ")),
        // 語の後ろの _ は今までどおり (下付きにしない)
        ("file_name", None), ("my_var", None),
    ] {
        ipc.clear_text().unwrap();
        let mut comp = Composition::default();
        comp.reset();
        for ch in raw.chars() {
            let text = ch.to_string();
            if TextServiceFactory::script_applies(&comp, &text) {
                TextServiceFactory::insert_script_char(ipc, &mut comp, ch).unwrap();
                continue;
            }
            if shift_ascii_enabled() && TextServiceFactory::update_temp_ascii(&mut comp, &text) {
                TextServiceFactory::insert_temp_ascii(ipc, &mut comp, &text).unwrap();
                continue;
            }
            comp.typed.push(ch);
            let (h, c, r) = ipc.edit_text_raw(Op::Insert, to_fullwidth(&text, false), 0).unwrap();
            comp.hiragana = apply_character_form(&h);
            comp.caret = c;
            comp.raw_input = r;
            if comp.typed_valid && super::auto_english::enabled() {
                TextServiceFactory::apply_auto_english(ipc, &mut comp, false).unwrap();
            }
        }
        let reading = composition_reading(&comp);
        let converted: Vec<String> = ipc.convert(reading.clone(), String::new()).unwrap().into_iter().map(|(t, _)| t).collect();
        let ok = match expected {
            Some(expected) => reading == expected,
            None => reading.contains('_') && !reading.chars().any(|c| "ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓ".contains(c)),
        };
        if !ok { failures.push(format!("{raw}: {reading} (expected {expected:?})")); }
        rows.push(serde_json::json!({"raw":raw,"reading":reading,"expected":expected,"converted":converted}));
    }
    ipc.clear_text().unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    serde_json::json!({"rows":rows})
}

#[cfg(test)]
pub(super) fn verify_mixed_loanwords(ipc: &mut IPCService) -> serde_json::Value {
    let mut rows = Vec::new();
    let factory = TextServiceFactory::default();
    for english_context in [false, true] {
        for (raw, expected) in [
            ("wikipedia", "wikipedia"), ("wikimedia", "wikimedia"), ("azookey", "azookey"),
            ("wikipediawoyomu", "wikipediaをよむ"),
            ("toyotanodexi-ra-", "とよたのでぃーらー"), ("nodexi-ra-", "のでぃーらー"),
            ("toyotanodeli-ra-", "とよたのでぃーらー"),
            ("do-ravi-ra-", "どーらゔぃーらー"),
            ("windowsproiretahougaii", "windowsproいれたほうがいい"),
        ] {
            super::auto_english::set_english_sentence(false);
            if english_context {
                super::auto_english::note_sentence_commit(true, true, true);
                super::auto_english::note_sentence_commit(true, true, true);
            }
            ipc.clear_text().unwrap();
            let mut comp = Composition::default();
            comp.reset();
            let mut prefixes = Vec::new();
            for ch in raw.chars() {
                comp.typed.push(ch);
                let (h, c, r) = ipc.edit_text_raw(Op::Insert, to_fullwidth(&ch.to_string(), false), 0).unwrap();
                comp.hiragana = apply_character_form(&h);
                comp.caret = c;
                comp.raw_input = r;
                TextServiceFactory::apply_auto_english(ipc, &mut comp, false).unwrap();
                prefixes.push(comp.hiragana.clone());
            }
            assert_eq!(composition_reading(&comp), expected, "{raw}, context={english_context}");
            TextServiceFactory::apply_auto_english(ipc, &mut comp, true).unwrap();
            assert_eq!(composition_reading(&comp), expected, "finished {raw}");
            let converted = ipc.convert_with(expected.into(), String::new(), true, true).unwrap();
            if raw == "toyotanodexi-ra-" {
                assert_eq!(converted.iter().map(|(text, _)| text.as_str()).collect::<String>(), "トヨタのディーラー");
            }
            rows.push(serde_json::json!({"raw":raw,"reading":expected,"english_context":english_context,
                "prefixes":prefixes,"converted":converted}));
        }
    }
    super::auto_english::set_english_sentence(false);
    for (reading, expected) in [("ravi", "ラヴィ"), ("raviーらー", "ラヴィーラー"), ("nodexiーらー", "のでぃーらー")] {
        let mut comp = Composition { hiragana: reading.into(), ..Default::default() };
        comp.segments = build_segments(reading, vec![(reading.into(), reading.chars().count() as i32)]);
        factory.ensure_candidates(ipc, &mut comp).unwrap();
        assert!(comp.segments[0].candidates.iter().any(|candidate| candidate == expected), "{reading}");
        rows.push(serde_json::json!({"recovery_reading":reading,"candidates":comp.segments[0].candidates}));
    }
    for (reading, word) in [("うぃきぺでぃあ", "wikipedia"), ("うぃきめでぃあ", "wikimedia"), ("あずーきー", "azookey")] {
        let candidates = ipc.list_candidates(reading.into(), String::new()).unwrap();
        assert!(candidates.iter().any(|candidate| candidate.eq_ignore_ascii_case(word)), "{reading}: {candidates:?}");
    }
    ipc.clear_text().unwrap();
    serde_json::json!(rows)
}

/// 2026-11-08 (仕事 52): 重いので AI を休んでいる間に候補の窓の下に添える文言 (利用者の指定。字はこのまま)
pub(crate) const AI_PAUSE_NOTE: &str = "負荷がかかっているためAIがオフになっています";

/// 候補の窓に添える文言: 変換サーバーが AI を休んでいて、設定 input.ai_pause_notice (初期値オン) がオンなら AI_PAUSE_NOTE
fn ai_pause_note() -> String {
    pause_note(super::ipc_service::ai_paused(), super::ipc_service::ai_pause_notice_enabled())
}

fn pause_note(paused: bool, notice: bool) -> String {
    if paused && notice { AI_PAUSE_NOTE.to_string() } else { String::new() }
}

/// 2026-11-08 (仕事 12): 打った列に、空白なしで打った英語の句 (打ち終えたときに語の間へ空白を入れる所) があるか
fn has_english_phrase(comp: &Composition) -> bool {
    comp.typed_valid && !comp.typed.is_empty() && !super::auto_english::segment_finishing_with_spaces(&comp.typed).1.is_empty()
}

fn typed_for_focus(comp: &Composition) -> Option<String> {
    let focused = comp.segments.get(comp.focus)?;
    if !comp.typed_valid || comp.typed.is_empty() { return None; }
    let reading: String = comp.segments.iter().map(|s| s.reading.as_str()).collect();
    if reading != composition_reading(comp) { return None; }
    if comp.segments.len() == 1 { return Some(comp.typed.clone()); }

    use super::auto_english::{runs, segment, segment_finishing, TokenKind};
    for finishing in [false, true] {
        let mut units = vec![];
        if comp.english_active {
            let tokens = if finishing { segment_finishing(&comp.typed) } else { segment(&comp.typed) };
            for (raw, kind) in runs(&tokens) {
                if kind == TokenKind::English {
                    units.extend(raw.chars().map(|c| super::romaji_style::Unit {
                        kana: c.to_string(), keys: c.to_string(), pending: false,
                    }));
                } else {
                    units.extend(super::romaji_style::simulate(&raw));
                }
            }
        } else {
            units = super::romaji_style::simulate(&comp.typed);
        }
        let generated: String = units.iter().map(|u| u.kana.as_str()).collect();
        if ends_with_pending_n(&generated) {
            if let Some(last) = units.last_mut() { last.kana = "ん".into(); }
        }
        let generated: String = units.iter().map(|u| apply_character_form(&u.kana)).collect();
        if !generated.eq_ignore_ascii_case(&reading) { continue; }
        let start: usize = comp.segments[..comp.focus].iter().map(|s| s.reading.chars().count()).sum();
        let end = start + focused.reading.chars().count();
        let typed: Vec<char> = comp.typed.chars().collect();
        let (mut reading_pos, mut key_pos) = (0, 0);
        let (mut key_start, mut key_end) = (None, None);
        // Only map exact syllable boundaries; a split inside きょ has no exact keystroke slice.
        for unit in &units {
            if reading_pos == start { key_start = Some(key_pos); }
            reading_pos += apply_character_form(&unit.kana).chars().count();
            key_pos += unit.keys.chars().count();
            if reading_pos == end { key_end = Some(key_pos); }
        }
        if let (Some(start), Some(end)) = (key_start, key_end) {
            return typed.get(start..end).map(|keys| keys.iter().collect());
        }
    }
    None
}

fn typed_latin_candidate(reading: &str, typed: Option<&str>) -> Option<String> {
    if !reading.chars().any(|c| ('ぁ'..='ゖ').contains(&c)) { return None; }
    let literal = apply_character_form(typed?);
    if literal.is_empty() || !literal.chars().all(|c| is_latin_letter(c) || "'-".contains(c))
        || !literal.chars().any(is_latin_letter) { return None; }
    // 2026-10-20: 前は英単語かどうかに関係なく、最大 5 通り (打ったとおり・表の大文字・頭だけ大文字・全部大文字・全角) を 4 番目に入れていて、
    // 意図した漢字が下に押し出されていた。2026-10-21: ローマ字として読める綴りは後ろへ (鍵を の候補に kenを が 4 番目に出ていた)。
    // 2026-11-02 (案 A): 1 通りだけ (打ったとおりの形。英単語の表に大文字の形があればその形) を、一覧の本当の末尾に (place_typed_latin_last)。
    // 一覧の頭で ↑ を 1 回押せば届く (select_candidate は rem_euclid で回り込む)。ほかの形 (全部大文字・頭だけ大文字) は F9・F10・無変換で出す。
    // 大文字で打った語 (Kenwo = Ken + を) は読みが英字のままなので、ここには来ない
    let lower = literal.to_lowercase();
    Some(super::auto_english::canonical_case(&lower).map(str::to_string).unwrap_or(literal))
}

/// 打ったとおりの英字の候補を、一覧の本当の末尾 (半角カタカナ・「単漢字…」・絵文字の後ろ) に置く。一覧の頭 (表示している候補) なら動かさない
fn place_typed_latin_last(candidates: &mut Vec<String>, latin: Option<&str>) {
    let Some(latin) = latin else { return };
    if candidates.first().is_some_and(|c| c == latin) { return; }
    candidates.retain(|c| c != latin);
    candidates.push(latin.to_string());
}

/// 2026-10-20: runs の k 番目 (英字) のすぐ前か後ろが、すき間なくローマ字の字 (日本語) か (windows + no・zoom + de)
fn glued_to_japanese(runs: &[(String, super::auto_english::TokenKind)], k: usize) -> bool {
    use super::auto_english::TokenKind;
    let romaji_letter = |run: Option<&(String, TokenKind)>, at_end: bool| run.is_some_and(|(text, kind)| *kind == TokenKind::Romaji
        && if at_end { text.chars().last() } else { text.chars().next() }.is_some_and(|c| c.is_ascii_alphabetic()));
    romaji_letter(k.checked_sub(1).and_then(|i| runs.get(i)), true) || romaji_letter(runs.get(k + 1), false)
}

fn latin_commit_text(text: &str) -> bool {
    let text = apply_character_form(text);
    !text.is_empty() && text.chars().all(|c| (c.is_ascii() && !c.is_ascii_control()) || is_latin_letter(c))
        && text.chars().any(is_latin_letter)
}

fn space_commits_selected_latin(comp: &Composition) -> bool {
    !comp.window_open && comp.latin_selected && latin_commit_text(&comp.converted_text())
}

/// 未確定の文字列の最後が英字 (ローマ字の打ちかけの とだy・ｍｙ も)
fn ends_with_pending_latin(text: &str) -> bool {
    apply_character_form(text).chars().last().is_some_and(|c| c.is_ascii_alphabetic())
}

fn latin_context_input(text: &str) -> bool {
    latin_commit_text(text) || (super::auto_english::in_english_sentence() && !text.is_empty()
        && text.chars().all(|c| c.is_ascii() && !c.is_ascii_control())
        && text.chars().any(|c| c.is_ascii_digit() || c.is_ascii_punctuation()))
}

fn selected_literal_latin(comp: &Composition) -> bool {
    let Some(typed) = typed_for_focus(comp) else { return false };
    let Some(segment) = comp.segments.get(comp.focus) else { return false };
    latin_commit_text(&segment.text)
        && apply_character_form(&segment.text).to_lowercase() == apply_character_form(&typed).to_lowercase()
}

fn smart_space(previous: Option<char>, reverse: bool) -> &'static str {
    let halfwidth = previous.is_some_and(|c| {
        let normalized = apply_character_form(&c.to_string());
        normalized.chars().any(|c| c.is_ascii_alphanumeric() || is_latin_letter(c))
    }) || super::auto_english::in_english_sentence();
    if halfwidth != reverse { " " } else { "　" }
}

fn windows_candidates(reading: &str) -> Vec<String> {
    super::windows_text::conversions(reading)
}

/// 2026-10-22: 遅れて来た Windows の候補を、出している一覧に足す (一覧を作ったときと同じ足し方)。
/// 選んでいる候補は変えない (一覧の中の位置は変わりうる)。選んでいる候補が一覧に無ければ足さない
fn merge_late_windows_candidates(segment: &mut Segment, windows: Vec<String>) {
    let mut candidates = segment.candidates.clone();
    merge_windows_candidates(&mut candidates, &segment.reading, windows);
    let mut candidates = finish_candidate_list(candidates, &segment.reading);
    place_typed_latin_last(&mut candidates, segment.typed_latin.as_deref());
    if let Some(index) = candidates.iter().position(|candidate| candidate == &segment.text) {
        segment.candidates = candidates;
        segment.index = index as i32;
    }
}

/// 2026-10-20: 記号だけの候補 (はてな → ？・から → ～・かっこ → ())。変換サーバーと同じく、覚えていなければ候補の後ろへ回す
/// (〇 だけの候補は ○ と同じ形なので記号に数える)
fn symbol_only(text: &str) -> bool {
    !text.is_empty() && (text.chars().all(|c| c == '〇')
        || (text.chars().all(|c| !c.is_alphanumeric()) && text.chars().any(|c| !c.is_whitespace())))
}

fn merge_windows_candidates(candidates: &mut Vec<String>, reading: &str, windows: Vec<String>) {
    // Windows の記号だけの候補は前へ繰り上げず、無ければ一覧の最後に足す (学習しているかはサーバーの並びに任せる)
    let (symbols, windows): (Vec<String>, Vec<String>) = windows.into_iter().partition(|text| symbol_only(text));
    for (rank, text) in windows.iter().take(3).enumerate() {
        if let Some(position) = candidates.iter().position(|candidate| candidate == text) {
            if position > 3 {
                let promoted = candidates.remove(position);
                candidates.insert((rank + 1).min(candidates.len()), promoted);
            }
        }
    }

    let mut added = 0;
    for text in windows {
        if candidates.contains(&text) {
            continue;
        }
        let position = if added < 5 {
            (3 + added).min(candidates.len())
        } else {
            candidates.iter().position(|candidate| candidate == reading).unwrap_or(candidates.len())
        };
        candidates.insert(position, text);
        added += 1;
    }
    for text in symbols {
        if !candidates.contains(&text) { candidates.push(text); }
    }
}

#[cfg(test)]
mod local_kana_tests {
    use super::{local_kana_step, LocalKana, LocalKey};

    fn step(pending: &str, key: LocalKey) -> (String, String, bool) {
        let LocalKana { commit, pending, eaten } = local_kana_step(pending, key);
        (commit, pending, eaten)
    }

    fn typed(keys: &str) -> String {
        let (mut text, mut pending) = (String::new(), String::new());
        for c in keys.chars() {
            let (commit, rest, _) = step(&pending, LocalKey::Char(c));
            text += &commit;
            pending = rest;
        }
        text + &step(&pending, LocalKey::Enter).0
    }

    #[test]
    fn romaji_becomes_hiragana_without_the_server() {
        assert_eq!(step("", LocalKey::Char('k')), ("".into(), "k".into(), true));
        assert_eq!(step("k", LocalKey::Char('a')), ("か".into(), "".into(), true));
        assert_eq!(step("ky", LocalKey::Char('a')), ("きゃ".into(), "".into(), true));
        assert_eq!(step("n", LocalKey::Char('k')), ("ん".into(), "k".into(), true));
        assert_eq!(step("k", LocalKey::Char('k')), ("っ".into(), "k".into(), true));
        assert_eq!(typed("kyouhaiitenkidesu."), "きょうはいいてんきです。");
        assert_eq!(typed("konnnichiha,sinbun-"), "こんにちは、しんぶんー");
        assert_eq!(typed("2018nenn"), "2018ねん");
    }

    #[test]
    fn pending_romaji_is_finished_or_dropped() {
        // 末尾の n は確定で ん、ほかの打ちかけは打ったまま
        assert_eq!(step("n", LocalKey::Enter), ("ん".into(), "".into(), true));
        assert_eq!(step("sh", LocalKey::Space), ("sh".into(), "".into(), true));
        assert_eq!(step("ky", LocalKey::Backspace), ("".into(), "k".into(), true));
        assert_eq!(step("k", LocalKey::Escape), ("".into(), "".into(), true));
        assert_eq!(step("n", LocalKey::Char('.')), ("ん。".into(), "".into(), true));
        // 打ちかけが無ければ、Enter・Space・Backspace・ほかのキーはアプリへ渡す
        for key in [LocalKey::Enter, LocalKey::Space, LocalKey::Backspace, LocalKey::Escape, LocalKey::Other] {
            assert!(!step("", key).2, "{key:?}");
        }
        // 打ちかけは 8 字まで。超えたら打ったまま確定して、次の字から数え直す
        assert_eq!(step("bcdfghjk", LocalKey::Char('l')), ("bcdfghjk".into(), "l".into(), true));
    }
}

/// 2026-10-29 (再点検の試験の穴): handle_action をそのまま通す試験。試験の中で立てる偽の変換サーバーを IME の状態に入れて呼ぶ
/// (TSF の文脈が無いので、入力中の表示は作られず、終わると入力の状態は None になる。動作が書き戻した文節・印を確かめる)
#[cfg(test)]
mod handle_action_tests {
    use super::*;
    use super::super::ipc_service::{test_services::{fake_services, GLOBAL_STATE}, EventBudget};
    use std::time::{Duration, Instant};

    /// Shift で打つ人 (input.shift_ascii_mode) の一時的な英字を、handle_action と同じ順 (update_temp_ascii → かな に戻す位置) で 1 字ずつ打ち、
    /// 英字のまま入った所を [ ] で囲んで返す (変換サーバーは使わない。画面の字は打った字のまま)。
    /// rules が None なら区切りの道具を使わない。old は 2026-11-03 より前の数え方 (数字で数え直す)
    fn shift_typing(input: &str, rules: Option<super::super::auto_english::TempAsciiRules>, old: bool) -> String {
        let mut comp = Composition::default();
        comp.reset();
        let mut ascii: Vec<bool> = vec![];
        for ch in input.chars() {
            // 下付き・上付きの数字 (C_2 の ₂) は、上付き・下付きの入力 (insert_script_char) が入れる。一時的な英数の状態は変えない
            if "₀₁₂₃₄₅₆₇₈₉⁰¹²³⁴⁵⁶⁷⁸⁹".contains(ch) {
                ascii.push(comp.temp_ascii);
                comp.hiragana.push(ch);
                comp.caret = comp.hiragana_len();
                continue;
            }
            let temp = if old {
                // 前の update_temp_ascii
                if !ch.is_ascii() || ch.is_ascii_control() { comp.temp_ascii = false; comp.shifted_count = 0; }
                else if ch.is_ascii_uppercase() { comp.temp_ascii = true; comp.shifted_count += 1; }
                else if ch.is_ascii_lowercase() { if comp.shifted_count > 1 { comp.temp_ascii = false; } comp.shifted_count = 0; }
                else { comp.shifted_count = 0; }
                comp.temp_ascii
            } else {
                TextServiceFactory::update_temp_ascii(&mut comp, &ch.to_string())
            };
            match rules.filter(|_| temp).and_then(|r| TextServiceFactory::temp_ascii_return_point(&comp, ch, r)) {
                Some((delete, _)) => {
                    let n = ascii.len();
                    ascii[n - delete..].iter_mut().for_each(|a| *a = false);
                    ascii.push(false);
                    comp.temp_ascii = false;
                    comp.shifted_count = 0;
                    comp.temp_ascii_start = None;
                }
                None => ascii.push(temp),
            }
            comp.hiragana.push(ch);
            comp.caret = comp.hiragana_len();
        }
        let mut out = String::new();
        for (k, ch) in input.chars().enumerate() {
            if ascii[k] && (k == 0 || !ascii[k - 1]) { out.push('['); }
            out.push(ch);
            if ascii[k] && (k + 1 == ascii.len() || !ascii[k + 1]) { out.push(']'); }
        }
        out
    }

    /// 2026-11-03: Shift の一時的な英字が、化学式・型番・知っている語・頭文字のあとで かな に戻る。読める名前は英字のまま (Mozc の決まりどおり)
    #[test]
    fn shift_ascii_returns_to_kana_after_words() {
        let rules = Some(super::super::auto_english::TEMP_ASCII_RULES);
        super::super::auto_english::set_test_names(SHIFT_TEST_NAMES);
        // 大文字 2 つ以上のあとの小文字は前から かな (数字を挟んでも数え直さない)
        assert_eq!(shift_typing("NHKnonyu-su", None, false), "[NHK]nonyu-su");
        assert_eq!(shift_typing("CO2nituite", None, false), "[CO2]nituite");
        assert_eq!(shift_typing("CO2nituite", None, true), "[CO2nituite]");
        assert_eq!(shift_typing("RTX4090de", None, false), "[RTX4090]de");
        if !super::super::auto_english::enabled() { return; }
        super::super::auto_english::set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        let cases = [
            ("C2nituite", "[C2]nituite"), ("C₂nituite", "[C₂]nituite"), ("Fe3O4nituite", "[Fe3O4]nituite"), ("E5kei", "[E5]kei"),
            ("Googlede", "[Google]de"), ("Pythonno", "[Python]no"), ("Asann", "[A]sann"), ("Xsenn", "[X]senn"), ("Jri-gu", "[J]ri-gu"),
            // 読める名前は英字のまま (区切りは Aiko を A + いこ、Haruto を Haru + と に割る。名前の表にある名前は表で守る)。
            // 2026-11-04 (仕事 9c): 表の英単語 + 助詞は、ローマ字として読める語 (ken) でも切る (Ken + の)。名前の途中 (Takemoto・Shinya) は名前の表で守る
            ("Aiko", "[Aiko]"), ("Haruto", "[Haruto]"), ("Hanako", "[Hanako]"), ("Tomoko", "[Tomoko]"), ("Kenno", "[Ken]no"),
            // 英単語・社名の打ちかけでは決めない
            ("Github", "[Github]"), ("Youtube", "[Youtube]"),
        ];
        for (input, expected) in cases {
            assert_eq!(shift_typing(input, rules, false), expected, "{input}");
        }
    }

    /// Shift の試験で使う名前の表 (名前の表はスレッドごとに差し込むので、どの試験も同じものを入れる)
    const SHIFT_TEST_NAMES: &[&str] = &["aiko", "ai", "haruto", "hanako", "takemoto", "take", "tomoko"];

    /// 2026-11-03 (仕事 9c): 名前の表で守って、もっと かな に戻す形 (TempAsciiRules の name_guard・syllable_guard。11-04 から初期値)
    #[test]
    fn shift_ascii_name_guard_returns_more() {
        use super::super::auto_english::{set_test_names, set_test_words, TempAsciiRules, TEMP_ASCII_RULES};
        if !super::super::auto_english::enabled() { return; }
        set_test_words(&["google", "test", "hello", "windows", "home", "game", "sushi", "python", "ken", "anki", "shop", "check", "think", "github", "youtube", "type", "main", "push", "chat", "two", "able", "tests", "login"]);
        set_test_names(SHIFT_TEST_NAMES);
        let rules = Some(TEMP_ASCII_RULES);
        for (input, expected) in [
            // 大文字 1 字 + 読めるローマ字・読める表の英単語 + 助詞
            ("Amondai", "[A]mondai"), ("Bmondai", "[B]mondai"), ("Ssaizu", "[S]saizu"), ("Homede", "[Home]de"),
            // 名前 + 助詞・敬称は名前の後ろで切る。名前とその打ちかけは英字のまま
            ("Aikoha", "[Aiko]ha"), ("Takemotosann", "[Takemoto]sann"), ("Aiko", "[Aiko]"), ("Haruto", "[Haruto]"), ("Takemoto", "[Takemoto]"),
            // 表に無い名前も、頭の 2 字が音節の頭なら大文字 1 字で切らない
            ("Kotomi", "[Kotomi]"), ("Shimizu", "[Shimizu]"),
            // 今までどおり
            ("Googlede", "[Google]de"), ("Github", "[Github]"), ("C2nituite", "[C2]nituite"),
        ] {
            assert_eq!(shift_typing(input, rules, false), expected, "{input}");
        }
        // syllable_guard が無ければ、表に無い名前は割れる (測りで比べる形)
        let loose = Some(TempAsciiRules { syllable_guard: false, ..TEMP_ASCII_RULES });
        assert_eq!(shift_typing("Kotomi", loose, false), "[K]otomi");
    }

    /// 2026-11-06 (仕事 34): 引用符の向き・Space 2 回のピリオド・文の頭の大文字の形。どれも初期値オフ
    #[test]
    fn ios_assist_shapes() {
        assert_eq!(smart_quote('"', None), '\u{201C}');
        assert_eq!(smart_quote('"', Some("say ")), '\u{201C}');
        assert_eq!(smart_quote('"', Some("word")), '\u{201D}');
        assert_eq!(smart_quote('\'', Some("don")), '\u{2019}');
        assert_eq!(smart_quote('\'', Some("(")), '\u{2018}');
        assert!(period_shortcut_follows("ok ") && period_shortcut_follows("a2 "));
        assert!(!period_shortcut_follows("ok  ") && !period_shortcut_follows(". ") && !period_shortcut_follows(" ") && !period_shortcut_follows("ok"));
        assert!(sentence_starts_after("ok. ") && sentence_starts_after("why?  ") && sentence_starts_after("line\n"));
        assert!(!sentence_starts_after("") && !sentence_starts_after("ok ") && !sentence_starts_after("v1.2") && !sentence_starts_after("ok."));
        assert!(!ios_assist(IosAssist::Period) && !ios_assist(IosAssist::Capital));
    }

    /// 2026-11-06 (設定 input.smart_punctuation): 半角英数の -- → —。直前が - (その前は - でない) のときだけ。初期値オフ
    #[test]
    fn smart_dash_only_after_a_single_hyphen() {
        assert!(smart_dash_follows("a-") && smart_dash_follows("-"));
        assert!(!smart_dash_follows("--") && !smart_dash_follows("a") && !smart_dash_follows("") && !smart_dash_follows("\u{2014}"));
        assert!(!smart_punctuation_enabled());
        SMART_PUNCTUATION_FOR_TESTS.with(|cell| cell.set(Some(true)));
        assert!(smart_punctuation_enabled());
        SMART_PUNCTUATION_FOR_TESTS.with(|cell| cell.set(None));
    }

    /// 2026-11-06 (仕事 20 の手 2): 母音を 1 字足した読み (かぢ → かだい) と、一覧に出すものの選び方
    #[test]
    fn one_vowel_lookups() {
        let spelling = super::super::auto_english::spelling();
        let alternatives = one_vowel_readings("かぢ", &spelling, 40);
        assert!(alternatives.contains(&"かだい".to_string()), "{alternatives:?}");
        assert!(!alternatives.contains(&"かぢ".to_string()));
        assert!(alternatives.len() <= 40);
        // っ の前・ん には足さない (がっこう の っ・ほん の ん)
        assert!(one_vowel_readings("ほん", &spelling, 40).iter().all(|r| r.ends_with('ん')));
        assert!(one_vowel_readings("abc", &spelling, 40).is_empty());
        let s = |d: f32, l: f32| (Some(d), Some(l));
        assert_eq!(pick_vowel_lookups(&[s(-20.0, -40.0), s(-15.0, -30.0), s(-25.0, -20.0), s(-18.0, -33.0), s(-10.0, -39.0)], 6.0), vec![1, 3]);
        assert!(pick_vowel_lookups(&[(Some(-20.0), None), s(-10.0, -10.0)], 6.0).is_empty());
    }

    /// 2026-11-06 (仕事 20 の手 2): 線の決め直しのための書き出し (AZK_VOWEL_LOOKUP_PROBE に 1 行 1 読み。<path>.out.tsv に 読み<TAB>足した読み…)
    #[test]
    fn vowel_lookup_probe() {
        let Ok(path) = std::env::var("AZK_VOWEL_LOOKUP_PROBE") else { return };
        let spelling = super::super::auto_english::spelling();
        let mut out = String::new();
        for reading in std::fs::read_to_string(&path).unwrap().lines().filter(|l| !l.is_empty()) {
            out += &format!("{reading}\t{}\n", one_vowel_readings(reading, &spelling, 40).join("\t"));
        }
        std::fs::write(format!("{path}.out.tsv"), out).unwrap();
    }

    /// 2026-11-06 (仕事 20 の手 1): 自動で直すほどではない母音の抜けを、一覧に出す候補として覚える。補った読みのほうが補う前より AI の点で上のものを、
    /// 点の高い順に。選んで確定したら補った読みで覚える
    #[test]
    fn typo_suggestions_for_the_candidate_list() {
        let s = |lm: f32| (Some(-10.0), Some(lm));
        assert_eq!(typo_suggestion_order(&[s(-30.0), s(-31.0), s(-28.0), s(-29.5)]), vec![2, 3]);
        assert!(typo_suggestion_order(&[s(-30.0), s(-31.0)]).is_empty());
        assert!(typo_suggestion_order(&[(Some(-10.0), None), s(-20.0)]).is_empty());
        // 文節の読みの中の所 (かだい を kd と打った: 読み かd の d を だい に)
        let spot = RepairedSpot { offset: 5, kana: "だい".into(), typed: "d".into() };
        assert_eq!(suggested_segment_reading("かd", 4, &spot).as_deref(), Some("かだい"));
        assert_eq!(suggested_segment_reading("きょうのかd", 0, &RepairedSpot { offset: 5, kana: "だい".into(), typed: "d".into() }).as_deref(), Some("きょうのかだい"));
        assert!(suggested_segment_reading("きょうは", 0, &spot).is_none());
        let segment = Segment { reading: "かd".into(), text: "課題".into(), repair_readings: vec![("課題".into(), "かだい".into())], ..Default::default() };
        assert_eq!(learning_reading(&segment), "かだい");
        assert_eq!(learning_reading(&Segment { text: "かd".into(), ..segment }), "かd");
    }

    /// 2026-11-03: Shift で打つ人の一時的な英字を、規則ごとに打つ (AZK_SHIFT_PROBE に 1 行 1 入力。<path>.out.tsv に 入力<TAB>前<TAB>数え方だけ<TAB>規則ごと…)
    #[test]
    fn shift_ascii_probe() {
        use super::super::auto_english::{TempAsciiRules, TEMP_ASCII_RULES};
        let Ok(path) = std::env::var("AZK_SHIFT_PROBE") else { return };
        let none = TempAsciiRules { after_digit: false, known_particle: false, known_unreadable: false, initial_honorific: false, unreadable_tail: 0, any: false,
            name_guard: false, syllable_guard: false };
        let variants: Vec<(&str, Option<TempAsciiRules>, bool)> = vec![
            ("old", None, true), ("count", None, false), ("prod", Some(TEMP_ASCII_RULES), false),
            ("any", Some(TempAsciiRules { any: true, ..none }), false),
            ("digit", Some(TempAsciiRules { after_digit: true, ..none }), false),
            ("digit_known", Some(TempAsciiRules { after_digit: true, known_particle: true, known_unreadable: true, ..none }), false),
            ("no_unreadable", Some(TempAsciiRules { unreadable_tail: 0, ..TEMP_ASCII_RULES }), false),
            ("unreadable3", Some(TempAsciiRules { unreadable_tail: 3, ..TEMP_ASCII_RULES }), false),
            ("no_initial", Some(TempAsciiRules { initial_honorific: false, ..TEMP_ASCII_RULES }), false),
            ("known_readable", Some(TempAsciiRules { known_unreadable: false, ..TEMP_ASCII_RULES }), false),
            // 2026-11-03 (仕事 9c): 名前の表で守って、もっと戻す形 (C) は 11-04 から初期値 (prod)。前の形 (名前の守りなし) と、音節の頭の決まり (syllable_guard) の無い形も
            ("before_9c", Some(TempAsciiRules { name_guard: false, syllable_guard: false, ..TEMP_ASCII_RULES }), false),
            ("C_nosyl", Some(TempAsciiRules { syllable_guard: false, ..TEMP_ASCII_RULES }), false),
        ];
        let mut out = format!("# 入力\t{}\n", variants.iter().map(|v| v.0).collect::<Vec<_>>().join("\t"));
        for line in std::fs::read_to_string(&path).unwrap().lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let input = line.split('\t').next().unwrap();
            let shown: Vec<String> = variants.iter().map(|(_, rules, old)| shift_typing(input, *rules, *old)).collect();
            out += &format!("{input}\t{}\n", shown.join("\t"));
        }
        std::fs::write(format!("{path}.out.tsv"), out).unwrap();
    }

    /// 2026-11-01: 上付きの数字の途中でふつうの字 (x) を打ったら、上付きの状態を終える (x^2x=1 の 1 を ¹ にしない)。数字なら続ける
    #[test]
    fn plain_char_ends_superscript_digits() {
        use super::super::scripts::ScriptState;
        let mut comp = Composition { hiragana: "x²".into(), caret: 2, script: ScriptState::Digits(true), ..Default::default() };
        TextServiceFactory::end_script_on_plain_char(&mut comp, "x");
        assert_eq!(comp.script, ScriptState::Off);
        if super::super::scripts::enabled() {
            comp.script = ScriptState::Digits(true);
            TextServiceFactory::end_script_on_plain_char(&mut comp, "3");
            assert_eq!(comp.script, ScriptState::Digits(true));
        }
    }

    /// IME の状態 (プロセスで 1 つ) に通信路を入れているあいだ。落とすと空に戻す
    struct Installed(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

    impl Installed {
        fn new(ipc: Option<IPCService>) -> Self {
            let one = GLOBAL_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            IMEState::get().unwrap().ipc_service = ipc;
            Installed(one)
        }
    }

    impl Drop for Installed {
        fn drop(&mut self) {
            if let Ok(mut state) = IMEState::get() {
                state.ipc_service = None;
            }
        }
    }

    fn factory_with(comp: Composition) -> TextServiceFactory {
        let factory = TextServiceFactory::default();
        *factory.borrow().unwrap().borrow_mut_composition().unwrap() = comp;
        factory
    }

    fn composition_of(factory: &TextServiceFactory) -> Composition {
        factory.borrow().unwrap().borrow_composition().unwrap().clone()
    }

    fn set_composition(factory: &TextServiceFactory, comp: Composition) {
        *factory.borrow().unwrap().borrow_mut_composition().unwrap() = comp;
    }

    fn seg(reading: &str, text: &str) -> Segment {
        Segment { reading: reading.into(), text: text.into(), ..Default::default() }
    }

    /// 2026-11-19: F6・F7 は、英字と判定した所も、ローマ字として読めるなら かな にする (無変換キーと同じ決まり)。読めない綴り (google) は英字のまま。
    /// Space の前 (入力全体を 1 文節) とあと (注目文節だけ) の両方
    #[test]
    fn f6_f7_turn_readable_latin_into_kana() {
        assert_eq!(type_text(SetTextType::Hiragana, "きょうはgoogleでけんさく", None), "きょうはgoogleでけんさく");
        assert_eq!(type_text(SetTextType::Hiragana, "きょうはmadeいい", None), "きょうはまでいい");
        assert_eq!(type_text(SetTextType::Katakana, "きょうはmadeいい", None), "キョウハマデイイ");
        assert_eq!(type_text(SetTextType::Katakana, "きょうはgoogleでけんさく", None), "キョウハgoogleデケンサク");
        let (_fake, ipc, _health) = fake_services("f6", &[]);
        let _installed = Installed::new(Some(ipc));
        // Space の前: 入力全体を 1 文節にして替える
        for (hiragana, f6, f7) in [("きょうはgoogleでけんさく", "きょうはgoogleでけんさく", "キョウハgoogleデケンサク"),
                                   ("きょうはmadeいい", "きょうはまでいい", "キョウハマデイイ")] {
            for (set_type, want) in [(SetTextType::Hiragana, f6), (SetTextType::Katakana, f7)] {
                let factory = factory_with(Composition {
                    state: CompositionState::Composing, hiragana: hiragana.into(), caret: hiragana.chars().count() as i32, ..Default::default()
                });
                factory.handle_action(&[ClientAction::ConvertAsOne, ClientAction::ConvertType(set_type)], CompositionState::Converting).unwrap();
                assert_eq!(composition_of(&factory).segments.iter().map(|s| s.text.as_str()).collect::<String>(), want, "{hiragana} {set_type:?}");
            }
        }
        // Space のあと: 注目文節だけを替える (ほかの文節はそのまま)
        for (english, f6) in [("made", "まで"), ("google", "google")] {
            let factory = factory_with(Composition {
                state: CompositionState::Converting, hiragana: format!("きょうは{english}いい"), caret: 0,
                segments: vec![seg("きょうは", "今日は"), seg(english, english), seg("いい", "良い")], focus: 1, ..Default::default()
            });
            factory.handle_action(&[ClientAction::ConvertType(SetTextType::Hiragana)], CompositionState::Converting).unwrap();
            let texts: Vec<String> = composition_of(&factory).segments.iter().map(|s| s.text.clone()).collect();
            assert_eq!(texts, vec!["今日は".to_string(), f6.to_string(), "良い".to_string()]);
        }
    }

    /// レビュー H1: キーの上限 (EVENT_BUDGET の代わりに 1 秒) の中で Space を押しても、AI の変換 (1.3 秒) を待って変換する
    #[test]
    fn space_waits_for_a_slow_server_through_handle_action() {
        let (_fake, ipc, _health) = fake_services("h1a", &[("convert", 1300)]);
        let _installed = Installed::new(Some(ipc));
        let factory = factory_with(Composition {
            state: CompositionState::Composing, hiragana: "きょうは".into(), caret: 4, display: "きょうは".into(), ..Default::default()
        });
        let started = Instant::now();
        let result = {
            let _key = EventBudget::start_with(Duration::from_millis(1000));
            factory.handle_action(&[ClientAction::Convert], CompositionState::Converting)
        };
        assert!(result.is_ok(), "{result:?}");
        assert!(started.elapsed() >= Duration::from_millis(1250), "{:?}", started.elapsed());
        assert_eq!(composition_of(&factory).segments.iter().map(|s| s.reading.as_str()).collect::<String>(), "きょうは");
    }

    /// 2026-11-10 (Fable の諮問 ux 1): ライブ変換の表示中の Esc。表示が読みと違えば 1 回目は読みに戻す (ShowReading)。読みのまま・読みに戻したあと・
    /// 予測の候補の窓が出ているとき・ライブ変換でないときは、今までどおり消す (Cancel)
    #[test]
    fn escape_first_shows_the_reading_in_live_conversion() {
        let live = |display: &str| Composition {
            state: CompositionState::Composing, live: true, hiragana: "きょうはいいてんき".into(), caret: 9,
            display: display.into(), ..Default::default()
        };
        assert!(TextServiceFactory::escape_shows_reading(&live("今日はいい天気")));
        assert!(!TextServiceFactory::escape_shows_reading(&live("きょうはいいてんき")));
        assert!(!TextServiceFactory::escape_shows_reading(&Composition { live_reading: true, ..live("今日はいい天気") }));
        assert!(!TextServiceFactory::escape_shows_reading(&Composition { live: false, ..live("今日はいい天気") }));
        assert!(!TextServiceFactory::escape_shows_reading(&Composition { hiragana: String::new(), caret: 0, ..live("") }));
        assert!(!TextServiceFactory::escape_shows_reading(&Composition {
            predictions: vec!["今日はいい天気ですね".into()], predicted_for: "きょうはいいてんき".into(), ..live("今日はいい天気") }));
        // 予測の候補が前の読みのもの (窓は出していない) なら、読みに戻す
        assert!(TextServiceFactory::escape_shows_reading(&Composition {
            predictions: vec!["今日は".into()], predicted_for: "きょうは".into(), ..live("今日はいい天気") }));
        // 読みに戻した表示は、時計からの清書・予測と、何もしないキー (動きが空) では終わらず、読みを変える・動かす・変換するキーで終わる
        assert!(!ends_reading_shown(&[ClientAction::ShowReading], false));
        assert!(!ends_reading_shown(&[ClientAction::RefineLive], true));
        assert!(!ends_reading_shown(&[ClientAction::ShowPredictions], true));
        assert!(!ends_reading_shown(&[], false));
        assert!(ends_reading_shown(&[ClientAction::Insert("a".into())], false));
        assert!(ends_reading_shown(&[ClientAction::MoveCaret(-1)], false));
        assert!(ends_reading_shown(&[ClientAction::Convert], false));
    }

    /// 2026-11-10 (Fable の諮問 quality 3、A 11-08 20:02): 文節が 1 つだけのときの文節ごとの確定 (CommitFirstSegment) も、全体の確定と同じく学習に送り、
    /// 確定の取り消しの控えを残す。偽の変換サーバーの learn を 400ms 待たせ、その待ちで学習に送ったことを見る。予測の候補から来た文節は送らない
    /// (字はカタカナ: 漢字を含む字は予測の履歴 (%APPDATA% の phrases.tsv) に書くので使わない)
    #[test]
    fn committing_the_only_segment_learns_it() {
        let (_fake, ipc, _health) = fake_services("cfs", &[("learn", 400)]);
        let _installed = Installed::new(Some(ipc));
        let converting = || Composition {
            state: CompositionState::Converting, hiragana: "きょう".into(), caret: 3, segments: vec![seg("きょう", "キョウ")], ..Default::default()
        };
        let factory = factory_with(converting());
        let started = Instant::now();
        factory.handle_action(&[ClientAction::CommitFirstSegment], CompositionState::Converting).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(380), "{:?}", started.elapsed());
        let comp = composition_of(&factory);
        assert_eq!(comp.state, CompositionState::None);
        assert!(comp.segments.is_empty() && comp.hiragana.is_empty());
        assert_eq!(comp.last_commit.as_ref().map(|last| last.text.as_str()), Some("キョウ"));

        let factory = factory_with(Composition { from_prediction: true, ..converting() });
        let started = Instant::now();
        factory.handle_action(&[ClientAction::CommitFirstSegment], CompositionState::Converting).unwrap();
        assert!(started.elapsed() < Duration::from_millis(300), "{:?}", started.elapsed());
        assert_eq!(composition_of(&factory).state, CompositionState::None);
    }

    /// 2026-11-10: ShowReading は未確定を消さず、ライブ変換の文節 (と、変換中のカーソルで打っていた単位) を捨てて、読みに戻した印を付ける。
    /// 2026-11-19: 上限で切って裏で待っている清書も捨て、届いた答えがあっても読みを見せている間は文節を替えない
    #[test]
    fn show_reading_keeps_the_composition() {
        use super::super::ipc_service::late_refine_pending_for_tests;
        {
            let (_fake, ipc, _health) = fake_services("esc", &[]);
            let _installed = Installed::new(Some(ipc));
            let factory = factory_with(Composition {
                state: CompositionState::Composing, live: true, refined: true, hiragana: "きょうはいいてんき".into(), caret: 9,
                segments: vec![seg("きょうは", "今日は"), seg("いいてんき", "いい天気")], segments_reading: "きょうはいいてんき".into(),
                display: "今日はいい天気".into(), edit_unit: true, edit_tail: vec![seg("いいてんき", "いい天気")], ..Default::default()
            });
            factory.handle_action(&[ClientAction::ShowReading], CompositionState::Composing).unwrap();
            let comp = composition_of(&factory);
            assert!(comp.live_reading);
            assert_eq!((comp.hiragana.as_str(), comp.caret), ("きょうはいいてんき", 9));
            assert!(comp.segments.is_empty() && comp.segments_reading.is_empty() && !comp.refined);
            assert!(!comp.edit_unit && comp.edit_tail.is_empty());
        }
        // 時計からの清書 (AI の変換 1.8 秒) を 1.5 秒で切り、裏で待っている間に Esc で読みに戻す
        let (_fake, ipc, _health) = fake_services("esc-late", &[("convert", 1800)]);
        let _installed = Installed::new(Some(ipc));
        let factory = factory_with(Composition {
            state: CompositionState::Composing, live: true, hiragana: "きょうはいいてんき".into(), caret: 9, display: "dきょうはいいてんき".into(),
            segments: vec![seg("きょうはいいてんき", "dきょうはいいてんき")], segments_reading: "きょうはいいてんき".into(), ..Default::default()
        });
        {
            let _key = EventBudget::start();
            factory.handle_action(&[ClientAction::RefineLive], CompositionState::Composing).unwrap();
        }
        assert_eq!(late_refine_pending_for_tests().as_deref(), Some("きょうはいいてんき"));
        // 試験の工場には TSF の未確定の文字列が無いので、handle_action の終わりで状態が None に戻る。入力中に戻して続ける
        let mut back = composition_of(&factory);
        back.state = CompositionState::Composing;
        set_composition(&factory, back);
        factory.handle_action(&[ClientAction::ShowReading], CompositionState::Composing).unwrap();
        assert!(late_refine_pending_for_tests().is_none(), "the late refine must be dropped");
        let mut shown = composition_of(&factory);
        shown.state = CompositionState::Composing;
        set_composition(&factory, shown.clone());
        assert!(shown.live_reading && shown.segments.is_empty());
        // 答えが届いても (時計が止まる前に届いていた形)、読みを見せている間は使わない
        assert!(!late_refine_applies(&shown, "きょうはいいてんき"));
        LATE_RESULT.with(|r| *r.borrow_mut() = Some(("きょうはいいてんき".into(), vec![("今日はいい天気".into(), 9)])));
        factory.apply_late_refine().unwrap();
        LATE_RESULT.with(|r| r.borrow_mut().take());
        let comp = composition_of(&factory);
        assert!(comp.live_reading && comp.segments.is_empty() && !comp.refined, "{comp:?}");
    }

    /// レビュー M2・再点検 R1: 句読点での AI の変換 (2 秒) は 1.5 秒で切り、問い合わせ直さずに、見えている辞書の仮表示に句読点を足して変換したことにする。
    /// 仮表示の後ろにかなも増えていれば、変換せずに入力を続け、見せるもの (仮表示 + かな) を今の読みのものにする (描画で問い合わせ直さない)
    #[test]
    fn punctuation_does_not_ask_again_after_cutting_the_ai() {
        let live = |segments: Vec<Segment>, segments_reading: &str| Composition {
            state: CompositionState::Composing, live: true, hiragana: "きょうはいいてんき。".into(), caret: 10,
            segments, segments_reading: segments_reading.into(), ..Default::default()
        };
        let (_fake, ipc, health) = fake_services("m2a", &[("convert", 2000)]);
        let _installed = Installed::new(Some(ipc));
        let factory = factory_with(live(vec![seg("きょうは", "今日は"), seg("いいてんき", "いい天気")], "きょうはいいてんき"));
        let started = Instant::now();
        let result = {
            let _key = EventBudget::start();
            factory.handle_action(&[ClientAction::PunctuationConvert], CompositionState::Converting)
        };
        let elapsed = started.elapsed();
        assert!(result.is_ok(), "{result:?}");
        assert!(elapsed >= Duration::from_millis(1450) && elapsed < Duration::from_millis(1900), "{elapsed:?}");
        let comp = composition_of(&factory);
        assert_eq!(comp.segments.iter().map(|s| s.text.as_str()).collect::<String>(), "今日はいい天気。");
        assert!(comp.punctuation_converted && !comp.refined);
        assert!(health.ai.slow_for_at(10, super::super::ipc_service::now_ms_for_tests()));
        drop(_installed);

        let (_fake, ipc, _health) = fake_services("r1a", &[("convert", 2000)]);
        let _installed = Installed::new(Some(ipc));
        let factory = factory_with(live(vec![seg("きょうは", "今日は")], "きょうは"));
        let started = Instant::now();
        let result = {
            let _key = EventBudget::start();
            factory.handle_action(&[ClientAction::PunctuationConvert], CompositionState::Converting)
        };
        assert!(result.is_ok(), "{result:?}");
        assert!(started.elapsed() < Duration::from_millis(1900), "{:?}", started.elapsed());
        let comp = composition_of(&factory);
        assert_eq!(comp.segments.iter().map(|s| s.text.as_str()).collect::<String>(), "今日はいいてんき。");
        assert_eq!(comp.segments_reading, "きょうはいいてんき。");
        assert!(!comp.punctuation_converted);
    }

    /// 2026-11-06 (仕事 29 の 2): 時計からの清書 (AI の変換 1.8 秒) は今までどおり 1.5 秒で切って仮表示のままにするが、問い合わせは裏で待ち続け、
    /// 届いたとき読みが同じなら清書に替える。読みが変わっていたら替えない。次の清書を始めたら、前の待ちはやめる
    #[test]
    fn a_late_refine_is_used_only_while_the_reading_is_unchanged() {
        use super::super::ipc_service::{late_refine_pending_for_tests, poll_late_refine, LateRefineState};
        let live = |hiragana: &str| Composition {
            state: CompositionState::Composing, live: true, hiragana: hiragana.into(), caret: hiragana.chars().count() as i32,
            display: hiragana.into(), segments: vec![seg(hiragana, &format!("d{hiragana}"))], segments_reading: hiragana.into(), ..Default::default()
        };
        let wait_ready = || {
            let deadline = Instant::now() + Duration::from_millis(4000);
            loop {
                match poll_late_refine() {
                    LateRefineState::Pending if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                    other => return other,
                }
            }
        };
        for changed in [false, true] {
            let (_fake, ipc, _health) = fake_services(if changed { "late2" } else { "late1" }, &[("convert", 1800)]);
            let _installed = Installed::new(Some(ipc));
            let factory = factory_with(live("きょうはいいてんき"));
            let started = Instant::now();
            let result = {
                let _key = EventBudget::start();
                factory.handle_action(&[ClientAction::RefineLive], CompositionState::Composing)
            };
            assert!(result.is_ok(), "{result:?}");
            assert!(started.elapsed() < Duration::from_millis(1750), "{:?}", started.elapsed());
            assert!(!composition_of(&factory).refined);
            assert_eq!(late_refine_pending_for_tests().as_deref(), Some("きょうはいいてんき"));
            // 試験の工場には TSF の未確定の文字列が無いので、handle_action の終わりで状態が None に戻る。入力中に戻して続ける
            let mut back = composition_of(&factory);
            back.state = CompositionState::Composing;
            set_composition(&factory, back);
            if changed {
                // 届く前に 1 字打った (読みが変わった)
                set_composition(&factory, live("きょうはいいてんきだ"));
            }
            let LateRefineState::Ready(reading, converted) = wait_ready() else { panic!("the late refine never arrived") };
            assert_eq!(reading, "きょうはいいてんき");
            let before = composition_of(&factory);
            assert_eq!(late_refine_applies(&before, &reading), !changed, "state={:?} reading={:?}", before.state, live_refinement_reading(&before, false));
            LATE_RESULT.with(|r| *r.borrow_mut() = Some((reading, converted)));
            factory.apply_late_refine().unwrap();
            let comp = composition_of(&factory);
            if changed {
                assert!(!comp.refined && comp.segments_reading == "きょうはいいてんきだ");
            } else {
                assert!(comp.refined, "{comp:?}");
                assert_eq!(comp.segments.iter().map(|s| s.text.as_str()).collect::<String>(), "Aきょ Aうは Aいい Aてん Aき".replace(' ', ""));
            }
            assert!(late_refine_pending_for_tests().is_none());
        }
        // 次の清書を始めたら、前の待ちはやめる (新しい読みの待ちだけになる)
        let (_fake, ipc, _health) = fake_services("late3", &[("convert", 1800)]);
        let _installed = Installed::new(Some(ipc));
        let factory = factory_with(live("あめがふる"));
        {
            let _key = EventBudget::start();
            factory.handle_action(&[ClientAction::RefineLive], CompositionState::Composing).unwrap();
        }
        set_composition(&factory, live("あめがふるかも"));
        {
            let _key = EventBudget::start();
            factory.handle_action(&[ClientAction::RefineLive], CompositionState::Composing).unwrap();
        }
        assert_eq!(late_refine_pending_for_tests().as_deref(), Some("あめがふるかも"));
        super::super::ipc_service::drop_late_refine();
    }

    /// 3 回目の点検 1・再点検 N2: ライブ変換の Space で、AI の変換 (0.5 秒) のあとの候補の一覧 (10 秒) が待つ動作の上限 (AWAITED_LIMIT の代わりに 3 秒)
    /// で切れたら、変換したものを見せて窓は開かない (確定しない)。2 秒以上待って切れても、待つ動作の上限で切ったので問い合わせを控えない
    /// (控えると、続けて押した Space が「返事をしない間」の道で確定し、候補を選べなかった)。
    /// 2026-10-30: ①② の初期値がオンになったので、オンのときも試す (② の短い入力は清書せず、見えていた辞書の変換のまま)
    #[test]
    fn a_late_candidate_list_neither_commits_nor_stops_asking() {
        for (keep_refined, short_dictionary) in [(false, false), (true, true)] {
            let _options = LiveOptions::set(keep_refined, short_dictionary);
            let (_fake, ipc, health) = fake_services("n2a", &[("convert", 500), ("list_candidates", 10_000)]);
            let _installed = Installed::new(Some(ipc));
            let factory = factory_with(Composition {
                state: CompositionState::Composing, live: true, hiragana: "きょうは".into(), caret: 4, display: "今日は".into(),
                segments: vec![seg("きょうは", "今日は")], segments_reading: "きょうは".into(), ..Default::default()
            });
            super::super::ipc_service::set_awaited_limit_for_tests(Some(Duration::from_millis(3000)));
            let started = Instant::now();
            let result = {
                let _key = EventBudget::start();
                factory.handle_action(&[ClientAction::Convert], CompositionState::Converting)
            };
            super::super::ipc_service::set_awaited_limit_for_tests(None);
            let elapsed = started.elapsed();
            assert!(result.is_ok(), "{result:?}");
            assert!(elapsed >= Duration::from_millis(2900) && elapsed < Duration::from_millis(3800), "{elapsed:?}");
            let comp = composition_of(&factory);
            assert!(!comp.segments.is_empty() && !comp.window_open, "{short_dictionary}");
            assert_eq!(comp.refined, !short_dictionary);
            if short_dictionary { assert_eq!(shown(&comp), "今日は"); }
            assert!(!health.server_open_for_tests(), "the awaited cut made later keys skip the server");
        }
    }

    fn shown(comp: &Composition) -> String {
        comp.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("|")
    }

    /// ライブ変換の設定 (①, ②) をこのスレッドだけ替える。落とすと戻す
    struct LiveOptions;

    impl LiveOptions {
        fn set(keep_refined: bool, short_dictionary: bool) -> Self {
            super::LIVE_OPTIONS_FOR_TESTS.with(|options| options.set(Some((keep_refined, short_dictionary))));
            LiveOptions
        }
    }

    impl Drop for LiveOptions {
        fn drop(&mut self) {
            super::LIVE_OPTIONS_FOR_TESTS.with(|options| options.set(None));
        }
    }

    /// 2026-11-21 (学習の効きすぎの入口): 清書した前のほうの後ろは、前のほうの最後の文節の読みを頭に付けて変換し、その後ろだけを使う。
    /// 偽の変換サーバーは頭から 2 字ずつ区切るので、最後の文節が 2 字なら切れ目が合い (あい + う → dう)、3 字なら合わずに後ろだけを変換し直す (え → dえ)
    #[test]
    fn the_rest_after_the_refined_front_is_converted_with_its_last_segment() {
        let (_fake, ipc, _health) = fake_services("liveanchor", &[]);
        let _installed = Installed::new(Some(ipc.clone()));
        let mut ipc = ipc;
        let text = |segments: Vec<Segment>| segments.iter().map(|s| format!("{}:{}", s.reading, s.text)).collect::<Vec<_>>().join("|");
        let kept2 = vec![seg("あい", "Aあい")];
        assert_eq!(text(convert_rest_after(&mut ipc, &kept2, "う").unwrap().1), "う:dう");
        let kept3 = vec![seg("あいう", "Aあいう")];
        assert_eq!(text(convert_rest_after(&mut ipc, &kept3, "え").unwrap().1), "え:dえ");
        assert_eq!(text(convert_rest_after(&mut ipc, &[], "おか").unwrap().1), "おか:dおか");
        // 2026-11-21: 切れ目が文節の中に落ちたとき (偽の変換サーバーは 2 字ずつ: あいう + え → dあい|dうえ)。前のほうの字 (dあいう) で始まらないので使わない
        let kept3d = vec![seg("あいう", "dあいう")];
        assert_eq!(text(convert_rest_after(&mut ipc, &kept3d, "え").unwrap().1), "え:dえ");
        assert!(!convert_rest_after(&mut ipc, &kept3d, "え").unwrap().0);
        // 2026-11-23: 前のほうの最後の文節が読みのかなのまま (AI が字を替えていない) なら、それも残さずに、つなげた変換を使う
        // (かなの文節と後ろの字で 1 語になる形。偽の変換サーバーでは う + え → dうえ)。漢字・カタカナ・英字を選んだ文節 (Aあいう) は今までどおり
        let kept_kana = vec![seg("あい", "Aあい"), seg("う", "う")];
        let (drop_anchor, tail) = convert_rest_after(&mut ipc, &kept_kana, "え").unwrap();
        assert!(drop_anchor);
        assert_eq!(text(tail), "うえ:dうえ");
        assert!(!convert_rest_after(&mut ipc, &kept3, "え").unwrap().0);
    }

    /// 2026-10-29 (試験中 ①、設定 input.live_keep_refined): ライブ変換で、手が止まって AI が清書した前のほうは、そのあと打っても辞書の結果に戻らない
    /// (後ろ = 清書したときの最後の文節と、あとから打った字だけを辞書で変換する)。前のほうまで消したら、今までどおり全体を辞書で。
    /// 偽の変換サーバーは辞書の変換に d、AI の変換に A を付けて 2 字ずつ区切る。設定がオフなら今までどおり、打つたびに前のほうが辞書の結果に戻る
    #[test]
    fn the_refined_front_stays_while_typing() {
        let (_fake, ipc, _health) = fake_services("live1", &[]);
        let _installed = Installed::new(Some(ipc.clone()));
        let mut ipc = ipc;
        let live = |hiragana: &str| Composition {
            state: CompositionState::Composing, live: true, hiragana: hiragana.into(), caret: hiragana.chars().count() as i32,
            segments: vec![seg(hiragana, &format!("d{hiragana}"))], segments_reading: hiragana.into(), ..Default::default()
        };
        let type_to = |comp: &mut Composition, ipc: &mut IPCService, reading: &str| {
            comp.hiragana = reading.into();
            comp.caret = reading.chars().count() as i32;
            TextServiceFactory::draft_live(ipc, comp, reading).unwrap();
            shown(comp)
        };
        for keep in [true, false] {
            let _options = LiveOptions::set(keep, false);
            // 手が止まって清書 (時計の道: refine_live → RefineLive → ensure_refined)
            let factory = factory_with(live("あいうえ"));
            factory.refine_live().unwrap();
            let mut comp = composition_of(&factory);
            assert_eq!(shown(&comp), "Aあい|Aうえ");
            assert_eq!(comp.live_settled, if keep { 1 } else { 0 });
            if keep {
                assert_eq!(type_to(&mut comp, &mut ipc, "あいうえお"), "Aあい|dうえ|dお");
                assert_eq!(type_to(&mut comp, &mut ipc, "あいうえおか"), "Aあい|dうえ|dおか");
                // 次に手が止まったら全体を清書し直し、固める前のほうも取り直す
                comp.state = CompositionState::Composing;
                let factory = factory_with(comp);
                factory.refine_live().unwrap();
                let mut comp = composition_of(&factory);
                assert_eq!((shown(&comp).as_str(), comp.live_settled), ("Aあい|Aうえ|Aおか", 2));
                assert_eq!(type_to(&mut comp, &mut ipc, "あいうえおかき"), "Aあい|Aうえ|dおか|dき");
                // 消していって、固めた前のほうの終わりまでならそのまま、前のほうまで消したら全体を辞書で
                assert_eq!(type_to(&mut comp, &mut ipc, "あいうえ"), "Aあい|Aうえ");
                assert_eq!(type_to(&mut comp, &mut ipc, "あいう"), "dあい|dう");
                assert_eq!(comp.live_settled, 0);
                assert_eq!(type_to(&mut comp, &mut ipc, "あいうえ"), "dあい|dうえ");
            } else {
                assert_eq!(type_to(&mut comp, &mut ipc, "あいうえお"), "dあい|dうえ|dお");
            }
        }
        // 2026-11-23: 固めた前のほうの最後の文節が読みのかなのまま (AI が字を替えていない) で、つなげた変換で切れ目をまたぐ語の字が替わるときは、
        // その文節も残さない (かなの文節と後ろの字で 1 語になる形。偽の変換サーバーでは う + え → dうえ)。固めた数も 1 つ減らす
        let _options = LiveOptions::set(true, false);
        let mut comp = live("あいう");
        comp.segments = vec![seg("あい", "Aあい"), seg("う", "う")];
        comp.live_settled = 2;
        assert_eq!(type_to(&mut comp, &mut ipc, "あいうえ"), "Aあい|dうえ");
        assert_eq!(comp.live_settled, 1);
        assert_eq!(type_to(&mut comp, &mut ipc, "あいうえお"), "Aあい|dうえ|dお");
    }

    /// 2026-10-29 (試験中 ②、設定 input.live_short_dictionary): 短い入力 (読みが 6 字まで) は、手が止まっても清書しない (AI に問い合わせない)。
    /// Space では見えている辞書の変換を一番上に残し、AI の変換を候補の 2 番目に混ぜる。句読点での変換も辞書だけ。7 字からは今までどおり
    #[test]
    fn short_inputs_keep_the_dictionary_conversion() {
        let _options = LiveOptions::set(false, true);
        let live = |hiragana: &str, segments: Vec<Segment>| Composition {
            state: CompositionState::Composing, live: true, hiragana: hiragana.into(), caret: hiragana.chars().count() as i32,
            segments_reading: hiragana.into(), segments, ..Default::default()
        };
        let (_fake, ipc, _health) = fake_services("live2", &[("convert", 3000)]);
        let _installed = Installed::new(Some(ipc));
        let factory = factory_with(live("あいう", vec![seg("あい", "dあい"), seg("う", "dう")]));
        let started = Instant::now();
        factory.refine_live().unwrap();
        assert!(started.elapsed() < Duration::from_millis(500), "refined a short input ({:?})", started.elapsed());
        assert_eq!(shown(&composition_of(&factory)), "dあい|dう");
        drop(_installed);

        let (_fake, ipc, _health) = fake_services("live3", &[]);
        let _installed = Installed::new(Some(ipc));
        let factory = factory_with(live("あいう", vec![seg("あい", "dあい"), seg("う", "dう")]));
        assert!(factory.handle_action(&[ClientAction::Convert], CompositionState::Converting).is_ok());
        let comp = composition_of(&factory);
        assert_eq!(shown(&comp), "dあい|dう");
        assert_eq!(comp.segments[0].candidates.get(..2), Some(&["dあい".to_string(), "Aあい".to_string()][..]));
        assert_eq!(comp.segments[1].ai_text.as_deref(), Some("Aう"));
        // 句読点での変換も辞書だけ
        let factory = factory_with(live("あいう。", vec![seg("あい", "dあい"), seg("う", "dう")]));
        assert!(factory.handle_action(&[ClientAction::PunctuationConvert], CompositionState::Converting).is_ok());
        let comp = composition_of(&factory);
        assert_eq!((shown(&comp).as_str(), comp.punctuation_converted), ("dあい|dう。", true));
        // 7 字は短くない: Space で AI の変換に替わる
        let factory = factory_with(live("あいうえおかき", vec![seg("あいうえおかき", "dあいうえおかき")]));
        assert!(factory.handle_action(&[ClientAction::Convert], CompositionState::Converting).is_ok());
        assert_eq!(shown(&composition_of(&factory)), "Aあい|Aうえ|Aおか|Aき");
    }

    /// 2026-10-29 (試験中 ②): AI の変換を混ぜるのは、文節の区切りが同じときはその文節に、見えているのが 1 文節なら全体を。区切りが違えば混ぜない
    #[test]
    fn ai_alternatives_follow_the_shown_segments() {
        let mut one = vec![seg("きょうは", "強は")];
        attach_ai_alternatives(&mut one, &[seg("きょう", "今日"), seg("は", "は")]);
        assert_eq!(one[0].ai_text.as_deref(), Some("今日は"));
        let mut same = vec![seg("きょう", "強"), seg("は", "は")];
        attach_ai_alternatives(&mut same, &[seg("きょう", "今日"), seg("は", "は")]);
        assert_eq!((same[0].ai_text.as_deref(), same[1].ai_text.as_deref()), (Some("今日"), None));
        let mut different = vec![seg("きょうは", "強は"), seg("いい", "良い")];
        attach_ai_alternatives(&mut different, &[seg("きょう", "今日"), seg("はいい", "はいい")]);
        assert!(different.iter().all(|s| s.ai_text.is_none()));
    }

    /// 2026-10-29: 候補の一覧を開いているときの Ctrl+Delete。選んでいる候補が学習した語 (偽の変換サーバーでは 空ネコ) なら一覧から外して次の候補を選び、
    /// 辞書の語なら何もしない
    #[test]
    fn ctrl_delete_forgets_only_a_learned_candidate() {
        let (_fake, ipc, _health) = fake_services("forget", &[]);
        let _installed = Installed::new(Some(ipc));
        let converting = |index: i32| {
            let candidates: Vec<String> = ["空ネコ", "空猫", "そらねこ"].iter().map(|s| s.to_string()).collect();
            Composition {
                state: CompositionState::Converting, window_open: true, hiragana: "そらねこ".into(), caret: 4,
                segments: vec![Segment { reading: "そらねこ".into(), text: candidates[index as usize].clone(), candidates, index, ..Default::default() }],
                ..Default::default()
            }
        };
        let factory = factory_with(converting(0));
        assert!(factory.handle_action(&[ClientAction::ForgetCandidate], CompositionState::Converting).is_ok());
        let segment = composition_of(&factory).segments[0].clone();
        assert_eq!((segment.candidates, segment.text, segment.index), (vec!["空猫".to_string(), "そらねこ".to_string()], "空猫".to_string(), 0));
        let factory = factory_with(converting(1));
        assert!(factory.handle_action(&[ClientAction::ForgetCandidate], CompositionState::Converting).is_ok());
        let segment = composition_of(&factory).segments[0].clone();
        assert_eq!((segment.candidates.len(), segment.text.as_str(), segment.index), (3, "空猫", 1));
        drop(_installed);
        // 変換サーバーが返事をしない (古い変換サーバーで Forget が無いのと同じく、やり取りが失敗する): 何もせず、動作は失敗させない (確定しない)
        let (_fake, ipc, _health) = fake_services("forget_slow", &[("forget", 10_000)]);
        let _installed = Installed::new(Some(ipc));
        let factory = factory_with(converting(0));
        let result = {
            let _key = super::super::ipc_service::EventBudget::start();
            super::super::ipc_service::set_awaited_limit_for_tests(Some(Duration::from_millis(500)));
            let result = factory.handle_action(&[ClientAction::ForgetCandidate], CompositionState::Converting);
            super::super::ipc_service::set_awaited_limit_for_tests(None);
            result
        };
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(composition_of(&factory).segments[0].candidates.len(), 3);
    }

    /// レビュー L6: 変換サーバーにつながっていない間も、入力モードを切り替え、確定 (フォーカスの移り) は失敗させない
    #[test]
    fn mode_switch_and_commit_work_while_disconnected() {
        let _installed = Installed::new(None);
        let before = IMEState::get().unwrap().input_mode.clone();
        let other = if before == InputMode::Kana { InputMode::Latin } else { InputMode::Kana };
        let factory = factory_with(Composition::default());
        // 言語バーの表示の更新は TSF が無いので失敗する (モードは先に切り替わる)
        let _ = factory.handle_action(&[ClientAction::SetIMEMode(other.clone())], CompositionState::None);
        let switched = IMEState::get().unwrap().input_mode.clone();
        IMEState::get().unwrap().input_mode = before;
        assert_eq!(switched, other);
        assert!(factory.handle_action(&[ClientAction::EndComposition], CompositionState::None).is_ok());
        assert!(factory.handle_action(&[ClientAction::Convert], CompositionState::Converting).is_err());
    }
}

/// 2026-10-29: 長押しのアクセント (10-29 の不具合の報告「打ち始めがアクセントになる」)。判定 (diacritics の連打の数え方・accents_on_hold) を試す。
/// 候補を出す動作そのもの (ShowAccents) は、入力中でないとき SendInput でアプリへキーを送るので、試験では走らせない
#[cfg(test)]
mod accent_hold_tests {
    use super::*;
    use super::super::diacritics::{is_repeat, note_key_at, note_key_up, note_passed, RETYPED_KEY_MARKER, SETTINGS_FOR_TESTS};

    const DOWN: isize = 0;
    const REPEATED: isize = 1 << 30;
    const KEY_A: usize = 0x41;

    /// 報告のあった設定 (スペイン語・かなでも長押し) をこのスレッドだけ使う
    struct Spanish;

    impl Spanish {
        fn set() -> Self {
            SETTINGS_FOR_TESTS.with(|settings| *settings.borrow_mut() = Some((vec!["es".to_string()], true)));
            Spanish
        }
    }

    impl Drop for Spanish {
        fn drop(&mut self) {
            SETTINGS_FOR_TESTS.with(|settings| *settings.borrow_mut() = None);
            note_key_up(KEY_A);
        }
    }

    fn composing(typed: &str, hiragana: &str) -> Composition {
        Composition { state: CompositionState::Composing, typed: typed.into(), typed_valid: true, hiragana: hiragana.into(),
            caret: hiragana.chars().count() as i32, ..Default::default() }
    }

    /// 2026-11-05 (長押し): 長押しのアクセントを使う人は、押しっぱなしの英字の繰り返しを捨てる (同じ字を続けて入れない)。
    /// 最初の押下・数字・記号・BackSpace は捨てない。長押しを使わない人 (言語を選んでいない) はいつも続けて入れる
    #[test]
    fn held_letter_repeats_are_dropped_only_for_accent_users() {
        {
            let _spanish = Spanish::set();
            note_key_up(KEY_A);
            note_key_at(KEY_A, DOWN, 10, 0);
            assert!(!letter_repeat_suppressed(is_repeat(), KEY_A), "the first press was dropped");
            for time in [520, 553, 586] {
                note_key_at(KEY_A, REPEATED, time, 0);
                assert!(letter_repeat_suppressed(is_repeat(), KEY_A), "a held letter kept repeating");
            }
            for key in [0x31, 0x08, 0x25, 0x20, 0xBE] {
                assert!(!letter_repeat_suppressed(true, key), "key 0x{key:02X} stopped repeating");
            }
        }
        // 言語を選んでいない (長押しを使わない) 人は今までどおり
        SETTINGS_FOR_TESTS.with(|settings| *settings.borrow_mut() = Some((vec![], true)));
        assert!(!letter_repeat_suppressed(true, KEY_A));
        SETTINGS_FOR_TESTS.with(|settings| *settings.borrow_mut() = None);
    }

    /// 最初の押下を見ていないキーは、30 ビット目が立っていても長押しにしない (候補を開かない)
    #[test]
    fn an_unseen_first_press_never_opens_accents() {
        let _spanish = Spanish::set();
        note_key_up(KEY_A);
        for time in [10, 43, 76] {
            note_key_at(KEY_A, REPEATED, time, 0);
            assert!(!is_repeat(), "a repeat of a press the IME never saw counted as a hold");
        }
    }

    /// かなモードで最初の押下が入力の始まりになったが、サーバーの失敗の逃げ道で入力が終わった (入力中でない)。そのまま押し続けても、
    /// 長押しの候補は出さない (出すとアプリへ BackSpace を送り、確定した あ を消して á にする)。連打はふつうのキーとして打つ
    #[test]
    fn holding_after_the_fallback_sends_no_backspace() {
        let _spanish = Spanish::set();
        note_key_at(KEY_A, DOWN, 100, 0);
        note_passed(KEY_A, true);
        let after_fallback = Composition::default();
        note_key_at(KEY_A, REPEATED, 600, 0);
        assert!(is_repeat());
        assert!(after_fallback.at_accent_letter('a'), "the start of input still accepts a hold in itself");
        assert!(!accents_on_hold(&after_fallback, &InputMode::Kana, 'a'));
        note_passed(KEY_A, false);
        assert!(!accents_on_hold(&after_fallback, &InputMode::Kana, 'a'), "a repeat changed what the first press did");
    }

    /// 端末で、自動で英数にしていたあと入力待ちでなくなった (claude などを起動した): 最初の字のキーでかなに戻し、そのキーを IME に打ち直す
    /// (RetypeAsKana → send_retype)。打ち直したキーは利用者がまだ押している間に届くので 30 ビット目が立つが、長押しの候補は出さない
    /// (前は母音・n なら BackSpace と á を送った。10-29 の不具合の報告「開いたばかりの時」「ターミナル」)
    #[test]
    fn the_terminal_retype_is_not_a_hold() {
        let _spanish = Spanish::set();
        let start = Composition::default();
        note_key_at(KEY_A, DOWN, 3000, 0);
        note_passed(KEY_A, true);
        note_key_at(KEY_A, REPEATED, 3001, RETYPED_KEY_MARKER);
        assert!(!is_repeat(), "the retyped key counted as a hold");
        note_passed(KEY_A, true);
        // 印が無くても (印を付ける前の版で打ち直したもの)、かなモードの打ち始めでは開かない
        note_key_at(KEY_A, REPEATED, 3002, 0);
        assert!(is_repeat());
        assert!(!accents_on_hold(&start, &InputMode::Kana, 'a'));
    }

    /// これまでどおり開く: 英数モードの長押し (最初の押下の a はアプリに入っている → á に置き換える)・かなの入力中の長押し (ka の a)・
    /// かなモードで最初の押下を字のままアプリへ渡していた (数字の欄など)
    #[test]
    fn ordinary_holds_still_open_accents() {
        let _spanish = Spanish::set();
        let start = Composition::default();
        note_key_at(KEY_A, DOWN, 1000, 0);
        note_passed(KEY_A, false);
        note_key_at(KEY_A, REPEATED, 1500, 0);
        assert!(is_repeat());
        assert!(accents_on_hold(&start, &InputMode::Latin, 'a'));
        assert!(accents_on_hold(&start, &InputMode::Kana, 'a'));
        note_key_up(KEY_A);

        note_key_at(KEY_A, DOWN, 2000, 0);
        note_passed(KEY_A, true);
        note_key_at(KEY_A, REPEATED, 2500, 0);
        assert!(is_repeat());
        assert!(accents_on_hold(&composing("ka", "か"), &InputMode::Kana, 'a'));
        // 入力中でも、最後に打った字でなければ開かない。アクセントの無い字 (k) も開かない
        assert!(!accents_on_hold(&composing("ki", "き"), &InputMode::Kana, 'a'));
        assert!(!accents_on_hold(&composing("k", "k"), &InputMode::Kana, 'k'));
    }
}

#[cfg(test)]
mod live_timer_tests {
    use super::{arms_live_refine, timer_fire, TimerFire, LIVE_REFINE_DELAY_MS, PREDICT_DELAY_MS};

    #[test]
    fn late_timer_counts_again_once() {
        // 長い欄で並べ直しに 400ms かかったあとに鳴った: 鳴った時点から 1 回だけ数え直す
        assert_eq!(timer_fire(600, LIVE_REFINE_DELAY_MS, false), TimerFire::Rearm);
        assert_eq!(timer_fire(600, LIVE_REFINE_DELAY_MS, true), TimerFire::Run);
        // 時計の刻み (16ms ほど) の遅れでは数え直さない
        assert_eq!(timer_fire(216, LIVE_REFINE_DELAY_MS, false), TimerFire::Run);
        assert_eq!(timer_fire(250, LIVE_REFINE_DELAY_MS, false), TimerFire::Run);
        assert_eq!(timer_fire(251, LIVE_REFINE_DELAY_MS, false), TimerFire::Rearm);
        // 予測の窓の時計も同じ
        assert_eq!(timer_fire(190, PREDICT_DELAY_MS, false), TimerFire::Run);
        assert_eq!(timer_fire(201, PREDICT_DELAY_MS, false), TimerFire::Rearm);
    }

    #[test]
    fn live_refine_counts_from_the_last_key() {
        // 辞書の仮表示を作ったキー (母音) で仕掛ける
        assert!(arms_live_refine(true, false, true, false));
        // ローマ字の打ちかけ (子音) でも、清書がまだなら掛け直す
        assert!(arms_live_refine(false, false, true, false));
        // 清書が済んでいれば、子音のキーでは仕掛けない
        assert!(!arms_live_refine(false, true, true, false));
        // 仮表示が無ければ仕掛けない
        assert!(!arms_live_refine(false, false, false, false));
        // 清書 (時計から呼ばれた動作) で清書できなかったときは掛け直さない (時計が回り続けないように)
        assert!(!arms_live_refine(false, false, true, true));
        // 清書のあとに仮表示を作り直したときは、今までどおり仕掛ける
        assert!(arms_live_refine(true, false, true, true));
    }
}

#[cfg(test)]
mod windows_candidate_tests {
    use super::{merge_late_windows_candidates, merge_windows_candidates, Segment};

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn promotes_top_three_without_moving_current_text() {
        let mut candidates = strings(&["current", "a", "b", "c", "third", "second", "first"]);
        merge_windows_candidates(&mut candidates, "reading", strings(&["first", "second", "third"]));
        assert_eq!(candidates, strings(&["current", "first", "second", "third", "a", "b", "c"]));
    }

    #[test]
    fn keeps_candidates_already_in_first_four_positions() {
        let mut candidates = strings(&["current", "a", "b", "c", "tail"]);
        let original = candidates.clone();
        merge_windows_candidates(&mut candidates, "reading", strings(&["current", "c", "b"]));
        assert_eq!(candidates, original);
    }

    #[test]
    fn inserts_first_five_then_remainder_before_reading() {
        let mut candidates = strings(&["current", "a", "b", "tail", "reading"]);
        merge_windows_candidates(&mut candidates, "reading", strings(&["a", "1", "2", "3", "4", "5", "6", "6", "7"]));
        assert_eq!(candidates, strings(&["current", "a", "b", "1", "2", "3", "4", "5", "tail", "6", "7", "reading"]));
    }

    #[test]
    fn late_windows_candidates_keep_the_selection() {
        // 一覧を作ったときは Windows が時間切れ。2 番目を選んでいるところへ答えが来た (ぱいたん → 白湯)
        let mut segment = Segment { reading: "ぱいたん".into(), text: "牌端".into(), index: 1,
            candidates: strings(&["パイ端", "牌端", "パイ誕", "パイたん", "ぱいたん", "ﾊﾟｲﾀﾝ"]), ..Default::default() };
        merge_late_windows_candidates(&mut segment, strings(&["白湯", "パイタン"]));
        assert_eq!(segment.candidates, strings(&["パイ端", "牌端", "パイ誕", "白湯", "パイタン", "パイたん", "ぱいたん", "ﾊﾟｲﾀﾝ"]));
        assert_eq!((segment.index, segment.text.as_str()), (1, "牌端"));
        // 選んでいる候補が一覧に無い (カタカナに切り替えた) ときは足さない
        let mut switched = Segment { reading: "ぱいたん".into(), text: "パイタン".into(), index: 0,
            candidates: strings(&["パイ端", "ぱいたん"]), ..Default::default() };
        merge_late_windows_candidates(&mut switched, strings(&["白湯"]));
        assert_eq!(switched.candidates, strings(&["パイ端", "ぱいたん"]));
    }

    #[test]
    fn appends_remainder_when_reading_is_absent() {
        let mut candidates = strings(&["current", "a", "b", "tail"]);
        merge_windows_candidates(&mut candidates, "reading", strings(&["1", "2", "3", "4", "5", "6", "7"]));
        assert_eq!(candidates, strings(&["current", "a", "b", "1", "2", "3", "4", "5", "tail", "6", "7"]));
    }

    #[test]
    fn windows_symbol_candidates_follow_ordinary_ones() {
        let mut candidates = strings(&["はてな", "果てな", "ハテナ", "tail", "はてな"]);
        merge_windows_candidates(&mut candidates, "はてな", strings(&["？", "はてな", "(?)"]));
        assert_eq!(candidates, strings(&["はてな", "果てな", "ハテナ", "tail", "はてな", "？", "(?)"]));
        // サーバーが前に置いた記号 (覚えた語) は動かさない
        let mut learned = strings(&["()", "括弧", "カッコ", "a", "b"]);
        merge_windows_candidates(&mut learned, "かっこ", strings(&["かっこ", "()", "括弧"]));
        assert_eq!(&learned[..3], &strings(&["()", "括弧", "カッコ"])[..]);
        for (text, symbol) in [("？", true), ("～", true), ("()", true), ("〇", true), ("(^^)", true), ("ー", false), ("例", false), ("3×2", false), (" ", false)] {
            assert_eq!(super::symbol_only(text), symbol, "{text}");
        }
    }

    #[test]
    fn handles_short_and_empty_lists() {
        let mut candidates = strings(&["current"]);
        merge_windows_candidates(&mut candidates, "reading", strings(&["one", "two", "one"]));
        assert_eq!(candidates, strings(&["current", "one", "two"]));
        let mut empty = Vec::new();
        merge_windows_candidates(&mut empty, "reading", strings(&["one", "two"]));
        assert_eq!(empty, strings(&["one", "two"]));
    }
}

#[cfg(test)]
mod latin_conversion_tests {
    use super::*;

    #[test]
    fn latin_detection_cannot_lock_out_kana_and_kanji_conversion() {
        for (latin, kana, kanji) in [("takai", "たかい", "高い"), ("yasui", "やすい", "安い"), ("kirei", "きれい", "綺麗"), ("in", "いん", "院"),
            ("ravi", "らゔぃ", "ラヴィ"), ("raviーらー", "らゔぃーらー", "ラヴィーラー"),
            ("nodexiーらー", "のでぃーらー", "のディーラー"), ("do-ravi-ra-", "どーらゔぃーらー", "ドーラヴィーラー")] {
            assert_eq!(latin_kana_reading(latin).as_deref(), Some(kana));
            let mut candidates = latin_spelling_candidates(latin, latin, vec![]);
            add_japanese_recovery(&mut candidates, kana, vec![kanji.into(), kana.into()]);
            assert_eq!(candidates[0], latin);
            assert!(candidates.contains(&kanji.to_string()));
            assert!(candidates.contains(&kana.to_string()));
            assert!(candidates.contains(&to_katakana(kana)));
            let selected = Segment { reading: latin.into(), text: kanji.into(), ..Default::default() };
            assert_eq!(learning_reading(&selected), kana);
            assert_eq!(next_kana_type(latin, latin, Some(latin), &[]).0, kana);
        }
        for latin in ["github", "killing", "my", "ni\u{f1}o", "2.5mol", ""] {
            assert!(latin_kana_reading(latin).is_none(), "{latin}");
        }
    }

    #[test]
    fn candidate_navigation_does_not_count_as_explicit_latin_learning() {
        let mut comp = Composition { latin_learning_intent: true, hiragana: "たかい".into(),
            typed: "takai".into(), typed_valid: true,
            segments: vec![Segment { reading: "たかい".into(), candidates: vec!["高い".into(), "takai".into()], ..Default::default() }],
            ..Default::default() };
        TextServiceFactory::select_candidate(&mut comp, 1);
        assert!(comp.latin_selected);
        assert!(!comp.latin_learning_intent);
        comp.reset();
        assert!(!comp.latin_learning_intent);
    }

    #[test]
    fn original_romaji_stays_available_after_every_letter_became_kana() {
        for (typed, reading) in [("in", "いん"), ("inn", "いん"), ("on", "おん"), ("to", "と"), ("no", "の"), ("at", "あt"), ("home", "ほめ"), ("game", "がめ"), ("shi", "し"), ("si", "し")] {
            let comp = Composition { hiragana: reading.into(), typed: typed.into(), typed_valid: true,
                segments: build_segments(reading, vec![]), ..Default::default() };
            let candidates = vec!["日本語候補1".into(), "日本語候補2".into(), "日本語候補3".into(), reading.into(), SINGLE_KANJI_MARKER.into()];
            let latin = typed_latin_candidate(reading, typed_for_focus(&comp).as_deref());
            let form = super::super::auto_english::canonical_case(typed).unwrap_or(typed);
            assert_eq!(latin.as_deref(), Some(form));
            let mut segment = Segment { reading: reading.into(), text: "日本語候補1".into(), typed_latin: latin, ..Default::default() };
            finish_segment_candidates(&mut segment, candidates);
            assert_eq!(&segment.candidates[..3], ["日本語候補1", "日本語候補2", "日本語候補3"]);
            // 英字は 1 つだけ、本当の末尾 (「単漢字…」の後ろ)。一覧の頭で ↑ を 1 回押せば届く
            assert_eq!(segment.candidates.last().map(String::as_str), Some(form), "{typed}: {:?}", segment.candidates);
            assert_eq!(segment.candidates[segment.candidates.len() - 2], SINGLE_KANJI_MARKER);
            assert_eq!(segment.candidates.iter().filter(|c| c.eq_ignore_ascii_case(typed)).count(), 1, "{typed}: {:?}", segment.candidates);
            assert!(!segment.candidates.iter().any(|c| has_fullwidth_lowercase(c)), "{typed}: {:?}", segment.candidates);
            let mut comp = Composition { segments: vec![segment], ..Default::default() };
            TextServiceFactory::select_candidate(&mut comp, -1);
            assert_eq!(comp.segments[0].text, form);
        }
    }

    /// 2026-11-02 (案 A): 打ったとおりの英字の候補は 1 通りだけ (英単語の表に大文字の形があればその形) を、一覧の本当の末尾 (半角カタカナ・
    /// 「単漢字…」・絵文字の後ろ) に。ローマ字として読めない英字 (かなに崩れた読み てst) も同じ。大文字で打った語 (Ken) は読みが英字なのでそのまま
    #[test]
    fn typed_latin_forms_do_not_push_down_japanese_candidates() {
        let japanese = |n: usize| (1..=n).map(|k| format!("候補{k}")).collect::<Vec<String>>();
        for (reading, typed) in [("きれい", "kirei"), ("けん", "ken"), ("は", "ha"), ("ほめ", "home"), ("つゆ", "tuyu"), ("てst", "test")] {
            let mut candidates = japanese(6);
            candidates.push(SINGLE_KANJI_MARKER.into());
            candidates.push("😀".into());
            let form = super::super::auto_english::canonical_case(typed).unwrap_or(typed);
            let mut segment = Segment { reading: reading.into(), text: "候補1".into(), typed_latin: typed_latin_candidate(reading, Some(typed)), ..Default::default() };
            finish_segment_candidates(&mut segment, candidates);
            assert_eq!(&segment.candidates[..6], &japanese(6)[..], "{typed}");
            assert_eq!(segment.candidates.iter().filter(|c| c.eq_ignore_ascii_case(typed)).count(), 1, "{typed}: {:?}", segment.candidates);
            assert_eq!(segment.candidates.last().map(String::as_str), Some(form), "{typed}: {:?}", segment.candidates);
            // 遅れて来た Windows の候補を足しても、英字は末尾のまま
            merge_late_windows_candidates(&mut segment, vec!["窓".into()]);
            assert_eq!(segment.candidates.last().map(String::as_str), Some(form), "{typed}: {:?}", segment.candidates);
        }
        // 英単語の表に大文字の形がある語はその形 (dna → DNA)
        assert_eq!(typed_latin_candidate("dな", Some("dna")).as_deref(), super::super::auto_english::canonical_case("dna").or(Some("dna")));
        // 表示している候補が英字 (前に選んだ) なら頭のまま
        let mut segment = Segment { reading: "けん".into(), text: "ken".into(), typed_latin: Some("ken".into()), ..Default::default() };
        finish_segment_candidates(&mut segment, vec!["ken".into(), "健".into(), "県".into()]);
        assert_eq!(segment.candidates[0], "ken");
        // 大文字で打った語は読みが英字のまま (Ken + を): 英字の候補を作らない
        assert_eq!(typed_latin_candidate("Ken", Some("Ken")), None);
    }

    #[test]
    fn case_for_names_applies_only_next_to_japanese() {
        use super::super::auto_english::TokenKind::{English, Romaji};
        let runs = |items: &[(&str, bool)]| items.iter().map(|(t, e)| (t.to_string(), if *e { English } else { Romaji })).collect::<Vec<_>>();
        assert!(glued_to_japanese(&runs(&[("windows", true), ("noupdate", false)]), 0));
        assert!(glued_to_japanese(&runs(&[("asita", false), ("zoom", true)]), 1));
        assert!(!glued_to_japanese(&runs(&[("windows", true)]), 0));
        assert!(!glued_to_japanese(&runs(&[("windows", true), (" ", false), ("10", false)]), 0));
    }

    #[test]
    fn typed_spans_preserve_original_n_and_romanization_inside_a_sentence() {
        for (typed, reading, first, second) in [("korehain", "これはいん", "これは", "いん"), ("sihain", "しはいん", "しは", "いん"), ("kyouhain", "きょうはいん", "きょうは", "いん")] {
            let comp = Composition { hiragana: reading.into(), typed: typed.into(), typed_valid: true, focus: 1,
                segments: vec![Segment { reading: first.into(), ..Default::default() }, Segment { reading: second.into(), ..Default::default() }], ..Default::default() };
            assert_eq!(typed_for_focus(&comp).as_deref(), Some("in"), "{typed}");
        }
        let mut comp = Composition { hiragana: "いん".into(), typed: "in".into(), typed_valid: false,
            segments: build_segments("いん", vec![]), ..Default::default() };
        assert_eq!(typed_for_focus(&comp), None);
        comp.typed_valid = true;
        comp.hiragana = "いんあ".into();
        assert_eq!(typed_for_focus(&comp), None);
        comp.hiragana = "きょ".into();
        comp.typed = "kyo".into();
        comp.segments = vec![Segment { reading: "き".into(), ..Default::default() }, Segment { reading: "ょ".into(), ..Default::default() }];
        assert_eq!(typed_for_focus(&comp), None);
    }

    #[test]
    fn literal_conversion_never_loses_the_last_letter() {
        for reading in ["killing", "running", "walking", "making", "my", "ni\u{f1}o"] {
            let short: String = reading.chars().take(reading.chars().count() - 1).collect();
            for length in [reading.chars().count() as i32, short.chars().count() as i32] {
                let segments = build_segments(reading, vec![(short.clone(), length)]);
                assert_eq!(segments.iter().map(|s| s.text.as_str()).collect::<String>(), reading);
            }
        }
        assert_eq!(build_segments("github", vec![("GitHub".into(), 6)])[0].text, "GitHub");
    }

    /// 2026-11-23 (セキュリティの点検 F-3): 長さが 1 より小さい文節があれば、足して合っても切り分けずに読みのかなに (負の長さで落ちていた)
    #[test]
    fn build_segments_rejects_lengths_below_one() {
        let texts = |segments: Vec<Segment>| segments.iter().map(|s| (s.reading.clone(), s.text.clone())).collect::<Vec<_>>();
        assert_eq!(texts(build_segments("きょうは", vec![("今日".into(), 6), ("は".into(), -2)])), vec![("きょうは".to_string(), "きょうは".to_string())]);
        assert_eq!(texts(build_segments("きょうは", vec![("".into(), 0), ("今日は".into(), 4)])), vec![("きょうは".to_string(), "きょうは".to_string())]);
        assert_eq!(texts(build_segments("きょうは", vec![("今日は".into(), -4)])), vec![("きょうは".to_string(), "きょうは".to_string())]);
        assert_eq!(texts(build_segments("きょうは", vec![("今日は".into(), 3)])), vec![("きょうは".to_string(), "きょうは".to_string())]);
        assert_eq!(texts(build_segments("きょうは", vec![("今日".into(), 3), ("は".into(), 1)])),
            vec![("きょう".to_string(), "今日".to_string()), ("は".to_string(), "は".to_string())]);
    }

    #[test]
    fn explicit_latin_choice_and_smart_spaces_keep_user_intent() {
        super::super::auto_english::set_english_sentence(false);
        let mut comp = Composition { hiragana: "いん".into(), typed: "in".into(), typed_valid: true,
            segments: vec![Segment { reading: "いん".into(), text: "印".into(), candidates: vec!["印".into(), "in".into()], ..Default::default() }], ..Default::default() };
        TextServiceFactory::select_candidate(&mut comp, 1);
        assert!(comp.latin_selected);
        TextServiceFactory::select_candidate(&mut comp, 0);
        assert!(!comp.latin_selected);
        for letter in ['n', '\u{e9}', '\u{f1}', '１', 'Ａ'] {
            assert_eq!(smart_space(Some(letter), false), " ");
        }
        assert_eq!(smart_space(Some('日'), false), "　");
        super::super::auto_english::note_sentence_commit(true, true, false);
        assert_eq!(smart_space(Some('.'), false), " ");
        assert_eq!(smart_space(None, false), " ");
        super::super::auto_english::set_english_sentence(false);
    }

    #[test]
    fn space_commits_english_words_that_were_pending_romaji() {
        use super::super::auto_english::{segment_finishing, spelling, TokenKind};
        super::super::auto_english::set_english_sentence(false);
        // Space の時点の判定 (打ち方の設定・学習で変わる) が英単語なら、最後が打ちかけの英字の語は確定して空白
        // (my = みゃ の途中、today = とだ+y)。nn と打つ人の末尾の n 1 つは変換に回す
        for (hiragana, typed) in [("my", "my"), ("ｍｙ", "my"), ("とだy", "today"), ("まny", "many"), ("けy", "key"), ("るn", "run")] {
            let comp = Composition { hiragana: hiragana.into(), caret: hiragana.chars().count() as i32, typed: typed.into(), typed_valid: true, ..Default::default() };
            let tokens = segment_finishing(typed);
            let english = !tokens.is_empty() && tokens.iter().all(|t| t.kind == TokenKind::English);
            let nn_single = spelling().nn && typed.ends_with('n');
            assert_eq!(TextServiceFactory::english_only_at_boundary(&comp), english && !nn_single, "{typed}: {tokens:?}");
        }
        // 打ちかけで終わっていない日本語・全体が英単語にならないものは、今までどおり変換に回す
        for (hiragana, typed) in [("なめ", "name"), ("みゃ", "mya"), ("きょと", "kyoto"), ("きょうはとだy", "kyouhatoday")] {
            let comp = Composition { hiragana: hiragana.into(), caret: hiragana.chars().count() as i32, typed: typed.into(), typed_valid: true, ..Default::default() };
            assert!(!TextServiceFactory::english_only_at_boundary(&comp), "{typed}");
        }
        // 途中にカーソルがあるときは確定しない
        let comp = Composition { hiragana: "とだy".into(), caret: 1, typed: "today".into(), typed_valid: true, ..Default::default() };
        assert!(!TextServiceFactory::english_only_at_boundary(&comp));
        // 2026-10-30: 英語の短縮形も確定して空白 (前は ' のあとの s・t を英字の語に数えず、Space が変換に回って候補の窓が開いた)
        if super::super::auto_english::segment_finishing("it's").len() == 1 {
            for word in ["it's", "I'm", "don't", "can't", "you're"] {
                let comp = Composition { hiragana: word.into(), caret: word.chars().count() as i32, typed: word.into(), typed_valid: true, ..Default::default() };
                assert!(TextServiceFactory::english_only_at_boundary(&comp), "{word}");
            }
        }
    }

    #[test]
    fn shift_space_inverts_the_contextual_width() {
        super::super::auto_english::set_english_sentence(false);
        for (previous, normal) in [
            (None, "\u{3000}"), (Some('a'), " "), (Some('Z'), " "),
            (Some('1'), " "), (Some('\u{ff11}'), " "), (Some('\u{ff21}'), " "),
            (Some('\u{e9}'), " "), (Some('\u{f1}'), " "),
            (Some('\u{65e5}'), "\u{3000}"), (Some('\u{304b}'), "\u{3000}"),
            (Some('.'), "\u{3000}"), (Some(' '), "\u{3000}"),
            (Some('\u{3000}'), "\u{3000}"),
        ] {
            assert_eq!(smart_space(previous, false), normal, "{previous:?}");
            let reversed = if normal == " " { "\u{3000}" } else { " " };
            assert_eq!(smart_space(previous, true), reversed, "{previous:?}");
            assert!(!super::super::auto_english::in_english_sentence());
        }
    }

    #[test]
    fn shift_space_reverses_english_context_without_changing_it() {
        super::super::auto_english::set_english_sentence(false);
        super::super::auto_english::note_sentence_commit(true, true, false);
        for previous in [None, Some('.'), Some('a'), Some('\u{65e5}'), Some(' ')] {
            assert_eq!(smart_space(previous, false), " ");
            assert_eq!(smart_space(previous, true), "\u{3000}");
            assert!(super::super::auto_english::in_english_sentence());
        }
        super::super::auto_english::set_english_sentence(false);
    }

    #[test]
    fn context_conversion_probe() {
        let Ok(path) = std::env::var("AZK_CONTEXT_CONVERSION_PROBE") else { return };
        use super::super::auto_english::{has_english, note_sentence_commit, runs, segment, segment_finishing, set_english_sentence, TokenKind};
        let mut rows = vec![];
        for typed in ["in", "inn", "on", "to", "no", "at", "home", "game", "shi", "si", "miso"] {
            set_english_sentence(false);
            let generated: String = super::super::romaji_style::simulate(typed).iter().map(|u| u.kana.as_str()).collect();
            rows.push(serde_json::json!({"kind":"romaji", "typed":typed, "reading":normalize_reading(&generated),
                "operations":[[to_fullwidth(typed, false),false]], "convert":true}));
        }
        for word in ["killing", "running", "walking", "making", "my", "why", "sky", "hello", "github", "azookey", "Teriyaki", "Sushi", "Ramen", "Karaoke", "Samurai"] {
            set_english_sentence(false);
            for end in 1..=word.len() {
                let raw = &word[..end];
                let tokens = segment(raw);
                if word.starts_with(|c: char| c.is_ascii_uppercase()) {
                    assert!(tokens.iter().all(|t| t.kind == TokenKind::English), "capitalized {raw}: {tokens:?}");
                }
                let operations: Vec<_> = runs(&tokens).iter().map(|(text, kind)| {
                    serde_json::json!([if *kind == TokenKind::English { text.clone() } else { to_fullwidth(text, false) }, *kind == TokenKind::English])
                }).collect();
                rows.push(serde_json::json!({"kind":"automatic", "typed":raw, "word":word,
                    "operations":operations, "literal":tokens.iter().all(|t| t.kind == TokenKind::English),
                    "convert":end==word.len()}));
            }
        }
        let sentences = [
            "I am in my home.", "We are on the way to school.", "I have no time to play a game.",
            "She is at home with me.", "He was struck by a car.", "I'm in the office.",
            "We're going to the shop.", "It's time to go home.", "I'd like to see it.",
            "He isn't in the room.", "We don't have to do it.", "She can't come to the game.",
            "I'll be at home.", "They've been to the park.", "Please check my work.",
            "This is a killing game.", "Running is good for me.", "My home is on the hill.",
            "The home-game is on Friday.", "I paid 2.5 for it.", "I need 3 of them.",
            "The answer is yes, not no.", "She said (hello) to me.", "I wrote 'hello' in my book.",
            "We use Python at work.", "Open GitHub in the browser.", "Check the spelling of azookey.",
            "The sky is blue.", "I will try to make it.", "Why is my code running?",
            "We use Claude and Codex at work.", "Read the arxiv paper in the browser.",
            "Deploy FastAPI with Pydantic and NumPy.", "Check tokenization in the tokenizer.",
            "We serve Teriyaki and Sushi at home.", "I use Ollama with LangChain and vllm.",
        ];
        for (index, sentence) in sentences.iter().enumerate() {
            set_english_sentence(false);
            for (number, word) in sentence.split_whitespace().enumerate() {
                for end in 1..=word.len() {
                    let raw = &word[..end];
                    let tokens = if number == 0 { vec![super::super::auto_english::Token { raw: raw.into(), kind: TokenKind::English }] } else { segment(raw) };
                    let operations: Vec<_> = runs(&tokens).iter().map(|(text, kind)| {
                        serde_json::json!([if *kind == TokenKind::English { text.clone() } else { to_fullwidth(text, false) }, *kind == TokenKind::English])
                    }).collect();
                    let literal = tokens.iter().all(|t| t.kind == TokenKind::English);
                    if end == word.len() { assert!(literal, "sentence {index}, word {word}: {tokens:?}"); }
                    rows.push(serde_json::json!({"kind":"sentence", "sentence":index, "typed":raw, "word":word,
                        "operations":operations, "literal":literal, "convert":false}));
                }
                let comp = Composition { hiragana: word.into(), caret: word.len() as i32,
                    typed: word.into(), typed_valid: true, english_active: true, ..Default::default() };
                if number != 0 { assert!(TextServiceFactory::english_only_at_boundary(&comp), "Space on {word}"); }
                note_sentence_commit(true, number == 0, true);
                assert_eq!(smart_space(word.chars().last(), false), " ");
            }
            for raw in ["korehanihongodesu", "sarerunoha", "kyouhatenkigayoi", "checkhadouiuimi"] {
                let contextual = segment_finishing(raw);
                set_english_sentence(false);
                assert_eq!(contextual, segment_finishing(raw), "Japanese return: {raw}");
                note_sentence_commit(true, true, false);
            }
        }
        set_english_sentence(false);
        assert!(has_english(&segment("killing")));
        std::fs::write(path, serde_json::to_string_pretty(&serde_json::json!({"sentences":sentences.len(), "rows":rows})).unwrap()).unwrap();
    }

    #[test]
    fn contextual_candidates_against_isolated_dll() {
        let Ok(path) = std::env::var("AZK_CONTEXT_SERVER_PROBE") else { return };
        let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut restored = 0;
        for row in report["results"].as_array().unwrap() {
            if !row["convert"].as_bool().unwrap() { continue; }
            let reading = row["server_reading"].as_str().unwrap();
            let typed = row["typed"].as_str().unwrap();
            let converted = row["segments"].as_array().unwrap().iter().map(|segment| {
                (segment[0].as_str().unwrap().to_string(), segment[1].as_i64().unwrap() as i32)
            }).collect();
            let segments = build_segments(reading, converted);
            if row["kind"] == "romaji" {
                let comp = Composition { hiragana: reading.into(), typed: typed.into(), typed_valid: true,
                    segments: build_segments(reading, vec![]), ..Default::default() };
                let mut candidates: Vec<_> = row["server_candidates"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
                let top = candidates.first().cloned();
                place_typed_latin_last(&mut candidates, typed_latin_candidate(reading, typed_for_focus(&comp).as_deref()).as_deref());
                assert_eq!(candidates.first(), top.as_ref());
                assert!(candidates.contains(&typed.to_string()), "{typed}: {candidates:?}");
                let (_, cycle) = next_kana_type(reading, reading, Some(typed), &[]);
                assert!(cycle.contains(&typed.to_lowercase()));
                assert!(!cycle.contains(&typed.to_uppercase()));
                restored += 1;
            } else if reading.chars().all(is_latin_letter) {
                assert_eq!(segments.iter().map(|s| s.text.as_str()).collect::<String>().to_lowercase(), typed.to_lowercase());
            }
        }
        assert_eq!(restored, 11);
    }

    #[test]
    fn symbol_forms_follow_the_settings() {
        let all = SymbolForms { full: shared::CONFIGURABLE_SYMBOLS.chars().collect(), yen: true };
        assert_eq!(apply_character_form_with("！？（）：＃１Ａ￥”", &all), "！？（）：＃1A￥”");
        let none = SymbolForms { full: vec![], yen: true };
        assert_eq!(apply_character_form_with("！？（）：＃１Ａ￥”", &none), "!?():#1A¥\"");
        let marks = SymbolForms { full: vec!['!', '?', '~'], yen: false };
        assert_eq!(apply_character_form_with("！？（～）￥", &marks), "！？(～)\\");
        // 「ろ」キー (＼) は ¥ キーの設定によらず \\
        assert_eq!(apply_character_form_with("￥＼", &all), "￥＼");
        assert_eq!(apply_character_form_with("￥＼", &none), "¥\\");
        // ローマ字表の日本語の記号はそのまま
        assert_eq!(apply_character_form_with("、。・「」ー", &none), "、。・「」ー");
    }

    /// 2026-10-30: 母音の多すぎの直しを選ぶ決まり (読みだけの AI の点数で 10 以上) と、「打ったとおり」に抜いた母音を戻すこと
    #[test]
    fn excess_vowel_repairs_need_a_clear_ai_margin() {
        let s = |z: f32| (Some(-30.0f32), Some(z));
        assert_eq!(pick_excess(&[s(-40.0), s(-27.9), s(-35.0)], &[12.0, 12.0]), Some(1));
        assert_eq!(pick_excess(&[s(-40.0), s(-28.1), s(-35.0)], &[12.0, 12.0]), None);
        assert_eq!(pick_excess(&[s(-40.0), s(-27.9), s(-35.0)], &[14.0, 12.0]), None);
        assert_eq!(pick_excess(&[(Some(-1.0), None), (Some(9.0), None)], &[10.0]), None, "AI の点数が無ければ直さない");
        assert_eq!(pick_excess(&[s(-40.0)], &[]), None);
        // 同じ母音が 3 つ続く所は 10、2 つ重ねた所は 14、違う母音を挟んだ所は 12
        assert_eq!(excess_margin("motsuuutsukushii", 5), 10.0);
        assert_eq!(excess_margin("motsuuutsukushii", 4), 10.0);
        assert_eq!(excess_margin("saaba", 2), 14.0);
        assert_eq!(excess_margin("kaeze", 2), 12.0);
        let spot = removed_spot("もつううつくしい", "もつうつくしい").unwrap();
        assert_eq!((spot.offset, spot.typed.as_str()), (3, "う"));
        assert_eq!(unrepaired_reading("もつうつくしい", 0, &[spot]), Some("もつううつくしい".to_string()));
        assert_eq!(removed_spot("あいう", "あいう"), None);
        assert_eq!(removed_spot("あいう", "かいう"), None);
    }

    /// 2026-10-30: 入力の頭・記号のあとの ー に数字が続いたら、マイナス・範囲の - に直す (-126)。かな・漢字のあとは長音のまま
    /// (10-30 の指摘: あー3なのか・えー2.6じゃないの・そっかー1だと思ってた を崩さない)
    #[test]
    fn a_hyphen_before_digits_is_a_minus_only_outside_japanese() {
        assert!(minus_fix("ー", None));
        assert!(minus_fix("ー", Some(' ')));
        assert!(minus_fix("ー", Some('=')));
        assert!(minus_fix("ー", Some('3')));
        assert!(minus_fix("x=ー", None));
        assert!(minus_fix("(ー", None));
        // 感動詞・長音のあと
        for (reading, before) in [("あー", None), ("えー", None), ("そっかー", None), ("なんばー", None), ("ー", Some('あ')), ("ー", Some('漢')),
            ("ー", Some('ー')), ("ナンバー", None), ("うーん、ー", Some('a'))] {
            assert_eq!(minus_fix(reading, before), reading == "うーん、ー", "{reading} {before:?}");
        }
        assert!(!minus_fix("あ", None));
        assert!(!minus_fix("", None));
        // 打った列から組み直すとき (英語の判定) も同じ: 頭の - は数字の前で英字のまま、かなのあとはローマ字の表 (ー) のまま
        assert_eq!(number_marks_in_typed("-126", None), vec![0]);
        assert_eq!(number_marks_in_typed("x=-1", None), vec![2]);
        assert_eq!(number_marks_in_typed("-126", Some('た')), Vec::<usize>::new());
        assert_eq!(number_marks_in_typed("a-3nanoka", None), Vec::<usize>::new());
        assert_eq!(number_marks_in_typed("e-2.6janaino", None), vec![3]);
        assert_eq!(number_marks_in_typed("sokka-1datoomotteta", None), Vec::<usize>::new());
        assert_eq!(number_marks_in_typed("nanba-1", None), Vec::<usize>::new());
        assert_eq!(number_marks_in_typed("-a", None), Vec::<usize>::new());
    }

    /// 2026-10-24: 数字の後ろの 。 、 を小数点・桁区切りに直すか。前の数字が今の入力に無い (変換・確定したあとに .5 と打った) ときは、入力を始めたときの直前の字で見る
    #[test]
    fn number_mark_fix_uses_the_char_before_the_composition() {
        assert_eq!(number_mark_fix("2。", None), Some("."));
        assert_eq!(number_mark_fix("ver2。", None), Some("."));
        assert_eq!(number_mark_fix("1、", None), Some(","));
        assert_eq!(number_mark_fix("。", Some('2')), Some("."));
        assert_eq!(number_mark_fix("。", Some('２')), Some("."));
        assert_eq!(number_mark_fix("、", Some('1')), Some(","));
        // 前が数字でなければ直さない (文末の 。 のあとに数字)
        assert_eq!(number_mark_fix("。", Some('た')), None);
        assert_eq!(number_mark_fix("。", None), None);
        assert_eq!(number_mark_fix("か。", Some('2')), None);
        assert_eq!(number_mark_fix("25", Some('2')), None);
    }

    /// 2026-10-28 (試験中): 。、？！ で変換するのは、設定がオンで読みの終わりで打ったときだけ。数字・英字の後ろ (2.5・1,000・github.com) は変換しない
    #[test]
    fn punctuation_converts_only_after_kana_at_the_end() {
        let comp = |hiragana: &str, caret: i32| Composition { hiragana: hiragana.into(), caret, state: CompositionState::Composing, ..Default::default() };
        assert!(punctuation_converts(&comp("きょうはいいてんきです", 11), '.', true));
        assert!(punctuation_converts(&comp("ほんとう", 4), '?', true));
        assert!(!punctuation_converts(&comp("きょうはいいてんきです", 11), '.', false));
        assert!(!punctuation_converts(&comp("きょうはいいてんきです", 5), '.', true));
        assert!(!punctuation_converts(&comp("ver2", 4), '.', true));
        assert!(!punctuation_converts(&comp("２", 1), '.', true));
        assert!(!punctuation_converts(&comp("github", 6), '.', true));
        assert!(!punctuation_converts(&comp("", 0), '.', true));
        // 、 も 。 と同じ (10-28 の決定)。数字の後ろの , は桁区切り
        assert!(punctuation_converts(&comp("きょう", 3), ',', true));
        assert!(!punctuation_converts(&comp("1", 1), ',', true));
        assert!(!punctuation_converts(&comp("きょう", 3), '/', true));
        // 2026-11-08: 日本語の文の中の英単語のあとの 、 は変換する。英字だけ (英文の , )・英字のあとの . ? !・数字のあとの , は変換しない
        assert!(punctuation_converts(&comp("にほんごでかいていてもIME", 14), ',', true));
        assert!(punctuation_converts(&comp("きょうはbilingual", 13), ',', true));
        assert!(!punctuation_converts(&comp("bilingual", 9), ',', true));
        assert!(!punctuation_converts(&comp("きょうはgithub", 10), '.', true));
        assert!(!punctuation_converts(&comp("きょうはIME", 7), '?', true));
        assert!(!punctuation_converts(&comp("きょうは1", 5), ',', true));
    }

    /// 2026-10-29 (レビュー M2・再点検 R1): 。、？！ で変換するとき AI・辞書の変換が間に合わなければ、見えている辞書の仮表示に、後ろに増えた読みを
    /// かなのまま足して見せる (問い合わせ直さない)。仮表示が今の読みの頭と合わない・ライブ変換でないときは使わない
    #[test]
    fn punctuation_falls_back_to_the_shown_preview() {
        let seg = |reading: &str, text: &str| Segment { reading: reading.into(), text: text.into(), ..Default::default() };
        let comp = Composition {
            live: true,
            segments: vec![seg("きょうは", "今日は"), seg("いいてんき", "いい天気")],
            segments_reading: "きょうはいいてんき".into(),
            ..Default::default()
        };
        let text = |segments: Vec<Segment>| segments.iter().map(|s| s.text.clone()).collect::<String>();
        let shown = preview_with_tail(&comp, "きょうはいいてんき。").unwrap();
        assert_eq!(shown.last().map(|s| s.reading.as_str()), Some("。"));
        assert_eq!(text(shown), "今日はいい天気。");
        assert_eq!(text(preview_with_tail(&comp, "きょうはいいてんきだ。").unwrap()), "今日はいい天気だ。");
        // 2026-11-21 (案 1c): 増えた読みが助詞・助動詞でなく、最後の文節が漢字で終われば、その文節をかなに戻す。句読点だけなら印を付けない
        assert_eq!(text(preview_with_tail(&comp, "きょうはいいてんきかな。").unwrap()), "今日はいいてんきかな。");
        let stale = Composition { segments: vec![seg("こうこつじょう", "恍惚上")], segments_reading: "こうこつじょう".into(), ..comp.clone() };
        assert_eq!(text(preview_with_tail(&stale, "こうこつじょうたい、").unwrap()), "こうこつじょうたい、");
        let stale = Composition { segments: vec![seg("きょうは", "今日は")], segments_reading: "きょうは".into(), ..comp.clone() };
        assert_eq!(text(preview_with_tail(&stale, "きょうはいいてんき。").unwrap()), "今日はいいてんき。");
        assert!(preview_with_tail(&comp, "きょうはいいてんきだ。").unwrap().iter().all(|s| s.patchwork));
        assert!(preview_with_tail(&comp, "きょうはいいてんき。").unwrap().iter().all(|s| !s.patchwork));
        assert!(preview_with_tail(&comp, "きょうはいいてんき").is_none());
        assert!(preview_with_tail(&comp, "あしたは。").is_none());
        assert!(preview_with_tail(&Composition { live: false, ..comp.clone() }, "きょうはいいてんき。").is_none());
        assert!(preview_with_tail(&Composition { segments: vec![], ..comp.clone() }, "きょうはいいてんき。").is_none());
    }

    /// 2026-10-29: Ctrl+Delete を受けるのは、変換中で候補の一覧を開いているときだけ (選んでいる候補を忘れる)。
    /// 一覧を閉じた変換中・入力中・入力していないときは受けない (アプリの Ctrl+Delete = 後ろの語を消す をそのまま通す)
    #[test]
    fn ctrl_delete_is_taken_only_with_the_candidate_list_open() {
        let comp = |state: CompositionState, window_open: bool| Composition {
            state, window_open, hiragana: "そらねこ".into(), caret: 4,
            segments: vec![Segment { reading: "そらねこ".into(), text: "空ネコ".into(), ..Default::default() }], ..Default::default()
        };
        assert_eq!(TextServiceFactory::process_ctrl_key(&comp(CompositionState::Converting, true), 0x2E, false),
            Some((vec![ClientAction::ForgetCandidate], CompositionState::Converting)));
        assert_eq!(TextServiceFactory::process_ctrl_key(&comp(CompositionState::Converting, false), 0x2E, false), None);
        assert!(TextServiceFactory::process_ctrl_key(&comp(CompositionState::Composing, false), 0x2E, false)
            .map_or(true, |(actions, _)| !actions.contains(&ClientAction::ForgetCandidate)));
        assert_eq!(TextServiceFactory::process_ctrl_key(&Composition::default(), 0x2E, false), None);
    }

    /// 2026-10-29 (レビュー H1): その場の上限 (3 秒) をかけずに変換を待つのは、利用者が結果を待っている動作だけ。
    /// 打っている途中の動作・句読点での変換・時計からの清書は 3 秒まで
    #[test]
    fn only_awaited_actions_lift_the_event_budget() {
        for action in [ClientAction::Convert, ClientAction::NextCandidate, ClientAction::PrevPage, ClientAction::Resize(1),
            ClientAction::ToggleDictionaryConversion, ClientAction::OpenSingleKanji, ClientAction::UndoCommit, ClientAction::UndoTypoRepair] {
            assert!(awaited_by_user(&action), "{action:?}");
        }
        for action in [ClientAction::Insert("a".into()), ClientAction::DeleteBackward, ClientAction::PunctuationConvert,
            ClientAction::RefineLive, ClientAction::ApplyLateRefine, ClientAction::ShowPredictions, ClientAction::EndComposition, ClientAction::StartComposition] {
            assert!(!awaited_by_user(&action), "{action:?}");
        }
    }

    /// 2026-10-29 (レビュー L6): サーバーが返事をしない間・つながっていない間も、半角/全角で入力モードを切り替える (Ctrl を押していれば切り替えない)
    #[test]
    fn the_mode_key_works_while_the_server_is_away() {
        assert_eq!(local_mode_toggle(WPARAM(0xF3), false, &InputMode::Kana), Some(InputMode::Latin));
        assert_eq!(local_mode_toggle(WPARAM(0xF4), false, &InputMode::Latin), Some(InputMode::Kana));
        assert_eq!(local_mode_toggle(WPARAM(0xF3), true, &InputMode::Kana), None);
        assert_eq!(local_mode_toggle(WPARAM(0x41), false, &InputMode::Kana), None);
    }

    /// 2026-10-28: 読みを組み直すときに英字のまま入れる、数字の間の . , (12 + ,345ninndesu の , が 、 に戻っていた)
    #[test]
    fn number_marks_survive_rebuilding_the_reading() {
        assert_eq!(number_marks_in_typed(",345ninndesu", Some('2')), vec![0]);
        assert_eq!(number_marks_in_typed("12,345ninn", None), vec![2]);
        assert_eq!(number_marks_in_typed("2.5desu", None), vec![1]);
        assert_eq!(number_marks_in_typed(".5desu", Some('２')), vec![0]);
        // 打っている途中で 3 桁に届いていない , はそのまま、4 桁になったら 、 (fix_thousands_comma と同じ)
        assert_eq!(number_marks_in_typed("1,00", None), vec![1]);
        assert_eq!(number_marks_in_typed("1,0000", None), Vec::<usize>::new());
        assert_eq!(number_marks_in_typed("1,00en", None), Vec::<usize>::new());
        // 前か後ろが数字でなければ 。 、 のまま
        assert_eq!(number_marks_in_typed("sou.", None), Vec::<usize>::new());
        assert_eq!(number_marks_in_typed("2.", None), Vec::<usize>::new());
        assert_eq!(number_marks_in_typed(",345", Some('た')), Vec::<usize>::new());
        assert_eq!(number_marks_in_typed("ka,3", None), Vec::<usize>::new());
    }

    /// 2026-10-28: handle_action は Composition の写しで動き、最後に欄ごとに書き戻す。書き戻しを忘れた欄は次のキーで元に戻る
    /// (char_before がそうで、2 を Enter で確定してから .5 と打つと 2。5 になっていた)。欄を足したら書き戻しにも足すこと。
    /// 写しを使わずに直に書き換える欄 (local_romaji) と、同じ動作の中で使い切る欄 (commit_suffix) だけは除く
    /// 2026-11-06 (端末で未確定の途中の形が積み重なった件): 入力中・変換中なのにコンポジションを持っていない状態は、次のキーの前に空にする
    /// (確定の途中で失敗して「入力中」だけが残ると、打つたびに字が消えるか、終わった範囲に書き続けた)。入力中でなければ空にしない
    #[test]
    fn composing_without_a_composition_is_reset() {
        let mut composition = Composition::default();
        assert!(!composition_needs_reset(&composition));
        for state in [CompositionState::Composing, CompositionState::Converting] {
            composition.state = state;
            assert!(composition_needs_reset(&composition));
        }
        assert!(!crate::tsf::edit_session::take_composition_lost());
    }

    #[test]
    fn every_composition_field_is_written_back() {
        let source = include_str!("composition.rs");
        let start = source.find("pub struct Composition {").unwrap();
        let end = start + source[start..].find("\n}\n").unwrap();
        let fields: Vec<&str> = source[start..end].lines().skip(1)
            .map(str::trim)
            .filter(|l| !l.starts_with("//") && !l.starts_with('#'))
            .filter_map(|l| l.trim_start_matches("pub(crate) ").trim_start_matches("pub ").split_once(':').map(|(name, _)| name.trim()))
            .filter(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .collect();
        assert!(fields.len() > 30 && fields.contains(&"char_before"), "{fields:?}");
        for field in fields {
            if ["local_romaji", "commit_suffix"].contains(&field) { continue; }
            assert!(source.contains(&format!("        composition.{field} = ")), "{field} is not written back in handle_action");
        }
    }

    /// 2026-10-28 (試験中): 母音の抜けの直しを選ぶ決まり・直した所・候補の「打ったとおり」
    #[test]
    fn typo_repair_choice_and_raw_candidate() {
        // [補う前, a, i, u, e, o] の (辞書, AI)。AI の点数で 2 位より 4 以上よいときだけ
        let scores = |lm: &[f32]| lm.iter().map(|&v| (Some(-30.0), Some(v))).collect::<Vec<_>>();
        assert_eq!(pick_repair(&scores(&[-63.8, -54.2, -57.0, -55.7, -57.0, -41.4])), Some(5));
        assert_eq!(pick_repair(&scores(&[-63.8, -54.2, -57.0, -55.7, -57.0, -51.4])), None);
        // 補う前の読みが 1 位なら直さない
        assert_eq!(pick_repair(&scores(&[-40.0, -50.0, -60.0])), None);
        // AI の点数が無ければ辞書の点数で 6 以上
        let dict = |d: &[f32]| d.iter().map(|&v| (Some(v), None)).collect::<Vec<_>>();
        assert_eq!(pick_repair(&dict(&[-37.8, -28.4, -17.1])), Some(2));
        assert_eq!(pick_repair(&dict(&[-37.8, -20.4, -17.1])), None);
        assert_eq!(pick_repair(&[(None, None), (None, None)]), None);
        // 2026-11-04 (仕事 20): 英字の区切りの中の直しは、AI の点数で english_repair_margin 以上よいときだけ。辞書の点数だけでは直さない
        assert_eq!(pick_english_repair(&scores(&[-99.4, -71.0, -59.3, -69.9])), Some(2));
        assert_eq!(pick_english_repair(&scores(&[-99.4, -71.0, -70.9, -69.9])), None);
        assert_eq!(pick_english_repair(&dict(&[-37.8, -28.4, -7.1])), None);
        let spot = repaired_spot("あshitnoschedule", "あしたのschedule").unwrap();
        assert_eq!(spot, RepairedSpot { offset: 1, kana: "したの".into(), typed: "shitno".into() });
        assert_eq!(unrepaired_reading("あしたの", 0, &[spot]), Some("あshitno".into()));
        let spot = repaired_spot("そういうkとです", "そういうことです").unwrap();
        assert_eq!(spot, RepairedSpot { offset: 4, kana: "こ".into(), typed: "k".into() });
        assert_eq!(repaired_spot("そういうkとです", "そういうことでした"), None);
        assert_eq!(repaired_spot("そういうことです", "そういうことです"), None);
        // 文節「ことです」(読みの 4 字目から) の打ったとおり
        assert_eq!(unrepaired_reading("ことです", 4, &[spot.clone()]), Some("kとです".into()));
        assert_eq!(unrepaired_reading("そういう", 0, &[spot.clone()]), None);
        assert_eq!(unrepaired_reading("ことです", 5, &[spot]), None);
    }

    /// 2026-10-25: 2。 を Space で変換したあとに数字を続けたら、確定せずに入力に戻す
    #[test]
    fn digits_after_a_converted_number_mark_return_to_composing() {
        let converting = |hiragana: &str, char_before: Option<char>| Composition { hiragana: hiragana.into(), char_before,
            state: CompositionState::Converting, segments: build_segments(hiragana, vec![(hiragana.into(), hiragana.chars().count() as i32)]),
            ..Default::default() };
        assert!(number_continues_conversion(&converting("2。", None)));
        assert!(number_continues_conversion(&converting("きょうは2、", None)));
        assert!(number_continues_conversion(&converting("。", Some('3'))));
        assert!(!number_continues_conversion(&converting("か。", None)));
        assert!(!number_continues_conversion(&converting("2", None)));
        // 変換のカーソルを途中の文節へ動かしていたら、今までどおり (カーソルの前を確定して入れる)
        let mut moved = Composition { hiragana: "2。".into(), state: CompositionState::Converting, cursor_moved: true,
            segments: build_segments("2。", vec![("2".into(), 1), ("。".into(), 1)]), ..Default::default() };
        assert!(!number_continues_conversion(&moved));
        moved.focus = 1;
        assert!(number_continues_conversion(&moved));
    }

    #[test]
    fn complete_live_conversion_never_reuses_a_shorter_latin_prefix() {
        for reading in ["my", "why", "sky", "try", "by", "hello", "github", "azookey", "ky", "sh", "Googlek", "これはmy", "きょうk", "2.5mol", "ni\u{f1}o"] {
            let comp = Composition {
                hiragana: reading.into(), live: true, refined: true,
                segments_reading: reading.chars().take(1).collect(),
                ..Default::default()
            };
            assert_eq!(live_refinement_reading(&comp, true), reading, "{reading}");
            assert_ne!(live_refinement_reading(&comp, true), comp.segments_reading);
        }
    }

    #[test]
    fn live_timer_still_excludes_unfinished_romaji_but_preserves_temporary_latin() {
        let mut comp = Composition { hiragana: "きょうk".into(), ..Default::default() };
        assert_eq!(live_refinement_reading(&comp, false), "きょう");
        assert_eq!(live_refinement_reading(&comp, true), "きょうk");
        comp.hiragana = "my".into();
        comp.temp_ascii = true;
        assert_eq!(live_refinement_reading(&comp, false), "my");
        comp.hiragana = "これはmy".into();
        comp.temp_ascii = false;
        comp.english_active = true;
        comp.typed_valid = false;
        assert_eq!(live_refinement_reading(&comp, false), "これはmy");
    }

    #[test]
    fn complete_live_conversion_keeps_the_pending_n_rules() {
        let mut comp = Composition::default();
        for (reading, expected) in [("n", "ん"), ("きょうn", "きょうん"), ("python", "python"), ("main", "main")] {
            comp.hiragana = reading.into();
            assert_eq!(live_refinement_reading(&comp, true), expected);
        }
        comp.hiragana = "n".into();
        comp.english_active = true;
        assert_eq!(composition_reading(&comp), "n");
        assert_eq!(live_refinement_reading(&comp, true), "n");
    }

    #[test]
    fn script_marker_returns_a_lone_vowel_kana_to_the_letter() {
        let mut comp = Composition::default();
        comp.reset();
        for (typed, hiragana, expected) in [
            ("a", "あ", Some('a')), ("suuretua", "すうれつあ", Some('a')), ("e", "え", Some('e')),
            ("ka", "か", None), ("xa", "ぁ", None), ("x", "x", None), ("a", "a", None), ("kan", "かn", None),
        ] {
            comp.typed = typed.into();
            comp.hiragana = hiragana.into();
            comp.caret = comp.hiragana_len();
            assert_eq!(vowel_before_script(&comp), expected, "{typed}");
        }
        comp.typed_valid = false;
        comp.typed = "a".into();
        comp.hiragana = "あ".into();
        assert_eq!(vowel_before_script(&comp), None);
    }

    #[test]
    fn enter_keeps_the_lone_n_for_people_who_type_nn() {
        let mut comp = Composition::default();
        for (reading, nn, expected) in [
            ("n", true, "n"), ("こn", true, "こn"), ("こんn", true, "こんn"), ("python", true, "python"),
            ("n", false, "ん"), ("こn", false, "こん"), ("python", false, "python"), ("こん", true, "こん"),
        ] {
            comp.hiragana = reading.into();
            assert_eq!(enter_reading(&comp, nn), expected, "{reading} nn={nn}");
        }
        comp.hiragana = "こn".into();
        comp.segments = build_segments("こ", vec![]);
        comp.segments_reading = "こ".into();
        assert_eq!(live_enter_suffix(&comp, true), "n");
        assert_eq!(live_enter_suffix(&comp, false), "ん");
        comp.hiragana = "こk".into();
        assert_eq!(live_enter_suffix(&comp, true), "k");
        comp.hiragana = "pythonn".into();
        comp.segments_reading = "python".into();
        assert_eq!(live_enter_suffix(&comp, false), "n");
    }

    /// 2026-10-31: 小文字を含む全角の英字の候補 (ｓｉｎ・ｋｅｎｗｏ) は、英字の文節の一覧にも日本語の文節の一覧にも出さない。
    /// 全部大文字の全角 (ＤＮＡ) と、一覧の頭 (F9 で全角にした今の表記) は残す
    #[test]
    fn fullwidth_lowercase_latin_is_not_listed() {
        let latin = finish_candidate_list(latin_spelling_candidates("sin", "sin", vec!["ｓｉｎ".into(), "SIN".into()]), "sin");
        assert!(latin.contains(&"sin".to_string()) && latin.contains(&"SIN".to_string()), "{latin:?}");
        assert!(!latin.iter().any(|c| has_fullwidth_lowercase(c)), "{latin:?}");
        let caps = finish_candidate_list(vec!["DNA".into(), "ＤＮＡ".into(), "ｄｎａ".into(), "Dna".into()], "DNA");
        assert_eq!(caps, ["DNA", "ＤＮＡ", "Dna"]);
        let japanese = finish_candidate_list(vec!["健を".into(), "ｋｅｎｗｏ".into(), "ＫＥＮＷＯ".into(), "けんを".into()], "けんを");
        assert_eq!(japanese, ["健を", "ＫＥＮＷＯ", "けんを", "ｹﾝｦ"]);
        let selected = finish_candidate_list(vec!["ｓｉｎ".into(), "sin".into(), "ｓｉｎｘ".into()], "sin");
        assert_eq!(selected, ["ｓｉｎ", "sin"]);
        let mut typed = vec!["健".to_string(), "県".to_string(), "けん".to_string()];
        place_typed_latin_last(&mut typed, typed_latin_candidate("けん", Some("ken")).as_deref());
        assert!(typed.contains(&"ken".to_string()) && !typed.iter().any(|c| has_fullwidth_lowercase(c)), "{typed:?}");
    }

    #[test]
    fn spelling_candidates_preserve_every_letter_not_only_the_first_one() {
        for first in b'a'..=b'z' {
            for last in b'a'..=b'z' {
                let reading = format!("{}{}", first as char, last as char);
                let shortened = (first as char).to_ascii_uppercase().to_string();
                let candidates = finish_candidate_list(
                    latin_spelling_candidates(&shortened, &reading, vec![shortened.clone(), reading.clone()]), &reading);
                assert_eq!(candidates[0], reading);
                assert!(candidates.contains(&reading.to_ascii_uppercase()));
                assert!(!candidates.contains(&to_fullwidth(&reading, true)));
                assert!(!candidates.contains(&shortened));
                for candidate in candidates {
                    assert_eq!(apply_character_form(&candidate).to_ascii_lowercase(), reading);
                }
            }
        }
    }

    #[test]
    fn complete_spelling_casing_and_chemical_notation_are_not_overwritten() {
        for (text, reading) in [("My", "my"), ("GitHub", "github"), ("H\u{2082}O", "H2O"), ("ni\u{f1}o", "ni\u{f1}o")] {
            let candidates = finish_candidate_list(latin_spelling_candidates(text, reading, vec![]), reading);
            assert_eq!(candidates[0], text);
        }
    }

    #[test]
    fn accent_letters_do_not_split_capitalization_inside_a_word() {
        for (word, capitalized, upper, broken) in [
            ("ni\u{f1}o", "Ni\u{f1}o", "NI\u{d1}O", "Ni\u{f1}O"),
            ("caf\u{e9}", "Caf\u{e9}", "CAF\u{c9}", "CAF\u{e9}"),
        ] {
            let candidates = latin_spelling_candidates(word, word, vec![]);
            assert!(candidates.contains(&capitalized.to_string()));
            assert!(candidates.contains(&upper.to_string()));
            assert!(!candidates.contains(&broken.to_string()));
        }
    }

    #[test]
    fn half_katakana_is_last_only_for_kana_readings() {
        let latin = vec!["my".to_string(), "My".to_string(), "MY".to_string()];
        assert_eq!(finish_candidate_list(latin.clone(), "my"), latin);
        let japanese = vec!["今日".to_string(), "ｷｮｳ".to_string(), "きょう".to_string(), SINGLE_KANJI_MARKER.to_string()];
        assert_eq!(finish_candidate_list(japanese, "きょう"), ["今日", "きょう", "ｷｮｳ", SINGLE_KANJI_MARKER]);
    }

    #[test]
    fn emoji_follow_all_ordinary_candidates_in_stable_order() {
        let emoji = ["\u{1f600}", "\u{2764}\u{fe0f}", "\u{1f44d}\u{1f3fd}",
            "\u{1f469}\u{200d}\u{1f4bb}", "\u{1f1ef}\u{1f1f5}", "1\u{fe0f}\u{20e3}"];
        let mut input = vec![emoji[0].to_string(), "笑顔".into(), "えがお".into(), SINGLE_KANJI_MARKER.into()];
        input.extend(emoji[1..].iter().map(|s| s.to_string()));
        input.push("Windows補完".into());
        let output = finish_candidate_list(input, "えがお");
        assert_eq!(&output[..5], &["笑顔", "えがお", "Windows補完", "ｴｶﾞｵ", SINGLE_KANJI_MARKER]);
        assert_eq!(&output[5..], &emoji);
        let symbols = ["1", "#", "*", "←", "→", "↑", "↓", "\u{2194}\u{fe0e}", "漢字", "髙"];
        for symbol in symbols { assert!(emojis::get(symbol).is_none(), "{symbol}"); }
        let input: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        assert_eq!(finish_candidate_list(input.clone(), "x"), input);
    }

    #[test]
    fn every_bundled_emoji_and_variant_is_recognized_without_rewriting_it() {
        let dictionary = include_str!(concat!(env!("CARGO_MANIFEST_DIR"),
            "/../../server-swift/azooKey_emoji_dictionary_storage/EmojiDictionary/emoji_all_E15.1.txt"));
        for line in dictionary.lines().filter(|line| !line.is_empty()) {
            let columns: Vec<_> = line.split('\t').collect();
            assert_eq!(columns.len(), 3);
            for emoji in std::iter::once(columns[0]).chain(columns[2].split(',').filter(|s| !s.is_empty())) {
                assert!(is_emoji_candidate(emoji), "unrecognized emoji: {emoji:?}");
                let input = vec![emoji.to_string(), "普通の候補".into()];
                assert_eq!(finish_candidate_list(input, "x"), ["普通の候補", emoji]);
            }
        }
    }

    #[test]
    fn moving_emoji_to_tail_preserves_the_selected_text_and_cycle() {
        let emoji = "\u{1f600}";
        let mut segment = Segment { text: emoji.into(), reading: "えがお".into(), ..Default::default() };
        finish_segment_candidates(&mut segment, vec![emoji.into(), "笑顔".into(), "えがお".into()]);
        assert_eq!(segment.text, emoji);
        assert_eq!(segment.index as usize, segment.candidates.len() - 1);
        let mut composition = Composition { segments: vec![segment], focus: 0, ..Default::default() };
        let next = composition.segments[0].index + 1;
        TextServiceFactory::select_candidate(&mut composition, next);
        assert_eq!(composition.segments[0].text, "笑顔");
    }

    #[test]
    fn segmented_latin_words_are_one_selectable_clause() {
        for (reading, converted, expected) in [
            ("my", vec![("M", 1), ("y", 1)], "My"),
            ("ni\u{f1}o", vec![("ni", 2), ("\u{f1}", 1), ("o", 1)], "ni\u{f1}o"),
            ("Espa\u{f1}a", vec![("Espa", 4), ("\u{f1}", 1), ("a", 1)], "Espa\u{f1}a"),
            ("SuperMario", vec![("Super", 5), ("Mario", 5)], "SuperMario"),
        ] {
            let segments = build_segments(reading, converted.into_iter().map(|(text, count)| (text.to_string(), count)).collect());
            assert_eq!(segments.len(), 1, "{reading}");
            assert_eq!(segments[0].reading, reading);
            assert_eq!(segments[0].text, expected);
        }
    }

    #[test]
    fn literal_word_merging_respects_japanese_spaces_numbers_and_changed_text() {
        for (reading, converted, expected_readings) in [
            ("これはmy", vec![("これは", 3), ("M", 1), ("y", 1)], vec!["これは", "my"]),
            ("myを", vec![("m", 1), ("y", 1), ("を", 1)], vec!["my", "を"]),
            ("myword", vec![("マイ", 2), ("word", 4)], vec!["my", "word"]),
            ("my word", vec![("my", 2), (" ", 1), ("word", 4)], vec!["my", " ", "word"]),
            ("H2O", vec![("H", 1), ("\u{2082}", 1), ("O", 1)], vec!["H", "2", "O"]),
        ] {
            let segments = build_segments(reading, converted.into_iter().map(|(text, count)| (text.to_string(), count)).collect());
            assert_eq!(segments.iter().map(|s| s.reading.as_str()).collect::<Vec<_>>(), expected_readings);
            assert_eq!(segments.iter().map(|s| s.reading.as_str()).collect::<String>(), reading);
        }
    }

    #[test]
    fn client_segments_and_candidates_against_isolated_dll() {
        let Ok(path) = std::env::var("AZK_LATIN_SERVER_PROBE") else { return };
        let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut checked = 0;
        for row in report["results"].as_array().unwrap() {
            let reading = row["reading"].as_str().unwrap();
            let converted = row["segments"].as_array().unwrap().iter().map(|segment| {
                (segment[0].as_str().unwrap().to_string(), segment[1].as_i64().unwrap() as i32)
            }).collect();
            let segments = build_segments(reading, converted);
            assert_eq!(segments.iter().map(|s| s.reading.as_str()).collect::<String>(), reading);
            if reading.chars().all(|c| c.is_alphabetic() && (c.is_ascii() || ('\u{00c0}'..='\u{024f}').contains(&c))) {
                assert_eq!(segments.len(), 1, "{reading}");
                let segment = &segments[0];
                let server_candidates = row["server_candidates"].as_array().unwrap().iter().map(|text| text.as_str().unwrap().to_string()).collect();
                let candidates = finish_candidate_list(latin_spelling_candidates(&segment.text, reading, server_candidates), reading);
                assert!(candidates.contains(&reading.to_string()), "{reading}: {candidates:?}");
                if reading.is_ascii() {
                    assert!(candidates.contains(&reading.to_ascii_uppercase()));
                    let full = to_fullwidth(reading, true);
                    assert_eq!(candidates.contains(&full), !has_fullwidth_lowercase(&full), "{reading}: {candidates:?}");
                }
            }
            checked += 1;
        }
        assert_eq!(checked, 15);
    }

    #[test]
    fn latin_conversion_probe() {
        let Ok(path) = std::env::var("AZK_LATIN_CONVERSION_PROBE") else { return };
        let readings = ["my", "why", "sky", "try", "by", "hello", "github", "azookey", "supermario", "ky", "sh", "Googlek", "これはmy", "きょうk", "ni\u{f1}o"];
        let mut rows = vec![];
        for reading in readings {
            let comp = Composition { hiragana: reading.into(), live: true, english_active: true,
                typed: reading.into(), typed_valid: true, ..Default::default() };
            let complete = live_refinement_reading(&comp, true);
            assert_eq!(complete, reading);
            let candidates = if reading.chars().all(is_latin_letter) {
                let initial = reading.chars().next().unwrap().to_string();
                finish_candidate_list(latin_spelling_candidates(&initial, reading, vec![]), reading)
            } else {
                vec![]
            };
            rows.push(serde_json::json!({"reading":reading,"complete":complete,
                "timer":live_refinement_reading(&comp, false),"spelling_candidates":candidates}));
        }
        std::fs::write(path, serde_json::to_string_pretty(&rows).unwrap()).unwrap();
    }
}

/// Shift_JIS (CP932) で表せない、または NEC/IBM 拡張の文字を含むか (= MS-IME の「環境依存」)
fn is_environment_dependent(text: &str) -> bool {
    use windows::Win32::Globalization::{WideCharToMultiByte, WC_NO_BEST_FIT_CHARS};
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return false;
    }
    let mut buffer = vec![0u8; wide.len() * 2 + 4];
    let mut used_default = windows::Win32::Foundation::BOOL(0);
    let written = unsafe {
        WideCharToMultiByte(
            932,
            WC_NO_BEST_FIT_CHARS,
            &wide,
            Some(&mut buffer),
            windows::core::PCSTR::null(),
            Some(&mut used_default),
        )
    };
    if written <= 0 || used_default.as_bool() {
        return true;
    }
    // CP932 の 2 バイト文字のうち NEC 特殊文字 (0x87xx) と IBM 拡張 (0xED/0xEE/0xFA-0xFC) は環境依存
    let bytes = &buffer[..written as usize];
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let lead = (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b);
        if lead {
            if b == 0x87 || b == 0xED || b == 0xEE || (0xFA..=0xFC).contains(&b) {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

/// 候補の右に出す注記 (Google 日本語入力 / MS-IME と同じく文字種と環境依存)
/// 2026-10-22 (M3.6): 設定 input.misuse_notes・homophone_notes (初期値はどちらもオフ)
/// 2026-11-06 (仕事 40): 注意の注記の 2 段 input.caution_slurs (差別・強い侮辱の語)・caution_broadcast (放送などで言い換えられる語)。初期値はどちらもオフ
#[derive(Clone, Copy, Default)]
struct NoteSettings {
    misuse: bool,
    homophone: bool,
    slurs: bool,
    broadcast: bool,
}

impl NoteSettings {
    /// 変換サーバーに注記を作らせるか (どれか 1 つでもオンなら)
    fn any(self) -> bool {
        self.misuse || self.homophone || self.slurs || self.broadcast
    }
}

fn note_settings() -> NoteSettings {
    static SETTINGS: std::sync::OnceLock<NoteSettings> = std::sync::OnceLock::new();
    *SETTINGS.get_or_init(|| {
        let input = shared::AppConfig::read_or_default().input;
        NoteSettings { misuse: input.misuse_notes, homophone: input.homophone_notes, slurs: input.caution_slurs, broadcast: input.caution_broadcast }
    })
}

/// 変換サーバーの注記 (「種類<TAB>注記」) のうち、設定でオンの種類のもの (誤用 = 本来は〜、異字同訓 = 使い分け、差別・言い換え = 注意)
fn allowed_note(note: &str, settings: NoteSettings) -> Option<&str> {
    let (kind, text) = note.split_once('\t')?;
    match kind {
        "誤用" if settings.misuse => Some(text),
        "異字同訓" if settings.homophone => Some(text),
        "差別" if settings.slurs => Some(text),
        "言い換え" if settings.broadcast => Some(text),
        // 2026-10-28: 母音の抜けを直したことの注記は、直す設定がオンなら出す
        "直し" => Some(text),
        // 2026-11-20: 読み間違いの本来の形 (ごちゅう → 御中) の「もしかして: 正しい読み」は、設定に関係なくいつも出す (Google 日本語入力と同じ。misuse.swift)
        "読み誤り" => Some(text),
        _ => None,
    }
}

/// 候補窓に出す注記 (制御文字を除き 200 字まで。D の 2 回目の調べ M-2・M-3)
/// 2026-11-08: 一覧の候補が学習した語を含む印 (segment.notes に、注記と並べて持つ。変換サーバーの注記の種類とは別)
const LEARNED_NOTE: &str = "\u{1}learned";
/// 学習した語を含む候補を選んでいる間だけ、候補の窓の注記の欄に出す案内 (候補の窓は U+001E のあとを、選んでいる行だけに出す)
const FORGET_HINT: &str = "Ctrl+Delete で忘れる";

fn server_note(segment: &Segment, text: &str) -> Option<String> {
    let note = segment.notes.iter().find(|(candidate, note)| candidate == text && note != LEARNED_NOTE).map(|(_, note)| note.as_str())?;
    allowed_note(note, note_settings()).map(|note| note.chars().filter(|c| !c.is_control()).take(200).collect())
}

/// 候補窓へ送る並び (候補<U+001F>注記)。注記を付けると候補窓の上限 (1 項目 8,192 バイト・1 回 65,536 バイト) を超えるときは、注記を付けずに送る
fn annotated_candidates(segment: &Segment) -> Vec<String> {
    // 2026-11-08: 学習した語を含む候補には「U+001E 案内」を足す (候補の窓は、選んでいる行だけに出す)
    let hint = |c: &String| if segment.notes.iter().any(|(candidate, note)| candidate == c && note == LEARNED_NOTE) {
        format!("\u{1e}{FORGET_HINT}")
    } else {
        String::new()
    };
    let plain = |c: &String| format!("{c}\u{1f}{}{}", candidate_note(c), hint(c));
    let noted: Vec<String> = segment.candidates.iter()
        .map(|c| server_note(segment, c).map_or_else(|| plain(c), |note| format!("{c}\u{1f}{note}{}", hint(c))))
        .collect();
    if noted.iter().any(|item| item.len() > 8_000) || noted.iter().map(String::len).sum::<usize>() > 60_000 {
        return segment.candidates.iter().map(plain).collect();
    }
    noted
}

fn candidate_note(text: &str) -> &'static str {
    if text.is_empty() || text == SINGLE_KANJI_MARKER {
        return "";
    }
    if super::ivs::contains_ivs(text) || super::ivs::contains_encoded_variant(text) {
        return "異体字";
    }
    let all = |f: fn(char) -> bool| text.chars().all(f);
    if all(|c| ('\u{3041}'..='\u{309F}').contains(&c) || c == 'ー') {
        "ひらがな"
    } else if all(|c| ('\u{30A1}'..='\u{30FF}').contains(&c)) {
        "カタカナ"
    } else if all(|c| ('\u{FF61}'..='\u{FF9F}').contains(&c)) {
        "[半] カタカナ"
    } else if all(|c| ('\u{FF01}'..='\u{FF5E}').contains(&c)) {
        "[全]"
    } else if all(|c| c.is_ascii() && !c.is_ascii_control()) {
        "[半]"
    } else if is_environment_dependent(text) {
        "環境依存"
    } else {
        ""
    }
}

/// 無変換キー 1 回分。戻り値は (新しい表記, 巡回順)。
/// かなの切り替えに、元の打鍵の小文字・先頭大文字・全大文字も含める。
fn next_kana_type(text: &str, reading: &str, typed: Option<&str>, cycle: &[String]) -> (String, Vec<String>) {
    let hiragana = latin_kana_reading(reading).unwrap_or_else(|| reading.to_string());
    let katakana = to_katakana(&hiragana);
    let half = to_half_katakana(&hiragana);
    if !cycle.iter().any(|t| t == text) {
        let mut cycle = if text == hiragana {
            vec![katakana, half, hiragana]
        } else if text == katakana {
            vec![hiragana, half, katakana]
        } else {
            vec![hiragana, katakana, half]
        };
        let roman = typed.map(str::to_string).unwrap_or_else(|| to_romaji(reading, &super::auto_english::spelling()));
        if !roman.is_empty() && roman.chars().any(is_latin_letter)
            && roman.chars().all(|c| is_latin_letter(c) || "'- ".contains(c)) {
            let lower = apply_character_form(&roman).to_lowercase();
            let mut chars = lower.chars();
            let title = chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default();
            let upper = lower.to_uppercase();
            let end = if text == reading || text == to_katakana(reading) { cycle.len() - 1 } else { cycle.len() };
            cycle.splice(end..end, [lower, title, upper]);
        }
        let mut seen = std::collections::HashSet::new();
        cycle.retain(|item| seen.insert(item.clone()));
        return (cycle[0].clone(), cycle);
    }
    let position = cycle.iter().position(|t| t == text).unwrap_or(0);
    (cycle[(position + 1) % cycle.len()].clone(), cycle.to_vec())
}

#[cfg(test)]
mod kana_type_tests {
    use super::*;

    #[test]
    fn admission_numeral_variants_stay_together_after_candidate_merging() {
        let vocabulary: serde_json::Value = serde_json::from_str(include_str!("../../../../japanese_vocabulary.json")).unwrap();
        let mut checked = 0;
        for entry in vocabulary["entries"].as_array().unwrap() {
            let Some(variants) = entry["variants"].as_array() else { continue };
            let reading = entry["reading"].as_str().unwrap();
            let kanji = entry["word"].as_str().unwrap();
            let roman = variants[0].as_str().unwrap();
            for (first, second) in [(kanji, roman), (roman, kanji)] {
                let input = vec![first.into(), "別候補".into(), "離散".into(), second.into()];
                let output = finish_candidate_list(input, reading);
                assert_eq!(&output[..4], [first, second, "別候補", "離散"], "{reading}");
            }
            let input = vec!["通常候補".into(), kanji.into(), "別候補".into(), roman.into()];
            let output = finish_candidate_list(input, reading);
            assert_eq!(&output[..4], ["通常候補", kanji, roman, "別候補"]);
            checked += 1;
        }
        assert_eq!(checked, 6);
        let untouched = vec!["離散数学".into(), "理三".into(), "別候補".into(), "理Ⅲ".into()];
        assert_eq!(&finish_candidate_list(untouched.clone(), "りさんすうがく")[..4], &untouched);
    }

    #[test]
    fn accent_picker_starts_with_the_top_candidate_selected() {
        let mut comp = Composition::default();
        assert_eq!(comp.begin_accents('u', &['\u{fa}', '\u{fc}']), Some('\u{fa}'));
        assert_eq!(comp.accent, Some('u'));
        assert_eq!(comp.accent_index, 0);
        assert_eq!(comp.accent_inserted, None);
        comp.close_accents();
        assert_eq!(comp.begin_accents('x', &[]), None);
        assert_eq!(comp.accent, None);
        assert_eq!(comp.accent_index, -1);
    }

    #[test]
    fn accent_reselection_replaces_one_letter_and_close_keeps_it() {
        for (typed, base, variants) in [
            ("espana", 'n', vec!['\u{f1}']),
            ("na", 'a', vec!['\u{e1}']),
            ("cafe", 'e', vec!['\u{e9}', '\u{e8}', '\u{ea}']),
            ("N", 'N', vec!['\u{d1}']),
        ] {
            let typed = if typed == "espana" { "espan" } else { typed };
            let mut comp = Composition { state: CompositionState::Composing, typed: typed.into(), typed_valid: true, ..Default::default() };
            let first = comp.begin_accents(base, &variants).unwrap();
            assert!(comp.replace_typed_accent(first));
            comp.accent_inserted = Some(first);
            let len = typed.chars().count();
            for ch in variants.iter().chain(variants.iter().rev()) {
                assert!(comp.replace_typed_accent(*ch));
                comp.accent_inserted = Some(*ch);
                assert_eq!(comp.typed.chars().count(), len);
                assert!(comp.typed.ends_with(*ch));
            }
            let inserted = comp.typed.clone();
            comp.close_accents();
            assert_eq!(comp.typed, inserted);
            assert_eq!(comp.accent_inserted, None);
            comp.typed.push(base);
            assert!(comp.typed.ends_with(base));
            assert_eq!(comp.typed.chars().count(), len + 1);
        }
    }

    #[test]
    fn accent_replacement_does_not_rewrite_unrelated_or_committed_text() {
        let mut comp = Composition { state: CompositionState::Composing, typed: "kana".into(), typed_valid: true, ..Default::default() };
        comp.begin_accents('n', &['\u{f1}']);
        assert!(!comp.replace_typed_accent('\u{f1}'));
        assert_eq!(comp.typed, "kana");
        comp.typed = "n".into();
        comp.typed_valid = false;
        assert!(!comp.replace_typed_accent('\u{f1}'));
        comp.typed_valid = true;
        comp.state = CompositionState::None;
        assert!(!comp.replace_typed_accent('\u{f1}'));
        comp.state = CompositionState::Converting;
        assert!(!comp.replace_typed_accent('\u{f1}'));
        comp.reset();
        assert_eq!(comp.accent, None);
        assert_eq!(comp.accent_index, -1);
        assert_eq!(comp.accent_inserted, None);
    }

    #[test]
    fn held_latin_key_uses_typed_letter_after_kana_conversion() {
        for (typed, reading, base) in [("a", "あ", 'a'), ("e", "え", 'e'), ("na", "な", 'a'), ("espana", "えすぱな", 'a'), ("nn", "ん", 'n')] {
            let mut comp = Composition::default();
            comp.state = CompositionState::Composing;
            comp.typed = typed.into();
            comp.typed_valid = true;
            comp.hiragana = reading.into();
            comp.caret = comp.hiragana_len();
            assert!(comp.at_accent_letter(base), "{typed}");
            assert!(!comp.at_accent_letter('z'));
            comp.caret -= 1;
            assert!(!comp.at_accent_letter(base));
            comp.caret += 1;
            comp.typed_valid = false;
            assert!(!comp.at_accent_letter(base));
            comp.typed_valid = true;
            comp.state = CompositionState::Converting;
            assert!(!comp.at_accent_letter(base));
        }
        let mut comp = Composition::default();
        assert!(comp.at_accent_letter('a'));
        comp.state = CompositionState::Composing;
        comp.hiragana = "n".into();
        comp.caret = 1;
        assert!(comp.at_accent_letter('n'));
    }

    fn presses(start: &str, reading: &str, n: usize) -> Vec<String> {
        let mut text = start.to_string();
        let mut cycle = vec![];
        let mut out = vec![];
        for _ in 0..n {
            let (t, c) = next_kana_type(&text, reading, Some("kyou"), &cycle);
            text = t;
            cycle = c;
            out.push(text.clone());
        }
        out
    }

    #[test]
    fn hiragana_first_cycles_through_kana_then_latin() {
        assert_eq!(presses("きょう", "きょう", 7), ["キョウ", "ｷｮｳ", "kyou", "Kyou", "KYOU", "きょう", "キョウ"]);
    }

    #[test]
    fn katakana_first_cycles_through_kana_then_latin() {
        assert_eq!(presses("キョウ", "きょう", 7), ["きょう", "ｷｮｳ", "kyou", "Kyou", "KYOU", "キョウ", "きょう"]);
    }

    #[test]
    fn kanji_first_cycles_through_kana_then_latin() {
        assert_eq!(presses("今日", "きょう", 7), ["きょう", "キョウ", "ｷｮｳ", "kyou", "Kyou", "KYOU", "きょう"]);
    }

    #[test]
    fn muhenkan_adds_lowercase_title_and_all_caps() {
        for (reading, typed, lower, title) in [("みそ", "miso", "miso", "Miso"), ("し", "shi", "shi", "Shi"),
            ("し", "si", "si", "Si"), ("いん", "in", "in", "In"), ("いん", "inn", "inn", "Inn")] {
            let (_, cycle) = next_kana_type(reading, reading, Some(typed), &[]);
            assert!(cycle.contains(&lower.to_string()));
            assert!(cycle.contains(&title.to_string()));
            assert!(cycle.contains(&typed.to_uppercase()));
            assert_eq!(cycle.iter().collect::<std::collections::HashSet<_>>().len(), cycle.len());
        }
        let (_, cycle) = next_kana_type("１２", "12", Some("12"), &[]);
        assert!(!cycle.iter().any(|text| text.chars().any(is_latin_letter)));
        let (_, cycle) = next_kana_type("みそ", "みそ", None, &[]);
        assert!(cycle.contains(&"miso".to_string()));
    }

    #[test]
    fn open_candidate_cycle_never_commits_latin_as_space() {
        let mut comp = Composition { hiragana: "みそ".into(), typed: "miso".into(), typed_valid: true,
            window_open: true, state: CompositionState::Converting, ..Default::default() };
        comp.segments = build_segments("みそ", vec![("味噌".into(), 2)]);
        comp.segments[0].candidates = vec!["味噌".into(), "miso".into(), "Miso".into(), "MISO".into()];
        for index in 0..9 {
            TextServiceFactory::select_candidate(&mut comp, index);
            assert!(!space_commits_selected_latin(&comp));
            assert_eq!(comp.segments[0].index, index % 4);
            assert!(!comp.converted_text().trim().is_empty());
        }
        TextServiceFactory::select_candidate(&mut comp, 1);
        assert!(comp.latin_selected);
        comp.window_open = false;
        assert!(space_commits_selected_latin(&comp));
    }

    #[test]
    fn misuse_notes_follow_the_settings() {
        // 既定 (どれもオフ) では出さない。種類ごとに切り替える
        let off = NoteSettings::default();
        let misuse = NoteSettings { misuse: true, ..off };
        let homophone = NoteSettings { homophone: true, ..off };
        assert!(!off.any());
        assert_eq!(allowed_note("誤用\t本来は 的を射る", off), None);
        assert_eq!(allowed_note("誤用\t本来は 的を射る", misuse), Some("本来は 的を射る"));
        assert_eq!(allowed_note("異字同訓\t一致する", misuse), None);
        assert_eq!(allowed_note("異字同訓\t一致する", homophone), Some("一致する"));
        assert_eq!(allowed_note("", NoteSettings { misuse: true, homophone: true, slurs: true, broadcast: true }), None);
    }

    #[test]
    fn misreading_notes_are_always_shown() {
        // 2026-11-20: 読み間違いの注記「もしかして: 正しい読み」は、注記の設定がみなオフ (既定) でも出す
        let off = NoteSettings::default();
        assert_eq!(allowed_note("読み誤り\tもしかして: おんちゅう", off), Some("もしかして: おんちゅう"));
        assert_eq!(allowed_note("読み誤り\tもしかして: おんちゅう", NoteSettings { misuse: true, ..off }), Some("もしかして: おんちゅう"));
        let mut segment = Segment { candidates: vec!["ごちゅう".into(), "御中".into()], ..Default::default() };
        segment.notes = vec![("御中".into(), "読み誤り\tもしかして: おんちゅう".into())];
        assert_eq!(annotated_candidates(&segment)[1], "御中\u{1f}もしかして: おんちゅう");
    }

    #[test]
    fn caution_notes_follow_their_own_settings() {
        // 2026-11-06 (仕事 40): 注意の注記の 2 段は、それぞれの設定がオンのときだけ (誤用・異字同訓の設定では出さない)
        let off = NoteSettings::default();
        let slurs = NoteSettings { slurs: true, ..off };
        let broadcast = NoteSettings { broadcast: true, ..off };
        let others = NoteSettings { misuse: true, homophone: true, ..off };
        assert!(slurs.any() && broadcast.any());
        assert_eq!(allowed_note("差別\t注意: 差別の語とされます", off), None);
        assert_eq!(allowed_note("差別\t注意: 差別の語とされます", others), None);
        assert_eq!(allowed_note("差別\t注意: 差別の語とされます", broadcast), None);
        assert_eq!(allowed_note("差別\t注意: 差別の語とされます", slurs), Some("注意: 差別の語とされます"));
        assert_eq!(allowed_note("言い換え\t注意: 言い換えられることがある語です", slurs), None);
        assert_eq!(allowed_note("言い換え\t注意: 言い換えられることがある語です", broadcast), Some("注意: 言い換えられることがある語です"));
    }

    #[test]
    fn caution_notes_in_the_published_table_follow_the_settings() {
        // 2026-11-07 (仕事 40): 配る表 (misuse_notes.tsv) の 差別・言い換え の行は、どの設定もオフなら 1 つも出ず、それぞれの設定のときだけ出る
        // (変換サーバーは「種類<TAB>注記」を候補に付けて返す)。誤用・異字同訓の設定では出ない
        let table = include_str!("../../../../misuse_notes.tsv");
        let off = NoteSettings::default();
        let slurs = NoteSettings { slurs: true, ..off };
        let broadcast = NoteSettings { broadcast: true, ..off };
        let others = NoteSettings { misuse: true, homophone: true, ..off };
        let (mut slur_rows, mut broadcast_rows) = (0, 0);
        for line in table.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
            let cols: Vec<&str> = line.split('\t').collect();
            let note = format!("{}\t{}", cols[0], cols[3]);
            assert_eq!(allowed_note(&note, off), None, "{line}");
            match cols[0] {
                "差別" => {
                    slur_rows += 1;
                    assert!(allowed_note(&note, slurs).is_some(), "{line}");
                    assert_eq!(allowed_note(&note, broadcast), None, "{line}");
                    assert_eq!(allowed_note(&note, others), None, "{line}");
                }
                "言い換え" => {
                    broadcast_rows += 1;
                    assert!(allowed_note(&note, broadcast).is_some(), "{line}");
                    assert_eq!(allowed_note(&note, slurs), None, "{line}");
                    assert_eq!(allowed_note(&note, others), None, "{line}");
                }
                _ => {
                    assert_eq!(allowed_note(&note, slurs), None, "{line}");
                    assert_eq!(allowed_note(&note, broadcast), None, "{line}");
                }
            }
        }
        assert!(slur_rows > 400 && broadcast_rows > 100, "{slur_rows} {broadcast_rows}");
    }

    #[test]
    fn candidate_notes() {
        assert_eq!(candidate_note("play"), "[半]");
        assert_eq!(candidate_note("ＡＢＣ"), "[全]");
        assert_eq!(candidate_note("プレイ"), "カタカナ");
        assert_eq!(candidate_note("ﾌﾟﾚｲ"), "[半] カタカナ");
        assert_eq!(candidate_note("ぷれい"), "ひらがな");
        assert_eq!(candidate_note("①"), "環境依存");
        assert_eq!(candidate_note("髙"), "異体字");
        assert_eq!(candidate_note("𠮷"), "環境依存");
        assert_eq!(candidate_note("高"), "");
        assert_eq!(candidate_note("今日は"), "");
        assert_eq!(candidate_note("邊\u{e0101}"), "異体字");
        assert_eq!(candidate_note("渡邊\u{e0101}さん"), "異体字");
        assert_eq!(candidate_note("邊"), "");
    }

    /// 2026-11-08: 学習した語を含む候補には「U+001E Ctrl+Delete で忘れる」を足す (候補の窓は選んでいる行だけに出す)。注記としては出さない
    #[test]
    fn learned_candidates_carry_the_forget_hint() {
        let mut segment = build_segments("かき", vec![]).remove(0);
        segment.candidates = vec!["柿".into(), "牡蠣".into(), "夏期".into()];
        segment.notes = vec![("牡蠣".into(), LEARNED_NOTE.into())];
        let annotated = annotated_candidates(&segment);
        assert_eq!(annotated[0], "柿\u{1f}");
        assert_eq!(annotated[1], format!("牡蠣\u{1f}\u{1e}{FORGET_HINT}"));
        assert_eq!(annotated[2], "夏期\u{1f}");
        assert_eq!(server_note(&segment, "牡蠣"), None);
    }

    #[test]
    fn windows_conversion_api() {
        let list = windows_candidates("こーひー");
        assert!(list.iter().any(|c| c == "珈琲"), "{list:?}");
    }

    /// 実際のサーバーを相手に、IME と同じ手順 (1 文字ずつ打つ → 英語の判定 → 組み直し) を再現する
    /// (AZK_LIVE_TEST を指定したときだけ。変換サーバーが動いている必要がある)
    #[test]
    fn typing_against_server() {
        let Ok(inputs) = std::env::var("AZK_LIVE_TEST") else { return };
        let mut ipc = crate::engine::ipc_service::IPCService::new().unwrap();
        let mut report = String::new();
        for input in inputs.split('|') {
            let mut comp = Composition::default();
            comp.reset();
            ipc.clear_text().unwrap();
            for ch in input.chars() {
                // handle_action の ClientAction::Insert と同じ流れ (一時的な英数入力 → 英語の自動判別)
                if TextServiceFactory::update_temp_ascii(&mut comp, &ch.to_string()) {
                    TextServiceFactory::insert_temp_ascii(&mut ipc, &mut comp, &ch.to_string()).unwrap();
                    continue;
                }
                comp.typed.push(ch);
                let text = to_fullwidth(&ch.to_string(), false);
                let (h, c, r) = ipc.edit_text_raw(Op::Insert, text, 0).unwrap();
                comp.hiragana = apply_character_form(&h);
                comp.caret = c;
                comp.raw_input = r;
                if comp.typed_valid {
                    TextServiceFactory::apply_auto_english(&mut ipc, &mut comp, false).unwrap();
                }
            }
            let reading = composition_reading(&comp);
            let converted = ipc.convert(reading.clone(), String::new()).unwrap();
            let text: Vec<String> = converted.into_iter().map(|(t, _)| t).collect();
            // ライブ変換の仮表示 (辞書だけ) と清書 (Zenzai 1 回)
            let draft: Vec<String> = ipc.convert_with(reading.clone(), String::new(), true, true).unwrap().into_iter().map(|(t, _)| t).collect();
            let refined: Vec<String> = ipc.convert_with(reading.clone(), String::new(), true, false).unwrap().into_iter().map(|(t, _)| t).collect();
            report += &format!("{input} -> 読み={reading} -> {} ／ 仮表示 {} ／ 清書 {}\n", text.join(" / "), draft.join(" / "), refined.join(" / "));
        }
        ipc.clear_text().unwrap();
        std::fs::write(std::env::temp_dir().join("typing_test.txt"), report).unwrap();
    }

    #[test]
    fn half_katakana_goes_to_hiragana() {
        assert_eq!(presses("ｷｮｳ", "きょう", 1), ["きょう"]);
    }
}

/// 末尾に残った n を「ん」にする (サーバー側の変換と同じ規則)
/// 読みの末尾が、かなの後ろの打ちかけの n (こn) か。英単語の末尾の n (python) は除く
fn ends_with_pending_n(hiragana: &str) -> bool {
    let chars: Vec<char> = hiragana.chars().collect();
    match chars.as_slice() {
        [.., prev, 'n'] => !prev.is_ascii_alphabetic(),
        ['n'] => true,
        _ => false,
    }
}

/// ローマ字の打ち方を判断するための回数を足す (英語と判定した部分は数えない。打鍵から作ったかなが読みと合うときだけ。1 回の入力で 1 度だけ)
fn record_romaji_style(comp: &mut Composition) {
    if !may_learn() {
        return;
    }
    if comp.style_counted || !comp.typed_valid || comp.typed.is_empty() {
        return;
    }
    comp.style_counted = true;
    let tokens = comp.english_active.then(|| super::auto_english::segment(&comp.typed));
    let add = super::romaji_style::count_commit(&comp.typed, &comp.hiragana, tokens.as_deref());
    super::romaji_style::record(&add);
}

/// 読みの最後の字が、母音 1 打で入ったかな (a → あ) なら、その英字。^ _ の直前なら英字に戻す (a_n → aₙ、e^x → eˣ)
fn vowel_before_script(comp: &Composition) -> Option<char> {
    if !comp.typed_valid || comp.typed.is_empty() || comp.caret < comp.hiragana_len() {
        return None;
    }
    let units = super::romaji_style::simulate(&comp.typed);
    let last = units.last().filter(|u| !u.pending && u.keys.len() == 1)?;
    let vowel = last.keys.chars().next().filter(|c| "aiueo".contains(*c))?;
    comp.hiragana.ends_with(last.kana.as_str()).then_some(vowel)
}

/// 読みの末尾が、かなの後ろ (または頭) に 1 つだけある n か (でn・n)。英単語の n (python・on) は違う
fn ends_with_lone_romaji_n(hiragana: &str) -> bool {
    let mut chars = hiragana.chars().rev();
    matches!(chars.next(), Some('n')) && !chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
}

fn normalize_reading(hiragana: &str) -> String {
    let mut text = hiragana.to_string();
    // 英単語の末尾の n (python など) は ん にしない
    let chars: Vec<char> = text.chars().collect();
    let previous_is_latin = chars.len() >= 2 && chars[chars.len() - 2].is_ascii_alphabetic();
    if (text.ends_with('n') || text.ends_with('ｎ')) && !previous_is_latin {
        text.pop();
        text.push('ん');
    }
    text
}

/// Enter で確定する読み (変換していない入力中)。ん を nn と打つ人には n 1 つは ん ではないので、最後の打ちかけの n は
/// 見えているとおり n のまま確定する (2026-10-17)。n 1 つで ん と打つ人は今までどおり ん
fn enter_reading(comp: &Composition, nn: bool) -> String {
    if nn && ends_with_pending_n(&comp.hiragana) {
        comp.hiragana.clone()
    } else {
        composition_reading(comp)
    }
}

/// ライブ変換の Enter で、変換済みの後ろに付ける打ちかけ。n で止めたら ん (nn と打つ人は見えているとおり n のまま)
fn live_enter_suffix(comp: &Composition, nn: bool) -> String {
    match live_pending_suffix(comp).unwrap_or_default() {
        p if p == "n" && !nn && ends_with_pending_n(&comp.hiragana) => "ん".to_string(),
        p => p,
    }
}

fn composition_reading(comp: &Composition) -> String {
    if comp.english_active && !comp.hiragana.is_empty() && comp.hiragana.chars().all(is_latin_letter) {
        comp.hiragana.clone()
    } else {
        normalize_reading(&comp.hiragana)
    }
}

/// 英数字は半角にする。記号は設定 (input.fullwidth_symbols) で記号ごとに全角か半角か、¥ キー (￥) は input.yen_key で ¥ か \\ か。
/// 「ろ」キー・US 配列の \\ キー (＼) はいつも \\ (全角・半角は \\ の設定に従う)
/// ローマ字表で日本語の記号になるもの (。、・「」ー〜) はそのまま
/// 2026-10-24: 数字に続けて数字を打ったとき、読みの最後の 。 、 を小数点・桁区切りに直すなら、入れる字 ("." か ",")。
/// 。 、 の前の数字が今の入力の中にある (2。) か、入力が 。 、 だけで、入力を始めたときのキャレットの直前の字が数字のとき
/// (2 を変換・確定したあとに .5 と打った。変換中に . を打つと 2 を確定して 。 から入力を始め直すので、前は今の入力の中に無い)。
/// 前の数字は全角 (２) でもよい。入れるのはいつも半角
fn number_mark_fix(hiragana: &str, char_before: Option<char>) -> Option<&'static str> {
    let chars: Vec<char> = hiragana.chars().collect();
    let (&mark, rest) = chars.split_last()?;
    if !matches!(mark, '。' | '、' | '．' | '，' | '.' | ',') {
        return None;
    }
    let digit_before = match rest.last() {
        Some(c) => c.is_ascii_digit(),
        None => char_before.is_some_and(is_number_digit),
    };
    digit_before.then_some(if matches!(mark, '。' | '．' | '.') { "." } else { "," })
}

/// 2026-10-28: 打った文字の列から読みを組み直すとき (apply_auto_english)、数字の間の . , (小数点・桁区切り) として英字のまま入れる位置 (バイト)。
/// 打っている間の決まり (number_mark_fix・fix_thousands_comma) と同じ: 前が数字 (列の頭なら入力を始めたときのキャレットの直前の字) で、後ろも数字。
/// , は後ろの数字がちょうど 3 桁か、まだ 3 桁に届かずに列の終わりまで数字のとき。組み直すとローマ字の表で 、 。 に戻っていた
/// (12 + Enter + ,345ninndesu の n で英語の判定が組み直し、人口は12、345人です になっていた。B の写経 10-28)
fn number_marks_in_typed(typed: &str, char_before: Option<char>) -> Vec<usize> {
    let b = typed.as_bytes();
    (0..b.len()).filter(|&i| {
        let mark = b[i];
        // 2026-10-30: 数字の前のマイナス・範囲の - (minus_fix と同じ決まり。前が列の頭なら入力を始めたときの直前の字、列の中なら英字でも数字でもない記号)
        if mark == b'-' {
            let before = if i == 0 { char_before.map_or(true, |c| !japanese_letter(c) && c != 'ー') } else { b[i - 1].is_ascii_punctuation() || b[i - 1] == b' ' };
            return before && b.get(i + 1).is_some_and(|c| c.is_ascii_digit());
        }
        if mark != b'.' && mark != b',' { return false; }
        let before = if i == 0 { char_before.is_some_and(is_number_digit) } else { b[i - 1].is_ascii_digit() };
        let after = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        before && after > 0 && (mark == b'.' || after == 3 || (after < 3 && i + 1 + after == b.len()))
    }).collect()
}

/// 半角か全角の数字
fn is_number_digit(c: char) -> bool {
    c.is_ascii_digit() || ('０'..='９').contains(&c)
}

/// 2026-10-30: かな・漢字 (と長音・々)。この字のあとの ー は日本語の長音 (あー3・そっかー1・ナンバー1)
fn japanese_letter(c: char) -> bool {
    ('\u{3041}'..='\u{30ff}').contains(&c) || ('\u{3400}'..='\u{9fff}').contains(&c) || ('\u{f900}'..='\u{faff}').contains(&c)
        || ('\u{ff66}'..='\u{ff9f}').contains(&c) || c == '々'
}

/// 2026-10-30: 読みの最後の ー (- を打った) に続けて数字を打ったら、マイナス・範囲の - に直すか (-126・x=-1・(-2))。
/// ー の前が 入力の頭 (入力を始めたときのキャレットの直前の字が、無い・空白・数字・英字・記号) か、かな・漢字でない字のとき。
/// かな・漢字のあとは日本語の長音のまま (あー3なのか・えー2.6じゃないの・そっかー1だと思ってた・ナンバー1)。
/// 英字・数字のすぐ後ろの - は打った時点でハイフンになる (Insert の - の決まり) ので、ここに来るのはそれ以外
fn minus_fix(hiragana: &str, char_before: Option<char>) -> bool {
    let chars: Vec<char> = hiragana.chars().collect();
    let Some((&mark, rest)) = chars.split_last() else { return false };
    if mark != 'ー' {
        return false;
    }
    match rest.last() {
        Some(&c) => !japanese_letter(c) && c != 'ー',
        None => char_before.map_or(true, |c| !japanese_letter(c) && c != 'ー'),
    }
}

fn apply_character_form(text: &str) -> String {
    apply_character_form_with(text, symbol_forms())
}

/// 2026-10-20: 記号の全角・半角の設定 (アプリを開いたときに 1 回読む)
struct SymbolForms {
    /// 全角にする記号 (半角の字)
    full: Vec<char>,
    /// ¥ キーで ¥ を入れる (false なら \\)
    yen: bool,
}

fn symbol_forms() -> &'static SymbolForms {
    static FORMS: std::sync::OnceLock<SymbolForms> = std::sync::OnceLock::new();
    FORMS.get_or_init(|| {
        // 試験は以前の決まった形 (！？～ だけ全角・¥ キーは \\) で
        if cfg!(test) {
            return SymbolForms { full: vec!['!', '?', '~'], yen: false };
        }
        let input = shared::AppConfig::read_or_default().input;
        SymbolForms {
            full: input.fullwidth_symbols.chars().filter(|c| shared::CONFIGURABLE_SYMBOLS.contains(*c)).collect(),
            yen: input.yen_key != "backslash",
        }
    })
}

fn apply_character_form_with(text: &str, forms: &SymbolForms) -> String {
    let full = |c: char| forms.full.contains(&c);
    text.chars()
        .map(|c| match c {
            '＼' => if full('\\') { '＼' } else { '\\' },
            '￥' => match (forms.yen, full('\\')) {
                (true, true) => '￥',
                (true, false) => '¥',
                (false, true) => '＼',
                (false, false) => '\\',
            },
            '”' | '“' => if full('"') { c } else { '"' },
            '’' | '‘' => if full('\'') { c } else { '\'' },
            '\u{FF01}'..='\u{FF5E}' => {
                let half = char::from_u32(c as u32 - 0xFEE0).unwrap_or(c);
                if half.is_ascii_alphanumeric() || !full(half) { half } else { c }
            }
            _ => c,
        })
        .collect()
}

/// 2026-10-14: 変換中のカーソルが全体の右端にあるか (←→ で動かしていない・最後の文節にいる)。右端なら文字キーは確定して次の入力
fn convert_cursor_at_end(comp: &Composition) -> bool {
    !comp.cursor_moved || comp.focus + 1 >= comp.segments.len()
}

/// 2026-10-25: 変換中に数字を打ったとき、確定せずに入力に戻して読みの最後の 。 、 を小数点・桁区切りに直すか
/// (2。 を Space で変換してから 5 と打った。前は 2。 を確定して 2。5 になっていた)。変換のカーソルが右端のときだけ
fn number_continues_conversion(comp: &Composition) -> bool {
    super::auto_english::number_punctuation() && convert_cursor_at_end(comp) && number_mark_fix(&comp.hiragana, comp.char_before).is_some()
}

/// 2026-11-06 (設定 input.smart_punctuation): 半角英数の -- → — を使うか (初期値オフ)
fn smart_punctuation_enabled() -> bool {
    #[cfg(test)]
    if let Some(on) = SMART_PUNCTUATION_FOR_TESTS.with(|cell| cell.get()) {
        return on;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !cfg!(test) && shared::AppConfig::read_or_default().input.smart_punctuation)
}

#[cfg(test)]
thread_local! {
    static SMART_PUNCTUATION_FOR_TESTS: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// 2026-11-06 (仕事 34): iOS と同じ入力の補助のうち、smart_punctuation のほかの切り替え (どれも初期値オフ)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IosAssist {
    Period,
    Capital,
}

fn ios_assist(which: IosAssist) -> bool {
    #[cfg(test)]
    if let Some(on) = SMART_PUNCTUATION_FOR_TESTS.with(|cell| cell.get()) {
        return on;
    }
    static ON: std::sync::OnceLock<(bool, bool)> = std::sync::OnceLock::new();
    let (period, capital) = *ON.get_or_init(|| {
        if cfg!(test) { return (false, false); }
        let input = shared::AppConfig::read_or_default().input;
        (input.period_shortcut, input.auto_capitalize)
    });
    match which {
        IosAssist::Period => period,
        IosAssist::Capital => capital,
    }
}

/// 引用符の向き (iOS と同じ): 直前が無い・空白・開き括弧・開きの引用符なら開き (“ ‘)、ほか (語のあと: don't の ') は閉じ (” ’)
fn smart_quote(c: char, before: Option<&str>) -> char {
    let open = before.and_then(|b| b.chars().last()).is_none_or(|p| p.is_whitespace() || "([{\u{201C}\u{2018}<".contains(p));
    match (c, open) {
        ('"', true) => '\u{201C}',
        ('"', false) => '\u{201D}',
        (_, true) => '\u{2018}',
        (_, false) => '\u{2019}',
    }
}

/// Space 2 回のピリオドにする形か: 直前が空白 1 つで、その前が英字か数字
fn period_shortcut_follows(before: &str) -> bool {
    let chars: Vec<char> = before.chars().collect();
    matches!(chars.as_slice(), [.., p, ' '] if p.is_alphanumeric())
}

/// 文の頭か: 直前が . ? ! と空白 (1 つ以上) か、改行。欄の頭 (直前が無い) は、名前・メールの欄のことがあるので大文字にしない
fn sentence_starts_after(before: &str) -> bool {
    if before.ends_with('\n') || before.ends_with('\r') { return true; }
    let trimmed = before.trim_end_matches(' ');
    trimmed.len() < before.len() && trimmed.chars().last().is_some_and(|c| ".?!".contains(c))
}

/// キャレットの直前 (2 字まで) が、- を打ったら — にする形か: 最後が - で、その前が - でない (--- は —- のまま。iOS と同じ)
fn smart_dash_follows(before: &str) -> bool {
    before.ends_with('-') && !before.ends_with("--")
}

/// 2026-10-28 (試験中、設定 input.punctuation_convert): 。、？！ を打ったら、そこまでを変換するか (ATOK の句読点変換。、 も同じ扱い)。
/// 確定は次の字か Enter (変換中に字を打つと確定して次の入力になる) なので、アプリから確定した字を読み戻さずに済み、Enter の回数も今と同じ。
/// 読みの終わりで打ったときだけ。数字・英字の後ろの . , ? ! は小数点・桁区切り・英文の記号のことがある (2.5・1,000・github.com) ので変換しない
fn punctuation_converts(comp: &Composition, c: char, enabled: bool) -> bool {
    // 2026-11-05 (仕事 28 の 1): . をいつも半角にする設定では、. は句点ではないので変換しない
    let period_is_kuten = !(c == '.' && super::auto_english::period_key() == super::auto_english::KeyForm::Latin);
    let last = comp.hiragana.chars().last();
    let after_digit = last.is_some_and(|d| d.is_ascii_digit() || ('０'..='９').contains(&d));
    let after_latin = last.is_some_and(|d| d.is_ascii_alphabetic());
    // 2026-11-08: 英字で終わる入力のあとは変換しない (ver2. ・github. ・英文の , )。ただし入力にかな・漢字がある日本語の文の中の英単語のあとの 、
    // (…IME、・…bilingual、) は変換する (変換しないと、、 のあとも同じ入力が続いていた)。数字のあとの , (桁区切り) と . ? ! は今までどおり
    let japanese_sentence = comp.hiragana.chars().any(|d| ('ぁ'..='ゖ').contains(&d) || ('ァ'..='ヺ').contains(&d) || ('一'..='鿿').contains(&d));
    enabled && period_is_kuten && matches!(c, '.' | ',' | '?' | '!') && !comp.hiragana.is_empty() && comp.caret == comp.hiragana_len()
        && !after_digit && (!after_latin || (c == ',' && japanese_sentence))
}

/// 2026-10-28 (試験中): 母音を補った読みの候補から、直すものを選ぶ (scores[0] は補う前の読み、残りは補った読み。(辞書の点数, 読みだけの AI の点数))。
/// 読みだけの AI の点数が全部あれば、1 位が補った読みで、2 位 (補う前の読みを含む) より 4 以上よいとき。AI の点数が無ければ辞書の点数で 6 以上
/// (10-28 の測り: 母音を 1 つ抜いた打ち間違いで、直す候補の 58〜63% が正しく直り、違う母音は 1〜2%。日本語 3,660 行 ×2 では直す候補が 0 件)
fn pick_repair(scores: &[(Option<f32>, Option<f32>)]) -> Option<usize> {
    pick_repair_with(scores, 4.0, Some(6.0))
}

/// 2026-11-04 (仕事 20): 英字の区切りの中の母音の抜け (watsh + iha) の直しを選ぶ。打ったとおりが英単語や名前のこともあるので、
/// 読みだけの AI の点数がそろっていて、1 位が補った読みで 2 位より english_repair_margin 以上よいときだけ (辞書の点数だけでは直さない)
fn pick_english_repair(scores: &[(Option<f32>, Option<f32>)]) -> Option<usize> {
    pick_repair_with(scores, super::auto_english::english_repair_margin(), None)
}

/// 補った読みの 1 位が、2 位 (補う前の読みを含む) より、読みだけの AI の点数で lm_margin 以上 (AI の点数が無ければ辞書の点数で dict_margin 以上) よいとき、その番号
fn pick_repair_with(scores: &[(Option<f32>, Option<f32>)], lm_margin: f32, dict_margin: Option<f32>) -> Option<usize> {
    let lm: Option<Vec<f32>> = scores.iter().map(|s| s.1).collect();
    let dict: Option<Vec<f32>> = scores.iter().map(|s| s.0).collect();
    let (values, margin) = match (lm, dict, dict_margin) {
        (Some(values), _, _) => (values, lm_margin),
        (None, Some(values), Some(margin)) => (values, margin),
        _ => return None,
    };
    let best = (1..values.len()).max_by(|&a, &b| values[a].total_cmp(&values[b]))?;
    let second = (0..values.len()).filter(|&i| i != best).map(|i| values[i]).fold(f32::NEG_INFINITY, f32::max);
    (values[best] - second >= margin).then_some(best)
}

/// 2026-10-30: 母音の多すぎの採点から、抜く打鍵の列を選ぶ (scores[0] は抜く前の読み、残りは 1 つ抜いた読み。(辞書の点数, 読みだけの AI の点数))。
/// margins[k] は scores[k + 1] を選ぶのに要る差 (excess_margin)。読みだけの AI の点数がそろっていて、いちばんよいものが抜く前よりその差以上よいとき。
/// AI の点数が無ければ直さない (辞書の点数は長音にも弱い)
fn pick_excess(scores: &[(Option<f32>, Option<f32>)], margins: &[f32]) -> Option<usize> {
    let values: Vec<f32> = scores.iter().map(|s| s.1).collect::<Option<_>>()?;
    let best = (1..values.len()).max_by(|&a, &b| values[a].total_cmp(&values[b]))?;
    (values[best] - values[0] >= *margins.get(best - 1)?).then_some(best)
}

/// 2026-10-30: 打った列の at の母音を抜くのに要る、読みだけの AI の点数の差。同じ母音が 3 つ続く所 (motsuuutsukushii の uuu) は打ち間違いの
/// しるしが強いので 10、同じ母音を 2 つ重ねた所 (tsu + u・sa + a) はカタカナ語の長音を母音で打った形 (saaba = サーバー) と見分けにくいので 14、
/// 違う母音を挟んだ所 (ka + e + ze) は 12。例文の測り (10-30) の、日本語 3,660 行で差 10 にすると 72 件直しすぎ、そのうち 53 件が 2 つ重ねた所だった。
/// この決まりで: 英語の判別が正しい例文で、正しく直る 同じ母音を重ねた形 約 40%・違う母音を挟んだ形 58%、違う直し 0.1%。正しく打った例文 1,421 文で
/// 直しすぎ 0 件、日本語 3,660 行で 15 件ほど (0.4%。半分ほどはもとの行の打ち間違いを直したもの、残りは メンバー＋集まって のような長音の後ろの同じ母音)
fn excess_margin(typed: &str, at: usize) -> f32 {
    let b = typed.as_bytes();
    let Some(&vowel) = b.get(at) else { return f32::INFINITY };
    let left = b[..at].iter().rev().take_while(|&&c| c == vowel).count();
    let right = b[at + 1..].iter().take_while(|&&c| c == vowel).count();
    match left + right + 1 {
        n if n >= 3 => 10.0,
        2 => 14.0,
        _ => 12.0,
    }
}

/// 2026-10-30: 母音を 1 つ抜いた所 (抜く前の読みの位置と、抜いたかな)。候補の「打ったとおり」はそのかなを戻したもの
/// (RepairedSpot の kana は空、typed に戻すかなを入れる)。違いがかな 1 字を抜いただけでなければ None
fn removed_spot(before: &str, after: &str) -> Option<RepairedSpot> {
    let (b, a): (Vec<char>, Vec<char>) = (before.chars().collect(), after.chars().collect());
    let prefix = b.iter().zip(&a).take_while(|(x, y)| x == y).count();
    let suffix = b[prefix..].iter().rev().zip(a[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    (b.len() == a.len() + 1 && prefix + suffix >= a.len()).then(|| RepairedSpot { offset: prefix, kana: String::new(), typed: b[prefix].to_string() })
}

/// 2026-10-28: 直す前と後の読みの違う所 (子音 1 字 → 補ったかな)。違いがそれだけでなければ None
/// 2026-11-04 (仕事 20): 英字の区切りの中の直しでは、直す前の英字の並びがかなになる (あshitno → あしたの)。直す前の違う所が小文字の英字だけなら、何字でも
fn repaired_spot(before: &str, after: &str) -> Option<RepairedSpot> {
    let (b, a): (Vec<char>, Vec<char>) = (before.chars().collect(), after.chars().collect());
    let prefix = b.iter().zip(&a).take_while(|(x, y)| x == y).count();
    let suffix = b[prefix..].iter().rev().zip(a[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    let typed: String = b[prefix..b.len() - suffix].iter().collect();
    (!typed.is_empty() && a.len() > prefix + suffix && typed.chars().all(|c| c.is_ascii_lowercase()))
        .then(|| RepairedSpot { offset: prefix, kana: a[prefix..a.len() - suffix].iter().collect(), typed })
}

/// 2026-11-06 (仕事 20 の手 1): 補った読み (scores[1..]) のうち、読みだけの AI の点数が補う前の読み (scores[0]) より高いものの番号、点の高い順。
/// AI の点数がそろっていなければ無し
fn typo_suggestion_order(scores: &[(Option<f32>, Option<f32>)]) -> Vec<usize> {
    let Some(values) = scores.iter().map(|s| s.1).collect::<Option<Vec<f32>>>() else { return vec![] };
    let mut order: Vec<usize> = (1..values.len()).filter(|&k| values[k] > values[0]).collect();
    order.sort_by(|&a, &b| values[b].total_cmp(&values[a]));
    order
}

/// 2026-11-06 (仕事 20 の手 1): 文節の読み (頭の位置 start) の中に直しの候補の所 spot があれば、その所を補ったかなにした読み
fn suggested_segment_reading(reading: &str, start: usize, spot: &RepairedSpot) -> Option<String> {
    let chars: Vec<char> = reading.chars().collect();
    let typed: Vec<char> = spot.typed.chars().collect();
    let off = spot.offset.checked_sub(start)?;
    (off + typed.len() <= chars.len() && chars[off..off + typed.len()] == typed[..]).then(|| {
        chars[..off].iter().collect::<String>() + &spot.kana + &chars[off + typed.len()..].iter().collect::<String>()
    })
}

/// 2026-11-06 (仕事 20 の手 2): 文節の読みに母音を 1 字足した読み (かぢ → かだい)。読みをローマ字に直し、子音のすぐ後ろに母音を 1 つ足して、
/// ローマ字の表でかなに戻せるものだけ (抜けても読めるローマ字になった所: kadai を kadi と打って かぢ)。同じ子音の重ね (っ) の前と、
/// ん の n には足さない。元の読みと同じものは除き、多くても max まで
fn one_vowel_readings(reading: &str, spelling: &super::romaji_style::Spelling, max: usize) -> Vec<String> {
    let romaji = to_romaji(reading, spelling);
    if romaji.is_empty() || !romaji.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        return vec![];
    }
    let bytes = romaji.as_bytes();
    let mut out: Vec<String> = vec![];
    for i in 0..bytes.len() {
        let c = bytes[i];
        if !c.is_ascii_lowercase() || b"aeiou".contains(&c) || bytes.get(i + 1) == Some(&c) {
            continue;
        }
        if c == b'n' && bytes.get(i + 1).is_none_or(|next| !b"aeiouy".contains(next)) {
            continue;
        }
        for v in ['a', 'i', 'u', 'e', 'o'] {
            let candidate = format!("{}{v}{}", &romaji[..=i], &romaji[i + 1..]);
            let units = super::romaji_style::simulate(&candidate);
            if units.is_empty() || units.iter().any(|u| u.pending) {
                continue;
            }
            let kana: String = units.iter().map(|u| u.kana.as_str()).collect();
            if kana != reading && !out.contains(&kana) {
                out.push(kana);
                if out.len() >= max {
                    return out;
                }
            }
        }
    }
    out
}

/// 2026-11-06 (仕事 20 の手 2): 母音を 1 字足した読みの採点 (scores[0] は元の読み) から、一覧に出すものを選ぶ。読みだけの AI の点数が元より
/// margin 以上よく、辞書の点数も元よりよいもの、AI の点の高い順に 2 つまで
fn pick_vowel_lookups(scores: &[(Option<f32>, Option<f32>)], margin: f32) -> Vec<usize> {
    let Some(&(Some(d0), Some(l0))) = scores.first() else { return vec![] };
    let mut order: Vec<usize> = (1..scores.len())
        .filter(|&k| matches!(scores[k], (Some(d), Some(l)) if l - l0 >= margin && d > d0))
        .collect();
    order.sort_by(|&a, &b| scores[b].1.unwrap_or(f32::MIN).total_cmp(&scores[a].1.unwrap_or(f32::MIN)));
    order.truncate(2);
    order
}

/// 手 2 で一覧に出すのに要る、読みだけの AI の点数の差 (B の組の dev で決め直す。11-06 は仮の値)
const VOWEL_LOOKUP_MARGIN: f32 = 6.0;

/// 2026-10-28: 直した文節の「打ったとおり」(読みの補ったかなを、もとの子音に戻したもの)。start はその文節の読みの頭の位置 (字数)
fn unrepaired_reading(reading: &str, start: usize, spots: &[RepairedSpot]) -> Option<String> {
    let chars: Vec<char> = reading.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    let mut changed = false;
    for spot in spots {
        let kana: Vec<char> = spot.kana.chars().collect();
        let Some(off) = spot.offset.checked_sub(start) else { continue };
        if off < at || off + kana.len() > chars.len() || chars[off..off + kana.len()] != kana[..] { continue; }
        out.extend(&chars[at..off]);
        out.push_str(&spot.typed);
        at = off + kana.len();
        changed = true;
    }
    out.extend(&chars[at..]);
    changed.then_some(out)
}

fn utf16_len(s: &str) -> i32 {
    s.encode_utf16().count() as i32
}

/// 端末で 1 字が占めるマス数 (東アジアの全角は 2、異体字セレクタなど幅のない字は 0)。候補ウィンドウを端末で注目文節に合わせるのに使う
fn terminal_cells(c: char) -> i32 {
    match c as u32 {
        0x0300..=0x036F | 0x200B..=0x200F | 0x3099..=0x309A | 0xFE00..=0xFE0F | 0xE0100..=0xE01EF => 0,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

fn is_latin_letter(c: char) -> bool {
    c.is_alphabetic() && (c.is_ascii() || ('\u{00c0}'..='\u{024f}').contains(&c))
}

fn literal_latin_segment(segment: &Segment) -> bool {
    let latin = |text: &str| !text.is_empty() && text.chars().all(is_latin_letter);
    latin(&segment.reading) && latin(&segment.text)
        && segment.reading.to_lowercase() == segment.text.to_lowercase()
}

fn cycle_focused_ivs(comp: &mut Composition) -> bool {
    let Some(segment) = comp.segments.get_mut(comp.focus) else { return false };
    let Some((choices, next)) = super::ivs::next_choices(&segment.text) else { return false };
    segment.text = choices[next].clone();
    segment.candidates = choices;
    segment.index = next as i32;
    segment.kana_cycle.clear();
    comp.window_open = true;
    comp.latin_selected = false;
    comp.latin_learning_intent = false;
    comp.refined = true;
    comp.segments_reading = composition_reading(comp);
    if comp.ivs_original.is_none() { comp.from_prediction = false; }
    true
}

/// 読みを文節ごとの長さで切り分けて Segment にする
fn build_segments(reading: &str, converted: Vec<(String, i32)>) -> Vec<Segment> {
    let output: String = converted.iter().map(|(text, _)| text.as_str()).collect();
    if !reading.is_empty() && reading.chars().all(is_latin_letter)
        && !output.is_empty() && output.chars().all(is_latin_letter)
        && reading.to_lowercase() != output.to_lowercase() {
        return vec![Segment { reading: reading.into(), text: reading.into(), ..Default::default() }];
    }
    let chars: Vec<char> = reading.chars().collect();
    let total: i32 = converted.iter().map(|(_, len)| *len).sum();
    // 2026-11-23 (セキュリティの点検 F-3): 長さが 1 より小さい文節があれば、足して合っても切り分けない (負の長さで chars[start..end] が落ちていた)。
    // 変換サーバーの返事は ipc_service の checked_segments で確かめてあるが、ここでも受ける
    if converted.is_empty() || total != chars.len() as i32 || converted.iter().any(|(_, len)| *len < 1) {
        // 2026-11-23 (Astra の点検 06): 文節が 1 つで長さが合わないときも、その表記は使わない (前は読み全体に置いていた)
        let text = reading.to_string();
        return vec![Segment {
            reading: reading.to_string(),
            text,
            candidates: vec![],
            index: 0,
            kana_cycle: vec![],
            windows_pending: false,
            notes: vec![],
            ai_text: None,
            typed_latin: None,
            repair_readings: vec![],
            patchwork: false,
        }];
    }

    let mut start = 0usize;
    let mut segments: Vec<Segment> = vec![];
    for (text, len) in converted {
        let end = start + len as usize;
        let segment = Segment {
            reading: chars[start..end].iter().collect(),
            text,
            candidates: vec![],
            index: 0,
            kana_cycle: vec![],
            windows_pending: false,
            notes: vec![],
            ai_text: None,
            typed_latin: None,
            repair_readings: vec![],
            patchwork: false,
        };
        start = end;
        // Keep literal words such as ni / n-tilde / o in one selectable clause.
        if let Some(previous) = segments.last_mut() {
            if literal_latin_segment(previous) && literal_latin_segment(&segment) {
                previous.reading += &segment.reading;
                previous.text += &segment.text;
                continue;
            }
        }
        segments.push(segment);
    }
    segments
}

/// 2026-11-19 (まとめ役の README の下書きの確かめの 5): F6・F7 で、読みの中の英字の並び (英字と判定した所) を、ローマ字として読めるときだけ かな にする
/// (無変換キーと同じ latin_kana_reading の決まり)。読めない綴り (google) は英字のまま。Microsoft IME の F6 は「全部ひらがな」なので、そう思って押した人と、
/// 無変換キーの無い US 配列の人のため (前は英字の所が英字のまま残った)
fn readable_latin_as_kana(reading: &str) -> String {
    fn flush(run: &mut String, out: &mut String) {
        if !run.is_empty() {
            out.push_str(&latin_kana_reading(run).unwrap_or_else(|| run.clone()));
            run.clear();
        }
    }
    let (mut out, mut run) = (String::with_capacity(reading.len()), String::new());
    for c in reading.chars() {
        if c.is_ascii_alphabetic() {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

fn type_text(set_type: SetTextType, reading: &str, raw_input: Option<&str>) -> String {
    match set_type {
        SetTextType::Hiragana => readable_latin_as_kana(reading),
        SetTextType::Katakana => to_katakana(&readable_latin_as_kana(reading)),
        SetTextType::HalfKatakana => to_half_katakana(reading),
        SetTextType::FullLatin => match raw_input {
            Some(raw) => to_fullwidth(raw, true),
            None => reading.to_string(),
        },
        SetTextType::HalfLatin => match raw_input {
            Some(raw) => to_halfwidth(raw),
            None => reading.to_string(),
        },
    }
}

impl ITfCompositionSink_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    fn OnCompositionTerminated(
        &self,
        _ecwrite: u32,
        _pcomposition: Option<&ITfComposition>,
    ) -> Result<()> {
        // if user clicked outside the composition, the composition will be terminated
        tracing::debug!("OnCompositionTerminated");
        // 2026-11-08 (メモ帳で確定した文が消えた件): IME 本体の編集のセッションの中で、アプリがその場でコンポジションを終えたときは、
        // ここで確定し直さない (外側の編集の途中で、確定が「RefCell already borrowed」で失敗し、未確定の状態も片付けられなかった)。
        // 印だけ立て、片付けは次のキーの頭 (take_composition_lost → forget_composition) でする
        if crate::tsf::edit_session::in_edit_session() {
            crate::tsf::edit_session::mark_composition_lost("the app ended the composition during our edit session");
            return Ok(());
        }
        // サーバーが返事をしない間の打ちかけ (未確定のローマ字) は、アプリが入力を終えたら打ったまま文書に残る。IME 本体の打ちかけも捨てる
        // 2026-11-06 (端末の件): アプリが終えたのが IME 本体の持っているコンポジションなら、先に手放す (終わったコンポジションに字を書かない)。
        // 確定の動作は、見えているものを覚えて入力中でなくするだけになる
        if let Ok(text_service) = self.borrow() {
            if let Ok(mut composition) = text_service.borrow_mut_composition() {
                composition.local_romaji.clear();
                let ours = composition.tip_composition.is_some()
                    && (_pcomposition.is_none() || _pcomposition == composition.tip_composition.as_ref());
                crate::debug_log::write(&format!("composition terminated by the app (state={:?}, ours={ours})", composition.state));
                if ours {
                    composition.tip_composition = None;
                }
            }
        }

        let actions = vec![ClientAction::EndComposition];
        if let Err(error) = self.handle_action(&actions, CompositionState::None) {
            crate::debug_log::write(&format!("composition terminated: commit failed: {error:?}"));
            self.forget_composition();
        }

        Ok(())
    }
}

impl TextServiceFactory {
    /// キーをどう扱うかを決める (副作用なし。OnTestKeyDown からも呼ばれる)
    #[tracing::instrument]
    pub fn process_key(
        &self,
        context: Option<&ITfContext>,
        wparam: WPARAM,
    ) -> Result<Option<(Vec<ClientAction>, CompositionState)>> {
        use ClientAction as A;
        use CompositionState as S;

        if context.is_none() {
            return Ok(None);
        };

        // 2026-11-08 (仕事 53): 入力モードのキー (Alt+` と VK_KANJI の保ったキー・VK_IME_ON/OFF・設定の Ctrl+Space) は、Alt・Ctrl を見る前に
        // 半角/全角 (0xF3) に読み替える (打ちかけがあるときも 半角/全角 と同じ扱い。mode_keys.rs)
        let mode_key = super::mode_keys::mode_key(wparam.0, VK_CONTROL.is_pressed(), VK_SHIFT.is_pressed());
        let wparam = if mode_key.is_some() { WPARAM(super::mode_keys::VK_HALF_FULL) } else { wparam };
        let ctrl = mode_key.is_none() && VK_CONTROL.is_pressed();
        if mode_key.is_none() && (VK_MENU.is_pressed() || VK_LWIN.is_pressed() || VK_RWIN.is_pressed()) {
            return Ok(None);
        }

        #[allow(clippy::let_and_return)]
        let (composition, mode) = {
            let text_service = self.borrow()?;
            let composition = text_service.borrow_composition()?.clone();
            let mode = IMEState::get()?.input_mode.clone();
            (composition, mode)
        };
        // 2026-11-06 (端末の件): 未確定の範囲を失った入力・コンポジションを持たない入力中は、入力中でないとして決める (handle_key が空にしてから扱う)
        let composition = if composition_needs_reset(&composition) { Composition::default() } else { composition };
        // 2026-11-08 (仕事 53): すでにそのモードの VK_IME_ON/OFF と、保ったキーで切り替えた同じ打鍵は、食べるだけ
        match &mode_key {
            Some(super::mode_keys::ModeKey::AlreadyHandled) => return Ok(Some((vec![], composition.state.clone()))),
            Some(super::mode_keys::ModeKey::To(target)) if *target == mode => return Ok(Some((vec![], composition.state.clone()))),
            _ => {}
        }
        // 2026-10-14: サーバーが返事をしない間は、入力中でなければキーをサーバーに聞かない。
        // 飲み込むと答えたキーは、WPF などではあとから返してもアプリに届かず消えるので、受けるかどうかはここで決めきる。
        // 2026-10-22: かなの入力なら、IME 本体のローマ字の表でひらがなにして入れる (local_kana_step)。打ちかけが残っている間は、サーバーが戻っても続ける
        if composition.state == S::None && (super::ipc_service::server_unresponsive() || !composition.live_local_romaji().is_empty()) {
            // 2026-10-29: 入力モードの切り替え (半角/全角) は、この間もする (handle_key と同じ判断。レビュー L6)
            if let Some(mode) = local_mode_toggle(wparam, ctrl, &mode) {
                return Ok(Some((vec![A::SetIMEMode(mode)], S::None)));
            }
            return Ok(self.local_kana_key(context, wparam, ctrl, mode == InputMode::Kana, &composition)
                .filter(|key| local_kana_step(composition.live_local_romaji(), *key).eaten)
                .map(|_| (vec![], S::None)));
        }

        let shift = VK_SHIFT.is_pressed();
        // 2026-10-14: 数字・電話・パスワード・PIN などの欄 (InputScope) と、ブラウザの拡張機能の小窓での数字は、
        // 入力中でなければ IME を通さずアプリへ渡す (direct_field.rs)。その欄で半角/全角を押したら普通に入力する
        if composition.state == S::None && !ctrl {
            match self.direct_field_decision(context, wparam, mode == InputMode::Kana, shift) {
                super::direct_field::Decision::Pass => return Ok(None),
                super::direct_field::Decision::EnableKana => {
                    return Ok(Some((vec![A::SetIMEMode(InputMode::Kana)], S::None)));
                }
                super::direct_field::Decision::Normal => {}
            }
        }
        if super::ivs::shortcut(wparam.0, ctrl, shift) && composition.state == S::None {
            return Ok(context.filter(|context| self.has_selected_ivs_target(context))
                .map(|_| (vec![A::CycleIvs], S::None)));
        }
        // SendInput で送った Unicode の文字 (長押しで選んだ ñ など) はそのままアプリへ
        if wparam.0 == 0xE7 {
            return Ok(None);
        }
        // The Latin-mode replacement Backspace must not close its own picker.
        if wparam.0 == 0x08 && unsafe { windows::Win32::UI::WindowsAndMessaging::GetMessageExtraInfo() }.0 as usize == ACCENT_INPUT_MARKER {
            return Ok(None);
        }
        if let Ok(UserAction::Input(c)) = UserAction::try_from(wparam.0) {
            if super::diacritics::consumed_repeat(c) {
                return Ok(Some((vec![], composition.state.clone())));
            }
        }
        if composition.state == S::None && mode == InputMode::Kana && !ctrl
            && super::terminal_choices::pass_key(wparam.0) {
            return Ok(None);
        }
        // 長押しでアクセント付きの文字 (diacritics.rs)
        if super::diacritics::enabled() && !ctrl {
            if let Ok(action) = UserAction::try_from(wparam.0) {
                if let Some(base) = composition.accent {
                    match action {
                        UserAction::Number(n) if (1..=9).contains(&n) => {
                            return Ok(Some((vec![A::PickAccent(n as i32)], composition.state.clone())));
                        }
                        // 押しっぱなしの続きは捨てる
                        UserAction::Input(c) if c == base && super::diacritics::is_repeat() => return Ok(Some((vec![], composition.state.clone()))),
                        UserAction::Escape => return Ok(Some((vec![A::CloseAccents], composition.state.clone()))),
                        // ふつうの変換候補と同じ操作: ↓ Space Tab で次、↑ Shift+Space で前、Enter で決める
                        UserAction::Navigation(Navigation::Down) | UserAction::Tab if !shift => {
                            return Ok(Some((vec![A::MoveAccent(1)], composition.state.clone())));
                        }
                        UserAction::Space | UserAction::Henkan if !shift => {
                            return Ok(Some((vec![A::MoveAccent(1)], composition.state.clone())));
                        }
                        UserAction::Navigation(Navigation::Up) | UserAction::Space | UserAction::Tab | UserAction::Henkan => {
                            return Ok(Some((vec![A::MoveAccent(-1)], composition.state.clone())));
                        }
                        // 選んでいなければ Enter はいつもどおり (候補は閉じる)
                        UserAction::Enter if composition.accent_index >= 0 => {
                            return Ok(Some((vec![A::PickCurrentAccent], composition.state.clone())));
                        }
                        UserAction::Unknown => {}
                        // ほかのキー: 候補を閉じて、そのキーはいつもどおり
                        _ => self.close_accents_now(),
                    }
                } else if super::diacritics::is_repeat() {
                    if let UserAction::Input(c) = action {
                        if accents_on_hold(&composition, &mode, c) {
                            return Ok(Some((vec![A::ShowAccents(c)], composition.state.clone())));
                        }
                    }
                }
                // 2026-11-05 (長押し): 長押しのアクセントを使う人は、英字のキーを押しっぱなしにしても同じ字を続けて入れない (あ と A の両方。
                // Mac の長押しと同じ。設定 input.letter_key_repeat で戻せる)。1 回目の繰り返しで長押しを決めるのは上のとおり。
                // 数字・記号・Backspace・矢印などは続けて入れる。端末・コードエディタ (vim の hjkl を押し続ける) でも続けて入れる
                if letter_repeat_suppressed(super::diacritics::is_repeat(), wparam.0) {
                    return Ok(Some((vec![], composition.state.clone())));
                }
            }
        }
        // 端末でシェルの入力待ちなら英数 (A) に切り替える。入力待ちでなくなったら あ に戻す (shell_prompt.rs)。
        // 切り替えるのは文字のキーを打ったとき。そのキーの文字は、切り替えたモードで入れる
        if composition.state == S::None && !ctrl {
            match UserAction::try_from(wparam.0) {
                Ok(UserAction::ToggleInputMode) => super::shell_prompt::note_manual_toggle(),
                Ok(action @ (UserAction::Input(_) | UserAction::Number(_))) => {
                    let ch = match action { UserAction::Input(c) => c, UserAction::Number(n) => (b'0' + n as u8) as char, _ => unreachable!() };
                    match super::shell_prompt::auto_switch(mode == InputMode::Kana) {
                        Some(true) => {
                            return Ok(Some((vec![A::SetIMEMode(InputMode::Latin), A::CommitText(ch.to_string())], S::None)));
                        }
                        Some(false) if ch.is_ascii_lowercase() || ch == '-' => {
                            // あ に戻して、このキーは IME に打ち直す (かなの入力として受け取る)
                            return Ok(Some((vec![A::SetIMEMode(InputMode::Kana), A::RetypeAsKana(0, ch.to_string())], S::None)));
                        }
                        Some(false) => {
                            return Ok(Some((vec![A::SetIMEMode(InputMode::Kana), A::CommitText(ch.to_string())], S::None)));
                        }
                        None => {}
                    }
                }
                _ => {}
            }
        }
        // TeX を書いているとき: 数式の中・コマンドの英字はそのままアプリへ。日本語の途中の $ は確定してから数式へ
        if mode == InputMode::Kana && !ctrl && super::tex::active() {
            if let Ok(action) = UserAction::try_from(wparam.0) {
                if composition.state == S::None {
                    if super::tex::decide_once(wparam.0, &action) == super::tex::Decision::Pass {
                        return Ok(None);
                    }
                } else if composition.state == S::Composing && matches!(action, UserAction::Input('$')) && !composition.hiragana.ends_with('\\') {
                    return Ok(Some((
                        vec![A::EndComposition, A::CommitText("$".to_string()), A::TexEnterMath],
                        S::None,
                    )));
                }
            }
        }
        // 2026-11-06 (設定 input.smart_punctuation、初期値オフ。iOS のスマート句読点と同じ): 半角英数で、キャレットの直前が - (その前は - でない) のときに
        // - を打ったら、直前の - を — にする (このキーは飲み込む)。端末・コードを書くアプリ (next_word の EXCLUDED_APPS) と、覚えないでと伝えている欄では働かせない。
        // オフのときは半角英数は今までどおり素通し
        if composition.state == S::None && mode == InputMode::Latin && !ctrl && !shift && smart_punctuation_enabled()
            && super::next_word::app_allows_latin_help() && !super::direct_field::private_field()
            && matches!(UserAction::try_from(wparam.0), Ok(UserAction::Input('-'))) {
            let before = context.and_then(|context| self.preceding_text_in(context, 2)).map(|(text, _)| text);
            if before.as_deref().is_some_and(smart_dash_follows) {
                return Ok(Some((vec![A::SmartDash], S::None)));
            }
        }
        // 2026-11-06 (仕事 34、どれも初期値オフ。端末・コードを書くアプリ・覚えないでと伝えている欄では働かせない):
        // ' と " は前の字で開き・閉じの引用符 (input.smart_punctuation)、空白のあとの Space は「. 」(input.period_shortcut)、
        // 文の終わりのあとの小文字は大文字 (input.auto_capitalize)
        if composition.state == S::None && mode == InputMode::Latin && !ctrl && (smart_punctuation_enabled() || ios_assist(IosAssist::Period) || ios_assist(IosAssist::Capital))
            && super::next_word::app_allows_latin_help() && !super::direct_field::private_field() {
            if let Ok(action) = UserAction::try_from(wparam.0) {
                let before = || context.and_then(|context| self.preceding_text_in(context, 3)).map(|(text, _)| text);
                match action {
                    UserAction::Input(c @ ('"' | '\'')) if smart_punctuation_enabled() => {
                        return Ok(Some((vec![A::CommitText(smart_quote(c, before().as_deref()).to_string())], S::None)));
                    }
                    UserAction::Space if !shift && ios_assist(IosAssist::Period) && before().as_deref().is_some_and(period_shortcut_follows) => {
                        return Ok(Some((vec![A::PeriodShortcut], S::None)));
                    }
                    UserAction::Input(c) if c.is_ascii_lowercase() && ios_assist(IosAssist::Capital) && before().as_deref().is_some_and(sentence_starts_after) => {
                        return Ok(Some((vec![A::CommitText(c.to_ascii_uppercase().to_string())], S::None)));
                    }
                    _ => {}
                }
            }
        }
        // 2026-10-19: 英数モードの次の語の候補。Tab で補う (候補を出しているときだけ飲み込む)。字のキーは素通しして、あとで候補を出し直す
        if composition.state == S::None && mode == InputMode::Latin && !ctrl {
            if let Ok(action) = UserAction::try_from(wparam.0) {
                if let Some(decision) = self.latin_next_word_key(context, &action, shift) {
                    return Ok(decision);
                }
            }
        }
        if composition.state == S::None {
            if let Ok(action) = UserAction::try_from(wparam.0) {
                super::latin_learn::observe_key(wparam.0, &action, mode == InputMode::Latin, || self.latin_field_private(context));
                // 改行・送信・欄の移動で英文の途中の扱いは終わり
                if matches!(action, UserAction::Enter | UserAction::Tab | UserAction::Escape | UserAction::Navigation(_)) || ctrl {
                    super::auto_english::set_english_sentence(false);
                }
            }
        }
        match self.direct_prefix_pass(context, wparam, &composition, &mode, ctrl) {
            super::prefix_direct::Decision::Pass => return Ok(None),
            decision @ super::prefix_direct::Decision::Retype { .. } => {
                // 2026-10-13: / のあとが日本語になった。このキーは飲み込み、渡した文字を消して打ち直す
                let (delete, text) = decision.retype_text().unwrap_or_default();
                return Ok(Some((vec![A::RetypeAsKana(delete, text)], S::None)));
            }
            super::prefix_direct::Decision::Normal => {}
        }
        if composition.state == S::None {
            if ctrl && wparam.0 == 0x08 && composition.last_commit.is_some() {
                return Ok(Some((vec![A::UndoCommit], S::Converting)));
            }
            // 2026-11-05 (仕事 24): つづりの自動修正で確定した直後の Backspace は、打ったとおりに戻す。
            // キャレットの前が直した字のままと読めるときだけ (読めないアプリ・カーソルを動かしたときは Backspace をアプリへ)
            if !ctrl && !shift && wparam.0 == 0x08 {
                if let Some((corrected, _, _)) = composition.last_autocorrect.as_ref() {
                    let n = corrected.encode_utf16().count() as i32 + 1;
                    if context.and_then(|context| self.preceding_text_in(context, n)).is_some_and(|(text, _)| text.ends_with(corrected.as_str())) {
                        return Ok(Some((vec![A::UndoAutocorrect], S::None)));
                    }
                }
            }
            // 確定の取り消しは確定直後だけ。ほかのキーを押したら忘れる
            // (修飾キーだけの押下では忘れない)
            if composition.last_commit.is_some() && !matches!(wparam.0, 0x10 | 0x11 | 0x12 | 0xA0..=0xA5)
                && (wparam.0 != 0x20 || ctrl) {
                if let Ok(text_service) = self.borrow() {
                    if let Ok(mut c) = text_service.borrow_mut_composition() {
                        let starts_input = mode == InputMode::Kana && !ctrl
                            && matches!(UserAction::try_from(wparam.0), Ok(UserAction::Input(_) | UserAction::Number(_)));
                        c.commit_tail = c.last_commit.as_ref().filter(|_| starts_input).and_then(|last| last.text.chars().last());
                        c.last_commit = None;
                        c.last_autocorrect = None;
                        c.last_phrase_runs.clear();
                    }
                }
            }
        }
        if ctrl {
            return Ok(Self::process_ctrl_key(&composition, wparam.0, shift));
        }
        let action = UserAction::try_from(wparam.0)?;
        crate::debug_log::write(&format!(
            "key vk=0x{:02X} action={:?} state={:?} mode={:?} repeat={} accents={}",
            wparam.0, action, composition.state, mode, super::diacritics::is_repeat(), super::diacritics::enabled()
        ));

        let function_type = |key: &Function| match key {
            Function::Six => SetTextType::Hiragana,
            Function::Seven => SetTextType::Katakana,
            Function::Eight => SetTextType::HalfKatakana,
            Function::Nine => SetTextType::FullLatin,
            Function::Ten => SetTextType::HalfLatin,
        };

        let (actions, transition) = match composition.state {
            S::None => match action {
                UserAction::Input(char) if mode == InputMode::Kana => (
                    vec![A::StartComposition, A::Insert(char.to_string())],
                    S::Composing,
                ),
                UserAction::Number(number) if mode == InputMode::Kana => (
                    vec![A::StartComposition, A::Insert(number.to_string())],
                    S::Composing,
                ),
                UserAction::Space if mode == InputMode::Kana => {
                    (vec![A::CommitSmartSpace { reverse: shift }], S::None)
                }
                UserAction::ToggleInputMode => (
                    vec![match mode {
                        InputMode::Kana => A::SetIMEMode(InputMode::Latin),
                        InputMode::Latin => A::SetIMEMode(InputMode::Kana),
                    }],
                    S::None,
                ),
                _ => return Ok(None),
            },

            S::Composing => match action {
                UserAction::Input(char) if punctuation_converts(&composition, char, super::auto_english::punctuation_convert()) => {
                    (vec![A::Insert(char.to_string()), A::PunctuationConvert], S::Converting)
                }
                UserAction::Input(char) => (vec![A::Insert(char.to_string())], S::Composing),
                UserAction::Number(number) => {
                    (vec![A::Insert(number.to_string())], S::Composing)
                }
                // 2026-10-28 (試験中): 直した母音の抜けを、変換を取り消した (Esc) あとでも Shift+Backspace で打ったとおりに戻す
                UserAction::Backspace if shift && composition.typo_repair.is_some() => (vec![A::UndoTypoRepair], S::Converting),
                UserAction::Backspace => {
                    if composition.caret == 0 {
                        (vec![], S::Composing)
                    } else if composition.hiragana_len() <= 1 {
                        (vec![A::Cancel], S::None)
                    } else {
                        (vec![A::DeleteBackward], S::Composing)
                    }
                }
                UserAction::Delete => {
                    if composition.caret >= composition.hiragana_len() {
                        (vec![], S::Composing)
                    } else if composition.hiragana_len() <= 1 {
                        (vec![A::Cancel], S::None)
                    } else {
                        (vec![A::DeleteForward], S::Composing)
                    }
                }
                UserAction::Enter => (vec![A::EndComposition], S::None),
                // 2026-11-10 (Fable の諮問 ux 1、まとめ役の決め): ライブ変換で表示が読み (かな) と違うときの Esc は、まず読みに戻す。もう一度 Esc で消す。
                // 表示が読みのまま・予測の候補の窓が出ているときは、今までどおり消す
                UserAction::Escape if Self::escape_shows_reading(&composition) => (vec![A::ShowReading], S::Composing),
                UserAction::Escape => (vec![A::Cancel], S::None),
                UserAction::Navigation(Navigation::Left) => (vec![A::MoveCaret(-1)], S::Composing),
                UserAction::Navigation(Navigation::Right) => (vec![A::MoveCaret(1)], S::Composing),
                UserAction::Home => (vec![A::CaretToEdge(false)], S::Composing),
                UserAction::End => (vec![A::CaretToEdge(true)], S::Composing),
                // 次の語の候補が出ていれば、Tab で一番上の候補に補う。続けて Tab で次の候補、Shift+Tab で前の候補に替える
                UserAction::Tab if Self::next_word_tab(&composition, shift).is_some() => {
                    (vec![A::CompleteNextWord(Self::next_word_tab(&composition, shift).unwrap())], S::Composing)
                }
                // 予測変換の候補が出ていれば Tab / Shift+Tab で選ぶ
                UserAction::Tab if !composition.predictions.is_empty() && composition.predicted_for == composition.hiragana => {
                    (vec![A::UsePrediction(!shift)], S::Converting)
                }
                // 英単語だけを打っているときの Space は、確定して半角空白 (英文を続けて打てるように。struck by …)。変換キーはいつも変換
                // (大文字の語を Shift を押したまま打ち、そのまま Space を押すことがあるので、Shift+Space も同じ)
                UserAction::Space if Self::english_only_at_boundary(&composition) => (
                    vec![A::CommitSuffix(" ".to_string()), A::EndComposition],
                    S::None,
                ),
                UserAction::Space | UserAction::Henkan | UserAction::Tab => {
                    (vec![A::Convert], S::Converting)
                }
                UserAction::Navigation(Navigation::Down) => {
                    (vec![A::CaretToEdge(true)], S::Composing)
                }
                UserAction::Muhenkan if shift => (
                    vec![A::ConvertAsOne, A::ConvertType(SetTextType::FullLatin)],
                    S::Converting,
                ),
                UserAction::Muhenkan => (vec![A::ConvertAsOne, A::SwitchKanaType], S::Converting),
                UserAction::Function(key) => {
                    (vec![A::ConvertAsOne, A::ConvertType(function_type(&key))], S::Converting)
                }
                UserAction::ToggleInputMode => (
                    vec![A::EndComposition, A::SetIMEMode(InputMode::Latin)],
                    S::None,
                ),
                UserAction::Navigation(Navigation::Up)
                | UserAction::PageUp
                | UserAction::PageDown => (vec![], S::Composing),
                _ => return Ok(None),
            },

            S::Converting => match action {
                // 2026-10-25: 数字のあとの 。 、 まで変換していた (2。 + Space) なら、数字を続けたときは確定せずに入力に戻す。
                // 。 、 は Insert で小数点・桁区切りに直る (2.5)。
                // 2026-10-28: 候補の窓が開いていても (ライブ変換の Space は窓を開く)、数字は候補を選ばずに数を続ける
                // (2。 + Space + 5 で 5 番目の候補を選んで 5 が消えていた。B の写経 10-28。0 は候補の番号でないので 1、 + 000 は通っていた)
                UserAction::Number(number) if mode == InputMode::Kana && number_continues_conversion(&composition) => (
                    vec![A::CancelConversion, A::Insert(number.to_string())],
                    S::Composing,
                ),
                UserAction::Number(number)
                    if composition.window_open && (1..=PAGE_SIZE as i8).contains(&number) =>
                {
                    let picks_marker = composition.segments.get(composition.focus).is_some_and(|s| {
                        let index = s.index / PAGE_SIZE * PAGE_SIZE + number as i32 - 1;
                        s.candidates.get(index as usize).is_some_and(|c| c == SINGLE_KANJI_MARKER)
                    });
                    if picks_marker {
                        (
                            vec![A::SelectNumber(number as i32), A::OpenSingleKanji],
                            S::Converting,
                        )
                    } else {
                        (
                            vec![A::SelectNumber(number as i32), A::EndComposition],
                            S::None,
                        )
                    }
                }
                UserAction::Enter
                    if composition.window_open
                        && composition.segments.get(composition.focus).is_some_and(|s| {
                            s.candidates.get(s.index as usize).is_some_and(|c| c == SINGLE_KANJI_MARKER)
                        }) =>
                {
                    (vec![A::OpenSingleKanji], S::Converting)
                }
                // 2026-10-14: カーソルを文節の間へ動かしていたら、そこに入れる (変換中のカーソルで編集できるように)。
                // 右端なら今までどおり確定して次の入力
                UserAction::Input(char) if !convert_cursor_at_end(&composition) => {
                    (vec![A::CommitBeforeCursor, A::Insert(char.to_string())], S::Composing)
                }
                UserAction::Number(number) if !convert_cursor_at_end(&composition) => {
                    (vec![A::CommitBeforeCursor, A::Insert(number.to_string())], S::Composing)
                }
                UserAction::Input(char) => (
                    vec![
                        A::EndComposition,
                        A::StartComposition,
                        A::Insert(char.to_string()),
                    ],
                    S::Composing,
                ),
                UserAction::Number(number) => (
                    vec![
                        A::EndComposition,
                        A::StartComposition,
                        A::Insert(number.to_string()),
                    ],
                    S::Composing,
                ),
                UserAction::Space if !shift && space_commits_selected_latin(&composition) => (
                    vec![A::CommitSuffix(" ".to_string()), A::EndComposition], S::None,
                ),
                UserAction::Space if shift => (vec![A::PrevCandidate], S::Converting),
                UserAction::Henkan if shift => (vec![A::PrevCandidate], S::Converting),
                UserAction::Navigation(Navigation::Down) if shift => {
                    (vec![A::NextPage], S::Converting)
                }
                UserAction::Navigation(Navigation::Up) if shift => {
                    (vec![A::PrevPage], S::Converting)
                }
                UserAction::Muhenkan if shift => (
                    vec![A::ConvertType(SetTextType::FullLatin)],
                    S::Converting,
                ),
                UserAction::Muhenkan => (vec![A::SwitchKanaType], S::Converting),
                UserAction::Tab if shift => (vec![A::PrevCandidate], S::Converting),
                UserAction::Space
                | UserAction::Henkan
                | UserAction::Tab
                | UserAction::Navigation(Navigation::Down) => {
                    (vec![A::NextCandidate], S::Converting)
                }
                UserAction::Navigation(Navigation::Up) => (vec![A::PrevCandidate], S::Converting),
                UserAction::PageDown => (vec![A::NextPage], S::Converting),
                UserAction::PageUp => (vec![A::PrevPage], S::Converting),
                UserAction::Enter => (vec![A::EndComposition], S::None),
                // 2026-10-28 (試験中): 母音の抜けを直した直後の Shift+Backspace は、打ったとおりに戻す (ATOK の 自動修復取消)
                UserAction::Backspace if shift && composition.typo_repair.is_some() => (vec![A::UndoTypoRepair], S::Converting),
                // 2026-10-28 (試験中): 。、？！ で変換した直後の Backspace は、変換を取り消して入力中に戻す (ATOK の 変換取消。次の Backspace で 。 を消す)
                UserAction::Backspace if composition.punctuation_converted => (vec![A::CancelConversion], S::Composing),
                // 2026-10-14: Backspace はカーソルの左の 1 字を消す。全体をかなに戻すのは Esc
                UserAction::Backspace => (vec![A::DeleteAtCursor], S::Converting),
                UserAction::Escape => {
                    (vec![A::CancelConversion], S::Composing)
                }
                UserAction::Navigation(Navigation::Left) if shift => {
                    (vec![A::Resize(-1)], S::Converting)
                }
                UserAction::Navigation(Navigation::Right) if shift => {
                    (vec![A::Resize(1)], S::Converting)
                }
                UserAction::Navigation(Navigation::Left) => (vec![A::MoveFocus(-1)], S::Converting),
                UserAction::Navigation(Navigation::Right) => (vec![A::MoveFocus(1)], S::Converting),
                UserAction::Home => (vec![A::FocusEdge(false)], S::Converting),
                UserAction::End => (vec![A::FocusEdge(true)], S::Converting),
                UserAction::Function(key) => {
                    (vec![A::ConvertType(function_type(&key))], S::Converting)
                }
                UserAction::ToggleInputMode => (
                    vec![A::EndComposition, A::SetIMEMode(InputMode::Latin)],
                    S::None,
                ),
                UserAction::Delete => (vec![A::CancelConversion], S::Composing),
                _ => return Ok(None),
            },
        };

        Ok(Some((actions, transition)))
    }

    /// IME を通さない欄か (direct_field.rs)。読むのは、入力中でないときの文字・数字・空白・半角/全角のキーだけ。
    /// 同じキーの OnTestKeyDown と OnKeyDown では 1 回だけ読む
    fn direct_field_decision(&self, context: Option<&ITfContext>, wparam: WPARAM, kana: bool, shift: bool) -> super::direct_field::Decision {
        use super::direct_field as df;
        let Ok(action) = UserAction::try_from(wparam.0) else {
            return df::Decision::Normal;
        };
        if !df::relevant(&action) || (!kana && !matches!(action, UserAction::ToggleInputMode)) {
            return df::Decision::Normal;
        }
        let Some(context) = context else {
            return df::Decision::Normal;
        };
        df::decide_once(wparam.0, || {
            let (field, field_id) = self.field_kind(context, "key");
            let decision = df::decide(&action, kana, shift, field, df::enabled_here(field_id));
            if decision == df::Decision::EnableKana {
                df::enable(field_id);
            }
            // 2026-11-06 (仕事 25): そのまま渡す欄では言語バーを A の絵に (半角/全角で戻したら あ)。保存しているモードは変えない
            if df::set_shown_latin(df::shows_latin(field, df::enabled_here(field_id))) {
                let _ = self.update_lang_bar();
            }
            crate::debug_log::write(&format!("direct field: {field:?} -> {decision:?}"));
            decision
        })
    }

    /// 2026-11-06 (仕事 25): 欄の種類 (direct_field.rs の Field) と欄の識別。キーの判定とフォーカスの移り (言語バーの絵) で使う。
    /// 開発の記録 (debug_log) に、欄の印 (InputScope・IME を使わない文脈か・小窓か) と判定を書く (打った中身・欄の名前は書かない)
    pub fn field_kind(&self, context: &ITfContext, when: &str) -> (super::direct_field::Field, super::direct_field::FieldId) {
        use super::direct_field as df;
        let hwnd = unsafe { context.GetActiveView().and_then(|view| view.GetWnd()) }
            .ok()
            .filter(|hwnd| !hwnd.0.is_null())
            .unwrap_or_else(|| unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() });
        let window = df::window_facts(hwnd);
        let disabled = df::context_disabled(context);
        let scopes = self.input_scopes_in(context);
        df::note_scopes(scopes.as_deref());
        let field_id = df::field_id(windows::core::Interface::as_raw(context) as usize, hwnd.0 as isize, scopes.as_deref());
        let popup = window.as_ref().is_some_and(|w| df::is_browser_popup(super::prefix_direct::host_exe(), w));
        // 2026-11-23 (Astra の点検 37): パスワード・PIN の欄は、ほかの指定より先に上書きできない Disabled (direct_field::field_of)
        let field = df::field_of(disabled, scopes.as_deref(), popup);
        crate::debug_log::write(&format!(
            "field ({when}): scopes={scopes:?} disabled={disabled} popup={popup} window={window:?} field={field:?}"
        ));
        (field, field_id)
    }

    /// 行頭・空白直後の `/` `$` `@` で始まる語なら、このキーをアプリへそのまま渡す (prefix_direct)
    fn direct_prefix_pass(
        &self,
        context: Option<&ITfContext>,
        wparam: WPARAM,
        composition: &Composition,
        mode: &InputMode,
        ctrl: bool,
    ) -> super::prefix_direct::Decision {
        use super::prefix_direct as pd;
        let prefixes = pd::prefixes();
        if prefixes.is_empty() || composition.state != CompositionState::None || *mode != InputMode::Kana || ctrl {
            pd::reset();
            return pd::Decision::Normal;
        }
        let decision = pd::decide_once(wparam.0, |state| {
            let Ok(action) = UserAction::try_from(wparam.0) else {
                return pd::Decision::Normal;
            };
            // 前の文字が読めないアプリでは、端末なら行頭とみなす (直前に IME で文字を確定したときは除く)
            let fallback = pd::host_is_terminal() && composition.last_commit.is_none();
            let decision = pd::step(
                state,
                &action,
                prefixes,
                || {
                    let preceding = context.map_or(pd::Preceding::Unknown, |c| self.preceding_in(c));
                    crate::debug_log::write(&format!(
                        "direct prefix: preceding={preceding:?} host={} fallback={fallback}",
                        pd::host_exe()
                    ));
                    preceding
                },
                fallback,
                super::slash_commands::stays_latin,
            );
            decision
        });
        decision
    }

    fn schedule_live_refine(&self) {
        cancel_live_refine();
        if let Ok(text_service) = self.borrow() {
            let tip = text_service.this.clone();
            LIVE_TARGET.with(|t| *t.borrow_mut() = tip);
        }
        arm_live_timer(false);
    }

    /// 2026-11-05 (仕事 27): 日本語の予測候補を候補窓に出す (英語の次の語の候補と同じく、一番上に Tab の印)。候補が無ければ窓を隠す
    /// 2026-11-10: 入力中の Esc で、消さずに読みに戻すか。ライブ変換で、今の表示 (清書・仮表示・カーソルの左右の変換) が読みと違い、
    /// まだ読みに戻していないときだけ。予測の候補の窓が出ているとき (show_prediction_window) は、今までどおり消す
    pub(crate) fn escape_shows_reading(comp: &Composition) -> bool {
        let prediction_window = !comp.predictions.is_empty() && comp.predicted_for == comp.hiragana;
        comp.live && !comp.live_reading && !comp.hiragana.is_empty() && comp.display != comp.hiragana && !prediction_window
    }

    fn show_prediction_window(&self, ipc: &mut IPCService, comp: &mut Composition) -> Result<()> {
        if comp.predictions.is_empty() {
            ipc.hide_window()?;
            return Ok(());
        }
        let failures = super::ipc_service::ui_failures();
        ipc.set_candidates(comp.predictions.iter().enumerate()
            .map(|(i, c)| if i == 0 { format!("{c}\u{1f}Tab") } else { format!("{c}\u{1f}") }).collect())?;
        ipc.set_selection(-1)?;
        self.update_pos()?;
        ipc.show_window()?;
        // 2026-10-29: 候補窓に頼めなかったら、見えていない予測の候補を Tab で使わない (再点検 R2)
        if super::ipc_service::ui_failures() != failures {
            comp.predictions.clear();
        }
        Ok(())
    }

    fn schedule_predict(&self) {
        cancel_predict();
        // 2026-11-05 (仕事 27): 予測候補を使わない設定 (初期値) では、タイマーも張らない
        if !super::predict::enabled() {
            return;
        }
        if let Ok(text_service) = self.borrow() {
            let tip = text_service.this.clone();
            LIVE_TARGET.with(|t| *t.borrow_mut() = tip);
        }
        arm_predict_timer(false);
    }

    /// 2026-10-19: 英数モードのキー。Some(判断) ならそのキーの扱いはここで決まり。None なら普通に続ける
    fn latin_next_word_key(&self, context: Option<&ITfContext>, action: &UserAction, shift: bool) -> Option<Option<(Vec<ClientAction>, CompositionState)>> {
        // パスワード欄など「覚えないで」と伝えている欄 (IS_PRIVATE) では出さない (打ちかけが候補窓に見えてしまう)
        if !super::next_word::latin_allowed() || super::direct_field::private_field() {
            return None;
        }
        match action {
            UserAction::Tab => {
                let state = super::next_word::latin_state()?;
                let n = state.list.len();
                // 候補窓が返事をしない (見えていない) とき・候補を出してはいけない欄のときは、Tab はアプリへ (Fable の再調査 L1・M1)
                if n == 0 || super::ipc_service::ui_unresponsive() || self.latin_field_blocked(context) {
                    hide_latin_suggestions();
                    return None;
                }
                // キャレットの前が候補を出したときのまま (打ちかけか、補った語) か確かめてから Tab を飲み込む
                let current = state.pick.and_then(|i| state.list.get(i).cloned()).unwrap_or_else(|| state.partial.clone());
                let before = context.and_then(|context| self.preceding_text_in(context, 60)).map(|(text, _)| text);
                let still = before.as_deref().and_then(super::next_word::split_before_caret).is_some_and(|(_, partial)| partial == current);
                if !still {
                    hide_latin_suggestions();
                    return None;
                }
                let index = match state.pick {
                    Some(i) => if shift { (i + n - 1) % n } else { (i + 1) % n },
                    None => if shift { n - 1 } else { 0 },
                };
                Some(Some((vec![ClientAction::CompleteLatinWord(index)], CompositionState::None)))
            }
            // 語の字・語中の '・Backspace はアプリへ渡し、入ったあとで候補を出し直す
            UserAction::Input(c) if c.is_alphabetic() || *c == '\'' => {
                self.schedule_latin_suggest();
                None
            }
            UserAction::Backspace => {
                self.schedule_latin_suggest();
                None
            }
            UserAction::Unknown => None,
            // ほかのキー (空白・記号・数字・Enter・矢印 など) は候補を閉じて、いつもどおり
            _ => {
                hide_latin_suggestions();
                None
            }
        }
    }

    fn schedule_latin_suggest(&self) {
        use windows::Win32::UI::WindowsAndMessaging::SetTimer;
        cancel_latin_suggest();
        if let Ok(text_service) = self.borrow() {
            let tip = text_service.this.clone();
            LIVE_TARGET.with(|t| *t.borrow_mut() = tip);
        }
        let id = unsafe { SetTimer(None, 0, LATIN_SUGGEST_DELAY_MS, Some(latin_timer_proc)) };
        LATIN_TIMER.with(|t| t.set(id));
    }

    /// 2026-10-19: 英数モードで打った字がアプリに入ったあとに呼ばれる。キャレットの前の打ちかけの語と、その前の語から
    /// 次の語の候補を出す (アプリの文字はそのまま。Tab で補う)。読めないアプリでは出さない
    pub fn show_latin_suggestions(&self) -> Result<()> {
        let mode = IMEState::get()?.input_mode.clone();
        let (idle, context) = {
            let text_service = self.borrow()?;
            let idle = text_service.borrow_composition()?.state == CompositionState::None;
            (idle, text_service.context::<ITfContext>().ok())
        };
        if mode != InputMode::Latin || !idle || !super::next_word::latin_allowed() || super::direct_field::private_field()
            || self.latin_field_blocked(context.as_ref()) {
            hide_latin_suggestions();
            return Ok(());
        }
        let before = context.and_then(|context| self.preceding_text_in(&context, 60)).map(|(text, _)| text);
        let Some((prev, partial)) = before.as_deref().and_then(super::next_word::split_before_caret) else {
            hide_latin_suggestions();
            return Ok(());
        };
        // Tab で補った語のままなら、一覧はそのまま
        if super::next_word::latin_state().is_some_and(|s| s.pick.and_then(|i| s.list.get(i)) == Some(&partial)) {
            return Ok(());
        }
        let list = super::next_word::suggest(&prev, &partial);
        if list.is_empty() {
            hide_latin_suggestions();
            return Ok(());
        }
        let state = super::next_word::LatinState { partial, list, pick: None };
        let Some(mut ipc) = IMEState::get()?.ipc_service.clone() else { return Ok(()) };
        // 2026-10-20 (Codex の再調査 L2): Tab で補えるのは、候補窓に表示を頼めたときだけ。頼む前に前の候補を消し、どこかで
        // 失敗したら (候補窓がエラーを返した・返事をしない) 候補を戻さずに窓を隠す (見えていない候補で Tab を飲み込まない)
        super::next_word::set_latin_state(None);
        let failures = super::ipc_service::ui_failures();
        let shown = (|| -> Result<()> {
            ipc.set_candidates(latin_candidate_list(&state))?;
            ipc.set_selection(-1)?;
            self.update_pos_at_selection();
            ipc.show_window()?;
            Ok(())
        })();
        // 2026-10-29: 頼んだうちのどれかが届かなかったときも (その場の上限で問い合わせなかったなど)、見えていない扱いにする (レビュー L3)
        if shown.is_ok() && !super::ipc_service::ui_unresponsive() && super::ipc_service::ui_failures() == failures {
            super::next_word::set_latin_state(Some(state));
        } else {
            let _ = ipc.hide_window();
        }
        Ok(())
    }

    /// 2026-10-20 (Fable の再調査 M1): 今の欄の種類 (InputScope) を読み、パスワード・PIN・数字・メール・URL・IS_PRIVATE なら true。
    /// 英数モードのキーは direct_field の判定 (かなモードだけ) を通らないので、候補を出す前・Tab の前にここで確かめる
    fn latin_field_blocked(&self, context: Option<&ITfContext>) -> bool {
        context
            .and_then(|context| self.input_scopes_in(context))
            .is_some_and(|scopes| super::direct_field::scopes_block_suggestions(&scopes))
    }

    /// 2026-10-20 (Codex の再調査 M1): 英数モードで語を打ち始めた欄が、覚えてはいけない欄か。
    /// 読んだ欄の種類は direct_field にも知らせる (IS_PRIVATE なら、日本語を確定しても英数モードの語を書かない)
    fn latin_field_private(&self, context: Option<&ITfContext>) -> bool {
        let scopes = context.and_then(|context| self.input_scopes_in(context));
        super::direct_field::note_scopes(scopes.as_deref());
        scopes.is_some_and(|scopes| super::direct_field::scopes_block_suggestions(&scopes))
    }

    /// 予測変換: 手が止まったときに呼ばれる。今の読みの続きの候補を出す
    pub fn show_predictions(&self) -> Result<()> {
        let comp = self.borrow()?.borrow_composition()?.clone();
        // 2026-11-05 (仕事 27): ライブ変換の中でも出す (前は || comp.live で出さなかった)
        if comp.state != CompositionState::Composing || comp.hiragana.is_empty() || comp.predicted_for == comp.hiragana {
            return Ok(());
        }
        self.handle_action(&[ClientAction::ShowPredictions], CompositionState::Composing)
    }

    /// 手が止まったときに呼ばれる。辞書だけの仮表示を Zenzai の変換に差し替える。
    /// 2026-10-29: AI が遅くてこの長さの読みが 1.5 秒で変換できそうにない PC では清書しない (打っている途中に UI スレッドが何秒も止まる。
    /// Space で AI の変換をする。レビュー M1・再点検 N1)
    pub fn refine_live(&self) -> Result<()> {
        let comp = self.borrow()?.borrow_composition()?.clone();
        let reading = live_refinement_reading(&comp, false);
        // 2026-10-29 (試験中、設定 input.live_short_dictionary): 短い入力は清書しない (辞書の変換のまま。AI の変換は Space の一覧に混ぜる)
        if comp.state != CompositionState::Composing || !comp.live || comp.refined || comp.hiragana.is_empty()
            || short_live_input(&comp, &reading) {
            return Ok(());
        }
        // 2026-11-08 (ただし伊豆の件): AI が遅い見込み (直近 3 分の測り) で清書を飛ばしたら、字数だけをログに書く
        if !super::ipc_service::ai_while_typing(reading.chars().count()) {
            crate::debug_log::write(&format!("live refine: skipped (AI slow mark or budget, chars={})", reading.chars().count()));
            return Ok(());
        }
        self.handle_action(&[ClientAction::RefineLive], CompositionState::Composing)
    }

    /// 2026-11-06 (仕事 29 の 2): 上限で切った清書の答えが遅れて届いたら、読みが同じときだけ仮表示を清書に替える (時計から)
    pub fn apply_late_refine(&self) -> Result<()> {
        let comp = self.borrow()?.borrow_composition()?.clone();
        let applies = LATE_RESULT.with(|r| r.borrow().as_ref().is_some_and(|(reading, _)| late_refine_applies(&comp, reading)));
        if !applies {
            crate::debug_log::write("late refine: dropped (the reading changed)");
            return Ok(());
        }
        self.handle_action(&[ClientAction::ApplyLateRefine], CompositionState::Composing)
    }

    /// ライブ変換の表示がまだ仮のものなら、Zenzai で変換し直す。late なら (時計からの清書) 上限で切れた問い合わせを裏で待ち続ける (convert_refine)
    fn ensure_refined(ipc: &mut IPCService, comp: &mut Composition, complete: bool, late: bool) -> Result<()> {
        let reading = live_refinement_reading(comp, complete);
        if comp.live && (!comp.refined || comp.segments_reading != reading) && !comp.hiragana.is_empty() {
            if reading.is_empty() {
                return Ok(());
            }
            let converted = if late {
                ipc.convert_refine(reading.clone(), String::new())?
            } else {
                ipc.convert_with(reading.clone(), String::new(), false, false)?
            };
            set_refined(comp, reading, converted);
        }
        Ok(())
    }

    /// ライブ変換の打鍵中の仮表示 (辞書だけ)。2026-10-29 (試験中、設定 input.live_keep_refined): AI が清書した前のほう
    /// (comp.live_settled の数の文節) は、読みの頭が合うあいだそのまま見せ、その後ろ (清書したときの最後の文節と、そのあと打った字) だけを辞書で変換する
    /// (前は打つたびに読み全体を辞書で変換し直し、清書した前のほうが辞書の結果に戻ってコロコロ変わった)。合わなくなったら今までどおり全体を辞書で
    fn draft_live(ipc: &mut IPCService, comp: &mut Composition, reading: &str) -> Result<()> {
        if live_options().0 {
            if let Some((mut kept, rest)) = settled_live_prefix(comp, reading) {
                if !rest.is_empty() {
                    let (drop_anchor, tail) = convert_rest_after(ipc, &kept, &rest)?;
                    if drop_anchor {
                        kept.pop();
                        comp.live_settled = comp.live_settled.saturating_sub(1);
                    }
                    kept.extend(tail);
                }
                comp.segments = kept;
                comp.segments_reading = reading.to_string();
                comp.refined = false;
                return Ok(());
            }
        }
        comp.live_settled = 0;
        let converted = ipc.convert_with(reading.to_string(), String::new(), true, true)?;
        comp.segments = build_segments(reading, converted);
        comp.segments_reading = reading.to_string();
        comp.refined = false;
        Ok(())
    }

    /// 2026-10-29 (試験中、設定 input.live_short_dictionary): 短い入力の Space。見えている変換 (辞書の仮表示) はそのまま一番上に残し、
    /// AI の変換は候補の一覧の 2 番目に混ぜる (attach_ai_alternatives)。見えているものが今の読みのものでなければ (打ちかけの n など)、辞書で変換し直す。
    /// AI の変換が間に合わない・失敗したら、辞書の変換だけで一覧を出す
    fn offer_ai_beside_dictionary(ipc: &mut IPCService, comp: &mut Composition, reading: &str) -> Result<()> {
        if comp.segments.is_empty() || comp.segments_reading != reading {
            let converted = ipc.convert_with(reading.to_string(), String::new(), true, true)?;
            comp.segments = build_segments(reading, converted);
            comp.segments_reading = reading.to_string();
            comp.refined = false;
        }
        match ipc.convert_with(reading.to_string(), String::new(), false, false) {
            Ok(converted) => attach_ai_alternatives(&mut comp.segments, &build_segments(reading, converted)),
            Err(error) if super::ipc_service::from_server(&error) => crate::debug_log::write(&format!("short input: no AI conversion ({error})")),
            Err(error) => return Err(error),
        }
        Ok(())
    }

    /// Ctrl を押しながらのキー (MS-IME 風のキー設定に合わせる)
    fn process_ctrl_key(
        composition: &Composition,
        vk: usize,
        shift: bool,
    ) -> Option<(Vec<ClientAction>, CompositionState)> {
        use ClientAction as A;
        use CompositionState as S;
        use SetTextType as T;

        // 2026-11-08 (仕事 53): 変換・無変換 キーの無い配列 (US) では、Ctrl+J が Ctrl+変換、Ctrl+B が Ctrl+無変換、Ctrl+Shift+J が Ctrl+Shift+変換 の代わり。
        // どれも入力中・変換中で読みがあるときだけ (ほかはアプリへ)。今の Ctrl の表では j・b を使っていない。JIS の 変換・無変換 もそのまま
        let henkan = vk == 0x1C || vk == 0x4A;
        let muhenkan = vk == 0x1D || vk == 0x42;
        if (super::ivs::shortcut(vk, true, shift) || (vk == 0x4A && shift))
            && (composition.state == S::Converting || (composition.state == S::Composing && composition.live))
            && composition.segments.get(composition.focus).is_some_and(|segment| super::ivs::can_cycle(&segment.text)) {
            return Some((vec![A::CycleIvs], S::Converting));
        }
        if henkan && !shift && composition.state != S::None && !composition.hiragana.is_empty() {
            return Some((vec![A::ToggleDictionaryConversion], S::Converting));
        }
        if muhenkan && !shift && composition.state != S::None && !composition.hiragana.is_empty() {
            return Some((vec![A::ToggleNotation], S::Converting));
        }
        let key = match vk {
            0x41..=0x5A => Some((b'a' + (vk - 0x41) as u8) as char),
            _ => None,
        };
        match composition.state {
            S::None => None,
            S::Composing => {
                let convert_as = |t: T| Some((vec![A::ConvertAsOne, A::ConvertType(t)], S::Converting));
                match (vk, key) {
                    (0x0D, _) | (_, Some('m')) => Some((vec![A::EndComposition], S::None)),
                    (0x08, _) | (_, Some('h')) => {
                        if composition.hiragana_len() <= 1 {
                            Some((vec![A::Cancel], S::None))
                        } else {
                            Some((vec![A::DeleteBackward], S::Composing))
                        }
                    }
                    (_, Some('g')) => {
                        if composition.caret >= composition.hiragana_len() {
                            Some((vec![], S::Composing))
                        } else if composition.hiragana_len() <= 1 {
                            Some((vec![A::Cancel], S::None))
                        } else {
                            Some((vec![A::DeleteForward], S::Composing))
                        }
                    }
                    (_, Some('z')) => Some((vec![A::Cancel], S::None)),
                    (0x25, _) | (0x26, _) | (_, Some('a')) | (_, Some('e')) => {
                        Some((vec![A::CaretToEdge(false)], S::Composing))
                    }
                    (0x27, _) | (0x28, _) | (_, Some('f')) | (_, Some('x')) | (_, Some('n')) => {
                        Some((vec![A::CaretToEdge(true)], S::Composing))
                    }
                    (_, Some('k')) | (_, Some('s')) => Some((vec![A::MoveCaret(-1)], S::Composing)),
                    (_, Some('l')) | (_, Some('d')) => Some((vec![A::MoveCaret(1)], S::Composing)),
                    (_, Some('u')) => convert_as(T::Hiragana),
                    (_, Some('i')) => convert_as(T::Katakana),
                    (_, Some('o')) => convert_as(T::HalfKatakana),
                    (_, Some('p')) => convert_as(T::FullLatin),
                    (_, Some('t')) => convert_as(T::HalfLatin),
                    (0x20, _) => Some((
                        vec![A::EndComposition, A::CommitText(if shift { "　" } else { " " }.to_string())],
                        S::None,
                    )),
                    _ => None,
                }
            }
            S::Converting => match (vk, key) {
                (0x0D, _) | (_, Some('m')) => Some((vec![A::EndComposition], S::None)),
                (0x08, _) | (_, Some('h')) | (_, Some('g')) | (_, Some('z')) => {
                    Some((vec![A::CancelConversion], S::Composing))
                }
                (0x28, _) | (_, Some('n')) => Some((vec![A::CommitFirstSegment], S::Converting)),
                (0x25, _) | (_, Some('a')) => Some((vec![A::FocusEdge(false)], S::Converting)),
                (0x27, _) | (_, Some('f')) => Some((vec![A::FocusEdge(true)], S::Converting)),
                (_, Some('s')) => Some((vec![A::MoveFocus(-1)], S::Converting)),
                (_, Some('d')) => Some((vec![A::MoveFocus(1)], S::Converting)),
                (_, Some('k')) => Some((vec![A::Resize(-1)], S::Converting)),
                (_, Some('l')) => Some((vec![A::Resize(1)], S::Converting)),
                (_, Some('x')) => Some((vec![A::NextCandidate], S::Converting)),
                (0x26, _) | (_, Some('e')) => Some((vec![A::PrevCandidate], S::Converting)),
                (_, Some('u')) => Some((vec![A::ConvertType(T::Hiragana)], S::Converting)),
                (_, Some('i')) => Some((vec![A::ConvertType(T::Katakana)], S::Converting)),
                (_, Some('o')) => Some((vec![A::ConvertType(T::HalfKatakana)], S::Converting)),
                (_, Some('p')) => Some((vec![A::ConvertType(T::FullLatin)], S::Converting)),
                (_, Some('t')) => Some((vec![A::ConvertType(T::HalfLatin)], S::Converting)),
                (0x20, _) => Some((
                    vec![A::EndComposition, A::CommitText(if shift { "　" } else { " " }.to_string())],
                    S::None,
                )),
                // 2026-10-29: 候補の一覧を開いているときの Ctrl+Delete は、選んでいる候補 (学習した語) を忘れる。ほかのときはアプリへそのまま
                (0x2E, _) if composition.window_open => Some((vec![A::ForgetCandidate], S::Converting)),
                _ => None,
            },
        }
    }

    /// 2026-10-22: サーバーが返事をしない間に IME 本体だけで打つキー。None はアプリへそのまま渡す
    /// (かなの入力でない・Ctrl・Alt・修飾キーだけ・数字やパスワードの欄 (direct_field))。打ちかけが残っていれば、かなの入力でなくても受けて片付ける
    fn local_kana_key(&self, context: Option<&ITfContext>, wparam: WPARAM, ctrl: bool, kana: bool, composition: &Composition) -> Option<LocalKey> {
        if VK_MENU.is_pressed() || VK_LWIN.is_pressed() || VK_RWIN.is_pressed() || matches!(wparam.0, 0x10..=0x12 | 0x14 | 0xA0..=0xA5) {
            return None;
        }
        if composition.live_local_romaji().is_empty() && (!kana || ctrl
            || !matches!(self.direct_field_decision(context, wparam, kana, VK_SHIFT.is_pressed()), super::direct_field::Decision::Normal)) {
            return None;
        }
        Some(match UserAction::try_from(wparam.0) {
            Ok(UserAction::Input(c)) if !ctrl => LocalKey::Char(c),
            Ok(UserAction::Number(n)) if !ctrl => LocalKey::Char(char::from(b'0' + n as u8)),
            Ok(UserAction::Enter) => LocalKey::Enter,
            Ok(UserAction::Space) => LocalKey::Space,
            Ok(UserAction::Backspace) => LocalKey::Backspace,
            Ok(UserAction::Escape) => LocalKey::Escape,
            _ => LocalKey::Other,
        })
    }

    /// 2026-10-22: サーバーが返事をしない間の 1 キーを入れる。かなになった字を確定し、打ちかけのローマ字だけを未確定で見せる。
    /// 受けなかった (アプリへ渡す) キーは false
    fn type_local_kana(&self, key: LocalKey) -> Result<bool> {
        let (pending, open) = {
            let text_service = self.borrow()?;
            let composition = text_service.borrow_composition()?;
            (composition.live_local_romaji().to_string(), composition.tip_composition.is_some())
        };
        let step = local_kana_step(&pending, key);
        if !step.eaten {
            return Ok(false);
        }
        let show = |text: &str| -> Result<()> {
            let len = utf16_len(text);
            self.set_preedit(text, &[PreeditSpan { start: 0, len, kind: SpanKind::Input }], len)
        };
        // 前の打ちかけを見せている入力は、確定する字に書き換えて終える (字が無ければ打ちかけを書き換えるだけ)
        let mut showing = open;
        if open && !(step.commit.is_empty() && !step.pending.is_empty()) {
            self.set_preedit(&step.commit, &[], utf16_len(&step.commit))?;
            self.end_composition()?;
            showing = false;
        } else if !open && !step.commit.is_empty() {
            self.commit_raw(&step.commit)?;
        }
        if !step.pending.is_empty() {
            if !showing {
                self.start_composition()?;
            }
            show(&step.pending)?;
        }
        // 打った字はログに残さない (字数だけ)
        crate::debug_log::write(&format!("local kana: commit {} chars, pending {} chars", step.commit.chars().count(), step.pending.chars().count()));
        self.borrow()?.borrow_mut_composition()?.local_romaji = step.pending;
        Ok(true)
    }

    /// サーバーとのやり取りに失敗したキーの字を入れる。かなの入力ならひらがなにし (type_local_kana)、ほかは打ったまま
    fn type_local_text(&self, context: Option<&ITfContext>, wparam: WPARAM, text: &str) -> Result<()> {
        let composition = self.borrow()?.borrow_composition()?.clone();
        let kana = IMEState::get()?.input_mode == InputMode::Kana;
        if self.local_kana_key(context, wparam, VK_CONTROL.is_pressed(), kana, &composition).is_some() {
            for c in text.chars() {
                self.type_local_kana(LocalKey::Char(c))?;
            }
            Ok(())
        } else {
            self.commit_raw(text)
        }
    }

    #[tracing::instrument]
    /// サーバーを使わずに、文字をそのまま確定して入れる
    fn commit_raw(&self, text: &str) -> Result<()> {
        self.start_composition()?;
        let _ = self.set_preedit(text, &[], utf16_len(text));
        self.end_composition()?;
        Ok(())
    }

    /// 2026-10-29: 変換サーバー・候補窓につながっていない間 (Activate でつなげず、まだつなぎ直せていない) の動作。確定 (フォーカスの移り) と
    /// 入力モードの切り替え (言語バー・半角/全角) だけはする (前は「ipc_service is None」で失敗し、言語バーの あ/A が効かず、
    /// フォーカスの移りの後片付けも飛ばしていた。レビュー L6)。ほかの動作は今までどおり失敗させる (呼ぶ側が IME 本体だけで打つ)
    fn handle_action_offline(&self, actions: &[ClientAction]) -> Result<()> {
        for action in actions {
            match action {
                ClientAction::EndComposition => self.discard_composition_state(),
                ClientAction::SetIMEMode(mode) => {
                    super::auto_english::set_english_sentence(false);
                    self.discard_composition_state();
                    super::latin_learn::mode_changed();
                    hide_latin_suggestions();
                    IMEState::get()?.input_mode = mode.clone();
                    self.update_lang_bar()?;
                }
                _ => anyhow::bail!("ipc_service is None"),
            }
        }
        Ok(())
    }

    /// 2026-11-06 (端末の件): 未確定の範囲を失ったあと、コンポジションに触れずに IME 本体の未確定の状態を空にする (候補窓も閉じる)
    fn forget_composition(&self) {
        if let Ok(text_service) = self.borrow() {
            if let Ok(mut composition) = text_service.borrow_mut_composition() {
                *composition = Composition::default();
            }
        }
        cancel_live_refine();
        if let Ok(state) = IMEState::get() {
            if let Some(mut ipc) = state.ipc_service.clone() {
                drop(state);
                let _ = ipc.hide_window();
                let _ = ipc.clear_text();
            }
        }
    }

    /// 見えているものを確定し、未確定の状態を空にする (サーバーの失敗・パニックのあと)
    fn discard_composition_state(&self) {
        let _ = self.end_composition();
        if let Ok(text_service) = self.borrow() {
            if let Ok(mut composition) = text_service.borrow_mut_composition() {
                let tip = composition.tip_composition.take();
                *composition = Composition::default();
                composition.tip_composition = tip;
            }
        }
    }

    pub fn handle_key(&self, context: Option<&ITfContext>, wparam: WPARAM) -> Result<bool> {
        // 2026-10-29: 1 回のキーでサーバー・候補窓を待つ時間の合計は EVENT_BUDGET まで (超えたら残りは問い合わせず、ひらがな・辞書だけで返す)
        let _budget = super::ipc_service::EventBudget::start();
        if let Some(context) = context {
            self.borrow_mut()?.context = Some(context.clone());
        } else {
            return Ok(false);
        };
        // 2026-10-28: IME が有効になったときにサーバー・候補窓へつなげていなければ、つなぎ直す (2 秒に 1 回まで。つながるまでは下の
        // 「サーバーが返事をしない間」と同じに、IME 本体だけでひらがなを入れる)
        super::ipc_service::reconnect_if_needed();
        // 2026-10-14: 前のキーの途中でパニックを受け止めていたら、見えているものを確定して未確定の状態を空にしてから進む
        if crate::guard::take_reset() {
            crate::debug_log::write("reset after a caught panic");
            self.discard_composition_state();
        }
        // 2026-11-06 (端末で未確定の途中の形が積み重なった件): 前のキーで未確定の範囲を失っていたら (アプリが未確定の字を確定して消した・
        // 書いた字を範囲が覆わなかった)、または入力中なのにコンポジションを持っていなければ (確定の途中で失敗した)、未確定の状態を空にしてから
        // このキーを新しい入力として扱う
        let needs_reset = {
            let text_service = self.borrow()?;
            let composition = text_service.borrow_composition()?;
            composition_needs_reset(&composition)
        };
        if crate::tsf::edit_session::take_composition_lost() || needs_reset {
            crate::debug_log::write("the composition was lost: starting a new input");
            self.forget_composition();
        }
        // 入力中にサーバーが返事をしなくなっていたら、見えているものを確定し、このキーの文字は IME 本体だけで入れる (かなの入力ならひらがな)
        if super::ipc_service::server_unresponsive()
            && self.borrow()?.borrow_composition()?.state != CompositionState::None
        {
            let typed = self.process_key(context, wparam).ok().flatten().map(|(actions, _)| raw_typed(&actions)).unwrap_or_default();
            self.discard_composition_state();
            if !typed.is_empty() {
                let _ = self.type_local_text(context, wparam, &typed);
            }
            return Ok(true);
        }
        // 2026-10-22: 入力中でなく、サーバーが返事をしない間 (と、そのときの打ちかけが残っている間) は、IME 本体だけで打つ (process_key と同じ判断)
        {
            let composition = self.borrow()?.borrow_composition()?.clone();
            if composition.state == CompositionState::None
                && (super::ipc_service::server_unresponsive() || !composition.live_local_romaji().is_empty())
            {
                let mode = IMEState::get()?.input_mode.clone();
                // 2026-11-08 (仕事 53): 入力モードのキーも 半角/全角 と同じに (process_key と同じ読み替え)
                let switched = match super::mode_keys::mode_key(wparam.0, VK_CONTROL.is_pressed(), VK_SHIFT.is_pressed()) {
                    Some(super::mode_keys::ModeKey::AlreadyHandled) => return Ok(true),
                    Some(super::mode_keys::ModeKey::To(target)) if target == mode => return Ok(true),
                    Some(_) => local_mode_toggle(WPARAM(super::mode_keys::VK_HALF_FULL), false, &mode),
                    None => local_mode_toggle(wparam, VK_CONTROL.is_pressed(), &mode),
                };
                if let Some(mode) = switched {
                    if let Err(error) = self.handle_action(&[ClientAction::SetIMEMode(mode)], CompositionState::None) {
                        crate::debug_log::write(&format!("mode switch while the server is away: {error:?}"));
                    }
                    return Ok(true);
                }
                let kana = mode == InputMode::Kana;
                return match self.local_kana_key(context, wparam, VK_CONTROL.is_pressed(), kana, &composition) {
                    Some(key) => self.type_local_kana(key),
                    None => Ok(false),
                };
            }
        }

        if let Some((actions, transition)) = self.process_key(context, wparam)? {
            // 2026-11-05 (長押し): 押しっぱなしの繰り返しで、することの無いキー (長押しの候補を出したあとの同じ字・続けて入れない英字) は、
            // 飲み込むだけで描き直さない (前は繰り返しのたびに未確定の文字列と候補の窓を描き直し、変換の候補がちらついた。13:37 の報告)
            if actions.is_empty() && super::diacritics::is_repeat() {
                return Ok(true);
            }
            if actions == [ClientAction::CycleIvs] && transition == CompositionState::None {
                return Ok(context.is_some_and(|context| self.cycle_selected_ivs(context).unwrap_or(false)));
            }
            let restoring_ivs = self.borrow()?.borrow_composition()?.ivs_original.is_some()
                && actions.contains(&ClientAction::CancelConversion);
            let (actions, transition) = if restoring_ivs {
                (vec![ClientAction::Cancel], CompositionState::None)
            } else { (actions, transition) };
            crate::debug_log::write(&format!("handle {:?} -> {:?}", actions, transition));
            // 2026-10-13: / のあとで日本語に戻した直後に、利用者がそれを全部消したら (Escape・Backspace)、英字のつもりだった綴りとして覚える
            if actions.iter().any(|a| matches!(a, ClientAction::Cancel)) {
                if let Some(word) = super::slash_commands::take_switched() {
                    let typed = self.borrow()?.borrow_composition()?.typed.clone();
                    if typed.len() <= word.len() + 3 && may_learn() { super::slash_commands::learn(&word); }
                }
            } else if actions.iter().any(|a| matches!(a, ClientAction::EndComposition | ClientAction::CommitFirstSegment)) {
                let _ = super::slash_commands::take_switched();
            }
            if let Err(error) = self.handle_action(&actions, transition) {
                // サーバーが落ちている・返事が来ない等。キーを飲み込まずにアプリへ返し、未確定状態を片付ける
                crate::debug_log::write(&format!("handle_action failed: {error:?}"));
                self.discard_composition_state();
                // サーバーとのやり取りで失敗したキーは、打った文字を IME 本体だけで入れる (返してもアプリに届かず消えるため。かなの入力ならひらがな)
                if super::ipc_service::from_server(&error) {
                    let typed = raw_typed(&actions);
                    if !typed.is_empty() && self.type_local_text(context, wparam, &typed).is_ok() {
                        return Ok(true);
                    }
                }
                return Ok(false);
            }
        } else {
            return Ok(false);
        }

        Ok(true)
    }

    /// 未確定の文字列が英単語だけか (かなが入っていない)
    /// 2026-10-19: 英単語だけを打っている途中の、次の語の候補。打った文字の列とかなの表示が同じ英字の語で、
    /// 候補を打ち終えても英字のまま (ローマ字に読み替えられない) ものだけ
    fn next_word_candidates(comp: &Composition) -> Vec<String> {
        let typed = comp.hiragana.as_str();
        // 大文字で始めた語 (一時的な英数入力) は打った文字の列を記録していないので、表示の英字で見る
        let typed_matches = (comp.typed_valid && comp.typed == comp.hiragana) || comp.temp_ascii;
        if super::direct_field::private_field() || !typed_matches || typed.is_empty()
            || !typed.chars().all(|c| c.is_alphabetic() || c == '\'') || comp.caret < comp.hiragana_len() {
            return vec![];
        }
        // 2026-11-05 (仕事 24): 英語の文の中で打っている語のつづりの指摘 (設定 input.latin_spell)。一覧の一番上に直した形を出し、Tab で替える。
        // 前の語を直す指摘 (fix.previous: 確定した Tu → Tú) は、確定した字を書き換えることになるので出さない
        let fixed = super::auto_english::in_english_sentence().then(|| super::latin_spell::suggestion(&Self::spell_before(comp), typed)).flatten()
            .filter(|fix| !fix.previous && fix.end <= typed.len() && typed.is_char_boundary(fix.start) && typed.is_char_boundary(fix.end))
            .map(|fix| format!("{}{}{}", &typed[..fix.start], fix.fixed, &typed[fix.end..]))
            .filter(|word| word != typed);
        let words: Vec<String> = if super::next_word::enabled() {
            super::next_word::suggest(&comp.prev_word, typed)
                .into_iter()
                .filter(|word| {
                    let tokens = super::auto_english::segment(word);
                    tokens.len() == 1 && tokens[0].kind == super::auto_english::TokenKind::English
                })
                .collect()
        } else {
            vec![]
        };
        fixed.into_iter().chain(words).fold(vec![], |mut list, word| { if !list.contains(&word) { list.push(word); } list })
    }

    /// 2026-11-05 (仕事 24): つづりの指摘・自動修正に渡す、打っている語より前の文字列 (キャレットの前。読めなければ最後に確定した文字列)
    fn spell_before(comp: &Composition) -> String {
        comp.text_before.clone().unwrap_or_else(super::next_word::last_commit_text)
    }

    /// Tab / Shift+Tab で補う候補の番号 (候補が出ていなければ None)
    fn next_word_tab(comp: &Composition, shift: bool) -> Option<usize> {
        let n = comp.next_words.len();
        if n == 0 {
            return None;
        }
        match comp.next_word_pick {
            Some(i) if comp.next_words.get(i) == Some(&comp.hiragana) => Some(if shift { (i + n - 1) % n } else { (i + 1) % n }),
            _ if comp.next_words_for == comp.hiragana => Some(if shift { n - 1 } else { 0 }),
            _ => None,
        }
    }

    fn english_only(comp: &Composition) -> bool {
        if comp.caret < comp.hiragana_len() || comp.hiragana.is_empty() {
            return false;
        }
        let ascii_letters = latin_context_input(&comp.hiragana);
        if !ascii_letters {
            return false;
        }
        if comp.typed_valid && !comp.typed.is_empty() {
            let tokens = super::auto_english::segment(&comp.typed);
            !tokens.is_empty() && tokens.iter().all(|t| t.kind == super::auto_english::TokenKind::English || !t.raw.chars().any(|c| c.is_ascii_alphabetic()))
        } else {
            // 大文字の一時英数などで打った英字
            comp.hiragana.chars().count() >= 2
        }
    }

    fn english_only_at_boundary(comp: &Composition) -> bool {
        let finishing_english = || !comp.typed_valid || super::auto_english::segment_finishing(&comp.typed).iter().all(|t|
            t.kind == super::auto_english::TokenKind::English || !t.raw.chars().any(|c| c.is_ascii_alphabetic()));
        if Self::english_only(comp) { return finishing_english(); }
        // 2026-10-16: my・today・many は打っている途中だと最後がローマ字の打ちかけ (my = みゃ の途中、today = とだ+y) で英単語にならず、
        // Space で変換に回って空白も英語の文脈も付かなかった (my name が my なめ になっていた)。
        // 最後が打ちかけの英字のまま Space を押し、その時点で全体が英単語と決まるなら、英単語と同じく確定して空白。
        // nn と打つ人の末尾の n 1 つ (run・kon) は今までどおり変換に回す (候補でかなに戻せるように)
        let nn_single = super::auto_english::spelling().nn && comp.typed.ends_with('n') && !comp.typed.ends_with("nn");
        comp.typed_valid && !comp.typed.is_empty() && comp.caret >= comp.hiragana_len() && !nn_single
            && ends_with_pending_latin(&comp.hiragana) && finishing_english()
            && !super::auto_english::segment_finishing(&comp.typed).is_empty()
    }

    /// 桁区切りの , が正しいか確かめる (iOS と同じ: , のあとがちょうど 3 桁なら ,、そうでなければ 、)。
    /// `finished` = 数字を打ち終えた (数字以外を打つ・変換・確定)。打っている途中でも 4 桁目で 、 に戻す
    fn fix_thousands_comma(ipc: &mut IPCService, comp: &mut Composition, finished: bool) -> Result<()> {
        if comp.caret < comp.hiragana_len() || !super::auto_english::number_punctuation() {
            return Ok(());
        }
        let chars: Vec<char> = comp.hiragana.chars().collect();
        let digits = chars.iter().rev().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || chars.len() < digits + 1 {
            return Ok(());
        }
        let comma = chars.len() - digits - 1;
        // , の前の数字は、入力の頭なら入力を始めたときのキャレットの直前の字 (確定した 1 のあとに ,0000 と打った)
        let digit_before = if comma == 0 { comp.char_before.is_some_and(is_number_digit) } else { chars[comma - 1].is_ascii_digit() };
        if chars[comma] != ',' || !digit_before {
            return Ok(());
        }
        if digits == 3 || (!finished && digits < 3) {
            return Ok(());
        }
        // , を 、 に戻す (キャレットを , の後ろまで戻して消し、、 を入れて、元の位置へ)
        ipc.edit_text(Op::MoveCursor, String::new(), -(digits as i32))?;
        ipc.edit_text_raw(Op::DeleteBackward, String::new(), 0)?;
        ipc.edit_text_raw(Op::InsertDirect, "、".to_string(), 0)?;
        let (hiragana, caret, raw) = ipc.edit_text_raw(Op::MoveCursor, String::new(), digits as i32)?;
        comp.hiragana = apply_character_form(&hiragana);
        comp.caret = caret;
        comp.raw_input = raw;
        Ok(())
    }

    /// 候補を表示したまま、number 番目 (1 始まり) の文字を反映する
    fn preview_accent(ipc: &mut IPCService, comp: &mut Composition, number: i32) -> Result<bool> {
        if let Some(base) = comp.accent {
            if let Some(ch) = number.checked_sub(1).and_then(|i| usize::try_from(i).ok())
                .and_then(|i| super::diacritics::variants(base).get(i).copied()) {
                comp.accent_index = number - 1;
                if comp.accent_inserted == Some(ch) {
                    return Ok(true);
                }
                if super::auto_english::enabled() && comp.replace_typed_accent(ch) {
                    // 打った文字の列を置き換えて組み直す (アクセント付きの文字を含む語は英語になるので、続きも英字のまま: España)
                    comp.english_active = true;
                    Self::apply_auto_english(ipc, comp, false)?;
                } else if comp.state == CompositionState::Composing {
                    // 未確定の文字列の最後の文字を置き換える
                    comp.typed_valid = false;
                    ipc.edit_text_raw(Op::DeleteBackward, String::new(), 0)?;
                    let (hiragana, caret, raw) = ipc.edit_text_raw(Op::InsertDirect, ch.to_string(), 0)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                    comp.raw_input = raw;
                } else {
                    // アプリに入っている元の文字を消して入れ直す
                    send_backspace_and_char(ch);
                }
                comp.accent_inserted = Some(ch);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 長押しの候補の number 番目 (1 始まり) に決めて候補窓を閉じる
    fn pick_accent(&self, ipc: &mut IPCService, comp: &mut Composition, number: i32) -> Result<()> {
        if !Self::preview_accent(ipc, comp, number)? { return Ok(()) }
        comp.close_accents();
        ipc.hide_window()?;
        Ok(())
    }

    /// 長押しの候補をすぐ閉じる (キーの判定中に、ほかのキーが押されたとき)
    fn close_accents_now(&self) {
        if let Ok(text_service) = self.borrow() {
            if let Ok(mut c) = text_service.borrow_mut_composition() {
                c.close_accents();
            }
        }
        if let Ok(state) = IMEState::get() {
            if let Some(mut ipc) = state.ipc_service.clone() {
                let _ = ipc.hide_window();
            }
        }
    }

    /// TeX のコマンドの文字か (\ と、それに続く英字)
    fn tex_command_char(comp: &Composition, text: &str) -> bool {
        let mut chars = text.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else { return false };
        if comp.caret < comp.hiragana_len() || !super::tex::active() {
            return false;
        }
        c == '\\' || (comp.tex_command && c.is_ascii_alphabetic())
            || (comp.tex_command && comp.hiragana.ends_with('\\') && !c.is_alphanumeric() && !c.is_whitespace())
    }

    /// ^ _ の上付き・下付きとして 1 文字入れる (script_applies が true のとき)。記号は英字のまま入れ、次の文字で置き換える
    fn insert_script_char(ipc: &mut IPCService, comp: &mut Composition, c: char) -> Result<()> {
        use super::scripts::{step, ScriptOp};
        let vowel = vowel_before_script(comp);
        let mut last = None;
        // 2026-11-01: 上付き・下付きにした字 (²・₂) と記号 (^ _) は、打った列 (typed) にもそのまま入れる。前は打った列を使えなくしていて、
        // そのあとの英語の判定が止まっていた (cos^2x+sin^2x の 2 つ目の sin が し + n)。英語の判定では英字でない字として扱われ、組み直すときも
        // そのまま入る。記号の直前の かな を英字に戻すとき (a_n) は、打った列と読みが合わなくなるので今までどおり使えなくする
        match step(&mut comp.script, c, &comp.hiragana.clone(), vowel) {
            ScriptOp::Normal => {}
            ScriptOp::Marker(m) => {
                comp.typed.push(m);
                last = Some(ipc.edit_text_raw(Op::InsertDirect, m.to_string(), 0)?);
            }
            ScriptOp::ReplaceMarker(t, vowel) => {
                if vowel.is_some() || comp.typed.pop().is_none_or(|marker| marker != '^' && marker != '_') {
                    comp.typed_valid = false;
                }
                comp.typed.push_str(&t);
                last = Some(ipc.edit_text_raw(Op::DeleteBackward, String::new(), 0)?);
                // a_n の あ は英字の a に戻す
                if let Some(v) = vowel {
                    ipc.edit_text_raw(Op::DeleteBackward, String::new(), 0)?;
                    last = Some(ipc.edit_text_raw(Op::InsertDirect, v.to_string(), 0)?);
                }
                if !t.is_empty() {
                    last = Some(ipc.edit_text_raw(Op::InsertDirect, t, 0)?);
                }
            }
            ScriptOp::Insert(t) => {
                comp.typed.push_str(&t);
                last = Some(ipc.edit_text_raw(Op::InsertDirect, t, 0)?);
            }
            ScriptOp::Nothing => {}
            ScriptOp::Revert(t) => {
                // 顔文字の ^o^: 上付きにした ᵒ を消して ^o^ を入れる
                comp.typed.pop();
                comp.typed.push_str(&t);
                ipc.edit_text_raw(Op::DeleteBackward, String::new(), 0)?;
                last = Some(ipc.edit_text_raw(Op::InsertDirect, t, 0)?);
            }
        }
        if let Some((hiragana, caret, raw)) = last {
            comp.hiragana = apply_character_form(&hiragana);
            comp.caret = caret;
            comp.raw_input = raw;
        }
        Ok(())
    }

    /// 2026-11-01: ^ _ の上付き・下付きの途中 (10^23 の数字・記号の直後) で、上付き・下付きにしない字 (x・=) を打ったら状態を終える。
    /// 前は script_applies で試すだけで状態が残り、x^2x=1 の最後の 1 まで上付き (=¹) になっていた
    fn end_script_on_plain_char(comp: &mut Composition, text: &str) {
        if comp.script != super::scripts::ScriptState::Off && !Self::script_applies(comp, text) {
            comp.script = super::scripts::ScriptState::Off;
        }
    }

    /// この 1 文字を ^ _ の上付き・下付きとして扱うか (状態は変えずに試す)
    fn script_applies(comp: &Composition, text: &str) -> bool {
        let mut chars = text.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else { return false };
        if !super::scripts::enabled() || comp.caret < comp.hiragana_len() || super::tex::active() {
            return false;
        }
        let mut state = comp.script;
        super::scripts::step(&mut state, c, &comp.hiragana, None) != super::scripts::ScriptOp::Normal
    }

    /// 一時的な英数入力の状態を、打った文字で進める (Mozc の Composer::ApplyTemporaryInputMode と同じ規則)。
    /// この文字を英字のまま入れるなら true
    fn update_temp_ascii(comp: &mut Composition, text: &str) -> bool {
        let mut chars = text.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else { return comp.temp_ascii };
        if !c.is_ascii() || c.is_ascii_control() {
            comp.temp_ascii = false;
            comp.shifted_count = 0;
            return false;
        }
        if c.is_ascii_uppercase() {
            if !comp.temp_ascii {
                comp.temp_ascii_start = (comp.caret >= comp.hiragana_len()).then(|| comp.hiragana.chars().count());
            }
            comp.temp_ascii = true;
            comp.shifted_count += 1;
        } else if c.is_ascii_lowercase() {
            // 大文字が 2 文字以上続いたあとの小文字で、かなに戻る
            if comp.shifted_count > 1 {
                comp.temp_ascii = false;
            }
            comp.shifted_count = 0;
        } else if !c.is_ascii_digit() {
            // 2026-11-03: 数字では数え直さない (前は CO2 の 2 で 0 に戻り、後ろの nituite が英字のまま続いていた)
            comp.shifted_count = 0;
        }
        if !comp.temp_ascii {
            comp.temp_ascii_start = None;
        }
        comp.temp_ascii
    }

    /// 2026-11-03: 一時的な英数入力の小文字 c で、Shift で始めた並び (temp_ascii_start から) を かな に戻すなら、(消す字の数, かな として入れ直す打鍵)。
    /// 並びを区切りの道具で見て、英字の語のあとが日本語のとき (auto_english::temp_ascii_return)。
    /// 前は大文字 1 字で始めた並び (C₂nituite・Googlede) が、確定するまで英字のまま続いていた
    fn temp_ascii_return_point(comp: &Composition, c: char, rules: super::auto_english::TempAsciiRules) -> Option<(usize, String)> {
        let start = comp.temp_ascii_start?;
        if !c.is_ascii_lowercase() || comp.caret < comp.hiragana_len() || comp.hiragana.chars().count() < start {
            return None;
        }
        let run: Vec<char> = comp.hiragana.chars().skip(start).chain([c]).collect();
        let p = super::auto_english::temp_ascii_return_with(&run.iter().collect::<String>(), rules)?;
        // 入れ直す打鍵は画面の英字 (全角の英字の設定なら半角に戻す)
        let keys: String = run[p..].iter().map(|&k| if ('\u{FF01}'..='\u{FF5E}').contains(&k) { char::from_u32(k as u32 - 0xFEE0).unwrap_or(k) } else { k }).collect();
        keys.is_ascii().then(|| (run.len() - 1 - p, keys))
    }

    /// 一時的な英数入力の 1 字を入れる (handle_action と試験で同じ手順)。かな に戻す位置があれば、そこから後ろを消してローマ字として入れ直す
    fn insert_temp_ascii(ipc: &mut IPCService, comp: &mut Composition, text: &str) -> Result<()> {
        comp.typed_valid = false;
        let back = text.chars().next().filter(|_| text.chars().count() == 1).and_then(|c| Self::temp_ascii_return_point(comp, c, super::auto_english::TEMP_ASCII_RULES));
        let (hiragana, caret, raw) = match back {
            Some((delete, keys)) => {
                for _ in 0..delete {
                    ipc.edit_text_raw(Op::DeleteBackward, String::new(), 0)?;
                }
                comp.temp_ascii = false;
                comp.shifted_count = 0;
                comp.temp_ascii_start = None;
                ipc.edit_text_raw(Op::Insert, to_fullwidth(&keys, false), 0)?
            }
            None => ipc.edit_text_raw(Op::InsertDirect, text.to_string(), 0)?,
        };
        comp.hiragana = apply_character_form(&hiragana);
        comp.caret = caret;
        comp.raw_input = raw;
        Ok(())
    }

    /// 打たれている文字の列を英単語とローマ字に区切り直し、英語と判定した語があれば
    /// サーバーの読みを組み直す (英語の部分は英字のまま入れる)
    /// finishing: Space・Enter を押した時点 (nn と打つ人の末尾の n 1 つを英語のしるしとして見る)
    fn apply_auto_english(ipc: &mut IPCService, comp: &mut Composition, finishing: bool) -> Result<()> {
        use super::auto_english::{canonical_case, has_english, runs, segment_finishing_with_spaces, TokenKind};
        // 2026-11-08 (仕事 12): 打ち終えたときは、空白なしで打った英語の句の語の切れ目 (打った列のバイトの位置) に半角の空白を入れる
        // 2026-11-11: 打っている途中も、製品名の表の綴り (aftereffects) は表記 (After Effects) にして見せる (Enter でもその表記で確定する)
        let (tokens, spaces) = if finishing { segment_finishing_with_spaces(&comp.typed) } else { super::auto_english::segment_with_products(&comp.typed) };
        let english = has_english(&tokens);
        if !english && !comp.english_active {
            return Ok(());
        }
        ipc.clear_text()?;
        if tokens.is_empty() {
            comp.hiragana.clear();
            comp.caret = 0;
            comp.raw_input.clear();
            comp.english_active = false;
            return Ok(());
        }
        let mut last = (String::new(), 0, String::new());
        let all_runs = runs(&tokens);
        // 数字の間の . , は英字のまま入れる (ローマ字の表だと 。 、 に戻る)。区切りが打った列とずれていれば使わない
        let marks = if super::auto_english::number_punctuation() && all_runs.iter().map(|(t, _)| t.len()).sum::<usize>() == comp.typed.len() {
            number_marks_in_typed(&comp.typed, comp.char_before)
        } else {
            vec![]
        };
        let mut offset = 0;
        for (k, (text, kind)) in all_runs.iter().cloned().enumerate() {
            let start = offset;
            offset += text.len();
            last = match kind {
                TokenKind::English => {
                    // 設定でオンのときだけ、決まった表記に直す (github → GitHub)。大文字小文字だけなので打った文字数は変わらない
                    // 2026-10-20: 普通の英単語と同じ綴りの名前 (windows・zoom) は、和文にすき間なくつながっているときだけ (windowsno → Windowsの)
                    let glued = glued_to_japanese(&all_runs, k);
                    let fixed = canonical_case(&text).or_else(|| glued.then(|| super::auto_english::japanese_context_case(&text)).flatten())
                        // 2026-11-04 (仕事 19 の 3a): 英字 1 字＋語 (jri-gu → Jリーグ・xziku → x軸) の字は、表の多い形に
                        .or_else(|| all_runs.get(k + 1).filter(|(_, kind)| *kind == TokenKind::Romaji)
                            .and_then(|(after, _)| super::auto_english::initial_word_case(&text, after)));
                    // 2026-11-04 (仕事 19 の 2): 英語の固有名詞の表 (einstein → Einstein。区切りの判定には使わない表) も
                    let fixed = fixed.map(str::to_string).or_else(|| super::auto_english::english_name_case(&text));
                    // 2026-11-05 (仕事 22 の 2): かなのすぐ後ろの . に続く、小文字で打った拡張子 (siryou.pdf) は直さずに渡す
                    // (資料.pdf か 〜だよ。PDF かは変換サーバーが決め、。 のままなら表の形にする。slash_dot.swift)
                    let kana_dot = k > 0 && all_runs[k - 1].1 == TokenKind::Romaji && super::auto_english::kana_dot_extension(&all_runs[k - 1].0, &text);
                    let fixed = if kana_dot { None } else { fixed };
                    let text = match super::auto_english::case_fix_enabled().then_some(fixed).flatten() {
                        Some(fixed) => fixed,
                        None => text,
                    };
                    let inserted = ipc.edit_text_raw(Op::InsertDirect, text, 0)?;
                    // 2026-11-08 (仕事 12): 英語の句の語の切れ目 (次も英字の語) なら、続けて半角の空白
                    if spaces.contains(&offset) && all_runs.get(k + 1).is_some_and(|(_, kind)| *kind == TokenKind::English) {
                        ipc.edit_text_raw(Op::InsertDirect, " ".to_string(), 0)?
                    } else {
                        inserted
                    }
                }
                TokenKind::Romaji => {
                    // 2026-10-30: ローマ字の並びの終わりの n 1 つのすぐ後ろに英字の語が来るなら ん にする (henAB = へん + AB)。
                    // 前は n を打ちかけのまま変換サーバーに渡し、後ろの英字をそのまま足していたので、へnAB と n が残っていた
                    let before_english = all_runs.get(k + 1).is_some_and(|(_, kind)| *kind == TokenKind::English);
                    let text = if before_english && text.ends_with('n') && !text.ends_with("nn")
                        && text.bytes().all(|b| b.is_ascii_lowercase()) { format!("{text}n") } else { text };
                    let mut piece_start = 0;
                    let mut result = None;
                    for &at in marks.iter().filter(|&&at| at >= start && at < start + text.len()) {
                        let at = at - start;
                        if at > piece_start {
                            ipc.edit_text_raw(Op::Insert, to_fullwidth(&text[piece_start..at], false), 0)?;
                        }
                        result = Some(ipc.edit_text_raw(Op::InsertDirect, text[at..at + 1].to_string(), 0)?);
                        piece_start = at + 1;
                    }
                    match result {
                        Some(r) if piece_start == text.len() => r,
                        _ => ipc.edit_text_raw(Op::Insert, to_fullwidth(&text[piece_start..], false), 0)?,
                    }
                }
            };
        }
        comp.hiragana = apply_character_form(&last.0);
        comp.caret = last.1;
        comp.raw_input = last.2;
        comp.english_active = english;
        Ok(())
    }

    /// 打った文字の列 (comp.typed) からサーバーの読みを組み直す (英語の判定もやり直す。打ち終えたときの区切り)
    fn rebuild_from_typed(ipc: &mut IPCService, comp: &mut Composition) -> Result<()> {
        comp.english_active = true;
        Self::apply_auto_english(ipc, comp, true)
    }

    /// 2026-10-28: 変換するときに、ローマ字の打ち間違いを直す。直す前の打った列は comp.typo_repair に残す (Shift+BackSpace で戻す)。
    /// ① 子音の 3 つ重ね (matttya の ttt) は、1 つ多い子音を消す (auto_english::sokuon_repairs。っっ は日本語に無いので採点しない。いつも)
    /// ② (試験中、設定 input.typo_repair) 母音の抜け (souiuktodesu の k): 直す所 (auto_english::vowel_repairs。読めない子音 1 字だけ) ごとに、
    /// 母音を補った打鍵の列でサーバーの読みを組み直し、補う前の読みといっしょに ScoreReadings で採点して、よいもの (pick_repair) があれば打った列をそれに替える。
    /// Shift+BackSpace で戻した綴り (typo_repair_ng.txt) は直さない。採点の RPC が無い古い変換サーバーなら直さない
    fn repair_typos(ipc: &mut IPCService, comp: &mut Composition) -> Result<()> {
        use super::auto_english::{excess_vowel_spots, sokuon_repairs, typo_repair_declined, typo_repair_enabled, typo_repair_key, vowel_in_english_enabled,
            vowel_repairs, vowel_repairs_in_english};
        if comp.typo_repair.is_some() || comp.typo_repair_declined || !comp.typed_valid
            || comp.edit_unit || comp.typed.is_empty() || comp.caret != comp.hiragana_len() {
            return Ok(());
        }
        comp.typo_suggestions.clear();
        let original_typed = comp.typed.clone();
        let mut typed = original_typed.clone();
        let extra = sokuon_repairs(&typed);
        for &at in extra.iter().rev() {
            typed.remove(at);
        }
        // 直す所ごとの (補う (位置, 母音) の組, 英字の区切りの中か)
        let mut spots: Vec<(Vec<(usize, char)>, bool)> = vec![];
        if typo_repair_enabled() && !super::ipc_service::server_unresponsive() {
            for (at, vowel) in vowel_repairs(&typed, true) {
                if typo_repair_declined(&typo_repair_key(&typed, at)) { continue; }
                match spots.last_mut() {
                    Some((group, _)) if group[0].0 == at => group.push((at, vowel)),
                    _ => spots.push((vec![(at, vowel)], false)),
                }
            }
            // 2026-11-04 (仕事 20、同じ設定): 抜けた所の子音が隣の英字とつながって英字の区切りになった所 (watsh + iha = わたしは)。
            // 表に無い小文字の英字の区切りごとに 1 組。直す所は前と合わせて 2 か所まで
            if vowel_in_english_enabled() {
                let groups: Vec<Vec<(usize, char)>> = vowel_repairs_in_english(&typed).into_iter()
                    .map(|group| group.into_iter().filter(|&(at, _)| !typo_repair_declined(&typo_repair_key(&typed, at))).collect::<Vec<_>>())
                    .filter(|group| !group.is_empty()).collect();
                if spots.len() + groups.len() <= 2 {
                    spots.extend(groups.into_iter().map(|group| (group, true)));
                    spots.sort_by_key(|(group, _)| group[0].0);
                }
            }
        }
        // 2026-10-30 (試験中、同じ設定): 母音を 1 つ多く打った所 (motsuu = もつう の 2 つめの u)。母音の抜けを直す所が無いときだけ
        let excess: Vec<usize> = if spots.is_empty() && typo_repair_enabled() && !super::ipc_service::server_unresponsive() {
            excess_vowel_spots(&typed).into_iter().filter(|&at| !typo_repair_declined(&typo_repair_key(&typed, at + 1))).take(15).collect()
        } else {
            vec![]
        };
        if extra.is_empty() && spots.is_empty() && excess.is_empty() {
            return Ok(());
        }
        if !extra.is_empty() {
            comp.typed = typed.clone();
            Self::rebuild_from_typed(ipc, comp)?;
        }
        let base_typed = typed.clone();
        let mut reading = comp.hiragana.clone();
        let (mut keys, mut repaired) = (vec![], vec![]);
        let mut inserted = 0;
        // 母音の直しは全体で 1.5 秒まで (超えたら、そこまでで止める)。子音の 3 つ重ねの組み直し (上) はこの上限の外 (レビュー M4)
        let started = std::time::Instant::now();
        let narrow = super::ipc_service::NarrowBudget::within(std::time::Duration::from_millis(1500));
        'spots: for (group, in_english) in spots {
            if started.elapsed() > std::time::Duration::from_millis(1500) {
                break;
            }
            let mut readings = vec![normalize_reading(&reading)];
            let mut variants = vec![];
            for (at, vowel) in group {
                let at = at + inserted;
                let mut candidate = typed.clone();
                candidate.insert(at, vowel);
                comp.typed = candidate.clone();
                // 2026-10-29: 組み直しが上限で切れたら、直すのをそこでやめる (下で、それまでに直した列で組み直す。前は変換ごと失敗し、
                // ひらがなのまま確定していた。レビュー M4)
                if Self::rebuild_from_typed(ipc, comp).is_err() {
                    break 'spots;
                }
                readings.push(normalize_reading(&comp.hiragana));
                variants.push((candidate, comp.hiragana.clone(), at));
            }
            // 2026-10-29: 採点が遅い PC (直近に測った速さで、この字数の採点が 1.5 秒以上かかりそう) では母音の直しをしない (再点検 N1)
            if super::ipc_service::scoring_slow_for(readings.iter().map(|r| r.chars().count()).sum()) {
                break;
            }
            // 採点できなければ (古い変換サーバー・時間切れ)、そこで直すのをやめる (下で、それまでに直した列で組み直す)
            let Ok(Some(scores)) = ipc.score_readings(readings, String::new()) else { break };
            let pick = if in_english { pick_english_repair(&scores) } else { pick_repair(&scores) };
            // 2026-11-06 (仕事 20 の手 1): 自動で直すほどではないが、補った読みのほうが補う前より AI の点で上なら、点の高い順に 2 つまでを
            // 一覧に出す候補として覚える (このあとの所の直しは、これより後ろの位置なので、ここの位置はずれない)
            if pick.is_none() {
                comp.typo_suggestions.extend(typo_suggestion_order(&scores).into_iter().take(2)
                    .filter_map(|k| variants.get(k - 1).and_then(|(_, after, _)| repaired_spot(&reading, after))));
            }
            if let Some(best) = pick {
                let (candidate, after, at) = variants.swap_remove(best - 1);
                repaired.extend(repaired_spot(&reading, &after));
                keys.push(typo_repair_key(&base_typed, at - inserted));
                (typed, reading) = (candidate, after);
                inserted += 1;
            }
        }
        // 母音の多すぎ: 1 つ抜いた打鍵の列ごとに読みを組み直し、抜く前の読みといっしょに採点して、読みだけの AI の点数が十分よいもの
        // (pick_excess。差の決まりは excess_margin) があれば抜く。直した所の「打ったとおり」は、抜いた母音のかなを戻したもの
        if !excess.is_empty() && started.elapsed() < std::time::Duration::from_millis(1500) {
            let mut readings = vec![normalize_reading(&reading)];
            let mut variants = vec![];
            for &at in &excess {
                let mut candidate = typed.clone();
                candidate.remove(at);
                comp.typed = candidate.clone();
                if Self::rebuild_from_typed(ipc, comp).is_err() {
                    break;
                }
                readings.push(normalize_reading(&comp.hiragana));
                variants.push((candidate, comp.hiragana.clone(), at));
            }
            let slow = super::ipc_service::scoring_slow_for(readings.iter().map(|r| r.chars().count()).sum());
            if variants.len() + 1 == readings.len() && !variants.is_empty() && !slow {
                let margins: Vec<f32> = variants.iter().map(|(_, _, at)| excess_margin(&typed, *at)).collect();
                if let Ok(Some(scores)) = ipc.score_readings(readings, String::new()) {
                    if let Some(best) = pick_excess(&scores, &margins) {
                        let (candidate, after, at) = variants.swap_remove(best - 1);
                        repaired.extend(removed_spot(&reading, &after));
                        keys.push(typo_repair_key(&typed, at + 1));
                        (typed, reading) = (candidate, after);
                    }
                }
            }
        }
        // 最後の組み直しは 1.5 秒の上限の外で (途中で切れると、IME 本体とサーバーの読みが食い違う)。キーの上限 (EVENT_BUDGET) はかかる
        drop(narrow);
        comp.typed = typed;
        Self::rebuild_from_typed(ipc, comp)?;
        if !keys.is_empty() || !extra.is_empty() {
            crate::debug_log::write(&format!("typo repair: {} vowel(s), {} extra sokuon", keys.len(), extra.len()));
            comp.typo_repair = Some(TypoRepair { original_typed, keys, spots: repaired });
        }
        Ok(())
    }

    /// 注目文節の候補を (まだなら) 取ってくる。今の表記を先頭に置く
    fn ensure_candidates(&self, ipc: &mut IPCService, comp: &mut Composition) -> Result<()> {
        let context = comp.text_before_focus();
        let typed = typed_for_focus(comp);
        let segment_start: usize = comp.segments.iter().take(comp.focus).map(|s| s.reading.chars().count()).sum();
        let repaired = comp.typo_repair.as_ref().map(|r| r.spots.clone()).unwrap_or_default();
        let suggestions = comp.typo_suggestions.clone();
        let segment = comp
            .segments
            .get_mut(comp.focus)
            .context("focused segment not found")?;
        if !segment.candidates.is_empty() {
            // 一覧を作ったときに Windows の答えが時間切れだったら、ここで 1 回だけ問い直す (2 回目も切れたらあきらめる。Space のたびに待たせない)
            if std::mem::take(&mut segment.windows_pending) {
                if let Some(windows) = super::windows_text::conversions_answered(&segment.reading) {
                    merge_late_windows_candidates(segment, windows);
                }
            }
            return Ok(());
        }
        let (mut candidates, notes, learned) = ipc.list_candidates_noted(segment.reading.clone(), context.clone(), note_settings().any())?;
        segment.notes = candidates.iter().cloned().zip(notes).filter(|(_, note)| !note.is_empty()).collect();
        // 2026-11-08: 学習した語を含む候補 (Ctrl+Delete で忘れられる候補) の印も、注記と同じ所に持つ (種類 LEARNED_NOTE。忘れたら注記と一緒に消える)
        segment.notes.extend(candidates.iter().zip(learned).filter(|(_, learned)| *learned).map(|(c, _)| (c.clone(), LEARNED_NOTE.to_string())));
        if segment.patchwork && !candidates.is_empty() {
            // 2026-11-21 (恍惚上たい の案 3): 本当の変換でない表示 (間に合わなかったときのつぎはぎ) は 1 番目に置き直さず、一覧の 1 番目を見せる
            // (B の途中まで打った読み 4,000 か所で、窓の 1 番目が正しいのは 間に合わなかった字数 2 で 514 → 2,267、3 で 297 → 2,206)
            segment.text = candidates[0].clone();
            segment.patchwork = false;
        } else {
            candidates.retain(|c| c != &segment.text);
            candidates.insert(0, segment.text.clone());
        }
        let windows = super::windows_text::conversions_answered(&segment.reading);
        segment.windows_pending = windows.is_none();
        merge_windows_candidates(&mut candidates, &segment.reading, windows.unwrap_or_default());
        // 英字を含む文節は表記の違いを先に並べ、日本語の候補は下で補う。
        if segment.text.chars().any(|c| c.is_ascii_alphabetic()) && segment.reading.chars().any(|c| c.is_ascii_alphabetic()) {
            candidates = latin_spelling_candidates(&segment.text, &segment.reading, candidates);
        }
        // English detection ranks candidates, but must never remove the Japanese escape path.
        if let Some(kana) = latin_kana_reading(&segment.reading) {
            let mut recovered = ipc.list_candidates(kana.clone(), context.clone())?;
            merge_windows_candidates(&mut recovered, &kana, windows_candidates(&kana));
            add_japanese_recovery(&mut candidates, &kana, recovered);
        }
        // 読みに英字が混ざっていたら (けltype)、打ったとおりの英字 (keltype) を 2 番目に
        // (かなのすぐ後ろに英字が続く読み = ローマ字の途中が崩れたもの。keltypeは のように英字の後ろに助詞が付くだけなら足さない)
        // 打ちかけの 1 文字 (しゃんp) はふつうのことなので含めない。かなの後ろに英字が 2 文字以上続くときだけ
        let chars: Vec<char> = segment.reading.chars().collect();
        let broken = chars.windows(3).any(|w| !w[0].is_ascii() && w[1].is_ascii_alphabetic() && w[2].is_ascii_alphabetic());
        if broken {
            let romaji = to_romaji(&segment.reading, &super::auto_english::spelling());
            candidates.retain(|c| c != &romaji);
            candidates.insert(1.min(candidates.len()), romaji);
        }
        segment.typed_latin = typed_latin_candidate(&segment.reading, typed.as_deref());
        // 2026-10-29 (試験中、設定 input.live_short_dictionary): 短い入力の Space で求めた AI の変換を 2 番目に (一番上は見えていた辞書の変換のまま)
        if let Some(ai) = segment.ai_text.clone().filter(|ai| ai != &segment.text) {
            candidates.retain(|c| c != &ai);
            candidates.insert(1.min(candidates.len()), ai);
        }
        // 2026-11-06 (仕事 20 の手 1): 自動で直すほどではなかった母音の抜けの所を含む文節なら、補った読みの一覧の頭 2 つを 2 番目に (注記「直し」)
        let mut suggested: Vec<String> = suggestions.iter().filter_map(|spot| suggested_segment_reading(&segment.reading, segment_start, spot)).collect();
        suggested.dedup();
        let mut at = 1.min(candidates.len());
        for reading in suggested.into_iter().take(2) {
            let Ok(list) = ipc.list_candidates(reading.clone(), context.clone()) else { break };
            for text in list.into_iter().filter(|t| t != &segment.text && !t.is_empty()).take(2) {
                if candidates.iter().position(|c| c == &text).is_some_and(|k| k <= at) { continue; }
                candidates.retain(|c| c != &text);
                candidates.insert(at.min(candidates.len()), text.clone());
                at += 1;
                segment.notes.retain(|(c, _)| c != &text);
                segment.notes.push((text.clone(), "直し\t母音を補った形 (Space で選ぶと、補った読みで覚える)".into()));
                segment.repair_readings.push((text, reading.clone()));
            }
        }
        // 2026-11-06 (仕事 20 の手 2): 母音の抜けでも読めるローマ字になった所 (かぢ = kadai を kadi)。直しの設定がオンで、手 1 の候補が無い
        // かなだけの文節 (2〜10 字) なら、読みに母音を 1 字足した読みを採点し、十分よいものの頭の 1 つを一覧に (注記「直し」)。採点が遅い PC ではしない
        let chars = segment.reading.chars().count();
        if super::auto_english::typo_repair_enabled() && segment.repair_readings.is_empty() && (2..=10).contains(&chars)
            && segment.reading.chars().all(|c| ('ぁ'..='ゖ').contains(&c) || c == 'ー') {
            let alternatives = one_vowel_readings(&segment.reading, &super::auto_english::spelling(), 40);
            let readings: Vec<String> = std::iter::once(segment.reading.clone()).chain(alternatives.iter().cloned()).collect();
            if !alternatives.is_empty() && !super::ipc_service::scoring_slow_for(readings.iter().map(|r| r.chars().count()).sum()) {
                if let Ok(Some(scores)) = ipc.score_readings(readings, context.clone()) {
                    for k in pick_vowel_lookups(&scores, VOWEL_LOOKUP_MARGIN) {
                        let reading = alternatives[k - 1].clone();
                        let Ok(list) = ipc.list_candidates(reading.clone(), context.clone()) else { break };
                        let Some(text) = list.into_iter().find(|t| t != &segment.text && !t.is_empty() && !candidates[..at.min(candidates.len())].contains(t)) else { continue };
                        candidates.retain(|c| c != &text);
                        candidates.insert(at.min(candidates.len()), text.clone());
                        at += 1;
                        segment.notes.retain(|(c, _)| c != &text);
                        segment.notes.push((text.clone(), "直し\t母音を 1 字補った形 (選ぶと、補った読みで覚える)".into()));
                        segment.repair_readings.push((text, reading));
                    }
                }
            }
        }
        // Emoji follow every ordinary candidate, including half-width kana and the menu.
        finish_segment_candidates(segment, candidates);
        // 2026-10-28 (試験中): 母音の抜けを直した文節は、打ったとおり (そういうkと) を一覧の最後に残し、直したことを注記で知らせる
        if let Some(raw) = unrepaired_reading(&segment.reading, segment_start, &repaired) {
            segment.candidates.retain(|c| c != &raw);
            segment.candidates.push(raw.clone());
            segment.notes.retain(|(c, _)| c != &raw && c != &segment.text);
            segment.notes.push((segment.text.clone(), "直し\t打ち間違いを直しました (Shift+BackSpace で打ったとおりに戻す)".into()));
            segment.notes.push((raw, "直し\t打ったとおり".into()));
        }
        Ok(())
    }

    /// 2026-10-14: 変換中の Backspace。カーソルの左の 1 字 (カーソルのある文節の読みの最後の字) をサーバーの読みからも消し、
    /// その文節だけ変換し直す (ほかの文節で選んだ候補はそのまま)。読みと文節の対応が崩れているときは、今までどおり全体をかなに戻す
    fn delete_at_convert_cursor(&self, ipc: &mut IPCService, comp: &mut Composition) -> Result<()> {
        let joined: String = comp.segments.iter().map(|s| s.reading.as_str()).collect();
        if comp.segments.is_empty() || joined != comp.hiragana {
            comp.segments.clear();
            comp.focus = 0;
            comp.cursor_moved = false;
            comp.window_open = false;
            let offset = comp.hiragana_len() - comp.caret;
            if offset != 0 {
                let (hiragana, caret) = ipc.edit_text(Op::MoveCursor, String::new(), offset)?;
                comp.hiragana = apply_character_form(&hiragana);
                comp.caret = caret;
            }
            comp.state = CompositionState::Composing;
            return Ok(());
        }
        let target = if convert_cursor_at_end(comp) { comp.segments.len() - 1 } else { comp.focus };
        // サーバーの読みの、その文節の最後の字の後ろへカーソルを動かして 1 字消し、末尾へ戻す
        let end: i32 = comp.segments[..=target].iter().map(|s| s.reading.chars().count() as i32).sum();
        ipc.edit_text(Op::MoveCursor, String::new(), end - comp.caret)?;
        let (deleted, caret) = ipc.edit_text(Op::DeleteBackward, String::new(), 0)?;
        let (hiragana, caret) = ipc.edit_text(Op::MoveCursor, String::new(), deleted.chars().count() as i32 - caret)?;
        comp.hiragana = apply_character_form(&hiragana);
        comp.caret = caret;
        comp.typed.clear();
        comp.typed_valid = false;
        comp.window_open = false;
        comp.segments[target].reading.pop();
        if comp.segments[target].reading.is_empty() {
            comp.segments.remove(target);
            if comp.segments.is_empty() {
                // 全部消えたら未確定文字列ごと終える
                let _ = self.set_preedit("", &[], 0);
                self.end_composition()?;
                ipc.clear_text()?;
                comp.reset();
                comp.state = CompositionState::None;
                return Ok(());
            }
            comp.focus = target.saturating_sub(1).min(comp.segments.len() - 1);
            if target == 0 { comp.cursor_moved = false; }
            return Ok(());
        }
        let context: String = comp.segments[..target].iter().map(|s| s.text.as_str()).collect();
        let reading = comp.segments[target].reading.clone();
        let candidates = ipc.list_candidates(reading.clone(), context)?;
        let segment = &mut comp.segments[target];
        segment.text = candidates.into_iter().next().unwrap_or(reading);
        segment.candidates.clear();
        segment.kana_cycle.clear();
        segment.index = 0;
        comp.focus = target;
        Ok(())
    }

    fn select_candidate(comp: &mut Composition, index: i32) {
        comp.latin_learning_intent = false;
        if let Some(segment) = comp.segments.get_mut(comp.focus) {
            let len = segment.candidates.len() as i32;
            if len == 0 {
                return;
            }
            let index = index.rem_euclid(len);
            segment.index = index;
            let candidate = &segment.candidates[index as usize];
            // 「単漢字…」を選んでいる間は読みを表示しておく
            segment.text = if candidate == SINGLE_KANJI_MARKER {
                segment.reading.clone()
            } else {
                candidate.clone()
            };
        }
        comp.latin_selected = selected_literal_latin(comp);
    }

    #[tracing::instrument]
    pub fn handle_action(
        &self,
        actions: &[ClientAction],
        transition: CompositionState,
    ) -> Result<()> {
        // 2026-10-29: 時計・フォーカスの移りから呼ばれたときも、待ちの合計は EVENT_BUDGET まで (キーから呼ばれたときはキーの上限のまま)
        let _budget = super::ipc_service::EventBudget::start();
        #[allow(clippy::let_and_return)]
        let (mut comp, mode) = {
            let text_service = self.borrow()?;
            let composition = text_service.borrow_composition()?.clone();
            let mode = IMEState::get()?.input_mode.clone();
            (composition, mode)
        };
        let Some(mut ipc) = IMEState::get()?.ipc_service.clone() else {
            return self.handle_action_offline(actions);
        };
        // 2026-11-05 (仕事 23): 数字を全角で見せるか (設定 input.fullwidth_digits)。F9・F10 で英字を選んだ入力は打ったとおり
        super::full_width::set_digit_display(digit_form_for(&comp, &mode));
        // 時計から呼ばれた動作 (手が止まったときの清書・予測の候補)。キーではないので、清書の時計は掛け直さない
        let from_timer = actions.iter().any(|action| matches!(action, ClientAction::RefineLive | ClientAction::ApplyLateRefine | ClientAction::ShowPredictions));
        // 。、？！ で変換した直後の印は、次のキーまで (PunctuationConvert で付け直す)
        if !from_timer {
            comp.punctuation_converted = false;
        }
        // 2026-11-10: Esc で読みに戻した表示は、次に何かをするキーまで (時計からの清書・予測と、何もしないキー (↑ など) では戻さない)
        if ends_reading_shown(actions, from_timer) {
            comp.live_reading = false;
        }

        self.update_context(&comp.display, comp.char_before)?;
        // 。、？！ で変換しようとして変換できなかった: 変換中にせず入力を続ける
        let mut keep_composing = false;
        // 利用者が待っている動作の上限は、1 回のキーで 1 つ (最初の待つ動作から、動作を終えるまで。[ConvertAsOne, ConvertType] のように
        // 待つ動作が並んでも足し算にしない。3 回目の点検 5)。描画はその場の上限に戻す
        let mut awaited = None;
        let end_composition = ClientAction::EndComposition;

        for action in actions {
            // 2026-11-10 (Fable の諮問 quality 3、A 11-08 20:02): 文節が 1 つしかないときの文節ごとの確定は、全体の確定 (EndComposition) と同じ道を通す。
            // 前はここで未確定を終えるだけで、学習 (ipc.learn)・予測の覚え・確定の取り消しの控えを通らず、文節ごとに確定する打ち方では何も覚えなかった
            let action = if *action == ClientAction::CommitFirstSegment && comp.segments.len() <= 1 { &end_composition } else { action };
            if awaited.is_none() && awaited_by_user(action) {
                awaited = Some(super::ipc_service::AwaitedBudget::begin());
            }
            if !keeps_dictionary_preview(action) {
                comp.dictionary_preview = None;
            }
            // 直した母音の抜けは、読みを打ち足す・消す・カーソルを動かすまで (そのあとの Shift+BackSpace はふつうの BackSpace)
            if matches!(action, ClientAction::Insert(_) | ClientAction::DeleteBackward | ClientAction::DeleteForward | ClientAction::DeleteAtCursor
                | ClientAction::MoveCaret(_) | ClientAction::CaretToEdge(_)) {
                comp.typo_repair = None;
                comp.typo_suggestions.clear();
            }
            // 数字を打ち終えたところで、桁区切りの , を確かめる
            let finishes_number = match action {
                ClientAction::Insert(text) => !text.chars().all(|c| c.is_ascii_digit()),
                ClientAction::Convert | ClientAction::ToggleDictionaryConversion | ClientAction::ToggleNotation | ClientAction::ConvertAsOne | ClientAction::EndComposition | ClientAction::CommitSuffix(_) => true,
                _ => false,
            };
            if finishes_number && comp.state != CompositionState::None && comp.state != CompositionState::Converting {
                Self::fix_thousands_comma(&mut ipc, &mut comp, true)?;
            }
            // ん を nn と打つ人が n 1 つで止めて Space を押したら、英単語かどうかを見直す (run = run、kon = こん)。
            // Enter は見えているものをそのまま確定するので見直さない
            let finishes_word = matches!(action, ClientAction::Convert | ClientAction::ToggleDictionaryConversion | ClientAction::ToggleNotation | ClientAction::ConvertAsOne);
            // 2026-10-16: 最後がローマ字の打ちかけ (きょうはとだy) のまま変換するときも、Space の時点で英単語かどうかを見直す (今日はtoday)
            // 2026-11-08 (仕事 12): 空白なしで打った英語の句があるときも、打ち終えたときの区切りで入れ直す (語の間に空白)
            if finishes_word && comp.state == CompositionState::Composing && comp.typed_valid
                && comp.caret >= comp.hiragana_len() && super::auto_english::enabled()
                && (super::auto_english::in_english_sentence() || ends_with_pending_latin(&comp.hiragana) || has_english_phrase(&comp)) {
                Self::apply_auto_english(&mut ipc, &mut comp, true)?;
            }
            if finishes_word
                && comp.state == CompositionState::Composing
                && comp.typed_valid
                && comp.caret >= comp.hiragana_len()
                && super::auto_english::enabled()
                && super::auto_english::spelling().nn
                && comp.typed.ends_with('n')
                && !comp.typed.ends_with("nn")
            {
                Self::apply_auto_english(&mut ipc, &mut comp, true)?;
                // ライブ変換の仮表示は n を除いた読みで変換してあるので、読み全体で変換し直す (末尾の英字は今までどおり後ろに付ける)
                if comp.live {
                    comp.refined = false;
                    let full = live_refinement_reading(&comp, true);
                    if full.is_empty() {
                        comp.segments.clear();
                        comp.segments_reading.clear();
                    } else {
                        Self::ensure_refined(&mut ipc, &mut comp, true, false)?;
                    }
                }
            }
            // 2026-10-16: Space で英単語として確定する語のうち、打っている途中はローマ字の打ちかけだったもの (my = みゃ の途中) は、
            // 英字として入れ直してから確定する (サーバーの読みを打ちかけのままにしない)。ライブ変換の仮表示は使わず、入れ直した英字を確定
            // 2026-11-08 (仕事 12): 英単語と判定していても、空白なしで打った英語の句なら入れ直す (語の間に空白)
            if matches!(action, ClientAction::CommitSuffix(_)) && comp.state == CompositionState::Composing && comp.typed_valid
                && comp.caret >= comp.hiragana_len() && super::auto_english::enabled() && (!comp.english_active || has_english_phrase(&comp)) {
                Self::apply_auto_english(&mut ipc, &mut comp, true)?;
                if comp.english_active {
                    comp.segments.clear();
                    comp.segments_reading.clear();
                }
            }
            if let ClientAction::Insert(text) = action {
                if mode == InputMode::Kana { Self::end_script_on_plain_char(&mut comp, text); }
            }
            match action {
                ClientAction::StartComposition => {
                    // 2026-10-14: 英単語の後ろの , . を 、 。 にするかの判定 (auto_english::english_punctuation) に、キャレットの直前の字を見ておく
                    let context = self.borrow()?.context.clone();
                    let before = context.and_then(|context| self.preceding_text_in(&context, 40));
                    super::auto_english::set_text_before(before.as_ref().map(|(text, start)| (text.as_str(), *start)));
                    let committed_last = comp.last_commit.as_ref().and_then(|last| last.text.chars().last()).or(comp.commit_tail);
                    self.start_composition()?;
                    self.update_pos()?;
                    comp.reset();
                    comp.prev_word = super::next_word::previous_word(before.as_ref().map(|(text, _)| text.as_str()));
                    comp.text_before = before.as_ref().map(|(text, _)| text.clone());
                    comp.char_before = before.as_ref().and_then(|(text, _)| text.chars().last()).or(committed_last);
                    comp.last_commit = None;
                    comp.english_active = false;
                    comp.live = is_live_profile();
                    ipc.clear_text()?;
                }
                ClientAction::EndComposition => {
                    // 確定するのは画面に見えているものそのまま (ここで変換し直さない)
                    cancel_live_refine();
                    let converted = !comp.segments.is_empty()
                        && (comp.state == CompositionState::Converting || comp.live);
                    cancel_predict();
                    if converted && !comp.from_prediction && may_learn() {
                        let readings = comp.segments.iter().map(learning_reading).collect();
                        let texts = comp.segments.iter().map(|s| s.text.clone()).collect();
                        if let Err(error) = ipc.learn(readings, texts) {
                            tracing::warn!("Failed to learn: {error:?}");
                        }
                    }
                    // 予測変換のために、確定した語句を読みと一緒に覚える (予測の候補を選んだときは覚え直して前に出す)
                    if converted && comp.ivs_original.is_none() && may_learn() {
                        let reading: String = comp.segments.iter().map(learning_reading).collect();
                        super::predict::remember(&reading, &comp.converted_text());
                    }
                    let suffix = std::mem::take(&mut comp.commit_suffix);
                    let mid_caret = comp.state == CompositionState::Composing && !comp.display.is_empty()
                        && (comp.edit_unit || (comp.live && comp.caret < comp.hiragana_len()));
                    // 2026-11-08 (ただし伊豆の件): ライブ変換の確定が、清書した表示か・辞書だけの表示か (カーソルが途中・短い入力) と、
                    // 学習に送ったかを、数と真偽だけログに書く (字は書かない)。辞書だけの表示を覚える決まりを決める材料
                    if comp.live && crate::debug_log::enabled() {
                        let reading = composition_reading(&comp);
                        crate::debug_log::write(&format!(
                            "commit (live): state={:?}, refined={}, mid_caret={}, short={}, chars={}, from_prediction={}, learned={}",
                            comp.state, comp.refined, mid_caret, short_live_input(&comp, &reading), reading.chars().count(),
                            comp.from_prediction, converted && !comp.from_prediction && may_learn()));
                    }
                    let text = if mid_caret {
                        // カーソルが途中のライブ変換は、見えている (左右を別々に変換した) ものをそのまま確定
                        comp.display.clone()
                    } else if converted {
                        let pending = if comp.live && comp.state == CompositionState::Composing {
                            live_enter_suffix(&comp, super::auto_english::spelling().nn)
                        } else {
                            String::new()
                        };
                        format!("{}{}", comp.converted_text(), pending)
                    } else {
                        enter_reading(&comp, super::auto_english::spelling().nn)
                    };
                    // 2026-11-05 (仕事 24): 英語の文で、英字の語を Space で確定するとき、設定が autocorrect なら確かな綴りの誤りを直して確定する
                    // (直した直後の Backspace で打ったとおりに戻す: UndoAutocorrect)。学習・次の語の候補には打ったとおりの字を渡さない (直した字)
                    let english_word = suffix == " " && super::auto_english::in_english_sentence() && latin_context_input(&text);
                    let fixes = if english_word { super::latin_spell::autocorrections(&Self::spell_before(&comp), &text) } else { vec![] };
                    let typed_text = text.clone();
                    let text = if fixes.is_empty() { text } else { super::latin_spell::apply(&text, &fixes) };
                    // 2026-11-08 (仕事 12): 空白なしで打った英語の句に空白を入れて確定するなら、直後の Backspace で空白なしに戻せるように
                    // (つづりの自動修正と同じ UndoAutocorrect)。確定する字の半角の空白が、入れた空白の数とちょうど同じときだけ
                    let phrase_spaces = if text.contains(' ') && comp.typed_valid && fixes.is_empty() && !mid_caret {
                        super::auto_english::segment_finishing_with_spaces(&comp.typed).1
                    } else {
                        vec![]
                    };
                    // 2026-11-19: 製品名 (After Effects) は打った綴り (aftereffects) に戻し、当てた綴りを覚える (auto_english::undo_for_spaces)
                    let unspaced = (!phrase_spaces.is_empty() && text.matches(' ').count() == phrase_spaces.len()).then(|| {
                        let products = super::auto_english::finishing_products(&comp.typed);
                        super::auto_english::undo_for_spaces(&text, &comp.typed, &phrase_spaces, &products)
                    });
                    // 末尾に足す文字 (英単語の後ろの空白) は、同じ確定に含める (確定のあとで別に入れると、先に確定した文字が消えるアプリがある)
                    let shown = format!("{text}{suffix}");
                    if shown != comp.display {
                        let len = utf16_len(&shown);
                        let _ = self.set_preedit(&shown, &[], len);
                    }
                    self.end_composition()?;
                    // 利用者が直した英字・かなを覚える (auto_english)。日本語を確定したら英数モードの語の学習に知らせる
                    if comp.typed_valid && !comp.typed.is_empty() && !text.is_empty() && may_learn() {
                        super::auto_english::learn_from_commit(&comp.typed, &typed_text, comp.latin_learning_intent);
                    }
                    if text.chars().any(|c| !c.is_ascii()) {
                        super::latin_learn::japanese_committed();
                        record_romaji_style(&mut comp);
                    }
                    let reading = enter_reading(&comp, super::auto_english::spelling().nn);
                    let segments = if converted {
                        comp.segments.iter().cloned().map(|mut segment| {
                            segment.reading = learning_reading(&segment);
                            segment
                        }).collect()
                    } else {
                        build_segments(&reading, vec![])
                    };
                    let remembered = (!text.is_empty() && comp.ivs_original.is_none()).then(|| LastCommit {
                        text: text.clone(),
                        reading: segments.iter().map(|s| s.reading.as_str()).collect(),
                        segments,
                    });
                    let latin = latin_context_input(&text);
                    let explicit_latin = comp.latin_selected;
                    let spaced = suffix == " ";
                    comp.reset();
                    comp.last_commit = remembered;
                    comp.last_autocorrect = (!fixes.is_empty() && text != typed_text)
                        .then(|| (shown.clone(), format!("{typed_text}{suffix}"), fixes.iter().map(|fix| fix.typed.clone()).collect()));
                    comp.last_phrase_runs.clear();
                    if let (None, Some((plain, runs))) = (&comp.last_autocorrect, unspaced) {
                        comp.last_autocorrect = Some((shown.clone(), format!("{plain}{suffix}"), vec![]));
                        comp.last_phrase_runs = runs;
                    }
                    comp.state = CompositionState::None;
                    // 2026-11-06 (c): 候補窓に知らせられなくても、確定は済んでいるので「入力中でない」まで書き戻す
                    // (前はここで抜けて、コンポジションを手放したのに IME 本体が入力中のまま残った)
                    if let Err(error) = ipc.hide_window().and_then(|_| ipc.clear_text()) {
                        crate::debug_log::write(&format!("commit: hiding the candidate window failed: {error:?}"));
                    }
                    super::auto_english::note_sentence_commit(latin, explicit_latin, spaced);
                    super::auto_english::note_commit(Some(&text));
                    super::next_word::note_commit(Some(&shown));
                }
                ClientAction::Cancel => {
                    cancel_live_refine();
                    let original = comp.ivs_original.as_deref().unwrap_or("");
                    // 2026-11-06 (c): 未確定の字を戻せなくても・候補窓に知らせられなくても、「入力中でない」まで書き戻す
                    if let Err(error) = self.set_preedit(original, &[], utf16_len(original)) {
                        crate::debug_log::write(&format!("cancel: restoring the text failed: {error:?}"));
                    }
                    self.end_composition()?;
                    comp.reset();
                    comp.state = CompositionState::None;
                    if let Err(error) = ipc.hide_window().and_then(|_| ipc.clear_text()) {
                        crate::debug_log::write(&format!("cancel: hiding the candidate window failed: {error:?}"));
                    }
                    super::auto_english::set_english_sentence(false);
                }
                ClientAction::CommitSmartSpace { reverse } => {
                    let previous = self.preceding_char().or_else(|| comp.last_commit.as_ref().and_then(|last| last.text.chars().last()));
                    let text = smart_space(previous, *reverse);
                    self.start_composition()?;
                    let _ = self.set_preedit(text, &[], 1);
                    self.end_composition()?;
                    comp.reset();
                    comp.last_commit = None;
                    comp.state = CompositionState::None;
                }
                ClientAction::PeriodShortcut => {
                    // 直前の空白を「. 」に入れ替える。直前が空白でなくなっていたら、打った Space をそのまま入れる
                    if self.start_composition_over_previous(" ")? {
                        self.set_text(". ", "")?;
                        self.end_composition()?;
                    } else {
                        self.start_composition()?;
                        let _ = self.set_preedit(" ", &[], 1);
                        self.end_composition()?;
                    }
                    comp.reset();
                    comp.state = CompositionState::None;
                }
                ClientAction::SmartDash => {
                    // 直前の - を — に入れ替える。直前が - でなくなっていたら (読めない・動かした)、打った - をそのまま入れる
                    if self.start_composition_over_previous("-")? {
                        self.set_text("\u{2014}", "")?;
                        self.end_composition()?;
                    } else {
                        self.start_composition()?;
                        let _ = self.set_preedit("-", &[], 1);
                        self.end_composition()?;
                    }
                    comp.reset();
                    comp.state = CompositionState::None;
                }
                ClientAction::CommitText(text) => {
                    self.start_composition()?;
                    let _ = self.set_preedit(text, &[], utf16_len(text));
                    self.end_composition()?;
                    comp.reset();
                    comp.state = CompositionState::None;
                }
                // 数字の後ろの 。 、 に続けて数字を打ったら、小数点・桁区切りに直す (2。→ 2.0、1、→ 1,000)。
                // 。 を打った時点では文末か小数点か分からないので、数字が続いたときにさかのぼって直す
                ClientAction::Insert(text)
                    if mode == InputMode::Kana
                        && super::auto_english::number_punctuation()
                        && text.len() == 1
                        && text.chars().all(|c| c.is_ascii_digit())
                        && comp.caret >= comp.hiragana_len()
                        && number_mark_fix(&comp.hiragana, comp.char_before).is_some() =>
                {
                    let ascii = number_mark_fix(&comp.hiragana, comp.char_before).unwrap();
                    if comp.typed_valid {
                        comp.typed.push_str(text);
                    }
                    ipc.edit_text_raw(Op::DeleteBackward, String::new(), 0)?;
                    ipc.edit_text_raw(Op::InsertDirect, ascii.to_string(), 0)?;
                    let (hiragana, caret, raw) = ipc.edit_text_raw(Op::InsertDirect, text.to_string(), 0)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                    comp.raw_input = raw;
                }
                // 2026-10-30: 入力の頭・記号のあとの - (ー になっている) に続けて数字を打ったら、マイナス・範囲の - に直す (-126・x=-1)。
                // かな・漢字のあとは長音のまま (あー3・えー2.6・そっかー1・ナンバー1。minus_fix)
                ClientAction::Insert(text)
                    if mode == InputMode::Kana
                        && super::auto_english::number_punctuation()
                        && text.len() == 1
                        && text.chars().all(|c| c.is_ascii_digit())
                        && comp.caret >= comp.hiragana_len()
                        && minus_fix(&comp.hiragana, comp.char_before) =>
                {
                    if comp.typed_valid {
                        comp.typed.push_str(text);
                    }
                    ipc.edit_text_raw(Op::DeleteBackward, String::new(), 0)?;
                    ipc.edit_text_raw(Op::InsertDirect, "-".to_string(), 0)?;
                    let (hiragana, caret, raw) = ipc.edit_text_raw(Op::InsertDirect, text.to_string(), 0)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                    comp.raw_input = raw;
                }
                // 英字・数字のすぐ後ろの - は長音 (ー) ではなくハイフン (e-mail, 3-4)。
                // ただし かなの後ろ (または頭) に 1 つだけある n はローマ字の打ちかけなので、n- は んー にする (2026-10-15)
                ClientAction::Insert(text)
                    if mode == InputMode::Kana
                        && text == "-"
                        && comp.caret >= comp.hiragana_len()
                        && comp.hiragana.chars().last().is_some_and(|c| c.is_ascii_alphanumeric())
                        && !ends_with_lone_romaji_n(&comp.hiragana)
                        // 2026-11-01: ^{ } の中の - は上付きの ⁻ (e^{-x} が e-ˣ になっていた)
                        && !Self::script_applies(&comp, text) =>
                {
                    if comp.typed_valid {
                        comp.typed.push('-');
                    }
                    let (hiragana, caret, raw) = ipc.edit_text_raw(Op::InsertDirect, "-".to_string(), 0)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                    comp.raw_input = raw;
                }
                // 2026-11-05 (仕事 22 の 1): 数字のすぐ後ろの / は ・ ではなく半角の / (1/2・11/5・24/7)。英字の語のすぐ後ろ (and/or・TCP/IP) は、
                // 打った文字の列の区切り直し (auto_english の segment_plain) で決める。数字にはさまれた / で後ろの語が ・ の形のもの (1・2年生) は、
                // 変換するときに変換サーバーが決め直す
                // 2026-11-05 (仕事 28 の 1): いつも半角の / か . にする設定 (input.slash_key = slash・period_key = period) なら、どこでも半角のまま
                ClientAction::Insert(text)
                    if mode == InputMode::Kana
                        && (text == "/" || text == ".")
                        && comp.caret >= comp.hiragana_len()
                        && ((text == "/" && super::auto_english::slash_auto() && comp.hiragana.chars().last().is_some_and(|c| c.is_ascii_digit()))
                            || (text == "/" && super::auto_english::slash_key() == super::auto_english::KeyForm::Latin)
                            || (text == "." && super::auto_english::period_key() == super::auto_english::KeyForm::Latin))
                        && !Self::script_applies(&comp, text) =>
                {
                    if comp.typed_valid {
                        comp.typed.push_str(text);
                    }
                    let (hiragana, caret, raw) = ipc.edit_text_raw(Op::InsertDirect, text.to_string(), 0)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                    comp.raw_input = raw;
                }
                ClientAction::Insert(text) if mode == InputMode::Kana && Self::tex_command_char(&comp, text) => {
                    // TeX のコマンド (\textbf の英字、\{ のような記号 1 字) は英字のまま
                    comp.typed_valid = false;
                    let c = text.chars().next().unwrap();
                    comp.tex_command = c == '\\' || (c.is_ascii_alphabetic() && comp.tex_command);
                    let (hiragana, caret, raw) = ipc.edit_text_raw(Op::InsertDirect, text.to_string(), 0)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                    comp.raw_input = raw;
                }
                ClientAction::Insert(text) if mode == InputMode::Kana && Self::script_applies(&comp, text) => {
                    // ^ _ の上付き・下付き。記号は英字のまま入れ、次の文字で置き換える
                    Self::insert_script_char(&mut ipc, &mut comp, text.chars().next().unwrap())?;
                }
                ClientAction::Insert(text) if mode == InputMode::Kana && shift_ascii_enabled() && Self::update_temp_ascii(&mut comp, text) => {
                    // 一時的な英数入力中は、打った文字をそのまま (半角で) 入れる。英語の自動判別はこの入力では使わない
                    Self::insert_temp_ascii(&mut ipc, &mut comp, text)?;
                }
                ClientAction::Insert(text) => {
                    let is_digit = !text.is_empty() && text.chars().all(|c| c.is_ascii_digit());
                    let at_end = comp.caret >= comp.hiragana_len();
                    if at_end {
                        comp.typed.push_str(text);
                    } else {
                        comp.typed_valid = false;
                    }
                    let text = match mode {
                        InputMode::Kana => to_fullwidth(text, false),
                        InputMode::Latin => text.to_string(),
                    };
                    let (hiragana, caret, raw) = ipc.edit_text_raw(Op::Insert, text, 0)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                    comp.raw_input = raw;
                    if mode == InputMode::Kana && at_end && comp.typed_valid && super::auto_english::enabled() {
                        Self::apply_auto_english(&mut ipc, &mut comp, false)?;
                    }
                    // 2026-10-28: 子音の 3 つ重ね (matttya の ttt) は、打っているうちに 1 つ多い子音を消して組み直す (っっ は日本語に無い)。
                    // 変換するときだけ直していたので、Enter で確定すると まっっちゃ のままだった (B の写経 10-28)
                    if mode == InputMode::Kana && at_end && comp.typed_valid {
                        let extra = super::auto_english::sokuon_repairs(&comp.typed);
                        if !extra.is_empty() {
                            for &at in extra.iter().rev() {
                                comp.typed.remove(at);
                            }
                            comp.english_active = true;
                            Self::apply_auto_english(&mut ipc, &mut comp, false)?;
                        }
                    }
                    // , のあとの 4 桁目で 、 に戻す
                    if mode == InputMode::Kana && is_digit {
                        Self::fix_thousands_comma(&mut ipc, &mut comp, false)?;
                    }
                }
                ClientAction::DeleteBackward => {
                    comp.script = super::scripts::ScriptState::Off;
                    let at_end = comp.caret >= comp.hiragana_len();
                    if mode == InputMode::Kana && at_end && comp.typed_valid && comp.english_active {
                        // 英語が入っているときは、打った文字の列から最後の 1 単位 (英字 1 文字 / ローマ字 1 音節) を消して組み直す
                        use super::auto_english::{segment, TokenKind};
                        let tokens = segment(&comp.typed);
                        if let Some(last) = tokens.last() {
                            let remove = match last.kind {
                                TokenKind::English => last.raw.chars().last().map(|c| c.len_utf8()).unwrap_or(0),
                                TokenKind::Romaji => last.raw.len(),
                            };
                            let keep = comp.typed.len() - remove;
                            comp.typed.truncate(keep);
                        }
                        Self::apply_auto_english(&mut ipc, &mut comp, false)?;
                    } else {
                        let (hiragana, caret, raw) = ipc.edit_text_raw(Op::DeleteBackward, String::new(), 0)?;
                        comp.hiragana = apply_character_form(&hiragana);
                        comp.caret = caret;
                        comp.raw_input = raw;
                        // 英語が入っていなければ、ローマ字 1 音節を消したものとして打った文字の列も合わせる
                        // (きょ のように 1 音節が 2 文字のかなになるものは、サーバーが ょ だけ消すので対応が崩れる → 以後は判定しない)
                        if comp.typed_valid && at_end {
                            use super::auto_english::segment;
                            let tokens = segment(&comp.typed);
                            match tokens.last() {
                                Some(last) if last.raw.len() <= 2 || last.raw == "tsu" || last.raw == "shi" || last.raw == "chi" => {
                                    let keep = comp.typed.len() - last.raw.len();
                                    comp.typed.truncate(keep);
                                }
                                Some(_) => comp.typed_valid = false,
                                None => {}
                            }
                        } else {
                            comp.typed_valid = false;
                        }
                    }
                }
                ClientAction::DeleteForward => {
                    comp.typed_valid = false;
                    let (hiragana, caret) = ipc.edit_text(Op::DeleteForward, String::new(), 0)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                }
                ClientAction::MoveCaret(offset) => {
                    comp.typed_valid = false;
                    let (hiragana, caret) = ipc.edit_text(Op::MoveCursor, String::new(), *offset)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                }
                ClientAction::CaretToEdge(end) => {
                    let offset = if *end {
                        comp.hiragana_len() - comp.caret
                    } else {
                        -comp.caret
                    };
                    let (hiragana, caret) = ipc.edit_text(Op::MoveCursor, String::new(), offset)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                }
                ClientAction::Convert if comp.edit_unit => {
                    // 単位だけを変換し、後ろの文節は選んだ候補のまま並べる
                    cancel_live_refine();
                    let tail_len: usize = comp.edit_tail.iter().map(|s| s.reading.chars().count()).sum();
                    let chars: Vec<char> = comp.hiragana.chars().collect();
                    let unit = normalize_reading(&chars[..chars.len() - tail_len].iter().collect::<String>());
                    let mut segments = if unit.is_empty() { vec![] } else if comp.live {
                        build_segments(&unit, ipc.convert_with(unit.clone(), String::new(), false, false)?)
                    } else {
                        build_segments(&unit, ipc.convert(unit.clone(), String::new())?)
                    };
                    segments.extend(std::mem::take(&mut comp.edit_tail));
                    comp.edit_unit = false;
                    comp.segments = segments;
                    comp.segments_reading = composition_reading(&comp);
                    comp.refined = true;
                    comp.focus = 0;
                    comp.cursor_moved = false;
                    comp.window_open = false;
                    if comp.live && !comp.segments.is_empty() {
                        self.ensure_candidates(&mut ipc, &mut comp)?;
                        comp.window_open = true;
                    }
                }
                ClientAction::Convert if comp.live && !comp.segments.is_empty() => {
                    // ライブ変換ではもう変換済みなので、Space で候補一覧を出す
                    cancel_live_refine();
                    // 2026-10-28 (試験中): 母音の抜けを直したら、読みが変わるので下で変換し直す
                    Self::repair_typos(&mut ipc, &mut comp)?;
                    // 仮表示が読みの一部 (nn と打つ人の打ちかけの n を除いた読み) で変換したものなら、読み全体で変換し直す (るn → るん)
                    let full = live_refinement_reading(&comp, true);
                    if full != comp.segments_reading {
                        comp.refined = false;
                    }
                    // 2026-11-21 (案 3): 間に合わなかったときのつぎはぎを見せていたら、読み全体で変換し直す
                    if comp.segments.iter().any(|s| s.patchwork) {
                        comp.refined = false;
                    }
                    if short_live_input(&comp, &full) {
                        Self::offer_ai_beside_dictionary(&mut ipc, &mut comp, &full)?;
                    } else {
                        Self::ensure_refined(&mut ipc, &mut comp, true, false)?;
                    }
                    comp.focus = 0;
                    comp.cursor_moved = false;
                    // 2026-10-29: 変換できたあとで候補の一覧が間に合わなければ (Space の上限 AWAITED_LIMIT)、変換したものを見せて窓は開かない
                    // (次の Space で一覧を取り直す。確定してしまわない。再点検 N2)
                    match self.ensure_candidates(&mut ipc, &mut comp) {
                        Ok(()) => comp.window_open = true,
                        Err(error) if super::ipc_service::from_server(&error) => {
                            crate::debug_log::write(&format!("candidates not in time after converting ({error})"));
                            comp.window_open = false;
                        }
                        Err(error) => return Err(error),
                    }
                }
                ClientAction::Convert => {
                    Self::repair_typos(&mut ipc, &mut comp)?;
                    let reading = composition_reading(&comp);
                    let converted = ipc.convert(reading.clone(), String::new())?;
                    comp.segments = build_segments(&reading, converted);
                    comp.focus = 0;
                    comp.cursor_moved = false;
                    comp.window_open = false;
                }
                ClientAction::RetypeAsKana(delete, text) => {
                    // 行頭の / から日本語に戻したときだけ覚える (シェルの入力待ちから あ に戻したときの 1 文字は除く)
                    if *delete > 0 { super::slash_commands::note_switched(text); }
                    crate::debug_log::write(&format!("slash: back to kana, delete {delete}, retype {text:?}"));
                    send_retype(*delete, text);
                }
                ClientAction::CycleIvs => {
                    cancel_live_refine();
                    cancel_predict();
                    cycle_focused_ivs(&mut comp);
                }
                ClientAction::ToggleDictionaryConversion | ClientAction::ToggleNotation => {
                    cancel_live_refine();
                    cancel_predict();
                    let reading = composition_reading(&comp);
                    // 2026-10-13: Ctrl+変換 = 辞書だけの変換、Ctrl+無変換 = 表記の自動変換 (H₂O・α) を外す。別々のスイッチで、両方オフで元へ
                    let mut preview = comp.dictionary_preview.clone().filter(|preview| preview.reading == reading).unwrap_or_else(|| {
                        let mut preview = DictionaryPreview::new(reading.clone(), ConversionSnapshot::capture(&comp, &reading));
                        // 表記だけ外すときは、いま見えている変換 (打っている途中のまだ清書していない表示も含む) をもとにする
                        preview.ai = ConversionSnapshot::capture_shown(&comp, &reading).map(|shown| shown.segments);
                        preview
                    });
                    if matches!(action, ClientAction::ToggleNotation) {
                        preview.plain = !preview.plain;
                    } else {
                        preview.dictionary_on = !preview.dictionary_on;
                    }
                    if !preview.dictionary_on && !preview.plain {
                        if let Some(original) = preview.original {
                            original.restore(&mut comp);
                        } else {
                            let converted = ipc.convert(reading.clone(), String::new())?;
                            comp.segments = build_segments(&reading, converted);
                            comp.focus = 0;
                            comp.window_open = false;
                            comp.refined = true;
                            comp.segments_reading = reading;
                        }
                        comp.dictionary_preview = None;
                    } else {
                        let dictionary_on = preview.dictionary_on;
                        let slot = if dictionary_on { &mut preview.dictionary } else { &mut preview.ai };
                        let base = match slot {
                            Some(segments) => segments.clone(),
                            None => {
                                let converted = if dictionary_on {
                                    ipc.convert_with(reading.clone(), String::new(), false, true)?
                                } else {
                                    ipc.convert(reading.clone(), String::new())?
                                };
                                let segments = build_segments(&reading, converted);
                                *slot = Some(segments.clone());
                                segments
                            }
                        };
                        comp.segments = preview_segments(preview.plain, base);
                        comp.focus = 0;
                        comp.window_open = false;
                        comp.refined = true;
                        comp.segments_reading = reading.clone();
                        comp.from_prediction = false;
                        comp.dictionary_preview = Some(preview);
                    }
                }
                ClientAction::ConvertAsOne => {
                    let reading = composition_reading(&comp);
                    // ライブ変換で漢字が見えているときは、見えている表記をもとにする (無変換で漢字→ひらがな)
                    let shown = if comp.live && !comp.segments.is_empty() {
                        Some(format!("{}{}", comp.converted_text(), live_pending_suffix(&comp).unwrap_or_default()))
                    } else {
                        None
                    };
                    comp.segments = build_segments(&reading, vec![]);
                    if let (Some(shown), Some(segment)) = (shown, comp.segments.get_mut(0)) {
                        segment.text = shown;
                    }
                    comp.focus = 0;
                    comp.window_open = false;
                }
                ClientAction::ShowReading => {
                    // 清書の時計を止め、ライブ変換の文節 (と、変換中のカーソルで打っていた単位) を捨てて、読みだけを見せる (画面は下の「入力中」の形)
                    cancel_live_refine();
                    // 2026-11-19: 上限で切って裏で待っている清書も捨てる (届くと、読みを見せているのに文節が漢字に替わり、Enter で漢字が確定していた)
                    cancel_late_refine_timer();
                    super::ipc_service::drop_late_refine();
                    comp.live_reading = true;
                    comp.edit_unit = false;
                    comp.edit_tail.clear();
                    comp.segments.clear();
                    comp.segments_reading.clear();
                    comp.refined = false;
                    comp.live_settled = 0;
                    comp.focus = 0;
                    comp.window_open = false;
                }
                ClientAction::CancelConversion => {
                    comp.latin_selected = false;
                    comp.from_prediction = false;
                    comp.segments.clear();
                    comp.focus = 0;
                    comp.window_open = false;
                    let offset = comp.hiragana_len() - comp.caret;
                    if offset != 0 {
                        let (hiragana, caret) =
                            ipc.edit_text(Op::MoveCursor, String::new(), offset)?;
                        comp.hiragana = apply_character_form(&hiragana);
                        comp.caret = caret;
                    }
                }
                ClientAction::NextCandidate => {
                    self.ensure_candidates(&mut ipc, &mut comp)?;
                    let index = comp.segments[comp.focus].index;
                    if comp.window_open {
                        Self::select_candidate(&mut comp, index + 1);
                    } else {
                        comp.window_open = true;
                        let len = comp.segments[comp.focus].candidates.len() as i32;
                        Self::select_candidate(&mut comp, (index + 1).min(len - 1));
                    }
                }
                ClientAction::PrevCandidate => {
                    self.ensure_candidates(&mut ipc, &mut comp)?;
                    let index = comp.segments[comp.focus].index;
                    comp.window_open = true;
                    Self::select_candidate(&mut comp, index - 1);
                }
                ClientAction::NextPage | ClientAction::PrevPage => {
                    self.ensure_candidates(&mut ipc, &mut comp)?;
                    comp.window_open = true;
                    let segment = &comp.segments[comp.focus];
                    let len = segment.candidates.len() as i32;
                    let page = segment.index / PAGE_SIZE;
                    let index = if *action == ClientAction::NextPage {
                        ((page + 1) * PAGE_SIZE).min(len - 1)
                    } else {
                        ((page - 1) * PAGE_SIZE).max(0)
                    };
                    Self::select_candidate(&mut comp, index);
                }
                ClientAction::SelectNumber(number) => {
                    if let Some(segment) = comp.segments.get(comp.focus) {
                        let index = segment.index / PAGE_SIZE * PAGE_SIZE + number - 1;
                        if index < segment.candidates.len() as i32 {
                            Self::select_candidate(&mut comp, index);
                        }
                    }
                }
                ClientAction::MoveFocus(delta) => {
                    comp.cursor_moved = true;
                    comp.window_open = false;
                    let last = comp.segments.len().saturating_sub(1) as i32;
                    comp.focus = (comp.focus as i32 + delta).clamp(0, last) as usize;
                }
                ClientAction::FocusEdge(last) => {
                    comp.cursor_moved = true;
                    comp.window_open = false;
                    comp.focus = if *last {
                        comp.segments.len().saturating_sub(1)
                    } else {
                        0
                    };
                }
                ClientAction::Resize(delta) => {
                    comp.cursor_moved = true;
                    let reading: Vec<char> = comp
                        .segments
                        .iter()
                        .flat_map(|s| s.reading.chars())
                        .collect();
                    let start: usize = comp.segments[..comp.focus]
                        .iter()
                        .map(|s| s.reading.chars().count())
                        .sum();
                    let current = comp.segments[comp.focus].reading.chars().count() as i32;
                    let new_len = current + delta;
                    if new_len >= 1 && start + new_len as usize <= reading.len() {
                        let end = start + new_len as usize;
                        let focused_reading: String = reading[start..end].iter().collect();
                        let rest: String = reading[end..].iter().collect();
                        let context = comp.text_before_focus();

                        let candidates =
                            ipc.list_candidates(focused_reading.clone(), context.clone())?;
                        let text = candidates
                            .first()
                            .cloned()
                            .unwrap_or_else(|| focused_reading.clone());
                        let focused = Segment {
                            reading: focused_reading,
                            text: text.clone(),
                            candidates: vec![],
                            index: 0,
                            kana_cycle: vec![],
                            windows_pending: false,
                            notes: vec![],
                            ai_text: None,
                            typed_latin: None,
                            repair_readings: vec![],
                            patchwork: false,
                        };

                        comp.segments.truncate(comp.focus);
                        comp.segments.push(focused);
                        if !rest.is_empty() {
                            let converted = ipc.convert(rest.clone(), format!("{context}{text}"))?;
                            comp.segments.extend(build_segments(&rest, converted));
                        }
                        comp.window_open = false;
                    }
                }
                ClientAction::ConvertType(set_type) => {
                    let whole = comp.segments.len() == 1;
                    let focused_typed = typed_for_focus(&comp);
                    // 英字にするときは打った文字の列を使う (サーバーの列は n+子音 を ん に書き換えている)。
                    // 文節ごとなら読みをローマ字に戻す (けltype → keltype)
                    let raw_input = if comp.typed_valid && !comp.typed.is_empty() { comp.typed.clone() } else { comp.raw_input.clone() };
                    if let Some(segment) = comp.segments.get_mut(comp.focus) {
                        let romaji = to_romaji(&segment.reading, &super::auto_english::spelling());
                        let raw = focused_typed.as_deref().or_else(|| if whole { Some(raw_input.as_str()) } else { Some(romaji.as_str()) });
                        segment.text = type_text(*set_type, &segment.reading, raw);
                    }
                    comp.latin_selected = matches!(set_type, SetTextType::FullLatin | SetTextType::HalfLatin);
                    comp.latin_learning_intent = comp.latin_selected;
                    comp.window_open = false;
                }
                ClientAction::TexEnterMath => super::tex::enter_inline_math(),
                ClientAction::CommitSuffix(suffix) => comp.commit_suffix = suffix.clone(),
                ClientAction::ShowAccents(c) => {
                    if comp.begin_accents(*c, &super::diacritics::variants(*c)).is_some() {
                        super::diacritics::consume_hold(*c);
                        Self::preview_accent(&mut ipc, &mut comp, 1)?;
                    }
                }
                ClientAction::MoveAccent(delta) => {
                    if let Some(base) = comp.accent {
                        let len = super::diacritics::variants(base).len() as i32;
                        if len > 0 {
                            comp.accent_index = if comp.accent_index < 0 {
                                if *delta > 0 { 0 } else { len - 1 }
                            } else {
                                (comp.accent_index + delta).rem_euclid(len)
                            };
                            let number = comp.accent_index + 1;
                            Self::preview_accent(&mut ipc, &mut comp, number)?;
                        }
                    }
                }
                ClientAction::PickCurrentAccent => {
                    // 何も選んでいなければ閉じるだけ (打った文字のまま)
                    if comp.accent_index < 0 {
                        comp.close_accents();
                        ipc.hide_window()?;
                    } else {
                        let number = comp.accent_index + 1;
                        self.pick_accent(&mut ipc, &mut comp, number)?;
                    }
                }
                ClientAction::CloseAccents => {
                    comp.close_accents();
                    ipc.hide_window()?;
                }
                ClientAction::PickAccent(number) => self.pick_accent(&mut ipc, &mut comp, *number)?,
                ClientAction::UndoCommit => {
                    if let Some(last) = comp.last_commit.take() {
                        if self.start_composition_over_previous(&last.text)? {
                            comp.reset();
                            comp.segments = last.segments;
                            comp.focus = 0;
                            comp.window_open = false;
                            comp.display = last.text;
                            // サーバー側の読みも戻す (Esc で入力中に戻ったときに使う)
                            ipc.clear_text()?;
                            let (hiragana, caret) = ipc.edit_text(Op::Insert, last.reading, 0)?;
                            comp.hiragana = apply_character_form(&hiragana);
                            comp.caret = caret;
                            comp.state = CompositionState::Converting;
                        }
                    }
                }
                ClientAction::UndoAutocorrect => {
                    // 2026-11-05 (仕事 24): 直した字 (キャレットの直前) を打ったとおりの字に入れ替え、その綴りを次から直さない。
                    // キャレットの前が直した字と合わない (動かした・読めない) ときは何もせず、Backspace をアプリへ渡したのと同じにはできないので次の打鍵に任せる
                    if let Some((corrected, original, typed)) = comp.last_autocorrect.take() {
                        if self.start_composition_over_previous(&corrected)? {
                            self.set_text(&original, "")?;
                            self.end_composition()?;
                            if may_learn() {
                                for word in &typed { super::latin_spell::decline(word); }
                            }
                            // 2026-11-08 (仕事 12): 英語の句の空白を戻したら、この IME の間は同じ並びを分けない (ファイルには書かない)
                            for run in std::mem::take(&mut comp.last_phrase_runs) { super::auto_english::decline_phrase(&run); }
                            comp.last_commit = None;
                            super::next_word::note_commit(None);
                            super::next_word::note_commit(Some(&original));
                        }
                    }
                }
                ClientAction::UndoTypoRepair => {
                    // 2026-10-28 (試験中): 直した直後の Shift+BackSpace。打ったとおりに戻して変換し直し、その綴りは次から直さない
                    if let Some(repair) = comp.typo_repair.take() {
                        if may_learn() {
                            super::auto_english::decline_typo_repair(&repair.keys);
                        }
                        comp.typo_repair_declined = true;
                        comp.typed = repair.original_typed;
                        Self::rebuild_from_typed(&mut ipc, &mut comp)?;
                        cancel_live_refine();
                        if comp.live {
                            comp.refined = false;
                            Self::ensure_refined(&mut ipc, &mut comp, true, false)?;
                        } else {
                            let reading = composition_reading(&comp);
                            let converted = ipc.convert(reading.clone(), String::new())?;
                            comp.segments = build_segments(&reading, converted);
                        }
                        comp.focus = 0;
                        comp.cursor_moved = false;
                        comp.window_open = false;
                    }
                }
                ClientAction::PunctuationConvert => {
                    // 2026-10-28 (試験中): Space と同じに変換するが、候補の窓は開かない。次の字で確定して新しい入力になるので、
                    // 入力中は句読点の区切りまでになり (長い入力の遅さ・ピコピコが前の区切りに及ばない)、次の AI の左の文脈に前の区切りが入る
                    cancel_live_refine();
                    Self::repair_typos(&mut ipc, &mut comp)?;
                    let reading = if comp.live { live_refinement_reading(&comp, true) } else { composition_reading(&comp) };
                    // 2026-10-29: AI の変換は 1.5 秒まで。AI が遅くてこの長さの読みが 1.5 秒で変換できそうにない PC と、その場の上限の残りが
                    // 1.5 秒より少ないときは、はじめから辞書だけの変換にする (CPU の AI では、、。 のたびに何秒も止まった)。Space を押せば AI で変換し直す
                    let converted = if comp.live && comp.refined && comp.segments_reading == reading {
                        Some((comp.segments.clone(), true))
                    } else if super::ipc_service::ai_while_typing(reading.chars().count()) && !short_live_input(&comp, &reading) {
                        let _narrow = super::ipc_service::NarrowBudget::within(std::time::Duration::from_millis(1500));
                        ipc.convert_with(reading.clone(), String::new(), false, false).ok().map(|c| (build_segments(&reading, c), true))
                    } else {
                        ipc.convert_with(reading.clone(), String::new(), true, true).ok().map(|c| (build_segments(&reading, c), false))
                    };
                    // 間に合わなければ問い合わせ直さない (サーバーはまだ切った AI の変換をしていて、辞書の変換もその後ろで待つ。レビュー M2)。
                    // 見えている辞書の仮表示の後ろに増えたのが句読点だけなら、句読点を足して変換したことにする
                    let shown = if converted.is_none() { preview_with_tail(&comp, &reading) } else { None };
                    let punctuation_only = reading.strip_prefix(comp.segments_reading.as_str())
                        .is_some_and(|tail| !tail.chars().any(char::is_alphanumeric));
                    match converted.or(shown.clone().filter(|_| punctuation_only).map(|segments| (segments, false))) {
                        Some((segments, refined)) => {
                            comp.segments = segments;
                            comp.segments_reading = reading;
                            comp.refined = refined;
                            comp.focus = 0;
                            comp.cursor_moved = false;
                            comp.window_open = false;
                            comp.punctuation_converted = true;
                        }
                        None => {
                            // 変換せず、句読点を入れただけで入力を続ける。ライブ変換なら、見せるもの (仮表示の後ろに増えた読みをかなのまま足したもの、
                            // 仮表示が読みの頭と合わなければ読み全体のかな) を今の読みのものにして、描画で問い合わせ直さないようにする (再点検 R1)
                            crate::debug_log::write("punctuation convert: no conversion in time; keeping the input");
                            if comp.live {
                                comp.segments = shown.unwrap_or_else(|| build_segments(&reading, vec![]));
                                comp.segments_reading = reading;
                            } else {
                                comp.segments.clear();
                                comp.segments_reading.clear();
                            }
                            comp.refined = false;
                            keep_composing = true;
                        }
                    }
                }
                ClientAction::ForgetCandidate => {
                    // 2026-10-29: 選んでいる候補が学習した語なら忘れ (変換サーバーが学習の表をその場で書き直す)、一覧から外して次の候補を選ぶ。
                    // 辞書の語 (忘れた語が 0) は何もしない。予測の候補の控えからも、その語を含むものを消す
                    let target = comp.segments.get(comp.focus).and_then(|segment| {
                        let index = usize::try_from(segment.index).ok()?;
                        let text = segment.candidates.get(index)?.clone();
                        Some((index, learning_reading(segment), text))
                    });
                    if let Some((index, reading, text)) = target {
                        // 変換サーバーとのやり取りの失敗 (Forget の無い古い変換サーバー・返事が無い) は「忘れなかった」として扱う
                        // (動作ごと失敗させると、見えている変換を確定してしまう)
                        let forgotten = match ipc.forget(reading.clone(), text.clone()) {
                            Ok(forgotten) => forgotten,
                            Err(error) if super::ipc_service::from_server(&error) => {
                                crate::debug_log::write(&format!("forget failed: {error}"));
                                0
                            }
                            Err(error) => return Err(error),
                        };
                        if forgotten > 0 {
                            crate::debug_log::write("forgot a learned candidate");
                            super::predict::forget(&reading, &text);
                            if let Some(segment) = comp.segments.get_mut(comp.focus) {
                                segment.candidates.remove(index);
                                segment.notes.retain(|(candidate, _)| candidate != &text);
                                if segment.ai_text.as_deref() == Some(text.as_str()) {
                                    segment.ai_text = None;
                                }
                                if segment.candidates.is_empty() {
                                    segment.text = segment.reading.clone();
                                    segment.index = 0;
                                } else {
                                    let next = index.min(segment.candidates.len() - 1) as i32;
                                    Self::select_candidate(&mut comp, next);
                                }
                            }
                        }
                    }
                }
                ClientAction::OpenSingleKanji => {
                    let context = comp.text_before_focus();
                    if let Some(segment) = comp.segments.get_mut(comp.focus) {
                        let list = ipc.list_candidates_with(segment.reading.clone(), context, true)?;
                        if !list.is_empty() {
                            segment.text = list[0].clone();
                            segment.candidates = list;
                            segment.index = 0;
                            comp.window_open = true;
                        }
                    }
                }
                ClientAction::ShowPredictions => {
                    let reading = composition_reading(&comp);
                    // 2026-11-05 (仕事 27): 覚えてはいけない欄 (IS_PRIVATE・管理者権限・隔離されたアプリ) では出さない (履歴を読まない)
                    let started = std::time::Instant::now();
                    comp.predictions = if may_learn() {
                        let conversions = windows_candidates(&reading);
                        super::predict::predict(&reading, &conversions)
                    } else {
                        vec![]
                    };
                    comp.predicted_for = comp.hiragana.clone();
                    // 予測を引く時間 (ライブ変換の清書の時間と比べるため。記録を有効にしたときだけ書く)
                    crate::debug_log::write(&format!("predict {}ms live={} n={} len={}", started.elapsed().as_millis(), comp.live,
                        comp.predictions.len(), reading.chars().count()));
                }
                ClientAction::CompleteLatinWord(index) => {
                    cancel_latin_suggest();
                    if let Some(mut state) = super::next_word::latin_state() {
                        let current = state.pick.and_then(|i| state.list.get(i).cloned()).unwrap_or_else(|| state.partial.clone());
                        let target = state.list.get(*index).cloned();
                        // キャレットの前がまだ打ちかけ (か補った語) のままなら置き換える。違っていれば何もしない
                        match target {
                            Some(target) if self.start_composition_over_previous(&current)? => {
                                self.set_text(&target, "")?;
                                self.end_composition()?;
                                state.pick = Some(*index);
                                ipc.set_candidates(latin_candidate_list(&state))?;
                                ipc.set_selection(*index as i32)?;
                                super::next_word::set_latin_state(Some(state));
                                self.update_pos_at_selection();
                                ipc.show_window()?;
                            }
                            _ => {
                                super::next_word::set_latin_state(None);
                                ipc.hide_window()?;
                            }
                        }
                    }
                }
                ClientAction::CompleteNextWord(index) => {
                    // 打った文字の列を候補の語に置き換えて組み直す (自分で打ったときと同じ扱いになる)
                    if let Some(target) = comp.next_words.get(*index).cloned() {
                        comp.typed = target;
                        comp.typed_valid = true;
                        Self::apply_auto_english(&mut ipc, &mut comp, false)?;
                        comp.next_word_pick = Some(*index);
                    }
                }
                ClientAction::UsePrediction(forward) => {
                    if comp.predictions.is_empty() {
                        let reading = composition_reading(&comp);
                        let converted = ipc.convert(reading.clone(), String::new())?;
                        comp.segments = build_segments(&reading, converted);
                        comp.focus = 0;
                        comp.window_open = false;
                    } else {
                        let index = if *forward { 0 } else { comp.predictions.len() as i32 - 1 };
                        comp.segments = vec![Segment {
                            reading: composition_reading(&comp),
                            text: comp.predictions[index as usize].clone(),
                            candidates: comp.predictions.clone(),
                            index,
                            kana_cycle: vec![],
                            windows_pending: false,
                            notes: vec![],
                            ai_text: None,
                            typed_latin: None,
                            repair_readings: vec![],
                            patchwork: false,
                        }];
                        comp.focus = 0;
                        comp.window_open = true;
                        comp.from_prediction = true;
                    }
                }
                ClientAction::RefineLive => {
                    // カーソルが途中にあるあいだは清書しない (左右を別々に辞書で変換して見せている。2026-10-14)
                    // 2026-10-29: 清書も 1.5 秒まで (句読点での変換と同じ。前はその場の上限の 3 秒まで UI スレッドが止まった)。間に合わなければ
                    // 辞書の仮表示のまま (遅い印が付き、この長さではしばらく清書しない。再点検 N1)
                    let reading = live_refinement_reading(&comp, false);
                    if comp.caret >= comp.hiragana_len() && !comp.edit_unit && super::ipc_service::ai_while_typing(reading.chars().count())
                        && !short_live_input(&comp, &reading) {
                        // 2026-11-08 (ただし伊豆の件): 清書を走らせたら、字数・かかった時間・表示が替わったかだけをログに書く (字は書かない)
                        let started = std::time::Instant::now();
                        let before = comp.converted_text();
                        let _narrow = super::ipc_service::NarrowBudget::within(std::time::Duration::from_millis(1500));
                        if let Err(error) = Self::ensure_refined(&mut ipc, &mut comp, false, true) {
                            if !super::ipc_service::from_server(&error) {
                                return Err(error);
                            }
                            crate::debug_log::write(&format!("live refine: not in time ({error})"));
                            // 2026-11-06 (仕事 29 の 2): 切った問い合わせを裏で待っていれば、届いたかを時計で見る
                            arm_late_refine_timer();
                        } else {
                            crate::debug_log::write(&format!("live refine: ran (chars={}, ms={}, changed={})",
                                reading.chars().count(), started.elapsed().as_millis(), comp.converted_text() != before));
                        }
                    } else if comp.caret < comp.hiragana_len() || comp.edit_unit {
                        crate::debug_log::write(&format!("live refine: skipped (caret in the middle={}, editing a segment={})",
                            comp.caret < comp.hiragana_len(), comp.edit_unit));
                    }
                }
                ClientAction::ApplyLateRefine => {
                    if let Some((reading, converted)) = LATE_RESULT.with(|r| r.borrow_mut().take()) {
                        if late_refine_applies(&comp, &reading) {
                            crate::debug_log::write("late refine: applied");
                            set_refined(&mut comp, reading, converted);
                        }
                    }
                }
                ClientAction::SwitchKanaType => {
                    comp.latin_learning_intent = false;
                    let typed = typed_for_focus(&comp);
                    if let Some(segment) = comp.segments.get_mut(comp.focus) {
                        let (text, cycle) =
                            next_kana_type(&segment.text, &segment.reading, typed.as_deref(), &segment.kana_cycle);
                        segment.text = text;
                        segment.kana_cycle = cycle;
                    }
                    comp.latin_selected = selected_literal_latin(&comp);
                    comp.window_open = false;
                }
                ClientAction::CommitBeforeCursor => {
                    // カーソルより左の文節を確定し、右側の読みをかなの入力に戻してカーソルを頭に置く (次の Insert がそこに入る)
                    if comp.converted_text().chars().any(|c| !c.is_ascii()) {
                        record_romaji_style(&mut comp);
                    }
                    let cut = (comp.focus + 1).min(comp.segments.len());
                    let left: Vec<Segment> = comp.segments.drain(..cut).collect();
                    if may_learn() {
                        let _ = ipc.learn(left.iter().map(learning_reading).collect(), left.iter().map(|s| s.text.clone()).collect());
                    }
                    let left_text: String = left.iter().map(|s| s.text.as_str()).collect();
                    let rest_reading: String = comp.segments.iter().map(|s| s.reading.as_str()).collect();
                    let rest_text = comp.converted_text();
                    self.set_preedit(&format!("{left_text}{rest_text}"), &[], 0)?;
                    self.shift_start(&left_text, &rest_reading)?;
                    // 打つ文字と直後の文節を 1 つの変換の単位にし、その後ろの文節は変換をそのまま残す
                    comp.edit_tail = if comp.segments.len() > 1 { comp.segments.drain(1..).collect() } else { vec![] };
                    comp.edit_unit = true;
                    comp.segments.clear();
                    comp.segments_reading.clear();
                    comp.refined = false;
                    comp.focus = 0;
                    comp.cursor_moved = false;
                    comp.window_open = false;
                    comp.typed.clear();
                    comp.typed_valid = false;
                    comp.raw_input.clear();
                    ipc.clear_text()?;
                    let (hiragana, caret) = ipc.edit_text(Op::Insert, rest_reading, 0)?;
                    let (hiragana, caret) = if caret > 0 { ipc.edit_text(Op::MoveCursor, String::new(), -caret)? } else { (hiragana, caret) };
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                    comp.state = CompositionState::Composing;
                }
                ClientAction::DeleteAtCursor => {
                    self.delete_at_convert_cursor(&mut ipc, &mut comp)?;
                }
                ClientAction::CommitFirstSegment => {
                    // 文節ごとに確定するときは、最初の確定で入力全体の綴りを数える (読みが残りの部分だけになる前に)
                    if comp.converted_text().chars().any(|c| !c.is_ascii()) {
                        record_romaji_style(&mut comp);
                    }
                    // 1 文節しかないときは、ループの頭で EndComposition に替えてある
                    let first = comp.segments.remove(0);
                    if may_learn() {
                        let _ = ipc.learn(vec![learning_reading(&first)], vec![first.text.clone()]);
                    }
                    let rest = comp.converted_text();
                    self.set_preedit(&format!("{}{}", first.text, rest), &[], 0)?;
                    self.shift_start(&first.text, &rest)?;
                    comp.focus = comp.focus.saturating_sub(1);
                    comp.window_open = false;
                    // サーバー側の読みも残りの部分だけにする
                    let reading: String = comp.segments.iter().map(|s| s.reading.as_str()).collect();
                    ipc.clear_text()?;
                    let (hiragana, caret) = ipc.edit_text(Op::Insert, reading, 0)?;
                    comp.hiragana = apply_character_form(&hiragana);
                    comp.caret = caret;
                    comp.raw_input.clear();
                }
                ClientAction::SetIMEMode(mode) => {
                    super::auto_english::set_english_sentence(false);
                    self.start_composition()?;
                    self.update_pos()?;
                    self.end_composition()?;

                    super::latin_learn::mode_changed();
                    hide_latin_suggestions();
                    let mut ime_state = IMEState::get()?;
                    ime_state.input_mode = mode.clone();
                    drop(ime_state);

                    // update the language bar
                    self.update_lang_bar()?;

                    let mode = match mode {
                        InputMode::Latin => "A",
                        InputMode::Kana => "あ",
                    };

                    ipc.set_input_mode(mode)?;
                    comp.reset();
                    comp.state = CompositionState::None;
                    ipc.clear_text()?;
                }
            }

            // 各アクションのあとの状態 (EndComposition の直後に Insert が来る場合などに必要)
            comp.state = match action {
                ClientAction::StartComposition
                | ClientAction::Insert(_)
                | ClientAction::CancelConversion => CompositionState::Composing,
                ClientAction::Convert | ClientAction::ToggleDictionaryConversion | ClientAction::ToggleNotation | ClientAction::ConvertAsOne => CompositionState::Converting,
                _ => comp.state.clone(),
            };
        }

        drop(awaited);
        comp.state = transition;
        if keep_composing && comp.state == CompositionState::Converting {
            comp.state = CompositionState::Composing;
        }
        // 未確定文字列が無いのに変換中扱いになっていたら戻す (確定の取り消しができなかった場合など)
        if comp.state != CompositionState::None {
            let has_composition = self
                .borrow()
                .ok()
                .and_then(|t| t.borrow_composition().ok().map(|c| c.tip_composition.is_some()))
                .unwrap_or(false);
            if !has_composition {
                comp.state = CompositionState::None;
            }
        }
        if comp.state == CompositionState::Converting && comp.segments.is_empty() {
            comp.state = CompositionState::Composing;
        }
        // 変換の単位で入力している間に、読みの後ろが残した文節と合わなくなった・カーソルが単位の外へ出たら、普通の入力に戻す
        if comp.edit_unit {
            let tail_reading: String = comp.edit_tail.iter().map(|s| s.reading.as_str()).collect();
            let unit_len = comp.hiragana_len() - tail_reading.chars().count() as i32;
            if comp.state != CompositionState::Composing || !comp.hiragana.ends_with(&tail_reading) || comp.caret > unit_len {
                comp.edit_unit = false;
                comp.edit_tail.clear();
                comp.segments.clear();
                comp.segments_reading.clear();
            }
        }

        // 候補ウィンドウは、変換中は注目文節の先頭に合わせる (文節を左右に移動したときにカーソルも動くように)。
        // キャレットは、←→ で動かしていなければ未確定文字列の右端 (文字キーで確定)、動かしたら注目文節の右端 (そこで編集できる。2026-10-14)
        let focus_start: i32 = comp.segments.iter().take(comp.focus).map(|s| utf16_len(&s.text)).sum();
        let focus_cells: i32 = comp.segments.iter().take(comp.focus).flat_map(|s| s.text.chars()).map(terminal_cells).sum();
        if let Ok(mut text_service) = self.borrow_mut() {
            text_service.candidate_anchor = (comp.state == CompositionState::Converting).then_some((focus_start, focus_cells));
        }

        // 2026-11-05 (仕事 23): F9・F10 で英字を選んだら、そのあとは打ったとおりに見せる
        super::full_width::set_digit_display(digit_form_for(&comp, &mode));
        // 画面に反映する
        match comp.state {
            CompositionState::Composing if Self::english_only(&comp)
                && (!comp.hiragana.chars().any(|c| c.is_ascii_digit())
                    || !comp.hiragana.chars().any(is_latin_letter)) => {
                // English has no kana conversion to refine; stale live prefixes must not replace it.
                cancel_live_refine();
                comp.segments.clear();
                comp.segments_reading.clear();
                let text = comp.hiragana.clone();
                let len = utf16_len(&text);
                self.set_preedit(&text, &[PreeditSpan { start: 0, len, kind: SpanKind::Input }], len)?;
                comp.display = text;
                // 2026-10-19: 次の語の候補 (Tab で補ったあとは、同じ一覧で補った語を選んだ状態にしておく)
                let picked = comp.next_word_pick.filter(|&i| comp.next_words.get(i) == Some(&comp.hiragana));
                if picked.is_none() {
                    comp.next_word_pick = None;
                    if comp.next_words_for != comp.hiragana {
                        comp.next_words = Self::next_word_candidates(&comp);
                        comp.next_words_for = comp.hiragana.clone();
                    }
                }
                if comp.next_words.is_empty() {
                    ipc.hide_window()?;
                } else {
                    let list = comp.next_words.iter().enumerate()
                        .map(|(i, w)| if i == 0 && picked.is_none() { format!("{w}\u{1f}Tab") } else { format!("{w}\u{1f}") })
                        .collect();
                    let failures = super::ipc_service::ui_failures();
                    ipc.set_candidates(list)?;
                    ipc.set_selection(picked.map_or(-1, |i| i as i32))?;
                    self.update_pos()?;
                    ipc.show_window()?;
                    // 2026-10-29: 候補窓に頼めなかったら、見えていない候補を Tab で使わない (再点検 R2)
                    if super::ipc_service::ui_failures() != failures {
                        comp.next_words.clear();
                        comp.next_word_pick = None;
                    }
                }
            }
            // 2026-10-14: 変換中のカーソルで打った文字 + 直後の文節を 1 つの単位として変換して見せる (の + なまえは = の名前は)。後ろの文節はそのまま。
            // ライブ変換でなければ単位はかなのまま。カーソルは、単位の変換で打った文字の後ろに当たる文節の切れ目に置く
            CompositionState::Composing if comp.edit_unit => {
                cancel_live_refine();
                let chars: Vec<char> = comp.hiragana.chars().collect();
                let tail_text: String = comp.edit_tail.iter().map(|s| s.text.as_str()).collect();
                let tail_len: usize = comp.edit_tail.iter().map(|s| s.reading.chars().count()).sum();
                let unit_len = chars.len() - tail_len;
                let caret = (comp.caret.max(0) as usize).min(unit_len);
                let before: String = chars[..caret].iter().collect();
                let after: String = chars[caret..unit_len].iter().collect();
                let pending_len = before.chars().rev().take_while(|c| c.is_ascii_alphabetic()).count();
                let before_kana: String = before.chars().take(caret - pending_len).collect();
                let pending: String = before.chars().skip(caret - pending_len).collect();
                let (unit_left, unit_right) = if comp.live {
                    let unit = normalize_reading(&format!("{before_kana}{after}"));
                    let key = format!("{unit}\u{2}");
                    if comp.segments_reading != key {
                        comp.segments = if unit.is_empty() { vec![] } else { build_segments(&unit, ipc.convert_with(unit.clone(), String::new(), true, true)?) };
                        comp.segments_reading = key;
                        comp.refined = false;
                    }
                    // 打った文字の後ろ (読みの位置) を含む文節の終わりまでを左に
                    let boundary = normalize_reading(&before_kana).chars().count();
                    let mut read = 0;
                    let mut left_count = 0;
                    for s in &comp.segments {
                        if read >= boundary { break; }
                        read += s.reading.chars().count();
                        left_count += 1;
                    }
                    (comp.segments[..left_count].iter().map(|s| s.text.as_str()).collect::<String>(),
                     comp.segments[left_count..].iter().map(|s| s.text.as_str()).collect::<String>())
                } else {
                    comp.segments.clear();
                    (before_kana.clone(), after.clone())
                };
                comp.focus = 0;
                comp.window_open = false;
                let text = format!("{unit_left}{pending}{unit_right}{tail_text}");
                let (l, p, r) = (utf16_len(&unit_left), utf16_len(&pending), utf16_len(&unit_right));
                let draft = if comp.live { SpanKind::LiveDraft } else { SpanKind::Input };
                let spans = [
                    PreeditSpan { start: 0, len: l, kind: draft },
                    PreeditSpan { start: l, len: p, kind: SpanKind::Input },
                    PreeditSpan { start: l + p, len: r, kind: draft },
                    PreeditSpan { start: l + p + r, len: utf16_len(&tail_text), kind: SpanKind::Converted },
                ];
                self.set_preedit(&text, &spans, l + p)?;
                comp.display = text;
                ipc.hide_window()?;
            }
            // 2026-10-14: ライブ変換でカーソルが途中にあるとき (変換中のカーソルで文字を入れたあとなど) は、カーソルの左と右を別々に辞書で変換して、
            // そのあいだにカーソルを見せる (の｜名前は中野です)。手が止まっても清書はしない (Space で全体を変換し直す)
            CompositionState::Composing if comp.live && !comp.live_reading && !comp.hiragana.is_empty() && comp.caret < comp.hiragana_len() => {
                cancel_live_refine();
                let chars: Vec<char> = comp.hiragana.chars().collect();
                let caret = (comp.caret.max(0) as usize).min(chars.len());
                let left_raw: String = chars[..caret].iter().collect();
                // カーソルの直前のローマ字の打ちかけ (n・k …) は変換せずにそのまま見せる
                let pending_len = left_raw.chars().rev().take_while(|c| c.is_ascii_alphabetic()).count();
                let left_kana: String = left_raw.chars().take(caret - pending_len).collect();
                let pending: String = left_raw.chars().skip(caret - pending_len).collect();
                let left = normalize_reading(&left_kana);
                let right = normalize_reading(&chars[caret..].iter().collect::<String>());
                let key = format!("{left}\u{1}{right}");
                if comp.segments_reading != key || comp.segments.is_empty() {
                    let mut segments = vec![];
                    if !left.is_empty() {
                        segments.extend(build_segments(&left, ipc.convert_with(left.clone(), String::new(), true, true)?));
                    }
                    let left_text: String = segments.iter().map(|s: &Segment| s.text.as_str()).collect();
                    if !right.is_empty() {
                        segments.extend(build_segments(&right, ipc.convert_with(right.clone(), left_text, true, true)?));
                    }
                    comp.segments = segments;
                    comp.segments_reading = key;
                    comp.refined = false;
                }
                comp.focus = 0;
                comp.window_open = false;
                let left_count = comp.segments.iter().scan(0usize, |n, s| { *n += s.reading.chars().count(); Some(*n) })
                    .take_while(|&n| n <= left.chars().count()).count();
                let left_text: String = comp.segments[..left_count].iter().map(|s| s.text.as_str()).collect();
                let right_text: String = comp.segments[left_count..].iter().map(|s| s.text.as_str()).collect();
                let text = format!("{left_text}{pending}{right_text}");
                let spans = [
                    PreeditSpan { start: 0, len: utf16_len(&left_text), kind: SpanKind::LiveDraft },
                    PreeditSpan { start: utf16_len(&left_text), len: utf16_len(&pending), kind: SpanKind::Input },
                    PreeditSpan { start: utf16_len(&left_text) + utf16_len(&pending), len: utf16_len(&right_text), kind: SpanKind::LiveDraft },
                ];
                self.set_preedit(&text, &spans, utf16_len(&left_text) + utf16_len(&pending))?;
                comp.display = text;
                ipc.hide_window()?;
            }
            CompositionState::Composing if comp.live && !comp.live_reading && !comp.hiragana.is_empty() => {
                // ん を nn と打つ人 (kunrei_nn) は、1 回目の n を「n」のまま見せる (どちらも「ん」だと何回打ったか分からない)
                let nn_pending = super::auto_english::spelling().nn && ends_with_pending_n(&comp.hiragana);
                let reading = if nn_pending {
                    let mut without_n = comp.hiragana.clone();
                    without_n.pop();
                    normalize_reading(&without_n)
                } else {
                    composition_reading(&comp)
                };
                let pending = if nn_pending {
                    Some("n".to_string())
                } else {
                    live_pending_suffix(&comp).filter(|p| !p.is_empty() && !p.ends_with('n'))
                };
                let mut drafted = false;
                if reading.is_empty() {
                    comp.segments.clear();
                    comp.segments_reading.clear();
                } else if (pending.is_none() || nn_pending) && (comp.segments.is_empty() || comp.segments_reading != reading) {
                    // 打鍵中は辞書だけで素早く仮表示し、手が止まったら Zenzai で変換し直す
                    Self::draft_live(&mut ipc, &mut comp, &reading)?;
                    drafted = true;
                }
                if arms_live_refine(drafted, comp.refined, !comp.segments.is_empty(), from_timer) {
                    self.schedule_live_refine();
                }
                comp.focus = 0;
                comp.window_open = false;
                // ローマ字の打ちかけは変換し直さず、変換済みの後ろにそのまま付ける
                let text = format!("{}{}", comp.converted_text(), pending.unwrap_or_default());
                // 仮表示は点線、Zenzai で清書したあとは細い実線 (ローマ字の打ちかけは波線)
                let converted_len = utf16_len(&comp.converted_text());
                let spans = [
                    PreeditSpan {
                        start: 0,
                        len: converted_len,
                        kind: if comp.refined { SpanKind::Converted } else { SpanKind::LiveDraft },
                    },
                    PreeditSpan {
                        start: converted_len,
                        len: utf16_len(&text) - converted_len,
                        kind: SpanKind::Input,
                    },
                ];
                self.set_preedit(&text, &spans, utf16_len(&text))?;
                comp.display = text;
                // 2026-11-05 (仕事 27): ライブ変換の中でも、手が止まったら日本語の予測候補を出す (設定 input.japanese_predictions)。
                // 予測は読み (hiragana) で引くので、清書で見た目が替わっても読みが同じあいだは残す
                if comp.predicted_for != comp.hiragana {
                    comp.predictions.clear();
                    self.schedule_predict();
                }
                self.show_prediction_window(&mut ipc, &mut comp)?;
            }
            CompositionState::Composing => {
                let text = comp.hiragana.clone();
                let caret: String = text.chars().take(comp.caret.max(0) as usize).collect();
                let spans = [PreeditSpan {
                    start: 0,
                    len: utf16_len(&text),
                    kind: SpanKind::Input,
                }];
                self.set_preedit(&text, &spans, utf16_len(&caret))?;
                comp.display = text;
                // 予測変換 (通常版): 読みが変わったら候補を消し、手が止まったら出し直す
                if comp.predicted_for != comp.hiragana {
                    comp.predictions.clear();
                    self.schedule_predict();
                }
                self.show_prediction_window(&mut ipc, &mut comp)?;
            }
            CompositionState::Converting => {
                let mut spans = vec![];
                let mut start = 0;
                for (i, segment) in comp.segments.iter().enumerate() {
                    let len = utf16_len(&segment.text);
                    spans.push(PreeditSpan {
                        start,
                        len,
                        kind: if i == comp.focus { SpanKind::Focused } else { SpanKind::Converted },
                    });
                    start += len;
                }
                let text = comp.converted_text();
                // カーソル: 動かしていなければ全体の右端、動かしたら注目文節の右端 (そこで打つと入り、Backspace で消える)
                let caret = if convert_cursor_at_end(&comp) { utf16_len(&text) }
                    else { focus_start + comp.segments.get(comp.focus).map_or(0, |s| utf16_len(&s.text)) };
                self.set_preedit(&text, &spans, caret)?;
                comp.display = text;

                if comp.window_open {
                    let segment = &comp.segments[comp.focus];
                    let mut annotated = annotated_candidates(segment);
                    // 2026-11-05 (仕事 23): 候補の窓も、未確定の文字列と同じ数字の形で見せる (選んだ候補は半角のまま持つ)
                    if super::full_width::digit_display() != super::full_width::DigitForm::Off {
                        for item in annotated.iter_mut() {
                            let (text, note) = item.split_once('\u{1f}').map_or((item.as_str(), None), |(t, n)| (t, Some(n)));
                            *item = match note {
                                Some(note) => format!("{}\u{1f}{note}", super::full_width::displayed_digits(text)),
                                None => super::full_width::displayed_digits(text),
                            };
                        }
                    }
                    let failures = super::ipc_service::ui_failures();
                    // 2026-11-08 (仕事 52): 重いので AI を休んでいる間は、候補の窓の下に文言を添える
                    ipc.set_candidates_with_note(annotated, ai_pause_note())?;
                    ipc.set_selection(segment.index)?;
                    self.update_pos()?;
                    ipc.show_window()?;
                    // 2026-10-29: 候補窓に頼めなかったら (返事が無い・上限で問い合わせなかった)、開いていない扱いにする
                    // (数字キーが見えていない一覧から選ばないように。次の Space で頼み直す。レビュー L3)
                    if super::ipc_service::ui_failures() != failures {
                        comp.window_open = false;
                    }
                } else {
                    ipc.hide_window()?;
                }
            }
            CompositionState::None => {}
        }

        // 長押しの候補 (アクセント付きの文字)
        if let Some(base) = comp.accent {
            let list: Vec<String> = super::diacritics::variants(base).iter().map(|c| format!("{c}\u{1f}")).collect();
            ipc.set_candidates(list)?;
            ipc.set_selection(comp.accent_index)?;
            if comp.state == CompositionState::None {
                self.update_pos_at_selection();
            } else {
                self.update_pos()?;
            }
            ipc.show_window()?;
        }

        // tip_composition は start/end_composition が直接書き換えるので、それ以外を書き戻す
        let text_service = self.borrow()?;
        let mut composition = text_service.borrow_mut_composition()?;
        composition.hiragana = comp.hiragana;
        composition.caret = comp.caret;
        composition.raw_input = comp.raw_input;
        composition.segments = comp.segments;
        composition.focus = comp.focus;
        composition.cursor_moved = comp.cursor_moved;
        composition.edit_unit = comp.edit_unit;
        composition.edit_tail = comp.edit_tail;
        composition.window_open = comp.window_open;
        composition.display = comp.display;
        composition.live = comp.live;
        composition.live_reading = comp.live_reading;
        composition.last_commit = comp.last_commit;
        composition.english_active = comp.english_active;
        composition.latin_selected = comp.latin_selected;
        composition.latin_learning_intent = comp.latin_learning_intent;
        composition.typed = comp.typed;
        // 2026-10-28: 入力を始めたときのキャレットの直前の字も残す (書き戻していなかったので、2 を Enter で確定してから .5 と打つと、5 を打ったときには
        // 直前の 2 が分からず 2。5 になっていた。10-24 に足したときからの抜け。B の写経 10-28)
        composition.char_before = comp.char_before;
        composition.typed_valid = comp.typed_valid;
        composition.style_counted = comp.style_counted;
        composition.temp_ascii = comp.temp_ascii;
        composition.shifted_count = comp.shifted_count;
        composition.temp_ascii_start = comp.temp_ascii_start;
        composition.predictions = comp.predictions;
        composition.predicted_for = comp.predicted_for;
        composition.from_prediction = comp.from_prediction;
        // 確定した最後の字の控えは、この入力の頭でだけ使う
        composition.commit_tail = None;
        composition.punctuation_converted = comp.punctuation_converted;
        composition.typo_repair = comp.typo_repair;
        composition.typo_repair_declined = comp.typo_repair_declined;
        composition.typo_suggestions = comp.typo_suggestions;
        composition.next_words = comp.next_words;
        composition.next_words_for = comp.next_words_for;
        composition.next_word_pick = comp.next_word_pick;
        composition.prev_word = comp.prev_word;
        composition.text_before = comp.text_before;
        composition.last_autocorrect = comp.last_autocorrect;
        composition.last_phrase_runs = comp.last_phrase_runs;
        composition.refined = comp.refined;
        composition.live_settled = comp.live_settled;
        composition.segments_reading = comp.segments_reading;
        composition.script = comp.script;
        composition.tex_command = comp.tex_command;
        composition.accent = comp.accent;
        composition.accent_index = comp.accent_index;
        composition.accent_inserted = comp.accent_inserted;
        composition.dictionary_preview = comp.dictionary_preview;
        composition.ivs_original = comp.ivs_original;
        composition.state = comp.state;

        Ok(())
    }
}

#[cfg(test)]
mod dictionary_conversion_tests {
    use super::*;

    #[test]
    fn convert_cursor_is_at_the_end_until_moved_into_the_middle() {
        let mut comp = Composition { state: CompositionState::Converting,
            segments: build_segments("きょうはいいてんき", vec![("今日は".into(), 4), ("いい".into(), 2), ("天気".into(), 3)]),
            focus: 0, ..Default::default() };
        // 変換した直後は先頭の文節に注目していても、カーソルは右端 (文字キーで確定)
        assert!(convert_cursor_at_end(&comp));
        comp.cursor_moved = true;
        assert!(!convert_cursor_at_end(&comp));
        comp.focus = 2;
        assert!(convert_cursor_at_end(&comp));
    }

    #[test]
    fn ctrl_shift_henkan_only_cycles_a_focused_single_ideograph() {
        let mut comp = Composition { hiragana: "わたなべ".into(), state: CompositionState::Converting,
            segments: build_segments("わたなべ", vec![("渡".into(), 2), ("邊".into(), 2)]),
            focus: 1, ..Default::default() };
        assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x1C, true),
            Some((vec![ClientAction::CycleIvs], CompositionState::Converting)));
        // 2026-11-08 (仕事 53): Ctrl+Shift+J も同じ (変換キーの無い配列の代わり)
        assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x4A, true),
            Some((vec![ClientAction::CycleIvs], CompositionState::Converting)));
        assert!(cycle_focused_ivs(&mut comp));
        assert_eq!(comp.converted_text(), "渡邊\u{e0100}");
        assert_eq!(comp.segments[1].reading, "なべ");
        assert_eq!(comp.segments[0].text, "渡");
        assert!(comp.window_open);
        assert_eq!(comp.segments[1].index, 1);
        TextServiceFactory::select_candidate(&mut comp, 2);
        assert_eq!(comp.converted_text(), "渡邊\u{e0101}");
        assert_eq!(utf16_len(&comp.converted_text()), 4);
        comp.segments[1].text = "渡邊".into();
        assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x1C, true), None);
        assert!(!cycle_focused_ivs(&mut comp));
        comp.state = CompositionState::Composing;
        comp.segments[1].text = "邊".into();
        assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x1C, true), None);
    }

    #[test]
    fn ctrl_henkan_strips_only_server_notation() {
        let seg = |reading: &str, text: &str| Segment { reading: reading.into(), text: text.into(), ..Default::default() };
        // 化学式・電荷・水和物・ギリシャ文字は打ったとおりに戻す。ほかの文節 (加える) はそのまま
        let segments = vec![seg("H2Oを", "H₂Oを"), seg("くわえる", "加える")];
        let stripped = strip_notation_segments(&segments).unwrap();
        assert_eq!(stripped.iter().map(|s| s.text.as_str()).collect::<String>(), "H2Oを加える");
        assert_eq!(strip_notation(&seg("SO42-", "SO₄²⁻")), "SO42-");
        assert_eq!(strip_notation(&seg("Ca(OH)2の", "Ca(OH)₂の")), "Ca(OH)2の");
        assert_eq!(strip_notation(&seg("CuSO4.5H2O", "CuSO₄·5H₂O")), "CuSO4.5H2O");
        assert_eq!(strip_notation(&seg("H2Oぶんし", "H₂O分子")), "H2O分子");
        assert_eq!(strip_notation(&seg("あるふぁせん", "α線")), "アルファ線");
        assert_eq!(strip_notation(&seg("くしー", "ξ")), "クシー");
        // ^ _ で自分で打った下付き・上付き (読みに入っている) は戻さない
        assert_eq!(strip_notation(&seg("H₂O", "H₂O")), "H₂O");
        assert_eq!(strip_notation(&seg("x²", "x²")), "x²");
        // 候補から自分で選んだ π (自動で置き換えない名前) は戻さない
        assert_eq!(strip_notation(&seg("ぱい", "π")), "π");
        // 表記の自動変換がなければ何もしない (辞書だけの変換へ)
        assert!(strip_notation_segments(&[seg("きしゃが", "記者が")]).is_none());
        assert!(strip_notation_segments(&[seg("PS5", "PS5")]).is_none());
    }

    #[test]
    fn ctrl_muhenkan_switches_notation_independently_of_dictionary() {
        let seg = |reading: &str, text: &str| Segment { reading: reading.into(), text: text.into(), ..Default::default() };
        let text = |segments: &[Segment]| segments.iter().map(|s| s.text.as_str()).collect::<String>();
        let ai = vec![seg("H2Oを", "H₂Oを"), seg("くわえる", "加える")];
        let dictionary = vec![seg("H2Oを", "H₂Oを"), seg("くわえる", "咥える")];
        assert_eq!(text(&preview_segments(true, ai.clone())), "H2Oを加える");
        assert_eq!(text(&preview_segments(false, dictionary.clone())), "H₂Oを咥える");
        assert_eq!(text(&preview_segments(true, dictionary)), "H2Oを咥える");
        assert_eq!(text(&preview_segments(true, vec![seg("きしゃが", "記者が")])), "記者が");
    }

    #[test]
    fn ctrl_henkan_toggles_only_with_uncommitted_text() {
        for state in [CompositionState::Composing, CompositionState::Converting] {
            let comp = Composition { hiragana: "きしゃが".into(), state, ..Default::default() };
            assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x1C, false),
                Some((vec![ClientAction::ToggleDictionaryConversion], CompositionState::Converting)));
            assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x1C, true), None);
            assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x1D, false),
                Some((vec![ClientAction::ToggleNotation], CompositionState::Converting)));
        }
        assert_eq!(TextServiceFactory::process_ctrl_key(&Composition::default(), 0x1C, false), None);
        assert_eq!(TextServiceFactory::process_ctrl_key(&Composition::default(), 0x1D, false), None);
        // 2026-11-08 (仕事 53): 変換・無変換 の無い配列の代わり。Ctrl+J = Ctrl+変換、Ctrl+B = Ctrl+無変換。読みが無ければアプリへ
        for state in [CompositionState::Composing, CompositionState::Converting] {
            let comp = Composition { hiragana: "きしゃが".into(), state, ..Default::default() };
            assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x4A, false),
                Some((vec![ClientAction::ToggleDictionaryConversion], CompositionState::Converting)));
            assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x42, false),
                Some((vec![ClientAction::ToggleNotation], CompositionState::Converting)));
            // Ctrl+Shift+J は異体字の対象が無ければ何もしない (Ctrl+Shift+変換 と同じ)
            assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x4A, true), None);
        }
        assert_eq!(TextServiceFactory::process_ctrl_key(&Composition::default(), 0x4A, false), None);
        assert_eq!(TextServiceFactory::process_ctrl_key(&Composition::default(), 0x42, false), None);
        let empty = Composition { state: CompositionState::Composing, ..Default::default() };
        assert_eq!(TextServiceFactory::process_ctrl_key(&empty, 0x1C, false), None);
    }

    /// 2026-11-08 (仕事 52): 候補の窓の下の文言は、変換サーバーが AI を休んでいて、知らせの設定がオンのときだけ (字は利用者の指定のまま)
    #[test]
    fn pause_note_only_while_paused_and_wanted() {
        assert_eq!(pause_note(true, true), "負荷がかかっているためAIがオフになっています");
        assert_eq!(pause_note(true, false), "");
        assert_eq!(pause_note(false, true), "");
    }

    #[test]
    fn ctrl_space_still_commits_and_inserts_space() {
        for state in [CompositionState::Composing, CompositionState::Converting] {
            let comp = Composition { hiragana: "にほんご".into(), state, ..Default::default() };
            assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x20, false),
                Some((vec![ClientAction::EndComposition, ClientAction::CommitText(" ".into())], CompositionState::None)));
            assert_eq!(TextServiceFactory::process_ctrl_key(&comp, 0x20, true),
                Some((vec![ClientAction::EndComposition, ClientAction::CommitText("　".into())], CompositionState::None)));
        }
    }

    #[test]
    fn dictionary_return_restores_original_segments_and_selection() {
        let mut comp = Composition { hiragana: "きしゃが".into(), state: CompositionState::Converting,
            segments: build_segments("きしゃが", vec![("記者".into(), 3), ("が".into(), 1)]),
            focus: 1, window_open: true, refined: true, segments_reading: "きしゃが".into(),
            latin_selected: true, from_prediction: true, ..Default::default() };
        comp.segments[0].candidates = vec!["記者".into(), "汽車".into()];
        comp.segments[0].index = 1;
        comp.segments[0].text = "汽車".into();
        let before = ConversionSnapshot::capture(&comp, "きしゃが").unwrap();
        comp.segments = build_segments("きしゃが", vec![("貴社が".into(), 4)]);
        comp.focus = 0;
        comp.window_open = false;
        comp.latin_selected = false;
        comp.from_prediction = false;
        before.restore(&mut comp);
        assert_eq!(comp.converted_text(), "汽車が");
        assert_eq!(comp.focus, 1);
        assert_eq!(comp.segments[0].index, 1);
        assert_eq!(comp.segments[0].candidates, vec!["記者", "汽車"]);
        assert!(comp.window_open && comp.refined && comp.latin_selected && comp.from_prediction);
    }

    #[test]
    fn incomplete_or_unrefined_live_text_is_not_saved_as_ai() {
        let mut comp = Composition { hiragana: "きょうはk".into(), state: CompositionState::Composing,
            live: true, refined: true, segments: build_segments("きょうは", vec![("今日は".into(), 4)]),
            ..Default::default() };
        assert!(ConversionSnapshot::capture(&comp, "きょうはk").is_none());
        assert!(ConversionSnapshot::capture(&comp, "きょうは").is_some());
        comp.refined = false;
        assert!(ConversionSnapshot::capture(&comp, "きょうは").is_none());
        comp.live = false;
        assert!(ConversionSnapshot::capture(&comp, "きょうは").is_none());
    }

    #[test]
    fn edits_cancel_preview_but_candidate_navigation_can_return() {
        for action in [ClientAction::Insert("a".into()), ClientAction::DeleteBackward, ClientAction::DeleteForward,
            ClientAction::Resize(1), ClientAction::CancelConversion, ClientAction::EndComposition,
            ClientAction::CommitFirstSegment, ClientAction::ConvertType(SetTextType::Katakana)] {
            assert!(!keeps_dictionary_preview(&action), "{action:?}");
        }
        for action in [ClientAction::ToggleDictionaryConversion, ClientAction::ToggleNotation, ClientAction::MoveFocus(1), ClientAction::NextCandidate,
            ClientAction::PrevPage, ClientAction::SelectNumber(3)] {
            assert!(keeps_dictionary_preview(&action), "{action:?}");
        }
        let mut comp = Composition { dictionary_preview: Some(DictionaryPreview::new("あ".into(), None)), ..Default::default() };
        comp.reset();
        assert!(comp.dictionary_preview.is_none());
    }

    #[test]
    fn dictionary_snapshots_roundtrip_actual_isolated_server_outputs() {
        let Ok(path) = std::env::var("AZK_DICTIONARY_SERVER_PROBE") else { return };
        let data: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let rows = data["dictionary_toggles"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        for row in rows {
            let reading = row["reading"].as_str().unwrap();
            let converted = |key: &str| row[key].as_array().unwrap().iter().map(|part|
                (part[0].as_str().unwrap().to_string(), part[1].as_i64().unwrap() as i32)).collect();
            let ai = build_segments(reading, converted("ai"));
            let original = ai.iter().map(|segment| segment.text.as_str()).collect::<String>();
            let mut comp = Composition { hiragana: reading.into(), state: CompositionState::Converting,
                segments: ai, refined: true, segments_reading: reading.into(), ..Default::default() };
            let snapshot = ConversionSnapshot::capture(&comp, reading).unwrap();
            comp.segments = build_segments(reading, converted("dictionary"));
            snapshot.restore(&mut comp);
            assert_eq!(comp.converted_text(), original);
            assert_eq!(comp.segments.iter().map(|segment| segment.reading.as_str()).collect::<String>(), reading);
        }
    }
}

#[cfg(test)]
mod unresponsive_server_tests {
    use super::{raw_typed, ClientAction as A};

    #[test]
    fn only_the_typed_characters_are_inserted_when_the_server_is_silent() {
        assert_eq!(raw_typed(&[A::StartComposition, A::Insert("n".into())]), "n");
        assert_eq!(raw_typed(&[A::Insert("N".into()), A::Insert(",".into())]), "N,");
        assert_eq!(raw_typed(&[A::EndComposition, A::Convert]), "");
        assert_eq!(raw_typed(&[A::Insert("あ".into()), A::Insert(" ".into())]), "");
    }
}

#[cfg(test)]
mod lone_romaji_n_tests {
    use super::ends_with_lone_romaji_n;

    /// 2026-10-15: かなの後ろ (または頭) に 1 つだけある n の後ろの - は ー (n- = んー)。英単語・数字の後ろはハイフンのまま
    #[test]
    fn lone_romaji_n_before_hyphen() {
        for reading in ["n", "でn", "こn", "ーn"] {
            assert!(ends_with_lone_romaji_n(reading), "{reading}");
        }
        for reading in ["python", "on", "3n", "ん", "", "で"] {
            assert!(!ends_with_lone_romaji_n(reading), "{reading}");
        }
    }
}
