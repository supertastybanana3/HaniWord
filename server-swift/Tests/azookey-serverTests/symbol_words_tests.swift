import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

// 2026-10-20: かなの読みに対する記号だけの語 (カッコ→()) は、覚えていなければ自動の変換に選ばず、候補一覧では後ろへ
final class SymbolWordsTests: XCTestCase {
    func testSymbolOnlyWords() {
        for word in ["()", "(?)", "(^^)", "×", "→", "☆☆☆", "※", "〒", "・", "〜", "…", "〇", "〇〇", "「」"] {
            XCTAssertTrue(isSymbolOnlyWord(word), word)
        }
        for word in ["括弧", "カッコ", "3×2", "らき☆すた", "H₂O", "2026/10/20", "〇時", "二〇二六", "😭", "ございます😭", "ー", "AI"] {
            XCTAssertFalse(isSymbolOnlyWord(word), word)
        }
    }

    func testOnlyLearnedSymbolWordsMayBeChosenAutomatically() {
        let unlearned = DicdataElement(word: "()", ruby: "カッコ", cid: CIDData.一般名詞.cid, mid: MIDData.一般.mid, value: -6.91)
        var learned = unlearned
        learned.metadata = .isLearned
        XCTAssertTrue(isPictographElement(unlearned))
        XCTAssertFalse(isPictographElement(learned))
        XCTAssertTrue(isSymbolOnlyElement(learned))
        // 読みが記号そのものの語 (打った記号) と、ふつうの語は対象外
        XCTAssertFalse(isPictographElement(DicdataElement(word: "(", ruby: "(", cid: CIDData.一般名詞.cid, mid: MIDData.一般.mid, value: -5)))
        XCTAssertFalse(isPictographElement(DicdataElement(word: "括弧", ruby: "カッコ", cid: CIDData.一般名詞.cid, mid: MIDData.一般.mid, value: -9.82)))
        // 絵文字は覚えていても今までどおり避ける
        var emoji = DicdataElement(word: "😭", ruby: "ナキガオ", cid: CIDData.一般名詞.cid, mid: MIDData.一般.mid, value: -8)
        emoji.metadata = .isLearned
        XCTAssertTrue(isPictographElement(emoji))
    }
}
