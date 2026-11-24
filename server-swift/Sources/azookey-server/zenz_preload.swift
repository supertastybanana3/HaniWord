import Foundation
import KanaKanjiConverterModule

/// 2026-11-18 (Fable の諮問 perf の 7「受け口を先に開き、zenz は裏で読む」、まとめ役 19:2x): 冷えた起動は約 9 秒で、そのうち約 7 秒が
/// zenz の最初の推論 (GPU の準備) だった。Initialize がそれを待ってから返していたので、受け口 (名前付きパイプ) が開くまで変換できなかった。
/// 今は Initialize の温めの変換を辞書だけにしてすぐ返し、zenz のモデルの読み込みと最初の推論は別のスレッドで済ませる (ZenzPreload)。
/// 読み終わるまでの変換は、AI を使う問い合わせも辞書だけで返す (仕事 52 の「休み」と同じ道。ただし休みの印 (LastConversionAIPaused) は立てない)。
/// 読み終えたら、次の問い合わせの頭 (getOptions) で変換器に渡し、そこから AI を使う。
/// 起動の段の印 loading・ready はそのスレッドが出す (launcher は loading の間に落ちたときだけ GPU (AVX2) の版のせいとみなす)。
/// 開発用のビルドだけ AZK_ZENZ_PRELOAD=0 で前の形 (Initialize の中で AI ありの温めの変換をしてから返す) に戻す (測って比べるため)
let zenzPreloadEnabled = devEnvironment["AZK_ZENZ_PRELOAD"] != "0"

/// 裏のスレッドと主のスレッドの受け渡し (錠で守る)
final class ZenzPreloadSlot: @unchecked Sendable {
    private let lock = NSLock()
    private var finished = false
    private var preload: ZenzPreload? = nil

    func finish(_ value: ZenzPreload?) {
        lock.lock(); defer { lock.unlock() }
        preload = value
        finished = true
    }

    /// 試験用: まだ済んでいない形に戻す
    func reset() {
        lock.lock(); defer { lock.unlock() }
        preload = nil
        finished = false
    }

    /// 済んでいれば (true, 読めた文脈 (読めなければ nil))。渡した文脈は手放す
    func take() -> (finished: Bool, preload: ZenzPreload?) {
        lock.lock(); defer { lock.unlock() }
        let result = (finished, preload)
        preload = nil
        return result
    }
}

let zenzPreloadSlot = ZenzPreloadSlot()
@MainActor var zenzPreloadStarted = false
@MainActor var zenzPreloadInstalled = false
@MainActor var zenzPreloadFailed = false
/// 2026-11-19: 読めなかったあと、設定の読み直しで 1 回だけ始め直したか
@MainActor var zenzPreloadRetried = false
/// Initialize が済んだあと (getOptions が裏読みを始めてよい)。Initialize の中では startZenzPreloadAtStartup が印つきで始める
@MainActor var zenzPreloadAutoStart = false

/// zenz を裏で読んでいる最中か (読み終えて変換器に渡すまで)。この間は AI を使わない
@MainActor func zenzWarming() -> Bool {
    zenzPreloadStarted && !zenzPreloadInstalled && !zenzPreloadFailed
}

/// 設定のモデルの場所 (getOptions の weight と同じ)
@MainActor func configuredZenzURL() -> URL {
    execURL.appendingPathComponent((config["model"] as? String) ?? "zenz.gguf")
}

/// zenz の裏読みを始める (1 度だけ)。marks: 起動の段の印を出すか (Initialize からのときだけ)
@MainActor func startZenzPreload(marks: Bool) {
    guard !zenzPreloadStarted else { return }
    zenzPreloadStarted = true
    let url = configuredZenzURL()
    let slot = zenzPreloadSlot
    let thread = Thread {
        if marks { startupMark("loading") }
        let started = Date()
        let preload = ZenzPreload.load(resourceURL: url)
        slot.finish(preload)
        debugWrite("zenz preload: \(preload == nil ? "failed" : "loaded") in \(Int(-started.timeIntervalSinceNow * 1000))ms\n".data(using: .utf8)!)
        if marks { startupMark("ready") }
    }
    thread.stackSize = 8 << 20
    thread.start()
}

/// 主のスレッドで、裏読みが済んでいれば変換器に渡す (getOptions の頭で呼ぶ)。読めなかったら、以後は AI を使わない
/// (getOptions は zenzai を .off で返し、変換は読んでいる間と同じく辞書だけ。モデルを読み直さない。2026-11-19 軽微 4)
@MainActor func installPreloadedZenzIfReady() {
    guard zenzWarming() else { return }
    let (finished, preload) = zenzPreloadSlot.take()
    guard finished else { return }
    if let preload {
        converter.installPreloadedZenz(preload)
        zenzPreloadInstalled = true
        if zenzPreloadRetried { modelMark("model-loaded") }
    } else {
        zenzPreloadFailed = true
        modelMark("model-failed")
    }
}

/// 2026-11-19 (まとめ役): 測りの道具が、zenz を読み終えたかを聞く口 (配る版にもある。IME 本体は使わない)。
/// 0 = 読んでいる (この間の変換は AI を頼んでも辞書だけ)、1 = 読み終えた (または読まない: AI を切った設定・前の形)、2 = 読めなかった。
/// 読み終えていれば、ここで変換器に渡す (次の変換の頭で渡すのと同じ)
@_silgen_name("ZenzPreloadState")
@MainActor public func zenz_preload_state() -> Int32 {
    installPreloadedZenzIfReady()
    if zenzPreloadFailed { return 2 }
    return zenzWarming() ? 0 : 1
}

/// 2026-11-19 (新しいエージェントの見直し): モデルを読めなかった・読み直して読めたを、標準エラーに 1 行出す (配る版でも。打った文は入れない)。
/// launcher がこの行を見て記録に書き、ai_backend.json に model_error を書く (設定画面の「HaniWord について」が「AI のモデルを読み込めませんでした」と出す)
func modelMark(_ event: String) {
    FileHandle.standardError.write(Data("haniword-server: \(event)\n".utf8))
}

/// 2026-11-19 (新しいエージェントの見直し): 読めなかった印は、前は変換サーバーが動いている間ずっと残り、設定で AI をオフ→オンしても始め直さなかった。
/// 設定を読み直したとき (LoadConfig)、AI がオンで読めなかったあとなら、印を戻して裏読みを 1 回だけ始め直す。始めたら真
@MainActor func retryFailedZenzPreloadIfNeeded() -> Bool {
    guard zenzPreloadEnabled, zenzPreloadFailed, !zenzPreloadRetried, (config["enable"] as? Bool) == true else { return false }
    zenzPreloadRetried = true
    zenzPreloadFailed = false
    zenzPreloadStarted = false
    zenzPreloadSlot.reset()
    startZenzPreload(marks: false)
    return true
}

/// Initialize の最後: AI を使う設定なら裏読みを始める。使わない設定・前の形 (AZK_ZENZ_PRELOAD=0) なら、印だけ前と同じ順に出す
@MainActor func startZenzPreloadAtStartup() {
    zenzPreloadAutoStart = zenzPreloadEnabled
    if zenzPreloadEnabled && (config["enable"] as? Bool) == true {
        startZenzPreload(marks: true)
    } else {
        startupMark("loading")
        startupMark("ready")
    }
}
