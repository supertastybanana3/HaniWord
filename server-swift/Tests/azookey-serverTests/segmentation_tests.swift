import KanaKanjiConverterModule
import XCTest
@testable import azookey_server

private func element(_ word: String, _ ruby: String, _ cid: Int = CIDData.一般名詞.cid) -> DicdataElement {
    DicdataElement(word: word, ruby: ruby, lcid: cid, rcid: cid, mid: 501, value: -10)
}

private func candidate(_ data: [DicdataElement]) -> Candidate {
    Candidate(text: data.map(\.word).joined(), value: -10,
              correspondingCount: data.reduce(0) { $0 + $1.ruby.count }, lastMid: 501, data: data)
}

final class SegmentationTests: XCTestCase {
    func testParticlesHaveTheirOwnCandidateRange() {
        let input = candidate([element("逆輸入語", "ギャクユニュウゴ"), element("が", "ガ", 147)])
        XCTAssertEqual(input.clauseSplit().map(\.word), ["逆輸入語が"])
        let parts = input.clauseSplit(separateParticles: true)
        XCTAssertEqual(parts.map(\.word), ["逆輸入語", "が"])
        XCTAssertEqual(parts.map(\.rubyCount), [8, 1])
    }

    func testCompoundNounsAreNotSplitIntoIndividualKanji() {
        let input = candidate([element("逆", "ギャク", 6), element("輸入", "ユニュウ"),
                               element("語", "ゴ", 1300), element("が", "ガ", 147)])
        XCTAssertEqual(input.clauseSplit(separateParticles: true).map(\.word), ["逆輸入語", "が"])
    }

    func testConsecutiveParticlesStayTogether() {
        let input = candidate([element("日本", "ニホン"), element("に", "ニ", 147), element("は", "ハ", 261)])
        XCTAssertEqual(input.clauseSplit(separateParticles: true).map(\.word), ["日本", "には"])
    }

    func testVerbInflectionIsNotDetachedLikeANounParticle() {
        let input = candidate([element("泳い", "オヨイ", 725), element("でいる", "デイル", 147)])
        XCTAssertEqual(input.clauseSplit(separateParticles: true).map(\.word), ["泳いでいる"])
    }

    func testLatinLettersStayOneWordWithSeparateJapaneseParticle() {
        let input = candidate("Teriyaki".map { element(String($0), String($0)) } + [element("が", "ガ", 147)])
        let parts = input.clauseSplit(separateParticles: true)
        XCTAssertEqual(parts.map(\.word), ["Teriyaki", "が"])
        XCTAssertEqual(parts.map(\.rubyCount), [8, 1])
    }

    func testAtomicNeuralNounCanBeSplitWithoutChangingTextOrCounts() {
        let parts = splitAttachedParticles(reading: "にほんごが", segments: [("日本語が", 5)]) { reading, text in
            reading == "にほんご" && text == "日本語"
        }
        XCTAssertEqual(parts.map(\.word), ["日本語", "が"])
        XCTAssertEqual(parts.map(\.rubyCount), [4, 1])
        let verb = splitAttachedParticles(reading: "およいで", segments: [("泳いで", 4)]) { _, _ in false }
        XCTAssertEqual(verb.map(\.word), ["泳いで"])
    }

    func testSelectedTextRecoversDictionaryBoundariesWithoutReplacingLiveCache() async throws {
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
            var live = ComposingText()
            live.insertAtCursorPosition("およいでいる", inputStyle: .direct)
            let before = converter.requestCandidates(live, options: options).mainResults.map(\.text)
            var input = ComposingText()
            input.insertAtCursorPosition("とうきょうでほんをかう", inputStyle: .direct)
            var selected = try XCTUnwrap(converter.dictionaryCandidate(input, matching: "東京で本を買う", options: options))
            selected.data = splitDictionaryParticleAffixes(selected.data) { reading, text in
                var part = ComposingText()
                part.insertAtCursorPosition(reading, inputStyle: .direct)
                return converter.dictionaryCandidate(part, matching: text, options: options)
            }
            XCTAssertEqual(selected.clauseSplit(separateParticles: true).map(\.word), ["東京", "で", "本", "を", "買う"])
            XCTAssertEqual(selected.correspondingCount, input.input.count)
            XCTAssertNil(converter.dictionaryCandidate(input, matching: "日本語が", options: options))
            XCTAssertEqual(converter.requestCandidates(live, options: options).mainResults.map(\.text), before)
        }
    }

    func testFusedParticleEntriesKeepVerbInflectionAndCompoundNouns() {
        let fusedVerb = DicdataElement(word: "を上げ", ruby: "ヲアゲ", lcid: 156, rcid: 625, mid: 501, value: -10)
        let input = [fusedVerb, element("たい", "タイ", 444)]
        let data = splitDictionaryParticleAffixes(input) { reading, text in
            reading == "あげ" && text == "上げ" ? candidate([element("上げ", "アゲ", 625)]) : nil
        }
        XCTAssertEqual(candidate(data).clauseSplit(separateParticles: true).map(\.word), ["を", "上げたい"])
        let fusedNoun = DicdataElement(word: "日本語が", ruby: "ニホンゴガ", lcid: 1285, rcid: 148, mid: 501, value: -10)
        let nounData = splitDictionaryParticleAffixes([fusedNoun]) { reading, text in
            reading == "にほんご" && text == "日本語" ? candidate([element("日本", "ニホン"), element("語", "ゴ", 1298)]) : nil
        }
        XCTAssertEqual(candidate(nounData).clauseSplit(separateParticles: true).map(\.word), ["日本語", "が"])
        XCTAssertEqual(splitDictionaryParticleAffixes(input) { _, _ in nil }.map(\.word), input.map(\.word))
    }

    func testCompoundParticlesStartTheirOwnClause() {
        // 2026-11-20: 連語の助詞 (とともに 182) の前で切る (既定 before)。こと (名詞,非自立) は前の動詞につながったまま。
        // 頭の助詞と残りに分ける形 (noverb・word) は、測るためのつまみとして残す
        let input = candidate([element("起こる", "オコル", 772), element("こと", "コト", 1310), element("とともに", "トトモニ", 182)])
        XCTAssertEqual(input.clauseSplit(separateParticles: true).map(\.word), ["起こることとともに"])
        XCTAssertEqual(clauseSplitAtCompoundParticles(input, mode: .off).map(\.word), ["起こることとともに"])
        XCTAssertEqual(clauseSplitAtCompoundParticles(input, mode: .before).map(\.word), ["起こること", "とともに"])
        let lookup: (String, String) -> Candidate? = { reading, text in
            switch (reading, text) {
            case ("ともに", "ともに"): return candidate([element("ともに", "トモニ", 1281)])
            case ("ついて", "ついて"): return candidate([element("ついて", "ツイテ", 687)])
            default: return nil
            }
        }
        var split = input
        split.data = splitCompoundParticles(input.data, mode: .wordTail, lookup: lookup)
        let parts = clauseSplitAtCompoundParticles(split, mode: .wordTail)
        XCTAssertEqual(parts.map(\.word), ["起こること", "と", "ともに"])
        XCTAssertEqual(parts.map(\.rubyCount), [5, 1, 3])
        // 残りが動詞 (ついて) なら、noverb では分けず前で切るだけ (日本／について)。word では分ける
        let about = candidate([element("日本", "ニホン"), element("について", "ニツイテ", 199)])
        XCTAssertEqual(clauseSplitAtCompoundParticles(candidate(splitCompoundParticles(about.data, mode: .nonVerbTail, lookup: lookup)), mode: .nonVerbTail).map(\.word),
                       ["日本", "について"])
        XCTAssertEqual(clauseSplitAtCompoundParticles(candidate(splitCompoundParticles(about.data, mode: .wordTail, lookup: lookup)), mode: .wordTail).map(\.word),
                       ["日本", "に", "ついて"])
        // 引くと 1 語にならない残り (辞書に無い) は分けない。引用・くだけた形 (という 173) の前では切らない
        XCTAssertEqual(splitCompoundParticles(about.data, mode: .wordTail) { _, _ in nil }.map(\.word), ["日本", "について"])
        let quote = candidate([element("起こる", "オコル", 772), element("という", "トイウ", 173), element("こと", "コト", 1310)])
        XCTAssertEqual(clauseSplitAtCompoundParticles(quote, mode: .wordTail).map(\.word), quote.clauseSplit(separateParticles: true).map(\.word))
    }

    func testCompoundParticleClauseIsNotSplitByAttachedParticleRule() {
        // 2026-11-20: 辞書だけの道で 起こること／と共／に になっていた (と共に 183 を splitAttachedParticles が と共＋に に割った)。連語の助詞の文節は割らない
        let input = candidate([element("起こる", "オコル", 772), element("こと", "コト", 1310), element("と共に", "トトモニ", 183)])
        let clauses = compoundParticleClauses(input, mode: .before)
        XCTAssertEqual(clauses.segments.map(\.word), ["起こること", "と共に"])
        // 1 は連語の助詞の文節、0 は最後の語 (こと) の右の品詞が助詞でない文節
        XCTAssertEqual(clauses.keep, [0, 1])
        let nominal: (String, String) -> Bool = { _, text in text == "と共" }
        XCTAssertEqual(splitAttachedParticles(reading: "おこることとともに", segments: clauses.segments, keep: clauses.keep, isNominalHead: nominal).map(\.word),
                       ["起こること", "と共に"])
        XCTAssertEqual(splitAttachedParticles(reading: "おこることとともに", segments: clauses.segments, isNominalHead: nominal).map(\.word),
                       ["起こること", "と共", "に"])
        // 切らない (off) なら印も無い
        XCTAssertEqual(compoundParticleClauses(input, mode: .off).keep, [])
    }

    func testFusedDictionaryEntriesAreSplitAtTheirInnerParticle() {
        // 2026-11-21 (案 1): 辞書の 1 つにした行 (ことが多い 1310→19) を こと＋が＋多い に分ける。前と後ろが辞書で引けるときだけ
        let fused = DicdataElement(word: "ことが多い", ruby: "コトガオオイ", lcid: 1310, rcid: 19, mid: 501, value: -9)
        let lookup: (String, String) -> Candidate? = { reading, text in
            switch (reading, text) {
            case ("こと", "こと"): return candidate([element("こと", "コト", 1310)])
            case ("おおい", "多い"): return candidate([element("多い", "オオイ", 19)])
            case ("が", "が"): return candidate([element("が", "ガ", 148)])
            case ("こ", "こ"): return candidate([element("こ", "コ", 1285)])
            case ("である", "である"): return candidate([element("で", "デ", 149), element("ある", "アル", 772)])
            case ("とである", "とである"): return candidate([element("と", "ト", 164), element("である", "デアル", 491)])
            case ("と", "と"): return candidate([element("と", "ト", 164)])
            case ("で", "で"): return candidate([element("で", "デ", 149)])
            default: return nil
            }
        }
        let input = [element("異なる", "コトナル", 772), fused]
        let data = splitFusedEntries(input, lookup: lookup)
        XCTAssertEqual(data.map(\.word), ["異なる", "こと", "が", "多い"])
        // 前の語は元の行の左の品詞のまま、後ろの語は元の行の右の品詞のまま
        XCTAssertEqual(data[1].lcid, 1310)
        XCTAssertEqual(data[3].rcid, 19)
        // こと (名詞,非自立) の後ろの助詞は前につながったまま (こと／が／多い にはしない)
        XCTAssertEqual(candidate(data).clauseSplit(separateParticles: true).map(\.word), ["異なることが", "多い"])
        XCTAssertEqual(candidate(input).clauseSplit(separateParticles: true).map(\.word), ["異なることが多い"])
        // と・で は使わない (ことである を こ＋と＋である・こと＋で＋ある にしない)
        let copula = DicdataElement(word: "ことである", ruby: "コトデアル", lcid: 1310, rcid: 491, mid: 501, value: -9)
        XCTAssertEqual(splitFusedEntries([copula], lookup: lookup).map(\.word), ["ことである"])
        // 右の品詞が連語の助詞 (により 211) の行は、尻の連語の助詞を表で見つけて前で切る (変更したこと／により。こと＋に＋より にしない)
        XCTAssertEqual(compoundParticleReadings.count, 76)
        XCTAssertEqual(compoundParticleReadings["に対して"], "にたいして")
        let byMeans = DicdataElement(word: "変更したことにより", ruby: "ヘンコウシタコトニヨリ", lcid: 1301, rcid: 211, mid: 501, value: -9)
        let byLookup: (String, String) -> Candidate? = { reading, text in
            switch (reading, text) {
            case ("へんこうしたこと", "変更したこと"): return candidate([element("変更", "ヘンコウ", 1301), element("した", "シタ", 610), element("こと", "コト", 1310)])
            case ("により", "により"): return candidate([element("に", "ニ", 151), element("より", "ヨリ", 155)])
            case ("より", "より"): return candidate([element("より", "ヨリ", 155)])
            case ("に", "に"): return candidate([element("に", "ニ", 151)])
            default: return nil
            }
        }
        let byData = splitFusedEntries([byMeans], lookup: byLookup)
        XCTAssertEqual(byData.map(\.word), ["変更したこと", "により"])
        XCTAssertEqual(compoundParticleClauses(candidate(byData), mode: .before).segments.map(\.word), ["変更したこと", "により"])
        // 引けない部分があれば分けない。左右の品詞が同じ行は分けない
        XCTAssertEqual(splitFusedEntries(input) { _, _ in nil }.map(\.word), ["異なる", "ことが多い"])
        let sameSides = DicdataElement(word: "天の川", ruby: "アマノガワ", lcid: 1288, rcid: 1288, mid: 501, value: -9)
        XCTAssertEqual(splitFusedEntries([sameSides], lookup: lookup).map(\.word), ["天の川"])
    }

    func testNounParticlesStayWithTheNounButLatinParticlesAreSeparate() {
        // 2026-11-21: 名詞の後ろの助詞は前につなげる (教室に／陥る・必要が／あります)。英字の語の後ろの助詞は別の文節のまま (Teriyaki／が)
        let fall = candidate([element("教室", "キョウシツ", 1285), element("に", "ニ", 151), element("陥る", "オチイル", 772)])
        XCTAssertEqual(fall.clauseSplit(separateParticles: true).map(\.word), ["教室", "に", "陥る"])
        XCTAssertEqual(fall.clauseSplit(separateParticles: true, latinParticlesOnly: true).map(\.word), ["教室に", "陥る"])
        let need = candidate([element("必要", "ヒツヨウ", 1287), element("が", "ガ", 148), element("あります", "アリマス", 772)])
        XCTAssertEqual(compoundParticleClauses(need, mode: .before, latinParticlesOnly: true).segments.map(\.word), ["必要が", "あります"])
        let latin = candidate("Teriyaki".map { element(String($0), String($0)) } + [element("が", "ガ", 147)])
        let latinParts = latin.clauseSplit(separateParticles: true, latinParticlesOnly: true)
        XCTAssertEqual(latinParts.map(\.word), ["Teriyaki", "が"])
        XCTAssertEqual(latinParts.map(\.rubyCount), [8, 1])
        let japan = candidate([element("日本", "ニホン"), element("に", "ニ", 147), element("は", "ハ", 261)])
        XCTAssertEqual(japan.clauseSplit(separateParticles: true, latinParticlesOnly: true).map(\.word), ["日本には"])
    }

    func testParticleLedVerbEntriesAreSplitWhenNounParticlesStayTogether() {
        // 2026-11-21: 助詞＋動詞＋て を 1 つにした行 (を信じて 156→307) は、名詞＋助詞を 1 文節にする形では 頭の助詞＋残り に分ける (本願を／信じて)
        let believe = DicdataElement(word: "を信じて", ruby: "ヲシンジテ", lcid: 156, rcid: 307, mid: 501, value: -10)
        let lookup: (String, String) -> Candidate? = { reading, text in
            reading == "しんじて" && text == "信じて" ? candidate([element("信じ", "シンジ", 640), element("て", "テ", 307)]) : nil
        }
        let input = [element("本願", "ホンガン"), believe]
        XCTAssertEqual(splitDictionaryParticleAffixes(input, lookup: lookup).map(\.word), ["本願", "を信じて"])
        let data = splitDictionaryParticleAffixes(input, particleLedClauses: true, lookup: lookup)
        XCTAssertEqual(data.map(\.word), ["本願", "を", "信じ", "て"])
        XCTAssertEqual(candidate(data).clauseSplit(separateParticles: true, latinParticlesOnly: true).map(\.word), ["本願を", "信じて"])
        // 連語の助詞 (について 199→199) は分けない
        let about = DicdataElement(word: "について", ruby: "ニツイテ", lcid: 199, rcid: 199, mid: 501, value: -3)
        XCTAssertEqual(splitDictionaryParticleAffixes([about], particleLedClauses: true) { _, _ in candidate([element("ついて", "ツイテ", 687)]) }.map(\.word),
                       ["について"])
    }

    func testFormalNounsAfterVerbsAndAdnominalsStartClauses() {
        // 2026-11-21 (案 2): 動詞のすぐ後ろの ため (1313)・結果 (1314) の前と、連体詞 (1315) の後ろで切る。こと (1310) は前につなげたまま
        let purpose = candidate([element("参加", "サンカ", 1283), element("する", "スル", 610), element("ため", "タメ", 1313), element("の", "ノ", 368)])
        XCTAssertEqual(compoundParticleClauses(purpose, mode: .before, latinParticlesOnly: true, heads: true).segments.map(\.word).suffix(1), ["ための"])
        let result = candidate([element("相談", "ソウダン", 1283), element("した", "シタ", 610), element("結果", "ケッカ", 1314)])
        XCTAssertEqual(compoundParticleClauses(result, mode: .before, latinParticlesOnly: true, heads: true).segments.map(\.word).suffix(1), ["結果"])
        let this = candidate([element("この", "コノ", 1315), element("不変量", "フヘンリョウ"), element("が", "ガ", 148)])
        XCTAssertEqual(this.clauseSplit(separateParticles: true, latinParticlesOnly: true).map(\.word), ["この不変量が"])
        XCTAssertEqual(compoundParticleClauses(this, mode: .before, latinParticlesOnly: true, heads: true).segments.map(\.word), ["この", "不変量が"])
        let thing = candidate([element("起こる", "オコル", 772), element("こと", "コト", 1310)])
        XCTAssertEqual(compoundParticleClauses(thing, mode: .before, latinParticlesOnly: true, heads: true).segments.map(\.word), ["起こること"])
        // 名詞のすぐ後ろの ため (国のため) は切らない (動詞・助動詞の後ろだけ)
        let nation = candidate([element("国", "クニ"), element("の", "ノ", 368), element("ため", "タメ", 1313)])
        XCTAssertEqual(compoundParticleClauses(nation, mode: .before, latinParticlesOnly: true, heads: true).segments.map(\.word),
                       nation.clauseSplit(separateParticles: true, latinParticlesOnly: true).map(\.word))
    }
}
