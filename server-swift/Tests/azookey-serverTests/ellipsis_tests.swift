import XCTest
@testable import azookey_server

/// 2026-11-06 (まとめ役 09:57): 。。。・・・・... を … に (候補の 2 番目、設定 input.ellipsis_auto で自動)
final class EllipsisTests: XCTestCase {
    func testDotRunsBecomeOneSegment() {
        var calls: [String] = []
        let convert: (String, String) -> [(word: String, rubyCount: Int)] = { reading, _ in
            calls.append(reading)
            return [(word: reading, rubyCount: reading.count)]
        }
        let out = preservingLiteralSymbols(reading: "えっと。。。・・・・・それ...", context: "", convert: convert)
        XCTAssertEqual(calls, ["えっと", "それ"])
        XCTAssertEqual(out?.map(\.word), ["えっと", "。。。", "・・・・・", "それ", "..."])
        XCTAssertEqual(out?.map(\.rubyCount), [3, 3, 5, 2, 3])
        // 2 つまでは今までどおり 1 字ずつ
        XCTAssertEqual(preservingLiteralSymbols(reading: "あ。。", context: "", convert: convert)?.map(\.word), ["あ", "。", "。"])
    }

    func testEllipsisForms() {
        XCTAssertEqual(ellipsisForm("。。。"), "…")
        XCTAssertEqual(ellipsisForm("・・・"), "…")
        XCTAssertEqual(ellipsisForm("..."), "…")
        XCTAssertEqual(ellipsisForm("。。。。。。"), "……")
        XCTAssertNil(ellipsisForm("。。"))
        XCTAssertNil(ellipsisForm("・・・・・"))
        XCTAssertNil(ellipsisForm("。・。"))
        XCTAssertNil(ellipsisForm("あああ"))
    }

    func testAutoAndList() {
        let segments: [(word: String, rubyCount: Int)] = [("えっと", 3), ("。。。", 3), ("・・・・・", 5)]
        XCTAssertEqual(replacingEllipsis(segments, enabled: false).map(\.word), ["えっと", "。。。", "・・・・・"])
        XCTAssertEqual(replacingEllipsis(segments, enabled: true).map(\.word), ["えっと", "…", "・・・・・"])
        XCTAssertEqual(replacingEllipsis(segments, enabled: true).map(\.rubyCount), [3, 3, 5])
        XCTAssertEqual(placingEllipsis(["。。。", "｡｡｡"], reading: "。。。", auto: false), ["。。。", "…", "｡｡｡"])
        XCTAssertEqual(placingEllipsis(["。。。", "｡｡｡"], reading: "。。。", auto: true), ["…", "。。。", "｡｡｡"])
        XCTAssertEqual(placingEllipsis(["今日"], reading: "きょう", auto: true), ["今日"])
    }
}
