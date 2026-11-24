import Foundation
import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-06 (数字＋かい): 数字のすぐ後ろの語を助数詞の候補に替える形 (counters.swift の preferringCounterAfterDigit)
final class DigitCounterTests: XCTestCase {
    func e(_ word: String, _ ruby: String, _ cid: Int) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: cid, rcid: cid, mid: 501, value: -10)
    }
    func c(_ data: [DicdataElement], _ value: Float) -> Candidate {
        Candidate(text: data.map(\.word).joined(), value: value, correspondingCount: data.map(\.ruby).joined().count, lastMid: 501, data: data)
    }

    let usage = ["回": 549_722, "階": 84_487, "分": 360_236, "文": 4_919]

    func testNounAfterDigitBecomesTheCounterCandidate() {
        // 辞書の書き出しのとおり: 6会 (6 は名詞 1285、会 は 1298) が 6回 (6 は数 1295、回 は助数詞 1300) より上
        let kai = c([e("6", "6", 1285), e("会", "カイ", 1298)], -21.07)
        let results = [kai, c([e("6", "6", 1295), e("回", "カイ", 1300)], -21.86), c([e("6", "6", 1285), e("階", "カイ", 1298)], -23.83)]
        XCTAssertEqual(preferringCounterAfterDigit(kai, results: results, usage: usage)?.text, "6回")
        // 後ろが続いていても、前後の表記が同じなら (あと6会で終わる → あと6回で終わる。会で は 1 語にまとまることがある)
        let ato = c([e("あと", "アト", 1285), e("6", "6", 1285), e("会で", "カイデ", 1298), e("終わる", "オワル", 772)], -47.42)
        let atoCounter = c([e("あと", "アト", 1285), e("6", "6", 1295), e("回", "カイ", 1300), e("で", "デ", 149), e("終わる", "オワル", 772)], -47.96)
        XCTAssertEqual(preferringCounterAfterDigit(ato, results: [ato, atoCounter], usage: usage)?.text, "あと6回で終わる")
    }

    func testOtherFormsAreLeftAlone() {
        // 数字のあとが 1 語の名詞 (2会社): 同じ形の助数詞の候補が無い
        let company = c([e("2", "2", 1285), e("会社", "カイシャ", 1285)], -26.94)
        let other = c([e("2", "2", 1295), e("回", "カイ", 1300), e("者", "シャ", 1298)], -29.51)
        XCTAssertNil(preferringCounterAfterDigit(company, results: [company, other], usage: usage))
        // もう助数詞
        let counter = c([e("5", "5", 1295), e("回戦", "カイセン", 1300)], -26.05)
        XCTAssertNil(preferringCounterAfterDigit(counter, results: [counter], usage: usage))
        // 数字が無い
        let plain = c([e("会", "カイ", 1298)], -10)
        XCTAssertNil(preferringCounterAfterDigit(plain, results: [plain, c([e("回", "カイ", 1300)], -11)], usage: usage))
        // 元の語が数字のあとでよく使われる (10分の1 の 分 は 文 の 10 倍以上)、値の差が 2 より大きい
        let minute = c([e("10", "10", 1285), e("分", "ブン", 1298), e("の", "ノ", 368), e("1", "1", 1285)], -30)
        let mon = c([e("10", "10", 1295), e("文", "ブン", 1300), e("の", "ノ", 368), e("1", "1", 1285)], -30.5)
        XCTAssertNil(preferringCounterAfterDigit(minute, results: [minute, mon], usage: usage))
        let far = c([e("6", "6", 1295), e("回", "カイ", 1300)], -23.5)
        XCTAssertNil(preferringCounterAfterDigit(c([e("6", "6", 1285), e("会", "カイ", 1298)], -21.07), results: [far], usage: usage))
    }

    /// 2026-11-22 (数字＋がつ): 数字のすぐ後ろのカタカナだけの助数詞 (11|ガツ) を、同じ読みのいちばん使われる助数詞の字に替える
    func v(_ word: String, _ ruby: String, _ cid: Int, _ value: Float) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: cid, rcid: cid, mid: 501, value: value)
    }
    let tops: [String: (word: String, value: Float)] = ["ガツ": ("月", -13.21), "ページ": ("頁", -12.5), "メートル": ("米", -13.0), "シリング": ("志", -12.0), "カ": ("日", -8.0)]

    func testKatakanaCounterAfterDigitBecomesTheUsualCounter() {
        let top: (String) -> (word: String, value: Float)? = { self.tops[$0] }
        // 辞書の書き出しのとおり: 2023|年|11|ガツ|に|は (ガツ は助数詞 1300 のカタカナ、−15.33)
        let gatsu = c([v("2023", "2023", 1295, -14), v("年", "ネン", 1300, -5.42), v("11", "11", 1295, -14), v("ガツ", "ガツ", 1300, -15.33),
                       v("に", "ニ", 151, -1.38), v("は", "ハ", 261, -1.37)], -71.74)
        let fixed = replacingKatakanaCounterAfterDigit(gatsu, topCounter: top)
        XCTAssertEqual(fixed?.text, "2023年11月には")
        XCTAssertEqual(fixed?.data.map(\.lcid), gatsu.data.map(\.lcid))
        XCTAssertEqual(fixed?.data.map(\.ruby), gatsu.data.map(\.ruby))
        // 漢数字のあとも (十二|ガツ)
        XCTAssertEqual(replacingKatakanaCounterAfterDigit(c([v("十二", "12", 1295, -16), v("ガツ", "ガツ", 1300, -15.33)], -30), topCounter: top)?.text, "十二月")
    }

    func testOtherCountersAreLeftAlone() {
        let top: (String) -> (word: String, value: Float)? = { self.tops[$0] }
        // もう漢字の助数詞
        XCTAssertNil(replacingKatakanaCounterAfterDigit(c([v("11", "11", 1295, -14), v("月", "ガツ", 1300, -13.21)], -30), topCounter: top))
        // 数字のあとでない
        XCTAssertNil(replacingKatakanaCounterAfterDigit(c([v("ガツ", "ガツ", 1300, -15.33), v("ン", "ン", 1285, -10)], -30), topCounter: top))
        // 助数詞の品詞でない (名詞のカタカナ)
        XCTAssertNil(replacingKatakanaCounterAfterDigit(c([v("3", "3", 1285, -10), v("ガツ", "ガツ", 1285, -15.33)], -30), topCounter: top))
        // カタカナで使われる助数詞: 替える先の辞書の値のほうが低い (C の見直し: ページ −7.4・メートル −6.4・シリング)
        XCTAssertNil(replacingKatakanaCounterAfterDigit(c([v("3", "3", 1295, -10), v("ページ", "ページ", 1300, -7.4)], -30), topCounter: top))
        XCTAssertNil(replacingKatakanaCounterAfterDigit(c([v("3", "3", 1295, -10), v("メートル", "メートル", 1300, -6.4)], -30), topCounter: top))
        XCTAssertNil(replacingKatakanaCounterAfterDigit(c([v("5", "5", 1295, -10), v("シリング", "シリング", 1300, -11.5)], -30), topCounter: top))
        // 1 字のカタカナ (3カ所 が カ＋所 に分かれた形) は替えない
        XCTAssertNil(replacingKatakanaCounterAfterDigit(c([v("3", "3", 1295, -10), v("カ", "カ", 1300, -12), v("所", "ショ", 1300, -9)], -30), topCounter: top))
        // 同じ読みの助数詞が表に無い
        XCTAssertNil(replacingKatakanaCounterAfterDigit(c([v("3", "3", 1295, -10), v("ダース", "ダース", 1300, -12)], -30), topCounter: top))
        // 3|が|ツ (助詞のあとの埋め草) はこの直しの外
        XCTAssertNil(replacingKatakanaCounterAfterDigit(c([v("3", "3", 1285, -10), v("が", "ガ", 148, -1.46), v("ツ", "ツ", 1285, -10)], -30), topCounter: top))
    }

    /// 替える先は、同じ読みの漢字の助数詞 (回数 1,000 以上) が 1 つだけのときだけ (C の見直し: ショク は 色・食 の 2 つ)
    func testUniqueKanjiCounter() {
        let usage = ["月": 1_200_000, "色": 30_000, "食": 2_400, "段": 50_000, "弾": 60_000, "団": 3_000, "ヶ月": 900, "本": 400_000, "冊": 999]
        XCTAssertEqual(uniqueKanjiCounter(["月": -13.21, "ガツ": -15.33], usage: usage)?.word, "月")
        XCTAssertEqual(uniqueKanjiCounter(["本": -9, "冊": -10], usage: usage)?.word, "本")  // 冊 は 1,000 回に足りない
        XCTAssertNil(uniqueKanjiCounter(["色": -11, "食": -12], usage: usage))
        XCTAssertNil(uniqueKanjiCounter(["段": -9, "弾": -12, "団": -12], usage: usage))
        XCTAssertNil(uniqueKanjiCounter(["ヶ月": -10], usage: usage))
        XCTAssertNil(uniqueKanjiCounter([:], usage: usage))
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ: 辞書だけの変換で、年 のあとの 数字＋がつ が カタカナの ガツ でなく 月 になる
    @MainActor
    func testKatakanaCounterInRealConversions() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        func convert(_ reading: String, fast: Bool) -> String {
            var count: Int32 = 0
            let converted = reading.withCString { r in "".withCString { c in
                convert_segments(hiraganaPtr: r, contextPtr: c, fast: fast, dictionaryOnly: true, lengthPtr: &count)
            } }
            return (0..<Int(count)).compactMap { i in converted[i].map { String(cString: $0.pointee.text) } }.joined()
        }
        // 本物の表と辞書で: カタカナで使われる助数詞・1 字のカタカナ・か月 は、辞書の最良の候補に当てても替えない (C の見直し)
        for reading in ["3ぺーじ", "3めーとる", "50ぱーせんと", "3かしょ", "2かげつ", "5しりんぐ", "3きろ", "10どる", "3ぐらむ", "2りっとる", "4てん",
                        "1にち3しょく", "3しょく", "3だん", "3かん", "2たま", "3よう", "3がた"] {
            let composing = directComposingText(reading)
            let best = converter.requestCandidates(composing, options: getOptions(context: "", prediction: false, zenzai: false))
                .mainResults.first(where: { $0.correspondingCount == composing.input.count })
            XCTAssertNotNil(best, reading)
            if let best {
                XCTAssertNil(replacingKatakanaCounterAfterDigit(best, topCounter: { bestSameReadingCounter($0, usage: counterCounts()) }), "\(reading) \(best.text)")
                // 一覧の上位の、数字＋カタカナの助数詞の形も替えない
                for other in converter.requestCandidates(composing, options: getOptions(context: "", prediction: false, zenzai: false, nBest: 20)).mainResults.prefix(20)
                where other.correspondingCount == composing.input.count && other.data.count >= 2 && other.data[1].lcid == counterCID && isKatakanaOnly(other.data[1].word) {
                    let fixed = replacingKatakanaCounterAfterDigit(other, topCounter: { bestSameReadingCounter($0, usage: counterCounts()) })
                    XCTAssertNil(fixed, "\(reading) \(other.text) -> \(fixed?.text ?? "")")
                }
            }
        }
        // ガツ には替える先 (月) があり、辞書の値が上
        XCTAssertEqual(bestSameReadingCounter("ガツ", usage: counterCounts())?.word, "月")
        // 意味の違う漢字の助数詞が 2 つ以上ある読みは替えない
        for ruby in ["ショク", "ダン", "カン", "タマ", "ヨウ", "ガタ"] {
            XCTAssertNil(bestSameReadingCounter(ruby, usage: counterCounts()), ruby)
        }
        for fast in [true, false] {
            XCTAssertEqual(convert("2023ねん11がつには", fast: fast), "2023年11月には")
            XCTAssertEqual(convert("どうねん12がつ25にち", fast: fast), "同年12月25日")
            XCTAssertTrue(convert("めいじがんねん11がつ、", fast: fast).contains("11月"))
        }
    }
}
