import Foundation
import KanaKanjiConverterModule
import XCTest
@testable import azookey_server

/// 2026-11-18 (perf の 7): zenz の裏読みの受け渡し (zenz_preload.swift の ZenzPreloadSlot)
final class ZenzPreloadSlotTests: XCTestCase {
    func testNotFinishedUntilTheThreadFinishes() {
        let slot = ZenzPreloadSlot()
        XCTAssertFalse(slot.take().finished)
        // 読めなかった (nil) も「済んだ」として渡す (変換器は以後 AI を使わない)
        slot.finish(nil)
        let taken = slot.take()
        XCTAssertTrue(taken.finished)
        XCTAssertNil(taken.preload)
    }

    func testFinishFromAnotherThreadIsSeen() {
        let slot = ZenzPreloadSlot()
        let done = expectation(description: "finished on another thread")
        let thread = Thread {
            slot.finish(nil)
            done.fulfill()
        }
        thread.start()
        wait(for: [done], timeout: 10)
        XCTAssertTrue(slot.take().finished)
    }
}

/// 2026-11-19 (C の見直し・軽微 4): モデルを裏で読めなかったら、以後は読んでいる間と同じく辞書だけで返し、問い合わせのたびに読み直さない
final class ZenzPreloadFailureTests: XCTestCase {
    func testMissingModelFileGivesNoPreload() {
        XCTAssertNil(ZenzPreload.load(resourceURL: URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("no_such_model_for_test.gguf")))
    }

    /// 裏のスレッドが「読めなかった」を渡したら: 以後 zenzai は .off (変換器の getModel を呼ばない = 主のスレッドで読み直さない)、裏読みも始め直さない
    @MainActor
    func testFailedPreloadTurnsAIOffWithoutReloading() throws {
        if zenzWarming() { throw XCTSkip("a real preload is in flight") }
        let saved = (config["enable"], zenzPreloadStarted, zenzPreloadInstalled, zenzPreloadFailed, zenzPreloadAutoStart)
        defer {
            config["enable"] = saved.0
            zenzPreloadStarted = saved.1; zenzPreloadInstalled = saved.2; zenzPreloadFailed = saved.3; zenzPreloadAutoStart = saved.4
            zenzPreloadSlot.reset()
        }
        config["enable"] = true
        zenzPreloadStarted = true; zenzPreloadInstalled = false; zenzPreloadFailed = false; zenzPreloadAutoStart = true
        zenzPreloadSlot.reset()
        XCTAssertTrue(zenzWarming())
        XCTAssertEqual(getOptions().zenzaiMode, .off)
        zenzPreloadSlot.finish(nil)
        installPreloadedZenzIfReady()
        XCTAssertTrue(zenzPreloadFailed)
        XCTAssertFalse(zenzWarming())
        for _ in 0..<3 {
            XCTAssertEqual(getOptions().zenzaiMode, .off)
            XCTAssertEqual(getOptions(fast: true).zenzaiMode, .off)
        }
        XCTAssertFalse(aiConversionEnabled())
        XCTAssertTrue(zenzPreloadStarted)
        XCTAssertFalse(zenzPreloadInstalled)
    }

    /// 2026-11-19: 測りの道具が聞く口: 読んでいる間は 0、読み終えたら 1 (読まない設定も 1)、読めなければ 2
    @MainActor
    func testPreloadStateForMeasurementTools() throws {
        if zenzWarming() { throw XCTSkip("a real preload is in flight") }
        let saved = (zenzPreloadStarted, zenzPreloadInstalled, zenzPreloadFailed)
        defer {
            zenzPreloadStarted = saved.0; zenzPreloadInstalled = saved.1; zenzPreloadFailed = saved.2
            zenzPreloadSlot.reset()
        }
        zenzPreloadStarted = false; zenzPreloadInstalled = false; zenzPreloadFailed = false
        XCTAssertEqual(zenz_preload_state(), 1)
        zenzPreloadStarted = true
        zenzPreloadSlot.reset()
        XCTAssertEqual(zenz_preload_state(), 0)
        XCTAssertEqual(zenz_preload_state(), 0)
        // 裏のスレッドが「読めなかった」(モデルのファイルが無い) を渡したら 2
        XCTAssertNil(ZenzPreload.load(resourceURL: URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("no_such_model_for_state.gguf")))
        zenzPreloadSlot.finish(nil)
        XCTAssertEqual(zenz_preload_state(), 2)
        XCTAssertTrue(zenzPreloadFailed)
    }

    /// 2026-11-19: 設定の読み直しで、読めなかったあとなら 1 回だけ始め直す (AI を切っていれば始めない。2 回目はしない)
    @MainActor
    func testRetryOnceAfterAFailure() throws {
        if zenzWarming() { throw XCTSkip("a real preload is in flight") }
        let saved = (config["enable"], config["model"], zenzPreloadStarted, zenzPreloadInstalled, zenzPreloadFailed, zenzPreloadRetried)
        defer {
            config["enable"] = saved.0; config["model"] = saved.1
            zenzPreloadStarted = saved.2; zenzPreloadInstalled = saved.3; zenzPreloadFailed = saved.4; zenzPreloadRetried = saved.5
            zenzPreloadSlot.reset()
        }
        config["model"] = "no_such_model_for_test.gguf"
        zenzPreloadStarted = true; zenzPreloadInstalled = false; zenzPreloadFailed = true; zenzPreloadRetried = false
        config["enable"] = false
        XCTAssertFalse(retryFailedZenzPreloadIfNeeded())
        XCTAssertTrue(zenzPreloadFailed)
        config["enable"] = true
        XCTAssertTrue(retryFailedZenzPreloadIfNeeded())
        XCTAssertFalse(zenzPreloadFailed)
        XCTAssertTrue(zenzWarming())
        // 無いモデルなので、また読めない。2 回目は始め直さない
        let limit = Date().addingTimeInterval(30)
        while zenzWarming() && Date() < limit {
            installPreloadedZenzIfReady()
            Thread.sleep(forTimeInterval: 0.05)
        }
        XCTAssertTrue(zenzPreloadFailed)
        XCTAssertFalse(retryFailedZenzPreloadIfNeeded())
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ: 読めなかったあとの AI ありの変換は、辞書だけの変換と同じ字で返り、休みの印は立てない
    @MainActor
    func testFailedPreloadAnswersWithTheDictionaryResult() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        let saved = (config["enable"], zenzPreloadStarted, zenzPreloadInstalled, zenzPreloadFailed)
        defer { config["enable"] = saved.0; zenzPreloadStarted = saved.1; zenzPreloadInstalled = saved.2; zenzPreloadFailed = saved.3 }
        func convert(_ reading: String, dictionaryOnly: Bool) -> String {
            var count: Int32 = 0
            let converted = reading.withCString { r in "".withCString { c in
                convert_segments(hiraganaPtr: r, contextPtr: c, fast: false, dictionaryOnly: dictionaryOnly, lengthPtr: &count)
            } }
            return (0..<Int(count)).compactMap { i in converted[i].map { String(cString: $0.pointee.text) } }.joined()
        }
        config["enable"] = true
        for reading in ["きしゃのきしゃがきしゃした", "にかいのへやでじゅうにかいのでんわをした"] {
            let dictionary = convert(reading, dictionaryOnly: true)
            zenzPreloadStarted = true; zenzPreloadInstalled = false; zenzPreloadFailed = true
            XCTAssertEqual(convert(reading, dictionaryOnly: false), dictionary, reading)
            XCTAssertFalse(last_conversion_ai_paused())
            XCTAssertEqual(getOptions().zenzaiMode, .off)
            zenzPreloadFailed = saved.3; zenzPreloadStarted = saved.1
        }
    }
}
