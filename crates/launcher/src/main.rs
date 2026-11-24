use shared::AppConfig;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::{env, thread};

mod update;

// A staged server executable is not a runnable bundle without its native runtime.
fn app_dir(exe_dir: &Path, appdata: &Path) -> PathBuf {
    let native_files = ["azookey-server.exe", "azookey-server.dll", "swiftCore.dll"];
    let has_backend = ["llama_cpu", "llama_cuda", "llama_vulkan"].iter()
        .any(|backend| exe_dir.join(backend).join("llama.dll").is_file());
    if native_files.iter().all(|name| exe_dir.join(name).is_file())
        && exe_dir.join("Dictionary").is_dir() && has_backend
    {
        exe_dir.to_path_buf()
    } else {
        appdata.join(shared::APP_DIR)
    }
}

fn main() -> anyhow::Result<()> {
    // 2026-10-20: GPU の DLL (vulkan-1.dll など) が無い PC で、変換サーバーが起動できないたびに窓が出て止まらないように
    shared::winsec::suppress_error_dialogs();
    if env::args().any(|argument| argument == "--export-client-settings") {
        if !shared::winsec::private_data_allowed() { anyhow::bail!("Settings export must run unelevated outside a sandbox"); }
        AppConfig::new().publish_client_settings()?;
        return Ok(());
    }
    let exe_dir = env::current_exe()?.parent().unwrap().to_path_buf();
    // 管理者として起動されたとき (ログオン時のタスク): 管理者権限が要るのは候補窓が UIAccess を取る最初の一瞬だけ。
    // 候補窓 (このフォルダの ui.exe) を起動したら、自分をふだんのユーザーの権限で起動し直し、変換サーバーはそちらで動かす。
    // 管理者権限のままでは、ユーザーが書き換えられる場所 (%APPDATA%) のものは何も動かさない・読まない
    let server_only = env::args().any(|a| a == "--server-only");
    let mut ui_process: Option<Child> = None;
    if shared::winsec::elevation_status()? {
        // この起動用はすぐ終わるので、候補窓の出力はつながない (つないだままだと、終わったあとの書き込みで候補窓が落ちる)
        if !server_only {
            if let Err(error) = Command::new(exe_dir.join("ui.exe")).current_dir(&exe_dir).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                eprintln!("[launcher]: failed to start ui.exe: {error:?}");
            }
        }
        // ログオン直後はエクスプローラーがまだ無いことがあるので、待つ
        let mut token = None;
        for _ in 0..120 {
            token = shared::winsec::desktop_user_token();
            if token.is_some() {
                break;
            }
            thread::sleep(Duration::from_secs(1));
        }
        let me = env::current_exe()?;
        match token.map(|token| shared::winsec::spawn_with_token(token.0, &me, "--server-only", &exe_dir)) {
            Some(Ok(())) => {}
            Some(Err(error)) => eprintln!("[launcher]: could not restart without elevation: {error:?}"),
            None => eprintln!("[launcher]: no desktop user token; not starting the server elevated"),
        }
        return Ok(());
    }
    let appdata = env::var_os("APPDATA").filter(|path| !path.is_empty())
        .ok_or_else(|| anyhow::anyhow!("APPDATA is unavailable"))?;
    let app = app_dir(&exe_dir, Path::new(&appdata));
    // 2026-10-29: 新しい版の知らせ (1 日 1 回、裏で GitHub の最新のリリースを確かめる)。利用者の権限で動くこちらだけ (管理者の起動用は上で終わる)
    update::spawn(Path::new(&appdata).join(shared::APP_DIR));
    let config = AppConfig::new();
    // GPU (Vulkan / CUDA) で変換サーバーが起動直後に落ちたことがあれば、印のファイルを置いて次からは CPU で動かす。
    // ドライバを更新したあとは、この印 (backend_failed.txt) を消せば GPU をもう一度試す
    let failed_marker = app.join("backend_failed.txt");
    let mut backend = config.zenzai.backend.clone();
    if backend != "cpu" && failed_marker.exists() {
        backend = "cpu".to_string();
    }
    // 2026-11-10: AVX2 の CPU 向けの版 (llama_cpu_avx2) で読み込みの段に落ちたら、この印を置いて以後は AVX 無しの版 (llama_cpu) を使う
    let avx2_marker = app.join("cpu_avx2_failed.txt");
    let avx2 = cpu_has_avx2();
    if !avx2 {
        record("the CPU lacks AVX2/FMA/F16C; using llama_cpu");
    }
    let original_path = env::var("PATH").unwrap_or_default();
    // 2026-11-05 (仕事 29): 起動して 60 秒以内に続けて終わった回数 (間を空けて起動し直し、4 回目は長く空ける)
    let mut quick_failures = 0u32;
    // 2026-11-10: 続けて終了コード 1 (受け口を作れない) で終わった回数
    let mut exit1_count = 0u32;
    // 2026-11-10: 4 回続けて落ちたあとは、印は置かずに CPU の版で起動し直す (次のログオンでは設定の版をもう一度試す)
    let mut force_cpu = false;

    loop {
        let has = |llama: Llama| app.join(llama.dir()).join("llama.dll").is_file();
        let llama = choose_llama(if force_cpu { "cpu" } else { backend.as_str() }, avx2, has, avx2_marker.exists());
        let backend_path = app.join(llama.dir());
        // その版の llama.dll が置かれていなければ起動しない (choose_llama は GPU・AVX2 の版が無ければ CPU の版を選ぶので、ここに来るのは CPU の版も無いとき)
        if !backend_path.join("llama.dll").is_file() {
            eprintln!("[launcher]: {} is missing", backend_path.join("llama.dll").display());
            break;
        }
        env::set_var("PATH", format!("{};{}", backend_path.to_string_lossy(), original_path));
        // 2026-11-19 (決め): AVX 無しの CPU の版 (GPU も使えない) で動かすときは、AI の入り切りが初めの値のままなら AI を切る (1 文に数秒かかるため)。
        // 利用者が自分でオンにしていればそのまま。速い版に戻ったら launcher が切った分だけ戻す。変換サーバーは起動のときに設定を読むので、起動の前に書く。
        // あわせて、実際に使う版と設定から下げたわけを ai_backend.json に書く (設定画面の「HaniWord について」が読む)
        let ai_off = apply_slow_cpu_setting(llama == Llama::Cpu);
        let gpu_failed = backend == "cpu" && config.zenzai.backend != "cpu";
        let reasons = fallback_reasons(&config.zenzai.backend, gpu_failed, force_cpu, avx2, avx2_marker.exists(), has, llama);
        write_backend_status(llama, &config.zenzai.backend, &reasons, ai_off);

        let started = Instant::now();
        let stage = Arc::new(AtomicU8::new(STAGE_NONE));
        let Some(mut server) = start_process(&app.join("azookey-server.exe"), &app, "[server]", Some(stage.clone())) else { break };
        if ui_process.is_none() && !server_only {
            ui_process = start_process(&exe_dir.join("ui.exe"), &exe_dir, "[ui]", None);
        }
        let status = server.wait();
        let code = status.as_ref().ok().and_then(|s| s.code());
        let ran = started.elapsed();
        let stage = stage.load(Ordering::SeqCst);
        // 2026-11-10: 落ちたときの手がかりの 1 行 (終了コードとその名前・動いた秒数・版・起動の段。打った字は書かない)
        let clue = format!("server exited ({}) after {}s on {}, stage {}", exit_code_text(code), ran.as_secs(), llama.dir(), stage_name(stage));
        exit1_count = if code == Some(1) { exit1_count + 1 } else { 0 };
        // 2026-11-05 (仕事 29): どんな理由で終わっても起動し直す (前は GPU で起動直後に落ちたときだけで、ほかは止まったままになり、
        // どのアプリもかなのまま入っていた。11-05 15:56〜16:01)。止めるのは、ログオフとシャットダウンのとき・終了コード 1 が続いたとき
        match after_exit(code, ran, llama, stage, session_ending(), quick_failures, exit1_count) {
            AfterExit::Stop(reason) => {
                record(&format!("{clue}; not restarting: {reason}"));
                break;
            }
            AfterExit::FallBack => {
                // 読み込みの段 (印が出る前・zenz の文脈を作っている間) に例外で落ちた: GPU の版なら CPU の版へ、AVX2 の版なら AVX 無しの版へ
                if llama.is_gpu() {
                    record(&format!("{clue}; falling back to cpu"));
                    let _ = std::fs::write(
                        &failed_marker,
                        format!("{} で変換サーバーが起動直後に落ちたので CPU で動かしています ({status:?})。このファイルを消すと次の起動で GPU をもう一度試します。\n", llama.dir()),
                    );
                    backend = "cpu".to_string();
                } else {
                    record(&format!("{clue}; falling back to llama_cpu"));
                    let _ = std::fs::write(
                        &avx2_marker,
                        format!("{} で変換サーバーが起動直後に落ちたので、AVX を使わない版で動かしています ({status:?})。このファイルを消すと次の起動でもう一度試します。\n", llama.dir()),
                    );
                }
                continue;
            }
            AfterExit::Restart(delay) => {
                quick_failures = if ran < QUICK_EXIT { quick_failures + 1 } else { 0 };
                record(&format!("{clue}; restarting in {}s", delay.as_secs()));
                thread::sleep(delay);
                continue;
            }
            AfterExit::RestartOnCpuLater(delay) => {
                // 2026-11-10: 前はここであきらめ、ログオンし直すまで変換サーバーが動かなかった (どのアプリもかなのまま)
                record(&format!("{clue}, {} quick exits in a row; restarting on cpu in {}s", quick_failures + 1, delay.as_secs()));
                quick_failures = 0;
                force_cpu = true;
                thread::sleep(delay);
                continue;
            }
        }
    }

    if let Some(mut ui) = ui_process {
        let _ = ui.wait();
    }

    Ok(())
}

/// 2026-11-10: llama.cpp の版 (%APPDATA%\HaniWord の下のフォルダ)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Llama {
    Vulkan,
    Cuda,
    /// AVX2・FMA・F16C を使う CPU の版
    CpuAvx2,
    /// AVX を使わない CPU の版 (どの x64 の CPU でも動く)
    Cpu,
}

impl Llama {
    fn dir(self) -> &'static str {
        match self {
            Llama::Vulkan => "llama_vulkan",
            Llama::Cuda => "llama_cuda",
            Llama::CpuAvx2 => "llama_cpu_avx2",
            Llama::Cpu => "llama_cpu",
        }
    }
    fn is_gpu(self) -> bool {
        matches!(self, Llama::Vulkan | Llama::Cuda)
    }
}

/// AVX2・FMA・F16C があるか。llama_cpu_avx2 と、GPU の版の中の CPU の部分 (ggml-cpu.dll) はこれを使い、無い CPU で読むと 0xC000001D で落ちる
fn cpu_has_avx2() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") && is_x86_feature_detected!("f16c")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// 使う版を決める。backend: 設定の裏方 (vulkan・cuda・cpu)、avx2: CPU に AVX2・FMA・F16C があるか、has: その版の llama.dll が置かれているか、
/// avx2_failed: AVX2 の版が読み込みの段で落ちた印があるか。AVX2 の無い CPU では GPU の版も選ばない (GPU の版の ggml-cpu も AVX2 で組んである)
fn choose_llama(backend: &str, avx2: bool, has: impl Fn(Llama) -> bool, avx2_failed: bool) -> Llama {
    let cpu = if avx2 && !avx2_failed && has(Llama::CpuAvx2) { Llama::CpuAvx2 } else { Llama::Cpu };
    let wanted = match backend {
        "vulkan" => Llama::Vulkan,
        "cuda" => Llama::Cuda,
        _ => cpu,
    };
    if wanted.is_gpu() && (!avx2 || !has(wanted)) { cpu } else { wanted }
}

/// 2026-11-10: 変換サーバーの起動の段。変換サーバーが標準エラーに 1 行ずつ出す印 (azookey_server.swift の Initialize) で進める
const STAGE_NONE: u8 = 0;
/// 変換サーバーの DLL を読み込み、Initialize に入った (印 "haniword-server: started")
const STAGE_STARTED: u8 = 1;
/// zenz の文脈 (GPU の準備) を作る温めの変換を始めた ("haniword-server: loading")
const STAGE_LOADING: u8 = 2;
/// 温めの変換が済んだ ("haniword-server: ready")
const STAGE_READY: u8 = 3;

fn stage_of_line(line: &str) -> Option<u8> {
    match line.trim() {
        "haniword-server: started" => Some(STAGE_STARTED),
        "haniword-server: loading" => Some(STAGE_LOADING),
        "haniword-server: ready" => Some(STAGE_READY),
        _ => None,
    }
}

fn stage_name(stage: u8) -> &'static str {
    match stage {
        STAGE_STARTED => "started",
        STAGE_LOADING => "loading",
        STAGE_READY => "ready",
        _ => "none",
    }
}

/// 終了コードを 16 進とよくある例外の名前で (手がかりの 1 行)
fn exit_code_text(code: Option<i32>) -> String {
    let Some(code) = code else { return "no code".to_string() };
    let name = match code as u32 {
        0xC000_0005 => " access violation",
        0xC000_001D => " illegal instruction",
        0xC000_0135 => " dll not found",
        0xC000_0139 => " entry point not found",
        0xC000_0409 => " stack buffer overrun",
        0xC000_00FD => " stack overflow",
        0xC000_0374 => " heap corruption",
        0x8000_0003 => " breakpoint",
        0xFFFF_FFFF => " terminated",
        _ => "",
    };
    format!("{code} = {:#010X}{name}", code as u32)
}

/// 2026-11-05 (仕事 29): 変換サーバーが終わったあとの扱い
#[derive(Debug, PartialEq, Eq)]
enum AfterExit {
    /// 起動し直さない (理由)
    Stop(&'static str),
    /// GPU・AVX2 の版で、読み込みの段に落ちた: 印を置き、CPU の版 (AVX2 の版なら AVX 無しの版) で起動し直す
    FallBack,
    /// 間を空けて起動し直す
    Restart(Duration),
    /// 短い間に 4 回続けて落ちた: 長く空けて、CPU の版で起動し直す
    RestartOnCpuLater(Duration),
}

/// これより短く動いて終わったら「続けて落ちた」に数える
const QUICK_EXIT: Duration = Duration::from_secs(60);

/// 続けて落ちたときの間 (1 回目・2 回目・3 回目)。4 回目は RESTART_LATER
const RESTART_DELAYS: [u64; 3] = [1, 5, 30];

/// 2026-11-10: 4 回続けて落ちたあとの間 (前はここであきらめていた)
const RESTART_LATER: Duration = Duration::from_secs(600);

/// 2026-11-10: 終了コード 1 (受け口を作れない) のあとの間と、続けて何回まで起動し直すか。更新の途中で前の変換サーバーが消えきっていないことがある
const EXIT1_DELAY: Duration = Duration::from_secs(60);
const EXIT1_RETRIES: u32 = 3;

/// code: 終了コード、ran: 動いた時間、llama: 使っていた版、stage: 起動の段、ending: ログオフ・シャットダウン中か、
/// quick_failures: それまでに続けて落ちた回数、exit1_count: 終了コード 1 で続けて終わった回数 (今回を含む)
fn after_exit(code: Option<i32>, ran: Duration, llama: Llama, stage: u8, ending: bool, quick_failures: u32, exit1_count: u32) -> AfterExit {
    if ending {
        return AfterExit::Stop("the session is ending");
    }
    // 2026-11-10: 終了コード 1 は、ほかの変換サーバーが動いているとは限らない (更新の途中・利用者が手で起動し直したとき)。1 分あけて 3 回まで試す
    if code == Some(1) {
        return if exit1_count <= EXIT1_RETRIES { AfterExit::Restart(EXIT1_DELAY) } else { AfterExit::Stop("the server stopped itself (exit code 1) 4 times in a row") };
    }
    // 例外のコード (0xC0000005・0x80000003 など)。-1 (0xFFFFFFFF) は Stop-Process・タスクマネージャーで止められたときのコードで、落ちたのではない
    // (前は起動直後に止められると GPU のせいとみなし、CPU に切り替えて印のファイルを残していた)
    let crashed = code.is_some_and(|code| (code as u32) >= 0x8000_0000 && code != -1);
    // 2026-11-10: 版のせいとみなすのは、印が 1 つも出ないうち (DLL の読み込み) と、zenz の文脈を作っている間 (loading と ready の間) に落ちたときだけ。
    // 前は「起動から 60 秒以内」で決めていて、設定・学習の壊れで起動直後に落ちる不具合でも GPU のせいにして、以後ずっと CPU で動かしていた
    let loading = stage == STAGE_NONE || stage == STAGE_LOADING;
    if crashed && loading && llama != Llama::Cpu {
        return AfterExit::FallBack;
    }
    let failures = if ran < QUICK_EXIT { quick_failures + 1 } else { 0 };
    match failures {
        0 => AfterExit::Restart(Duration::from_secs(RESTART_DELAYS[0])),
        n if (n as usize) <= RESTART_DELAYS.len() => AfterExit::Restart(Duration::from_secs(RESTART_DELAYS[n as usize - 1])),
        _ => AfterExit::RestartOnCpuLater(RESTART_LATER),
    }
}

/// ログオフ・シャットダウン中か (GetSystemMetrics の SM_SHUTTINGDOWN)
fn session_ending() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};
    unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) != 0 }
}

/// %APPDATA%\HaniWord\compat_log.txt に「時刻 launcher.exe pid 出来事」を 1 行足す (IME 本体の guard::record と同じ形。64KB を超えたら古いほうの半分を捨てる)
/// 2026-11-19: 遅い CPU の版を選んだときの AI の初めの値 (shared の ZenzaiConfig::apply_slow_cpu_default)。替えたときだけ settings.json に書く。
/// launcher が AI を切っている (切った印がある) なら真
fn apply_slow_cpu_setting(slow: bool) -> bool {
    let mut config = AppConfig::read();
    if config.zenzai.apply_slow_cpu_default(slow) {
        record(if slow { "AI turned off by default: the CPU lacks AVX2 and no GPU version can be used" } else { "AI turned back on: a faster llama version is used" });
        if let Err(error) = config.try_write() {
            record(&format!("cannot write settings: {error}"));
        }
    }
    !config.zenzai.enable && config.zenzai.auto_off == shared::AI_AUTO_OFF_SLOW_CPU
}

/// 2026-11-19: 設定の裏方 (setting) から下げたわけ (起きた順)。gpu_failed: GPU で起動直後に落ちた印があるか、この起動の中で落ちて CPU にした。
/// quick_exits: 続けて 4 回落ちたので、この起動だけ CPU の版にした
fn fallback_reasons(setting: &str, gpu_failed: bool, quick_exits: bool, avx2: bool, avx2_failed: bool, has: impl Fn(Llama) -> bool, chosen: Llama) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    let wanted_gpu = match setting {
        "vulkan" => Some(Llama::Vulkan),
        "cuda" => Some(Llama::Cuda),
        _ => None,
    };
    if let Some(gpu) = wanted_gpu {
        if !chosen.is_gpu() {
            reasons.push(if gpu_failed { "gpu_failed_before" } else if quick_exits { "quick_exits" } else if !avx2 { "no_avx2" } else if !has(gpu) { "gpu_missing" } else { "quick_exits" });
        }
    } else if quick_exits && chosen != Llama::CpuAvx2 && avx2 && !avx2_failed && has(Llama::CpuAvx2) {
        reasons.push("quick_exits");
    }
    if chosen == Llama::Cpu {
        let why = if !avx2 { "no_avx2" } else if avx2_failed { "avx2_failed_before" } else { "avx2_missing" };
        if !reasons.contains(&why) {
            reasons.push(why);
        }
    }
    reasons
}

/// 2026-11-19: 変換サーバーの「モデルを読めなかった (true)・読み直して読めた (false)」の行
fn model_event_of_line(line: &str) -> Option<bool> {
    match line.trim_end() {
        "haniword-server: model-failed" => Some(true),
        "haniword-server: model-loaded" => Some(false),
        _ => None,
    }
}

/// 2026-11-23 (セキュリティの点検 12): 変換サーバーが壊れた学習のファイルを memory_corrupt_<日時> に退けた行。記録 (compat_log.txt) に書く文を返す。
/// 行に入るのは壊れ方の種類・ファイルの数・退けた先の名前だけ (学習の中身は入らない)。長すぎる行は書かない
fn learning_quarantine_note(line: &str) -> Option<&str> {
    let note = line.trim_end().strip_prefix("haniword-server: ")?;
    (note.starts_with("learning files were corrupt") && note.len() <= 400).then_some(note)
}

/// ai_backend.json の model_error (読めなければ "load_failed"、読めたら消す)
fn with_model_error(mut status: shared::AiBackendStatus, failed: bool) -> shared::AiBackendStatus {
    if failed {
        status.extra.insert("model_error".into(), serde_json::Value::String("load_failed".into()));
    } else {
        status.extra.remove("model_error");
    }
    status
}

fn write_backend_status(llama: Llama, setting: &str, reasons: &[&str], ai_off: bool) {
    let status = shared::AiBackendStatus {
        llama: llama.dir().to_string(),
        setting: setting.to_string(),
        reasons: reasons.iter().map(|r| r.to_string()).collect(),
        ai_off: if ai_off { shared::AI_AUTO_OFF_SLOW_CPU.to_string() } else { String::new() },
        updated: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        extra: serde_json::Map::new(),
    };
    if let Err(error) = status.write() {
        record(&format!("cannot write the AI backend status: {error}"));
    }
}

fn record(event: &str) {
    eprintln!("[launcher]: {event}");
    if cfg!(test) { return; }
    let Some(appdata) = env::var_os("APPDATA") else { return };
    let path = Path::new(&appdata).join(shared::APP_DIR).join("compat_log.txt");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 64 * 1024) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let middle = text.len() / 2;
            let start = text.as_bytes()[middle..].iter().position(|&b| b == b'\n').map_or(text.len(), |i| middle + i + 1);
            let _ = std::fs::write(&path, &text[start..]);
        }
    }
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let line = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} launcher.exe pid={} {event}\n", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, std::process::id());
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = file.write_all(line.as_bytes());
    }
}

/// stage: 変換サーバーなら、標準エラーの印で起動の段を進める先
fn start_process(exe: &Path, dir: &Path, prefix: &str, stage: Option<Arc<AtomicU8>>) -> Option<Child> {
    let mut child = match Command::new(exe).current_dir(dir).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("[launcher]: failed to start {}: {error:?}", exe.display());
            return None;
        }
    };

    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stdout_reader = BufReader::new(stdout);
    let prefix_stdout = prefix.to_string();
    thread::spawn(move || {
        for line in stdout_reader.lines() {
            if let Ok(line) = line {
                println!("{}: {}", prefix_stdout, line);
            }
        }
    });

    let stderr = child.stderr.take().expect("Failed to capture stderr");
    let stderr_reader = BufReader::new(stderr);
    let prefix_stderr = prefix.to_string();
    thread::spawn(move || {
        for line in stderr_reader.lines() {
            if let Ok(line) = line {
                if let (Some(stage), Some(next)) = (&stage, stage_of_line(&line)) {
                    stage.fetch_max(next, Ordering::SeqCst);
                }
                // 2026-11-19: 変換サーバーが AI のモデルを読めなかった・読み直して読めた。記録に 1 行書き、ai_backend.json に書く (設定画面が読む)
                if stage.is_some() {
                    if let Some(note) = learning_quarantine_note(&line) {
                        record(note);
                    }
                    if let Some(failed) = model_event_of_line(&line) {
                        record(if failed { "the AI model could not be loaded; conversions use the dictionary only" } else { "the AI model was loaded after a retry" });
                        if let Some(status) = shared::AiBackendStatus::read() {
                            if let Err(error) = with_model_error(status, failed).write() {
                                record(&format!("cannot write the AI backend status: {error}"));
                            }
                        }
                    }
                }
                eprintln!("{}: {}", prefix_stderr, line);
            }
        }
    });

    Some(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = env::temp_dir().join(format!("azookey_launcher_{}_{}",
                std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn add(&self, name: &str) {
            let file = self.0.join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, b"test fixture").unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    /// 2026-11-05 (仕事 29): どんな理由で終わっても起動し直す。2026-11-10: 版のせいにするのは読み込みの段の落ちだけ・終了コード 1 は 3 回まで試す・
    /// 4 回続けて落ちたら 10 分あけて CPU の版で
    #[test]
    fn server_restarts_after_any_exit() {
        let secs = Duration::from_secs;
        let av = 0xC000_0005u32 as i32;
        let ill = 0xC000_001Du32 as i32;
        // Stop-Process (TerminateProcess の -1) で止められた・落ちた・ふつうに終わった: 1 秒あけて起動し直す
        assert_eq!(after_exit(Some(-1), secs(3600), Llama::Cpu, STAGE_READY, false, 0, 0), AfterExit::Restart(secs(1)));
        assert_eq!(after_exit(Some(av), secs(600), Llama::Vulkan, STAGE_READY, false, 0, 0), AfterExit::Restart(secs(1)));
        assert_eq!(after_exit(Some(0), secs(120), Llama::Cpu, STAGE_READY, false, 0, 0), AfterExit::Restart(secs(1)));
        assert_eq!(after_exit(None, secs(120), Llama::Cpu, STAGE_READY, false, 0, 0), AfterExit::Restart(secs(1)));
        // ログオフ・シャットダウンは起動し直さない
        assert!(matches!(after_exit(Some(-1), secs(5), Llama::Cpu, STAGE_READY, true, 0, 0), AfterExit::Stop(_)));
        // 終了コード 1 (受け口を作れない): 1 分あけて 3 回まで起動し直し、4 回続いたら止める
        assert_eq!(after_exit(Some(1), secs(5), Llama::Cpu, STAGE_READY, false, 0, 1), AfterExit::Restart(secs(60)));
        assert_eq!(after_exit(Some(1), secs(5), Llama::Cpu, STAGE_READY, false, 0, 3), AfterExit::Restart(secs(60)));
        assert!(matches!(after_exit(Some(1), secs(5), Llama::Cpu, STAGE_READY, false, 0, 4), AfterExit::Stop(_)));
        // GPU・AVX2 の版で、印が無いうち (DLL の段) か zenz の文脈を作っている間に落ちたら、版を替える
        assert_eq!(after_exit(Some(0xC000_0135u32 as i32), secs(1), Llama::Vulkan, STAGE_NONE, false, 0, 0), AfterExit::FallBack);
        assert_eq!(after_exit(Some(av), secs(5), Llama::Vulkan, STAGE_LOADING, false, 0, 0), AfterExit::FallBack);
        assert_eq!(after_exit(Some(ill), secs(5), Llama::CpuAvx2, STAGE_LOADING, false, 0, 0), AfterExit::FallBack);
        // Swift の trap (0x80000003) も落ちたとみなす
        assert_eq!(after_exit(Some(0x8000_0003u32 as i32), secs(5), Llama::Vulkan, STAGE_LOADING, false, 0, 0), AfterExit::FallBack);
        // 設定・学習の壊れ (started のあと loading の前、ready のあと) で落ちたのは版のせいにしない: 同じ版で起動し直す
        assert_eq!(after_exit(Some(av), secs(5), Llama::Vulkan, STAGE_STARTED, false, 0, 0), AfterExit::Restart(secs(1)));
        assert_eq!(after_exit(Some(av), secs(5), Llama::Vulkan, STAGE_READY, false, 0, 0), AfterExit::Restart(secs(1)));
        // AVX 無しの版はもう替える先が無い。止められた (-1) のは落ちたのではない
        assert_eq!(after_exit(Some(av), secs(5), Llama::Cpu, STAGE_LOADING, false, 0, 0), AfterExit::Restart(secs(1)));
        assert_eq!(after_exit(Some(-1), secs(5), Llama::Vulkan, STAGE_LOADING, false, 0, 0), AfterExit::Restart(secs(1)));
        // 続けて落ちたら 1 秒・5 秒・30 秒、4 回目は 10 分あけて CPU の版で。長く動いたら数え直す
        assert_eq!(after_exit(Some(-1), secs(5), Llama::Cpu, STAGE_READY, false, 0, 0), AfterExit::Restart(secs(1)));
        assert_eq!(after_exit(Some(-1), secs(5), Llama::Cpu, STAGE_READY, false, 1, 0), AfterExit::Restart(secs(5)));
        assert_eq!(after_exit(Some(-1), secs(5), Llama::Cpu, STAGE_READY, false, 2, 0), AfterExit::Restart(secs(30)));
        assert_eq!(after_exit(Some(-1), secs(5), Llama::Cpu, STAGE_READY, false, 3, 0), AfterExit::RestartOnCpuLater(secs(600)));
        assert_eq!(after_exit(Some(-1), secs(300), Llama::Cpu, STAGE_READY, false, 3, 0), AfterExit::Restart(secs(1)));
    }

    /// 2026-11-10: 版の選び方 (AVX2 の無い CPU では GPU の版も AVX2 の版も選ばない)
    #[test]
    fn llama_is_chosen_by_cpu_features() {
        let all = |_: Llama| true;
        let no_avx2_dir = |l: Llama| l != Llama::CpuAvx2;
        let no_vulkan = |l: Llama| l != Llama::Vulkan;
        assert_eq!(choose_llama("vulkan", true, all, false), Llama::Vulkan);
        assert_eq!(choose_llama("vulkan", false, all, false), Llama::Cpu);
        assert_eq!(choose_llama("cpu", true, all, false), Llama::CpuAvx2);
        assert_eq!(choose_llama("cpu", false, all, false), Llama::Cpu);
        assert_eq!(choose_llama("cpu", true, all, true), Llama::Cpu);
        assert_eq!(choose_llama("cpu", true, no_avx2_dir, false), Llama::Cpu);
        assert_eq!(choose_llama("vulkan", true, no_vulkan, false), Llama::CpuAvx2);
        assert_eq!(choose_llama("vulkan", true, no_avx2_dir, true), Llama::Vulkan);
        assert_eq!(choose_llama("something", true, all, false), Llama::CpuAvx2);
    }

    /// 2026-11-23: 壊れた学習を退けた行は記録に書き、ほかの行は書かない
    #[test]
    fn learning_quarantine_lines_are_recorded() {
        let line = "haniword-server: learning files were corrupt (metadata が途中で切れている); moved 5 files to memory_corrupt_20261123-120000\r";
        assert_eq!(learning_quarantine_note(line), Some("learning files were corrupt (metadata が途中で切れている); moved 5 files to memory_corrupt_20261123-120000"));
        assert_eq!(learning_quarantine_note("haniword-server: ready"), None);
        assert_eq!(learning_quarantine_note("learning files were corrupt"), None);
    }

    /// 2026-11-19: モデルを読めなかった行と、ai_backend.json の model_error
    #[test]
    fn model_events_update_the_status() {
        assert_eq!(model_event_of_line("haniword-server: model-failed"), Some(true));
        assert_eq!(model_event_of_line("haniword-server: model-loaded\r"), Some(false));
        assert_eq!(model_event_of_line("haniword-server: ready"), None);
        let status = shared::AiBackendStatus { llama: "llama_vulkan".into(), setting: "vulkan".into(), ..Default::default() };
        let failed = with_model_error(status, true);
        assert_eq!(failed.extra["model_error"], "load_failed");
        let loaded = with_model_error(failed, false);
        assert!(loaded.extra.get("model_error").is_none());
        assert_eq!(loaded.llama, "llama_vulkan");
    }

    /// 2026-11-19: 設定の裏方から下げたわけ (起きた順。ai_backend.json に書く)
    #[test]
    fn fallback_reasons_follow_the_choice() {
        let all = |_: Llama| true;
        assert!(fallback_reasons("vulkan", false, false, true, false, all, Llama::Vulkan).is_empty());
        assert_eq!(fallback_reasons("vulkan", true, false, true, false, all, Llama::CpuAvx2), vec!["gpu_failed_before"]);
        assert_eq!(fallback_reasons("vulkan", false, false, false, false, all, Llama::Cpu), vec!["no_avx2"]);
        assert_eq!(fallback_reasons("vulkan", false, false, true, false, |l: Llama| l != Llama::Vulkan, Llama::CpuAvx2), vec!["gpu_missing"]);
        assert_eq!(fallback_reasons("vulkan", false, true, true, false, all, Llama::CpuAvx2), vec!["quick_exits"]);
        assert_eq!(fallback_reasons("vulkan", true, false, true, true, all, Llama::Cpu), vec!["gpu_failed_before", "avx2_failed_before"]);
        assert_eq!(fallback_reasons("vulkan", true, false, false, false, all, Llama::Cpu), vec!["gpu_failed_before", "no_avx2"]);
        assert_eq!(fallback_reasons("cpu", false, false, true, false, |l: Llama| l != Llama::CpuAvx2, Llama::Cpu), vec!["avx2_missing"]);
        assert_eq!(fallback_reasons("cpu", false, false, false, false, all, Llama::Cpu), vec!["no_avx2"]);
        assert!(fallback_reasons("cpu", false, false, true, false, all, Llama::CpuAvx2).is_empty());
    }

    /// 2026-11-10: 変換サーバーの印の読み取りと、手がかりの 1 行の終了コード
    #[test]
    fn stage_marks_and_exit_codes() {
        assert_eq!(stage_of_line("haniword-server: started"), Some(STAGE_STARTED));
        assert_eq!(stage_of_line("haniword-server: loading\r"), Some(STAGE_LOADING));
        assert_eq!(stage_of_line("haniword-server: ready"), Some(STAGE_READY));
        assert_eq!(stage_of_line("haniword-server: readyx"), None);
        assert_eq!(stage_of_line("something else"), None);
        assert_eq!(stage_name(STAGE_NONE), "none");
        assert_eq!(exit_code_text(Some(0xC000_001Du32 as i32)), "-1073741795 = 0xC000001D illegal instruction");
        assert_eq!(exit_code_text(Some(1)), "1 = 0x00000001");
        assert_eq!(exit_code_text(None), "no code");
    }

    #[test]
    fn protected_launcher_uses_appdata() {
        let fixture = Fixture::new();
        let appdata = fixture.0.join("appdata");
        assert_eq!(app_dir(&fixture.0, &appdata), appdata.join(shared::APP_DIR));
    }

    #[test]
    fn staged_server_is_not_a_runtime_bundle() {
        let fixture = Fixture::new();
        fixture.add("azookey-server.exe");
        fixture.add("azookey-server.dll");
        let appdata = fixture.0.join("appdata");
        assert_eq!(app_dir(&fixture.0, &appdata), appdata.join(shared::APP_DIR));
    }

    #[test]
    fn complete_legacy_bundle_remains_supported() {
        let fixture = Fixture::new();
        for file in ["azookey-server.exe", "azookey-server.dll", "swiftCore.dll", "llama_cpu/llama.dll"] {
            fixture.add(file);
        }
        std::fs::create_dir(fixture.0.join("Dictionary")).unwrap();
        assert_eq!(app_dir(&fixture.0, &fixture.0.join("appdata")), fixture.0);
    }
}
