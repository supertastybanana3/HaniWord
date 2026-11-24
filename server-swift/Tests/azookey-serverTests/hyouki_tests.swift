import Foundation
import XCTest
@testable import azookey_server

/// 2026-11-03: 表記ゆれの学習 (hyouki.swift) の決まり
final class HyoukiTests: XCTestCase {
    // 読み・左・右・形・種類・ゆれ・印・Wikipedia・青空文庫・コーパスの最多・辞書の最上・辞書の値・確かめ・出どころ・JMdict の項目
    static func row(_ cells: [String]) -> String { cells.joined(separator: "\t") }
    let text = [
        "# 試験用の表",
        HyoukiTests.row(["いう", "818", "818", "いう|云う", "動詞のかな書き", "かな", "かなは別の語と共有 (言う)", "1574108|1921", "386907|50243", "いう", "いう", "-3.3|-8.2", "JMdict", "azooKey", "1008860,1264080"]),
        HyoukiTests.row(["いう", "818", "818", "いう|言う", "動詞のかな書き", "かな", "かなは別の語と共有 (云う)", "1574108|46954", "386907|17709", "いう", "言う", "-3.3|-2.9", "JMdict", "azooKey", "1008860,1263950"]),
        HyoukiTests.row(["まく", "1285", "1285", "まく|膜", "名詞のかな書き", "かな", "かなは別の語と共有 (幕)", "3000|90000", "10|500", "膜", "膜", "-9|-6", "JMdict", "azooKey", "1500001"]),
        HyoukiTests.row(["まく", "1285", "1285", "まく|幕", "名詞のかな書き", "かな", "かなは別の語と共有 (膜)", "3000|120000", "10|3000", "幕", "幕", "-9|-6", "JMdict", "azooKey", "1500002"]),
        HyoukiTests.row(["みる", "800", "800", "みる|見る", "動詞のかな書き", "かな", "", "50000|400000", "1000|9000", "見る", "見る", "-5|-3", "JMdict", "azooKey", "1259290"]),
        HyoukiTests.row(["こと", "1310", "1310", "こと|事", "形式名詞", "かな", "使い分け: 形式名詞はかな・実質の意味は漢字；かなは別の語と共有 (古都・殊・琴・異・箏…)", "4001930|314308", "531135|93640", "こと", "こと", "-2.7|-4.9", "JMdict", "azooKey", "1313580"]),
        HyoukiTests.row(["ください", "900", "900", "ください|下さい", "補助動詞", "かな", "使い分け: 補助動詞はかな・実質の意味は漢字", "300000|200000", "10000|20000", "ください", "下さい", "-4|-4", "JMdict", "azooKey", "1184270"]),
        HyoukiTests.row(["うけつけ", "1285", "1285", "受付|受け付け", "名詞の送り仮名", "送り仮名", "通則7", "90000|30000", "100|50", "受付", "受付", "-6|-8", "JMdict", "azooKey", "1325150"]),
    ].joined(separator: "\n")
    /// 重みは測りで決めるので、試験では決まりだけを見る (既定の値に頼らない)
    let weights = HyoukiWeights(accepted: 0, prior: 1, chosen: 1, direction: 8, margin: 0.5, minimum: 2)

    func testRowsOfTheSameWordAreMergedButSharedKanaIsNot() {
        let table = parseHyoukiTable(text)
        let iuKey = HyoukiTable.key(reading: "いう", lcid: 818, rcid: 818)
        // JMdict の項目を共有する いう|云う と いう|言う は 1 つの語。括弧の語がどれもまとめた形に入るので、かなは共有ではない
        let iu = table.lexeme(iuKey, containing: "いう")
        XCTAssertEqual(iu?.forms, ["いう", "云う", "言う"])
        XCTAssertEqual(iu?.counts, [1574108 + 386907, 1921 + 50243, 46954 + 17709])
        XCTAssertEqual(iu?.sharedKana, false)
        // かなの形だけを共有する まく|膜 と まく|幕 は別の語。まく はどちらの語か分からない
        let makuKey = HyoukiTable.key(reading: "まく", lcid: 1285, rcid: 1285)
        XCTAssertEqual(table.lexemes[makuKey]?.count, 2)
        XCTAssertNil(table.lexeme(makuKey, containing: "まく"))
        XCTAssertEqual(table.lexeme(makuKey, containing: "膜")?.forms, ["まく", "膜"])
        XCTAssertEqual(table.lexeme(makuKey, containing: "膜")?.sharedKana, true)
        // … で切れた共有の印は、共有とみる
        XCTAssertEqual(table.lexeme(HyoukiTable.key(reading: "こと", lcid: 1310, rcid: 1310), containing: "事")?.sharedKana, true)
        XCTAssertEqual(table.byReading["まく"]?.count, 1)
    }

    func testChoicesCountFormsAndDirections() {
        let table = parseHyoukiTable(text)
        var learned = HyoukiLearned()
        XCTAssertTrue(noteHyoukiChoice(&learned, table: table, reading: "みる", lcid: 800, rcid: 800, form: "みる", weight: 1))
        XCTAssertTrue(noteHyoukiChoice(&learned, table: table, reading: "うけつけ", lcid: 1285, rcid: 1285, form: "受け付け", weight: 1))
        XCTAssertEqual(learned.forms[HyoukiTable.key(reading: "みる", lcid: 800, rcid: 800)], ["みる": 1])
        XCTAssertEqual(learned.directions["動詞のかな書き"], HyoukiDirection(kana: 1, kanji: 0))
        // 送り仮名の多い形 (受け付け) はかなの割合が高いので、かな寄り
        XCTAssertEqual(learned.directions["名詞の送り仮名"], HyoukiDirection(kana: 1, kanji: 0))
        // 重み 0 (今の表示のまま確定した形を数えないとき)・表に無い形・番号の違う語・どの語か分からないかなは覚えない
        XCTAssertFalse(noteHyoukiChoice(&learned, table: table, reading: "みる", lcid: 800, rcid: 800, form: "見る", weight: 0))
        XCTAssertFalse(noteHyoukiChoice(&learned, table: table, reading: "みる", lcid: 801, rcid: 801, form: "見る", weight: 1))
        XCTAssertFalse(noteHyoukiChoice(&learned, table: table, reading: "みる", lcid: 800, rcid: 800, form: "観る", weight: 1))
        XCTAssertFalse(noteHyoukiChoice(&learned, table: table, reading: "まく", lcid: 1285, rcid: 1285, form: "まく", weight: 1))
    }

    func testPreferredFormFollowsTheUserAndNotTheCorpusAlone() {
        let table = parseHyoukiTable(text)
        let miru = table.lexeme(HyoukiTable.key(reading: "みる", lcid: 800, rcid: 800), containing: "見る")!
        // 覚えた記録が無ければ、コーパスで多い形 (見る) があっても替えない
        XCTAssertNil(hyoukiPreferredForm(miru, current: "みる", chosen: nil, direction: nil, weights: weights))
        // その語で何度かかなを選んだら、かなに (1 回ではコーパスの差に届かない)
        XCTAssertNil(hyoukiPreferredForm(miru, current: "見る", chosen: ["みる": 1], direction: nil, weights: weights))
        XCTAssertEqual(hyoukiPreferredForm(miru, current: "見る", chosen: ["みる": 3], direction: nil, weights: weights), "みる")
        // まだ選んだことの無い語にも、同じ種類のかな寄りの向きが効く
        XCTAssertEqual(hyoukiPreferredForm(miru, current: "見る", chosen: nil, direction: HyoukiDirection(kana: 6, kanji: 0), weights: weights), "みる")
        // 向きの数が少ないうちは使わない
        XCTAssertNil(hyoukiPreferredForm(miru, current: "見る", chosen: nil, direction: HyoukiDirection(kana: 1, kanji: 0), weights: weights))
        // 漢字寄りの人には漢字に
        XCTAssertEqual(hyoukiPreferredForm(miru, current: "みる", chosen: nil, direction: HyoukiDirection(kana: 0, kanji: 6), weights: weights), "見る")
    }

    func testSharedKanaIsNotTurnedIntoKanji() {
        let table = parseHyoukiTable(text)
        let koto = table.lexeme(HyoukiTable.key(reading: "こと", lcid: 1310, rcid: 1310), containing: "こと")!
        // こと は 古都・琴 … のかなでもあるので、漢字寄りの人でも こと → 事 にしない
        XCTAssertNil(hyoukiPreferredForm(koto, current: "こと", chosen: ["事": 5], direction: HyoukiDirection(kana: 0, kanji: 6), weights: weights))
        // 漢字 → かな はよい
        XCTAssertEqual(hyoukiPreferredForm(koto, current: "事", chosen: nil, direction: HyoukiDirection(kana: 6, kanji: 0), weights: weights), "こと")
        // まとめて共有でなくなった いう は、漢字寄りの人には 言う に
        let iu = table.lexeme(HyoukiTable.key(reading: "いう", lcid: 818, rcid: 818), containing: "いう")!
        XCTAssertEqual(hyoukiPreferredForm(iu, current: "いう", chosen: ["言う": 8], direction: nil, weights: weights), "言う")
    }

    /// 2026-11-04: 同じ読みのほかの番号の行に同じかなの形があれば、表の印が無くても共有とみる (名詞の および (お呼び) と接続詞の および (及び))
    func testKanaSharedWithAnotherPartOfSpeechIsNotTurnedIntoKanji() {
        let table = parseHyoukiTable([
            HyoukiTests.row(["および", "1285", "1285", "および|お呼び", "名詞のかな書き", "かな", "", "8|232", "1|518", "お呼び", "および", "-7.9|-10.7", "JMdict", "azooKey", "1694210"]),
            HyoukiTests.row(["および", "556", "556", "および|及び", "接続詞", "かな", "", "428767|274185", "2180|5218", "および", "及び", "-4.4|-4.2", "JMdict", "azooKey", "1228120"]),
            HyoukiTests.row(["みる", "800", "800", "みる|見る", "動詞のかな書き", "かな", "", "50000|400000", "1000|9000", "見る", "見る", "-5|-3", "JMdict", "azooKey", "1259290"]),
        ].joined(separator: "\n"))
        let noun = table.lexeme(HyoukiTable.key(reading: "および", lcid: 1285, rcid: 1285), containing: "および")!
        let conjunction = table.lexeme(HyoukiTable.key(reading: "および", lcid: 556, rcid: 556), containing: "および")!
        XCTAssertTrue(noun.sharedKana)
        XCTAssertTrue(conjunction.sharedKana)
        XCTAssertNil(hyoukiPreferredForm(noun, current: "および", chosen: ["お呼び": 5], direction: HyoukiDirection(kana: 0, kanji: 6), weights: weights))
        // 漢字 → かな はよい。ほかの番号に同じ読みの行が無い語 (みる) は今までどおり
        XCTAssertEqual(hyoukiPreferredForm(noun, current: "お呼び", chosen: ["および": 5], direction: HyoukiDirection(kana: 6, kanji: 0), weights: weights), "および")
        XCTAssertFalse(table.lexeme(HyoukiTable.key(reading: "みる", lcid: 800, rcid: 800), containing: "みる")!.sharedKana)
    }

    func testDirectionsStayInsideTheirCategory() {
        let table = parseHyoukiTable(text)
        var learned = HyoukiLearned()
        // 動詞のかな書きでかなを選び続けた人
        for _ in 0..<6 { _ = noteHyoukiChoice(&learned, table: table, reading: "みる", lcid: 800, rcid: 800, form: "みる", weight: 1) }
        let kudasaiKey = HyoukiTable.key(reading: "ください", lcid: 900, rcid: 900)
        let kudasai = table.lexeme(kudasaiKey, containing: "下さい")!
        // 補助動詞 (使い分けの種類) には、動詞のかな書きの向きを使わない
        XCTAssertNil(hyoukiPreferredForm(kudasai, current: "下さい", chosen: learned.forms[kudasaiKey], direction: learned.directions[kudasai.category], weights: weights))
        // 形式名詞でかなを選んだら、形式名詞の向きだけが増える
        for _ in 0..<6 { _ = noteHyoukiChoice(&learned, table: table, reading: "こと", lcid: 1310, rcid: 1310, form: "こと", weight: 1) }
        XCTAssertEqual(learned.directions["形式名詞"], HyoukiDirection(kana: 6, kanji: 0))
        XCTAssertEqual(learned.directions["動詞のかな書き"], HyoukiDirection(kana: 6, kanji: 0))
        XCTAssertNil(learned.directions["補助動詞"])
    }

    /// 学習データを使ってはいけない接続 (personalDataAllowed が偽) では、記録を使わない・書かない
    func testRecordsAreNeitherUsedNorWrittenWithoutPermission() async {
        await MainActor.run {
            let saved = (personalDataAllowed, hyoukiLearnedCache)
            defer {
                personalDataAllowed = saved.0
                hyoukiLearnedCache = saved.1
            }
            var learned = HyoukiLearned()
            learned.directions["形式名詞"] = HyoukiDirection(kana: 3, kanji: 0)
            hyoukiLearnedCache = learned
            personalDataAllowed = false
            XCTAssertFalse(hyoukiLearningEnabled())
            XCTAssertTrue(hyoukiLearned().isEmpty)
            saveHyoukiLearned(HyoukiLearned())
            personalDataAllowed = true
            XCTAssertEqual(hyoukiLearned(), learned)
        }
    }

    func testForgettingAndTheFileRoundTrip() {
        let table = parseHyoukiTable(text)
        var learned = HyoukiLearned()
        _ = noteHyoukiChoice(&learned, table: table, reading: "みる", lcid: 800, rcid: 800, form: "みる", weight: 1)
        _ = noteHyoukiChoice(&learned, table: table, reading: "うけつけ", lcid: 1285, rcid: 1285, form: "受け付け", weight: 0.5)
        XCTAssertEqual(parseHyoukiLearned(formatHyoukiLearned(learned)), learned)
        let forgotten = forgettingHyouki(learned, words: ["受け付け"])
        XCTAssertNil(forgotten.forms[HyoukiTable.key(reading: "うけつけ", lcid: 1285, rcid: 1285)])
        XCTAssertEqual(forgotten.forms[HyoukiTable.key(reading: "みる", lcid: 800, rcid: 800)], ["みる": 1])
        // 壊れた行は読まない
        XCTAssertTrue(parseHyoukiLearned("F\tみる\tx\t800\tみる\t1\nF\tみる\t800\t800\tみる\t-1\nD\t形式名詞\tnan\t1\n").isEmpty)
    }
}
