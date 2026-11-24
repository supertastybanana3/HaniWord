import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-07 (仕事 45): カーソルの右の字と左の文脈の終わりの語を、組み合わせの数の表の外側の文脈に使う (right_context.swift)
final class RightContextTests: XCTestCase {
    func element(_ word: String, _ ruby: String, _ lcid: Int) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: lcid, rcid: lcid, mid: 0, value: -10)
    }

    let table = parseCollocTable("""
    キク\tR1\t薬\t効く\t120\t利く:15
    キク\tL2\t薬|が\t効く\t200\t
    キカイ\tR2\tを|操作\t機械\t300\t
    キカイ\tL1\t新しい\t機械\t40\t
    """)

    func lookup(_ ruby: String, _ word: String) -> DicdataElement? {
        [("キク", "効く"), ("キク", "聞く"), ("キカイ", "機械"), ("キカイ", "機会")].contains { $0 == (ruby, word) } ? element(word, ruby, 1285) : nil
    }

    func testTrim() {
        XCTAssertEqual(trimmedRightContext("を飲む。\n次の行"), "を飲む。")
        XCTAssertEqual(trimmedRightContext("\nを飲む"), "")
        XCTAssertEqual(trimmedRightContext(String(repeating: "あ", count: 30)).count, 20)
    }

    func testWords() {
        // 頭の語: 漢字の並び＋送り仮名 2・1・0 字、カタカナの並び。かなで始まる・数詞だけは無し
        XCTAssertEqual(collocLeadingWords("飲むと"), ["飲むと", "飲む", "飲"])
        XCTAssertEqual(collocLeadingWords("薬を"), ["薬を", "薬"])
        XCTAssertEqual(collocLeadingWords("シアノバクテリアが"), ["シアノバクテリア"])
        XCTAssertEqual(collocLeadingWords("される"), [])
        XCTAssertEqual(collocLeadingWords("三つ"), [])
        // 終わりの語: 漢字の並び＋後ろのかな 3 字まで全部、カタカナの並び
        XCTAssertEqual(collocTrailingWords("とても新しい"), ["新しい"])
        XCTAssertEqual(collocTrailingWords("京都市"), ["京都市"])
        XCTAssertEqual(collocTrailingWords("見てしまった"), [])
        XCTAssertEqual(collocTrailingWords("は、"), [])
    }

    func testOuterContexts() {
        let o = collocOuterContexts(left: "古い薬が", right: "を操作する", useLeft: true, useRight: true)
        XCTAssertEqual(o.left, ["L2\t薬|が"])
        XCTAssertEqual(o.right, ["R2\tを|操作する", "R2\tを|操作す", "R2\tを|操作"])
        XCTAssertEqual(o.rightWords, [])  // 右の字の頭が格助詞なので、候補の最後の格助詞の前の語には使わない
        let o2 = collocOuterContexts(left: "とても新しい", right: "薬を飲む", useLeft: true, useRight: true)
        XCTAssertEqual(o2.left, ["L1\t新しい"])
        XCTAssertEqual(o2.right, ["R1\t薬を", "R1\t薬"])
        XCTAssertTrue(collocOuterContexts(left: "古い薬が", right: "薬を飲む", useLeft: false, useRight: false).isEmpty)
    }

    func testChoosingWithOuter() {
        // 右の字だけで: 聞く|(右: 薬を飲む) → 効く
        let o = collocOuterContexts(left: "", right: "薬を飲む", useLeft: true, useRight: true)
        XCTAssertEqual(choosingByColloc([element("聞く", "キク", 640)], table: table, lookup: lookup, keep: { _ in false }, outer: o)?.map(\.word), ["効く"])
        // 外側の文脈が無ければ今までどおり替えない
        XCTAssertNil(choosingByColloc([element("聞く", "キク", 640)], table: table, lookup: lookup, keep: { _ in false }))
        // 左の文脈の終わりの 語|格助詞: (左: 古い薬が)|聞く → 効く
        let l = collocOuterContexts(left: "古い薬が", right: "", useLeft: true, useRight: true)
        XCTAssertEqual(choosingByColloc([element("聞く", "キク", 640)], table: table, lookup: lookup, keep: { _ in false }, outer: l)?.map(\.word), ["効く"])
        // 候補の最後が格助詞: 機会|を|(右: 操作する) → 機械を
        let r = collocOuterContexts(left: "", right: "操作する", useLeft: true, useRight: true)
        let kikai = [element("機会", "キカイ", 1285), element("を", "ヲ", 147)]
        XCTAssertEqual(choosingByColloc(kikai, table: table, lookup: lookup, keep: { _ in false }, outer: r)?.map(\.word), ["機械", "を"])
        // 左右の文脈が食い違えば替えない (左: 新しい → 機械、右: … → ほかの表記)
        let mixed = parseCollocTable("キカイ\tL1\t新しい\t機械\t40\t\nキカイ\tR1\t均等\t機会\t90\t\n")
        let m = collocOuterContexts(left: "とても新しい", right: "均等に", useLeft: true, useRight: true)
        XCTAssertNil(choosingByColloc([element("器械", "キカイ", 1285)], table: mixed, lookup: lookup, keep: { _ in false }, outer: m))
    }

    func candidate(_ text: String, _ count: Int = 1) -> Candidate {
        Candidate(text: text, value: -10, correspondingCount: count, lastMid: 0, data: [element(text, "ト", 1285)])
    }

    func testRerankPool() {
        // 今の 1 位を頭に、読み全体を覆う別の表記を上から n 個まで (読みの一部だけの候補・同じ表記は入れない)
        let pool = rightRerankPool(chosen: candidate("戸"), others: [candidate("戸"), candidate("と"), candidate("ト", 0), candidate("都"), candidate("徒")], count: 1, n: 3)
        XCTAssertEqual(pool.map(\.text), ["戸", "と", "都"])
    }

    func testRerankChoice() {
        // 点 = 候補の確率 + w × 右の字が続く確率。1 位を替えるのは差が幅を超えるときだけ
        XCTAssertEqual(rightRerankChoice(base: [-2, -3], continuation: [-9, -1], w: 1, margin: 0), 1)
        XCTAssertNil(rightRerankChoice(base: [-2, -3], continuation: [-9, -1], w: 1, margin: 10))
        XCTAssertNil(rightRerankChoice(base: [-2, -3], continuation: [-3, -1], w: 0.1, margin: 0))
        XCTAssertNil(rightRerankChoice(base: [-2], continuation: [-1], w: 1, margin: 0))
    }
}
