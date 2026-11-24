import XCTest
import KanaKanjiConverterModule

/// 2026-11-06 (まとめ役 09:57): z ＋記号の近道 (Mozc と同じ)。IME 本体は記号を全角にして渡す (. → 。 , → 、 / → ・ - → ー [ → 「 ] → 」)
final class ZShortcutTests: XCTestCase {
    func typed(_ keys: [String]) -> String {
        var text = ComposingText()
        for key in keys { text.insertAtCursorPosition(key, inputStyle: .roman2kana) }
        return text.convertTarget
    }

    func testZAndSymbolBecomeTheMark() {
        XCTAssertEqual(typed(["z", "。"]), "…")
        XCTAssertEqual(typed(["z", "、"]), "‥")
        XCTAssertEqual(typed(["z", "・"]), "・")
        XCTAssertEqual(typed(["z", "ー"]), "〜")
        XCTAssertEqual(typed(["z", "「"]), "『")
        XCTAssertEqual(typed(["z", "」"]), "』")
        XCTAssertEqual(typed(["k", "o", "r", "e", "z", "。"]), "これ…")
        XCTAssertEqual(typed(["z", "。", "z", "。"]), "……")
    }

    func testOtherZSpellingsAreUnchanged() {
        XCTAssertEqual(typed(["z", "u"]), "ず")
        XCTAssertEqual(typed(["z", "e", "h", "i"]), "ぜひ")
        XCTAssertEqual(typed(["z", "h"]), "←")
        // z のあとでない記号は今までどおり
        XCTAssertEqual(typed(["k", "a", "。"]), "か。")
    }
}
