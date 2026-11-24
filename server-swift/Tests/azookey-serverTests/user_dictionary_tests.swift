import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

final class UserDictionaryTests: XCTestCase {
    func testRowsBecomeRegisteredWordsWithTheirPartOfSpeech() {
        let elements = parseUserDictionary([
            ["はにわたろう", "埴輪太郎", "人名", "試験の例"],
            ["はにわーど", "HaniWord", "固有名詞"],
            ["めあど", "someone@example.com", "短縮よみ"],
            ["かおもじ", "(^_^)"],
            ["さっぽろえき", "札幌駅", "地名"],
            ["なぞ", "謎語", "よく分からない品詞"],
        ])
        XCTAssertEqual(elements.map(\.word), ["埴輪太郎", "HaniWord", "someone@example.com", "(^_^)", "札幌駅", "謎語"])
        XCTAssertEqual(elements.map(\.ruby), ["ハニワタロウ", "ハニワード", "メアド", "カオモジ", "サッポロエキ", "ナゾ"])
        XCTAssertEqual(elements.map(\.lcid), [CIDData.人名一般.cid, CIDData.固有名詞.cid, CIDData.一般名詞.cid, CIDData.一般名詞.cid,
                                              CIDData.地名一般.cid, CIDData.一般名詞.cid])
        XCTAssertTrue(elements.allSatisfy { $0.metadata == .isUserRegistered && $0.lcid == $0.rcid })
    }

    func testBrokenRowsAreDroppedAndRepeatsAreKeptOnce() {
        let elements = parseUserDictionary([
            ["ハニワ", "埴輪"],              // 読みがカタカナ
            ["はにわ1", "埴輪"],             // 読みに数字
            ["", "空"],                       // 読みが空
            ["そら", ""],                     // 語が空
            [String(repeating: "あ", count: 21), "長すぎる読み"],
            [String(repeating: "あ", count: 20), "二十字の読み"],
            ["ゔぁいおりん", "ヴァイオリン"],
            ["はにわたろう", "埴輪太郎"],
            ["はにわたろう", "埴輪太郎", "名詞", "重なり"],
        ])
        XCTAssertEqual(elements.map(\.word), ["二十字の読み", "ヴァイオリン", "埴輪太郎"])
    }

    /// 後処理 (英字のカタカナ化・表記ゆれなど) は、登録した語の所だけ元に戻し、ほかの所の直しは残す
    func testFixesKeepRegisteredWords() {
        var registered = DicdataElement(word: "LINE", ruby: "ライン", cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -5)
        registered.metadata = .isUserRegistered
        let particle = DicdataElement(word: "デ", ruby: "デ", cid: 158, mid: MIDData.一般.mid, value: -5)
        func candidate(_ data: [DicdataElement]) -> Candidate {
            Candidate(text: data.map(\.word).joined(), value: -10, correspondingCount: 4, lastMid: MIDData.一般.mid, data: data)
        }
        let best = candidate([registered, particle])
        var katakana = registered
        katakana.word = "ライン"
        katakana.metadata = []
        var hiragana = particle
        hiragana.word = "で"
        // 登録した語と助詞の両方を直した → 助詞の直しだけ残る
        XCTAssertEqual(keepingUserRegisteredWords(best, candidate([katakana, hiragana]))?.text, "LINEで")
        // 登録した語だけを直した → 直しは無し
        XCTAssertNil(keepingUserRegisteredWords(best, candidate([katakana, particle])))
        // 区切りが変わる直しで登録した語が消える → 使わない。残る → 使う
        let merged = DicdataElement(word: "ラインデ", ruby: "ラインデ", cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -5)
        XCTAssertNil(keepingUserRegisteredWords(best, candidate([merged])))
        let split = [registered, DicdataElement(word: "で", ruby: "デ", cid: 158, mid: MIDData.一般.mid, value: -5)]
        XCTAssertEqual(keepingUserRegisteredWords(best, Candidate(text: "LINEで", value: -10, correspondingCount: 4, lastMid: 0, data: split))?.text, "LINEで")
        // 登録した語が無ければそのまま
        XCTAssertEqual(keepingUserRegisteredWords(candidate([katakana, particle]), candidate([katakana, hiragana]))?.text, "ラインで")
    }

    @MainActor func testSegmentFixesKeepRegisteredText() {
        let saved = (appliedUserWords, appliedUserWordLengths)
        defer { (appliedUserWords, appliedUserWordLengths) = saved }
        appliedUserWords = ["商用"]
        appliedUserWordLengths = [2]
        let before = [(word: "当局の", rubyCount: 6), (word: "商用", rubyCount: 4)]
        let after = [(word: "当局の", rubyCount: 6), (word: "慫慂", rubyCount: 4)]
        XCTAssertEqual(keepingUserRegisteredText(before, after).map(\.word), ["当局の", "商用"])
        let other = [(word: "当局が", rubyCount: 6), (word: "商用", rubyCount: 4)]
        XCTAssertEqual(keepingUserRegisteredText(before, other).map(\.word), ["当局が", "商用"])
        appliedUserWords = []
        XCTAssertEqual(keepingUserRegisteredText(before, after).map(\.word), ["当局の", "慫慂"])
    }

    /// 読み全体が登録した読みと同じ入力は、登録した語にする (選んだ候補がその読みの登録した語なら、そのまま)
    @MainActor func testWholeReadingBecomesTheRegisteredWord() {
        let saved = appliedUserWordsByRuby
        defer { appliedUserWordsByRuby = saved }
        let words = parseUserDictionary([["めあど", "someone@example.com", "短縮よみ"], ["めあど", "other@example.com", "短縮よみ"]])
        appliedUserWordsByRuby = Dictionary(grouping: words, by: \.ruby)
        let model = Candidate(text: "メアド", value: -10, correspondingCount: 3, lastMid: MIDData.一般.mid,
                              data: [DicdataElement(word: "メアド", ruby: "メアド", cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -11)])
        XCTAssertEqual(wholeReadingUserWord("めあど", count: 3, chosen: model)?.text, "someone@example.com")
        XCTAssertEqual(wholeReadingUserWord("めあど", count: 3, chosen: nil)?.text, "someone@example.com")
        let second = Candidate(text: "other@example.com", value: -5, correspondingCount: 3, lastMid: MIDData.一般.mid, data: [words[1]])
        XCTAssertNil(wholeReadingUserWord("めあど", count: 3, chosen: second))
        XCTAssertNil(wholeReadingUserWord("めあどを", count: 4, chosen: model))
        appliedUserWordsByRuby = [:]
        XCTAssertNil(wholeReadingUserWord("めあど", count: 3, chosen: model))
    }

    /// 上流 AzooKeyKanaKanjiConverter #370 の 1 つ目 (辞書の前方一致より長い読みの登録した語) と、取り込みで登録の印が消えないこと
    func testLongRegisteredWordIsFoundAndKeepsItsMark() async throws {
        guard let path = ProcessInfo.processInfo.environment["AZK_TEST_DICTIONARY_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        try await MainActor.run {
            let root = URL(filePath: path)
            let converter = KanaKanjiConverter()
            let options = ConvertRequestOptions(requireJapanesePrediction: false, requireEnglishPrediction: false,
                keyboardLanguage: .ja_JP, learningType: .nothing, dictionaryResourceURL: root.appendingPathComponent("Dictionary"),
                memoryDirectoryURL: root.appendingPathComponent("memory"), sharedContainerURL: root.appendingPathComponent("shared"),
                textReplacer: .empty, metadata: nil)
            let extra = DicdataElement(word: "GeoGuessr", ruby: "ジオゲッサー", cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -10)
            converter.sendToDicdataStore(.importDynamicUserDict([extra] + parseUserDictionary([["はにわたろう", "埴輪太郎", "人名"]])))
            var input = ComposingText()
            input.insertAtCursorPosition("はにわたろうさんはこうえんに", inputStyle: .direct)
            let best = try XCTUnwrap(converter.requestCandidates(input, options: options).mainResults.first)
            let word = try XCTUnwrap(best.data.first { $0.word == "埴輪太郎" }, "\(best.text)")
            XCTAssertTrue(word.metadata.contains(.isUserRegistered))
            XCTAssertTrue(word.metadata.contains(.isFromUserDictionary))
            var other = ComposingText()
            other.insertAtCursorPosition("じおげっさー", inputStyle: .direct)
            let extraWord = try XCTUnwrap(converter.requestCandidates(other, options: options).mainResults.first?.data.first)
            XCTAssertEqual(extraWord.word, "GeoGuessr")
            XCTAssertFalse(extraWord.metadata.contains(.isUserRegistered), "the shipped extra dictionary is not a registered word")
        }
    }
}
