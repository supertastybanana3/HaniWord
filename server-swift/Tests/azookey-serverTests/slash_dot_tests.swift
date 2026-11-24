import KanaKanjiConverterModule
import XCTest
@testable import azookey_server

/// 2026-11-05 (仕事 22 の 2): 変換するときの / と 。 の決め直し (slash_dot.swift)
@MainActor
final class SlashDotTests: XCTestCase {
    private func segs(_ parts: [(String, Int)]) -> [(word: String, rubyCount: Int)] { parts.map { (word: $0.0, rubyCount: $0.1) } }
    private func text(_ s: [(word: String, rubyCount: Int)]) -> String { s.map(\.word).joined() }

    /// 名詞 (資料・以上) は一般名詞、だよ の よ は助詞、する は動詞として引ける辞書の代わり
    private let pos: SlashDotPartOfSpeech = { reading, word, _ in
        switch word {
        case "資料", "議事録", "レポート": return (1285, 1285)
        // 以上 は辞書では名詞の非自立に近い番号 (1313) で、名詞として扱わない。体言止めの見分けを試すため、ここでは一般名詞として引く
        case "以上": return (1285, 1285)
        case "だよ", "です", "ね": return (300, 300)
        case "する": return (600, 600)
        default: return nil
        }
    }

    func testPeriodBeforeAnExtensionAfterANounBecomesAFileName() {
        // しりょう。pdfは → 資料.pdfは
        let out = decidingSlashDot(segs([("資料", 4), ("。", 1), ("pdfは", 4)]), reading: "しりょう。pdfは", context: "", decide: true, caseFix: true, partOfSpeech: pos)
        XCTAssertEqual(text(out), "資料.pdfは")
        // 文の終わりの形 (だよ) のあとは 。 のまま、拡張子は表の形
        let end = decidingSlashDot(segs([("これが", 3), ("まとめだよ", 5), ("。", 1), ("pdfは", 4)]), reading: "これがまとめだよ。pdfは", context: "", decide: true, caseFix: true,
                                   partOfSpeech: { r, w, l in w == "まとめだよ" ? (300, 300) : nil })
        XCTAssertEqual(text(end), "これがまとめだよ。PDFは")
        // 体言止め (結論は以上。) は 迷う形 として 。 のまま
        let noun = decidingSlashDot(segs([("結論は", 5), ("以上", 4), ("。", 1), ("pdfは", 4)]), reading: "けつろんはいじょう。pdfは", context: "", decide: true, caseFix: true, partOfSpeech: pos)
        XCTAssertEqual(text(noun), "結論は以上。PDFは")
        // 動詞のあと・引けない語のあとも 。 のまま
        XCTAssertEqual(text(decidingSlashDot(segs([("する", 2), ("。", 1), ("pdf", 3)]), reading: "する。pdf", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "する。PDF")
    }

    func testTypingPreviewAndOtherShapesStay() {
        // 打っている途中の仮表示は決め直さない (拡張子の形だけ直す)
        let preview = decidingSlashDot(segs([("資料", 4), ("。", 1), ("pdfは", 4)]), reading: "しりょう。pdfは", context: "", decide: false, caseFix: true, partOfSpeech: pos)
        XCTAssertEqual(text(preview), "資料。PDFは")
        // 表記直しがオフなら小文字のまま
        XCTAssertEqual(text(decidingSlashDot(segs([("資料", 4), ("。", 1), ("pdfは", 4)]), reading: "しりょう。pdfは", context: "", decide: false, caseFix: false, partOfSpeech: pos)), "資料。pdfは")
        // 大文字で打った (読みに大文字で入った) 拡張子・拡張子でない語 (pdfs) は触らない。後ろがカタカナ (pdfファイル) なら決めない (表の形にはする)
        XCTAssertEqual(text(decidingSlashDot(segs([("資料", 4), ("。", 1), ("PDFは", 4)]), reading: "しりょう。PDFは", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "資料。PDFは")
        XCTAssertEqual(text(decidingSlashDot(segs([("資料", 4), ("。", 1), ("pdfファイル", 7)]), reading: "しりょう。pdfふぁいる", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "資料。PDFファイル")
        XCTAssertEqual(text(decidingSlashDot(segs([("資料", 4), ("。", 1), ("pdfs", 4)]), reading: "しりょう。pdfs", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "資料。pdfs")
        // 後ろが漢字の語 (pdf回収) は決めない (表の形にはする)。拡張子だけの文節のあとの助詞の文節は見る
        XCTAssertEqual(text(decidingSlashDot(segs([("資料", 4), ("。", 1), ("pdf回収", 8)]), reading: "しりょう。pdfかいしゅう", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "資料。PDF回収")
        XCTAssertEqual(text(decidingSlashDot(segs([("資料", 4), ("。", 1), ("pdf", 3), ("を", 1)]), reading: "しりょう。pdfを", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "資料.pdfを")
        // 文節の数え方が読みと合わないときは何もしない
        XCTAssertEqual(text(decidingSlashDot(segs([("資料", 3), ("。", 1), ("pdf", 3)]), reading: "しりょう。pdf", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "資料。pdf")
    }

    func testLatinPairsFromTheTableBecomeSlash() {
        // TCP・IP (IME 本体は略語のあとを ・ で入れる) → TCP/IP。表に無い組 (CT・MRI)・数字が続く組・打っている途中は ・ のまま
        XCTAssertEqual(text(decidingSlashDot(segs([("TCP", 3), ("・", 1), ("IPの", 3)]), reading: "TCP・IPの", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "TCP/IPの")
        XCTAssertEqual(text(decidingSlashDot(segs([("CT", 2), ("・", 1), ("MRI", 3)]), reading: "CT・MRI", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "CT・MRI")
        XCTAssertEqual(text(decidingSlashDot(segs([("TCP", 3), ("・", 1), ("IP", 2)]), reading: "TCP・IP", context: "", decide: false, caseFix: true, partOfSpeech: pos)), "TCP・IP")
        XCTAssertEqual(text(decidingSlashDot(segs([("国語", 3), ("・", 1), ("数学", 4)]), reading: "こくご・すうがく", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "国語・数学")
    }

    /// 2026-11-05 (仕事 22 の 3): 記号 1 字の文節は一覧にいつもほかの形。選び直した形は前後の語の組で覚え、決めに戻したら忘れる
    func testAlternativesAndChoiceLearning() {
        XCTAssertEqual(appendingSlashDotAlternatives(["・", "･"], reading: "・"), ["・", "/", "／", "･"])
        XCTAssertEqual(appendingSlashDotAlternatives(["。"], reading: "。"), ["。", "."])
        XCTAssertEqual(appendingSlashDotAlternatives(["国語"], reading: "こくご"), ["国語"])
        // 組の形
        XCTAssertEqual(slashDotLearningKey(texts: ["賛成", "・", "反対"], index: 1, reading: "・", math: false), "jj\t賛成\t反対")
        XCTAssertEqual(slashDotLearningKey(texts: ["4", "/", "5千人"], index: 1, reading: "/", math: false), "dd\t千人")
        XCTAssertEqual(slashDotLearningKey(texts: ["CT", "・", "MRIで"], index: 1, reading: "・", math: false), "ll\tCT\tMRI")
        XCTAssertEqual(slashDotLearningKey(texts: ["資料", "。", "PDFは"], index: 1, reading: "。", math: false), "dot\t資料\tpdf")
        XCTAssertEqual(slashDotLearningKey(texts: ["x", "/", "y"], index: 1, reading: "/", math: true), "math:ll\tx\ty")
        XCTAssertNil(slashDotLearningKey(texts: ["・", "反対"], index: 0, reading: "・", math: false))
        // 覚える・かける・忘れる
        var learned = SlashDotLearned()
        XCTAssertTrue(noteSlashDotChoice(&learned, key: "jj\t賛成\t反対", chosen: "/", defaultForm: "・"))
        XCTAssertFalse(noteSlashDotChoice(&learned, key: "jj\t賛成\t反対", chosen: "/", defaultForm: "・"))
        XCTAssertEqual(parseSlashDotLearned(formatSlashDotLearned(learned)), learned)
        let applied = applyingSlashDotLearned(segs([("賛成", 4), ("・", 1), ("反対", 4)]), reading: "さんせい・はんたい", learned: learned, math: false)
        XCTAssertEqual(text(applied.segments), "賛成/反対")
        XCTAssertEqual(applied.defaults, [4: "・"])
        // ほかの組・数学の文には効かない
        XCTAssertEqual(text(applyingSlashDotLearned(segs([("国語", 3), ("・", 1), ("数学", 4)]), reading: "こくご・すうがく", learned: learned, math: false).segments), "国語・数学")
        XCTAssertEqual(text(applyingSlashDotLearned(segs([("賛成", 4), ("・", 1), ("反対", 4)]), reading: "さんせい・はんたい", learned: learned, math: true).segments), "賛成・反対")
        XCTAssertTrue(noteSlashDotChoice(&learned, key: "jj\t賛成\t反対", chosen: "・", defaultForm: "・"))
        XCTAssertEqual(learned, SlashDotLearned())
        // 上限を越えたら古い組から消す
        for k in 0..<(slashDotLearnedLimit + 3) { _ = noteSlashDotChoice(&learned, key: "jj\t\(k)\tx", chosen: "/", defaultForm: "・") }
        XCTAssertEqual(learned.order.count, slashDotLearnedLimit)
        XCTAssertNil(learned.forms["jj\t0\tx"])
    }

    /// 2026-11-05 (まとめ役 14:51): 数学の文では、英字 1 字・数字・閉じ括弧のすぐ後ろの区切りを / に。ふつうの文・略語のあと・かなのあとは今までどおり
    func testMathSentencesPutSlashFirst() {
        let signals = DomainBoost(boost: nil, signals: "関数\nベクトル\n")
        XCTAssertTrue(isMathSentence(context: "", text: "関数のA・Bを", signals: signals))
        XCTAssertTrue(isMathSentence(context: "", text: "x+1=0のとき", signals: signals))
        XCTAssertFalse(isMathSentence(context: "", text: "国語・数学", signals: signals))
        XCTAssertFalse(isMathSentence(context: "", text: "関数のA・B", signals: nil))
        XCTAssertTrue(mathSlashLeft("A") && mathSlashLeft("2") && mathSlashLeft("(x+1)") && mathSlashLeft("のa"))
        XCTAssertFalse(mathSlashLeft("CT") || mathSlashLeft("国語") || mathSlashLeft(""))
        let typed = segs([("関数の", 5), ("A", 1), ("・", 1), ("B", 1)])
        XCTAssertEqual(text(decidingSlashDot(typed, reading: "かんすうのA・B", context: "", decide: true, caseFix: true, partOfSpeech: pos, math: true)), "関数のA/B")
        XCTAssertEqual(text(decidingSlashDot(typed, reading: "かんすうのA・B", context: "", decide: true, caseFix: true, partOfSpeech: pos, math: false)), "関数のA・B")
        XCTAssertEqual(text(decidingSlashDot(typed, reading: "かんすうのA・B", context: "", decide: false, caseFix: true, partOfSpeech: pos, math: true)), "関数のA・B")
        XCTAssertEqual(text(decidingSlashDot(segs([("CT", 2), ("・", 1), ("MRI", 3)]), reading: "CT・MRI", context: "", decide: true, caseFix: true, partOfSpeech: pos, math: true)), "CT・MRI")
        // 2026-11-06 (まとめ役 10:17): 同じ文か直前の文に 内積・ベクトル があれば、英字 1 字どうしの ・ は ・ のまま (内積 a・b)。ベクトルの無い数学の文の分数は /
        let inner = segs([("ベクトル", 4), ("a", 1), ("・", 1), ("bの", 2), ("内積", 4)])
        XCTAssertEqual(text(decidingSlashDot(inner, reading: "べくとるa・bのないせき", context: "", decide: true, caseFix: true, partOfSpeech: pos, math: true)), "ベクトルa・bの内積")
        let product = segs([("a", 1), ("・", 1), ("bを", 2), ("求めよ", 4)])
        XCTAssertEqual(text(decidingSlashDot(product, reading: "a・bをもとめよ", context: "2つのベクトルがある。", decide: true, caseFix: true, partOfSpeech: pos, math: true)), "a・bを求めよ")
        let fraction = segs([("分数", 4), ("a", 1), ("・", 1), ("bを", 2), ("約分", 4)])
        XCTAssertEqual(text(decidingSlashDot(fraction, reading: "ぶんすうa・bをやくぶん", context: "", decide: true, caseFix: true, partOfSpeech: pos, math: true)), "分数a/bを約分")
        // 内積の文でも、数字・閉じ括弧のあと (2・3 → 2/3) と、英字 1 字どうしでない所は今までどおり
        let numbers = segs([("内積は", 5), ("2", 1), ("・", 1), ("3", 1)])
        XCTAssertEqual(text(decidingSlashDot(numbers, reading: "ないせきは2・3", context: "", decide: true, caseFix: true, partOfSpeech: pos, math: true)), "内積は2/3")
    }

    /// 2026-11-05 (仕事 28): いつも ・ か / (slash_key)・いつも 。 か . (period_key) のときはその記号を決め直さず、覚えた形もかけない。
    /// 数学の式の直し (math_expressions) は、書いていなければよく打つ分野に数学か物理があればオン
    func testSettingsStopOnlyTheirOwnDecision() {
        let grade = segs([("1", 1), ("/", 1), ("2年生", 5)])
        XCTAssertEqual(text(decidingSlashDot(grade, reading: "1/2ねんせい", context: "", decide: true, caseFix: true, partOfSpeech: pos, slashes: false)), "1/2年生")
        XCTAssertEqual(text(decidingSlashDot(grade, reading: "1/2ねんせい", context: "", decide: true, caseFix: true, partOfSpeech: pos, periods: false)), "1・2年生")
        let file = segs([("資料", 4), ("。", 1), ("pdfは", 4)])
        XCTAssertEqual(text(decidingSlashDot(file, reading: "しりょう。pdfは", context: "", decide: true, caseFix: true, partOfSpeech: pos, periods: false)), "資料。pdfは")
        XCTAssertEqual(text(decidingSlashDot(file, reading: "しりょう。pdfは", context: "", decide: true, caseFix: true, partOfSpeech: pos, slashes: false)), "資料.pdfは")
        var learned = SlashDotLearned()
        _ = noteSlashDotChoice(&learned, key: "jj\t賛成\t反対", chosen: "/", defaultForm: "・")
        let words = segs([("賛成", 4), ("・", 1), ("反対", 4)])
        XCTAssertEqual(text(applyingSlashDotLearned(words, reading: "さんせい・はんたい", learned: learned, math: false, families: "。.").segments), "賛成・反対")
        XCTAssertEqual(text(applyingSlashDotLearned(words, reading: "さんせい・はんたい", learned: learned, math: false, families: "・/／").segments), "賛成/反対")
        XCTAssertTrue(mathExpressionsOn(config: [:], focused: ["domain_math"]))
        XCTAssertTrue(mathExpressionsOn(config: [:], focused: ["domain_physics"]))
        XCTAssertFalse(mathExpressionsOn(config: [:], focused: ["domain_law"]))
        XCTAssertFalse(mathExpressionsOn(config: ["mathExpressions": false], focused: ["domain_math"]))
        XCTAssertTrue(mathExpressionsOn(config: ["mathExpressions": true], focused: []))
    }

    func testCandidateListAfterAKeptPeriodUsesTheTableForm() {
        XCTAssertEqual(dotExtensionCandidates(["pdfは", "PDFは", "ｐｄｆは"], reading: "pdfは", context: "資料。", caseFix: true), ["PDFは", "ｐｄｆは"])
        XCTAssertEqual(dotExtensionCandidates(["pdfは"], reading: "pdfは", context: "資料.", caseFix: true), ["pdfは"])
        XCTAssertEqual(dotExtensionCandidates(["pdfは"], reading: "pdfは", context: "資料。", caseFix: false), ["pdfは"])
        XCTAssertEqual(dotExtensionCandidates(["pdfs"], reading: "pdfs", context: "資料。", caseFix: true), ["pdfs"])
    }

    func testSlashBetweenDigitsStaysUnlessTheFollowingWordIsInTheTable() {
        for reading in ["1/2", "11/5に", "24/7", "2024/11/05", "1/2の"] {
            let parts = Array(reading).map { (word: String($0), rubyCount: 1) }
            XCTAssertEqual(text(decidingSlashDot(parts, reading: reading, context: "", decide: true, caseFix: true, partOfSpeech: pos)), reading, reading)
        }
        // 1/2ねんせい → 1・2年生、2/26じけん → 2・26事件 (変換した表記で照らす)。打っている途中は / のまま。迷う形 (3/4がつ = 3/4月) は / のまま
        let grade = segs([("1", 1), ("/", 1), ("2年生", 5)])
        XCTAssertEqual(text(decidingSlashDot(grade, reading: "1/2ねんせい", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "1・2年生")
        XCTAssertEqual(text(decidingSlashDot(grade, reading: "1/2ねんせい", context: "", decide: false, caseFix: true, partOfSpeech: pos)), "1/2年生")
        XCTAssertEqual(text(decidingSlashDot(segs([("2", 1), ("/", 1), ("26事件", 5)]), reading: "2/26じけん", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "2・26事件")
        XCTAssertEqual(text(decidingSlashDot(segs([("3", 1), ("/", 1), ("4月", 3)]), reading: "3/4がつ", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "3/4月")
        // 3 つ以上の並び (1・2・3丁目) も、最後の数字のあとで決める。日付 (2024/11/05に) は / のまま
        XCTAssertEqual(text(decidingSlashDot(segs([("1", 1), ("/", 1), ("2", 1), ("/", 1), ("3丁目", 5)]), reading: "1/2/3ちょうめ", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "1・2・3丁目")
        XCTAssertEqual(text(decidingSlashDot(segs([("2024", 4), ("/", 1), ("11", 2), ("/", 1), ("05に", 3)]), reading: "2024/11/05に", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "2024/11/05に")
        // 数字が別の形に変わったら照らさない
        XCTAssertEqual(text(decidingSlashDot(segs([("1", 1), ("/", 1), ("二年生", 5)]), reading: "1/2ねんせい", context: "", decide: true, caseFix: true, partOfSpeech: pos)), "1/二年生")
    }

    /// 品詞の番号の確かめ (辞書の置き場 AZK_TEST_DICTIONARY_STAGE があるときだけ。番号を書き出す)。
    /// 2026-11-05 の辞書: 資料・写真・版 1285、レポート・申請 1283、議事録・報告書 1288、東京 1293、田中 1290、さん 1302 (ここまで名詞)、
    /// 以上 1313・彼 1306・こと 1310・静か 1287 (名詞として扱わない)、だ 453・よ 288・です 460・ない 20・する 599
    func testPrintPartOfSpeechForProbeWords() async throws {
        guard let path = ProcessInfo.processInfo.environment["AZK_TEST_DICTIONARY_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        let root = URL(filePath: path)
        let converter = KanaKanjiConverter()
        let options = ConvertRequestOptions(requireJapanesePrediction: false, requireEnglishPrediction: false,
            keyboardLanguage: .ja_JP, learningType: .nothing, dictionaryResourceURL: root.appendingPathComponent("Dictionary"),
            memoryDirectoryURL: root.appendingPathComponent("memory"), sharedContainerURL: root.appendingPathComponent("shared"),
            textReplacer: .empty, metadata: nil)
        for (reading, word) in [("しりょう", "資料"), ("れぽーと", "レポート"), ("ぎじろく", "議事録"), ("ほうこくしょ", "報告書"), ("いじょう", "以上"),
                                ("しゃしん", "写真"), ("しんせい", "申請"), ("かれ", "彼"), ("こと", "こと"), ("しずか", "静か"), ("だよ", "だよ"), ("です", "です"),
                                ("ました", "ました"), ("ない", "ない"), ("する", "する"), ("とうきょう", "東京"), ("たなか", "田中"), ("さん", "さん"), ("ばん", "版")] {
            var input = ComposingText()
            input.insertAtCursorPosition(reading, inputStyle: .direct)
            let found = converter.dictionaryCandidate(input, matching: word, options: options)?.data
            print("SLASHDOTPOS \(reading) \(word) \(found.map { $0.map { "\($0.word):\($0.lcid)/\($0.rcid)" }.joined(separator: " ") } ?? "nil")")
        }
    }
}
