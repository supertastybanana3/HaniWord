import Foundation
import XCTest
@testable import azookey_server

final class HomophoneWordsTests: XCTestCase {
    let words = parseHomophoneWords("""
    # 読み\t語\t勝つ語\t学習\t一覧
    さてつ\t蹉跌\t砂鉄\t1\t1
    きすう\t基数\t奇数\t0\t1
    きすう\t帰趨\t奇数\t1\t0
    かいしゃ\t膾炙\t会社\t0\t0
    きのう\t機能\t昨日\t0\t0
    壊れた行\t語だけ
    さてつ\t砂鉄\t砂鉄\t1\t1
    """)

    func spots(_ left: String, _ output: String, _ reading: String, learned: [String: String]) -> String {
        applyingHomophones(output, learnedHomophoneSpots(left: left, output: output, reading: reading, words: words, learned: learned))
    }

    func testWordsAreRead() {
        XCTAssertEqual(words.entries.count, 9)
        XCTAssertEqual(words.entries[HomophoneKey("奇数")!]?.readings, ["きすう"])
        XCTAssertEqual(words.groups["きすう"], ["基数", "帰趨", "奇数"])
        XCTAssertEqual(words.learnable, ["さてつ\t蹉跌", "きすう\t帰趨"])
        XCTAssertEqual(words.promotable["奇数"]?.map(\.word), ["基数"])
        XCTAssertEqual(words.longestWord, 2)
        // 欄の数が違う行・語と勝つ語が同じ行は読まない
        XCTAssertNil(words.entries[HomophoneKey("語だけ")!])
    }

    func testLearnedWordReplacesItsNeighborsOnly() {
        let learned = ["さてつ": "蹉跌", "きすう": "帰趨", "かいしゃ": "膾炙"]
        XCTAssertEqual(spots("", "事業の砂鉄から学んだ", "じぎょうのさてつからまなんだ", learned: learned), "事業の蹉跌から学んだ")
        // 同じ読みの行の語 (基数) も仲間
        XCTAssertEqual(spots("選挙の", "基数が決まる", "きすうがきまる", learned: learned), "帰趨が決まる")
        // 打った読みに語の読みが無い・長い語の一部 (砂鉄鉱・左の文脈とつないで 黒砂鉄)・学習 (0) の語 (膾炙)・学習していない読み は替えない
        XCTAssertEqual(spots("", "砂鉄", "すなてつ", learned: learned), "砂鉄")
        XCTAssertEqual(spots("", "砂鉄鉱を掘る", "さてつこうをほる", learned: learned), "砂鉄鉱を掘る")
        XCTAssertEqual(spots("黒", "砂鉄", "さてつ", learned: learned), "砂鉄")
        XCTAssertEqual(spots("", "会社に行く", "かいしゃにいく", learned: learned), "会社に行く")
        XCTAssertEqual(spots("", "昨日の砂鉄", "きのうのさてつ", learned: ["きのう": "機能"]), "昨日の砂鉄")
        XCTAssertEqual(spots("", "蹉跌", "さてつ", learned: learned), "蹉跌")
    }

    func testZenzScoresDecideWhichSpotsToKeep() {
        let found = learnedHomophoneSpots(left: "", output: "砂鉄と奇数", reading: "さてつときすう", words: words, learned: ["さてつ": "蹉跌", "きすう": "帰趨"])
        XCTAssertEqual(found.map(\.replacement), ["蹉跌", "帰趨"])
        // 元の文 -20。蹉跌 に替えた文 -25 (差 5 は 6 までなので替える)、帰趨 に替えた文 -30 (差 10 は替えない)
        XCTAssertEqual(acceptedLearnedSpots(found, scores: [-20, -25, -30], bonus: 6, preview: "蹉跌と帰趨").map(\.replacement), ["蹉跌"])
        XCTAssertEqual(acceptedLearnedSpots(found, scores: [-20, -26, -26], bonus: 6, preview: "蹉跌と帰趨").count, 2)
        // 点が取れない (AI がオフ)・数が合わない・非数 なら替えない
        XCTAssertTrue(acceptedLearnedSpots(found, scores: nil, bonus: 6, preview: "蹉跌と帰趨").isEmpty)
        XCTAssertTrue(acceptedLearnedSpots(found, scores: [-20, -21], bonus: 6, preview: "蹉跌と帰趨").isEmpty)
        XCTAssertTrue(acceptedLearnedSpots(found, scores: [.nan, -21, -21], bonus: 6, preview: "蹉跌と帰趨").isEmpty)
        XCTAssertEqual(acceptedLearnedSpots(found, scores: [-20, -21, .nan], bonus: 6, preview: "蹉跌と帰趨").map(\.replacement), ["蹉跌"])
        // 仮表示に学習した語が出ていない所は替えない (清書だけ変わると、表示が切り替わる回数が増える)
        XCTAssertEqual(acceptedLearnedSpots(found, scores: [-20, -21, -21], bonus: 6, preview: "砂鉄と帰趨").map(\.replacement), ["帰趨"])
        XCTAssertTrue(acceptedLearnedSpots(found, scores: [-20, -21, -21], bonus: 6, preview: nil).isEmpty)
        // 文節: 読みの字数は変えない
        let segments = mergingHomophoneReplacements([(word: "砂鉄", rubyCount: 3), (word: "と", rubyCount: 1), (word: "奇数", rubyCount: 3)],
                                                    acceptedLearnedSpots(found, scores: [-20, -25, -30], bonus: 6, preview: "蹉跌と帰趨"))
        XCTAssertEqual(segments.map(\.word), ["蹉跌", "と", "奇数"])
        XCTAssertEqual(segments.map(\.rubyCount), [3, 1, 3])
    }

    func testListPromotesAWordOnlyWhenTheWinnerIsFirst() {
        // 1 番目が勝つ語 (砂鉄)、一覧の語 (蹉跌) が 3 番目 → 2 番目へ
        XCTAssertEqual(promotingHomophoneWords(["砂鉄", "左鉄", "蹉跌", "さてつ"], reading: "さてつ", words: words, learned: [:]),
                       ["砂鉄", "蹉跌", "左鉄", "さてつ"])
        // 文節の中の語でも (砂鉄したら → 蹉跌したら)
        XCTAssertEqual(promotingHomophoneWords(["砂鉄したら", "左鉄したら", "蹉跌したら"], reading: "さてつしたら", words: words, learned: [:]),
                       ["砂鉄したら", "蹉跌したら", "左鉄したら"])
        // 2 番目がすでに同じ読みの語 (基数) なら変えない。一覧 (0) の語 (帰趨) は上げない
        XCTAssertEqual(promotingHomophoneWords(["奇数", "基数", "鬼数", "帰趨"], reading: "きすう", words: words, learned: [:]),
                       ["奇数", "基数", "鬼数", "帰趨"])
        XCTAssertEqual(promotingHomophoneWords(["奇数", "鬼数", "帰趨"], reading: "きすう", words: words, learned: [:]),
                       ["奇数", "鬼数", "帰趨"])
        // 1 番目が勝つ語でない・一覧の語が一覧に無い なら変えない
        XCTAssertEqual(promotingHomophoneWords(["左鉄", "砂鉄", "蹉跌"], reading: "さてつ", words: words, learned: [:]), ["左鉄", "砂鉄", "蹉跌"])
        XCTAssertEqual(promotingHomophoneWords(["砂鉄", "左鉄", "さてつ"], reading: "さてつ", words: words, learned: [:]), ["砂鉄", "左鉄", "さてつ"])
    }

    func testListPutsTheLearnedWordSecond() {
        // 学習した語 (帰趨) は、2 番目が同じ読みの語 (基数) でも 2 番目へ。一覧に無ければ足す
        XCTAssertEqual(promotingHomophoneWords(["奇数", "基数", "鬼数", "帰趨"], reading: "きすう", words: words, learned: ["きすう": "帰趨"]),
                       ["奇数", "帰趨", "基数", "鬼数"])
        XCTAssertEqual(promotingHomophoneWords(["選挙の奇数", "選挙の基数"], reading: "せんきょのきすう", words: words, learned: ["きすう": "帰趨"]),
                       ["選挙の奇数", "選挙の帰趨", "選挙の基数"])
        XCTAssertEqual(promotingHomophoneWords(["奇数", "帰趨", "基数"], reading: "きすう", words: words, learned: ["きすう": "帰趨"]),
                       ["奇数", "帰趨", "基数"])
    }

    func testCommittedWordsAreLearnedAndRevertedOnlyWhereOffered() {
        // 確定した文節に 学習 (1) の語があれば覚える (読みごとに最後の 1 語)
        var learned = learnedHomophoneChoices(readings: ["じぎょうの", "さてつから"], texts: ["事業の", "蹉跌から"], words: words, learned: [:], offered: [])
        XCTAssertEqual(learned, ["さてつ": "蹉跌"])
        // 学習 (0) の語 (基数・膾炙)・文節の読みに読みが無い語 は覚えない
        XCTAssertEqual(learnedHomophoneChoices(readings: ["きすう", "かいしゃ"], texts: ["基数", "膾炙"], words: words, learned: [:], offered: []), [:])
        XCTAssertEqual(learnedHomophoneChoices(readings: ["すなてつ"], texts: ["蹉跌"], words: words, learned: [:], offered: []), [:])
        // 替えた所で元の語を確定したら消す。替えていない所で相手の語を確定しても消さない
        learned = learnedHomophoneChoices(readings: ["じしゃくでさてつを"], texts: ["磁石で砂鉄を"], words: words, learned: learned, offered: [])
        XCTAssertEqual(learned, ["さてつ": "蹉跌"])
        learned = learnedHomophoneChoices(readings: ["じしゃくでさてつを"], texts: ["磁石で砂鉄を"], words: words, learned: learned,
                                          offered: ["さてつ\t砂鉄\t蹉跌"])
        XCTAssertEqual(learned, [:])
        // 替えた語を確定したら、そのまま
        XCTAssertEqual(learnedHomophoneChoices(readings: ["さてつ"], texts: ["蹉跌"], words: words, learned: ["さてつ": "蹉跌"],
                                               offered: ["さてつ\t砂鉄\t蹉跌"]), ["さてつ": "蹉跌"])
    }

    func testForgettingLearningAlsoForgetsLearnedWords() {
        let learned = ["さてつ": "蹉跌", "きすう": "帰趨"]
        XCTAssertEqual(forgottenLearnedHomophones(learned, reading: "さてつした", text: "蹉跌した"), ["さてつ"])
        XCTAssertEqual(forgottenLearnedHomophones(learned, reading: "さてつ", text: "砂鉄"), [])
        XCTAssertEqual(forgottenLearnedHomophones(learned, discarded: ["趨"]), ["きすう"])
        XCTAssertEqual(forgottenLearnedHomophones(learned, discarded: [""]), [])
    }

    func testPublishedWords() throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let text = try String(contentsOf: root.appendingPathComponent("homophone_words.tsv"), encoding: .utf8)
        let start = Date()
        let published = parseHomophoneWords(text)
        print("homophone words: \(Int(-start.timeIntervalSinceNow * 1000)) ms, \(published.entries.count) words, \(published.learnable.count) learnable, \(published.promotable.values.reduce(0) { $0 + $1.count }) promotable")
        XCTAssertGreaterThan(published.entries.count, 10_000)
        XCTAssertTrue(published.learnable.contains("さてつ\t蹉跌"))
        XCTAssertTrue(published.groups["さてつ"]?.contains("砂鉄") ?? false)
        // 会社 のような毎日打つ語の相手は学習しない
        XCTAssertFalse(published.learnable.contains("きのう\t機能"))
    }
}
