import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-06 (仕事 36): かなで打った数詞を漢数字の形で先に出す設定 (counter_numerals.swift)
final class CounterNumeralsTests: XCTestCase {
    func testKanjiNumeral() {
        let cases: [(String, String?)] = [
            ("1", "一"), ("10", "十"), ("11", "十一"), ("20", "二十"), ("23", "二十三"), ("100", "百"), ("101", "百一"), ("110", "百十"),
            ("1000", "千"), ("1980", "千九百八十"), ("2024", "二千二十四"), ("10000", "一万"), ("10010", "一万十"), ("100000000", "一億"),
            ("120000", "十二万"), ("100000001", "一億一"), ("0", "〇"), ("007", nil), ("", nil), ("12345678901234567", nil),
        ]
        for (digits, expected) in cases {
            XCTAssertEqual(kanjiNumeral(digits), expected, digits)
        }
    }

    func testKanjifyingWords() {
        let cases: [(String, String?)] = [
            ("1回", "一回"), ("第1回", "第一回"), ("23人", "二十三人"), ("3時15分", "三時十五分"), ("2回目", "二回目"), ("1人暮らし", "一人暮らし"),
            ("600", "六百"), ("1980年", "一九八〇年"), ("2024年度", "二〇二四年度"), ("2000年", "二千年"), ("1900年", "千九百年"),
            ("794年", "七百九十四年"), ("1980円", "千九百八十円"), ("4K", nil), ("5G", nil), ("3.5", nil), ("50%", nil), ("1/3", nil), ("1,000円", nil), ("一緒", nil), ("ｘ1", nil),
        ]
        for (word, expected) in cases {
            XCTAssertEqual(kanjifyingNumerals(word), expected, word)
        }
        XCTAssertTrue(isKanaOnlyReading("いっかい") && isKanaOnlyReading("イッカイ") && isKanaOnlyReading("にじゅうさんにん"))
        XCTAssertFalse(isKanaOnlyReading("1かい") || isKanaOnlyReading("1カイ") || isKanaOnlyReading("") || isKanaOnlyReading("ｇかい"))
    }

    func testElements() {
        func element(_ word: String, _ ruby: String) -> DicdataElement {
            DicdataElement(word: word, ruby: ruby, lcid: 1288, rcid: 1288, mid: 0, value: -10)
        }
        let data = [element("今日", "キョウ"), element("は", "ハ"), element("3回目", "サンカイメ"), element("です", "デス")]
        XCTAssertEqual(kanjifyingNumeralElements(data, kept: [])?.map(\.word), ["今日", "は", "三回目", "です"])
        XCTAssertNil(kanjifyingNumeralElements(data, kept: ["3回目"]))
        // 数字のキーで打った数字 (読みに数字がある語) は変えない
        XCTAssertNil(kanjifyingNumeralElements([element("1回", "1カイ")], kept: []))
        XCTAssertNil(kanjifyingNumeralElements([element("一緒に", "イッショニ")], kept: []))
    }

    func testList() {
        let list = ["1回", "1階", "一階", "一介", "一回"]
        XCTAssertEqual(placingKanjiNumerals(list, reading: "いっかい", enabled: true, kept: []), ["一回", "1回", "1階", "一階", "一介"])
        XCTAssertEqual(placingKanjiNumerals(["23人", "二十三人"], reading: "にじゅうさんにん", enabled: true, kept: []), ["二十三人", "23人"])
        XCTAssertEqual(placingKanjiNumerals(["23人"], reading: "にじゅうさんにん", enabled: true, kept: []), ["二十三人", "23人"])
        XCTAssertEqual(placingKanjiNumerals(list, reading: "いっかい", enabled: false, kept: []), list)
        XCTAssertEqual(placingKanjiNumerals(list, reading: "1かい", enabled: true, kept: []), list)
        XCTAssertEqual(placingKanjiNumerals(list, reading: "いっかい", enabled: true, kept: ["1回"]), list)
        XCTAssertEqual(placingKanjiNumerals(["一番", "1番"], reading: "いちばん", enabled: true, kept: []), ["一番", "1番"])
        XCTAssertEqual(placingKanjiNumerals(["5G", "五G"], reading: "ふぁいぶじー", enabled: true, kept: []), ["5G", "五G"])
        // 学習した語などが 1 番目にあれば、その後ろの算用数字の形のすぐ前に。漢数字の形がもう前にあれば変えない
        XCTAssertEqual(placingKanjiNumerals(["一介", "1回", "1階", "一回"], reading: "いっかい", enabled: true, kept: []), ["一介", "一回", "1回", "1階"])
        XCTAssertEqual(placingKanjiNumerals(["一回", "1回"], reading: "いっかい", enabled: true, kept: []), ["一回", "1回"])
    }

    func testNotingChoices() {
        let arabic = [DicdataElement(word: "1回", ruby: "イッカイ", lcid: 1288, rcid: 1288, mid: 0, value: -10)]
        let kanji = [DicdataElement(word: "一回", ruby: "イッカイ", lcid: 1295, rcid: 1300, mid: 0, value: -14)]
        // 一覧で 1回 を選び直した → 覚える。今の表示のまま確定した → 数えない
        XCTAssertEqual(notingCounterNumerals([], data: arabic, text: "1回", defaultText: "一回"), ["1回"])
        XCTAssertEqual(notingCounterNumerals([], data: arabic, text: "1回", defaultText: "1回"), [])
        // 覚えたあと、一覧で 一回 を選び直した → 記録を消す
        XCTAssertEqual(notingCounterNumerals(["1回", "2人"], data: kanji, text: "一回", defaultText: "1回"), ["2人"])
        XCTAssertEqual(parseCounterNumeralsKept(formatCounterNumeralsKept(["1回", "23人"])), ["1回", "23人"])
        XCTAssertEqual(parseCounterNumeralsKept("# 説明\n1回\n4K\n\n"), ["1回"])
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ: "kanji" で、かなで打った数詞が漢数字になり、一覧の 2 番目に算用数字の形が来る。
    /// 数字のキーで打った数字と、もともと漢字の語は変えない。"arabic" なら今のまま
    @MainActor
    func testRealReadings() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        let saved = config["counterNumerals"]
        let savedKept = counterNumeralsKeptCache
        defer { config["counterNumerals"] = saved; counterNumeralsKeptCache = savedKept; clearPreviewMemo() }
        counterNumeralsKeptCache = []
        func run(_ reading: String) -> (String, [String]) {
            clearPreviewMemo()
            var count: Int32 = 0
            let converted = reading.withCString { r in "".withCString { c in
                convert_segments(hiraganaPtr: r, contextPtr: c, fast: false, dictionaryOnly: true, lengthPtr: &count)
            } }
            let text = (0..<Int(count)).compactMap { i in converted[i].map { String(cString: $0.pointee.text) } }.joined()
            var listCount: Int32 = 0
            let listed = reading.withCString { r in "".withCString { c in
                list_candidates(hiraganaPtr: r, contextPtr: c, lengthPtr: &listCount)
            } }
            return (text, (0..<Int(listCount)).compactMap { i in listed[i].map { String(cString: $0.pointee.text) } })
        }
        let cases: [(reading: String, arabic: String, kanji: String)] = [
            ("いっかい", "1回", "一回"), ("ふたり", "2人", "二人"), ("にどめ", "2度目", "二度目"), ("さんこ", "3個", "三個"),
            ("にじゅうさんにん", "23人", "二十三人"), ("みっか", "3日", "三日"), ("だいいっかい", "第1回", "第一回"),
            ("さんねんまえ", "3年前", "三年前"), ("きょうはさんかいめです", "今日は3回目です", "今日は三回目です"),
            ("せんきゅうひゃくはちじゅうねん", "1980年", "一九八〇年"),
            ("1かい", "1回", "1回"), ("いっしょに", "一緒に", "一緒に"), ("いちぶ", "一部", "一部"),
        ]
        for c in cases {
            config["counterNumerals"] = "arabic"
            let (offText, offList) = run(c.reading)
            config["counterNumerals"] = "kanji"
            let (text, list) = run(c.reading)
            print("numerals \(c.reading) arabic=\(offText) \(offList.prefix(3)) kanji=\(text) \(list.prefix(3))")
            XCTAssertEqual(offText, c.arabic, c.reading)
            XCTAssertEqual(text, c.kanji, c.reading)
            // 一覧の 1 番目は、同じ実行の前の試験 (学習・AI の置き場) で変わることがあるので、漢数字の形が算用数字の形のすぐ前にあるかで見る
            if c.arabic != c.kanji && !c.reading.contains("は") { XCTAssertTrue(list.contains(c.arabic), c.reading) }
            if c.arabic != c.kanji && !c.reading.contains("は"), let at = list.firstIndex(of: c.arabic) {
                XCTAssertEqual(at > 0 ? list[at - 1] : nil, c.kanji, "\(c.reading) \(list.prefix(4))")
            }
        }
    }
}
