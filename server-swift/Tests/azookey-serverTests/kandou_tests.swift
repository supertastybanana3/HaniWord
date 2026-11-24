import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

final class KandouTests: XCTestCase {
    func e(_ word: String, _ ruby: String, _ cid: Int = 1285) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: cid, rcid: cid, mid: 501, value: -10)
    }

    func testFinalWaraIsFoundOnlyAtTheEnd() {
        // それな|藁 (文末の わら) は替える所。藁 だけ (1 語)・文の途中の 藁・読みの違う 藁 (わらび ではない) は見ない
        XCTAssertTrue(endsWithWara([e("それな", "ソレナ"), e("藁", "ワラ")]))
        XCTAssertTrue(endsWithWara([e("それな", "ソレナ"), e("ワラ", "ワラ")]))
        XCTAssertFalse(endsWithWara([e("藁", "ワラ")]))
        XCTAssertFalse(endsWithWara([e("藁", "ワラ"), e("を", "ヲ", 156), e("運ぶ", "ハコブ")]))
        XCTAssertFalse(endsWithWara([e("それな", "ソレナ"), e("笑", "ワラ")]))
    }

    func testSentenceHeadIsAfterNothingOrASentenceEnd() {
        XCTAssertTrue(atSentenceHead(""))
        XCTAssertTrue(atSentenceHead("そうだった。"))
        XCTAssertTrue(atSentenceHead("本当？ "))
        XCTAssertTrue(atSentenceHead("「"))
        XCTAssertFalse(atSentenceHead("今日は"))
        XCTAssertFalse(atSentenceHead("それで、"))
    }

    func candidate(_ data: [DicdataElement]) -> Candidate {
        Candidate(text: data.map(\.word).joined(), value: -10, correspondingCount: data.map(\.ruby).joined().count, lastMid: 501, data: data)
    }

    /// 2026-11-23: 文の頭の 1 語 + 読点で、その読みに感動詞があれば感動詞に替える。辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ
    @MainActor
    func testSentenceHeadWordBeforeACommaBecomesTheInterjection() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        let learned = [e("運", "ウン"), e("、", "、", 5), e("そう", "ソウ"), e("だね", "ダネ")]
        // 読点が文節に入った形 (うん、そうだね) も、読点の前で分けた部分 (うん + 後ろの字が 、) も替える
        let joined = replacingInitialInterjection(candidate(learned), hiragana: "うん、そうだね", context: "", follower: nil)
        XCTAssertEqual(joined?.data.first?.word, "うん")
        XCTAssertEqual(joined?.data.first?.lcid, interjectionCid)
        XCTAssertEqual(joined?.text, "うん、そうだね")
        let part = replacingInitialInterjection(candidate([e("運", "ウン")]), hiragana: "うん", context: "", follower: "、")
        XCTAssertEqual(part?.text, "うん")
        // 文の頭でない・後ろが読点でない・読みが感動詞と違う (運転)・もう感動詞・登録した語 は替えない
        XCTAssertNil(replacingInitialInterjection(candidate(learned), hiragana: "うん、そうだね", context: "今日の", follower: nil))
        XCTAssertNil(replacingInitialInterjection(candidate([e("運", "ウン"), e("が", "ガ", 147)]), hiragana: "うんが", context: "", follower: nil))
        XCTAssertNil(replacingInitialInterjection(candidate([e("運", "ウン")]), hiragana: "うん", context: "", follower: nil))
        XCTAssertNil(replacingInitialInterjection(candidate([e("運転", "ウンテン"), e("、", "、", 5)]), hiragana: "うんてん、", context: "", follower: nil))
        XCTAssertNil(replacingInitialInterjection(candidate([e("うん", "ウン", interjectionCid), e("、", "、", 5)]), hiragana: "うん、", context: "", follower: nil))
        // 読みのひらがなと同じ字の語 (品詞が感動詞でなくても) は替えない。替える先はひらがなの感動詞だけ (まア・くッ には替えない。
        // ひらがなの感動詞が辞書に無い くっ は クッ のまま)。カタカナを選んでいたときも、ひらがなの感動詞があれば替える
        XCTAssertNil(replacingInitialInterjection(candidate([e("まあ", "マア", 1285), e("、", "、", 5)]), hiragana: "まあ、", context: "", follower: nil))
        XCTAssertNil(replacingInitialInterjection(candidate([e("クッ", "クッ"), e("、", "、", 5)]), hiragana: "くっ、", context: "", follower: nil))
        let katakana = replacingInitialInterjection(candidate([e("ウン", "ウン"), e("、", "、", 5)]), hiragana: "うん、", context: "", follower: nil)
        XCTAssertEqual(katakana?.data.first?.word, "うん")
        var registered = e("運", "ウン")
        registered.metadata = .isUserRegistered
        XCTAssertNil(replacingInitialInterjection(candidate([registered, e("、", "、", 5)]), hiragana: "うん、", context: "", follower: nil))
    }
}
