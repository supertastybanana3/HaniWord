import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-06 (仕事 (4)): 前後の語との組み合わせの回数で同音の字を選び直す (colloc.swift)
final class CollocTests: XCTestCase {
    func element(_ word: String, _ ruby: String, _ lcid: Int) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: lcid, rcid: lcid, mid: 0, value: -10)
    }

    let table = parseCollocTable("""
    # 見出し
    キク\tR1\t薬\t効く\t120\t利く:15
    カンショウ\tR2\tに|浸る\t感傷\t300\t
    キク\tL2\t話|を\t聞く\t500\t
    キク\tL2\t薬|が\t効く\t200\t
    壊れた行
    """)

    func lookup(_ ruby: String, _ word: String) -> DicdataElement? {
        [("キク", "効く"), ("キク", "聞く"), ("カンショウ", "感傷"), ("カンショウ", "鑑賞")].contains { $0 == (ruby, word) } ? element(word, ruby, 640) : nil
    }

    func testTable() {
        XCTAssertEqual(table.choice("キク\tR1\t薬"), CollocChoice(word: "効く", count: 120, close: ["利く"]))
        XCTAssertEqual(table.choice("カンショウ\tR2\tに|浸る")?.word, "感傷")
        XCTAssertEqual(table.choice("キク\tL2\t話|を")?.word, "聞く")
        XCTAssertNil(table.choice("キク\tR1\t話"))
        XCTAssertNil(table.choice("キ\tR1\t薬"))
        // 字のまま持った表 (頭の # の行あり・鍵の順に並んだもの) も同じに引ける。読みが前の読みの頭 (キク と キクカ) でも
        let raw = "# 見出し\nキク\tR1\t薬\t効く\t83\t利く:15\nキクカ\tR1\t薬\t菊花\t12\t\nキクカ\tR2\tを|飾る\t菊花\t20\t\n"
        let sorted = CollocTable(bytes: Array(raw.utf8))
        XCTAssertEqual(sorted.choice("キク\tR1\t薬")?.close, ["利く"])
        XCTAssertEqual(sorted.choice("キクカ\tR1\t薬")?.word, "菊花")
        XCTAssertEqual(sorted.choice("キクカ\tR2\tを|飾る")?.count, 20)
        XCTAssertNil(sorted.choice("キク\tR2\tを|飾る"))
    }

    func testChoosing() {
        // よく|聞く|薬 → よく 効く 薬 (すぐ後ろの 薬)
        let a = [element("よく", "ヨク", 10), element("聞く", "キク", 640), element("薬", "クスリ", 1285)]
        XCTAssertEqual(choosingByColloc(a, table: table, lookup: lookup, keep: { _ in false })?.map(\.word), ["よく", "効く", "薬"])
        // 鑑賞|に|浸る → 感傷 (後ろの に＋浸る)
        let b = [element("鑑賞", "カンショウ", 1283), element("に", "ニ", 151), element("浸る", "ヒタル", 640)]
        XCTAssertEqual(choosingByColloc(b, table: table, lookup: lookup, keep: { _ in false })?.map(\.word), ["感傷", "に", "浸る"])
        // 話|を|聞く|薬 は、前 (話を → 聞く) と後ろ (薬 → 効く) が食い違うので替えない
        let c = [element("話", "ハナシ", 1285), element("を", "ヲ", 147), element("聞く", "キク", 640), element("薬", "クスリ", 1285)]
        XCTAssertNil(choosingByColloc(c, table: table, lookup: lookup, keep: { _ in false }))
        // すでに選ぶ表記・替えない語 (学習した語など)・辞書に無い表記 は替えない
        XCTAssertNil(choosingByColloc([element("効く", "キク", 640), element("薬", "クスリ", 1285)], table: table, lookup: lookup, keep: { _ in false }))
        XCTAssertNil(choosingByColloc(a, table: table, lookup: lookup, keep: { $0.word == "聞く" }))
        XCTAssertNil(choosingByColloc(a, table: table, lookup: { _, _ in nil }, keep: { _ in false }))
        // 近い表記 (利く) は替えない
        XCTAssertNil(choosingByColloc([element("利く", "キク", 640), element("薬", "クスリ", 1285)], table: table, lookup: lookup, keep: { _ in false }))
        // 1 字の語は、切り替えで替えない形にできる。固有名詞の品詞の語は替えない
        let single = parseCollocTable("キ\tR1\t薬\t効\t50\t\n")
        let one = [element("聞", "キ", 1285), element("薬", "クスリ", 1285)]
        let lookupOne: (String, String) -> DicdataElement? = { ruby, word in self.element(word, ruby, 1285) }
        XCTAssertEqual(choosingByColloc(one, table: single, lookup: lookupOne, keep: { _ in false })?.map(\.word), ["効", "薬"])
        XCTAssertNil(choosingByColloc(one, table: single, lookup: lookupOne, keep: { _ in false }, singleCharacter: false))
        XCTAssertNil(choosingByColloc([element("聞く", "キク", 1290), element("薬", "クスリ", 1285)], table: table, lookup: lookup, keep: { _ in false }))
        // 助詞と 1 語になった語は、分けて文脈と狙いの語を見る (関心|を買う → 歓心を買う、上司|の関心|を買う → 上司の歓心を買う)
        let kanshin = parseCollocTable("カンシン\tR2\tを|買う\t歓心\t110\t\n")
        let lookupK: (String, String) -> DicdataElement? = { ruby, word in ruby == "カンシン" && word == "歓心" ? self.element("歓心", "カンシン", 1285) : nil }
        let k1 = [element("関心", "カンシン", 1285), DicdataElement(word: "を買う", ruby: "ヲカウ", lcid: 156, rcid: 817, mid: 0, value: -10)]
        XCTAssertEqual(choosingByColloc(k1, table: kanshin, lookup: lookupK, keep: { _ in false })?.map(\.word), ["歓心", "を買う"])
        let k2 = [element("上司", "ジョウシ", 1285), DicdataElement(word: "の関心", ruby: "ノカンシン", lcid: 368, rcid: 1285, mid: 0, value: -10),
                  DicdataElement(word: "を買う", ruby: "ヲカウ", lcid: 156, rcid: 817, mid: 0, value: -10)]
        let k2fixed = choosingByColloc(k2, table: kanshin, lookup: lookupK, keep: { _ in false })
        XCTAssertEqual(k2fixed?.map(\.word), ["上司", "の歓心", "を買う"])
        XCTAssertEqual(k2fixed?[1].ruby, "ノカンシン")
        XCTAssertEqual(k2fixed?[1].lcid, 368)
        XCTAssertEqual(collocPieces(k2).map(\.element.word), ["上司", "の", "関心", "を", "買う"])
        // 助詞・助動詞は文脈にしない
        XCTAssertEqual(collocContexts([element("聞く", "キク", 640), element("た", "タ", 400)], at: 0), [])
        // 数詞と接尾の語は、文脈の語にしない (一|台 → 一第、包み|方 → 堤方 にしない)。狙いの語にはしてよい (細胞|幕 → 細胞膜)
        let numsuf = parseCollocTable("ダイ\tL1\t一\t第\t11\t大:5\nツツミ\tR1\t方\t堤\t27\t\nマク\tL1\t細胞\t膜\t120\t\n")
        let lookupN: (String, String) -> DicdataElement? = { ruby, word in self.element(word, ruby, 1285) }
        XCTAssertNil(choosingByColloc([element("一", "イチ", 1295), element("台", "ダイ", 1300)], table: numsuf, lookup: lookupN, keep: { _ in false }))
        XCTAssertNil(choosingByColloc([element("一", "イチ", 1295), element("台", "ダイ", 1285)], table: numsuf, lookup: lookupN, keep: { _ in false }))
        XCTAssertNil(choosingByColloc([element("包み", "ツツミ", 1285), element("方", "カタ", 1304)], table: numsuf, lookup: lookupN, keep: { _ in false }))
        XCTAssertEqual(choosingByColloc([element("細胞", "サイボウ", 1285), element("幕", "マク", 1298)], table: numsuf, lookup: lookupN, keep: { _ in false })?.map(\.word), ["細胞", "膜"])
    }
}
