import Foundation
import XCTest
@testable import azookey_server

final class HomophonesTests: XCTestCase {
    let tables = parseHomophoneTables(phrases: """
    # 替える前の連語\t替えた後の連語\t語の位置\t替える前の語\t替えた後の語\t読み
    当局の商用\t当局の慫慂\t3\t商用\t慫慂\tしょうよう
    砂鉄し\t蹉跌し\t0\t砂鉄\t蹉跌\tさてつ
    戦争の基数\t戦争の帰趨\t3\t基数\t帰趨\tきすう
    """, triggers: """
    # しきい値 6.0
    pair\tいっかつ\t一喝\t一括\t-2.0
    feat\t悪\t4.0
    feat\t請求\t-5.0
    feat\t$と\t2.0
    feat\t$「悪を\t4.0
    """)

    func replaced(_ left: String, _ output: String, _ reading: String, declined: Set<String> = []) -> String {
        applyingHomophones(output, homophoneReplacements(left: left, output: output, reading: reading, tables: tables, declined: declined))
    }

    func testTablesAreRead() {
        XCTAssertEqual(tables.phrases.count, 3)
        XCTAssertEqual(tables.pairs[HomophoneKey("一括")!]?.pairs.first?.features.count, 2)
        XCTAssertEqual(tables.pairs[HomophoneKey("一括")!]?.pairs.first?.suffixes.count, 2)
        XCTAssertEqual(tables.threshold, 6.0)
        XCTAssertEqual(tables.longestWord, 2)
        XCTAssertEqual(tables.replacements(of: "商用"), ["慫慂"])
        XCTAssertEqual(tables.replacements(of: "一括"), ["一喝"])
        // 鍵は字の並びごとに別 (長さの違う並び・9 字を超える並び)
        XCTAssertNotEqual(HomophoneKey("あ"), HomophoneKey("ああ"))
        XCTAssertNotEqual(HomophoneKey("一二三四"), HomophoneKey("一二三五"))
        XCTAssertNil(HomophoneKey("一二三四五六七八九十"))
    }

    func testPhraseReplacesOnlyTheWordInTheConvertedText() {
        // 連語の前の部分 (当局の) は左の文脈にあってよい。替えるのは変換した文字列の中の語だけ
        XCTAssertEqual(replaced("当局の", "商用による企業統合で", "しょうようによるきぎょうとうごうで"), "慫慂による企業統合で")
        XCTAssertEqual(replaced("", "当局の商用による", "とうきょくのしょうようによる"), "当局の慫慂による")
        XCTAssertEqual(replaced("当局の商用", "による", "による"), "による")
        // 打った読みに語の読みが無ければ替えない
        XCTAssertEqual(replaced("当局の", "商用による", "しよう"), "商用による")
        // 前後に漢字が続く (長い語の一部) なら替えない
        XCTAssertEqual(replaced("当局の", "商用車で", "しょうようしゃで"), "商用車で")
        XCTAssertEqual(replaced("計画が", "黒砂鉄した", "くろさてつした"), "黒砂鉄した")
    }

    func testSuffixOnlyRowsSkipParticles() {
        XCTAssertEqual(replaced("計画が", "砂鉄したら", "さてつしたら"), "蹉跌したら")
        // 後ろの字だけの行 (砂鉄し) は、し・す・さ・せ の後ろが しか・しも・さえ… のとき替えない (砂鉄しか取れない)
        XCTAssertEqual(replaced("", "砂鉄しか取れない", "さてつしかとれない"), "砂鉄しか取れない")
        XCTAssertEqual(replaced("", "砂鉄しも", "さてつしも"), "砂鉄しも")
        // 文の終わりで切れていれば替える
        XCTAssertEqual(replaced("計画が", "砂鉄し", "さてつし"), "蹉跌し")
    }

    func testTriggersAddWeightsOnceAndCompareWithTheThreshold() {
        // 事前 -2 + 悪 4 + 文脈の終わりの と 2 = 4 < 6
        XCTAssertEqual(replaced("悪と", "一括した", "いっかつした"), "一括した")
        // 同じ目印は 1 回だけ (悪 を 2 回数えると 8 になる)
        XCTAssertEqual(replaced("悪悪と", "一括した", "いっかつした"), "一括した")
        // -2 + 悪 4 + 「悪を 4 = 6 (しきい値ちょうどは替える)
        XCTAssertEqual(replaced("「悪を", "一括した", "いっかつした"), "一喝した")
        // 文脈のどこかにある目印は負の重みも効く (請求 -5)
        XCTAssertEqual(replaced("請求の「悪を", "一括した", "いっかつした"), "一括した")
        // 目印は語の前 120 字まで (それより前の 請求 は見ない)
        XCTAssertEqual(replaced("請求" + String(repeating: "あ", count: 116) + "「悪を", "一括した", "いっかつした"), "一喝した")
        XCTAssertEqual(replaced("請求" + String(repeating: "あ", count: 115) + "「悪を", "一括した", "いっかつした"), "一括した")
        // 変換した文字列の中の、語より前の字も文脈 (「悪を|一括した)
        XCTAssertEqual(replaced("", "「悪を一括した", "「あくをいっかつした"), "「悪を一喝した")
        // 読みが無ければ替えない
        XCTAssertEqual(replaced("「悪を", "一括した", "いっかした"), "一括した")
    }

    func testScalarRowsFollowTheSameRules() {
        let text = "# 説明\na\tb\n\nc\td\te\r\nf\u{7}\tg\nh\t\n" + String(repeating: "x", count: 65) + "\ty\n"
        var rows: [[String]] = []
        BoundedTable.scalarRows(text: text, maxLines: 10, columns: 2...2, maxFieldLength: 64) { rows.append($0.map { homophoneString($0) }) }
        XCTAssertEqual(rows, [["a", "b"], ["h", ""]])
        XCTAssertEqual(rows, BoundedTable.rows(text: text, maxLines: 10, columns: 2...2, maxFieldLength: 64))
        var limited = 0
        BoundedTable.scalarRows(text: text, maxLines: 1, columns: 2...3, maxFieldLength: 64) { _ in limited += 1 }
        XCTAssertEqual(limited, 1)
    }

    func testNumberFieldsReadLikeDoubleOfString() {
        for text in ["-3.2966", "4.0000", "0.4439", "-0.0445", "2.4279", "6", "+1.5", "-8.5795", "123456789012.123", "1e3", "0.1234567890123456789"] {
            XCTAssertEqual(homophoneDouble(Array(text.unicodeScalars)[...]), Double(text), text)
        }
        XCTAssertNil(homophoneDouble(Array("-".unicodeScalars)[...]))
        XCTAssertNil(homophoneDouble(Array("1.2.3".unicodeScalars)[...]))
        XCTAssertEqual(homophoneInt(Array("12".unicodeScalars)[...]), 12)
        XCTAssertNil(homophoneInt(Array("-1".unicodeScalars)[...]))
    }

    func testPythonSumCompensation() {
        // 手本 (Python 3.12 の sum) と同じ。ふつうに足すと 0 になる
        XCTAssertEqual(homophoneSum([1e16, 1, -1e16]), 1)
        XCTAssertEqual(homophoneSum([]), 0)
        XCTAssertEqual(homophoneSum([0.1, 0.2, 0.3]), 0.6)
    }

    func testSegmentsKeepRubyCountsAndMergeOnlyWhenTheWordIsSplit() {
        let within = replacingHomophones([(word: "戦争の", rubyCount: 5), (word: "基数を", rubyCount: 4), (word: "見守る", rubyCount: 4)],
                                         reading: "せんそうのきすうをみまもる", context: "", tables: tables)
        XCTAssertEqual(within.segments.map(\.word), ["戦争の", "帰趨を", "見守る"])
        XCTAssertEqual(within.segments.map(\.rubyCount), [5, 4, 4])
        // AI が 基|数を と切っていたら、その 2 つの文節を 1 つにまとめる (読みの字数の合計は同じ)
        let split = replacingHomophones([(word: "戦争の", rubyCount: 5), (word: "基", rubyCount: 1), (word: "数を", rubyCount: 3), (word: "見守る", rubyCount: 4)],
                                        reading: "せんそうのきすうをみまもる", context: "", tables: tables)
        XCTAssertEqual(split.segments.map(\.word), ["戦争の", "帰趨を", "見守る"])
        XCTAssertEqual(split.segments.map(\.rubyCount), [5, 4, 4])
        // 替える所が無ければそのまま (空の文節も)
        let none = replacingHomophones([(word: "", rubyCount: 0), (word: "基数を", rubyCount: 4)], reading: "きすうを", context: "", tables: tables)
        XCTAssertEqual(none.segments.map(\.word), ["", "基数を"])
        XCTAssertTrue(none.replacements.isEmpty)
    }

    func testCandidateListPutsTheReplacedFormFirstAndKeepsTheOriginalSecond() {
        let ordered = orderingHomophones(["商用による", "賞用による", "慫慂による", "しょうようによる"], reading: "しょうようによる", context: "当局の", tables: tables)
        XCTAssertEqual(ordered.texts, ["慫慂による", "商用による", "賞用による", "しょうようによる"])
        XCTAssertEqual(ordered.replacements.map(\.replacement), ["慫慂"])
        // 1 番目を替えないなら並びはそのまま
        XCTAssertEqual(orderingHomophones(["賞用による", "商用による"], reading: "しょうようによる", context: "当局の", tables: tables).texts,
                       ["賞用による", "商用による"])
    }

    func testDeclinedPairsAreNotReplaced() {
        XCTAssertEqual(replaced("当局の", "商用による", "しょうようによる", declined: ["しょうよう\t商用"]), "商用による")
        XCTAssertEqual(replaced("「悪を", "一括した", "いっかつした", declined: ["いっかつ\t一括"]), "一括した")
        // 替えた語を元の語に戻して確定したら、その組を覚える。替えた語のまま確定したら何もしない
        let offered: Set<String> = ["しょうよう\t商用\t慫慂"]
        XCTAssertEqual(declinedHomophones(reading: "しょうようによる", text: "商用による", offered: offered), ["しょうよう\t商用"])
        XCTAssertTrue(declinedHomophones(reading: "しょうようによる", text: "慫慂による", offered: offered).isEmpty)
        // 元の語が長い語の一部 (商用車) なら戻したことにしない
        XCTAssertTrue(declinedHomophones(reading: "しょうようしゃ", text: "商用車", offered: offered).isEmpty)
        // 前に戻した組でも、替えた後の語を確定したらまた替える
        XCTAssertEqual(acceptedHomophones(reading: "しょうようによる", text: "慫慂による", declined: ["しょうよう\t商用", "さてつ\t砂鉄"], tables: tables),
                       ["しょうよう\t商用"])
    }

    func testForgettingLearningAlsoForgetsDeclinedPairs() {
        let declined: Set<String> = ["しょうよう\t商用", "かいしゃ\t会社", "いっかつ\t一括"]
        // 候補の一覧で忘れた候補 (Ctrl+Delete) に読みと元の語が入っている組
        XCTAssertEqual(forgottenHomophones(declined: declined, reading: "しょうようによる", text: "商用による"), ["しょうよう\t商用"])
        XCTAssertTrue(forgottenHomophones(declined: declined, reading: "しょうようによる", text: "慫慂による").isEmpty)
        XCTAssertTrue(forgottenHomophones(declined: declined, reading: "しょうようしゃ", text: "商用車").isEmpty)
        // 学習から外す語 (learning_ng.txt) を含む元の語の組
        XCTAssertEqual(forgottenHomophones(declined: declined, discarded: ["会", ""]), ["かいしゃ\t会社"])
        XCTAssertTrue(forgottenHomophones(declined: declined).isEmpty)
    }

    /// 配る表 (リポジトリの直下) で、表を作った側の手本 (ref_apply.py) と同じ答えになる例
    func testPublishedTables() throws {
        let root = URL(filePath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let phraseText = try String(contentsOf: root.appendingPathComponent("homophone_phrases.tsv"), encoding: .utf8)
        let triggerText = try String(contentsOf: root.appendingPathComponent("homophone_triggers.tsv"), encoding: .utf8)
        var rows = 0
        let reading = Date()
        for text in [phraseText, triggerText] {
            BoundedTable.scalarRows(text: text, maxLines: homophoneLimits.maxLines, columns: 3...6, maxFieldLength: homophoneLimits.maxFieldLength) { _ in rows += 1 }
        }
        let readMs = Int(-reading.timeIntervalSinceNow * 1000)
        let start = Date()
        let published = parseHomophoneTables(phrases: phraseText, triggers: triggerText)
        print("homophone tables: \(Int(-start.timeIntervalSinceNow * 1000)) ms (rows only \(readMs) ms, \(rows) rows), \(published.phrases.count) words, \(published.pairs.count) trigger words")
        XCTAssertEqual(published.phrases.values.reduce(0) { $0 + $1.rules.count }, 64_273)
        XCTAssertEqual(published.pairs.values.reduce(0) { $0 + $1.pairs.count }, 79)
        XCTAssertEqual(published.threshold, 6.0)
        let cases: [(left: String, output: String, reading: String, expected: String)] = [
            ("計画が", "砂鉄した", "さてつした", "蹉跌した"),
            ("計画が", "砂鉄しか", "さてつしか", "砂鉄しか"),
            ("当局の", "商用を受けて", "しょうようをうけて", "慫慂を受けて"),
            ("限界生産性は", "低減する", "ていげんする", "逓減する"),
            ("陸軍の学校で試験を", "委託した", "いたくした", "依託した"),
            ("運営を", "委託した", "いたくした", "委託した"),
            ("人質が", "解放された", "かいほうされた", "解放された"),
            ("戦争の", "基数を見守る", "きすうをみまもる", "帰趨を見守る"),
            ("人口に", "会社している", "かいしゃしている", "膾炙している"),
            ("それは", "演技が良い", "えんぎがよい", "演技が良い"),
            ("悪を", "一括する", "いっかつする", "一喝する"),
            ("代金を", "一括して払う", "いっかつしてはらう", "一括して払う"),
            // 読みが JMdict に無い当て字の行 (白毛 しらが・旗下 はたもと) は外したので、一般語のまま
            ("", "最初の白髪", "さいしょのしらが", "最初の白髪"),
            ("", "旗本の武将", "はたもとのぶしょう", "旗本の武将"),
        ]
        for c in cases {
            XCTAssertEqual(applyingHomophones(c.output, homophoneReplacements(left: c.left, output: c.output, reading: c.reading, tables: published)),
                           c.expected, c.left + "|" + c.output)
        }
        // 速さ: 打つたびに走るので、ほとんど時間を足さないこと (ふつうの文で 1 回 1ms 未満)
        let sentences = [("", "昨日は会社の会議で新しい計画について話し合い、その結果を報告した", "きのうはかいしゃのかいぎであたらしいけいかくについてはなしあい、そのけっかをほうこくした"),
                         ("当局の", "商用による企業統合で解放された人々の保護が問題になった", "しょうようによるきぎょうとうごうでかいほうされたひとびとのほごがもんだいになった")]
        let loops = 200
        let timer = Date()
        for _ in 0..<loops {
            for s in sentences { _ = homophoneReplacements(left: s.0, output: s.1, reading: s.2, tables: published) }
        }
        let perCall = -timer.timeIntervalSinceNow * 1000 / Double(loops * sentences.count)
        print("homophone replacements: \(String(format: "%.3f", perCall)) ms per call")
        XCTAssertLessThan(perCall, 5)
    }

    /// 表を作り直したときの確かめの入口: AZK_TEST_HOMOPHONE_EVAL=<cases.tsv>;<probe.jsonl>;<out.jsonl> (AZK_TEST_HOMOPHONE_TABLES=表のフォルダ。無ければリポジトリの直下)
    /// cases.tsv: 読み<TAB>左の文脈<TAB>正しい語<TAB>相手の語<TAB>出どころ (wiki_H は一般語の文)。probe.jsonl: {"full": 今の変換の結果} を 1 行ずつ。
    /// out.jsonl に差し替えたあとの結果と理由を書き、手本 (ref_apply.py) と同じ数え方で 直る・壊す を出す
    func testEvaluateProbeResults() throws {
        let env = ProcessInfo.processInfo.environment
        guard let spec = env["AZK_TEST_HOMOPHONE_EVAL"] else { throw XCTSkip("Run with cases, probe results and an output path") }
        let paths = spec.split(separator: ";").map(String.init)
        XCTAssertEqual(paths.count, 3)
        let root = env["AZK_TEST_HOMOPHONE_TABLES"].map { URL(filePath: $0) }
            ?? URL(filePath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let published = parseHomophoneTables(phrases: try String(contentsOf: root.appendingPathComponent("homophone_phrases.tsv"), encoding: .utf8),
                                             triggers: try String(contentsOf: root.appendingPathComponent("homophone_triggers.tsv"), encoding: .utf8))
        let cases = try String(contentsOfFile: paths[0], encoding: .utf8).split(whereSeparator: \.isNewline).filter { !$0.hasPrefix("#") && !$0.trimmingCharacters(in: .whitespaces).isEmpty }
            .map { $0.split(separator: "\t", omittingEmptySubsequences: false).map(String.init) }
        let results = try String(contentsOfFile: paths[1], encoding: .utf8).split(whereSeparator: \.isNewline).compactMap { line in
            (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any]
        }
        var counts: [String: Int] = [:]
        var lines: [String] = []
        let timer = Date()
        for (c, r) in zip(cases, results) {
            guard c.count >= 5, let full = r["full"] as? String else { continue }
            let found = homophoneReplacements(left: c[1], output: full, reading: c[0], tables: published)
            let new = applyingHomophones(full, found)
            let tag = c[4] == "wiki_H" ? "一般語の文" : "難読語の文"
            let before = full.hasPrefix(c[2]), after = new.hasPrefix(c[2])
            let kind = before && after ? "もとから正しい" : after ? "直る" : before ? "壊す" : new != full ? "誤りのまま(替えた)" : "誤りのまま"
            counts[tag + "\t" + kind, default: 0] += 1
            let data = try JSONSerialization.data(withJSONObject: ["full": new, "why": found.map(\.reason)], options: [.sortedKeys])
            lines.append(String(decoding: data, as: UTF8.self))
        }
        print("homophone eval: \(lines.count) lines, \(String(format: "%.3f", -timer.timeIntervalSinceNow * 1000 / Double(max(1, lines.count)))) ms per line")
        for key in counts.keys.sorted() { print("homophone eval\t\(key)\t\(counts[key]!)") }
        try (lines.joined(separator: "\n") + "\n").write(toFile: paths[2], atomically: true, encoding: .utf8)
    }
}
