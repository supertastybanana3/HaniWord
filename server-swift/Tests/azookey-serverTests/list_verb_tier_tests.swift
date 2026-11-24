import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-10: 候補の一覧で、動詞・形容詞の自立語の基本形 (乞う・問う) を、漢字だけの語 (単漢字を含む) と同じ段に置く (buildCandidatesCore の 3.)
final class ListVerbTierTests: XCTestCase {
    func element(_ word: String, _ cid: Int) -> DicdataElement {
        DicdataElement(word: word, ruby: "コウ", lcid: cid, rcid: cid, mid: 0, value: -5)
    }

    func testDictionaryFormIds() {
        XCTAssertEqual(dictionaryFormInflectableCids.count, 48)
        XCTAssertTrue(isDictionaryFormWord(element("乞う", 802)))   // 動詞,自立,五段・ワ行ウ音便,基本形
        XCTAssertTrue(isDictionaryFormWord(element("問う", 817)))   // 動詞,自立,五段・ワ行促音便,基本形
        XCTAssertTrue(isDictionaryFormWord(element("濃い", 19)))    // 形容詞,自立,形容詞・アウオ段,基本形
        XCTAssertTrue(isDictionaryFormWord(element("問う", 810)))   // 動詞,自立,五段・ワ行ウ音便,連用タ接続 (問うた。綴りは基本形と同じ)
        XCTAssertFalse(isDictionaryFormWord(element("問い", 812)))  // 動詞,自立,五段・ワ行ウ音便,連用形
        XCTAssertFalse(isDictionaryFormWord(element("濃う", 31)))   // 形容詞,自立,形容詞・アウオ段,連用ゴザイ接続
        XCTAssertFalse(isDictionaryFormWord(element("斯う", 1282))) // 副詞
        XCTAssertFalse(isDictionaryFormWord(element("項", 1285)))   // 名詞,一般
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ: 文頭の こう・とう・かく の一覧で 乞う・問う・書く が 15 番目までに入る
    /// (前は 72・44・27 番目。単漢字が全部先だった。並びは変換の点の順なので、何番目かまでは決めない)
    @MainActor
    func testVerbsComeWithinTheFirstFifteen() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        for (reading, verb) in [("こう", "乞う"), ("とう", "問う"), ("かく", "書く")] {
            var count: Int32 = 0
            let listed = reading.withCString { r in "".withCString { c in
                list_candidates(hiraganaPtr: r, contextPtr: c, lengthPtr: &count)
            } }
            let list = (0..<Int(count)).compactMap { i in listed[i].map { String(cString: $0.pointee.text) } }
            XCTAssertTrue(list.prefix(15).contains(verb), "\(reading) \(list.prefix(16))")
        }
    }
}
