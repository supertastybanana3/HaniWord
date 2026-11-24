import KanaKanjiConverterModule
import XCTest
@testable import azookey_server

/// 2026-11-06: くだけた一言 (casual.swift・casual_words.tsv)
@MainActor
final class CasualWordsTests: XCTestCase {
    func testTableKeepsTheOrderOfForms() {
        let table = parseCasualWords("# 説明\nだる\tだる|ダル\t手持ち\nどんま\tドンマ\t手持ち\n壊れた行\n\tだる\tx\n")
        XCTAssertEqual(table["だる"], ["だる", "ダル"])
        XCTAssertEqual(table["どんま"], ["ドンマ"])
        XCTAssertEqual(table.count, 2)
    }

    func testWholeReadingGivesTheFirstFormUnlessARegisteredWordExists() {
        let saved = (casualWordsTable, appliedUserWordsByRuby)
        defer { casualWordsTable = saved.0; appliedUserWordsByRuby = saved.1 }
        casualWordsTable = parseCasualWords("だる\tだる|ダル\t手持ち\nりょ\tりょ\t手持ち\n")
        appliedUserWordsByRuby = [:]
        XCTAssertEqual(casualWordForms("だる"), ["だる", "ダル"])
        XCTAssertEqual(casualWordForms("りょ"), ["りょ"])
        // 読みの一部 (だるい) や表に無い読みは今までどおり
        XCTAssertNil(casualWordForms("だるい"))
        XCTAssertNil(casualWordForms("ねこ"))
        // 読み全体が登録した語なら、登録した語を先にする
        appliedUserWordsByRuby = ["リョ": [DicdataElement(word: "了", ruby: "リョ", lcid: 1285, rcid: 1285, mid: 501, value: -7)]]
        XCTAssertNil(casualWordForms("りょ"))
    }
}
