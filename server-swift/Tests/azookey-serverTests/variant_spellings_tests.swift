import XCTest
@testable import azookey_server

final class VariantSpellingsTests: XCTestCase {
    let table = parseVariantSpellings("""
    # 読み\t代わりの字の書き方\t本来の書き方\tかな書き
    ろんをまたない\t論を待たない\t論を俟たない\t論をまたない
    かんにさわる\t勘に障る\t癇に障る\t
    きかいをうかがっ\t機会を伺っ\t機会を窺っ\t機会をうかがっ
    チャンスをうかがう\tチャンスを伺う\tチャンスを窺う\t
    """)

    func testTableKeysUseHiraganaReadings() {
        XCTAssertEqual(table.count, 4)
        XCTAssertEqual(table[variantSpellingKey(reading: "ろんをまたない", text: "論を待たない")], VariantSpelling(original: "論を俟たない", kana: "論をまたない"))
        XCTAssertEqual(table[variantSpellingKey(reading: "かんにさわる", text: "勘に障る")]?.kana, "")
        // 打った読みはひらがな (ちゃんす)。表のカタカナの読み (チャンス) と同じに引ける
        XCTAssertNotNil(table[variantSpellingKey(reading: "ちゃんすをうかがう", text: "チャンスを伺う")])
    }

    func testSubstituteSpellingInConversionBecomesOriginal() {
        // 句が文節に分かれていても、つながった文節の字と読みが合えば 1 文節にまとめて本来の書き方にする
        let fixed = replacingVariantSpellings([(word: "それは", rubyCount: 3), (word: "勘", rubyCount: 2), (word: "に", rubyCount: 1), (word: "障る", rubyCount: 3), (word: "。", rubyCount: 1)],
                                              reading: "それはかんにさわる。", table: table)
        XCTAssertEqual(fixed.map(\.word), ["それは", "癇に障る", "。"])
        XCTAssertEqual(fixed.map(\.rubyCount), [3, 6, 1])
        // 活用した形 (機会を伺って) も、表にある形なら同じ
        let inflected = replacingVariantSpellings([(word: "機会を伺っ", rubyCount: 8), (word: "て", rubyCount: 1)], reading: "きかいをうかがって", table: table)
        XCTAssertEqual(inflected.map(\.word), ["機会を窺っ", "て"])
        // 後ろのかなが同じ文節に続いていても (機会を|伺って)
        let joined = replacingVariantSpellings([(word: "機会を", rubyCount: 4), (word: "伺って", rubyCount: 5)], reading: "きかいをうかがって", table: table)
        XCTAssertEqual(joined.map(\.word), ["機会を窺って"])
        XCTAssertEqual(joined.map(\.rubyCount), [9])
    }

    func testOtherConversionsAreUntouched() {
        // かな書き (論をまたない) は今の変換のまま
        let kana = [(word: "論をまたない", rubyCount: 7)]
        XCTAssertEqual(replacingVariantSpellings(kana, reading: "ろんをまたない", table: table).map(\.word), ["論をまたない"])
        // 同じ字でも読みが違えばさわらない (勘|に|障る の読みが かんにさわる でない)
        let other = [(word: "勘", rubyCount: 2), (word: "に", rubyCount: 1), (word: "障る", rubyCount: 3)]
        XCTAssertEqual(replacingVariantSpellings(other, reading: "かんにさわり", table: table).map(\.word), ["勘", "に", "障る"])
        // 文節の読みの長さの合計が読みと合わないときもさわらない
        XCTAssertEqual(replacingVariantSpellings(other, reading: "かんにさわる。", table: table).map(\.word), ["勘", "に", "障る"])
    }

    func testEmptySegmentTextDoesNotCrash() {
        // 表記が空の文節が来ても落ちない (D の 2 回目の調べ R2-7)。空の文節は後ろの句と一緒に置き換わる
        let fixed = replacingVariantSpellings([(word: "", rubyCount: 0), (word: "勘に障る", rubyCount: 6)], reading: "かんにさわる", table: table)
        XCTAssertEqual(fixed.map(\.word), ["癇に障る"])
        XCTAssertEqual(fixed.map(\.rubyCount), [6])
    }

    func testCandidateListPutsOriginalAndKanaBeforeSubstitute() {
        let ordered = orderingVariantSpellings(["論を待たない", "ロンを待たない", "論をまたない", "ろんをまたない"], reading: "ろんをまたない", table: table)
        XCTAssertEqual(ordered, ["論を俟たない", "論をまたない", "論を待たない", "ロンを待たない", "ろんをまたない"])
        // かな書きが無い句は本来の書き方だけ。一覧に表の候補が無ければそのまま
        XCTAssertEqual(orderingVariantSpellings(["感に触る", "勘に障る"], reading: "かんにさわる", table: table), ["感に触る", "癇に障る", "勘に障る"])
        XCTAssertEqual(orderingVariantSpellings(["感に触る", "間に触る"], reading: "かんにさわる", table: table), ["感に触る", "間に触る"])
    }
}
