import XCTest
@testable import azookey_server

/// 2026-11-06 (問いの か): 「…か？」の ？ を切らずに渡す形と、切った形との選び方 (question_ka.swift)
final class QuestionKaTests: XCTestCase {
    func testMarksAreOnlyAfterKaWithoutOtherSymbols() {
        XCTAssertEqual(questionKaMarks("びぶんか？"), "？")
        XCTAssertEqual(questionKaMarks("びぶんか?？"), "?？")
        XCTAssertNil(questionKaMarks("か？"))
        XCTAssertNil(questionKaMarks("びぶん？"))
        XCTAssertNil(questionKaMarks("びぶんか"))
        XCTAssertNil(questionKaMarks("それ、びぶんか？"))
    }

    func testWholeIsTakenWhenOnlyTheLastKanjiBecomesKa() {
        let never: (String, String) -> Bool = { _, _ in false }
        // 変換の文節は 積分｜か？ の形 (か は ？ と一緒)。返す文節は 積分か｜？ に組み直す (読みの長さも)
        XCTAssertEqual(choosingQuestionKa(split: [("積分家", 5), ("？", 1)], whole: [("積分", 4), ("か？", 2)],
                                          reading: "せきぶんか？", marks: "？", single: never).map(\.word), ["積分か", "？"])
        XCTAssertEqual(choosingQuestionKa(split: [("これ", 2), ("は", 1), ("正解化", 5), ("？", 1)], whole: [("これ", 2), ("は", 1), ("正解", 4), ("か？", 2)],
                                          reading: "これはせいかいか？", marks: "？", single: never).map(\.word), ["これ", "は", "正解か", "？"])
        XCTAssertEqual(choosingQuestionKa(split: [("正解化", 5), ("？", 1)], whole: [("正解か", 5), ("？", 1)],
                                          reading: "せいかいか？", marks: "？", single: never).map(\.word), ["正解か", "？"])
        XCTAssertEqual(choosingQuestionKa(split: [("積分家", 5), ("？", 1)], whole: [("積分", 4), ("か？", 2)],
                                          reading: "せきぶんか？", marks: "？", single: never).map(\.rubyCount), [5, 1])
        XCTAssertEqual(choosingQuestionKa(split: [("これ", 2), ("は", 1), ("正解化", 5), ("？", 1)], whole: [("これ", 2), ("は", 1), ("正解", 4), ("か？", 2)],
                                          reading: "これはせいかいか？", marks: "？", single: never).map(\.rubyCount), [2, 1, 5, 1])
        // 替えたときは、替えた文節の読みと もとの形 を返す (一覧の 2 番目に残すため)
        let noted = choosingQuestionKaNoting(split: [("これ", 2), ("は", 1), ("正解化", 5), ("？", 1)], whole: [("これ", 2), ("は", 1), ("正解", 4), ("か？", 2)],
                                             reading: "これはせいかいか？", marks: "？", single: never)
        XCTAssertEqual(noted.original?.reading, "せいかいか")
        XCTAssertEqual(noted.original?.text, "正解化")
        XCTAssertNil(choosingQuestionKaNoting(split: [("低評価", 5), ("？", 1)], whole: [("定評", 4), ("か？", 2)],
                                              reading: "ていひょうか？", marks: "？", single: never).original)
        // 名詞＋漢字 1 字 を調べるときは、その読み (せきぶんか) と表記 (積分家)
        var asked: [String] = []
        _ = choosingQuestionKa(split: [("積分家", 5), ("？", 1)], whole: [("積分", 4), ("か？", 2)], reading: "せきぶんか？", marks: "？",
                               single: { asked.append("\($0)/\($1)"); return false })
        XCTAssertEqual(asked, ["せきぶんか/積分家"])
    }

    func testReSegmentedFormsAreKept() {
        let never: (String, String) -> Bool = { _, _ in false }
        // 切り方ごと替わる形 (低評価？ → 定評か？・微＋文化？ → 微分か？) は替えない
        XCTAssertEqual(choosingQuestionKa(split: [("低評価", 5), ("？", 1)], whole: [("定評", 4), ("か？", 2)],
                                          reading: "ていひょうか？", marks: "？", single: never).map(\.word), ["低評価", "？"])
        XCTAssertEqual(choosingQuestionKa(split: [("微", 1), ("文化", 3), ("？", 1)], whole: [("微分", 3), ("か？", 2)],
                                          reading: "びぶんか？", marks: "？", single: never).map(\.word), ["微", "文化", "？"])
    }

    func testSingleDictionaryWordsAreNotSplit() {
        // 進化？ (1 語) は 新か？ にしない。数学科？ も同じ
        let single: (String, String) -> Bool = { reading, text in (reading, text) == ("しんか", "進化") || (reading, text) == ("すうがくか", "数学科") }
        XCTAssertEqual(choosingQuestionKa(split: [("進化", 3), ("？", 1)], whole: [("新か", 3), ("？", 1)],
                                          reading: "しんか？", marks: "？", single: single).map(\.word), ["進化", "？"])
        XCTAssertEqual(choosingQuestionKa(split: [("数学科", 5), ("？", 1)], whole: [("数学か", 5), ("？", 1)],
                                          reading: "すうがくか？", marks: "？", single: single).map(\.word), ["数学科", "？"])
    }

    func testOtherDifferencesKeepTheSplitForm() {
        let never: (String, String) -> Bool = { _, _ in false }
        // 切った形がもう か で終わる
        XCTAssertEqual(choosingQuestionKa(split: [("本当か", 5), ("？", 1)], whole: [("ほんとうか", 5), ("？", 1)],
                                          reading: "ほんとうか？", marks: "？", single: never).map(\.word), ["本当か", "？"])
        // ？ ごとの形が か＋？ で終わらない
        XCTAssertEqual(choosingQuestionKa(split: [("積分家", 5), ("？", 1)], whole: [("積分化？", 6)],
                                          reading: "せきぶんか？", marks: "？", single: never).map(\.word), ["積分家", "？"])
    }

    func testQuestionAfterKaIsPassedWithItsPart() {
        var calls: [String] = []
        let convert: (String, String) -> [(word: String, rubyCount: Int)] = { reading, _ in
            calls.append(reading)
            return [(word: reading, rubyCount: reading.count)]
        }
        let out = preservingLiteralSymbols(reading: "えっと、びぶんか？？それ", context: "", attachQuestionKa: true, convert: convert)
        XCTAssertEqual(calls, ["えっと", "びぶんか？？", "それ"])
        XCTAssertEqual(out?.map(\.word).joined(), "えっと、びぶんか？？それ")
        calls = []
        _ = preservingLiteralSymbols(reading: "えっと、びぶんか？それ", context: "", convert: convert)
        XCTAssertEqual(calls, ["えっと", "びぶんか", "それ"])
        // か の後ろでない ？ は今までどおり切る
        calls = []
        _ = preservingLiteralSymbols(reading: "なに？それ", context: "", attachQuestionKa: true, convert: convert)
        XCTAssertEqual(calls, ["なに", "それ"])
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE、表もそろえたもの) があるときだけ: 替えた形の文節 (長野か) の一覧で、IME 本体と同じに見えている形を
    /// 一番上に入れ直すと、もとの形 (長野家・全自動化) が 2 番目に来る (まとめ役 11-06 06:39 の条件: Space 1 回で戻れる)
    @MainActor
    func testOriginalFormIsSecondInTheList() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        for (reading, shown, original) in [("ながのか？", "長野か", "長野家"), ("ぜんじどうか？", "全自動か", "全自動化")] {
            var count: Int32 = 0
            let converted = reading.withCString { r in "".withCString { c in
                convert_segments(hiraganaPtr: r, contextPtr: c, fast: false, dictionaryOnly: true, lengthPtr: &count)
            } }
            let segments = (0..<Int(count)).compactMap { i in converted[i].map { String(cString: $0.pointee.text) } }
            XCTAssertEqual(segments, [shown, "？"], reading)
            let focusReading = String(reading.dropLast())
            var listCount: Int32 = 0
            let listed = focusReading.withCString { r in "".withCString { c in
                list_candidates(hiraganaPtr: r, contextPtr: c, lengthPtr: &listCount)
            } }
            var list = (0..<Int(listCount)).compactMap { i in listed[i].map { String(cString: $0.pointee.text) } }
            list.removeAll { $0 == shown }
            list.insert(shown, at: 0)
            XCTAssertEqual(Array(list.prefix(2)), [shown, original], "\(reading) \(list.prefix(4))")
        }
    }
}
