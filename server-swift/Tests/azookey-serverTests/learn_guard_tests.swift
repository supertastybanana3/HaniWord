import XCTest
@testable import azookey_server

/// 2026-11-20 (学習の効きすぎ): L2 の危ない要素の決めと、L1 の覚える単位のまとめ方
final class LearnGuardTests: XCTestCase {
    /// 本体の辞書の代わり: 規則・永田・志賀・滋賀・教室 は 1 語で載っている。規則賀 は載っていない
    let dictionary: Set<String> = ["キソク\t規則", "ナガタ\t永田", "シガ\t志賀", "シガ\t滋賀", "キョウシツ\t教室", "エイガ\t映画"]
    func inDictionary(_ ruby: String, _ word: String) -> Bool { dictionary.contains(ruby + "\t" + word) }
    let rare: Set<String> = ["きょうしつ\t狂疾"]

    func risky(_ word: String, _ ruby: String, hyouki: Set<String>? = ["事"]) -> Bool {
        isRiskyLearnedElement(word: word, ruby: ruby, rare: rare, hyouki: hyouki, inDictionary: inDictionary)
    }

    func testRiskyElements() {
        // (A) 助詞の読みの 1 語
        XCTAssertTrue(risky("賀", "ガ"))
        XCTAssertTrue(risky("値", "ネ"))
        XCTAssertFalse(risky("が", "ガ"))
        // (B) 難読語のまれな側。つないだ要素は使う
        XCTAssertTrue(risky("狂疾", "キョウシツ"))
        XCTAssertFalse(risky("狂疾に陥る", "キョウシツニオチイル"))
        XCTAssertFalse(risky("教室", "キョウシツ"))
        // (C) 規則賀 型。辞書の 1 語 (志賀・滋賀・映画) と、残りが 1 字 (古賀) は使う
        XCTAssertTrue(risky("規則賀", "キソクガ"))
        XCTAssertTrue(risky("永田賀", "ナガタガ"))
        XCTAssertFalse(risky("志賀", "シガ"))
        XCTAssertFalse(risky("滋賀", "シガ"))
        XCTAssertFalse(risky("映画", "エイガ"))
        XCTAssertFalse(risky("古賀", "コガ"))
        // 2026-11-21: 文節が「名詞＋助詞」のとき覚えた 狂疾に も危ない。教室に は危なくない
        XCTAssertTrue(risky("狂疾に", "キョウシツニ"))
        XCTAssertFalse(risky("教室に", "キョウシツニ"))
        // 表記の書き分けの組の字は (A)・(C) では危なくない
        XCTAssertFalse(risky("事", "コト"))
        // 2026-11-22: (B) は字だけで外さない (痴呆 は書き分けの組に載るが、勝つ語 地方 とは別の語)
        XCTAssertTrue(isRiskyLearnedElement(word: "痴呆", ruby: "チホウ", rare: ["ちほう\t痴呆"], hyouki: ["痴呆"], inDictionary: inDictionary))
        // 表がまだ読めていなければ (B) と書き分けの除外はかけない
        XCTAssertFalse(isRiskyLearnedElement(word: "狂疾", ruby: "キョウシツ", rare: nil, hyouki: nil, inDictionary: inDictionary))
    }

    func units(_ readings: [String], _ texts: [String]) -> [String] {
        learningUnits(readings: readings, texts: texts, rare: rare, enabled: true).map { u in
            "\(u.text)\(u.kind == .functionAlone || u.kind == .rareAlone ? "(1 語)" : u.merged ? "(つなぐ)" : "")"
        }
    }

    func testLearningUnits() {
        XCTAssertEqual(units(["し", "が"], ["志", "賀"]), ["志賀(つなぐ)"])
        XCTAssertEqual(units(["きそく", "が"], ["規則", "賀"]), ["規則賀(つなぐ)"])
        XCTAssertEqual(units(["が"], ["賀"]), ["賀(1 語)"])
        XCTAssertEqual(units(["きょうしつ", "に", "おちいる"], ["狂疾", "に", "陥る"]), ["狂疾に陥る(つなぐ)"])
        XCTAssertEqual(units(["きょうしつ", "に"], ["狂疾", "に"]), ["狂疾(1 語)", "に"])
        XCTAssertEqual(learningUnits(readings: ["し", "が"], texts: ["志", "賀"], rare: rare, enabled: true).first?.partTexts, ["志", "賀"])
        XCTAssertEqual(learningUnits(readings: ["きょうしつ", "に", "おちいる"], texts: ["狂疾", "に", "陥る"], rare: rare, enabled: true).first?.joinable, true)
        XCTAssertEqual(units(["きょうしつ", "に", "いく"], ["教室", "に", "行く"]), ["教室", "に", "行く"])
        // 2026-11-21: 文節が「名詞＋助詞」の区切り (狂疾に｜陥る) でも、難読語を右の文節までつなぐ。後ろが無ければ 1 語で
        XCTAssertEqual(units(["きょうしつに", "おちいる"], ["狂疾に", "陥る"]), ["狂疾に陥る(つなぐ)"])
        XCTAssertEqual(units(["きょうしつに"], ["狂疾に"]), ["狂疾に(1 語)"])
        XCTAssertEqual(units(["きょうしつに", "いく"], ["教室に", "行く"]), ["教室に", "行く"])
        XCTAssertEqual(units(["きそくが"], ["規則が"]), ["規則が"])
        // 助詞のかなのままの文節はそのまま
        XCTAssertEqual(units(["きそく", "が"], ["規則", "が"]), ["規則", "が"])
        // つまみで切れば前の形
        XCTAssertEqual(learningUnits(readings: ["が"], texts: ["賀"], rare: rare, enabled: false).map(\.kind), [.normal])
    }

    /// 2026-11-21: L2 で外した学習の語を、一覧の 2 番目に上げる (1 番目は変えない。2 番目までにあればそのまま)
    func testPromotingDroppedLearned() {
        let dropped = LearnGuardDropped()
        dropped.note(ruby: "キョウシツ", word: "狂疾")
        dropped.note(ruby: "ガ", word: "賀")
        let kyou = dropped.words(matching: "キョウシツニイク")
        XCTAssertEqual(promotingDroppedLearned(["教室に行く", "教室にいく", "強室に行く", "狂疾に行く"], dropped: kyou),
                       ["教室に行く", "狂疾に行く", "教室にいく", "強室に行く"])
        XCTAssertEqual(promotingDroppedLearned(["教室に行く", "狂疾に行く", "強室に行く"], dropped: kyou), ["教室に行く", "狂疾に行く", "強室に行く"])
        // 1 字の読みの語は、読み全体と同じときだけ
        XCTAssertEqual(dropped.words(matching: "ガ").map(\.word), ["賀"])
        XCTAssertTrue(dropped.words(matching: "ガイコク").isEmpty)
        XCTAssertEqual(promotingDroppedLearned(["が", "我", "蛾", "賀"], dropped: dropped.words(matching: "ガ")), ["が", "賀", "我", "蛾"])
        dropped.clear()
        XCTAssertTrue(dropped.words(matching: "キョウシツ").isEmpty)
    }

    /// 2026-11-22: 難読語の一覧を読むとき、語と勝つ語が同じ読みの同じ書き分けの組にある行 (充分・十分) だけ除く
    func testRareRowsExcludeNotationVariants() {
        let hyouki = parseLearnGuardHyoukiGroups("じゅうぶん\t1\t1\t十分|充分\tx\n" + "ちほう\t1\t1\t痴呆|痴ほう\tx\n")
        let rare = parseLearnGuardRare("じゅうぶん\t充分\t十分\t1\t1\n" + "ちほう\t痴呆\t地方\t1\t1\n", hyoukiGroups: hyouki)
        XCTAssertEqual(rare, ["ちほう\t痴呆"])
    }

    /// 2026-11-22: 表を読み終えた直後の 1 回だけ真
    func testJustLoadedIsConsumedOnce() {
        let tables = LearnGuardTables()
        XCTAssertFalse(tables.loaded)
        XCTAssertFalse(tables.consumeJustLoaded())
        tables.set(rare: ["きょうしつ\t狂疾"], hyouki: [])
        XCTAssertTrue(tables.loaded)
        XCTAssertTrue(tables.consumeJustLoaded())
        XCTAssertFalse(tables.consumeJustLoaded())
    }
}
