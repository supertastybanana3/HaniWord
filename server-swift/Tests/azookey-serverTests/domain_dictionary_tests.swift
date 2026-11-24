import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

final class DomainDictionaryTests: XCTestCase {
    func testPublishedDomainsAreOnUnlessTurnedOff() {
        XCTAssertEqual(enabledDomainIdentifiers(config: [:], environment: [:], published: []), [])
        XCTAssertEqual(enabledDomainIdentifiers(config: [:], environment: [:], published: ["law", "med"]), ["domain_law", "domain_med"])
        XCTAssertEqual(enabledDomainIdentifiers(config: ["domainsOff": ["law"]], environment: [:], published: ["law", "med"]), ["domain_med"])
        XCTAssertEqual(enabledDomainIdentifiers(config: [:], environment: ["AZK_DOMAIN_LAW": "0"], published: ["law"]), [])
        XCTAssertEqual(enabledDomainIdentifiers(config: ["domainsOff": ["law"]], environment: ["AZK_DOMAIN_LAW": "1"], published: ["law"]), ["domain_law"])
    }

    func testOnlyPublishedDomainsAreRead() {
        // 古い個人用の設定 (domain_biology など) が残っていても、domains.txt に無い分野は読まない
        XCTAssertEqual(enabledDomainIdentifiers(config: ["domainBiology": true], environment: ["AZK_DOMAIN_BIO": "1"], published: ["law"]), ["domain_law"])
    }

    func testDialectDictionariesAreReadOnlyWhenChosen() {
        // 2026-11-06 (仕事 39): 方言の言い方の辞書 (dialects.txt) は、設定 input.dialects で選んだ名前だけ読む (初期値は読まない)
        XCTAssertEqual(dialectIdentifiers(config: [:], environment: [:], published: ["kansai"]), [])
        XCTAssertEqual(dialectIdentifiers(config: ["dialects": ["kansai"]], environment: [:], published: ["kansai"]), ["domain_dialect_kansai"])
        // dialects.txt に無い名前・追加辞書の名前は読まない。分野の設定 (domains_off・domains_focus) は効かない
        XCTAssertEqual(dialectIdentifiers(config: ["dialects": ["hakata", "core_forms"]], environment: [:], published: ["kansai", "core_forms"]), [])
        XCTAssertEqual(dialectIdentifiers(config: ["dialects": ["kansai"], "domainsOff": ["kansai"]], environment: [:], published: ["kansai"]), ["domain_dialect_kansai"])
        XCTAssertEqual(dialectIdentifiers(config: ["domainsFocus": ["kansai"]], environment: [:], published: ["kansai"]), [])
        // 試験用の切り替え
        XCTAssertEqual(dialectIdentifiers(config: [:], environment: ["AZK_DIALECT_KANSAI": "1"], published: ["kansai"]), ["domain_dialect_kansai"])
        XCTAssertEqual(dialectIdentifiers(config: ["dialects": ["kansai"]], environment: ["AZK_DIALECT_KANSAI": "0"], published: ["kansai"]), [])
        XCTAssertEqual(dialectIdentifiers(config: ["dialects": ["kansai"]], environment: [:], published: publishedDomainNames(nil)), [])
    }

    func testSlangDictionariesAreReadOnlyWhenTurnedOn() {
        // 2026-11-07 (仕事 43): ネットスラングの辞書 (slang.txt の internet) は、設定 input.internet_slang をオンにしたときだけ読む (初期値は読まない)
        XCTAssertEqual(slangIdentifiers(config: [:], environment: [:], published: ["internet"]), [])
        XCTAssertEqual(slangIdentifiers(config: ["internetSlang": false], environment: [:], published: ["internet"]), [])
        XCTAssertEqual(slangIdentifiers(config: ["internetSlang": true], environment: [:], published: ["internet"]), ["domain_slang_internet"])
        // slang.txt に無ければ読まない。知らない名前・追加辞書の名前は読まない。方言・分野の設定は効かない
        XCTAssertEqual(slangIdentifiers(config: ["internetSlang": true], environment: [:], published: publishedDomainNames(nil)), [])
        XCTAssertEqual(slangIdentifiers(config: ["internetSlang": true], environment: [:], published: ["gyaru", "core_forms"]), [])
        XCTAssertEqual(slangIdentifiers(config: ["dialects": ["internet"], "domainsFocus": ["internet"]], environment: [:], published: ["internet"]), [])
        // 試験用の切り替え
        XCTAssertEqual(slangIdentifiers(config: [:], environment: ["AZK_SLANG_INTERNET": "1"], published: ["internet"]), ["domain_slang_internet"])
        XCTAssertEqual(slangIdentifiers(config: ["internetSlang": true], environment: ["AZK_SLANG_INTERNET": "0"], published: ["internet"]), [])
    }

    func testAlwaysOnDictionariesAreReadUnlessTurnedOffForTests() {
        // オフにできない追加辞書 (always.txt) は設定 domains_off を見ない。試験用の環境変数でだけ 1 つずつ外せる
        XCTAssertEqual(alwaysOnIdentifiers(environment: [:], names: ["core_forms", "core_phrases"]), ["domain_core_forms", "domain_core_phrases"])
        XCTAssertEqual(alwaysOnIdentifiers(environment: ["AZK_ALWAYS_CORE_FORMS": "0"], names: ["core_forms", "core_phrases"]), ["domain_core_phrases"])
        // core_ で始まらない名前 (分野の名前) は追加辞書として読まない
        XCTAssertEqual(alwaysOnIdentifiers(environment: [:], names: ["law", "core_words"]), ["domain_core_words"])
        XCTAssertEqual(alwaysOnIdentifiers(environment: [:], names: publishedDomainNames(nil)), [])
    }

    func testOnlyTheMisreadingDictionaryMarksReadingCorrections() {
        // 読み間違いの本来の形の印 (AI の変換で落とさない) は core_misreadings の語だけ。外した追加辞書・分野辞書には付けない
        let always = alwaysOnIdentifiers(environment: [:], names: ["core_forms", "core_misreadings", "core_phrases"])
        XCTAssertEqual(readingCorrectionIdentifiers(environment: [:], always: always), ["domain_core_misreadings"])
        XCTAssertEqual(readingCorrectionIdentifiers(environment: [:], always: ["domain_core_forms", "domain_law"]), [])
        XCTAssertEqual(readingCorrectionIdentifiers(environment: ["AZK_READING_CORRECTIONS": "0"], always: always), [])
        XCTAssertEqual(readingCorrectionIdentifiers(environment: [:], always: alwaysOnIdentifiers(environment: ["AZK_ALWAYS_CORE_MISREADINGS": "0"], names: ["core_misreadings"])), [])
    }

    func testBoundedTableSkipsBrokenRowsAndStopsAtLimits() {
        // 説明・制御文字のある行・欄の数が合わない行・長すぎる欄の行は捨て、行数の上限で止める (D の調べ D2-2)
        let text = "# 説明\nあ\tい\t-10\nう\u{0007}\tえ\t-10\nお\tか\nき\t" + String(repeating: "く", count: 300) + "\t-10\nけ\tこ\t-12\nさ\tし\t-13\n"
        XCTAssertEqual(BoundedTable.rows(text: text, maxLines: 2, columns: 3 ... 3, maxFieldLength: 256), [["あ", "い", "-10"], ["け", "こ", "-12"]])
        XCTAssertEqual(BoundedTable.value("-12.5"), -12.5)
        XCTAssertNil(BoundedTable.value("nan"))
        XCTAssertNil(BoundedTable.value("-inf"))
        XCTAssertNil(BoundedTable.value("5"))
        XCTAssertNil(BoundedTable.value("-1e9"))
    }

    func testBoundedTableDoesNotReadFilesOverTheSizeLimit() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("hw_bounded_table_\(UUID().uuidString).tsv")
        try Data("あ\tい\t-10\n".utf8).write(to: url)
        defer { try? FileManager.default.removeItem(at: url) }
        XCTAssertEqual(BoundedTable.text(url, maxBytes: 64), "あ\tい\t-10\n")
        XCTAssertNil(BoundedTable.text(url, maxBytes: 4))
        XCTAssertEqual(BoundedTable.rows(url, maxBytes: 4, columns: 3 ... 3), [])
        XCTAssertNil(BoundedTable.text(url.appendingPathExtension("missing"), maxBytes: 64))
    }

    func testPublishedDomainNamesAreFileNameSafe() {
        XCTAssertEqual(publishedDomainNames("# 分野辞書\nlaw\n\nmed\nlaw\n../x\nLaw\nit_terms\n"), ["law", "med", "it_terms"])
        XCTAssertEqual(publishedDomainNames(nil), [])
    }

    func testDomainBoostReadsWordsAndSignals() {
        let boost = DomainBoost(boost: "# 説明\n債権\tさいけん\n刑法\tケイホウ\n\t空\n債権\tサイケン\n", signals: "被告人\n裁判所\n")!
        XCTAssertEqual(boost.byReading["サイケン"], ["債権"])
        XCTAssertTrue(boost.pairs.contains("ケイホウ\t刑法"))
        XCTAssertEqual(boost.signals, ["債権", "刑法", "被告人", "裁判所"])
        XCTAssertNil(DomainBoost(boost: nil, signals: nil))
    }

    func testFocusedDomainsAreActiveWithoutSignals() {
        // よく打つ分野 (input.domains_focus) は、文脈に目印の語が無くても優遇を効かせる。選ばない分野は目印があるときだけ
        XCTAssertEqual(focusedDomainIdentifiers(config: [:]), [])
        XCTAssertEqual(focusedDomainIdentifiers(config: ["domainsFocus": ["math", "../x", "Law", "law"]]), ["domain_math", "domain_law"])
        var boost = DomainBoost(boost: "乗\tじょう\n", signals: "関数\n方程式\n")!
        XCTAssertFalse(boost.isActive("xの2畳を計算する", minimum: 1))
        XCTAssertTrue(boost.isActive("この関数の2畳", minimum: 1))
        boost.focused = true
        XCTAssertTrue(boost.isActive("xの2畳を計算する", minimum: 1))
    }

    func testCombinedDomainVariantReplacesEveryPlace() {
        // 差し替えられる所を全部まとめた並び (初稿・と・交差 → 初項・と・公差)。助詞 (と) は差し替えない。1 か所しか直らなければ nil
        let boost = DomainBoost(boost: "初項\tしょこう\n公差\tこうさ\n", signals: nil)!
        func e(_ w: String, _ r: String, _ cid: Int) -> DicdataElement { DicdataElement(word: w, ruby: r, lcid: cid, rcid: cid, mid: 1, value: -10) }
        let elems = [e("初稿", "ショコウ", 1285), e("と", "ト", 200), e("交差", "コウサ", 1285)]
        XCTAssertEqual(combinedDomainVariant(elems, boosts: [boost])?.map(\.word), ["初項", "と", "公差"])
        XCTAssertNil(combinedDomainVariant([e("初稿", "ショコウ", 1285), e("と", "ト", 200)], boosts: [boost]))
        XCTAssertNil(combinedDomainVariant([e("初項", "ショコウ", 1285), e("交差", "コウサ", 1285)], boosts: [boost]))
        // 採点の加点は、差し替えた語の数 (もとの並びにあった分野の語は数えない)
        XCTAssertEqual(replacedDomainWords([e("初項", "ショコウ", 1285), e("と", "ト", 200), e("公差", "コウサ", 1285)], base: elems, boosts: [boost]), 2)
        XCTAssertEqual(replacedDomainWords([e("初項", "ショコウ", 1285), e("と", "ト", 200), e("交差", "コウサ", 1285)], base: elems, boosts: [boost]), 1)
        XCTAssertEqual(replacedDomainWords(elems, base: elems, boosts: [boost]), 0)
    }

    func testDomainSignalsCountDistinctWords() {
        let boost = DomainBoost(boost: nil, signals: "被告人\n裁判所\n被告\n")!
        XCTAssertEqual(boost.signalCount("被告人は裁判所に出頭した。", minimum: 5), 2)
        XCTAssertEqual(boost.signalCount("被告人と被告人", minimum: 5), 1)
        XCTAssertEqual(boost.signalCount("今日は良い天気だ", minimum: 2), 0)
        XCTAssertEqual(boost.signalCount("被告人は裁判所に", minimum: 1), 1)
        // 文の終わりの目印も数える。長い語が先に当たれば、同じ所から始まる短い語 (被告) は数えない
        XCTAssertEqual(boost.signalCount("出頭したのは被告", minimum: 5), 1)
        XCTAssertEqual(boost.signalCount("被告人", minimum: 5), 1)
        XCTAssertEqual(boost.lengthsByFirst["被"], [3, 2])
    }

    func testNotationSkeletonIgnoresOkuriganaAndDigits() {
        XCTAssertEqual(notationSkeleton("取り消し"), notationSkeleton("取消"))
        XCTAssertEqual(notationSkeleton("第1号"), notationSkeleton("第一号"))
        XCTAssertNotEqual(notationSkeleton("再建"), notationSkeleton("債権"))
    }
}
