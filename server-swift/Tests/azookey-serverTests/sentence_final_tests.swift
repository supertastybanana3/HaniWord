import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-17 (迷うね → 迷う値): 辞書だけの変換で、終止形のすぐ後ろの文の終わりの終助詞の読みを名詞にしていたら、終助詞に戻す
final class SentenceFinalParticleTests: XCTestCase {
    func e(_ word: String, _ ruby: String, _ cid: Int) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: cid, rcid: cid, mid: 0, value: -10)
    }
    func c(_ data: [DicdataElement]) -> Candidate {
        Candidate(text: data.map(\.word).joined(), value: -10, correspondingCount: data.reduce(0) { $0 + $1.ruby.count }, lastMid: 0, data: data)
    }
    let particles: [String: Int] = ["ね": 279, "よ": 288, "さ": 268, "わ": 291, "な": 275, "か": 359]
    func particle(_ reading: String) -> DicdataElement? {
        particles[reading].map { e(reading, String(String.UnicodeScalarView(reading.unicodeScalars.map { Unicode.Scalar($0.value + 0x60)! })), $0) }
    }

    func testNounAfterPredicateBecomesParticle() {
        // 迷う (動詞 五段・ワ行ウ音便 基本形 802) + 値 (名詞) → 迷うね
        let mayou = c([e("それ", "ソレ", 1285), e("は", "ハ", 261), e("迷う", "マヨウ", 802), e("値", "ネ", 1285)])
        XCTAssertEqual(sentenceFinalParticleFixed(mayou, context: "", particle: particle)?.text, "それは迷うね")
        // 無い (形容詞 基本形 20) + 差 → 無いさ
        let nai = c([e("仕方", "シカタ", 1285), e("が", "ガ", 147), e("無い", "ナイ", 20), e("差", "サ", 1285)])
        XCTAssertEqual(sentenceFinalParticleFixed(nai, context: "", particle: particle)?.text, "仕方が無いさ")
        // 辞書に 1 語の句で入っている語 (仕方がない、名詞の品詞) の後ろも、表記の終わりで見る
        let phrase = c([e("仕方がない", "シカタガナイ", 1288), e("差", "サ", 1298)])
        XCTAssertEqual(sentenceFinalParticleFixed(phrase, context: "", particle: particle)?.text, "仕方がないさ")
    }

    func testTailOnlyUsesTheLeftContext() {
        // 後ろの読みだけ (よ → 世) は、左の文脈の終わりが終止形の終わり方のときだけ
        XCTAssertEqual(sentenceFinalParticleFixed(c([e("世", "ヨ", 1285)]), context: "これは迷う", particle: particle)?.text, "よ")
        XCTAssertNil(sentenceFinalParticleFixed(c([e("世", "ヨ", 1285)]), context: "", particle: particle))
        XCTAssertNil(sentenceFinalParticleFixed(c([e("世", "ヨ", 1285)]), context: "この", particle: particle))
    }

    func testParticleEntriesMatchTheDictionary() {
        XCTAssertEqual(dictionaryFinalParticle("さ")?.lcid, 268)
        XCTAssertEqual(dictionaryFinalParticle("よ")?.ruby, "ヨ")
        XCTAssertEqual(dictionaryFinalParticle("か")?.lcid, 359)
        XCTAssertNil(dictionaryFinalParticle("き"))
        for (reading, _) in finalParticleEntries {
            XCTAssertTrue(isFinalParticleCid(dictionaryFinalParticle(reading)!.lcid), reading)
        }
    }

    /// 2026-11-19: 文の終わりに当たらない所 (開く括弧・、・ の前、右の文脈が続く) では替えない
    func testOnlyAtTheSentenceEnd() {
        let ookii = c([e("大きい", "オオキイ", 20), e("差", "サ", 1285)])
        XCTAssertEqual(sentenceFinalParticleFixed(ookii, context: "", atSentenceEnd: true, particle: particle)?.text, "大きいさ")
        XCTAssertNil(sentenceFinalParticleFixed(ookii, context: "", atSentenceEnd: false, particle: particle))
        for follower in ["。", "！", "？", "!", "?", "」", "』", "）", ")", "…", "〜", "～"] as [Character] {
            XCTAssertTrue(finalParticleAtSentenceEnd(follower: follower, rightContext: "続き"), String(follower))
        }
        for follower in ["（", "(", "、", "，", ",", "・", "「", "『", "：", "/"] as [Character] {
            XCTAssertFalse(finalParticleAtSentenceEnd(follower: follower, rightContext: ""), String(follower))
        }
        // 読みの終わり: 右の文脈が空か、文の終わりの記号で始まるときだけ
        XCTAssertTrue(finalParticleAtSentenceEnd(follower: nil, rightContext: ""))
        XCTAssertTrue(finalParticleAtSentenceEnd(follower: nil, rightContext: "。次の文"))
        XCTAssertFalse(finalParticleAtSentenceEnd(follower: nil, rightContext: "（約2倍）"))
        XCTAssertFalse(finalParticleAtSentenceEnd(follower: nil, rightContext: "がある"))
    }

    /// 2026-11-19: すぐ前の語は右の品詞 (後ろにつながる側) で見る。辞書の句で、左が名詞・右が形容詞の基本形なら替え、左が動詞の基本形・右が連用形なら替えない
    func testPreviousWordUsesTheRightPartOfSpeech() {
        func pair(_ word: String, _ ruby: String, lcid: Int, rcid: Int) -> DicdataElement {
            DicdataElement(word: word, ruby: ruby, lcid: lcid, rcid: rcid, mid: 0, value: -10)
        }
        let rightPredicate = c([pair("仕方無い", "シカタナイ", lcid: 1285, rcid: 20), e("差", "サ", 1285)])
        XCTAssertEqual(sentenceFinalParticleFixed(rightPredicate, context: "", particle: particle)?.text, "仕方無いさ")
        let rightContinuative = c([pair("迷い", "マヨイ", lcid: 802, rcid: 812), e("値", "ネ", 1285)])
        XCTAssertNil(sentenceFinalParticleFixed(rightContinuative, context: "", particle: particle))
    }

    func testNounsThatBelongStay() {
        // 名詞の後ろ (山田さ は替えない)・もう終助詞・終助詞の読みでない語・終止形でない前 (連用形) は替えない
        XCTAssertNil(sentenceFinalParticleFixed(c([e("山", "ヤマ", 1285), e("差", "サ", 1285)]), context: "", particle: particle))
        XCTAssertNil(sentenceFinalParticleFixed(c([e("迷う", "マヨウ", 802), e("ね", "ネ", 279)]), context: "", particle: particle))
        XCTAssertNil(sentenceFinalParticleFixed(c([e("迷う", "マヨウ", 802), e("木", "キ", 1285)]), context: "", particle: particle))
        XCTAssertNil(sentenceFinalParticleFixed(c([e("迷い", "マヨイ", 812), e("値", "ネ", 1285)]), context: "", particle: particle))
    }
}

/// 2026-11-23 (a/symseg F5): 読点と括弧を前後と一緒に変換すると 」） が同じ候補の中の語になる。文の終わりの記号の語のすぐ前も文の終わりとして見る
final class SentenceFinalParticleBeforeSymbolTests: XCTestCase {
    func e(_ word: String, _ ruby: String, _ cid: Int) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: cid, rcid: cid, mid: 0, value: -10)
    }
    func c(_ data: [DicdataElement]) -> Candidate {
        Candidate(text: data.map(\.word).joined(), value: -10, correspondingCount: data.reduce(0) { $0 + $1.ruby.count }, lastMid: 0, data: data)
    }
    func particle(_ reading: String) -> DicdataElement? {
        reading == "ね" ? e("ね", "ネ", 279) : nil
    }

    func testParticleBeforeClosingBracket() {
        let quoted = c([e("「", "「", 1285), e("迷う", "マヨウ", 802), e("値", "ネ", 1285), e("」", "」", 1285)])
        XCTAssertEqual(sentenceFinalParticleFixed(quoted, context: "", atSentenceEnd: false, particle: particle)?.text, "「迷うね」")
        let middle = c([e("迷う", "マヨウ", 802), e("値", "ネ", 1285), e("」", "」", 1285), e("と", "ト", 261)])
        XCTAssertEqual(sentenceFinalParticleFixed(middle, context: "「", atSentenceEnd: false, particle: particle)?.text, "迷うね」と")
    }

    func testNotBeforeCommaOrOpeningBracket() {
        let comma = c([e("迷う", "マヨウ", 802), e("値", "ネ", 1285), e("、", "、", 1285)])
        XCTAssertNil(sentenceFinalParticleFixed(comma, context: "", atSentenceEnd: false, particle: particle))
        let opening = c([e("迷う", "マヨウ", 802), e("値", "ネ", 1285), e("（", "（", 1285), e("約", "ヤク", 1285)])
        XCTAssertNil(sentenceFinalParticleFixed(opening, context: "", atSentenceEnd: false, particle: particle))
        // 文の終わりの記号の前でも、すぐ前が終止形でなければ替えない (迷い|値」)
        let continuative = c([e("迷い", "マヨイ", 812), e("値", "ネ", 1285), e("」", "」", 1285)])
        XCTAssertNil(sentenceFinalParticleFixed(continuative, context: "", atSentenceEnd: false, particle: particle))
    }
}
