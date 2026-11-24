import XCTest
@testable import azookey_server

/// 2026-11-23: 変換の口の呼び返しの守り。辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ
final class ConversionGuardTests: XCTestCase {
    @MainActor
    func testReenteringTheSameReadingReturnsThePlainConversion() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        let reading = "「それはほんとうか？」"
        // 守りが無ければふつうに変換される
        let normal = conversionSegmentsUncached(hiragana: reading, context: "", fast: false, dictionaryOnly: true)
        XCTAssertEqual(normal.reduce(0) { $0 + $1.rubyCount }, reading.count)
        XCTAssertTrue(conversionsInProgress.isEmpty)
        XCTAssertEqual(conversionDepth, 0)
        // 同じ読み・同じ形で入り直した形を作ると、分ける所を通さずに読み全体の 1 番を返す (落ちない)
        let key = "\(QuestionKaMode.decide)\u{1F}true\u{1F}false\u{1F}\(reading)"
        conversionsInProgress.insert(key)
        defer { conversionsInProgress.remove(key) }
        let guarded = conversionSegmentsUncached(hiragana: reading, context: "", fast: false, dictionaryOnly: true)
        XCTAssertEqual(guarded.map(\.word), plainConversionSegments(hiragana: reading, context: "", fast: false, dictionaryOnly: true).map(\.word))
        XCTAssertEqual(guarded.reduce(0) { $0 + $1.rubyCount }, reading.count)
        // 深さの上限でも同じ
        conversionsInProgress.remove(key)
        conversionDepth = conversionDepthLimit
        defer { conversionDepth = 0 }
        let deep = conversionSegmentsUncached(hiragana: "きょうはいい、てんき。", context: "", fast: false, dictionaryOnly: true)
        XCTAssertEqual(deep.reduce(0) { $0 + $1.rubyCount }, "きょうはいい、てんき。".count)
        XCTAssertEqual(conversionDepth, conversionDepthLimit)
    }
}
