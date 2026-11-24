import XCTest
@testable import azookey_server

/// 2026-11-19: 部分を変換する前に、その部分のすぐ後ろの字を知らせる (終助詞の直しを文の終わりの前だけにするため)
final class LiteralSymbolFollowerTests: XCTestCase {
    func testPartFollowerIsTheNextSymbol() {
        var seen: [String] = []
        var follower: Character? = nil
        _ = preservingLiteralSymbols(reading: "おおきいさ（やく2ばい）、ちいさいね。それな", context: "", convert: { reading, _ in
            seen.append(reading + "|" + (follower.map(String.init) ?? "end"))
            return [(word: reading, rubyCount: reading.count)]
        }, partFollower: { follower = $0 })
        XCTAssertEqual(seen, ["おおきいさ|（", "やく2ばい|）", "ちいさいね|。", "それな|end"])
    }
}

final class LiteralSymbolsTests: XCTestCase {
    func testAllDirectionsBypassAutomaticConversion() {
        for arrow in ["←", "↓", "↑", "→", "←↓↑→", "＋−±×÷≠≤≥", "？！。、・", "「」『』（）[]", "♪♭♯√〒★", "↔︎", "☀️"] {
            let result = preservingLiteralSymbols(reading: arrow, context: "") { _, _ in
                XCTFail("Literal arrows must not reach the learned or AI converter")
                return [("→", 1)]
            }
            XCTAssertEqual(result?.map(\.word).joined(), arrow)
            XCTAssertEqual(result?.reduce(0) { $0 + $1.rubyCount }, arrow.count)
        }
    }

    func testTypedLatinKeepsItsCase() {
        let fixed = preservingTypedLatinCase(reading: "たて1m62", segments: [(word: "縦", rubyCount: 2), (word: "1M62", rubyCount: 4)])
        XCTAssertEqual(fixed.map(\.word), ["縦", "1m62"])
        XCTAssertEqual(fixed.map(\.rubyCount), [2, 4])
        // 英字の並びが違う (読みに無い英字を足した) ときと、かなから英字にしたとき (あい → AI) は変えない
        XCTAssertEqual(preservingTypedLatinCase(reading: "H2Oを", segments: [(word: "H₂Oを", rubyCount: 4)]).map(\.word), ["H₂Oを"])
        XCTAssertEqual(preservingTypedLatinCase(reading: "あい", segments: [(word: "AI", rubyCount: 2)]).map(\.word), ["AI"])
        XCTAssertEqual(preservingTypedLatinCase(reading: "GitHubの", segments: [(word: "Githubの", rubyCount: 7)]).map(\.word), ["GitHubの"])
    }

    func testSurroundingTextKeepsContextAndReadingLengths() {
        var contexts: [String] = []
        let result = preservingLiteralSymbols(reading: "さらに←みぎ→した", context: "文脈") { reading, left in
            contexts.append(left)
            return [(word: ["さらに": "更に", "みぎ": "右", "した": "下"][reading]!, rubyCount: reading.count)]
        }
        XCTAssertEqual(result?.map(\.word), ["更に", "←", "右", "→", "下"])
        XCTAssertEqual(result?.map(\.rubyCount), [3, 1, 2, 1, 2])
        XCTAssertEqual(contexts, ["文脈", "文脈更に←", "文脈更に←右→"])
        XCTAssertNil(preservingLiteralSymbols(reading: "やじるしこーひー時々", context: "") { _, _ in
            XCTFail("Ordinary readings retain normal candidate conversion")
            return []
        })
    }
    /// 2026-11-05 (仕事 22 の 2): 数字の並び (/ が 3 つ以上) の / のあとの部分は、左の文脈に ・ を渡す (出す字は / のまま)。短い並び・数字にはさまれていない / と、切ったときは / を渡す
    func testDigitSlashPassesAMiddleDotAsContext() {
        var lefts: [String] = []
        let convert: (String, String) -> [(word: String, rubyCount: Int)] = { reading, left in lefts.append(left); return [(word: reading, rubyCount: reading.count)] }
        let out = preservingLiteralSymbols(reading: "7/8/9/10だい", context: "", digitSlashContext: true, convert: convert)
        XCTAssertEqual(out?.map(\.word).joined(), "7/8/9/10だい")
        XCTAssertEqual(lefts, ["", "7・", "7・8・", "7・8・9・"])
        lefts = []
        _ = preservingLiteralSymbols(reading: "7/8/9/10だい", context: "", digitSlashContext: false, convert: convert)
        XCTAssertEqual(lefts.last, "7/8/9/")
        // / が 1 つ・2 つの並び (分数・日付) は / のまま
        lefts = []
        _ = preservingLiteralSymbols(reading: "1/5と2024/11/05", context: "", digitSlashContext: true, convert: convert)
        XCTAssertFalse(lefts.contains { $0.contains("・") }, "\(lefts)")
        XCTAssertEqual(digitSlashChainLength(Array("a1/2/3/4b"), at: 2), 3)
        XCTAssertEqual(digitSlashChainLength(Array("1/2"), at: 1), 1)
        XCTAssertEqual(digitSlashChainLength(Array("あ/い"), at: 1), 0)
        lefts = []
        _ = preservingLiteralSymbols(reading: "あ/い", context: "", digitSlashContext: true, convert: convert)
        XCTAssertEqual(lefts, ["", "あ/"])
    }
}

/// 2026-11-23: 読点と括弧では読みを分けない (splitOrdinarySymbols が偽)
final class LiteralSymbolsNoSplitTests: XCTestCase {
    func parts(_ reading: String, digitSlashContext: Bool = false) -> (seen: [String], joined: String?, count: Int?) {
        var seen: [String] = []
        let out = preservingLiteralSymbols(reading: reading, context: "", digitSlashContext: digitSlashContext, splitOrdinarySymbols: false) { text, left in
            seen.append(left + "|" + text)
            return [(word: text, rubyCount: text.count)]
        }
        return (seen, out?.map(\.word).joined(), out?.reduce(0) { $0 + $1.rubyCount })
    }

    func testCommasAndBracketsStayInThePart() {
        let r = parts("いご、がくぎょうに「はげみ」（ほとんど）、しゅせきであった。それな")
        // 読点・括弧は前後と一緒に 1 回で変換し、文の終わりの 。 では今までどおり分ける
        XCTAssertEqual(r.seen, ["|いご、がくぎょうに「はげみ」（ほとんど）、しゅせきであった", "いご、がくぎょうに「はげみ」（ほとんど）、しゅせきであった。|それな"])
        XCTAssertEqual(r.joined, "いご、がくぎょうに「はげみ」（ほとんど）、しゅせきであった。それな")
        XCTAssertEqual(r.count, 33)
    }

    func testOtherSymbolsStillSplit() {
        // ・ / ： は今までどおり分ける (・ の前の名前の扱い・数字/数字 の文脈のため)。英数字にはさまれた読点 (1,000) も分ける
        XCTAssertEqual(parts("やまだ・たろう").seen, ["|やまだ", "やまだ・|たろう"])
        XCTAssertEqual(parts("2024/11/05にかいさい").seen, ["|2024", "2024/|11", "2024/11/|05にかいさい"])
        XCTAssertEqual(parts("1,000えん、やすい").seen, ["|1", "1,|000えん、やすい"])
        XCTAssertEqual(parts("7/8/9/10だい", digitSlashContext: true).seen, ["|7", "7・|8", "7・8・|9", "7・8・9・|10だい"])
    }

    /// 分ける所が 1 つも無い (読点と括弧しか記号が無い) 読みは nil を返し、呼ぶ側がふつうに変換する
    /// (読み全体を convert に渡すと、変換の口がまたここへ来て終わらなかった)
    func testNoSplitPointGivesNil() {
        let r = parts("いご、がくぎょうに「はげみ」（ほとんど）")
        XCTAssertEqual(r.seen, [])
        XCTAssertNil(r.joined)
        // 分ける所があれば、部分はどれも読みより短い (同じ読みで convert を呼び返さない)
        for reading in ["いご、がくぎょう。それ", "やまだ・たろう、はなこ", "（1/2）のかくりつ"] {
            for seen in parts(reading).seen {
                XCTAssertLessThan(seen.split(separator: "|", maxSplits: 1, omittingEmptySubsequences: false).last.map { $0.count } ?? 0, reading.count, reading)
            }
        }
    }

    func testSymbolOnlyPartsAreNotConverted() {
        // 記号だけの部分 (分けずに残った 「」) は変換に渡さない
        let r = parts("「」、")
        XCTAssertEqual(r.seen, [])
        XCTAssertEqual(r.joined, "「」、")
        XCTAssertEqual(r.count, 3)
        // 前の形 (splitOrdinarySymbols が真、既定) は今までどおり、どの記号でも分ける
        var seen: [String] = []
        _ = preservingLiteralSymbols(reading: "いご、がくぎょう", context: "") { text, _ in
            seen.append(text)
            return [(word: text, rubyCount: text.count)]
        }
        XCTAssertEqual(seen, ["いご", "がくぎょう"])
    }
}

/// 2026-11-23 (2 つ目): AI ありの道では 。！？ と ・ でも分けない (extraJoinable)
final class LiteralSymbolsJoinExtraTests: XCTestCase {
    func parts(_ reading: String, extra: Set<Character>, attachQuestionKa: Bool = false) -> (seen: [String], out: [(word: String, rubyCount: Int)]?) {
        var seen: [String] = []
        let out = preservingLiteralSymbols(reading: reading, context: "", attachQuestionKa: attachQuestionKa, splitOrdinarySymbols: false, extraJoinable: extra) { text, _ in
            seen.append(text)
            return [(word: text, rubyCount: text.count)]
        }
        return (seen, out)
    }
    let ai: Set<Character> = ["。", "．", "！", "!", "・", "？", "?"]

    func testSentenceEndsAndNakaguroJoinOnTheAIPath() {
        // 分ける所が無いので nil (呼ぶ側が読み全体をふつうに変換する)
        XCTAssertNil(parts("いご、がくぎょうにはげんだ。それな", extra: ai).out)
        XCTAssertNil(parts("びる・じょいがせつりつした。", extra: ai).out)
        // か？ でも部分を終えない
        XCTAssertNil(parts("えっと、びぶんか？それ", extra: ai, attachQuestionKa: true).out)
        // ？ を足さない (.split の側) なら、か？ で今までどおり分ける
        var noQuestion = ai; noQuestion.subtract(["？", "?"])
        XCTAssertEqual(parts("えっと、びぶんか？それ", extra: noQuestion, attachQuestionKa: true).seen, ["えっと、びぶんか？", "それ"])
        // 数字/数字 の / と ：では今までどおり分ける
        XCTAssertEqual(parts("2024/11/05。それ", extra: ai).seen, ["2024", "11", "05。それ"])
    }

    /// 2026-11-23 (a/dotext): AI の道でつなげる記号 (。．！？・) でも、すぐ後ろが英数字なら今までどおり分ける (0366345 と同じ分け方)。
    /// かなの後ろの 。 + 小文字の拡張子 を . に決め直す決まり (slash_dot の ②) は、。 だけの文節を前提にしている
    func testSentenceEndBeforeLatinOrDigitsStillSplits() {
        // 。 + 小文字の拡張子 (資料。pdf を読んで)
        XCTAssertEqual(parts("しりょう。pdfをよんで", extra: ai).seen, ["しりょう", "pdfをよんで"])
        XCTAssertEqual(parts("しりょう。pdfをよんで", extra: []).seen, ["しりょう", "pdfをよんで"])
        // 文末の 。 のあとの英字 (今日はいい天気です。pdf を送る)・大文字で始まる英字・数字も、つなげずに分ける (0366345 と同じ)
        XCTAssertEqual(parts("いいてんきです。pdfをおくる", extra: ai).seen, ["いいてんきです", "pdfをおくる"])
        XCTAssertEqual(parts("てんきです。Googleで", extra: ai).seen, ["てんきです", "Googleで"])
        XCTAssertEqual(parts("てんきです。5じに", extra: ai).seen, ["てんきです", "5じに"])
        XCTAssertEqual(parts("3。5", extra: ai).seen, ["3", "5"])
        // ！？・ も同じ。後ろがかななら、今までどおりつなげる (分ける所が無ければ nil)
        XCTAssertEqual(parts("すごい！Appleが", extra: ai).seen, ["すごい", "Appleが"])
        XCTAssertNil(parts("いいてんきです。それで", extra: ai).out)
        // 読点と括弧は英字の前でもつなげる (今までどおり。英数字にはさまれたときだけ分ける)
        XCTAssertNil(parts("しりょう、pdfをよんで", extra: ai).out)
    }

    /// 2026-11-23 (a/dotext): AI を切った設定・AI の休み (aiEnabled が偽) は、Space・清書 (dictionaryOnly が偽) でも辞書だけの道と同じに分ける
    @MainActor
    func testDictionaryConversionWithAIOffSplitsLikeTheDictionaryPath() {
        XCTAssertEqual(literalJoinExtra(dictionaryOnly: false, aiEnabled: false, questionKa: .decide), [])
        XCTAssertEqual(literalJoinExtra(dictionaryOnly: true, aiEnabled: true, questionKa: .decide), [])
        XCTAssertTrue(literalJoinExtra(dictionaryOnly: false, aiEnabled: true, questionKa: .decide).contains("。"))
    }

    /// 部分が読み全体と同じになるときは、どこから来ても変換の口に渡さず nil (か？ の ？ を前につなぐ所: 「それはほんとうか？」 は、
    /// 「 を分けないと ？ までの部分が読み全体になり、変換の口が同じ読みで呼び返して落ちた)
    func testAttachedQuestionKaNeverPassesTheWholeReading() {
        for reading in ["「それはほんとうか？", "「それはほんとうか？」", "（なにか？", "えっと、びぶんか？"] {
            for extra in [Set<Character>(), ai] {
                let r = parts(reading, extra: extra, attachQuestionKa: true)
                XCTAssertFalse(r.seen.contains(reading), "\(reading) \(extra.count)")
            }
        }
        // 記号のあとに部分が続けば、か？ までで分けて変換する (今までどおり)
        XCTAssertEqual(parts("「それはほんとうか？」そうだ", extra: [], attachQuestionKa: true).seen, ["「それはほんとうか？", "」そうだ"])
    }

    func testLeadingNakaguroBecomesItsOwnSegment() {
        let out = preservingLiteralSymbols(reading: "いぜん・いごに/あ", context: "", splitOrdinarySymbols: false, extraJoinable: ai) { text, _ in
            // 変換の口が ・ を後ろの文節につけた形 (以前|・以後に)
            text == "いぜん・いごに" ? [(word: "以前", rubyCount: 3), (word: "・以後に", rubyCount: 4)] : [(word: text, rubyCount: text.count)]
        }
        XCTAssertEqual(out?.map(\.word), ["以前", "・", "以後に", "/", "あ"])
        XCTAssertEqual(out?.map(\.rubyCount), [3, 1, 3, 1, 1])
    }
}

/// 2026-11-23 (a/symseg): 前後と一緒に変換した記号を、記号だけの文節に分け直す。約物は打った字に戻す
final class SymbolSegmentSplitTests: XCTestCase {
    func pairs(_ parts: [(word: String, reading: String)]?) -> [String]? {
        parts?.map { "\($0.word)/\($0.reading)" }
    }

    func testSplitsSymbolOffTheSegment() {
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "資料。", reading: "しりょう。")), ["資料/しりょう", "。/。"])
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "1形で、", reading: "1けいで、")), ["1形で/1けいで", "、/、"])
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "「象は」に", reading: "「ぞうは」に")), ["「/「", "象は/ぞうは", "」/」", "に/に"])
    }

    func testRestoresTypedSymbolForm() {
        // 全角・半角の句読点 (。．、，) は、打った字 (読みの字) に戻す
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "使用.", reading: "しよう。")), ["使用/しよう", "。/。"])
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "使用．", reading: "しよう。")), ["使用/しよう", "。/。"])
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "警告，", reading: "けいこく、")), ["警告/けいこく", "、/、"])
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "警告、", reading: "けいこく，")), ["警告/けいこく", "，/，"])
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "使用。", reading: "しよう．")), ["使用/しよう", "．/．"])
        // 学習の側 (typedSymbols が偽) は、選んだ字のまま
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "警告，", reading: "けいこく、", typedSymbols: false)), ["警告/けいこく", "，/、"])
    }

    func testKeepsSegmentWhenSymbolCountsDiffer() {
        // AI が記号を落とした・足した
        XCTAssertNil(splitSegmentAtSymbols(word: "資料", reading: "しりょう。"))
        XCTAssertNil(splitSegmentAtSymbols(word: "資料。。", reading: "しりょう。"))
        // 別の記号にした (、 → 。)
        XCTAssertNil(splitSegmentAtSymbols(word: "警告。", reading: "けいこく、"))
        // 約物の間の読みと表記の片方だけが空
        XCTAssertNil(splitSegmentAtSymbols(word: "。資料", reading: "しりょう。"))
        // 読みに無い種類の約物 (辞書の名前の ・・AI が足した 、) は語の中に残し、読みの約物だけを分ける
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "トム・ハンクス、", reading: "とむはんくす、")), ["トム・ハンクス/とむはんくす", "、/、"])
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "資料、です。", reading: "しりょうです。")), ["資料、です/しりょうです", "。/。"])
        // 絵文字は約物に数えない (表記の 😭 は読みに無い)
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "ございます😭、", reading: "ございます、")), ["ございます😭/ございます", "、/、"])
    }

    func testNestedBrackets() {
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "「『象』は」と", reading: "「『ぞう』は」と")),
                       ["「/「", "『/『", "象/ぞう", "』/』", "は/は", "」/」", "と/と"])
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "（「象」）", reading: "（「ぞう」）")), ["（/（", "「/「", "象/ぞう", "」/」", "）/）"])
    }

    func testLeavesSymbolOnlyAndQuestionKaSegments() {
        XCTAssertNil(splitSegmentAtSymbols(word: "。。。", reading: "。。。"))
        XCTAssertNil(splitSegmentAtSymbols(word: "・", reading: "・"))
        XCTAssertNil(splitSegmentAtSymbols(word: "資料", reading: "しりょう"))
        // 問いの か？ は question_ka.swift が か？ ごとに比べるので分けない。か の無い ？ は分ける
        XCTAssertNil(splitSegmentAtSymbols(word: "本当か？", reading: "ほんとうか？"))
        XCTAssertEqual(pairs(splitSegmentAtSymbols(word: "本当？", reading: "ほんとう？")), ["本当/ほんとう", "？/？"])
    }

    func testSplitsConvertedSegments() {
        let segments: [(word: String, rubyCount: Int)] = [("初代", 4), ("1形で、", 5), ("ハイ", 2), ("5号は", 4), ("。", 1)]
        let out = splittingJoinedSymbols(segments, reading: "しょだい1けいで、はい5ごうは。")
        XCTAssertEqual(out.map(\.word), ["初代", "1形で", "、", "ハイ", "5号は", "。"])
        XCTAssertEqual(out.map(\.rubyCount), [4, 4, 1, 2, 4, 1])
        // 読みの長さと合わなければそのまま
        XCTAssertEqual(splittingJoinedSymbols(segments, reading: "しょだい1けいで、").map(\.word), segments.map(\.word))
    }

    func testLearningDoesNotKeepSymbolAttachedWords() {
        // 一覧で記号付きの文節 (警告，) を選んで確定しても、覚えるのは 警告 と ， に分けた形
        let split = splittingJoinedSymbols(readings: ["けいこく、", "や", "「ぞうは」"], texts: ["警告，", "や", "「象は」"])
        XCTAssertEqual(split.readings, ["けいこく", "、", "や", "「", "ぞうは", "」"])
        XCTAssertEqual(split.texts, ["警告", "，", "や", "「", "象は", "」"])
        XCTAssertFalse(split.texts.contains { $0.count > 1 && $0.contains(where: isPunctuationSymbol) })
        // 数が合わなければそのまま (数が違うものを無理に分けない)
        let kept = splittingJoinedSymbols(readings: ["しりょう。"], texts: ["資料"])
        XCTAssertEqual(kept.texts, ["資料"])
    }
}

/// 2026-11-24 (a/longcomma): 長い読みは、AI ありの道で読点のすぐ後ろで部分を終え、左の部分の出力を右の部分の左の文脈に渡す
final class LongReadingCommaSplitTests: XCTestCase {
    func testCommaEndsThePartAndPassesTheLeftOutput() {
        var calls: [String] = []
        let out = preservingLiteralSymbols(reading: "あいう、えお、かき", context: "まえ", splitOrdinarySymbols: false, splitCommas: true, convert: { reading, left in
            calls.append("\(reading)|\(left)")
            return [(word: reading.uppercased(), rubyCount: reading.count)]
        })
        XCTAssertEqual(calls, ["あいう、|まえ", "えお、|まえあいう、", "かき|まえあいう、えお、"])
        XCTAssertEqual(out?.map(\.word).joined(), "あいう、えお、かき")
        XCTAssertEqual(out?.reduce(0) { $0 + $1.rubyCount }, 9)
    }

    func testCommaBetweenDigitsAndWithoutTheSwitch() {
        var calls: [String] = []
        let convert: (String, String) -> [(word: String, rubyCount: Int)] = { reading, left in
            calls.append(reading)
            return [(word: reading, rubyCount: reading.count)]
        }
        // 英数字にはさまれた , は今までどおり分ける (読点として部分を終えない)
        _ = preservingLiteralSymbols(reading: "1,000えん、です", context: "", splitOrdinarySymbols: false, splitCommas: true, convert: convert)
        XCTAssertEqual(calls, ["1", "000えん、", "です"])
        // splitCommas が偽なら、読点では分けない (分ける所が無いので nil。呼ぶ側がふつうに変換する)
        calls = []
        XCTAssertNil(preservingLiteralSymbols(reading: "あいう、えお", context: "", splitOrdinarySymbols: false, convert: convert))
        // 読点が読みの終わりにしか無ければ、部分が読み全体になるので nil
        XCTAssertNil(preservingLiteralSymbols(reading: "あいうえお、", context: "", splitOrdinarySymbols: false, splitCommas: true, convert: convert))
    }

    func testThresholdAndPath() {
        let short = String(repeating: "あ", count: 49), long = String(repeating: "あ", count: 50)
        XCTAssertEqual(longReadingSplit(short), .none)
        XCTAssertEqual(longReadingSplit(long), .comma)
        XCTAssertEqual(longReadingSplit(long, aiPath: false), .none)
    }

    @MainActor
    func testCommaModeKeepsSentenceEndsJoined() {
        XCTAssertTrue(literalJoinExtra(dictionaryOnly: false, questionKa: .decide, long: .comma).contains("。"))
        XCTAssertFalse(literalJoinExtra(dictionaryOnly: false, questionKa: .decide, long: .period).contains("。"))
    }
}
