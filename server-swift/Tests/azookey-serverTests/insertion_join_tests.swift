import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-23: 文の途中に差し込んだ短い読みの、辞書だけの変換 (insertion_join.swift)
final class InsertionJoinTests: XCTestCase {
    func testTrailingKanaRun() {
        XCTAssertEqual(trailingKanaRun("今日はとも")?.text, "はとも")
        XCTAssertEqual(trailingKanaRun("今日はとも")?.script, .hiragana)
        XCTAssertEqual(trailingKanaRun("これはバナ")?.text, "バナ")
        XCTAssertEqual(trailingKanaRun("これはバナ")?.script, .katakana)
        XCTAssertEqual(trailingKanaRun("スーパー")?.text, "スーパー")
        XCTAssertEqual(trailingKanaRun("あいうえおかきくけこ")?.text, "うえおかきくけこ")
        XCTAssertNil(trailingKanaRun(""))
        XCTAssertNil(trailingKanaRun("学校"))
        XCTAssertNil(trailingKanaRun("終わり。"))
        XCTAssertEqual(katakanaToHiraganaText("バナナー"), "ばななー")
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ: かなで書いた語の途中に字を足すと、足した所は左と同じ字の種類のかなのまま。
    /// 助詞のあとに語を差し込む形は左を入れて引いた変換、助詞と読みがつながって別の語になる形 (から + てを の 空手) はつなげない
    @MainActor
    func testInsertedReadingStaysKanaInsideAKanaWord() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        func inserted(_ reading: String, _ left: String, _ right: String = "") -> [String]? {
            rightContextText = right
            rightContextReading = reading
            defer { rightContextText = ""; rightContextReading = "" }
            return insertedKanaSegments(hiragana: reading, context: left, fast: true)?.map(\.word)
        }
        // ひらがなの語の途中 (左のかなが別の字になる): 読みはかなのまま
        XCTAssertEqual(inserted("め", "あしたはあ", "がふるらしい"), ["め"])
        XCTAssertEqual(inserted("へん", "それはたい", "だった"), ["へん"])
        // カタカナの語の途中: 右もカタカナならカタカナ、右がひらがなでも、つなげてカタカナの語になればカタカナ
        XCTAssertEqual(inserted("ぼ", "マグナ", "ックス"), ["ボ"])
        XCTAssertEqual(inserted("な", "これはバナ", "だ"), ["ナ"])
        // 今までどおり: 右が漢字・左のかなが 1 字・読みが長い・つなげても後ろに漢字が出る・助詞と読みがつながる・左がかなでない
        XCTAssertNil(inserted("め", "あしたはあ", "雨"))
        XCTAssertNil(inserted("ごく", "競技も", "わずか"))
        XCTAssertNil(inserted("だんかいに", "２つの", ""))
        XCTAssertNil(inserted("ときに", "この", "はじめて"))
        XCTAssertNil(inserted("てを", "鍵から", "はなす"))
        XCTAssertNil(inserted("だち", "今日は友", "と"))
        XCTAssertNil(inserted("だち", ""))
    }
}
