import Foundation
import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

final class CountersTests: XCTestCase {
    func e(_ word: String, _ ruby: String, _ cid: Int) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: cid, rcid: cid, mid: 501, value: -10)
    }

    let counters: (String) -> [String] = { ruby in ruby == "チョウ" ? ["丁", "挺", "張", "梃"] : [] }

    func testCounterPhraseGetsTheOtherCountersOfTheSameReading() {
        // 二|丁 (変換サーバーの書き出しのとおり 数 1295・助数詞 1300) → 二挺 二張 二梃 (同じ読みの助数詞の順)
        XCTAssertEqual(counterAlternatives([e("二", "ニ", 1295), e("丁", "チョウ", 1300)], counters: counters), ["二挺", "二張", "二梃"])
        XCTAssertEqual(counterAlternatives([e("二", "ニ", 1295), e("十", "ジュウ", 1295), e("丁", "チョウ", 1300)], counters: counters),
                       ["二十挺", "二十張", "二十梃"])
    }

    func testWholeWordNumeralAndCounter() {
        // 辞書に 1 語で載る 一丁 (イッチョウ。品詞 1314) も、漢数字＋同じ読みの助数詞 なら同じに
        XCTAssertEqual(counterAlternatives([e("一丁", "イッチョウ", 1314)], counters: counters), ["一挺", "一張", "一梃"])
        // 漢数字でない語 (一聴 の 聴 は助数詞でない・伍長)
        XCTAssertEqual(counterAlternatives([e("一聴", "イッチョウ", 1285)], counters: counters), [])
        XCTAssertEqual(counterAlternatives([e("伍長", "ゴチョウ", 1285)], counters: counters), [])
    }

    func testOtherWordSequencesAreLeftAlone() {
        // 数詞だけ・助数詞のあとに語が続く・数詞でない語 (二|長 の 長 は 1298)・助数詞だけ
        XCTAssertEqual(counterAlternatives([e("二", "ニ", 1295)], counters: counters), [])
        XCTAssertEqual(counterAlternatives([e("二", "ニ", 1295), e("丁", "チョウ", 1300), e("の", "ノ", 368)], counters: counters), [])
        XCTAssertEqual(counterAlternatives([e("二", "ニ", 1295), e("長", "チョウ", 1298)], counters: counters), [])
        XCTAssertEqual(counterAlternatives([e("拳銃", "ケンジュウ", 1285), e("丁", "チョウ", 1300)], counters: counters), [])
        XCTAssertEqual(counterAlternatives([e("丁", "チョウ", 1300)], counters: counters), [])
        // 数の字でない語 (辞書で品詞が数の ○)・頭が 万 億 兆 の数
        XCTAssertEqual(counterAlternatives([e("○", "マル", 1295), e("丁", "チョウ", 1300)], counters: counters), [])
        XCTAssertEqual(counterAlternatives([e("兆", "チョウ", 1295), e("丁", "チョウ", 1300)], counters: counters), [])
        XCTAssertEqual(counterAlternatives([e("2", "ニ", 1295), e("丁", "チョウ", 1300)], counters: counters), ["2挺", "2張", "2梃"])
    }

    func testPublishedCounterCounts() throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        var counts: [String: Int] = [:]
        for row in BoundedTable.rows(root.appendingPathComponent("counter_counts.tsv"), maxBytes: 1 << 20, maxLines: 20_000, columns: 2...2, maxFieldLength: 16) {
            counts[row[0]] = Int(row[1])
        }
        XCTAssertGreaterThan(counts.count, 500)
        // 挺 は足し、旧字体・異体字 (罐・疊・點) は足さない線 (counterMinCount)
        XCTAssertGreaterThanOrEqual(counts["挺"] ?? 0, counterMinCount)
        for old in ["罐", "疊", "點", "卷"] { XCTAssertLessThan(counts[old] ?? 0, counterMinCount, old) }
    }
}
