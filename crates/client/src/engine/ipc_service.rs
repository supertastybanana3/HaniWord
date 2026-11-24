use anyhow::Result;
use hyper_util::rt::TokioIo;
use shared::proto::{
    azookey_service_client::AzookeyServiceClient, window_service_client::WindowServiceClient,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use std::os::windows::io::IntoRawHandle;
use tokio::{net::windows::named_pipe::NamedPipeClient, time};
use tonic::transport::Endpoint;
use tower::service_fn;
use windows::Win32::Foundation::ERROR_PIPE_BUSY;

// 2026-10-14: サーバー・候補窓への問い合わせの待ち時間の上限。IME はアプリの中で動くので、返事が来ないまま待つと
// 打っているアプリごと固まる。上限を超えたらあきらめ (呼び出し側はキーをアプリへ返し、見えているものを確定する)、
// しばらく (3 秒から倍々で最大 30 秒) は待たずに失敗させる。ふだんは読みの編集が 10ms 未満、160 字の辞書変換が 0.34 秒
const KEY_LIMIT: Duration = Duration::from_millis(2000);
const CONVERT_LIMIT: Duration = Duration::from_millis(6000);
const UI_LIMIT: Duration = Duration::from_millis(1000);
const CONNECT_LIMIT: Duration = Duration::from_millis(3000);

#[cfg(test)]
pub(crate) fn now_ms_for_tests() -> u64 {
    now_ms()
}

fn now_ms() -> u64 {
    static START: std::sync::LazyLock<std::time::Instant> = std::sync::LazyLock::new(std::time::Instant::now);
    START.elapsed().as_millis() as u64 + 1
}

/// 返事が来なかった相手には、しばらく問い合わせずに失敗させる
#[derive(Debug)]
pub(crate) struct Breaker {
    until_ms: AtomicU64,
    streak: AtomicU32,
}

impl Breaker {
    const fn new() -> Self {
        Self { until_ms: AtomicU64::new(0), streak: AtomicU32::new(0) }
    }
    fn open(&self) -> bool {
        now_ms() < self.until_ms.load(Ordering::Relaxed)
    }
    fn failed(&self) -> u64 {
        let streak = self.streak.fetch_add(1, Ordering::Relaxed).min(4);
        let cool = (3000u64 << streak).min(30_000);
        self.until_ms.store(now_ms() + cool, Ordering::Relaxed);
        cool
    }
    fn succeeded(&self) {
        if self.streak.load(Ordering::Relaxed) != 0 {
            self.streak.store(0, Ordering::Relaxed);
            self.until_ms.store(0, Ordering::Relaxed);
        }
    }
}

/// 2026-10-29: 直近に測った AI の変換・読みの採点の速さと、測った時刻。遅い PC (AI を CPU で動かす) では、打っている途中の AI
/// (ライブ変換の清書・句読点での変換) を辞書だけにし、母音の直しをしない (、。 のたびに何秒も止まらないように)。
/// 印は 3 分で消えて測り直す (前は一度遅いと決めたら、その AI を使わないので測り直せず、ずっと辞書だけだった。レビュー M3)。
/// 速さは 1 字あたりの時間で持ち、これから変換する字数で見積もる (前は 1 回の時間で持ったので、CPU の PC では短い Space で印が消えて
/// 長い文の清書が止まり、速い PC では長い文の Space が 1.6 秒かかっただけで短い文の AI も 3 分止まった。再点検 N1)。
/// 1.5 秒の上限で切ったときは、その字数で 1.5 秒以上 (待った時間がそれより長ければ待った時間) かかるものとして持つ (打っている途中の AI・採点は
/// 1.5 秒しか待たないので、切れたらその字数では間に合わない)。前の値 (3 分以内) より遅いときだけ替える。8 字より短い変換の答えでは替えない
#[derive(Debug)]
pub(crate) struct SlowMark {
    us_per_char: AtomicU64,
    at_ms: AtomicU64,
}

const SLOW_MS: u64 = 1500;
const SLOW_FOR_MS: u64 = 180_000;
/// 2026-10-29 (3 回目の点検 2): これより短い変換の答えでは速さを替えない (毎回かかる固定の時間・順番待ちが 1 字あたりに大きく入り、
/// 1〜2 字の Space が 0.3 秒かかっただけで、速い PC でも 10 字を超える清書が止まった)
const MIN_MEASURED_CHARS: usize = 8;
/// 打っている途中の AI・採点に与える上限は 1.5 秒。切れたときに与えていた上限がほぼ 1.5 秒 (これより短く欠けていない) なら、その字数では間に合わない
const CUT_TOLERANCE: Duration = Duration::from_millis(50);

impl SlowMark {
    const fn new() -> Self {
        Self { us_per_char: AtomicU64::new(0), at_ms: AtomicU64::new(0) }
    }
    fn fresh_us(&self, now: u64) -> u64 {
        let at = self.at_ms.load(Ordering::Relaxed);
        if at != 0 && now < at.saturating_add(SLOW_FOR_MS) { self.us_per_char.load(Ordering::Relaxed) } else { 0 }
    }
    /// 答えが来た: chars 字に waited かかった (短い変換では替えない)
    fn answered_at(&self, waited: Duration, chars: usize, now: u64) {
        if chars < MIN_MEASURED_CHARS {
            return;
        }
        self.us_per_char.store(waited.as_micros() as u64 / chars.max(1) as u64, Ordering::Relaxed);
        self.at_ms.store(now, Ordering::Relaxed);
    }
    /// 上限で切った: chars 字に少なくとも waited かかる。与えた上限 (budget) が 1.5 秒あったなら、少なくとも 1.5 秒かかる
    /// (上限の残りが少なくて早く切れたときは、待った時間だけ。3 回目の点検 3)
    fn cut_at(&self, waited: Duration, budget: Duration, chars: usize, now: u64) {
        let slow = Duration::from_millis(SLOW_MS);
        let at_least = if budget + CUT_TOLERANCE >= slow { waited.max(slow) } else { waited };
        let at_least = (at_least.as_micros() as u64).div_ceil(chars.max(1) as u64);
        self.us_per_char.store(at_least.max(self.fresh_us(now)), Ordering::Relaxed);
        self.at_ms.store(now, Ordering::Relaxed);
    }
    /// chars 字の変換に 1.5 秒以上かかりそうか
    pub(crate) fn slow_for_at(&self, chars: usize, now: u64) -> bool {
        self.fresh_us(now).saturating_mul(chars.max(1) as u64) >= SLOW_MS * 1000
    }
}

/// 変換サーバー・候補窓の具合。ふだんはプロセスで 1 つ (HEALTH)。試験では試験ごとに別のものを使う (ほかの試験の問い合わせを止めない)
#[derive(Debug)]
pub(crate) struct Health {
    server: Breaker,
    ui: Breaker,
    /// 候補窓に頼めなかった回数 (返事が無い・上限で問い合わせなかった・まだ受け付けていない)。見えていないものを見えている扱いにしないために見る
    ui_failures: AtomicU64,
    pub(crate) ai: SlowMark,
    scoring: SlowMark,
}

impl Health {
    #[cfg(test)]
    pub(crate) fn server_open_for_tests(&self) -> bool {
        self.server.open()
    }

    const fn new() -> Self {
        Self { server: Breaker::new(), ui: Breaker::new(), ui_failures: AtomicU64::new(0), ai: SlowMark::new(), scoring: SlowMark::new() }
    }
}

static HEALTH: Health = Health::new();

/// 上限までに返事が来なかった (あるいは、少し前に来なかったので問い合わせなかった)
#[derive(Debug)]
pub struct Unresponsive(&'static str);

impl std::fmt::Display for Unresponsive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no response from {}", self.0)
    }
}

impl std::error::Error for Unresponsive {}

/// サーバーが少し前に返事をしなかったので、今は問い合わせない (キーはアプリへそのまま渡す)。
/// IME が有効になったときにつなげず、まだつなぎ直せていない間も同じ (IME 本体だけでひらがなを入れる)
pub fn server_unresponsive() -> bool {
    HEALTH.server.open() || HANDOVER.connection.disconnected()
}

/// 名前付きパイプを開く。混んでいる (ERROR_PIPE_BUSY) あいだは 50ms おきに 3 秒まで待つ (つなぎ直しのときも無限には待たない)
async fn open_pipe_waiting(base: String, executable: String) -> std::io::Result<TokioIo<NamedPipeClient>> {
    let mut tries = 0;
    let client = loop {
        match open_pipe(&base, &executable) {
            Ok(client) => break client,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) && tries < 60 => tries += 1,
            Err(e) => return Err(e),
        }

        time::sleep(Duration::from_millis(50)).await;
    };

    Ok(TokioIo::new(client))
}

/// 名前付きパイプでつなぐ。上限は CONNECT_LIMIT と、その場の上限の残りの短いほう
async fn connect_channel(uri: &'static str, base: &str, executable: &str) -> Result<tonic::transport::Channel> {
    let Some(limit) = budgeted(CONNECT_LIMIT) else {
        return Err(Unresponsive("connect").into());
    };
    let (base, executable) = (base.to_owned(), executable.to_owned());
    time::timeout(limit, Endpoint::try_from(uri)?.connect_with_connector(service_fn(
        move |_| open_pipe_waiting(base.clone(), executable.clone()),
    ))).await.map_err(|_| anyhow::Error::from(Unresponsive("connect")))?.map_err(anyhow::Error::from)
}

/// 2026-10-29: 最初の問い合わせのときにつなぐ通信路 (つなげなければ、その問い合わせが Unavailable で失敗し、次の問い合わせでまた試す)。
/// tokio の実行環境の中で呼ぶ
fn lazy_channel(uri: &'static str, base: &str, executable: &str) -> Result<tonic::transport::Channel> {
    let (base, executable) = (base.to_owned(), executable.to_owned());
    Ok(Endpoint::try_from(uri)?.connect_with_connector_lazy(service_fn(
        move |_| open_pipe_waiting(base.clone(), executable.clone()),
    )))
}

/// 2026-10-28: IME が有効になったとき (Activate) に変換サーバー・候補窓へつなげなかったときの、つなぎ直し。
/// 前は Activate がキーの受け口を登録しないまま終わり、そのアプリでは最後まで英字だけが入っていた (ログオン直後・サーバーや候補窓の
/// 再起動の途中。写経の準備で 7 回中 4 回)。今は受け口を登録し、キーが来たら (フォーカスが移ったら) つなぎ直す。
/// 2026-10-29: つなぐのは裏のスレッドで 1 本ずつ (UI スレッドで待つと、相手が受け付けても返事をしないとき、キーのたびに 3〜6 秒止まり、
/// アプリが固まって見えた。写経 10-29)。試すあいだは 2 秒・4 秒・8 秒・10 秒・10 秒…と空ける。UI スレッドは、裏でつながったものを受け取るだけ
pub(crate) struct Reconnect {
    disconnected: AtomicBool,
    /// 次に試してよい時刻 (ms)
    next_ms: AtomicU64,
    /// 続けて失敗した回数
    failures: AtomicU32,
    /// 裏で試している最中か、裏でつながったものを UI スレッドがまだ受け取っていない (そのあいだは次を始めない。レビュー M5)
    in_flight: AtomicBool,
}

const RECONNECT_FIRST_MS: u64 = 2000;
// 2026-10-29: 間の上限は 10 秒 (ログオン直後に 1 分ひらがなだけになるのは長い。試しは裏のスレッドで、パイプが無ければすぐ失敗するので安い)
const RECONNECT_MAX_MS: u64 = 10_000;

impl Reconnect {
    pub(crate) const fn new() -> Self {
        Self { disconnected: AtomicBool::new(false), next_ms: AtomicU64::new(0), failures: AtomicU32::new(0), in_flight: AtomicBool::new(false) }
    }
    fn disconnected(&self) -> bool {
        self.disconnected.load(Ordering::Acquire)
    }
    fn failed_at_activation(&self, now: u64) {
        self.failures.store(0, Ordering::Relaxed);
        self.next_ms.store(now + RECONNECT_FIRST_MS, Ordering::Relaxed);
        self.disconnected.store(true, Ordering::Release);
    }
    fn connected(&self) {
        self.disconnected.store(false, Ordering::Release);
        self.failures.store(0, Ordering::Relaxed);
        self.in_flight.store(false, Ordering::Release);
    }
    /// 試してよければ、試している印を付けて true (裏で試しているあいだ・受け取られるのを待っているあいだ・間を空けているあいだは false)
    fn begin(&self, now: u64) -> bool {
        self.disconnected() && now >= self.next_ms.load(Ordering::Relaxed)
            && self.in_flight.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_ok()
    }
    /// 裏の試しが失敗した。次は 4 秒・8 秒・10 秒・10 秒…あと (終わった時刻から数える。最初は Activate から 2 秒)。続けて失敗した回数を返す
    fn failed(&self, now: u64) -> u32 {
        let failures = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        let wait = (RECONNECT_FIRST_MS << (failures.min(6))).min(RECONNECT_MAX_MS);
        self.next_ms.store(now + wait, Ordering::Relaxed);
        self.in_flight.store(false, Ordering::Release);
        failures
    }
    /// 裏でつながったが、もうほかの道筋でつながっていたので捨てた
    fn discarded(&self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

/// 2026-10-29: つなぎ直しの状態と、裏でつながってまだ UI スレッドが受け取っていない通信路。ふだんはプロセスで 1 つ (HANDOVER)。
/// 試験では別のものを作って、受け渡しの道をそのまま通す
pub(crate) struct Handover {
    connection: Reconnect,
    /// 裏でつながったもの (UI スレッドが次のキーで受け取る。取り合わないよう、UI スレッドは try_lock で見るだけ)
    pending: std::sync::Mutex<Option<IPCService>>,
}

static HANDOVER: Handover = Handover::new();

impl Handover {
    pub(crate) const fn new() -> Self {
        Self { connection: Reconnect::new(), pending: std::sync::Mutex::new(None) }
    }

    /// 待って取る (裏のスレッドと、つながった印を付けるとき。持っている途中でパニックしていても中身は使える。レビュー L1)
    fn lock_pending(&self) -> std::sync::MutexGuard<'_, Option<IPCService>> {
        self.pending.lock().unwrap_or_else(|poisoned| {
            self.pending.clear_poison();
            poisoned.into_inner()
        })
    }

    /// 待たずに取る (UI スレッドがキーのたびに見る)
    fn try_pending(&self) -> Option<std::sync::MutexGuard<'_, Option<IPCService>>> {
        match self.pending.try_lock() {
            Ok(slot) => Some(slot),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                self.pending.clear_poison();
                Some(poisoned.into_inner())
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }

    /// つながった。裏でつながって受け取られていないものがあれば取り出す (古いものを、あとで切れたときに受け取らないように。レビュー M5)。
    /// 先に印を変えてから取り出す (裏のスレッドは、PENDING を持ったまま印を確かめてから入れる。持つのは確かめて入れるあいだだけなので、待ってよい)。
    /// 取り出したものは呼ぶ側が IME の状態を離してから捨てる (捨てるときに tokio の実行環境を止める数ミリ秒、ほかのスレッドのキーが状態を取れない。3 回目の点検 4)
    #[must_use]
    fn mark_connected(&self) -> Option<IPCService> {
        self.connection.connected();
        self.lock_pending().take()
    }

    /// UI スレッド: 裏でつながったものがあれば、IME の状態の通信路 (slot) が空のときに入れる。slot が埋まっていれば
    /// (ほかの道筋でもうつながっていても) つながった扱いにする。入れたら true と、捨てるもの
    fn pick_up(&self, slot: &mut Option<IPCService>) -> (bool, Option<IPCService>) {
        let mut installed = false;
        if slot.is_none() {
            if let Some(ipc) = self.try_pending().and_then(|mut pending| pending.take()) {
                *slot = Some(ipc);
                installed = true;
            }
        }
        let stale = if slot.is_some() { self.mark_connected() } else { None };
        (installed, stale)
    }

    /// Activate でつないだものを入れる。slot が None (IME の状態を取れなかった) なら、裏でつながったものと同じに次のキーで受け取る (レビュー L2)。
    /// 捨てるもの (入れ替えた前の通信路・受け取られていなかったもの) を返す
    #[must_use]
    fn install(&self, ipc: IPCService, slot: Option<&mut Option<IPCService>>) -> Vec<IPCService> {
        match slot {
            Some(slot) => {
                let previous = slot.replace(ipc);
                previous.into_iter().chain(self.mark_connected()).collect()
            }
            None => {
                let previous = self.lock_pending().replace(ipc);
                self.connection.in_flight.store(true, Ordering::Release);
                self.connection.failed_at_activation(now_ms());
                previous.into_iter().collect()
            }
        }
    }

    /// 裏のスレッドで 1 回つないでみて、つながったら PENDING に入れる。入れる直前に、まだつながっていないかを確かめる
    /// (ほかの道筋でつながっていれば、これは捨てる)。途中でパニックしても、試している印を残さない (残ると、二度とつなぎ直さない。レビュー L1)
    fn attempt(&self, connect: impl FnOnce() -> Result<IPCService>) {
        struct Attempt<'a>(&'a Reconnect, bool);
        impl Drop for Attempt<'_> {
            fn drop(&mut self) {
                if !self.1 {
                    self.0.failed(now_ms());
                }
            }
        }
        let mut attempt = Attempt(&self.connection, false);
        match connect() {
            Ok(ipc) => {
                let mut slot = self.lock_pending();
                attempt.1 = true;
                if self.connection.disconnected() && slot.is_none() {
                    *slot = Some(ipc);
                } else {
                    drop(slot);
                    self.connection.discarded();
                }
            }
            Err(_) => {
                attempt.1 = true;
                if self.connection.failed(now_ms()) == 1 {
                    let line = "reconnect: still no conversion server or candidate window; keys go in as hiragana until then";
                    crate::guard::record(line);
                    crate::debug_log::write(line);
                }
            }
        }
    }
}

/// Activate で呼ぶ。つなげなければ None (つなぎ直すまでは、サーバーが返事をしない間と同じ扱い)。
/// compat_log.txt (いつも) と client.log (動作確認のログを有効にしているとき) に 1 行残す (打った文字は書かない)
pub fn connect_at_activation() -> Option<IPCService> {
    match IPCService::new() {
        Ok(ipc) => {
            drop(HANDOVER.mark_connected());
            Some(ipc)
        }
        Err(error) => {
            // 2026-10-29: IME の状態はプロセスで 1 つなので、ほかのスレッドがもうつないでいれば、それを使い続ける
            // (前は、ほかのスレッドの Activate がつなげなかっただけで、つながっているスレッドまでひらがなだけになった。レビュー M5)
            let shared = super::state::IMEState::get().is_ok_and(|state| state.ipc_service.is_some());
            if !shared {
                HANDOVER.connection.failed_at_activation(now_ms());
            }
            let line = format!("activate: could not connect to the conversion server or the candidate window ({error}); {}",
                if shared { "keeping the connection of another thread" } else { "retrying in the background" });
            crate::guard::record(&line);
            crate::debug_log::write(&line);
            None
        }
    }
}

/// 2026-10-29: Activate でつないだものを IME の状態に入れる。状態を取れなければ (ほかが持っている)、裏でつながったものと同じに
/// 次のキーで受け取る (前は Activate がそこで抜け、キーの受け口を登録しなかった。レビュー L2)
pub fn install(ipc: IPCService) {
    let discarded = match super::state::IMEState::get() {
        Ok(mut state) => HANDOVER.install(ipc, Some(&mut state.ipc_service)),
        Err(_) => HANDOVER.install(ipc, None),
    };
    drop(discarded);
}

/// 2026-10-28: Activate でつなげていなければ、つなぎ直す (キー・フォーカスの移りのたびに呼ぶ)。
/// 2026-10-29: UI スレッドでは待たない。裏でつながっていれば受け取り、まだなら (間が空いていて、裏で試していなければ) 裏のスレッドで 1 回試す。
/// つながったことと、つなぎ直しの最初の失敗だけを compat_log.txt・client.log に残す
pub fn reconnect_if_needed() {
    if !HANDOVER.connection.disconnected() {
        return;
    }
    let Ok(mut state) = super::state::IMEState::get() else { return };
    let (installed, stale) = HANDOVER.pick_up(&mut state.ipc_service);
    let kana = state.input_mode == super::input_mode::InputMode::Kana;
    if state.ipc_service.is_some() {
        let picked = if installed { state.ipc_service.clone() } else { None };
        drop(state);
        drop(stale);
        if let Some(mut ipc) = picked {
            // 2026-10-29: 裏でつないでいるあいだに入力モードを切り替えていたら候補窓の あ/A が古いので、受け取ったときの今のモードを送る (再点検 R4)
            let _ = ipc.set_input_mode(if kana { "\u{3042}" } else { "A" });
            let line = "reconnected to the conversion server and the candidate window";
            crate::guard::record(line);
            crate::debug_log::write(line);
        }
        return;
    }
    drop(state);
    if !HANDOVER.connection.begin(now_ms()) {
        return;
    }
    let spawned = std::thread::Builder::new().name("haniword-reconnect".into()).spawn(|| {
        HANDOVER.attempt(|| {
            let mut ipc = IPCService::new()?;
            // 入力の状態を合わせる (裏のスレッドなので待ってもアプリは止まらない。時間切れでも渡す: その時間切れで問い合わせを控える印が付くので、UI スレッドは待たない)
            let _ = ipc.append_text(String::new());
            Ok(ipc)
        });
    });
    if spawned.is_err() {
        HANDOVER.connection.failed(now_ms());
    }
}

/// 2026-10-20 (Fable の再調査 L1): 候補窓が少し前に返事をしなかった (見えていない候補に Tab を使わないため)
pub fn ui_unresponsive() -> bool {
    HEALTH.ui.open()
}

/// 2026-10-29: 候補窓に頼めなかった回数 (この前後で変わっていたら、頼んだものの一部が見えていない。レビュー L3)
pub fn ui_failures() -> u64 {
    HEALTH.ui_failures.load(Ordering::Relaxed)
}

/// サーバーとのやり取りで起きた失敗か (返事が来ない・サーバーが返したエラー・通信路の切断)。
/// そのキーの表示はまだ書き換えていないので、打った文字をそのまま入れてよい
pub fn from_server(error: &anyhow::Error) -> bool {
    error.chain().any(|e| e.is::<Unresponsive>() || e.is::<tonic::Status>() || e.is::<tonic::transport::Error>())
}

/// 2026-10-29: 1 回のキー・フォーカスの移り・時計で、UI スレッドがサーバー・候補窓を待つ時間の合計の上限。
/// 問い合わせごとの上限 (KEY_LIMIT・CONVERT_LIMIT…) だけだと、1 回のキーで何度も問い合わせる道筋 (母音の直し・句読点での変換・つなぎ直し) で
/// 待ちが積み重なり、アプリが何秒も固まった (写経 10-29)。超えたら残りは問い合わせずに Unresponsive を返す (呼ぶ側はひらがな・辞書だけで返す)
pub(crate) const EVENT_BUDGET: Duration = Duration::from_millis(3000);

thread_local! {
    static DEADLINE: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
    /// 利用者が待っている動作 (AwaitedBudget) の中か
    static AWAITED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// 試験で AWAITED_LIMIT の代わりに使う上限 (そのスレッドだけ)
    #[cfg(test)]
    static AWAITED_LIMIT_FOR_TESTS: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_awaited_limit_for_tests(limit: Option<Duration>) {
    AWAITED_LIMIT_FOR_TESTS.with(|cell| cell.set(limit));
}

/// その場の上限を始める (入れ子なら外側の上限のまま)。落とすと終わる
pub(crate) struct EventBudget(bool);

impl EventBudget {
    pub(crate) fn start() -> Self {
        Self::start_with(EVENT_BUDGET)
    }
    pub(crate) fn start_with(limit: Duration) -> Self {
        DEADLINE.with(|deadline| match deadline.get() {
            Some(_) => EventBudget(false),
            None => {
                deadline.set(Some(std::time::Instant::now() + limit));
                EventBudget(true)
            }
        })
    }
}

impl Drop for EventBudget {
    fn drop(&mut self) {
        if self.0 {
            DEADLINE.with(|deadline| deadline.set(None));
        }
    }
}

/// その場の上限を、いまから limit までに狭める (外側の上限のほうが早ければそのまま)。落とすと元に戻る
pub(crate) struct NarrowBudget(Option<std::time::Instant>);

impl NarrowBudget {
    pub(crate) fn within(limit: Duration) -> Self {
        DEADLINE.with(|deadline| {
            let previous = deadline.get();
            let narrowed = std::time::Instant::now() + limit;
            deadline.set(Some(previous.map_or(narrowed, |p| p.min(narrowed))));
            NarrowBudget(previous)
        })
    }
}

impl Drop for NarrowBudget {
    fn drop(&mut self) {
        DEADLINE.with(|deadline| deadline.set(self.0));
    }
}

/// 2026-10-29: 利用者が結果を待っている動作 (Space・候補の一覧などの変換) のあいだは、その場の上限 (EVENT_BUDGET) の代わりに
/// いまから AWAITED_LIMIT までを上限にする (問い合わせごとの上限 CONVERT_LIMIT まで待てる。内側で狭めた上限 NarrowBudget はかかる)。
/// かかった時間はその場の上限に数えない (落とすと、残りが始める前と同じに戻る)。
/// 前は Space でも 3 秒で切り、遅い PC (AI を CPU で動かす) では長い文の変換が間に合わず、ひらがなのまま確定していた (レビュー H1)。
/// 上限を無しにすると、1 回の Space で組み直し・母音の直し・AI の変換 6 秒・候補の一覧 6 秒と足し算になる (再点検 N2)
pub(crate) const AWAITED_LIMIT: Duration = Duration::from_millis(8000);

pub(crate) struct AwaitedBudget {
    previous: Option<std::time::Instant>,
    started: std::time::Instant,
    was_awaited: bool,
}

impl AwaitedBudget {
    pub(crate) fn begin() -> Self {
        #[cfg(test)]
        if let Some(limit) = AWAITED_LIMIT_FOR_TESTS.with(|cell| cell.get()) {
            return Self::begin_with(limit);
        }
        Self::begin_with(AWAITED_LIMIT)
    }
    pub(crate) fn begin_with(limit: Duration) -> Self {
        let started = std::time::Instant::now();
        let was_awaited = AWAITED.with(|awaited| awaited.replace(true));
        DEADLINE.with(|deadline| AwaitedBudget { previous: deadline.replace(Some(started + limit)), started, was_awaited })
    }
}

impl Drop for AwaitedBudget {
    fn drop(&mut self) {
        let spent = self.started.elapsed();
        AWAITED.with(|awaited| awaited.set(self.was_awaited));
        DEADLINE.with(|deadline| deadline.set(self.previous.map(|previous| previous + spent)));
    }
}

/// 2026-11-06 (仕事 29 の 2): 上限 (1.5 秒) で切ったライブ変換の清書の問い合わせ。切ったあとも裏で待ち続け、届いたら、打っている読みが同じなら使う
/// (変換サーバーは切られても計算を続けるので、AI を CPU で動かす PC では、その答えを捨てると、次のキーがその計算の後ろで待つだけになっていた。
/// 測り 11-06: AI を CPU で動かすと清書の 60 回のうち 6〜14 回が 1.5 秒を超え、最大 3.2 秒)。待つのは LATE_REFINE_LIMIT まで
struct LateRefine {
    reading: String,
    chars: usize,
    started: std::time::Instant,
    handle: tokio::task::JoinHandle<Result<tonic::Response<shared::proto::ConvertResponse>, tonic::Status>>,
    runtime: Arc<tokio::runtime::Runtime>,
    health: &'static Health,
}

const LATE_REFINE_LIMIT: Duration = CONVERT_LIMIT;

thread_local! {
    static LATE_REFINE: std::cell::RefCell<Option<LateRefine>> = const { std::cell::RefCell::new(None) };
}

/// 遅れた清書の様子
#[derive(Debug, PartialEq)]
pub enum LateRefineState {
    /// 待っている問い合わせが無い (待ちきれずにあきらめた・答えが失敗だった)
    Nothing,
    Pending,
    /// 届いた (読み, 文節ごとの (表記, 読みの文字数))
    Ready(String, Vec<(String, i32)>),
}

/// 遅れた清書が届いたかを見る (時計から呼ぶ。待たない)
pub fn poll_late_refine() -> LateRefineState {
    LATE_REFINE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(late) = slot.as_ref() else { return LateRefineState::Nothing };
        if !late.handle.is_finished() {
            if late.started.elapsed() < LATE_REFINE_LIMIT {
                return LateRefineState::Pending;
            }
            if let Some(late) = slot.take() {
                late.handle.abort();
            }
            return LateRefineState::Nothing;
        }
        let Some(late) = slot.take() else { return LateRefineState::Nothing };
        let waited = late.started.elapsed();
        match late.runtime.block_on(late.handle) {
            Ok(Ok(response)) => {
                // 本当にかかった時間で速さを測り直す (切ったときは「少なくとも 1.5 秒」として持っている)
                late.health.ai.answered_at(waited, late.chars, now_ms());
                let response = response.into_inner();
                note_ai_paused(&response, false);
                let segments = checked_segments(&late.reading, response.segments.into_iter().map(|s| (s.text, s.length)).collect());
                LateRefineState::Ready(late.reading, segments)
            }
            _ => LateRefineState::Nothing,
        }
    })
}

/// 待っている遅れた清書をやめる (新しい清書を始めるとき)
pub fn drop_late_refine() {
    if let Some(late) = LATE_REFINE.with(|cell| cell.borrow_mut().take()) {
        late.handle.abort();
    }
}

#[cfg(test)]
pub(crate) fn late_refine_pending_for_tests() -> Option<String> {
    LATE_REFINE.with(|cell| cell.borrow().as_ref().map(|late| late.reading.clone()))
}

/// 2026-11-08 (仕事 52): 変換サーバーが重いので AI を休んでいるか (AI ありの変換 (Space・清書) の答えに付く印。辞書だけの変換 (仮表示) の答えでは替えない。
/// 古い変換サーバーは印を付けないので、いつも false)
#[cfg(not(test))]
static AI_PAUSED: AtomicBool = AtomicBool::new(false);
// 2026-11-19 (C の見直し): 試験ではスレッド (試験) ごとに持つ。大域のままだと、並べて流すほかの試験の AI ありの変換 (偽の変換サーバーは印を付けない) が
// false を書き込み、印を見る試験がまれに落ちる。印を書く所 (ipc の変換の答え) と読む所は、どちらも試験のスレッドの上
#[cfg(test)]
thread_local! {
    static AI_PAUSED_IN_TEST: AtomicBool = const { AtomicBool::new(false) };
}

fn with_ai_paused<R>(f: impl FnOnce(&AtomicBool) -> R) -> R {
    #[cfg(not(test))]
    return f(&AI_PAUSED);
    #[cfg(test)]
    return AI_PAUSED_IN_TEST.with(|cell| f(cell));
}

pub fn ai_paused() -> bool {
    with_ai_paused(|cell| cell.load(Ordering::Relaxed))
}

/// 2026-11-08 (仕事 52): AI を休んでいることを知らせるか (設定 input.ai_pause_notice、初期値オン。候補の窓の下の文言と札の点)
pub(crate) fn ai_pause_notice_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| cfg!(test) || shared::AppConfig::read_or_default().input.ai_pause_notice)
}

fn note_ai_paused(response: &shared::proto::ConvertResponse, dictionary_only: bool) {
    if !dictionary_only {
        with_ai_paused(|cell| cell.store(response.ai_paused, Ordering::Relaxed));
    }
}

/// 打っている途中に AI で変換してよいか (ライブ変換の清書・句読点での変換)。chars 字の変換が、直近に測った速さで 1.5 秒より早く終わりそうで、
/// その場の上限の残りが 1.5 秒以上あるとき (残りが少ないまま試すと、切れても遅いとは分からず、毎回試しては切っていた。再点検 N1)。
/// だめなら辞書だけで変換する (Space を押せば AI で変換し直す)
pub fn ai_while_typing(chars: usize) -> bool {
    let slow = Duration::from_millis(SLOW_MS);
    !HEALTH.ai.slow_for_at(chars, now_ms()) && budgeted(slow).is_some_and(|left| left >= slow)
}

/// 合わせて chars 字の読みの採点が、直近に測った速さで 1.5 秒以上かかりそうか (3 分で測り直す)。そうなら母音の直しはしない
pub fn scoring_slow_for(chars: usize) -> bool {
    HEALTH.scoring.slow_for_at(chars, now_ms())
}

/// 問い合わせの上限を、その場の上限の残りで切る。残りが無ければ None (問い合わせない)
fn budgeted(limit: Duration) -> Option<Duration> {
    match DEADLINE.with(|deadline| deadline.get()) {
        None => Some(limit),
        Some(deadline) => {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            (!left.is_zero()).then(|| limit.min(left))
        }
    }
}

fn wait<T>(
    runtime: &tokio::runtime::Runtime,
    breaker: &Breaker,
    limit: Duration,
    what: &'static str,
    call: impl std::future::Future<Output = Result<tonic::Response<T>, tonic::Status>>,
) -> anyhow::Result<T> {
    wait_marked(runtime, breaker, None, limit, what, call)
}

/// mark: 遅いかどうかを測る問い合わせ (AI の変換・読みの採点) と、その字数。問い合わせて待った時間を残す (問い合わせずに返した失敗は残さない。レビュー M3)
fn wait_marked<T>(
    runtime: &tokio::runtime::Runtime,
    breaker: &Breaker,
    mark: Option<(&SlowMark, usize)>,
    limit: Duration,
    what: &'static str,
    call: impl std::future::Future<Output = Result<tonic::Response<T>, tonic::Status>>,
) -> anyhow::Result<T> {
    if breaker.open() {
        return Err(Unresponsive(what).into());
    }
    // その場の上限を使い切っていれば問い合わせない (相手のせいではないので、問い合わせを控える印は付けない)
    let Some(budget) = budgeted(limit) else {
        return Err(Unresponsive(what).into());
    };
    let started = std::time::Instant::now();
    let result = runtime.block_on(async { time::timeout(budget, call).await });
    let waited = started.elapsed();
    match result {
        Ok(response) => {
            breaker.succeeded();
            if let Some((mark, chars)) = mark {
                mark.answered_at(waited, chars, now_ms());
            }
            Ok(response?.into_inner())
        }
        // その場の上限で切った (問い合わせごとの上限より前): 相手が遅いとは限らないので、問い合わせを控える印は付けない。
        // 2026-10-29: ただし AI は「少なくともこれだけ遅い」を残す (打っている途中の AI をしばらくやめる。ai_while_typing)。AI でない問い合わせは、
        // キーの上限 (2 秒) 以上待っても返事が無ければ、上限で切れたのと同じに扱う (前は印を付けず、次のキー・次のアプリでまた止まった。
        // 新しいアプリでは Activate で 3 秒・フォーカスの移りで 2 秒。レビュー M1)。
        // 利用者が待っている動作の上限 (AWAITED_LIMIT) で切ったときも控えない (前の AI の変換に時間を使っただけ。控えると、続けて押した
        // Space が「返事をしない間」の道で確定し、候補を選べなかった。3 回目の点検 1)
        Err(_) if budget < limit && (mark.is_some() || budget < KEY_LIMIT || AWAITED.with(|awaited| awaited.get())) => {
            if let Some((mark, chars)) = mark {
                mark.cut_at(waited, budget, chars, now_ms());
            }
            crate::debug_log::write(&format!("IPC cut by the event budget: {what} ({} ms)", budget.as_millis()));
            Err(Unresponsive(what).into())
        }
        Err(_) => {
            if let Some((mark, chars)) = mark {
                mark.cut_at(waited, budget, chars, now_ms());
            }
            let cool = breaker.failed();
            crate::guard::record(&format!("no response to {what} within {} ms; not asking again for {} ms", budget.as_millis(), cool));
            crate::debug_log::write(&format!("IPC timeout: {what} ({} ms), cooling down {cool} ms", budget.as_millis()));
            Err(Unresponsive(what).into())
        }
    }
}

/// 候補窓は見た目だけなので、返事が来なくても打つ処理は止めない (ほかの失敗はそのまま返す)。
/// 2026-10-29: 候補窓がまだ受け付けていない (Unavailable。IME が有効になったときに ui.exe がまだ起きていなかった) ときも止めない (レビュー L7)。
/// 頼めなかったら数える (見えていないものを、見えている扱いにしない。レビュー L3)
fn wait_ui<T>(
    runtime: &tokio::runtime::Runtime,
    health: &Health,
    what: &'static str,
    call: impl std::future::Future<Output = Result<tonic::Response<T>, tonic::Status>>,
) -> anyhow::Result<()> {
    match wait(runtime, &health.ui, UI_LIMIT, what, call) {
        Ok(_) => Ok(()),
        Err(e) if e.is::<Unresponsive>()
            || e.downcast_ref::<tonic::Status>().is_some_and(|status| status.code() == tonic::Code::Unavailable) => {
            health.ui_failures.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

// connect to kkc server
#[derive(Debug, Clone)]
pub struct IPCService {
    // kkc server client
    azookey_client: AzookeyServiceClient<tonic::transport::channel::Channel>,
    // candidate window server client
    window_client: WindowServiceClient<tonic::transport::channel::Channel>,
    runtime: Arc<tokio::runtime::Runtime>,
    health: &'static Health,
}

#[derive(Debug, Clone, Default)]
pub struct Candidates {
    pub texts: Vec<String>,
    pub sub_texts: Vec<String>,
    pub hiragana: String,
    pub corresponding_count: Vec<i32>,
}

/// 2026-10-19 (security review #9): 候補窓 (ui.exe) は、この IME 本体と同じ保護された場所 (Program Files\HaniWord) の下で
/// 動いているものだけを相手にする。IME 本体が Releases\<版> にあればその 2 つ上 (Program Files\HaniWord)。
/// それ以外 (開発・試験のビルド) は今までどおり名前だけで確かめる (2026-10-20: 利用者が変えられる環境変数は見ない。Fable の再調査 L3)
fn protected_root() -> Option<std::path::PathBuf> {
    let dir = super::auto_english::dll_dir()?;
    let parent = dir.parent()?;
    if parent.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Releases")) {
        return parent.parent().map(|p| p.to_path_buf());
    }
    None
}

/// 通信路を開く。利用者 (セッション) ごとの名前を先に、無ければ以前の名前 (サーバーが古い版のとき)
fn open_pipe(base: &str, executable: &str) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    let root = if executable.eq_ignore_ascii_case("ui.exe") { protected_root() } else { None };
    let open = |path: &str| {
        let handle = shared::winsec::open_client_pipe_in(path, executable, root.as_deref())?;
        unsafe { NamedPipeClient::from_raw_handle(handle.into_raw_handle()) }
    };
    match open(&shared::winsec::pipe_name(base)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => open(&shared::winsec::legacy_pipe_name(base)),
        other => other,
    }
}

impl IPCService {
    pub fn new() -> Result<Self> {
        Self::new_for("haniword_server", "haniword_ui")
    }

    fn new_for(server_base: &str, ui_base: &str) -> Result<Self> {
        Self::new_with(server_base, "azookey-server.exe", ui_base, "ui.exe", &HEALTH)
    }

    /// つなぐ相手の名前付きパイプと、相手の実行ファイルの名前 (試験では偽のサーバーを立てるので、試験の実行ファイルの名前にする)
    fn new_with(server_base: &str, server_exe: &str, ui_base: &str, ui_exe: &str, health: &'static Health) -> Result<Self> {
        let runtime = tokio::runtime::Runtime::new()?;
        let server_channel = runtime.block_on(connect_channel("http://[::]:50051", server_base, server_exe))?;
        // 2026-10-29: 候補窓 (ui.exe) がまだ受け付けていなくても、変換はできるようにする (候補窓には、次に頼むときにつなぐ。
        // 前は両方そろうまでつなげず、ui.exe が起きるまで、ひらがなだけだった。レビュー L7)
        let ui_channel = match runtime.block_on(connect_channel("http://[::]:50052", ui_base, ui_exe)) {
            Ok(channel) => channel,
            Err(error) => {
                crate::debug_log::write(&format!("candidate window not ready ({error}); connecting when it is first needed"));
                let _entered = runtime.enter();
                lazy_channel("http://[::]:50052", ui_base, ui_exe)?
            }
        };

        let azookey_client = AzookeyServiceClient::new(server_channel);
        let window_client = WindowServiceClient::new(ui_channel);
        tracing::debug!("Connected to server: {:?}", azookey_client);

        Ok(Self {
            azookey_client,
            window_client,
            runtime: Arc::new(runtime),
            health,
        })
    }
}

/// 2026-10-29: 試験の中で立てる偽の変換サーバー・候補窓 (ipc_service と composition の試験で使う。本番のパイプは使わない)
#[cfg(test)]
pub(crate) mod test_services {
    use super::*;

    /// IME の状態 (プロセスで 1 つ) を書き換える試験は、これを持って 1 つずつ流す
    pub(crate) static GLOBAL_STATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 名前付きパイプで受け付けた通信路を順に渡す (tonic の Server に渡す)
    macro_rules! incoming {
        ($base:expr) => {{
            let name = shared::winsec::pipe_name($base);
            async_stream::stream! {
                let mut first = true;
                loop {
                    let server = match tokio::net::windows::named_pipe::ServerOptions::new().first_pipe_instance(first).create(&name) {
                        Ok(server) => server,
                        Err(error) => { yield Err(error); break; }
                    };
                    first = false;
                    if let Err(error) = server.connect().await { yield Err(error); break; }
                    yield Ok::<_, std::io::Error>(FakePipe(server));
                }
            }
        }};
    }

    /// 試験の中で立てる偽の変換サーバー・候補窓。名前ごとに決めた時間だけ待ってから空の返事をする。変換サーバーへの問い合わせは
    /// 1 本ずつ順に受ける (本物と同じ。切られた問い合わせも、終わるまで次を待たせる)
    #[derive(Clone)]
    pub(crate) struct Fake {
        delays: Arc<std::collections::HashMap<&'static str, Duration>>,
        queue: Arc<tokio::sync::Mutex<()>>,
    }

    impl Fake {
        /// delays: 問い合わせの名前 (convert・learn…) と、返事をするまでの時間 (ms)
        pub(crate) fn new(delays: &[(&'static str, u64)]) -> Self {
            Fake {
                delays: Arc::new(delays.iter().map(|&(name, ms)| (name, Duration::from_millis(ms))).collect()),
                queue: Arc::new(tokio::sync::Mutex::new(())),
            }
        }

        /// 偽の変換サーバーを名前付きパイプ base で受け付ける
        pub(crate) fn serve_server(&self, runtime: &tokio::runtime::Runtime, base: &str) {
            let incoming = incoming!(base);
            runtime.spawn(tonic::transport::Server::builder()
                .add_service(p::azookey_service_server::AzookeyServiceServer::new(self.clone())).serve_with_incoming(incoming));
        }

        /// 偽の候補窓を名前付きパイプ base で受け付ける
        pub(crate) fn serve_ui(&self, runtime: &tokio::runtime::Runtime, base: &str) {
            let incoming = incoming!(base);
            runtime.spawn(tonic::transport::Server::builder()
                .add_service(p::window_service_server::WindowServiceServer::new(self.clone())).serve_with_incoming(incoming));
        }

        async fn pause(&self, what: &'static str, queued: bool) {
            let delay = self.delays.get(what).copied().unwrap_or_default();
            let queue = self.queue.clone();
            let _ = tokio::spawn(async move {
                let _turn = if queued { Some(queue.lock_owned().await) } else { None };
                time::sleep(delay).await;
            }).await;
        }
    }

    macro_rules! fake_service {
        (@reply $res:ty, $request:ident) => { <$res>::default() };
        (@reply $res:ty, $request:ident, $make:expr) => { $make };
        ($trait:path, $queued:expr, $($name:ident: $req:ty => $res:ty $(= |$request:ident| $make:expr)?),* $(,)?) => {
            #[tonic::async_trait]
            impl $trait for Fake {
                $(#[allow(unused_variables)]
                async fn $name(&self, request: tonic::Request<$req>) -> Result<tonic::Response<$res>, tonic::Status> {
                    self.pause(stringify!($name), $queued).await;
                    $(let $request = request.into_inner();)?
                    Ok(tonic::Response::new(fake_service!(@reply $res, request $(, $make)?)))
                })*
            }
        };
    }

    /// 偽の変換: 読みを 2 字ずつの文節に分け、辞書だけの変換は頭に d、AI の変換は A を付ける (どちらで変換したか・文節の区切りが見えるように)
    fn fake_convert(request: p::ConvertRequest) -> p::ConvertResponse {
        let mark = if request.dictionary_only { "d" } else { "A" };
        let chars: Vec<char> = request.hiragana.chars().collect();
        p::ConvertResponse {
            segments: chars.chunks(2).map(|chunk| p::Segment { text: format!("{mark}{}", chunk.iter().collect::<String>()), length: chunk.len() as i32 }).collect(),
            ai_paused: false,
        }
    }

    use shared::proto as p;
    fake_service!(p::azookey_service_server::AzookeyService, true,
        append_text: p::AppendTextRequest => p::AppendTextResponse = |_request| p::AppendTextResponse { composing_text: Some(Default::default()) },
        remove_text: p::RemoveTextRequest => p::RemoveTextResponse,
        shrink_text: p::ShrinkTextRequest => p::ShrinkTextResponse,
        move_cursor: p::MoveCursorRequest => p::MoveCursorResponse,
        clear_text: p::ClearTextRequest => p::ClearTextResponse,
        set_context: p::SetContextRequest => p::SetContextResponse,
        update_config: p::UpdateConfigRequest => p::UpdateConfigResponse,
        edit_text: p::EditTextRequest => p::EditTextResponse,
        convert: p::ConvertRequest => p::ConvertResponse = |request| fake_convert(request),
        list_candidates: p::ListCandidatesRequest => p::ListCandidatesResponse,
        learn: p::LearnRequest => p::LearnResponse,
        score_readings: p::ScoreReadingsRequest => p::ScoreReadingsResponse,
        // 偽の変換サーバーでは 空ネコ だけが学習した語
        forget: p::ForgetRequest => p::ForgetResponse = |request| p::ForgetResponse { forgotten: (request.text == "空ネコ") as i32 },
        reset_learning: p::ResetLearningRequest => p::ResetLearningResponse,
    );
    fake_service!(p::window_service_server::WindowService, false,
        show_window: p::EmptyResponse => p::EmptyResponse,
        hide_window: p::EmptyResponse => p::EmptyResponse,
        set_candidate: p::SetCandidateRequest => p::EmptyResponse,
        set_selection: p::SetSelectionRequest => p::EmptyResponse,
        set_window_position: p::SetPositionRequest => p::EmptyResponse,
        set_input_mode: p::SetInputModeRequest => p::EmptyResponse,
    );

    struct FakePipe(tokio::net::windows::named_pipe::NamedPipeServer);

    impl tonic::transport::server::Connected for FakePipe {
        type ConnectInfo = ();
        fn connect_info(&self) {}
    }

    impl tokio::io::AsyncRead for FakePipe {
        fn poll_read(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>)
            -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.0).poll_read(cx, buf)
        }
    }

    impl tokio::io::AsyncWrite for FakePipe {
        fn poll_write(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &[u8])
            -> std::task::Poll<std::io::Result<usize>> {
            std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
        }
        fn poll_flush(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.0).poll_flush(cx)
        }
        fn poll_shutdown(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
        }
    }

    /// 偽の変換サーバー・候補窓を立て、つないだものを返す (具合は試験ごとに別。ほかの試験の問い合わせを止めない)。
    /// delays: 問い合わせの名前 (convert・learn…) と、返事をするまでの時間 (ms)
    pub(crate) fn fake_services(tag: &str, delays: &[(&'static str, u64)]) -> (tokio::runtime::Runtime, IPCService, &'static Health) {
        let exe = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned();
        let base = format!("haniword_fake_{tag}_{}", std::process::id());
        let ui_base = format!("{base}_ui");
        let fake = Fake::new(delays);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        fake.serve_server(&runtime, &base);
        fake.serve_ui(&runtime, &ui_base);
        let health: &'static Health = Box::leak(Box::new(Health::new()));
        let ipc = connect_when_ready(&base, &exe, &ui_base, health);
        (runtime, ipc, health)
    }

    /// 偽の変換サーバーが受け付け始めるまで待ってつなぐ (候補窓は、まだ受け付けていなければ次に頼むときにつなぐ)
    pub(crate) fn connect_when_ready(base: &str, exe: &str, ui_base: &str, health: &'static Health) -> IPCService {
        for _ in 0..50 {
            if let Ok(ipc) = IPCService::new_with(base, exe, ui_base, exe, health) {
                return ipc;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the fake services never became ready");
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use super::test_services::fake_services;

    /// ^ _ の上付き・下付きを、隔離したサーバーを相手に打つ (AZK_SCRIPT_PAIR_FIXTURE を指定したときだけ。本番のサーバーは使わない)
    #[test]
    fn isolated_script_typing() {
        let Ok(directory) = std::env::var("AZK_SCRIPT_PAIR_FIXTURE") else { return };
        let root = std::path::PathBuf::from(directory);
        let stage = root.join("stage");
        assert!(stage.join("isolated_security_fixture.json").is_file());
        assert_eq!(std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap()).canonicalize().unwrap(),
            root.join("appdata").canonicalize().unwrap());
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
        }
        let server_base = format!("haniword_rpc_test_scripts_{}", std::process::id());
        let ui_base = format!("haniword_ui_test_scripts_{}", std::process::id());
        let spawn = |name: &str, variable: &str, base: &str| {
            Child(std::process::Command::new(stage.join(name)).env(variable, base)
                .stdout(std::fs::File::create(root.join(format!("{name}.stdout.txt"))).unwrap())
                .stderr(std::fs::File::create(root.join(format!("{name}.stderr.txt"))).unwrap())
                .spawn().unwrap())
        };
        let mut server = spawn("azookey-server.exe", "AZK_TEST_PIPE", &server_base);
        let mut ui = spawn("ui.exe", "AZK_TEST_UI_PIPE", &ui_base);
        let mut ready = false;
        for _ in 0..300 {
            assert!(server.0.try_wait().unwrap().is_none(), "Fixture server exited");
            assert!(ui.0.try_wait().unwrap().is_none(), "Fixture UI exited");
            let probe = |base: &str, executable: &str| shared::winsec::open_client_pipe(&shared::winsec::pipe_name(base), executable).is_ok();
            if probe(&server_base, "azookey-server.exe") && probe(&ui_base, "ui.exe") { ready = true; break; }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(ready, "Fixture services never became ready");
        let mut ipc = IPCService::new_for(&server_base, &ui_base).unwrap();
        let typed = super::super::composition::verify_script_typing(&mut ipc);
        assert!(server.0.try_wait().unwrap().is_none());
        std::fs::write(root.join("script_typing.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "status":"verified","typed":typed,"production_pipe_used":false,"user_keystrokes_sent":false})).unwrap()).unwrap();
    }

    /// 2026-11-08 (仕事 45 の 3): カーソルの右の字を、隔離したサーバーに SetContext で送り、辞書だけの変換 (ライブ変換の表示と同じ問い合わせ) に
    /// 効くかを確かめる (AZK_RIGHT_CONTEXT_FIXTURE を指定したときだけ。本番のサーバー・パイプ・利用者のキーボードは使わない)。
    /// 右の字ありとなしの変換の結果を right_context.json に書く。改行で始まる右の字は、変換サーバーが空にする (なしと同じ)
    #[test]
    fn isolated_right_context() {
        let Ok(directory) = std::env::var("AZK_RIGHT_CONTEXT_FIXTURE") else { return };
        let root = std::path::PathBuf::from(directory);
        let stage = root.join("stage");
        assert!(stage.join("isolated_security_fixture.json").is_file());
        assert_eq!(std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap()).canonicalize().unwrap(),
            root.join("appdata").canonicalize().unwrap());
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
        }
        let server_base = format!("haniword_rpc_test_right_{}", std::process::id());
        let ui_base = format!("haniword_ui_test_right_{}", std::process::id());
        let spawn = |name: &str, variable: &str, base: &str| {
            Child(std::process::Command::new(stage.join(name)).env(variable, base)
                .stdout(std::fs::File::create(root.join(format!("{name}.stdout.txt"))).unwrap())
                .stderr(std::fs::File::create(root.join(format!("{name}.stderr.txt"))).unwrap())
                .spawn().unwrap())
        };
        let mut server = spawn("azookey-server.exe", "AZK_TEST_PIPE", &server_base);
        let mut ui = spawn("ui.exe", "AZK_TEST_UI_PIPE", &ui_base);
        let mut ready = false;
        for _ in 0..600 {
            assert!(server.0.try_wait().unwrap().is_none(), "Fixture server exited");
            assert!(ui.0.try_wait().unwrap().is_none(), "Fixture UI exited");
            let probe = |base: &str, executable: &str| shared::winsec::open_client_pipe(&shared::winsec::pipe_name(base), executable).is_ok();
            if probe(&server_base, "azookey-server.exe") && probe(&ui_base, "ui.exe") { ready = true; break; }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(ready, "Fixture services never became ready");
        let mut ipc = IPCService::new_for(&server_base, &ui_base).unwrap();
        let mut convert = |left: &str, right: &str, reading: &str| -> String {
            ipc.set_context(left.into(), right.into()).unwrap();
            ipc.convert_with(reading.into(), String::new(), true, true).unwrap().into_iter().map(|(text, _)| text).collect()
        };
        let cases = [("", "を操作する", "きかい"), ("", "薬を飲む", "きく"), ("古い薬が", "", "きく")];
        let mut rows = Vec::new();
        let mut changed = 0;
        for (left, right, reading) in cases {
            let without = convert(left, "", reading);
            let with = convert(left, right, reading);
            // 右の字が改行で始まれば、右の字なしと同じ
            let after_line_break = convert(left, &format!("\r\n{right}"), reading);
            let again = convert(left, "", reading);
            assert_eq!(after_line_break, without, "{reading}");
            assert_eq!(again, without, "{reading}: the right context stayed after it was cleared");
            changed += usize::from(with != without);
            rows.push(serde_json::json!({"left": left, "right": right, "reading": reading, "without_right": without, "with_right": with}));
        }
        assert!(server.0.try_wait().unwrap().is_none());
        std::fs::write(root.join("right_context.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "rows": rows, "changed_by_right_context": changed,
            "production_pipe_used": false, "user_keystrokes_sent": false})).unwrap()).unwrap();
        // 右の字だけの 2 つ (機械・効く) のどちらかは、右の字で変わる
        assert!(rows.iter().take(2).any(|row| row["with_right"] != row["without_right"]), "{rows:?}");
    }

    /// 2026-11-08 (仕事 52): 「AI を休んでいる」の印は、AI ありの変換 (Space・清書) の答えからだけ受ける。辞書だけの変換 (仮表示) の答えはいつも印が無いので替えない。
    /// 古い変換サーバー (印を付けない) なら、いつも休んでいない扱い
    #[test]
    fn ai_paused_follows_only_ai_conversions() {
        let paused = shared::proto::ConvertResponse { segments: vec![], ai_paused: true };
        let resumed = shared::proto::ConvertResponse { segments: vec![], ai_paused: false };
        super::note_ai_paused(&paused, false);
        assert!(super::ai_paused());
        super::note_ai_paused(&resumed, true);
        assert!(super::ai_paused(), "a dictionary-only preview must not clear the mark");
        super::note_ai_paused(&resumed, false);
        assert!(!super::ai_paused());
        let old_server = shared::proto::ConvertResponse { segments: vec![], ..Default::default() };
        super::note_ai_paused(&old_server, false);
        assert!(!super::ai_paused());
    }

    /// 2026-10-29: Activate でつなげなかったら、裏で 1 本ずつつなぎ直す。間は 2 秒・4 秒・8 秒・10 秒・10 秒… (終わった時刻から)。つながったら扱いを戻す
    #[test]
    fn reconnects_in_the_background_with_growing_waits() {
        let state = super::Reconnect::new();
        // つながっていれば試さない
        assert!(!state.begin(10_000));
        state.failed_at_activation(10_000);
        assert!(state.disconnected());
        // すぐのキーでは試さない
        assert!(!state.begin(11_000));
        // 2 秒たって試す。試しているあいだは次を始めない
        assert!(state.begin(12_000));
        assert!(!state.begin(20_000));
        // 失敗 (13 秒に終わった): 次は 4 秒あと
        assert_eq!(state.failed(13_000), 1);
        assert!(!state.begin(16_000));
        assert!(state.begin(17_000));
        assert_eq!(state.failed(17_500), 2);
        assert!(!state.begin(25_000));
        assert!(state.begin(25_500));
        // 3 回目からは 10 秒 (16 秒にしない)、何度失敗しても 10 秒
        assert_eq!(state.failed(30_000), 3);
        assert!(!state.begin(39_999));
        assert!(state.begin(40_000));
        for _ in 0..10 { state.failed(100_000); }
        assert!(!state.begin(109_999));
        assert!(state.begin(110_000));
        state.connected();
        assert!(!state.disconnected() && !state.begin(500_000));
    }

    /// 2026-10-29: 受け付けるが返事をしない偽の変換サーバー・候補窓 (この試験の中で名前付きパイプを立てる。本番のパイプは使わない) につないで
    /// 最初のやり取りをしても、UI スレッドはその場の上限 (EVENT_BUDGET の代わりに 600ms) を超えて待たない (写経 10-29 で固まった件)
    #[test]
    fn a_silent_server_never_holds_the_ui_thread_past_the_budget() {
        use tokio::net::windows::named_pipe::ServerOptions;
        let exe = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned();
        let base = format!("haniword_silent_test_{}", std::process::id());
        let ui_base = format!("{base}_ui");
        let fake = tokio::runtime::Runtime::new().unwrap();
        for name in [shared::winsec::pipe_name(&base), shared::winsec::pipe_name(&ui_base)] {
            fake.spawn(async move {
                // 受け付けて、つないだまま何も返さない
                let mut held = vec![];
                loop {
                    let Ok(server) = ServerOptions::new().first_pipe_instance(held.is_empty()).create(&name) else { break };
                    if server.connect().await.is_err() { break; }
                    held.push(server);
                }
            });
        }
        std::thread::sleep(Duration::from_millis(300));
        let started = std::time::Instant::now();
        let health: &'static Health = Box::leak(Box::new(Health::new()));
        let result = {
            let _budget = EventBudget::start_with(Duration::from_millis(600));
            IPCService::new_with(&base, &exe, &ui_base, &exe, health).and_then(|mut ipc| ipc.append_text(String::new()).map(|_| ()))
        };
        let elapsed = started.elapsed();
        // 偽のサーバーに本当につながってから、返事が無くて上限で切れた (つなぐ前にはねられたのではない)
        let error = result.err().expect("the silent server answered");
        assert!(error.chain().any(|e| e.is::<Unresponsive>()), "{error:?}");
        assert!(elapsed >= Duration::from_millis(450), "gave up too early ({elapsed:?}): {error:?}");
        assert!(elapsed < Duration::from_millis(1200), "waited {elapsed:?}");
        // 上限で切っただけ (キーの上限より短い) なので、問い合わせを控える印は付けない
        assert!(!health.server.open());
    }

    /// 2026-10-29 (レビュー H1): 遅いが答える変換サーバー (AI の変換に 1.3 秒。読みは 11 字)。打っている途中の AI はその場の上限 (EVENT_BUDGET の代わりに 1 秒) で
    /// 切るが、Space の変換 (利用者が待っている) は上限をかけずに答えを待つ。切った変換がサーバーで続いていて、その後ろに並んでも待つ。
    /// 待った時間はその場の上限に数えない
    #[test]
    fn space_waits_for_a_slow_but_answering_server() {
        let (_fake, mut ipc, health) = fake_services("h1", &[("convert", 1300)]);
        let _budget = EventBudget::start_with(Duration::from_millis(1000));
        assert!(ipc.set_context(String::new(), String::new()).is_ok());
        let started = std::time::Instant::now();
        let space = {
            let _awaited = AwaitedBudget::begin();
            ipc.convert("きょうはいいてんきです".into(), String::new())
        };
        assert!(space.is_ok(), "{space:?}");
        assert!(started.elapsed() >= Duration::from_millis(1250), "{:?}", started.elapsed());
        assert!(budgeted(KEY_LIMIT).unwrap() > Duration::from_millis(800), "the awaited conversion was charged to the budget");
        // 打っている途中の AI の変換 (ライブ変換の清書など): 上限の残りで切る。相手が遅いだけかもしれないので、問い合わせを控える印は付けない
        let started = std::time::Instant::now();
        let typing = ipc.convert_with("きょうはいいてんきです".into(), String::new(), false, false);
        assert!(typing.unwrap_err().is::<Unresponsive>());
        assert!(started.elapsed() < Duration::from_millis(1300), "{:?}", started.elapsed());
        assert!(!health.server.open());
        // Space: サーバーはまだ切った変換を続けている。その後ろに並んでも答えを待ち、ひらがなで確定しない
        let started = std::time::Instant::now();
        let space = {
            let _awaited = AwaitedBudget::begin();
            ipc.convert("きょうはいいてんきです".into(), String::new())
        };
        assert!(space.is_ok(), "{space:?}");
        assert!(started.elapsed() >= Duration::from_millis(1300), "{:?}", started.elapsed());
        // 11 字に 1.5 秒以上かかったので、1 字あたりの速さで見積もると、11 字以上は打っている途中に AI で変換しない (5 字なら変換する)
        assert!(health.ai.slow_for_at(11, now_ms()));
        assert!(!health.ai.slow_for_at(5, now_ms()));
    }

    /// 2026-10-29 (レビュー M1): 返事をしない変換サーバー。新しいアプリで Activate (空の追加) がキーの上限 (2 秒) で切れたら問い合わせを控え、
    /// 続くフォーカスの移り (文脈) では待たない (前は Activate の 3 秒をその場の上限で切って印を付けず、続けて 2 秒止まった)
    #[test]
    fn a_new_app_stalls_once_not_twice() {
        let (_fake, mut ipc, health) = fake_services("m1a", &[("append_text", 10_000), ("set_context", 10_000)]);
        let started = std::time::Instant::now();
        {
            let _activate = EventBudget::start();
            assert!(ipc.append_text(String::new()).unwrap_err().is::<Unresponsive>());
        }
        assert!(started.elapsed() < Duration::from_millis(2600), "{:?}", started.elapsed());
        assert!(health.server.open());
        let started = std::time::Instant::now();
        {
            let _focus = EventBudget::start();
            assert!(ipc.set_context("前の文".into(), String::new()).unwrap_err().is::<Unresponsive>());
        }
        assert!(started.elapsed() < Duration::from_millis(100), "{:?}", started.elapsed());
    }

    /// 2026-10-29 (レビュー M1): その場の上限で切った問い合わせでも、キーの上限 (2 秒) 以上待っていれば問い合わせを控える。
    /// AI の変換は遅いだけかもしれないので控えず、「遅い」の印を付ける (ライブ変換の清書・句読点での AI をしばらくやめる)
    #[test]
    fn a_budget_cut_after_two_seconds_counts_as_a_stall() {
        let (_fake, mut ipc, health) = fake_services("m1b", &[("learn", 10_000)]);
        {
            let _budget = EventBudget::start_with(Duration::from_millis(2200));
            assert!(ipc.learn(vec!["きょう".into()], vec!["今日".into()]).unwrap_err().is::<Unresponsive>());
        }
        assert!(health.server.open());

        let (_fake, mut ipc, health) = fake_services("m1c", &[("convert", 10_000)]);
        {
            let _budget = EventBudget::start_with(Duration::from_millis(2200));
            assert!(ipc.convert_with("きょう".into(), String::new(), false, false).unwrap_err().is::<Unresponsive>());
        }
        assert!(!health.server.open());
        assert!(health.ai.slow_for_at(3, now_ms()));
    }

    /// 2026-10-29 (レビュー M3・再点検 N1): 「遅い」の印は 1 字あたりの速さで持ち、これから変換する字数で見積もる。3 分で消える。
    /// 上限で切ったときは「少なくともこれだけ遅い」で、前の (もっと遅い) 値は下げない。問い合わせずに返した失敗 (控えている・上限を使い切った) は残さない
    #[test]
    fn slow_marks_expire_and_count_only_calls_that_were_sent() {
        let slow = Duration::from_millis(SLOW_MS);
        let mark = SlowMark::new();
        assert!(!mark.slow_for_at(100, 1000));
        // 20 字に 0.3 秒: 20 字は速い、200 字は遅い。3 分で消える
        mark.answered_at(Duration::from_millis(300), 20, 1000);
        assert!(!mark.slow_for_at(20, 1001));
        assert!(mark.slow_for_at(200, 1001));
        assert!(!mark.slow_for_at(200, 1000 + SLOW_FOR_MS));
        // 10 字を 1.5 秒の上限で切った: 10 字は遅い。短い文 (2 字) の答えでは替えない (毎回かかる固定の時間が 1 字あたりに大きく入るので。3 回目の点検 2)
        mark.cut_at(slow, slow, 10, 5000);
        assert!(mark.slow_for_at(10, 5001));
        mark.answered_at(Duration::from_millis(30), 2, 6000);
        assert!(mark.slow_for_at(10, 6001));
        let fresh_short = SlowMark::new();
        fresh_short.answered_at(Duration::from_millis(300), 2, 6000);
        assert!(!fresh_short.slow_for_at(15, 6001), "a 2-character answer set the speed");
        // 1.5 秒の上限で切ったら、待ちが少し短くても、その字数は 1.5 秒以上かかるものとして持つ (前の値も下げない)
        let short = SlowMark::new();
        short.cut_at(Duration::from_millis(1490), Duration::from_millis(1490), 10, 7000);
        assert!(short.slow_for_at(10, 7001) && !short.slow_for_at(9, 7001));
        mark.cut_at(Duration::from_millis(100), slow, 10, 7000);
        assert!(mark.slow_for_at(10, 7001));
        // 上限の残りが少なくて早く切れた (0.5 秒) ときは、待った時間だけ (実際より遅いと決めない。3 回目の点検 3)
        let early = SlowMark::new();
        early.cut_at(Duration::from_millis(500), Duration::from_millis(500), 10, 8000);
        assert!(!early.slow_for_at(10, 8001) && early.slow_for_at(30, 8001));

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let fresh = SlowMark::new();
        let cooling = Breaker::new();
        cooling.failed();
        let refused: anyhow::Result<i32> = wait_marked(&runtime, &cooling, Some((&fresh, 10)), CONVERT_LIMIT, "test", std::future::pending());
        assert!(refused.unwrap_err().is::<Unresponsive>());
        {
            let _spent = EventBudget::start_with(Duration::ZERO);
            let skipped: anyhow::Result<i32> = wait_marked(&runtime, &Breaker::new(), Some((&fresh, 10)), CONVERT_LIMIT, "test", std::future::pending());
            assert!(skipped.unwrap_err().is::<Unresponsive>());
        }
        assert_eq!(fresh.at_ms.load(Ordering::Relaxed), 0, "a call that was never sent left a mark");
    }

    /// 2026-10-29 (レビュー M5): 裏でつながったものを UI スレッドが受け取るまでは、次の試しを始めない (2 本目の裏のスレッドを立てない)。
    /// つながったら試している印も下りる。ほかの道筋でつながっていて捨てたときも下りる
    #[test]
    fn a_connection_waiting_to_be_taken_blocks_new_attempts() {
        let state = Reconnect::new();
        state.failed_at_activation(10_000);
        assert!(state.begin(12_000));
        assert!(!state.begin(60_000));
        state.connected();
        assert!(!state.disconnected() && !state.begin(60_000));
        state.failed_at_activation(70_000);
        assert!(state.begin(72_000));
        state.discarded();
        assert!(state.begin(72_001));
    }

    /// 2026-10-29 (再点検の試験の穴): 裏でつながったものは PENDING を通って 1 回だけ受け取られる。本物の受け渡しの道 (Handover) をそのまま通す。
    /// ①受け取るまで次の試しを始めない ②UI スレッドが空の通信路に入れ、つながった扱いにする ③もうつながっていたら、裏でつながったものは捨てる
    /// (取り残さない) ④つながった印を付けると、受け取られていない古いものを捨てる ⑤IME の状態を取れなかった Activate のものも次のキーで受け取る
    /// ⑥裏のスレッドが途中でパニックしても、試している印を残さない ⑦ほかの道筋でもうつながっていれば、それを使う
    #[test]
    fn a_background_connection_goes_through_pending_once() {
        let (_fake, ipc, _health) = fake_services("m5", &[]);
        let handover = Handover::new();
        let pending = |h: &Handover| h.lock_pending().is_some();
        // ①②
        handover.connection.failed_at_activation(10_000);
        assert!(handover.connection.begin(12_000));
        handover.attempt(|| Ok(ipc.clone()));
        assert!(pending(&handover) && handover.connection.disconnected());
        assert!(!handover.connection.begin(60_000), "a second attempt started while one waits to be taken");
        let mut slot = None;
        assert!(handover.pick_up(&mut slot).0);
        assert!(slot.is_some() && !pending(&handover) && !handover.connection.disconnected());
        assert!(!handover.pick_up(&mut slot).0);
        // ③ 試しているあいだに、ほかの道筋でつながった
        handover.connection.failed_at_activation(70_000);
        assert!(handover.connection.begin(72_000));
        handover.connection.connected();
        handover.attempt(|| Ok(ipc.clone()));
        assert!(!pending(&handover));
        // ④
        handover.connection.failed_at_activation(80_000);
        assert!(handover.connection.begin(82_000));
        handover.attempt(|| Ok(ipc.clone()));
        assert!(pending(&handover));
        assert!(handover.mark_connected().is_some(), "the stale connection was not handed back to be dropped outside the state");
        assert!(!pending(&handover) && !handover.connection.begin(90_000));
        // ⑤
        assert!(handover.install(ipc.clone(), None).is_empty());
        assert!(pending(&handover) && handover.connection.disconnected() && !handover.connection.begin(200_000));
        let mut empty = None;
        assert!(handover.pick_up(&mut empty).0 && empty.is_some() && !handover.connection.disconnected());
        // Activate が入れ替えた前の通信路も、IME の状態を離してから捨てるよう返す
        let mut occupied = Some(ipc.clone());
        assert_eq!(handover.install(ipc.clone(), Some(&mut occupied)).len(), 1);
        // ⑥
        handover.connection.failed_at_activation(300_000);
        assert!(handover.connection.begin(302_000));
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handover.attempt(|| panic!("connecting panicked"))));
        assert!(panicked.is_err());
        // (失敗の時刻は本物の時計なので、間が過ぎたことにするには十分先の時刻で試す)
        assert!(!pending(&handover) && handover.connection.begin(10_000_000), "a panic left the attempt marked");
        // ⑦ 失敗した試しのあとでも、IME の状態にもう通信路があれば、つながった扱いにする (裏では試さない)
        handover.attempt(|| anyhow::bail!("no server"));
        let mut shared = Some(ipc);
        assert!(!handover.pick_up(&mut shared).0);
        assert!(!handover.connection.disconnected() && !handover.connection.begin(1_000_000));
    }

    /// 2026-10-29: その場の上限を狭めると、元の上限より短く切り、落とすと元に戻る。入れ子の上限は外側のまま
    #[test]
    fn narrowed_budgets_restore_the_outer_budget() {
        assert_eq!(budgeted(Duration::from_secs(5)), Some(Duration::from_secs(5)));
        // 2026-10-29 (再点検 N1・N2): 打っている途中の AI は、上限の残りが 1.5 秒以上あるときだけ。利用者が待っている動作の上限は
        // 外側の上限の代わりにいまから AWAITED_LIMIT まで (無しにはしない)。落とすと外側の残りに戻る
        assert!(ai_while_typing(5));
        {
            let _short = EventBudget::start_with(Duration::from_millis(1000));
            assert!(!ai_while_typing(5));
            {
                let _awaited = AwaitedBudget::begin_with(Duration::from_millis(500));
                let left = budgeted(CONVERT_LIMIT).unwrap();
                assert!(left > Duration::from_millis(400) && left <= Duration::from_millis(500), "{left:?}");
            }
            assert!(budgeted(CONVERT_LIMIT).unwrap() <= Duration::from_millis(1000));
        }
        let _outer = EventBudget::start_with(Duration::from_secs(3));
        let inner = EventBudget::start();
        assert!(budgeted(Duration::from_secs(5)).unwrap() <= Duration::from_secs(3));
        drop(inner);
        assert!(budgeted(Duration::from_secs(5)).unwrap() > Duration::from_millis(2500));
        {
            let _narrow = NarrowBudget::within(Duration::from_millis(100));
            assert!(budgeted(Duration::from_secs(5)).unwrap() <= Duration::from_millis(100));
        }
        assert!(budgeted(Duration::from_secs(5)).unwrap() > Duration::from_millis(2500));
    }

    /// 2026-10-28: サーバー・候補窓の名前付きパイプが無ければ、つなぐのはすぐ失敗する (Activate・つなぎ直しで打つ手を止めない)
    #[test]
    fn connecting_without_the_services_fails_quickly() {
        let started = std::time::Instant::now();
        let base = format!("haniword_absent_{}", std::process::id());
        assert!(IPCService::new_for(&base, &format!("{base}_ui")).is_err());
        assert!(started.elapsed() < Duration::from_millis(1500), "{:?}", started.elapsed());
    }

    /// 2026-10-29 (レビュー L7): 候補窓 (ui.exe) がまだ受け付けていなくても、変換サーバーにつながれば変換できる。候補窓への頼みは、
    /// 失敗を数えて打つ処理を止めない。候補窓が受け付け始めたら、次に頼むときにつなぐ
    #[test]
    fn the_candidate_window_can_come_up_later() {
        use super::test_services::{connect_when_ready, Fake};
        let exe = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned();
        let base = format!("haniword_fake_l7_{}", std::process::id());
        let ui_base = format!("{base}_ui");
        let fake = Fake::new(&[]);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        fake.serve_server(&runtime, &base);
        let health: &'static Health = Box::leak(Box::new(Health::new()));
        let mut ipc = connect_when_ready(&base, &exe, &ui_base, health);
        assert!(ipc.set_context(String::new(), String::new()).is_ok());
        let started = std::time::Instant::now();
        assert!(ipc.set_input_mode("A").is_ok());
        assert!(started.elapsed() < Duration::from_millis(1500), "{:?}", started.elapsed());
        assert_eq!(health.ui_failures.load(Ordering::Relaxed), 1);
        fake.serve_ui(&runtime, &ui_base);
        let reached = (0..30).any(|_| {
            let before = health.ui_failures.load(Ordering::Relaxed);
            assert!(ipc.set_input_mode("A").is_ok());
            let ok = health.ui_failures.load(Ordering::Relaxed) == before;
            if !ok { std::thread::sleep(Duration::from_millis(100)); }
            ok
        });
        assert!(reached, "the candidate window was never reached after it came up");
    }

    #[test]
    fn isolated_client_server_ui_roundtrip() {
        let Ok(directory) = std::env::var("AZK_CLIENT_PAIR_FIXTURE") else { return };
        let root = std::path::PathBuf::from(directory);
        let stage = root.join("stage");
        assert!(stage.join("isolated_security_fixture.json").is_file());
        assert_eq!(std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap()).canonicalize().unwrap(),
            root.join("appdata").canonicalize().unwrap());
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
        }
        let server_base = format!("haniword_rpc_test_pair_{}", std::process::id());
        let ui_base = format!("haniword_ui_test_pair_{}", std::process::id());
        let spawn = |name: &str, variable: &str, base: &str| {
            Child(std::process::Command::new(stage.join(name)).env(variable, base)
                .stdout(std::fs::File::create(root.join(format!("{name}.stdout.txt"))).unwrap())
                .stderr(std::fs::File::create(root.join(format!("{name}.stderr.txt"))).unwrap())
                .spawn().unwrap())
        };
        let mut server = spawn("azookey-server.exe", "AZK_TEST_PIPE", &server_base);
        let mut ui = spawn("ui.exe", "AZK_TEST_UI_PIPE", &ui_base);
        let mut ready = false;
        for _ in 0..300 {
            assert!(server.0.try_wait().unwrap().is_none(), "Fixture server exited");
            assert!(ui.0.try_wait().unwrap().is_none(), "Fixture UI exited");
            let probe = |base: &str, executable: &str| shared::winsec::open_client_pipe(&shared::winsec::pipe_name(base), executable).is_ok();
            if probe(&server_base, "azookey-server.exe") && probe(&ui_base, "ui.exe") { ready = true; break; }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(ready, "Fixture services never became ready");
        let mut ipc = IPCService::new_for(&server_base, &ui_base).unwrap();
        use shared::proto::edit_text_request::Op;
        let (reading, _, raw) = ipc.edit_text_raw(Op::Insert, "kyouha".into(), 0).unwrap();
        assert_eq!(raw, "kyouha");
        assert_eq!(reading, "\u{304d}\u{3087}\u{3046}\u{306f}");
        let converted = ipc.convert_with(reading, String::new(), true, true).unwrap();
        assert!(!converted.is_empty());
        let (reading, _, raw) = ipc.edit_text_raw(Op::DeleteBackward, String::new(), 0).unwrap();
        assert_eq!(reading, "\u{304d}\u{3087}\u{3046}");
        assert_eq!(raw, "kyou");
        ipc.set_window_position(40, 40, 64, 200).unwrap();
        ipc.set_candidates(vec!["Backspace\u{1f}fixture".into(), "\u{78ba}\u{8a8d}\u{1f}fixture".into()]).unwrap();
        ipc.set_selection(0).unwrap();
        ipc.set_input_mode("A").unwrap();
        ipc.set_input_mode("\u{3042}").unwrap();
        ipc.show_window().unwrap();
        std::thread::sleep(Duration::from_millis(700));
        ipc.hide_window().unwrap();
        assert!(ipc.set_input_mode("not-a-mode").is_err());
        ipc.clear_text().unwrap();
        let (reading, _, raw) = ipc.edit_text_raw(Op::MoveCursor, String::new(), 0).unwrap();
        assert!(reading.is_empty() && raw.is_empty());
        let mixed_loanwords = super::super::composition::verify_mixed_loanwords(&mut ipc);
        assert!(server.0.try_wait().unwrap().is_none() && ui.0.try_wait().unwrap().is_none());
        std::fs::write(root.join("client_pair.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "status":"verified", "real_client_ipc_used":true, "dictionary_conversion":true,
            "backspace":true,"candidate_ui_rpcs":true,"input_modes":true,"mixed_loanwords":mixed_loanwords,
            "production_pipe_used":false,"user_keystrokes_sent":false})).unwrap()).unwrap();
    }
}

// implement methods to interact with kkc server
impl IPCService {
    /// 読みを編集し、打たれている文字の列 (英語の自動判別用) も返す
    pub fn edit_text_raw(
        &mut self,
        op: shared::proto::edit_text_request::Op,
        text: String,
        offset: i32,
    ) -> anyhow::Result<(String, i32, String)> {
        let request = tonic::Request::new(shared::proto::EditTextRequest {
            op: op as i32,
            text,
            offset,
        });
        let response = wait(&self.runtime, &self.health.server, KEY_LIMIT, "edit_text", self.azookey_client.edit_text(request))?;
        Ok((checked_reading(response.hiragana)?, response.cursor, checked_reading(response.raw)?))
    }

    /// 読みを編集する (変換はしない)。戻り値は (読み, キャレット位置)
    #[tracing::instrument]
    pub fn edit_text(
        &mut self,
        op: shared::proto::edit_text_request::Op,
        text: String,
        offset: i32,
    ) -> anyhow::Result<(String, i32)> {
        let request = tonic::Request::new(shared::proto::EditTextRequest {
            op: op as i32,
            text,
            offset,
        });
        let response = wait(&self.runtime, &self.health.server, KEY_LIMIT, "edit_text", self.azookey_client.edit_text(request))?;
        Ok((checked_reading(response.hiragana)?, response.cursor))
    }

    /// 読みを変換し、文節ごとの (表記, 読みの文字数) を返す
    #[tracing::instrument]
    pub fn convert(&mut self, hiragana: String, context: String) -> anyhow::Result<Vec<(String, i32)>> {
        self.convert_with(hiragana, context, false, false)
    }

    pub fn convert_with(
        &mut self,
        hiragana: String,
        context: String,
        fast: bool,
        dictionary_only: bool,
    ) -> anyhow::Result<Vec<(String, i32)>> {
        let chars = hiragana.chars().count();
        let reading = hiragana.clone();
        let request = tonic::Request::new(shared::proto::ConvertRequest {
            hiragana,
            context,
            fast,
            dictionary_only,
            plain: super::tex::active(),
            refine: false,
        });
        let mark = (!dictionary_only).then_some((&self.health.ai, chars));
        let response = wait_marked(&self.runtime, &self.health.server, mark, CONVERT_LIMIT, "convert", self.azookey_client.convert(request))?;
        note_ai_paused(&response, dictionary_only);
        Ok(checked_segments(&reading, response.segments.into_iter().map(|segment| (segment.text, segment.length)).collect()))
    }

    /// 2026-11-06 (仕事 29 の 2): ライブ変換の清書 (AI の変換、convert_with(…, false, false) と同じ)。その場の上限で切れたら、問い合わせを裏で
    /// 待ち続ける (poll_late_refine で受け取る)。切れたことは今までどおり Unresponsive で返し、遅い印も今までどおり付く
    pub fn convert_refine(&mut self, hiragana: String, context: String) -> anyhow::Result<Vec<(String, i32)>> {
        if self.health.server.open() || budgeted(CONVERT_LIMIT).is_none() {
            return Err(Unresponsive("convert").into());
        }
        drop_late_refine();
        let chars = hiragana.chars().count();
        let request = tonic::Request::new(shared::proto::ConvertRequest {
            hiragana: hiragana.clone(),
            context,
            fast: false,
            dictionary_only: false,
            plain: super::tex::active(),
            // 2026-11-08 (仕事 52): 清書の印。重いときに AI を休んでいる間、AI が戻ったかの試しに使ってよい
            refine: true,
        });
        let mut client = self.azookey_client.clone();
        let started = std::time::Instant::now();
        let mut handle = self.runtime.spawn(async move { client.convert(request).await });
        let result = wait_marked(&self.runtime, &self.health.server, Some((&self.health.ai, chars)), CONVERT_LIMIT, "convert", async {
            match (&mut handle).await {
                Ok(response) => response,
                Err(error) => Err(tonic::Status::cancelled(error.to_string())),
            }
        });
        match result {
            Ok(response) => {
                note_ai_paused(&response, false);
                Ok(checked_segments(&hiragana, response.segments.into_iter().map(|segment| (segment.text, segment.length)).collect()))
            }
            Err(error) => {
                if error.is::<Unresponsive>() && !handle.is_finished() {
                    LATE_REFINE.with(|cell| *cell.borrow_mut() = Some(LateRefine { reading: hiragana, chars, started, handle, runtime: self.runtime.clone(), health: self.health }));
                }
                Err(error)
            }
        }
    }

    /// 2026-10-28: ローマ字の母音の抜けを直すときの、補った読みの候補の採点。readings と同じ並びで (辞書だけの変換の点数, 読みだけの AI の点数)。
    /// この RPC が無い古い変換サーバーなら None (直さずに今までどおり)
    pub fn score_readings(&mut self, readings: Vec<String>, context: String) -> anyhow::Result<Option<Vec<(Option<f32>, Option<f32>)>>> {
        let chars = readings.iter().map(|reading| reading.chars().count()).sum();
        let request = tonic::Request::new(shared::proto::ScoreReadingsRequest { readings, context });
        match wait_marked(&self.runtime, &self.health.server, Some((&self.health.scoring, chars)), CONVERT_LIMIT, "score_readings", self.azookey_client.score_readings(request)) {
            Ok(r) => Ok(Some(r.dict.iter().zip(&r.has_dict).zip(r.lm.iter().zip(&r.has_lm))
                .map(|((d, has_d), (l, has_l))| (has_d.then_some(*d), has_l.then_some(*l))).collect())),
            Err(e) if e.downcast_ref::<tonic::Status>().is_some_and(|s| s.code() == tonic::Code::Unimplemented) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// 2026-10-29: 学習した語を忘れる (候補の一覧での Ctrl+Delete)。忘れた学習の語の数を返す (0 なら学習した語ではなかった。辞書の語は消えない)
    pub fn forget(&mut self, reading: String, text: String) -> anyhow::Result<i32> {
        let request = tonic::Request::new(shared::proto::ForgetRequest { reading, text });
        let response = wait(&self.runtime, &self.health.server, CONVERT_LIMIT, "forget", self.azookey_client.forget(request))?;
        Ok(response.forgotten)
    }

    /// 確定した文節を学習させる
    #[tracing::instrument]
    pub fn learn(&mut self, readings: Vec<String>, texts: Vec<String>) -> anyhow::Result<i32> {
        let request = tonic::Request::new(shared::proto::LearnRequest {
            readings,
            texts,
            context: String::new(),
        });
        let response = wait(&self.runtime, &self.health.server, CONVERT_LIMIT, "learn", self.azookey_client.learn(request))?;
        Ok(response.learned)
    }

    /// 読みに対応する候補の一覧
    #[tracing::instrument]
    pub fn list_candidates(&mut self, hiragana: String, context: String) -> anyhow::Result<Vec<String>> {
        self.list_candidates_with(hiragana, context, false)
    }

    /// 2026-10-22 (M3.6): 候補と、候補ごとの注記 (「種類<TAB>注記」、無ければ空。誤用の「本来は〜」と異字同訓)
    /// notes: 注記が要るか (設定でどちらかがオンのとき。要らなければ変換サーバーは作らない)
    /// 2026-11-08: 3 つ目は候補ごとに、学習した語を含むか (Ctrl+Delete で忘れられる候補。古い変換サーバーなら全部偽)
    pub fn list_candidates_noted(&mut self, hiragana: String, context: String, notes: bool) -> anyhow::Result<(Vec<String>, Vec<String>, Vec<bool>)> {
        let chars = hiragana.chars().count();
        let request = tonic::Request::new(shared::proto::ListCandidatesRequest {
            hiragana,
            context,
            single_kanji: false,
            plain: super::tex::active(),
            notes,
        });
        let response = wait(&self.runtime, &self.health.server, CONVERT_LIMIT, "list_candidates", self.azookey_client.list_candidates(request))?;
        Ok(without_private_use_noted(response.candidates, response.notes, response.learned, chars))
    }

    pub fn list_candidates_with(
        &mut self,
        hiragana: String,
        context: String,
        single_kanji: bool,
    ) -> anyhow::Result<Vec<String>> {
        let chars = hiragana.chars().count();
        let request = tonic::Request::new(shared::proto::ListCandidatesRequest {
            hiragana,
            context,
            single_kanji,
            plain: super::tex::active(),
            notes: false,
        });
        let response = wait(&self.runtime, &self.health.server, CONVERT_LIMIT, "list_candidates", self.azookey_client.list_candidates(request))?;
        Ok(without_private_use(response.candidates, chars))
    }

    #[tracing::instrument]
    pub fn append_text(&mut self, text: String) -> anyhow::Result<Candidates> {
        let request = tonic::Request::new(shared::proto::AppendTextRequest {
            text_to_append: text,
        });

        // 2026-10-29: 空の追加 (IME が有効になったとき・つなぎ直したときに入力の状態を合わせる) だけなので、キーの上限で待つ (レビュー M1)
        let response = wait(&self.runtime, &self.health.server, KEY_LIMIT, "append_text", self.azookey_client.append_text(request))?;
        let composing_text = response.composing_text;

        let candidates = if let Some(composing_text) = composing_text {
            Candidates {
                texts: composing_text
                    .suggestions
                    .iter()
                    .map(|s| s.text.clone())
                    .collect(),
                sub_texts: composing_text
                    .suggestions
                    .iter()
                    .map(|s| s.subtext.clone())
                    .collect(),
                hiragana: checked_reading(composing_text.hiragana)?,
                corresponding_count: composing_text
                    .suggestions
                    .iter()
                    .map(|s| s.corresponding_count)
                    .collect(),
            }
        } else {
            anyhow::bail!("composing_text is None");
        };

        Ok(checked_candidates(candidates))
    }

    #[tracing::instrument]
    pub fn remove_text(&mut self) -> anyhow::Result<Candidates> {
        let request = tonic::Request::new(shared::proto::RemoveTextRequest {});
        let response = wait(&self.runtime, &self.health.server, CONVERT_LIMIT, "remove_text", self.azookey_client.remove_text(request))?;
        let composing_text = response.composing_text;

        let candidates = if let Some(composing_text) = composing_text {
            Candidates {
                texts: composing_text
                    .suggestions
                    .iter()
                    .map(|s| s.text.clone())
                    .collect(),
                sub_texts: composing_text
                    .suggestions
                    .iter()
                    .map(|s| s.subtext.clone())
                    .collect(),
                hiragana: checked_reading(composing_text.hiragana)?,
                corresponding_count: composing_text
                    .suggestions
                    .iter()
                    .map(|s| s.corresponding_count)
                    .collect(),
            }
        } else {
            anyhow::bail!("composing_text is None");
        };

        Ok(checked_candidates(candidates))
    }

    #[tracing::instrument]
    pub fn clear_text(&mut self) -> anyhow::Result<()> {
        let request = tonic::Request::new(shared::proto::ClearTextRequest {});
        let _response = wait(&self.runtime, &self.health.server, KEY_LIMIT, "clear_text", self.azookey_client.clear_text(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn shrink_text(&mut self, offset: i32) -> anyhow::Result<Candidates> {
        let request = tonic::Request::new(shared::proto::ShrinkTextRequest { offset });
        let response = wait(&self.runtime, &self.health.server, CONVERT_LIMIT, "shrink_text", self.azookey_client.shrink_text(request))?;
        let composing_text = response.composing_text;

        let candidates = if let Some(composing_text) = composing_text {
            Candidates {
                texts: composing_text
                    .suggestions
                    .iter()
                    .map(|s| s.text.clone())
                    .collect(),
                sub_texts: composing_text
                    .suggestions
                    .iter()
                    .map(|s| s.subtext.clone())
                    .collect(),
                hiragana: checked_reading(composing_text.hiragana)?,
                corresponding_count: composing_text
                    .suggestions
                    .iter()
                    .map(|s| s.corresponding_count)
                    .collect(),
            }
        } else {
            anyhow::bail!("composing_text is None");
        };

        Ok(checked_candidates(candidates))
    }

    /// 左の文脈と、カーソルの右の字 (2026-11-08、仕事 45。設定 input.right_context がオフ・取れないアプリでは空)
    pub fn set_context(&mut self, context: String, right_context: String) -> anyhow::Result<()> {
        let request = tonic::Request::new(shared::proto::SetContextRequest { context, right_context });
        let _response = wait(&self.runtime, &self.health.server, KEY_LIMIT, "set_context", self.azookey_client.set_context(request))?;

        Ok(())
    }
}

// implement methods to interact with candidate window server
impl IPCService {
    #[tracing::instrument]
    pub fn show_window(&mut self) -> anyhow::Result<()> {
        let request = tonic::Request::new(shared::proto::EmptyResponse {});
        wait_ui(&self.runtime, self.health, "show_window", self.window_client.show_window(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn hide_window(&mut self) -> anyhow::Result<()> {
        let request = tonic::Request::new(shared::proto::EmptyResponse {});
        wait_ui(&self.runtime, self.health, "hide_window", self.window_client.hide_window(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn set_window_position(
        &mut self,
        top: i32,
        left: i32,
        bottom: i32,
        right: i32,
    ) -> anyhow::Result<()> {
        let request = tonic::Request::new(shared::proto::SetPositionRequest {
            position: Some(shared::proto::WindowPosition {
                top,
                left,
                bottom,
                right,
            }),
        });
        wait_ui(&self.runtime, self.health, "set_window_position", self.window_client.set_window_position(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn set_candidates(&mut self, candidates: Vec<String>) -> anyhow::Result<()> {
        self.set_candidates_with_note(candidates, String::new())
    }

    /// 2026-11-08 (仕事 52): 候補の窓の下の欄に文言を添えて候補を出す (重いので AI を休んでいる間。古い候補の窓は文言を読まない)
    pub fn set_candidates_with_note(&mut self, candidates: Vec<String>, note: String) -> anyhow::Result<()> {
        let request = tonic::Request::new(shared::proto::SetCandidateRequest { candidates, note });
        wait_ui(&self.runtime, self.health, "set_candidate", self.window_client.set_candidate(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn set_selection(&mut self, index: i32) -> anyhow::Result<()> {
        let request = tonic::Request::new(shared::proto::SetSelectionRequest { index });
        wait_ui(&self.runtime, self.health, "set_selection", self.window_client.set_selection(request))?;

        Ok(())
    }

    #[tracing::instrument]
    pub fn set_input_mode(&mut self, mode: &str) -> anyhow::Result<()> {
        let request = tonic::Request::new(shared::proto::SetInputModeRequest {
            mode: mode.to_string(),
            // 2026-11-08 (仕事 52): 重いので AI を休んでいる間は札の右下に点 (知らせの設定がオンのとき)
            ai_paused: ai_paused() && ai_pause_notice_enabled(),
        });
        wait_ui(&self.runtime, self.health, "set_input_mode", self.window_client.set_input_mode(request))?;

        Ok(())
    }
}

/// 2026-10-13: 私用領域の字 (U+E000〜U+F8FF) を含む候補を外す。本体の辞書は Apple の端末向けで、あっぷる の第 1 候補が
/// U+F8FF (Apple のロゴ) だった。Windows のフォントでは空白か豆腐になり、候補一覧の先頭が空に見える
/// 候補と注記と学習の印 (同じ並び) から、私的利用の字の候補を除く (注記・印も一緒に)
/// 2026-11-23 (セキュリティの点検 F-3): 制御文字・双方向の向きの字を含む候補と、読みに比べて長すぎる候補も外す (acceptable_text)
fn without_private_use_noted(candidates: Vec<String>, notes: Vec<String>, learned: Vec<bool>, reading_chars: usize) -> (Vec<String>, Vec<String>, Vec<bool>) {
    let notes = notes.into_iter().chain(std::iter::repeat(String::new()));
    let learned = learned.into_iter().chain(std::iter::repeat(false));
    let mut lists = (Vec::new(), Vec::new(), Vec::new());
    for ((candidate, note), learned) in candidates.into_iter().zip(notes).zip(learned) {
        if candidate.chars().any(|ch| ('\u{e000}'..='\u{f8ff}').contains(&ch)) || !acceptable_text(&candidate, reading_chars) {
            continue;
        }
        lists.0.push(candidate);
        lists.1.push(note);
        lists.2.push(learned);
    }
    lists
}

fn without_private_use(candidates: Vec<String>, reading_chars: usize) -> Vec<String> {
    candidates.into_iter().filter(|c| !c.chars().any(|ch| ('\u{e000}'..='\u{f8ff}').contains(&ch)) && acceptable_text(c, reading_chars)).collect()
}

/// 2026-11-23 (セキュリティの点検 F-3): 変換サーバーの返事 (文節の表記と読みの長さ・候補・読み) は、使う前にここで確かめる。変換サーバーは利用者の
/// フォルダで動くので、同じ利用者の別のプログラムが置き換えると、管理者のアプリの未確定の文字列にも好きな字を入れられる。次は使わない:
/// 制御文字 (改行・タブを含む) と、双方向の向きを変える字 (U+202A〜U+202E・U+2066〜U+2069) を含む表記、読みに比べて長すぎる表記
/// (読みの字数の TEXT_PER_READING_CHAR 倍と TEXT_LIMIT_FLOOR 字の大きいほうを超える。登録した語の 短縮よみ (めあど → メールアドレス) も入る長さ)
const TEXT_PER_READING_CHAR: usize = 16;
const TEXT_LIMIT_FLOOR: usize = 256;

pub(crate) fn unsafe_text(text: &str) -> bool {
    text.chars().any(|c| c.is_control() || ('\u{202a}'..='\u{202e}').contains(&c) || ('\u{2066}'..='\u{2069}').contains(&c))
}

fn acceptable_text(text: &str, reading_chars: usize) -> bool {
    !unsafe_text(text) && text.chars().count() <= (reading_chars * TEXT_PER_READING_CHAR).max(TEXT_LIMIT_FLOOR)
}

/// 文節の返事 (表記, 読みの字数) を確かめる。表記が使えない・長さが 1 より小さい・前から足していって読みの字数を超える・足して読みの字数に
/// ならない (文節が 1 つでも) なら、読みのかなの 1 文節に戻す (Astra の点検 06: 前は文節が 1 つなら長さが合わなくてもその表記で読み全体を置いた)
pub(crate) fn checked_segments(reading: &str, segments: Vec<(String, i32)>) -> Vec<(String, i32)> {
    if segments.is_empty() {
        return segments;
    }
    let chars = reading.chars().count() as i64;
    let mut covered = 0i64;
    let mut ok = true;
    for (text, len) in &segments {
        let len = *len as i64;
        if len < 1 || covered + len > chars || !acceptable_text(text, len as usize) {
            ok = false;
            break;
        }
        covered += len;
    }
    if ok && covered == chars {
        return segments;
    }
    crate::debug_log::write(&format!("server reply rejected: {} segment(s) for {chars} chars", segments.len()));
    vec![(reading.to_string(), chars as i32)]
}

/// 変換サーバーが返した読み (edit_text・append_text など)。制御文字・双方向の向きの字を含むか、長すぎれば失敗にする (読みは IME の表示そのもの)
fn checked_reading(reading: String) -> anyhow::Result<String> {
    if unsafe_text(&reading) || reading.chars().count() > TEXT_LIMIT_FLOOR * 4 {
        anyhow::bail!("server reply rejected: reading");
    }
    Ok(reading)
}

/// append_text などの候補 (表記・下の字・読みの長さ)。使えない表記の候補は、並びをそろえたまま外す
fn checked_candidates(candidates: Candidates) -> Candidates {
    let reading_chars = candidates.hiragana.chars().count();
    let keep: Vec<bool> = candidates.texts.iter().enumerate()
        .map(|(i, text)| acceptable_text(text, reading_chars) && candidates.sub_texts.get(i).map_or(true, |sub| !unsafe_text(sub))
            && candidates.corresponding_count.get(i).map_or(true, |count| *count >= 0))
        .collect();
    fn pick<T>(items: Vec<T>, keep: &[bool]) -> Vec<T> {
        items.into_iter().enumerate().filter(|(i, _)| keep.get(*i).copied().unwrap_or(false)).map(|(_, item)| item).collect()
    }
    Candidates {
        texts: pick(candidates.texts, &keep),
        sub_texts: pick(candidates.sub_texts, &keep),
        hiragana: candidates.hiragana,
        corresponding_count: pick(candidates.corresponding_count, &keep),
    }
}

#[cfg(test)]
mod private_use_tests {
    #[test]
    fn apple_logo_is_not_a_candidate() {
        let got = super::without_private_use(vec!["\u{f8ff}".into(), "アップル".into(), "Apple".into(), "🍎\u{fe0f}".into()], 4);
        assert_eq!(got, vec!["アップル".to_string(), "Apple".into(), "🍎\u{fe0f}".into()]);
    }

    /// 2026-11-08: 私的利用の字の候補を除くとき、注記と学習の印も一緒に除く (並びがずれない)。足りない分は空・偽
    #[test]
    fn private_use_candidates_drop_their_notes_and_learned_marks() {
        let (candidates, notes, learned) = super::without_private_use_noted(
            vec!["\u{f8ff}".into(), "アップル".into(), "林檎".into()], vec!["誤用\tx".into(), String::new()], vec![true, false, true], 4);
        assert_eq!(candidates, vec!["アップル".to_string(), "林檎".into()]);
        assert_eq!(notes, vec![String::new(), String::new()]);
        assert_eq!(learned, vec![false, true]);
    }
}

/// 2026-11-23 (セキュリティの点検 F-3): 変換サーバーの返事を使う前に確かめる
#[cfg(test)]
mod reply_check_tests {
    use super::{checked_candidates, checked_reading, checked_segments, unsafe_text, without_private_use, without_private_use_noted, Candidates};

    fn kana(reading: &str) -> Vec<(String, i32)> {
        vec![(reading.to_string(), reading.chars().count() as i32)]
    }

    #[test]
    fn good_segments_pass_through() {
        let segments = vec![("今日は".to_string(), 4), ("いい天気".to_string(), 5)];
        assert_eq!(checked_segments("きょうはいいてんき", segments.clone()), segments);
        assert_eq!(checked_segments("あずーきー", vec![("azooKey".to_string(), 5)]), vec![("azooKey".to_string(), 5)]);
        // 登録した語の短縮よみ (2 字で長い表記) も通る
        let address = "someone.with.a.long.name@example.com".to_string();
        assert_eq!(checked_segments("めあど", vec![(address.clone(), 3)]), vec![(address, 3)]);
        assert_eq!(checked_segments("", vec![]), vec![]);
    }

    #[test]
    fn control_and_direction_characters_fall_back_to_the_reading() {
        for bad in ["今日\nは", "今日\tは", "今日\rは", "今日\u{0}は", "今日\u{7f}は", "今日\u{202e}は", "今日\u{202a}は", "今日\u{2066}は", "今日\u{2069}は"] {
            assert_eq!(checked_segments("きょうは", vec![(bad.to_string(), 4)]), kana("きょうは"), "{bad:?}");
            assert_eq!(checked_segments("きょうはいい", vec![(bad.to_string(), 4), ("いい".to_string(), 2)]), kana("きょうはいい"), "{bad:?}");
            assert!(unsafe_text(bad));
        }
        // ふつうの字・絵文字の異体字セレクタ・ゼロ幅接合子は通す
        for good in ["今日は", "🍎\u{fe0f}", "👨\u{200d}👩", "(^_^)", "H₂O"] {
            assert!(!unsafe_text(good), "{good:?}");
        }
    }

    #[test]
    fn bad_lengths_fall_back_to_the_reading() {
        // 負の長さ (足すと合う)・0 の長さ・足して合わない 2 つ以上の文節
        assert_eq!(checked_segments("きょうは", vec![("今日".to_string(), 6), ("は".to_string(), -2)]), kana("きょうは"));
        assert_eq!(checked_segments("きょうは", vec![("".to_string(), 0), ("今日は".to_string(), 4)]), kana("きょうは"));
        assert_eq!(checked_segments("きょうは", vec![("今日".to_string(), 3), ("は".to_string(), 2)]), kana("きょうは"));
        assert_eq!(checked_segments("きょうは", vec![("今日は".to_string(), -4)]), kana("きょうは"));
        // 文節が 1 つでも、長さが合わなければかなに (Astra の点検 06)。前から足して読みを超えるのも
        assert_eq!(checked_segments("あずーきー", vec![("azooKey".to_string(), 3)]), kana("あずーきー"));
        assert_eq!(checked_segments("きょうは", vec![("今日は".to_string(), 9)]), kana("きょうは"));
        assert_eq!(checked_segments("きょうは", vec![("今日は今日は".to_string(), 6), ("x".to_string(), -2)]), kana("きょうは"));
    }

    #[test]
    fn too_long_texts_fall_back_to_the_reading() {
        let long = "あ".repeat(257);
        assert_eq!(checked_segments("あ", vec![(long.clone(), 1)]), kana("あ"));
        assert_eq!(checked_segments("あ", vec![("あ".repeat(256), 1)]), vec![("あ".repeat(256), 1)]);
        // 文節ごと: 1 字の文節に長い表記
        let reading = "あ".repeat(20);
        assert_eq!(checked_segments(&reading, vec![("い".repeat(300), 1), ("あ".repeat(19), 19)]), kana(&reading));
        assert_eq!(checked_segments(&reading, vec![("い".repeat(320), 20)]), vec![("い".repeat(320), 20)]);
    }

    #[test]
    fn unusable_candidates_are_dropped() {
        assert_eq!(without_private_use(vec!["今日".into(), "今\n日".into(), "\u{202e}日今".into(), "あ".repeat(300), "京".into()], 3),
            vec!["今日".to_string(), "京".into()]);
        let (candidates, notes, learned) = without_private_use_noted(
            vec!["今日".into(), "今\t日".into(), "京".into()], vec!["a".into(), "b".into(), "c".into()], vec![false, true, true], 3);
        assert_eq!(candidates, vec!["今日".to_string(), "京".into()]);
        assert_eq!(notes, vec!["a".to_string(), "c".into()]);
        assert_eq!(learned, vec![false, true]);
        let got = checked_candidates(Candidates {
            texts: vec!["今日".into(), "今\n日".into(), "京".into(), "負".into()],
            sub_texts: vec!["".into(), "".into(), "\u{2067}".into(), "".into()],
            hiragana: "きょう".into(),
            corresponding_count: vec![3, 3, 3, -1],
        });
        assert_eq!(got.texts, vec!["今日".to_string()]);
        assert_eq!(got.sub_texts, vec!["".to_string()]);
        assert_eq!(got.corresponding_count, vec![3]);
    }

    #[test]
    fn unusable_readings_are_errors() {
        assert_eq!(checked_reading("きょうは".into()).unwrap(), "きょうは");
        assert!(checked_reading("きょう\nは".into()).is_err());
        assert!(checked_reading("\u{2066}きょう".into()).is_err());
        assert!(checked_reading("あ".repeat(2000)).is_err());
    }
}

#[cfg(test)]
mod unresponsive_tests {
    use super::{wait, wait_ui, Breaker, Unresponsive};
    use std::time::{Duration, Instant};

    fn never<T>() -> impl std::future::Future<Output = Result<tonic::Response<T>, tonic::Status>> {
        std::future::pending()
    }

    #[test]
    fn a_silent_server_costs_one_wait_then_fails_fast_until_it_answers_again() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let breaker = Breaker::new();
        let start = Instant::now();
        let first: anyhow::Result<i32> = wait(&runtime, &breaker, Duration::from_millis(150), "test", never());
        assert!(first.unwrap_err().is::<Unresponsive>());
        assert!(start.elapsed() >= Duration::from_millis(150) && start.elapsed() < Duration::from_millis(1500));
        // 少し前に返事が来なかったので、待たずに失敗する
        let start = Instant::now();
        let second: anyhow::Result<i32> = wait(&runtime, &breaker, Duration::from_millis(150), "test", never());
        assert!(second.unwrap_err().is::<Unresponsive>());
        assert!(start.elapsed() < Duration::from_millis(50));
        // 冷ます時間が過ぎたら問い合わせ、返事が来れば元に戻る
        breaker.until_ms.store(1, std::sync::atomic::Ordering::Relaxed);
        let answered = wait(&runtime, &breaker, Duration::from_millis(150), "test", async { Ok(tonic::Response::new(7)) });
        assert_eq!(answered.unwrap(), 7);
        assert!(!breaker.open());
        // サーバーが返したエラーは待ちきれなかったのとは別 (そのまま返す)
        let refused: anyhow::Result<i32> = wait(&runtime, &breaker, Duration::from_millis(150), "test",
            async { Err(tonic::Status::internal("refused")) });
        assert!(!refused.unwrap_err().is::<Unresponsive>());
        assert!(!breaker.open());
    }

    #[test]
    fn cooling_down_grows_but_stays_within_thirty_seconds() {
        let breaker = Breaker::new();
        let cools: Vec<u64> = (0..7).map(|_| breaker.failed()).collect();
        assert_eq!(cools, vec![3000, 6000, 12000, 24000, 30000, 30000, 30000]);
        breaker.streak.store(1, std::sync::atomic::Ordering::Relaxed);
        breaker.succeeded();
        assert!(!breaker.open());
    }

    #[test]
    fn a_silent_candidate_window_does_not_stop_typing() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let health: &'static super::Health = Box::leak(Box::new(super::Health::new()));
        let start = Instant::now();
        assert!(wait_ui(&runtime, health, "test_ui", never::<()>()).is_ok());
        assert!(start.elapsed() < Duration::from_millis(2500));
        // 頼めなかったことは数える (見えていない候補を、見えている扱いにしない)
        assert_eq!(health.ui_failures.load(std::sync::atomic::Ordering::Relaxed), 1);
        // 候補窓がまだ受け付けていない (Unavailable) ときも打つ処理は止めない。候補窓が返したほかのエラーはそのまま返す
        let fresh: &'static super::Health = Box::leak(Box::new(super::Health::new()));
        assert!(wait_ui(&runtime, fresh, "test_ui", async { Err::<tonic::Response<()>, _>(tonic::Status::unavailable("not yet")) }).is_ok());
        assert_eq!(fresh.ui_failures.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(wait_ui(&runtime, fresh, "test_ui", async { Err::<tonic::Response<()>, _>(tonic::Status::invalid_argument("bad mode")) }).is_err());
    }
}
