import Foundation
import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

final class AdjectivalNaTests: XCTestCase {
    func e(_ word: String, _ ruby: String, _ lcid: Int, _ rcid: Int) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: lcid, rcid: rcid, mid: 501, value: -10)
    }

    func testNaFollowedByAParticleOrPunctuation() {
        // まずこうかなとおもう を AI が選んだ形 (変換サーバーの書き出しのとおりの品詞)
        XCTAssertTrue(hasDanglingAdjectivalNa([e("まず", "マズ", 1281, 1281), e("高価な", "コウカナ", 1287, 454), e("と思う", "トオモウ", 164, 817)]))
        // 語幹と な が別の語・句読点
        XCTAssertTrue(hasDanglingAdjectivalNa([e("無理", "ムリ", 1287, 1287), e("な", "ナ", 454, 454), e("が", "ガ", 160, 160)]))
        XCTAssertTrue(hasDanglingAdjectivalNa([e("静かな", "シズカナ", 1287, 454), e("。", "。", 5, 5)]))
    }

    func testNaFollowedByOtherWordsIsKept() {
        // 辞書だけの変換 (正しい)・の ので のに ん・名詞・副助詞・文の終わり
        XCTAssertFalse(hasDanglingAdjectivalNa([e("まず", "マズ", 1281, 1281), e("こう", "コウ", 1282, 1282), e("かなと思う", "カナトオモウ", 359, 817)]))
        XCTAssertFalse(hasDanglingAdjectivalNa([e("静かな", "シズカナ", 1287, 454), e("のがいい", "ノガイイ", 1310, 37)]))
        XCTAssertFalse(hasDanglingAdjectivalNa([e("無理", "ムリ", 1287, 1287), e("な", "ナ", 454, 454), e("ので", "ノデ", 316, 316)]))
        XCTAssertFalse(hasDanglingAdjectivalNa([e("大変な", "タイヘンナ", 1287, 454), e("ことになった", "コトニナッタ", 1310, 435)]))
        XCTAssertFalse(hasDanglingAdjectivalNa([e("大変な", "タイヘンナ", 1287, 454), e("くらい", "クライ", 200, 200)]))
        XCTAssertFalse(hasDanglingAdjectivalNa([e("静かな", "シズカナ", 1287, 454)]))
        // 助詞の字のあとが小書きの語 (にゃ) は、助詞の に でない
        XCTAssertFalse(hasDanglingAdjectivalNa([e("必要な", "ヒツヨウナ", 1287, 454), e("にゃ", "ニャ", 275, 275)]))
        // 語幹でない語のあとの な (名詞＋な) は見ない
        XCTAssertFalse(hasDanglingAdjectivalNa([e("効果", "コウカ", 1285, 1285), e("な", "ナ", 454, 454), e("と思う", "トオモウ", 164, 817)]))
    }

    func testSentenceFinalKanaAndAdjectivalNaEndings() {
        XCTAssertTrue(endsWithAdjectivalNa([e("まず", "マズ", 1281, 1281), e("高価な", "コウカナ", 1287, 454)]))
        XCTAssertTrue(endsWithAdjectivalNa([e("無理", "ムリ", 1287, 1287), e("な", "ナ", 454, 454)]))
        XCTAssertFalse(endsWithAdjectivalNa([e("高価な", "コウカナ", 1287, 454), e("と", "ト", 164, 164)]))
        XCTAssertFalse(endsWithAdjectivalNa([e("効果", "コウカ", 1285, 1285), e("な", "ナ", 454, 454)]))
        // 辞書の書き出しのとおり: 静か|か|な・これで|いいかな
        XCTAssertTrue(endsWithFinalKana([e("静か", "シズカ", 1287, 1287), e("か", "カ", 359, 359), e("な", "ナ", 275, 275)]))
        XCTAssertTrue(endsWithFinalKana([e("これで", "コレデ", 1306, 458), e("いいかな", "イイカナ", 131, 275)]))
        XCTAssertFalse(endsWithFinalKana([e("綺麗", "キレイ", 1287, 1287), e("な", "ナ", 275, 275)]))
        // 名詞の かな (仮名) は終助詞でない
        XCTAssertFalse(endsWithFinalKana([e("仮名", "カナ", 1285, 1285)]))
        XCTAssertFalse(endsWithFinalKana([e("かな", "カナ", 1285, 1285)]))
    }
}
