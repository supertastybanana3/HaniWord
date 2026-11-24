import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-23: 候補の一覧の頭の入れ替え (buildCandidatesCore の 2.) で、読みの頭から末尾の助詞の前までを 1 語で覆う辞書の語があり、
/// それが 1 番の最初の語より長いときは、その長さでも頭を入れ替えて先に並べる (前は のわきとは の一覧の 10 番目までに 野分とは が無く、
/// の|脇 の頭を入れ替えた 之脇とは・乗脇とは… が並んでいた)。辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ
final class ListLongHeadTests: XCTestCase {
    @MainActor
    func testWholeWordBeforeTheParticleIsListedEarly() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        for (reading, word) in [("のわきとは", "野分とは"), ("だげんが", "打弦が")] {
            var count: Int32 = 0
            let listed = reading.withCString { r in "".withCString { c in
                list_candidates(hiraganaPtr: r, contextPtr: c, lengthPtr: &count)
            } }
            let list = (0..<Int(count)).compactMap { i in listed[i].map { String(cString: $0.pointee.text) } }
            XCTAssertTrue(list.prefix(5).contains(word), "\(reading) \(list.prefix(12))")
        }
    }
}
