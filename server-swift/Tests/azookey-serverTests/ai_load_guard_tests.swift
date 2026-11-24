import XCTest
@testable import azookey_server

/// 2026-11-08 (仕事 52): 重いときに AI を休む決め (ai_load_guard.swift)
final class AILoadGuardTests: XCTestCase {
    /// ふだん 1 字 20ms (10 字で 200ms) の AI を 10 回
    func warmed(_ backend: String = "vulkan") -> AILoadGuard {
        var guard_ = AILoadGuard()
        for k in 0..<10 { guard_.noteAI(ms: 200, chars: 10, backend: backend, refine: false, cpuBusy: false, now: Double(k)) }
        return guard_
    }

    func testPausesAfterThreeSlowOfFour() {
        var g = warmed()
        XCTAssertEqual(g.baseline("vulkan"), 20)
        // 2.5 倍 (500ms) かつ +0.4 秒 (600ms) を超える 700ms が 2 回では落ちない
        g.noteAI(ms: 700, chars: 10, backend: "vulkan", refine: false, cpuBusy: false, now: 20)
        g.noteAI(ms: 700, chars: 10, backend: "vulkan", refine: false, cpuBusy: false, now: 21)
        XCTAssertFalse(g.paused)
        // 2.5 倍は超えても +0.4 秒を超えない 550ms は遅いに数えない
        g.noteAI(ms: 550, chars: 10, backend: "vulkan", refine: false, cpuBusy: false, now: 22)
        XCTAssertFalse(g.paused)
        g.noteAI(ms: 700, chars: 10, backend: "vulkan", refine: false, cpuBusy: false, now: 23)
        XCTAssertTrue(g.paused)
    }

    func testCPUBusyPausesAfterTwo() {
        var g = warmed()
        g.noteAI(ms: 700, chars: 10, backend: "vulkan", refine: false, cpuBusy: true, now: 20)
        XCTAssertFalse(g.paused)
        g.noteAI(ms: 700, chars: 10, backend: "vulkan", refine: false, cpuBusy: true, now: 21)
        XCTAssertTrue(g.paused)
    }

    /// 2026-11-19: 上限 (Space 4 秒・清書 1.5 秒が 2 回) は、ふだんよりも遅い回だけを数える。ふだんの値がまだ無ければ数えない
    func testCapsCountOnlyWhenSlowerThanUsual() {
        // ふだんの値が無いうちは、4 秒を超える Space も、1.5 秒を超える清書が続いても落ちない (前は 1 回で落ちていた)
        var fresh = AILoadGuard()
        fresh.noteAI(ms: 4100, chars: 3, backend: "llama_vulkan", refine: false, cpuBusy: false, now: 1)
        for k in 2..<5 { fresh.noteAI(ms: 1600, chars: 5, backend: "llama_vulkan", refine: true, cpuBusy: false, now: Double(k)) }
        XCTAssertFalse(fresh.paused)
        // ふだん 1 字 20ms なら、Space の 4 秒超え (短い読みでも) は 1 回で落ちる
        var g = warmed()
        g.noteAI(ms: 4100, chars: 3, backend: "vulkan", refine: false, cpuBusy: false, now: 20)
        XCTAssertTrue(g.paused)
        // 清書の 1.5 秒超えは 2 回続いたら (短い読みなので、ふだんの 2.5 倍の決まりの数には入らない)
        var r = warmed()
        r.noteAI(ms: 1600, chars: 5, backend: "vulkan", refine: true, cpuBusy: false, now: 20)
        r.noteAI(ms: 900, chars: 5, backend: "vulkan", refine: true, cpuBusy: false, now: 21)
        r.noteAI(ms: 1600, chars: 5, backend: "vulkan", refine: true, cpuBusy: false, now: 22)
        XCTAssertFalse(r.paused)
        r.noteAI(ms: 1600, chars: 5, backend: "vulkan", refine: true, cpuBusy: false, now: 23)
        XCTAssertTrue(r.paused)
        // もともと遅い GPU (ふだん 1 字 60ms) では、40 字の清書が 2 秒かかっても、ふだんどおりなので上限に数えない
        var slowGPU = AILoadGuard()
        for k in 0..<10 { slowGPU.noteAI(ms: 600, chars: 10, backend: "llama_vulkan", refine: false, cpuBusy: false, now: Double(k)) }
        for k in 10..<20 { slowGPU.noteAI(ms: 2000, chars: 40, backend: "llama_vulkan", refine: true, cpuBusy: false, now: Double(k)) }
        XCTAssertFalse(slowGPU.paused)
    }

    /// 2026-11-19 (C の見直し): CPU だけの人 (AVX 無しの版で 1 字 150ms) は、30 字の Space が 4.5 秒かかっても、ふだんどおりなら
    /// 最初の 1 回から落ちない。負荷で 2.5 倍を超えて遅くなったら落ちる
    func testUsualSpeedNeverPausesOnASlowCPU() {
        var g = AILoadGuard()
        for k in 0..<20 {
            g.noteAI(ms: 4500, chars: 30, backend: "llama_cpu", refine: k % 2 == 1, cpuBusy: false, now: Double(k))
            XCTAssertFalse(g.paused, "\(k)")
        }
        XCTAssertEqual(g.baseline("llama_cpu"), 150)
        // 負荷: 30 字の Space が 12 秒 (2.5 倍の 11.25 秒を超え、4 秒の上限も超える) なら 1 回で落ちる
        g.noteAI(ms: 12_000, chars: 30, backend: "llama_cpu", refine: false, cpuBusy: false, now: 30)
        XCTAssertTrue(g.paused)
        // AVX2 の版 (1 字 40ms) では、40 字の清書が 1.6 秒かかるのが続いても落ちない
        var avx2 = AILoadGuard()
        for k in 0..<20 { avx2.noteAI(ms: 1600, chars: 40, backend: "llama_cpu_avx2", refine: true, cpuBusy: false, now: Double(k)) }
        XCTAssertFalse(avx2.paused)
    }

    func testCPUOnlyUsersAreComparedWithTheirOwnBaseline() {
        // CPU の人はふだん 1 字 150ms。同じ遅さが続いても落ちない (GPU の人の値とは別に持つ)
        var g = AILoadGuard()
        for k in 0..<10 { g.noteAI(ms: 1500, chars: 10, backend: "cpu", refine: false, cpuBusy: false, now: Double(k)) }
        XCTAssertEqual(g.baseline("cpu"), 150)
        XCTAssertNil(g.baseline("vulkan"))
        for k in 10..<20 { g.noteAI(ms: 1600, chars: 10, backend: "cpu", refine: false, cpuBusy: false, now: Double(k)) }
        XCTAssertFalse(g.paused)
    }

    /// 2026-11-19 (b): 前の起動のふだんの値があれば、負荷のある中で起動しても休む
    func testStartingUnderLoadPausesWithTheSavedBaseline() {
        var g = AILoadGuard()
        g.restoreBaselines(["llama_vulkan|ac": Array(repeating: 20, count: 50)])
        XCTAssertEqual(g.baseline("llama_vulkan|ac"), 20)
        for k in 0..<3 { g.noteAI(ms: 700, chars: 10, backend: "llama_vulkan|ac", refine: false, cpuBusy: false, now: Double(k)) }
        XCTAssertTrue(g.paused)
    }

    /// 2026-11-19 (b): 少しずつ増える負荷 (2 倍) はふだんの値に取り込まず、そのあと 3 倍になれば休む。1.4 倍は取り込む
    func testGradualLoadIsNotAbsorbed() {
        var g = warmed()
        for k in 0..<60 { g.noteAI(ms: 400, chars: 10, backend: "vulkan", refine: false, cpuBusy: false, now: Double(20 + k)) }
        XCTAssertFalse(g.paused)
        XCTAssertEqual(g.baseline("vulkan"), 20)
        for k in 0..<3 { g.noteAI(ms: 700, chars: 10, backend: "vulkan", refine: false, cpuBusy: false, now: Double(100 + k)) }
        XCTAssertTrue(g.paused)
        var h = warmed()
        for k in 0..<50 { h.noteAI(ms: 280, chars: 10, backend: "vulkan", refine: false, cpuBusy: false, now: Double(20 + k)) }
        XCTAssertEqual(h.baseline("vulkan"), 28)
        XCTAssertFalse(h.paused)
    }

    /// 2026-11-19 (c): 電池の組は AC の組と別のふだんの値。電池で遅くなっただけでは休まない
    func testBatteryKeepsItsOwnBaseline() {
        var g = AILoadGuard()
        g.restoreBaselines(["llama_vulkan|ac": Array(repeating: 20, count: 50)])
        for k in 0..<20 { g.noteAI(ms: 600, chars: 10, backend: "llama_vulkan|battery", refine: false, cpuBusy: false, now: Double(k)) }
        XCTAssertFalse(g.paused)
        XCTAssertEqual(g.baseline("llama_vulkan|battery"), 60)
        XCTAssertEqual(g.baseline("llama_vulkan|ac"), 20)
        XCTAssertEqual(aiSpeedKey(backend: "llama_vulkan", power: "battery"), "llama_vulkan|battery")
    }

    /// 2026-11-19 (a): 遅い PC (AVX2 の CPU で 1 字 40ms) は、前の起動の値があっても、負荷が無ければ休まない
    func testSlowPCWithSavedBaselineDoesNotPause() {
        var g = AILoadGuard()
        g.restoreBaselines(["llama_cpu_avx2|ac": Array(repeating: 40, count: 50)])
        for k in 0..<50 { g.noteAI(ms: Double([1200, 1600, 2000][k % 3]), chars: 40, backend: "llama_cpu_avx2|ac", refine: k % 2 == 0, cpuBusy: false, now: Double(k)) }
        XCTAssertFalse(g.paused)
    }

    /// 2026-11-19: 落ちたまま 60 分たち、悪い試しが 10 回続けば、今の速さをふだんの値にして戻す
    func testLongPauseRebaselines() {
        var g = warmed()
        g.noteAI(ms: 4100, chars: 10, backend: "vulkan", refine: false, cpuBusy: false, now: 100)
        XCTAssertTrue(g.paused)
        for k in 0..<12 { g.noteProbe(ms: 3000, chars: 10, backend: "vulkan", now: 200 + Double(k) * 30) }
        XCTAssertTrue(g.paused)
        g.noteProbe(ms: 3000, chars: 10, backend: "vulkan", now: 3800)
        XCTAssertFalse(g.paused)
        XCTAssertEqual(g.baseline("vulkan"), 300)
    }

    /// 2026-11-19: 残したふだんの値は、正の有限の値だけを直近 50 個まで、まだ値の無い組にだけ戻す
    func testRestoreBaselinesValidates() throws {
        var g = warmed()
        let saved = try JSONDecoder().decode([String: [Double]].self, from: Data(#"{"vulkan": [99, 99, 99, 99, 99, 99, 99, 99, 99, 99], "llama_cpu|ac": [-1, 0, 150, 150, 150, 150, 150, 150, 150, 150, 150, 150]}"#.utf8))
        g.restoreBaselines(saved)
        XCTAssertEqual(g.baseline("vulkan"), 20)
        XCTAssertEqual(g.baseline("llama_cpu|ac"), 150)
        var many = AILoadGuard()
        many.restoreBaselines(["k": (0..<80).map { Double($0 + 1) }])
        XCTAssertEqual(many.baselines["k"]?.count, 50)
        XCTAssertEqual(many.baselines["k"]?.first, 31)
    }

    func testShortReadingsDoNotCountAsSlow() {
        var g = warmed()
        for k in 0..<4 { g.noteAI(ms: 900, chars: 3, backend: "vulkan", refine: false, cpuBusy: false, now: Double(20 + k)) }
        XCTAssertFalse(g.paused)
    }

    func testProbesAfterTheMinimumAndResumesAfterTwoGood() {
        var g = warmed()
        g.noteAI(ms: 4100, chars: 10, backend: "vulkan", refine: false, cpuBusy: false, now: 100)
        XCTAssertTrue(g.paused)
        // 最低 20 秒は試さない。清書の印があれば 30 秒から、印が無ければ 120 秒から
        XCTAssertFalse(g.probeAllowed(now: 115, refineMarked: true))
        XCTAssertFalse(g.probeAllowed(now: 125, refineMarked: true))
        XCTAssertTrue(g.probeAllowed(now: 130, refineMarked: true))
        XCTAssertFalse(g.probeAllowed(now: 130, refineMarked: false))
        XCTAssertTrue(g.probeAllowed(now: 220, refineMarked: false))
        g.noteProbeStarted(now: 130)
        XCTAssertFalse(g.probeAllowed(now: 150, refineMarked: true))
        // ふだん 200ms (10 字) の 1.5 倍 = 300ms 以内を良いとみる。良・悪・良・良 で戻る
        g.noteProbe(ms: 250, chars: 10, backend: "vulkan")
        g.noteProbe(ms: 2000, chars: 10, backend: "vulkan")
        XCTAssertTrue(g.paused)
        g.noteProbe(ms: 250, chars: 10, backend: "vulkan")
        g.noteProbe(ms: 250, chars: 10, backend: "vulkan")
        XCTAssertFalse(g.paused)
    }

    /// 2026-11-19: 短い読みの試しは、8 字の変換のふだんの 1.5 倍と 1.5 秒の大きいほう以内を良いとみる (遅い CPU で、短い試しのたびに数え直さない)
    func testShortProbesUseTheBaselineOfEightChars() {
        var g = AILoadGuard()
        for k in 0..<10 { g.noteAI(ms: 1500, chars: 10, backend: "llama_cpu", refine: false, cpuBusy: false, now: Double(k)) }
        g.noteAI(ms: 12_000, chars: 10, backend: "llama_cpu", refine: false, cpuBusy: false, now: 100)
        XCTAssertTrue(g.paused)
        // 8 字のふだん 1,200ms の 1.5 倍 + 0.4 秒 = 2,200ms 以内なら良 (前は 1.5 秒を超えると悪)
        g.noteProbe(ms: 1700, chars: 5, backend: "llama_cpu")
        XCTAssertTrue(g.paused)
        g.noteProbe(ms: 1700, chars: 5, backend: "llama_cpu")
        XCTAssertFalse(g.paused)
        // 2026-11-19: 線は字数によらず 1 本 (前は GPU で 7 字 1,400ms が良・8 字 300ms が悪と逆になっていた)
        let v = warmed()
        XCTAssertEqual(v.probeLimit(chars: 7, backend: "vulkan"), 640)
        XCTAssertEqual(v.probeLimit(chars: 8, backend: "vulkan"), 640)
        XCTAssertEqual(v.probeLimit(chars: 20, backend: "vulkan"), 1000)
        for chars in 1..<40 {
            XCTAssertLessThanOrEqual(v.probeLimit(chars: chars, backend: "vulkan"), v.probeLimit(chars: chars + 1, backend: "vulkan"))
        }
    }

    func testProbeComparesWithTheBaseline() {
        var g = warmed()
        g.noteAI(ms: 4100, chars: 10, backend: "vulkan", refine: false, cpuBusy: false, now: 100)
        // ふだん 200ms (10 字) の 1.5 倍 + 0.4 秒 = 700ms
        g.noteProbe(ms: 800, chars: 10, backend: "vulkan")
        g.noteProbe(ms: 690, chars: 10, backend: "vulkan")
        XCTAssertTrue(g.paused)
        g.noteProbe(ms: 690, chars: 10, backend: "vulkan")
        XCTAssertFalse(g.paused)
    }

    /// 2026-11-19: 設定に書いていなければ自動の休みはオフ (0.1.0 の初期値)。書いてあるオンはそのまま
    @MainActor
    func testAutoPauseIsOffWhenNotSet() {
        let saved = config["aiAutoPause"]
        defer { config["aiAutoPause"] = saved }
        config["aiAutoPause"] = nil
        XCTAssertFalse(aiAutoPauseEnabled())
        config["aiAutoPause"] = true
        XCTAssertTrue(aiAutoPauseEnabled())
    }

    /// 変換の頭の決め: 休むのは AI の設定がオンの AI の変換だけ。清書の印があれば試し、印が無ければ 120 秒まで試さない
    @MainActor
    func testBeginConversionPausesOnlyAIConversions() {
        let saved = (config["enable"], config["aiAutoPause"], aiLoadGuard, nextConvertRefine)
        defer { config["enable"] = saved.0; config["aiAutoPause"] = saved.1; aiLoadGuard = saved.2; nextConvertRefine = saved.3 }
        config["enable"] = true
        config["aiAutoPause"] = true
        aiLoadGuard = warmed(aiBackendName)
        XCTAssertFalse(beginAIConversion(dictionaryOnly: false).paused)
        aiLoadGuard.noteAI(ms: 5000, chars: 10, backend: aiBackendName, refine: false, cpuBusy: false, now: nowSeconds() - 100)
        XCTAssertTrue(aiLoadGuard.paused)
        XCTAssertTrue(aiPausedNow())
        // 辞書だけの変換 (仮表示) はそのまま
        XCTAssertFalse(beginAIConversion(dictionaryOnly: true).paused)
        // AI の変換は休む (清書の印が無く、落ちてから 120 秒たっていなければ試さない)
        let space = beginAIConversion(dictionaryOnly: false)
        XCTAssertTrue(space.paused && !space.probe)
        // 清書の印があれば試し (印は 1 回で消える)
        set_convert_refine(refine: true)
        let refine = beginAIConversion(dictionaryOnly: false)
        XCTAssertTrue(refine.probe && !refine.paused && refine.refine)
        XCTAssertFalse(nextConvertRefine)
        // 自動で休む設定がオフ・AI を切った設定では何もしない
        config["aiAutoPause"] = false
        XCTAssertFalse(beginAIConversion(dictionaryOnly: false).paused)
        XCTAssertFalse(aiPausedNow())
        config["aiAutoPause"] = true
        config["enable"] = false
        XCTAssertFalse(beginAIConversion(dictionaryOnly: false).paused)
        XCTAssertFalse(aiPausedNow())
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ: 落ちた状態で、古い IME 本体と同じ問い合わせ (清書の印なし・AI ありの変換) が来ても、
    /// 辞書だけ向けの変換 (辞書だけの変換・速くない形) で正しく返る。新しい IME 本体には「AI を休んだ」の印が返る (古い IME 本体は読まないだけ)
    @MainActor
    func testPausedConversionAnswersWithTheDictionaryResult() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        let saved = (config["enable"], config["aiAutoPause"], aiLoadGuard)
        defer { config["enable"] = saved.0; config["aiAutoPause"] = saved.1; aiLoadGuard = saved.2 }
        func convert(_ reading: String, dictionaryOnly: Bool) -> String {
            var count: Int32 = 0
            let converted = reading.withCString { r in "".withCString { c in
                convert_segments(hiraganaPtr: r, contextPtr: c, fast: false, dictionaryOnly: dictionaryOnly, lengthPtr: &count)
            } }
            return (0..<Int(count)).compactMap { i in converted[i].map { String(cString: $0.pointee.text) } }.joined()
        }
        config["aiAutoPause"] = true
        for reading in ["きしゃのきしゃがきしゃした", "にかいのへやでじゅうにかいのでんわをした"] {
            // 辞書だけ向け = 辞書だけの変換・速くない形 (仕事 54 の直しもかかる)
            config["enable"] = true
            aiLoadGuard = warmed(aiBackendName)
            let dictionary = convert(reading, dictionaryOnly: true)
            XCTAssertFalse(last_conversion_ai_paused())
            aiLoadGuard.noteAI(ms: 5000, chars: 10, backend: aiBackendName, refine: false, cpuBusy: false, now: nowSeconds())
            XCTAssertTrue(aiLoadGuard.paused)
            let paused = convert(reading, dictionaryOnly: false)
            XCTAssertTrue(last_conversion_ai_paused())
            XCTAssertEqual(paused, dictionary, reading)
        }
    }

    func testCPUBusyMeterNeedsTenSecondsAboveNinetyPercent() {
        var m = CPUBusyMeter()
        XCTAssertFalse(m.busy(now: 0, idle: 0, total: 0))
        XCTAssertFalse(m.busy(now: 5, idle: 10, total: 1000))
        // 0 秒から 12 秒で idle 50 / total 1000 → 95% 使用
        XCTAssertTrue(m.busy(now: 12, idle: 50, total: 1000))
        // 12 秒からの 10 秒で idle 300 / total 1000 → 70%
        XCTAssertFalse(m.busy(now: 22, idle: 350, total: 2000))
        // 2026-11-19: 自分の CPU の分は引く。全体は 95% 使用でも、そのうち 40% が自分なら、ほかは 55% で忙しくない
        var own = CPUBusyMeter()
        XCTAssertFalse(own.busy(now: 0, idle: 0, total: 0, own: 0))
        XCTAssertFalse(own.busy(now: 12, idle: 50, total: 1000, own: 400))
        var others = CPUBusyMeter()
        XCTAssertFalse(others.busy(now: 0, idle: 0, total: 0, own: 0))
        XCTAssertTrue(others.busy(now: 12, idle: 50, total: 1000, own: 20))
    }
}
