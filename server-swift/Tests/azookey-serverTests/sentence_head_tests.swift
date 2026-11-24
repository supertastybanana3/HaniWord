import KanaKanjiConverterModule
import XCTest
@testable import azookey_server

final class SentenceHeadTests: XCTestCase {
    private func words(_ items: [(String, String, Int)]) -> [DicdataElement] {
        items.map { DicdataElement(word: $0.0, ruby: $0.1, cid: $0.2, mid: 501, value: -10) }
    }

    func testSentenceHeadIsAnEmptyLeftContextOrTheEndOfASentence() {
        XCTAssertTrue(isSentenceHead(""))
        XCTAssertTrue(isSentenceHead("きのうは晴れた。"))
        XCTAssertTrue(isSentenceHead("本当？ "))
        XCTAssertTrue(isSentenceHead("done.\n"))
        XCTAssertTrue(isSentenceHead("一行目\n"))
        // 確定した字のすぐ後ろ (私 + もびーる) と、読点の後ろは文の途中
        XCTAssertFalse(isSentenceHead("私"))
        XCTAssertFalse(isSentenceHead("それから、"))
        XCTAssertFalse(isSentenceHead("API "))
    }

    func testOnlyCandidatesStartingWithAParticleAtTheSentenceHeadAreLowered() {
        // も (助詞 262) + ビール は文の頭でだけ下げる。モビール (名詞) と、文の途中の も|ビール はそのまま
        let particleFirst = words([("も", "モ", 262), ("ビール", "ビール", 1285)])
        let noun = words([("モビール", "モビール", 1288)])
        XCTAssertTrue(startsWithParticleAtSentenceHead(particleFirst, left: ""))
        XCTAssertTrue(startsWithParticleAtSentenceHead(particleFirst, left: "部屋を片付けた。"))
        XCTAssertFalse(startsWithParticleAtSentenceHead(particleFirst, left: "私"))
        XCTAssertFalse(startsWithParticleAtSentenceHead(noun, left: ""))
        // 読みの無い語 (区切りの印) は飛ばして、読みのある最初の語で見る
        XCTAssertTrue(startsWithParticleAtSentenceHead(words([("", "", 1285)]) + particleFirst, left: ""))
        XCTAssertFalse(startsWithParticleAtSentenceHead([], left: ""))
    }
}
