import XCTest
@testable import azookey_server

/// 2026-11-06 (仕事 34 の 4): 式のあとの = で答えを候補に (calc.swift)
final class CalcTests: XCTestCase {
    func testResults() {
        XCTAssertEqual(calcResult("1+2="), "3")
        XCTAssertEqual(calcResult("１＋２＝"), "3")
        XCTAssertEqual(calcResult("10ー3＝"), "7")
        XCTAssertEqual(calcResult("2*(3+4)="), "14")
        XCTAssertEqual(calcResult("7/2="), "3.5")
        XCTAssertEqual(calcResult("7・2＝"), "3.5")
        XCTAssertEqual(calcResult("2^10="), "1024")
        XCTAssertEqual(calcResult("-3+5="), "2")
        XCTAssertEqual(calcResult("1/3="), "0.3333333333")
        XCTAssertEqual(calcResult("0.1+0.2="), "0.3")
    }

    func testNotExpressions() {
        XCTAssertNil(calcResult("12="))
        XCTAssertNil(calcResult("1+2"))
        XCTAssertNil(calcResult("1/0="))
        XCTAssertNil(calcResult("a+b="))
        XCTAssertNil(calcResult("きょう+1="))
        XCTAssertNil(calcResult("(1+2="))
        XCTAssertNil(calcResult("+="))
    }

    func testSegmentsAndList() {
        let segments: [(word: String, rubyCount: Int)] = [("1", 1), ("+", 1), ("2", 1), ("=", 1)]
        XCTAssertEqual(mergingCalcSegments(segments, reading: "1+2=", enabled: true).map(\.word), ["1+2="])
        XCTAssertEqual(mergingCalcSegments(segments, reading: "1+2=", enabled: false).count, 4)
        XCTAssertEqual(placingCalcResult(["1+2=", "１＋２＝"], reading: "1+2=", enabled: true), ["1+2=", "1+2=3", "3", "１＋２＝"])
        XCTAssertEqual(placingCalcResult(["1+2="], reading: "1+2=", enabled: false), ["1+2="])
        XCTAssertEqual(placingCalcResult(["今日"], reading: "きょう", enabled: true), ["今日"])
        XCTAssertEqual(placingCalcResult(["１＋２＝"], reading: "１＋２＝", enabled: true), ["１＋２＝", "１＋２＝３", "３"])
        XCTAssertEqual(placingCalcResult(["７／２＝"], reading: "7・2＝", enabled: true), ["７／２＝", "７／２＝３．５", "３．５"])
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ: かなの入力で打った形 (＋ ＝ ー ・ ＊ が全角) の読みが 1 つの文節になり、一覧の 2・3 番目に答えが出る。
    /// 設定がオフなら文節も一覧も変わらない
    @MainActor
    func testRealReadings() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        let saved = config["calcCandidates"]
        defer { config["calcCandidates"] = saved }
        func run(_ reading: String) -> ([String], [String]) {
            var count: Int32 = 0
            let converted = reading.withCString { r in "".withCString { c in
                convert_segments(hiraganaPtr: r, contextPtr: c, fast: false, dictionaryOnly: true, lengthPtr: &count)
            } }
            let segments = (0..<Int(count)).compactMap { i in converted[i].map { String(cString: $0.pointee.text) } }
            var listCount: Int32 = 0
            let listed = reading.withCString { r in "".withCString { c in
                list_candidates(hiraganaPtr: r, contextPtr: c, lengthPtr: &listCount)
            } }
            return (segments, (0..<Int(listCount)).compactMap { i in listed[i].map { String(cString: $0.pointee.text) } })
        }
        for (reading, answer) in [("1＋2＝", "3"), ("10ー3＝", "7"), ("7・2＝", "3.5"), ("2＊（3＋4）＝", "14"), ("１＋２＝", "３"), ("1+2=", "3")] {
            config["calcCandidates"] = false
            let (offSegments, offList) = run(reading)
            config["calcCandidates"] = true
            let (segments, list) = run(reading)
            print("calc \(reading) off=\(offSegments) \(offList.prefix(3)) on=\(segments) \(list.prefix(4))")
            XCTAssertEqual(segments.count, 1, reading)
            XCTAssertEqual(list.dropFirst().first.map { $0.hasSuffix(answer) }, true, "\(reading) \(list.prefix(4))")
            XCTAssertEqual(Array(list.dropFirst(2).prefix(1)), [answer], reading)
            XCTAssertFalse(offList.contains(answer), reading)
        }
    }
}
