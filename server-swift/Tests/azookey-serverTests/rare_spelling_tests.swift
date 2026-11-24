import XCTest
@testable import azookey_server

/// 2026-11-08 (仕事 42 の直し): 常用外の字の表記の表の語は、ふつうの書き方より下の候補 (rare_spelling.swift)
final class RareSpellingTests: XCTestCase {
    private func w(_ word: String, _ lcid: Int) -> RareSpellingWord { RareSpellingWord(word: word, lcid: lcid) }

    func testDictionaryWordGoesBackToTheTop() {
        // 表が無いときの 1 番目が辞書の 1 語 (編む) なら、それを 1 番目に戻し、表の語 (虻) はすぐ後ろ
        XCTAssertEqual(orderingRareSpelling(["虻", "亜夢", "編む", "あむ"], previous: "編む", words: [w("編む", 712)]), ["編む", "虻", "亜夢", "あむ"])
        // 辞書の語＋助詞 (隠し|て)
        XCTAssertEqual(orderingRareSpelling(["斯くして", "隠して"], previous: "隠して", words: [w("隠し", 735), w("て", 307)]), ["隠して", "斯くして"])
        // 前の 1 番目が今の一覧に無ければ足す
        XCTAssertEqual(orderingRareSpelling(["虻", "亜夢"], previous: "編む", words: [w("編む", 712)]), ["編む", "虻", "亜夢"])
    }

    func testSameWordWithKanaGoesBackToTheTop() {
        // 同じ語の一部をかなにした形 (阿弥陀|くじ・キツネ|うどん) は、語が 2 つでも戻す
        XCTAssertEqual(orderingRareSpelling(["阿弥陀籤", "阿弥陀くじ"], previous: "阿弥陀くじ", words: [w("阿弥陀", 1285), w("くじ", 1285)]), ["阿弥陀くじ", "阿弥陀籤"])
        XCTAssertEqual(orderingRareSpelling(["狐饂飩", "狐うどん", "キツネうどん"], previous: "キツネうどん", words: [w("キツネ", 1285), w("うどん", 1285)]),
                       ["キツネうどん", "狐饂飩", "狐うどん"])
        // 語の並びが分からなくても、漢字が全部今の 1 番目にあれば戻す
        XCTAssertEqual(orderingRareSpelling(["嗚呼嗚呼", "嗚呼ああ"], previous: "嗚呼ああ", words: nil), ["嗚呼ああ", "嗚呼嗚呼"])
    }

    func testBrokenJoinKeepsRareSpellingFirst() {
        // 表が無いときの 1 番目がばらばらの語をつないだ崩れた形 (消し|かけ|て、秋|済す) なら、表の語が 1 番目のまま
        XCTAssertEqual(orderingRareSpelling(["嗾けて", "消しかけて"], previous: "消しかけて", words: [w("消し", 735), w("かけ", 735), w("て", 307)]), ["嗾けて", "消しかけて"])
        XCTAssertEqual(orderingRareSpelling(["秋茄子", "秋済す"], previous: "秋済す", words: [w("秋", 1285), w("済す", 712)]), ["秋茄子", "秋済す"])
    }

    func testNothingChangesWhenTheTableDidNotChangeTheTop() {
        // 表にもある 死屍累々 が、表が無くても 1 番目なら何もしない (前の形は 死屍累累 を上げていた)
        XCTAssertEqual(orderingRareSpelling(["死屍累々", "死屍累累"], previous: "死屍累々", words: [w("死屍", 1285), w("累々", 1287)]), ["死屍累々", "死屍累累"])
        XCTAssertEqual(orderingRareSpelling([], previous: "編む", words: nil), [])
    }

    func testOnlyTheRareKanjiTableIsMarked() {
        XCTAssertEqual(rareSpellingIdentifiers(environment: [:], always: ["domain_core_forms", "domain_core_rare_kanji", "domain_law"]), ["domain_core_rare_kanji"])
        XCTAssertEqual(rareSpellingIdentifiers(environment: [:], always: ["domain_core_forms"]), [])
        XCTAssertEqual(rareSpellingIdentifiers(environment: ["AZK_RARE_DEMOTE": "0"], always: ["domain_core_rare_kanji"]), [])
    }
}
