import Foundation
import WinSDK

/// 2026-11-08 (仕事 52): 重いときは自動で AI を休み、辞書だけの変換に落とす (設定 input.ai_auto_pause、2026-11-19 から初期値オフ)。
/// 決めは変換サーバーに置く (IME 本体はアプリごとに別のプロセスなので、ふだんの値をアプリごとに測り直すことになるため。変換サーバーはどのアプリの変換も通り、
/// AI が Vulkan か CPU かも知っている)。数はまとめ役 14:49 の決め (最後は測りで決める):
/// - ふだんの値: 落ちていない間の AI の変換 (8 字以上) の 1 字あたりの時間の中央値 (直近 50 回)。AI の一式 (vulkan・cpu) ごとに持つ
/// - 落とす: 直近 4 回のうち 3 回が「ふだんの 2.5 倍、かつふだん + 0.4 秒」より遅い。CPU が 10 秒以上 90% を超えていれば 2 回で。
///   または上限: ふだんよりも遅く (同じ 2.5 倍・+0.4 秒)、そのうえ Space (清書でない変換) が 4 秒を超えた・清書が 1.5 秒を超えたのが 2 回続いた
/// - 戻す: 落ちてから最低 20 秒は辞書だけ。そのあと 30 秒に 1 回、清書の 1 回だけを AI で試し (IME 本体の印が付いたもの。Space は試さない)、
///   ふだんの 1.5 倍以内が 2 回続いたら戻す。印を付けない古い IME 本体だけのときは、120 秒たったら次の AI の変換を試しにする
/// 2026-11-19 (C の見直し・まとめ役の決め「どんな PC でも、いつもの速さでは落ちない」): 前は上限をふだんの値と関係なく当てていた。CPU だけの人は
/// 負荷が無くても 1 文で上限を超え (AVX 無しの版は 1 文の中央値 7.5 秒・AVX2 の版も 90% 点 1.4 秒)、上限に当たった回はふだんの値に足さないので、
/// 一度落ちると戻らなかった。今は、どの決まりも「ふだんよりも遅い」回だけを数える。ふだんの値が溜まるまで (8 字以上の AI の変換 10 回) は落とさない (52 の前と同じ)
/// 2026-11-19 (新しいエージェントの見直し・まとめ役): 負荷のある中で起動すると、ふだんの値が重いときの値になり、何秒かかっても休まなかった。今は:
/// - ふだんの値を、一式と電源 (AC か電池か) の組ごとに起動をまたいで残す (memory/ai_speed.json)。前の起動の値があれば、起動してすぐの負荷でも休む。
///   電池で遅くなっただけなら、電池の組のふだんの値と比べるので休まない。初めての起動から重いときだけは、52 の前と同じく休まない
/// - ふだんの値に足すのは、ふだんの 1.5 倍以内の回だけ (少しずつ増える負荷を取り込みにくくする。2 倍の負荷は取り込まない)
/// - 落ちたまま 60 分たち、試しが 10 回続けて悪ければ、今の速さでふだんの値を作り直して戻す (PC がずっと遅くなったときに、ずっと休まないように)
/// - 戻すときの試しの良し悪しは、字数によらず 1 本の線 (ふだんの 1.5 倍 × 字数 (8 字未満は 8 字) + 0.4 秒)。前は 8 字の前後で線が逆になっていた
/// - 時刻は起動からの時間 (単調な時計) で数える (壁時計が戻ると試しが始まらなかった)
struct AILoadGuard {
    static let slowFactor = 2.5
    static let slowExtraMs = 400.0
    static let window = 4
    static let slowNeeded = 3
    static let slowNeededWhenCPUBusy = 2
    static let spaceCapMs = 4000.0
    static let refineCapMs = 1500.0
    static let refineCapsNeeded = 2
    static let minPausedSeconds = 20.0
    static let probeEverySeconds = 30.0
    static let unmarkedProbeAfterSeconds = 120.0
    static let probeFactor = 1.5
    static let probesNeeded = 2
    static let baselineCount = 50
    /// ふだんの値として使うのに要る数 (これより少ないうちは落とさない)
    static let baselineMinimum = 10
    static let minChars = 8
    /// ふだんの値に足すのは、ふだんの値のこの倍以内の回だけ
    static let acceptFactor = 1.5
    /// 落ちたままこの秒数がたち、試しが rebaselineProbes 回続けて悪ければ、ふだんの値を作り直す
    static let rebaselineAfterSeconds = 3600.0
    static let rebaselineProbes = 10
    /// 残すふだんの値の組の数の上限 (読み込むときに確かめる)
    static let maxSavedKeys = 16

    private(set) var baselines: [String: [Double]] = [:]
    private(set) var recent: [Bool] = []
    private(set) var refineCaps = 0
    private(set) var pausedAt: Double? = nil
    private(set) var lastProbeAt: Double = 0
    private(set) var goodProbes = 0
    /// 落ちている間の悪い試しの 1 字あたりの時間 (8 字以上。ふだんの値を作り直すとき用)
    private(set) var failedProbes: [Double] = []
    /// ふだんの値に足した回数 (残すときの目安)
    private(set) var acceptedCount = 0

    var paused: Bool { pausedAt != nil }

    /// 残したふだんの値を戻す (まだ値の無い組だけ。正の有限の値だけを、直近 50 個まで)
    mutating func restoreBaselines(_ saved: [String: [Double]]) {
        for (key, values) in saved.prefix(Self.maxSavedKeys) where baselines[key] == nil && !key.isEmpty && key.count <= 64 {
            let valid = values.filter { $0.isFinite && $0 > 0 }.suffix(Self.baselineCount)
            if !valid.isEmpty { baselines[key] = Array(valid) }
        }
    }

    /// 1 字あたりのふだんの値 (ms)。数が足りなければ nil
    func baseline(_ backend: String) -> Double? {
        guard let values = baselines[backend], values.count >= Self.baselineMinimum else { return nil }
        let sorted = values.sorted()
        return sorted[sorted.count / 2]
    }

    /// 落ちている間に、この問い合わせを AI の試しにするか (refineMarked: IME 本体が「清書なので試してよい」の印を付けた)
    func probeAllowed(now: Double, refineMarked: Bool) -> Bool {
        guard let at = pausedAt, now - at >= Self.minPausedSeconds else { return false }
        let since = now - max(at, lastProbeAt)
        return refineMarked ? since >= Self.probeEverySeconds : since >= Self.unmarkedProbeAfterSeconds
    }

    mutating func noteProbeStarted(now: Double) {
        lastProbeAt = now
    }

    /// AI の変換が 1 回終わった (落ちていない間)。ms: かかった時間、chars: 読みの字数、refine: 清書、cpuBusy: CPU が 10 秒以上 90% を超えている
    mutating func noteAI(ms: Double, chars: Int, backend: String, refine: Bool, cpuBusy: Bool, now: Double) {
        guard pausedAt == nil else { return }
        /// ふだんの値があるときの「ふだんより遅い」(2.5 倍、かつ +0.4 秒)。ふだんの値がまだ無ければ nil
        let slowerThanUsual = baseline(backend).map { base in
            ms > Self.slowFactor * base * Double(chars) && ms > base * Double(chars) + Self.slowExtraMs
        }
        // 上限は、ふだんよりも遅い回だけを数える (ふだんの値がまだ無ければ数えない。もともと遅い PC で、長い文のたびに落ちないように)
        let overCap = (refine ? ms > Self.refineCapMs : ms > Self.spaceCapMs) && (slowerThanUsual ?? false)
        var capped = false
        if refine {
            refineCaps = overCap ? refineCaps + 1 : 0
            capped = refineCaps >= Self.refineCapsNeeded
        } else {
            capped = overCap
        }
        if chars >= Self.minChars {
            let perChar = ms / Double(chars)
            let slow = slowerThanUsual ?? false
            recent.append(slow)
            if recent.count > Self.window { recent.removeFirst(recent.count - Self.window) }
            let accepted = baseline(backend).map { perChar <= Self.acceptFactor * $0 } ?? true
            if !slow && !capped && accepted {
                baselines[backend, default: []].append(perChar)
                if baselines[backend]!.count > Self.baselineCount { baselines[backend]!.removeFirst() }
                acceptedCount += 1
            }
        }
        let slowCount = recent.filter { $0 }.count
        if capped || slowCount >= Self.slowNeeded || (cpuBusy && slowCount >= Self.slowNeededWhenCPUBusy) {
            pausedAt = now
            lastProbeAt = 0
            goodProbes = 0
            failedProbes = []
        }
    }

    /// 試しを良とする時間の上限 (ms)。ふだんの値があれば ふだんの 1.5 倍 × 字数 (8 字未満は 8 字) + 0.4 秒、無ければ清書の上限 1.5 秒
    func probeLimit(chars: Int, backend: String) -> Double {
        guard let base = baseline(backend) else { return Self.refineCapMs }
        return Self.probeFactor * base * Double(max(chars, Self.minChars)) + Self.slowExtraMs
    }

    /// 落ちている間の AI の試しが終わった。ふだんの 1.5 倍以内が 2 回続いたら戻す (ふだんの値が無ければ、清書の上限 1.5 秒以内を良いとみる)。
    /// 2026-11-19: 短い読み (8 字未満) は、8 字の変換のふだんの 1.5 倍と 1.5 秒の大きいほう以内を良いとみる (前は 1.5 秒だけで、遅い CPU では短い試しのたびに数え直しになった)
    mutating func noteProbe(ms: Double, chars: Int, backend: String, now: Double = 0) {
        guard let at = pausedAt else { return }
        let good = ms <= probeLimit(chars: chars, backend: backend)
        goodProbes = good ? goodProbes + 1 : 0
        if !good && chars >= Self.minChars {
            failedProbes.append(ms / Double(chars))
            if failedProbes.count > Self.baselineCount { failedProbes.removeFirst() }
        }
        if good { failedProbes = [] }
        // 落ちたまま長くたち、悪い試しが続けば、今の速さをふだんの値にして戻す
        let rebaseline = !good && now - at >= Self.rebaselineAfterSeconds && failedProbes.count >= Self.rebaselineProbes
        if rebaseline {
            baselines[backend] = failedProbes
        }
        if goodProbes >= Self.probesNeeded || rebaseline {
            pausedAt = nil
            recent = []
            refineCaps = 0
            goodProbes = 0
            failedProbes = []
        }
    }
}

/// CPU の使用率 (Windows 全体)。GetSystemTimes の前の値との差で、10 秒以上の間の使用率を出す (変換の問い合わせのときに引く。別のスレッドは立てない)
struct CPUBusyMeter {
    private var samples: [(at: Double, idle: UInt64, total: UInt64, own: UInt64)] = []

    /// 今の値を足し、10 秒以上前の値からの使用率が 90% を超えていれば true。
    /// 2026-11-19: own は変換サーバー自身の CPU の時間 (GetProcessTimes)。使用率からは自分の分を引く (CPU の版の AI の変換は CPU を使い切るので、
    /// 前は続けて変換すると自分の分で「忙しい」に入り、2 回遅いだけで休んだ。11-19 の朝の測り)
    mutating func busy(now: Double, idle: UInt64, total: UInt64, own: UInt64 = 0) -> Bool {
        samples.append((now, idle, total, own))
        samples.removeAll { now - $0.at > 60 }
        guard let old = samples.last(where: { now - $0.at >= 10 }), total > old.total else { return false }
        let idleDelta = Double(idle &- old.idle)
        let totalDelta = Double(total - old.total)
        let ownDelta = Double(own &- old.own)
        return 1 - (idleDelta + ownDelta) / totalDelta > 0.9
    }
}

// MARK: - 変換サーバーの中での使い方 (2026-11-08 仕事 52)

@MainActor var aiLoadGuard = AILoadGuard()
@MainActor var cpuBusyMeter = CPUBusyMeter()
/// 今の問い合わせを AI を休んでするか (getOptions と aiConversionEnabled が見る。問い合わせの間だけ立てる)
@MainActor var aiPausedForRequest = false
/// IME 本体が次の変換に付けた「清書」の印 (SetConvertRefine。変換 1 回で消す)
@MainActor var nextConvertRefine = false
/// 直前の変換を AI を休んで辞書だけでしたか (LastConversionAIPaused で IME 本体へ返す)
@MainActor var lastConversionAIPaused = false
/// AI の一式の名前を記録に出したか (最初の AI の変換で 1 回)
@MainActor var aiBackendLogged = false

@_silgen_name("SetConvertRefine")
@MainActor public func set_convert_refine(refine: Bool) {
    nextConvertRefine = refine
}

@_silgen_name("LastConversionAIPaused")
@MainActor public func last_conversion_ai_paused() -> Bool {
    lastConversionAIPaused
}

/// 設定 input.ai_auto_pause (2026-11-19 から初期値オフ: 負荷の無いときの Vulkan で誤って休むことがあったため)。開発用の切り替え AZK_AI_AUTO_PAUSE=1/0
@MainActor func aiAutoPauseEnabled() -> Bool {
    if let knob = devEnvironment["AZK_AI_AUTO_PAUSE"] { return knob == "1" }
    return (config["aiAutoPause"] as? Bool) ?? false
}

/// 休んでいる間の並べ方が「AI ありと同じ」(仮表示と同じ fast の形) か。初期値は辞書だけ向け
@MainActor func aiPauseOrderPreview() -> Bool {
    (config["aiPauseOrder"] as? String) == "preview"
}

/// AI の一式: 起動の道具 (launcher) が PATH の頭に置いた llama のフォルダの名前 (llama_vulkan・llama_cpu・llama_cuda)
let aiBackendName: String = {
    let first = (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ";").first.map(String.init) ?? ""
    let name = first.split(whereSeparator: { $0 == "\\" || $0 == "/" }).last.map(String.init) ?? ""
    return name.lowercased().hasPrefix("llama_") ? name.lowercased() : "unknown"
}()

/// 2026-11-19: 起動からの時間 (秒、単調な時計)。前は壁時計で、時計が戻ると試しが始まらなかった
func nowSeconds() -> Double {
    ProcessInfo.processInfo.systemUptime
}

/// 2026-11-19: 電源 ("ac" か "battery")。ふだんの値は一式と電源の組ごとに持つ (電池で遅くなっただけで休まないように)。分からなければ ac
func powerSourceName() -> String {
    var status = SYSTEM_POWER_STATUS()
    guard GetSystemPowerStatus(&status) else { return "ac" }
    return status.ACLineStatus == 0 ? "battery" : "ac"
}

/// ふだんの値の組の名前 (一式|電源)
func aiSpeedKey(backend: String, power: String) -> String { "\(backend)|\(power)" }

/// 2026-11-19: ふだんの値を起動をまたいで残すファイル (学習の場所 memory/ai_speed.json)。Initialize の前 (試験) と、利用者のデータを使わない問い合わせでは使わない
@MainActor func aiSpeedURL() -> URL? {
    guard execURLReady, personalDataAllowed, devEnvironment["AZK_AI_SPEED_STORE"] != "0" else { return nil }
    return execURL.appendingPathComponent("memory").appendingPathComponent("ai_speed.json")
}

@MainActor var aiSpeedLoaded = false
@MainActor var aiSpeedSavedCount = 0

/// 残したふだんの値を 1 回だけ読む (1MB を超える・読めないなら読まない)
@MainActor func loadAISpeedBaselinesOnce() {
    guard !aiSpeedLoaded, let url = aiSpeedURL() else { return }
    aiSpeedLoaded = true
    guard let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize, size <= 1 << 20,
          let data = try? Data(contentsOf: url), let saved = try? JSONDecoder().decode([String: [Double]].self, from: data) else { return }
    aiLoadGuard.restoreBaselines(saved)
    aiSpeedSavedCount = aiLoadGuard.acceptedCount
}

/// ふだんの値に 10 回足すたびに書く (一時ファイルに書いてから置き換える)
@MainActor func saveAISpeedBaselinesIfNeeded() {
    guard aiLoadGuard.acceptedCount - aiSpeedSavedCount >= 10, let url = aiSpeedURL(),
          let data = try? JSONEncoder().encode(aiLoadGuard.baselines) else { return }
    aiSpeedSavedCount = aiLoadGuard.acceptedCount
    try? data.write(to: url, options: .atomic)
}

/// Windows 全体の CPU の時間 (idle, kernel+user) と、このプロセスの CPU の時間 (kernel+user)。取れなければ nil
func systemCPUTimes() -> (idle: UInt64, total: UInt64, own: UInt64)? {
    var idle = FILETIME(), kernel = FILETIME(), user = FILETIME()
    guard GetSystemTimes(&idle, &kernel, &user) else { return nil }
    func value(_ t: FILETIME) -> UInt64 { (UInt64(t.dwHighDateTime) << 32) | UInt64(t.dwLowDateTime) }
    var creation = FILETIME(), exit = FILETIME(), pKernel = FILETIME(), pUser = FILETIME()
    let own = GetProcessTimes(GetCurrentProcess(), &creation, &exit, &pKernel, &pUser) ? value(pKernel) + value(pUser) : 0
    return (value(idle), value(kernel) + value(user), own)
}

/// 変換の問い合わせの頭で決める: AI を休んでするか (paused)・この問い合わせを AI の試しにするか (probe)。
/// 辞書だけの変換・AI を切った設定・自動で休む設定がオフなら、どちらでもない
@MainActor func beginAIConversion(dictionaryOnly: Bool) -> (paused: Bool, probe: Bool, refine: Bool) {
    let refine = nextConvertRefine
    nextConvertRefine = false
    guard !dictionaryOnly, aiSettingEnabled(), aiAutoPauseEnabled() else { return (false, false, refine) }
    if devEnvironment["AZK_AI_PAUSE_FORCE"] == "1" { return (true, false, refine) }
    guard aiLoadGuard.paused else { return (false, false, refine) }
    let now = nowSeconds()
    if aiLoadGuard.probeAllowed(now: now, refineMarked: refine) {
        aiLoadGuard.noteProbeStarted(now: now)
        return (false, true, refine)
    }
    return (true, false, refine)
}

/// AI で変換したあとで、かかった時間を数える (休んでいない間と試し)
@MainActor func endAIConversion(ms: Double, chars: Int, refine: Bool, probe: Bool) {
    guard aiAutoPauseEnabled() else { return }
    loadAISpeedBaselinesOnce()
    let key = aiSpeedKey(backend: aiBackendName, power: powerSourceName())
    if probe {
        let was = aiLoadGuard.paused
        aiLoadGuard.noteProbe(ms: ms, chars: chars, backend: key, now: nowSeconds())
        if was && !aiLoadGuard.paused { saveAISpeedBaselinesIfNeeded() }
        return
    }
    if !aiBackendLogged {
        // 2026-11-19 (C): ふだんの値は一式ごとに持つので、一式の名前 (分からなければ unknown) を試験用のビルドの記録に 1 回出す (夜の測りで見分ける)
        aiBackendLogged = true
        debugWrite("ai backend: \(aiBackendName)\n".data(using: .utf8)!)
    }
    let busy = systemCPUTimes().map { cpuBusyMeter.busy(now: nowSeconds(), idle: $0.idle, total: $0.total, own: $0.own) } ?? false
    let was = aiLoadGuard.paused
    aiLoadGuard.noteAI(ms: ms, chars: chars, backend: key, refine: refine, cpuBusy: busy, now: nowSeconds())
    if !was && aiLoadGuard.paused {
        debugWrite("ai paused: \(Int(ms))ms for \(chars) chars (\(key), cpu busy \(busy))\n".data(using: .utf8)!)
    }
    saveAISpeedBaselinesIfNeeded()
}

/// 今、重いので AI を休んでいるか (候補の一覧・採点など、変換でない問い合わせ用。試しにはしない)
@MainActor func aiPausedNow() -> Bool {
    guard aiSettingEnabled(), aiAutoPauseEnabled() else { return false }
    return aiLoadGuard.paused || devEnvironment["AZK_AI_PAUSE_FORCE"] == "1"
}
