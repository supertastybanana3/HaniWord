import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-06 (仕事 (4) と一緒の品詞の直し): 品詞に合わない漢字を、同じ読みでつながる語に替える (pos_mismatch.swift)
final class PosMismatchTests: XCTestCase {
    func element(_ word: String, _ ruby: String, _ lcid: Int, _ rcid: Int? = nil) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: lcid, rcid: rcid ?? lcid, mid: 0, value: -10)
    }

    func testPatterns() {
        // 名詞＋な＋名詞 は形容動詞の語幹に
        let p1 = [element("器械", "キカイ", 1285), element("な", "ナ", 454), element("形", "カタチ", 1285)]
        XCTAssertEqual(posMismatches(p1).map(\.index), [0])
        XCTAssertEqual(posMismatches(p1).map(\.wantLcid), [1287])
        // 名詞＋な＋の (機会なので)・文の終わり・形容動詞の語幹＋な は替えない
        XCTAssertTrue(posMismatches([element("機会", "キカイ", 1285), element("な", "ナ", 454), element("の", "ノ", 1310)]).isEmpty)
        XCTAssertTrue(posMismatches([element("機会", "キカイ", 1285), element("な", "ナ", 454)]).isEmpty)
        XCTAssertTrue(posMismatches([element("奇怪", "キカイ", 1287), element("な", "ナ", 454), element("形", "カタチ", 1285)]).isEmpty)
        // な＋名詞 が 1 語 (な形/ナカタチ: 左 454・右 1285) でも
        XCTAssertEqual(posMismatches([element("機会", "キカイ", 1285), element("な形", "ナカタチ", 454, 1285)]).map(\.index), [0])
        // 形容動詞の語幹＋し・する はサ変の名詞に。サ変の名詞＋し は替えない
        let p2 = [element("高尚", "コウショウ", 1287), element("し", "シ", 610), element("て", "テ", 307)]
        XCTAssertEqual(posMismatches(p2).map(\.wantLcid), [1283])
        XCTAssertTrue(posMismatches([element("交渉", "コウショウ", 1283), element("し", "シ", 610)]).isEmpty)
        // かなだけの語は替えない
        XCTAssertTrue(posMismatches([element("きかい", "キカイ", 1285), element("な", "ナ", 454), element("形", "カタチ", 1285)]).isEmpty)
    }

    func testFixing() {
        let p1 = [element("器械", "キカイ", 1285), element("な", "ナ", 454), element("形", "カタチ", 1285)]
        let fixed = fixingPosMismatches(p1) { ruby, lcid in ruby == "キカイ" && lcid == 1287 ? self.element("奇怪", "キカイ", 1287) : nil }
        XCTAssertEqual(fixed?.map(\.word), ["奇怪", "な", "形"])
        XCTAssertEqual(fixed?.first?.lcid, 1287)
        XCTAssertNil(fixingPosMismatches(p1) { _, _ in nil })
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ: 辞書だけの変換で、品詞に合わない形を直し、正しい形 (機会なので・学生なのに) は変えない
    @MainActor
    func testRealReadings() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        func run(_ reading: String) -> String {
            clearPreviewMemo()
            var count: Int32 = 0
            let converted = reading.withCString { r in "".withCString { c in
                convert_segments(hiraganaPtr: r, contextPtr: c, fast: false, dictionaryOnly: true, lengthPtr: &count)
            } }
            return (0..<Int(count)).compactMap { i in converted[i].map { String(cString: $0.pointee.text) } }.joined()
        }
        for (reading, expected) in [("きかいなじけん", "奇怪な事件"), ("きかいなかたち", "奇怪な形"), ("きかいなので", "機会なので"),
                                    ("がくせいなのに", "学生なのに")] {
            let text = run(reading)
            print("pos \(reading) → \(text)")
            XCTAssertEqual(text, expected, reading)
        }
    }
}
