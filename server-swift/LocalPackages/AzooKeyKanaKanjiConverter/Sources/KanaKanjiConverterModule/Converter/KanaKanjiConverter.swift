//
//  KanaKanjiConverter.swift
//  AzooKeyKanaKanjiConverter
//
//  Created by ensan on 2020/09/03.
//  Copyright © 2020 ensan. All rights reserved.
//

import Foundation
import SwiftUtils
import EfficientNGram

/// かな漢字変換の管理を受け持つクラス
@MainActor public final class KanaKanjiConverter {
    public init() {}
    public init(dicdataStore: DicdataStore) {
        self.converter = .init(dicdataStore: dicdataStore)
    }

    private var converter = Kana2Kanji()
    @MainActor private var checker = SpellChecker()
    private var checkerInitialized: [KeyboardLanguage: Bool] = [.none: true, .ja_JP: true]

    // 前回の変換や確定の情報を取っておく部分。
    private var previousInputData: ComposingText?
    private var nodes: [[LatticeNode]] = []
    /// 2026-11-05 (azooKey-Windows): いまの格子 (nodes) が Zenzai の下書き (all_zenzai の 1 回目。N_best 2 か、前の打鍵の制約で語を落としたもの) か。
    /// AI でない変換 (辞書だけ) がこの格子を使い回すと語が落ちる (AI のすぐあとに辞書だけで変換すると あるふぁばん が α版 でなく あるふぁ版) ので、作り直す
    private var latticeFromZenzai = false
    private var completedData: Candidate?
    private var lastData: DicdataElement?
    /// azooKey-Windows (2026-11-08 仕事 49): 始点の rcid ごとに取っておいた格子 (文頭は -1)。setStartRcid で始点を替えるとき、いまの格子をしまい、
    /// 替えた先の始点の格子があれば戻す (辞書だけの変換は、左の語からと文頭からを打鍵ごとに替えることがあり、毎回捨てると打ち足しの使い回しが効かない)
    private var savedLattices: [Int: (previousInputData: ComposingText?, nodes: [[LatticeNode]], latticeFromZenzai: Bool, completedData: Candidate?)] = [:]
    /// Zenzaiのためのzenzモデル
    private var zenz: Zenz? = nil
    private var zenzaiCache: Kana2Kanji.ZenzaiCache? = nil
    /// 融合採点 (azooKey-Windows): true のとき、Zenzai が途中で作った候補を lastZenzaiPool に残す
    public var collectFusionPool = false
    /// 直前の Zenzai 変換で作られた文全体の候補 (先頭が Zenzai の最終候補)。重複を含む
    public private(set) var lastZenzaiPool: [Candidate] = []
    private var zenzaiPersonalization: (mode: ConvertRequestOptions.ZenzaiMode.PersonalizationMode, base: EfficientNGram, personal: EfficientNGram)?
    public private(set) var zenzStatus: String = ""

    /// azooKey-Windows (2026-11-08): 前の読みの格子と Zenzai の制約だけを捨てる (Zenzai の会話の続き・学習の個人化は残す)。
    /// 辞書の引き方を変えて同じ読みを変換し直すとき (表を隠した一覧) に、前の格子が使い回されないように使う
    public func resetLatticeCache() {
        self.savedLattices = [:]
        self.zenzaiCache = nil
        self.previousInputData = nil
        self.nodes = []
        self.latticeFromZenzai = false
        self.completedData = nil
        self.lastData = nil
    }

    /// azooKey-Windows (2026-11-08 仕事 49): 格子の始点を、左の文脈の終わりの語の右の品詞 (rcid) にする (nil なら文頭)。
    /// 替えるときは、いまの格子を始点ごとにしまい、替えた先の始点の格子があれば戻す (無ければ作り直す)。Zenzai の制約と会話の続きはそのまま
    public func setStartRcid(_ rcid: Int?) {
        let current = self.converter.dicdataStore.startRcid
        guard current != rcid else { return }
        self.savedLattices[current ?? -1] = (self.previousInputData, self.nodes, self.latticeFromZenzai, self.completedData)
        self.converter.dicdataStore.startRcid = rcid
        if let saved = self.savedLattices.removeValue(forKey: rcid ?? -1) {
            (self.previousInputData, self.nodes, self.latticeFromZenzai, self.completedData) = saved
        } else {
            self.previousInputData = nil
            self.nodes = []
            self.latticeFromZenzai = false
            self.completedData = nil
        }
        // しまうのは文頭と、左の語の始点 1 つまで (左の語が替わったら古いほうは捨てる)
        if self.savedLattices.count > 1 {
            self.savedLattices = self.savedLattices.filter { $0.key == -1 }
        }
    }

    /// azooKey-Windows (2026-11-08 仕事 49): 品詞のつながりの点 (former の右 → latter の左)
    public func connectionValue(_ former: Int, _ latter: Int) -> PValue {
        self.converter.dicdataStore.getCCValue(former, latter)
    }

    /// リセットする関数
    public func stopComposition() {
        self.savedLattices = [:]
        self.zenz?.endSession()
        self.zenzaiPersonalization = nil
        self.zenzaiCache = nil
        self.previousInputData = nil
        self.nodes = []
        self.latticeFromZenzai = false
        self.completedData = nil
        self.lastData = nil
    }

    private func getZenzaiPersonalization(mode: ConvertRequestOptions.ZenzaiMode.PersonalizationMode?) -> (mode: ConvertRequestOptions.ZenzaiMode.PersonalizationMode, base: EfficientNGram, personal: EfficientNGram)? {
        guard let mode else {
            return nil
        }
        if let zenzaiPersonalization, zenzaiPersonalization.mode == mode {
            return zenzaiPersonalization
        }
        let tokenizer = ZenzTokenizer()
        let baseModel = EfficientNGram(baseFilename: mode.baseNgramLanguageModel, n: mode.n, d: mode.d, tokenizer: tokenizer)
        let personalModel = EfficientNGram(baseFilename: mode.personalNgramLanguageModel, n: mode.n, d: mode.d, tokenizer: tokenizer)
        self.zenzaiPersonalization = (mode, baseModel, personalModel)
        return (mode, baseModel, personalModel)
    }

    /// azooKey-Windows: 読み (ひらがな) を output と書く確率を zenz で求める (v3 のプロンプト、条件なし)。
    /// Zenzai の貪欲な直しが辞書どおりの変換 (跳梁跋扈) を崩したときに、文全体で比べ直すのに使う
    public func zenzSequenceScore(hiragana: String, output: String, leftSideContext: String, options: ConvertRequestOptions) -> Float? {
        guard options.zenzaiMode.enabled, let zenz = self.getModel(modelURL: options.zenzaiMode.weightURL) else { return nil }
        let input = hiragana.toKatakana()
        let prompt = leftSideContext.isEmpty
            ? "\u{EE00}" + input + "\u{EE01}"
            : "\u{EE02}" + leftSideContext + "\u{EE00}" + input + "\u{EE01}"
        return zenz.sequenceLogProb(prompt: prompt, output: output)
    }

    /// azooKey-Windows (2026-10-28 試作): 読みと表記をいっしょに書く確率 (対数)。読みが違う候補 (打ち間違いを直した読み) どうしを比べるのに使う。
    /// zenzSequenceScore は読みを条件にした表記の確率なので、読みどうしは比べられない
    public func zenzJointScore(hiragana: String, output: String, leftSideContext: String, options: ConvertRequestOptions) -> Float? {
        guard options.zenzaiMode.enabled, let zenz = self.getModel(modelURL: options.zenzaiMode.weightURL) else { return nil }
        let head = leftSideContext.isEmpty ? "" : "\u{EE02}" + leftSideContext
        return zenz.sequenceLogProb(prompt: head, output: "\u{EE00}" + hiragana.toKatakana() + "\u{EE01}" + output)
    }

    /// azooKey-Windows (2026-11-07、仕事 45): 候補 output のあとにカーソルの右の字 right が続く確率 (対数、EOS は足さない)。
    /// hiragana があれば、読みつきのプロンプト (zenzSequenceScore と同じ形) の出力の続きとして、nil なら読みを渡さず左の文脈の続きとして測る
    public func zenzRightContextScore(hiragana: String?, output: String, right: String, leftSideContext: String, options: ConvertRequestOptions) -> Float? {
        guard options.zenzaiMode.enabled, let zenz = self.getModel(modelURL: options.zenzaiMode.weightURL) else { return nil }
        let head = leftSideContext.isEmpty ? "" : "\u{EE02}" + leftSideContext
        let prompt = hiragana.map { head + "\u{EE00}" + $0.toKatakana() + "\u{EE01}" } ?? head
        return zenz.continuationLogProb(prompt: prompt, prefix: output, continuation: right)
    }

    /// 融合採点: 読み hiragana に対して outputs それぞれを zenz が書く対数確率 (EOS まで)。プロンプトは Zenzai と同じ (左文脈・話題など)
    public func zenzScoreSequences(hiragana: String, outputs: [String], options: ConvertRequestOptions) -> [Float]? {
        let t = Date()
        defer {
            if azkDevEnvironment["AZK_FUSION_PROF"] == "1" {
                FileHandle.standardError.write("FUSIONPROF total=\(Int(-t.timeIntervalSinceNow * 1000))\n".data(using: .utf8)!)
            }
        }
        guard options.zenzaiMode.enabled, let zenz = self.getModel(modelURL: options.zenzaiMode.weightURL) else { return nil }
        return zenz.scoreSequences(input: hiragana.toKatakana(), outputs: outputs, versionDependentConfig: options.zenzaiMode.versionDependentMode)
    }

    /// 融合採点: 語の並びの値 (各語の値 + 品詞の連接、BOS/EOS 込み)。Zenzai の候補の Candidate.value は
    /// 制約付きラティスで作られると辞書だけの最良と同じ値になっていることがあり比べられないので、同じ式で計算し直す
    /// 学習語は学習前の辞書の値で数える (学習の値 −3〜−8 は辞書の語のほぼ全部に勝つので、そのまま足すと 至難の業→至難の技 のように学習に引っ張られる)。
    /// AZK_FUSION_LEARNED_VALUE=raw で学習の値のまま
    public func fusionPathValue(_ data: [DicdataElement]) -> PValue {
        var value: PValue = 0
        var prev = CIDData.BOS.cid
        for e in data {
            let v = e.metadata.contains(.isLearned) && !Self.fusionRawLearnedValue ? self.dictionaryValue(of: e) : e.value()
            value += v + self.converter.dicdataStore.getCCValue(prev, e.lcid)
            prev = e.rcid
        }
        return value + self.converter.dicdataStore.getCCValue(prev, CIDData.EOS.cid)
    }

    private static let fusionRawLearnedValue = azkDevEnvironment["AZK_FUSION_LEARNED_VALUE"] == "raw"
    private var dictionaryValueCache: [String: PValue] = [:]

    /// 学習データを除いた辞書での値 (同じ読み・同じ表記)。辞書に無い語は同じ読みの最も低い値、それも無ければ −20
    private func dictionaryValue(of e: DicdataElement) -> PValue {
        let key = e.ruby + "\t" + e.word
        if let v = self.dictionaryValueCache[key] { return v }
        var composing = ComposingText()
        composing.insertAtCursorPosition(e.ruby.toHiragana(), inputStyle: .direct)
        let count = composing.input.count
        var result: PValue = -20
        if count > 0 {
            let nodes = self.converter.dicdataStore.getLOUDSData(inputData: composing, from: 0, to: count - 1, needTypoCorrection: false)
            let dict = nodes.map(\.data).filter { !$0.metadata.contains(.isLearned) && $0.ruby == e.ruby }
            if let same = dict.filter({ $0.word == e.word }).map({ $0.value() }).max() {
                result = same
            } else if let lowest = dict.map({ $0.value() }).min() {
                result = lowest
            }
        }
        if self.dictionaryValueCache.count > 4096 { self.dictionaryValueCache.removeAll() }
        self.dictionaryValueCache[key] = result
        return result
    }

    /// 2026-11-18 (azooKey-Windows、perf の 7): 別のスレッドで読み込んで温めた zenz (ZenzPreload) を使う。同じモデルをもう持っていれば何もしない
    public func installPreloadedZenz(_ preload: ZenzPreload) {
        if let model = self.zenz, model.resourceURL == preload.resourceURL { return }
        self.zenz = Zenz(resourceURL: preload.resourceURL, preloadedContext: preload.context)
        self.zenzStatus = "load \(preload.resourceURL.absoluteString)"
    }

    package func getModel(modelURL: URL) -> Zenz? {
        if let model = self.zenz, model.resourceURL == modelURL {
            self.zenzStatus = "load \(modelURL.absoluteString)"
            return model
        } else {
            do {
                self.zenz = try Zenz(resourceURL: modelURL)
                self.zenzStatus = "load \(modelURL.absoluteString)"
                return self.zenz
            } catch {
                self.zenzStatus = "load \(modelURL.absoluteString)    " + error.localizedDescription
                return nil
            }
        }
    }
    
    public func predictNextCharacter(leftSideContext: String, count: Int, options: ConvertRequestOptions) -> [(character: Character, value: Float)] {
        guard let zenz = self.getModel(modelURL: options.zenzaiMode.weightURL) else {
            print("zenz-v2 model unavailable")
            return []
        }
        guard options.zenzaiMode.versionDependentMode.version == .v2 else {
            print("next character prediction requires zenz-v2 models, not zenz-v1 nor zenz-v3 and later")
            return []
        }
        let results = zenz.predictNextCharacter(leftSideContext: leftSideContext, count: count)
        
        return results
    }

    /// 入力する言語が分かったらこの関数をなるべく早い段階で呼ぶことで、SpellCheckerの初期化が行われ、変換がスムーズになる
    public func setKeyboardLanguage(_ language: KeyboardLanguage) {
        if !checkerInitialized[language, default: false] {
            switch language {
            case .en_US:
                Task { @MainActor in
                    _ = self.checker.completions(forPartialWordRange: NSRange(location: 0, length: 1), in: "a", language: "en-US")
                    self.checkerInitialized[language] = true
                }
            case .el_GR:
                Task { @MainActor in
                    _ = self.checker.completions(forPartialWordRange: NSRange(location: 0, length: 1), in: "a", language: "el-GR")
                    self.checkerInitialized[language] = true
                }
            case .none, .ja_JP:
                checkerInitialized[language] = true
            }
        }
    }

    /// 上流の関数から`dicdataStore`で行うべき操作を伝播する関数。
    /// - Parameters:
    ///   - data: 行うべき操作。
    /// azooKey-Windows (2026-11-18、perf の 7 の 2b): 別のスレッドで読んだ本体の辞書を覚えに足す
    public func installPreloadedDictionary(_ preloaded: PreloadedDictionary) {
        self.converter.dicdataStore.installPreloadedDictionary(preloaded)
    }

    public func sendToDicdataStore(_ data: DicdataStore.Notification) {
        self.converter.dicdataStore.sendToDicdataStore(data)
    }
    /// 確定操作後、内部状態のキャッシュを変更する関数。
    /// - Parameters:
    ///   - candidate: 確定された候補。
    public func setCompletedData(_ candidate: Candidate) {
        self.completedData = candidate
    }

    /// 確定操作後、学習メモリをアップデートする関数。
    /// - Parameters:
    ///   - candidate: 確定された候補。
    public func updateLearningData(_ candidate: Candidate) {
        self.converter.dicdataStore.updateLearningData(candidate, with: self.lastData)
        self.lastData = candidate.data.last
    }

    /// 確定操作後、学習メモリをアップデートする関数。
    /// - Parameters:
    ///   - candidate: 確定された候補。
    public func updateLearningData(_ candidate: Candidate, with predictionCandidate: PostCompositionPredictionCandidate) {
        self.converter.dicdataStore.updateLearningData(candidate, with: predictionCandidate)
        self.lastData = predictionCandidate.lastData
    }

    /// 賢い変換候補を生成する関数。
    /// - Parameters:
    ///   - string: 入力されたString
    /// - Returns:
    ///   `賢い変換候補
    private func getWiseCandidate(_ inputData: ComposingText, options: ConvertRequestOptions) -> [Candidate] {
        var result = [Candidate]()

        // toWarekiCandidates/toSeirekiCandidatesは以前は設定可能にしていたが、特にoffにする需要がなさそうなので常時有効化した
        result.append(contentsOf: self.toWarekiCandidates(inputData))
        result.append(contentsOf: self.toSeirekiCandidates(inputData))
        result.append(contentsOf: self.toEmailAddressCandidates(inputData))

        if options.typographyLetterCandidate {
            result.append(contentsOf: self.typographicalCandidates(inputData))
        }
        if options.unicodeCandidate {
            result.append(contentsOf: self.unicodeCandidates(inputData))
        }
        result.append(contentsOf: self.toVersionCandidate(inputData, options: options))
        return result
    }

    /// 変換候補の重複を除去する関数。
    /// - Parameters:
    ///   - candidates: uniqueを実行する候補列。
    /// - Returns:
    ///   `candidates`から重複を削除したもの。
    private func getUniqueCandidate(_ candidates: some Sequence<Candidate>, seenCandidates: Set<String> = []) -> [Candidate] {
        var result = [Candidate]()
        var textIndex = [String: Int]()
        for candidate in candidates where !candidate.text.isEmpty && !seenCandidates.contains(candidate.text) {
            if let index = textIndex[candidate.text] {
                if result[index].value < candidate.value || result[index].correspondingCount < candidate.correspondingCount {
                    result[index] = candidate
                }
            } else {
                textIndex[candidate.text] = result.endIndex
                result.append(candidate)
            }
        }
        return result
    }

    /// 変換候補の重複を除去する関数。
    /// - Parameters:
    ///   - candidates: uniqueを実行する候補列。
    /// - Returns:
    ///   `candidates`から重複を削除したもの。
    private func getUniquePostCompositionPredictionCandidate(_ candidates: some Sequence<PostCompositionPredictionCandidate>, seenCandidates: Set<String> = []) -> [PostCompositionPredictionCandidate] {
        var result = [PostCompositionPredictionCandidate]()
        for candidate in candidates where !candidate.text.isEmpty && !seenCandidates.contains(candidate.text) {
            if let index = result.firstIndex(where: {$0.text == candidate.text}) {
                if result[index].value < candidate.value {
                    result[index] = candidate
                }
            } else {
                result.append(candidate)
            }
        }
        return result
    }

    /// 外国語への予測変換候補を生成する関数
    /// - Parameters:
    ///   - inputData: 変換対象のデータ。
    ///   - language: 言語コード。現在は`en-US`と`el(ギリシャ語)`のみ対応している。
    /// - Returns:
    ///   予測変換候補
    private func getForeignPredictionCandidate(inputData: ComposingText, language: String, penalty: PValue = -5) -> [Candidate] {
        switch language {
        case "en-US":
            var result: [Candidate] = []
            let ruby = String(inputData.input.map {$0.character})
            let range = NSRange(location: 0, length: ruby.utf16.count)
            if !ruby.onlyRomanAlphabet {
                return result
            }
            if let completions = checker.completions(forPartialWordRange: range, in: ruby, language: language) {
                if !completions.isEmpty {
                    let data = [DicdataElement(ruby: ruby, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: penalty)]
                    let candidate: Candidate = Candidate(
                        text: ruby,
                        value: penalty,
                        correspondingCount: inputData.input.count,
                        lastMid: MIDData.一般.mid,
                        data: data
                    )
                    result.append(candidate)
                }
                var value: PValue = -5 + penalty
                let delta: PValue = -10 / PValue(completions.count)
                for word in completions {
                    let data = [DicdataElement(ruby: word, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: value)]
                    let candidate: Candidate = Candidate(
                        text: word,
                        value: value,
                        correspondingCount: inputData.input.count,
                        lastMid: MIDData.一般.mid,
                        data: data
                    )
                    result.append(candidate)
                    value += delta
                }
            }
            return result
        case "el":
            var result: [Candidate] = []
            let ruby = String(inputData.input.map {$0.character})
            let range = NSRange(location: 0, length: ruby.utf16.count)
            if let completions = checker.completions(forPartialWordRange: range, in: ruby, language: language) {
                if !completions.isEmpty {
                    let data = [DicdataElement(ruby: ruby, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: penalty)]
                    let candidate: Candidate = Candidate(
                        text: ruby,
                        value: penalty,
                        correspondingCount: inputData.input.count,
                        lastMid: MIDData.一般.mid,
                        data: data
                    )
                    result.append(candidate)
                }
                var value: PValue = -5 + penalty
                let delta: PValue = -10 / PValue(completions.count)
                for word in completions {
                    let data = [DicdataElement(ruby: word, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: value)]
                    let candidate: Candidate = Candidate(
                        text: word,
                        value: value,
                        correspondingCount: inputData.input.count,
                        lastMid: MIDData.一般.mid,
                        data: data
                    )
                    result.append(candidate)
                    value += delta
                }
            }
            return result
        default:
            return []
        }
    }

    /// 予測変換候補を生成する関数
    /// - Parameters:
    ///   - sums: 変換対象のデータ。
    /// - Returns:
    ///   予測変換候補
    private func getPredictionCandidate(_ sums: [(CandidateData, Candidate)], composingText: ComposingText, options: ConvertRequestOptions) -> [Candidate] {
        // 予測変換は次の方針で行う。
        // prepart: 前半文節 lastPart: 最終文節とする。
        // まず、lastPartがnilであるところから始める

        var candidates: [Candidate] = []
        var prepart: CandidateData = sums.max {$0.1.value < $1.1.value}!.0
        var lastpart: CandidateData.ClausesUnit?
        var count = 0
        while true {
            if count == 2 {
                break
            }
            if prepart.isEmpty {
                break
            }
            if let oldlastPart = lastpart {
                // 現在の最終分節をもう1つ取得
                let lastUnit = prepart.clauses.popLast()!   // prepartをmutatingでlastを取る。
                let newUnit = lastUnit.clause               // 新しいlastpartとなる部分。
                newUnit.merge(with: oldlastPart.clause)     // マージする。(最終文節の範囲を広げたことになる)
                let newValue = lastUnit.value + oldlastPart.value
                let newlastPart: CandidateData.ClausesUnit = (clause: newUnit, value: newValue)
                let predictions = converter.getPredictionCandidates(composingText: composingText, prepart: prepart, lastClause: newlastPart.clause, N_best: 5)
                lastpart = newlastPart
                // 結果がemptyでなければ
                if !predictions.isEmpty {
                    candidates += predictions
                    count += 1
                }
            } else {
                // 最終分節を取得
                lastpart = prepart.clauses.popLast()
                // 予測変換を受け取る
                let predictions = converter.getPredictionCandidates(composingText: composingText, prepart: prepart, lastClause: lastpart!.clause, N_best: 5)
                // 結果がemptyでなければ
                if !predictions.isEmpty {
                    // 結果に追加
                    candidates += predictions
                    count += 1
                }
            }
        }
        return candidates
    }

    /// トップレベルに追加する付加的な変換候補を生成する関数
    /// - Parameters:
    ///   - inputData: 変換対象のInputData。
    /// - Returns:
    ///   付加的な変換候補
    private func getTopLevelAdditionalCandidate(_ inputData: ComposingText, options: ConvertRequestOptions) -> [Candidate] {
        var candidates: [Candidate] = []
        if inputData.input.allSatisfy({$0.inputStyle == .roman2kana}) {
            if options.englishCandidateInRoman2KanaInput {
                candidates.append(contentsOf: self.getForeignPredictionCandidate(inputData: inputData, language: "en-US", penalty: -10))
            }
        }
        return candidates
    }
    /// 部分がカタカナである可能性を調べる
    /// 小さいほどよい。
    private func getKatakanaScore<S: StringProtocol>(_ katakana: S) -> PValue {
        var score: PValue = 1
        // テキスト分析によってこれらのカタカナが入っている場合カタカナ語である可能性が高いと分かった。
        for c in katakana {
            if "プヴペィフ".contains(c) {
                score *= 0.5
            } else if "ュピポ".contains(c) {
                score *= 0.6
            } else if "パォグーム".contains(c) {
                score *= 0.7
            }
        }
        return score
    }

    /// 付加的な変換候補を生成する関数
    /// - Parameters:
    ///   - inputData: 変換対象のInputData。
    /// - Returns:
    ///   付加的な変換候補
    private func getAdditionalCandidate(_ inputData: ComposingText, options: ConvertRequestOptions) -> [Candidate] {
        var candidates: [Candidate] = []
        let string = inputData.convertTarget.toKatakana()
        let correspondingCount = inputData.input.count
        do {
            // カタカナ
            let value = -14 * getKatakanaScore(string)
            let data = DicdataElement(ruby: string, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: value)
            let katakana = Candidate(
                text: string,
                value: value,
                correspondingCount: correspondingCount,
                lastMid: MIDData.一般.mid,
                data: [data]
            )
            candidates.append(katakana)
        }
        let hiraganaString = string.toHiragana()
        do {
            // ひらがな
            let data = DicdataElement(word: hiraganaString, ruby: string, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -14.5)

            let hiragana = Candidate(
                text: hiraganaString,
                value: -14.5,
                correspondingCount: correspondingCount,
                lastMid: MIDData.一般.mid,
                data: [data]
            )
            candidates.append(hiragana)
        }
        do {
            // 大文字
            let word = string.uppercased()
            let data = DicdataElement(word: word, ruby: string, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -15)
            let uppercasedLetter = Candidate(
                text: word,
                value: -14.6,
                correspondingCount: correspondingCount,
                lastMid: MIDData.一般.mid,
                data: [data]
            )
            candidates.append(uppercasedLetter)
        }
        if options.fullWidthRomanCandidate {
            // 全角英数字
            let word = string.applyingTransform(.fullwidthToHalfwidth, reverse: true) ?? ""
            let data = DicdataElement(word: word, ruby: string, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -15)
            let fullWidthLetter = Candidate(
                text: word,
                value: -14.7,
                correspondingCount: correspondingCount,
                lastMid: MIDData.一般.mid,
                data: [data]
            )
            candidates.append(fullWidthLetter)
        }
        if options.halfWidthKanaCandidate {
            // 半角カタカナ
            let word = string.applyingTransform(.fullwidthToHalfwidth, reverse: false) ?? ""
            let data = DicdataElement(word: word, ruby: string, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -15)
            let halfWidthKatakana = Candidate(
                text: word,
                value: -15,
                correspondingCount: correspondingCount,
                lastMid: MIDData.一般.mid,
                data: [data]
            )
            candidates.append(halfWidthKatakana)
        }

        return candidates
    }

    /// ラティスを処理し変換候補の形にまとめる関数
    /// - Parameters:
    ///   - inputData: 変換対象のInputData。
    ///   - result: convertToLatticeによって得られた結果。
    ///   - options: リクエストにかかるオプション。
    /// - Returns:
    ///   重複のない変換候補。
    /// - Note:
    ///   現在の実装は非常に複雑な方法で候補の順序を決定している。
    private func processResult(inputData: ComposingText, result: (result: LatticeNode, nodes: [[LatticeNode]]), options: ConvertRequestOptions) -> ConversionResult {
        self.previousInputData = inputData
        self.nodes = result.nodes
        let clauseResult = result.result.getCandidateData()
        if clauseResult.isEmpty {
            let candidates = self.getUniqueCandidate(self.getAdditionalCandidate(inputData, options: options))
            return ConversionResult(mainResults: candidates, firstClauseResults: candidates)   // アーリーリターン
        }
        let clauseCandidates: [Candidate] = clauseResult.map {(candidateData: CandidateData) -> Candidate in
            let first = candidateData.clauses.first!
            var count = 0
            do {
                var str = ""
                while true {
                    str += candidateData.data[count].word
                    if str == first.clause.text {
                        break
                    }
                    count += 1
                }
            }
            return Candidate(
                text: first.clause.text,
                value: first.value,
                correspondingCount: first.clause.inputRange.count,
                lastMid: first.clause.mid,
                data: Array(candidateData.data[0...count])
            )
        }
        let sums: [(CandidateData, Candidate)] = clauseResult.map {($0, converter.processClauseCandidate($0))}
        // 文章全体を変換した場合の候補上位5件を作る
        let whole_sentence_unique_candidates = self.getUniqueCandidate(sums.map {$0.1})
        if case .完全一致 = options.requestQuery {
            if options.zenzaiMode.enabled {
                return ConversionResult(mainResults: whole_sentence_unique_candidates, firstClauseResults: [])
            } else {
                return ConversionResult(mainResults: whole_sentence_unique_candidates.sorted(by: {$0.value > $1.value}), firstClauseResults: [])
            }
        }
        // モデル重みを統合
        let sentence_candidates: [Candidate]
        if options.zenzaiMode.enabled {
            // FIXME: もう少し良い方法はありそうだけど、短期的にかなりハックな実装にした
            // candidateのvalueをZenzaiの出力順に書き換えることで、このあとのrerank処理で騙されてくれるようになっている
            // より根本的には、`Candidate`にAI評価値をもたせるなどの方法が必要そう
            var first5 = Array(whole_sentence_unique_candidates.prefix(5))
            let values = first5.map(\.value).sorted(by: >)
            for (i, v) in zip(first5.indices, values) {
                first5[i].value = v
            }
            sentence_candidates = first5
        } else {
            sentence_candidates = whole_sentence_unique_candidates.min(count: 5, sortedBy: {$0.value > $1.value})
        }
        // 予測変換を最大3件作成する
        let prediction_candidates: [Candidate] = options.requireJapanesePrediction ? Array(self.getUniqueCandidate(self.getPredictionCandidate(sums, composingText: inputData, options: options)).min(count: 3, sortedBy: {$0.value > $1.value})) : []

        // 英単語の予測変換。appleのapiを使うため、処理が異なる。
        var foreign_candidates: [Candidate] = []

        if options.requireEnglishPrediction {
            foreign_candidates.append(contentsOf: self.getForeignPredictionCandidate(inputData: inputData, language: "en-US"))
        }
        if options.keyboardLanguage == .el_GR {
            foreign_candidates.append(contentsOf: self.getForeignPredictionCandidate(inputData: inputData, language: "el"))
        }

        // 文全体変換5件と予測変換3件を混ぜてベスト8を出す
        let best8 = getUniqueCandidate(sentence_candidates.prefix(5).chained(prediction_candidates)).sorted {$0.value > $1.value}
        // その他のトップレベル変換（先頭に表示されうる変換候補）
        let toplevel_additional_candidate = self.getTopLevelAdditionalCandidate(inputData, options: options)
        // best8、foreign_candidates、zeroHintPrediction_candidates、toplevel_additional_candidateを混ぜて上位5件を取得する
        let full_candidate = getUniqueCandidate(
            best8
                .chained(foreign_candidates)
                .chained(toplevel_additional_candidate)
        ).min(count: 5, sortedBy: {$0.value > $1.value})
        // 重複のない変換候補を作成するための集合
        var seenCandidate: Set<String> = full_candidate.mapSet {$0.text}
        // 文節のみ変換するパターン（上位5件）
        let clause_candidates = self.getUniqueCandidate(clauseCandidates, seenCandidates: seenCandidate).min(count: 5) {
            if $0.correspondingCount == $1.correspondingCount {
                $0.value > $1.value
            } else {
                $0.correspondingCount > $1.correspondingCount
            }
        }
        seenCandidate.formUnion(clause_candidates.map {$0.text})

        // 最初の辞書データ
        let dicCandidates: [Candidate] = result.nodes[0]
            .map {
                Candidate(
                    text: $0.data.word,
                    value: $0.data.value(),
                    correspondingCount: $0.inputRange.count,
                    lastMid: $0.data.mid,
                    data: [$0.data]
                )
            }
        // その他辞書データに追加する候補
        let additionalCandidates: [Candidate] = self.getAdditionalCandidate(inputData, options: options)

        // 文字列の長さごとに並べ、かつその中で評価の高いものから順に並べる。
        var word_candidates: [Candidate] = self.getUniqueCandidate(dicCandidates.chained(additionalCandidates), seenCandidates: seenCandidate)
            .sorted {
                let count0 = $0.correspondingCount
                let count1 = $1.correspondingCount
                return count0 == count1 ? $0.value > $1.value : count0 > count1
            }
        seenCandidate.formUnion(word_candidates.map {$0.text})

        // 賢く変換するパターン（任意件数）
        let wise_candidates: [Candidate] = self.getUniqueCandidate(self.getWiseCandidate(inputData, options: options), seenCandidates: seenCandidate)
        // 途中でwise_candidatesを挟む
        word_candidates.insert(contentsOf: wise_candidates, at: min(5, word_candidates.endIndex))

        var result = Array(full_candidate)

        // 3番目までに最低でも1つ、（誤り訂正ではなく）入力に完全一致する候補が入るようにする
        let checkRuby: (Candidate) -> Bool = {$0.data.reduce(into: "") {$0 += $1.ruby} == inputData.convertTarget.toKatakana()}
        if !result.prefix(3).contains(where: checkRuby) {
            if let candidateIndex = result.dropFirst(3).firstIndex(where: checkRuby) {
                // 3番目以降にある場合は順位を入れ替える
                let candidate = result.remove(at: candidateIndex)
                result.insert(candidate, at: min(result.endIndex, 2))
            } else if let candidate = sentence_candidates.first(where: checkRuby) {
                result.insert(candidate, at: min(result.endIndex, 2))
            } else if let candidate = whole_sentence_unique_candidates.first(where: checkRuby) {
                result.insert(candidate, at: min(result.endIndex, 2))
            }
        }

        result.append(contentsOf: clause_candidates)
        result.append(contentsOf: word_candidates)

        result.mutatingForeach { item in
            item.withActions(self.getAppropriateActions(item))
            item.parseTemplate()
        }
        // 文節のみ変換するパターン（上位5件）
        let firstClauseResults = self.getUniqueCandidate(clauseCandidates).min(count: 5) {
            if $0.correspondingCount == $1.correspondingCount {
                $0.value > $1.value
            } else {
                $0.correspondingCount > $1.correspondingCount
            }
        }
        return ConversionResult(mainResults: result, firstClauseResults: firstClauseResults)
    }

    /// 入力からラティスを構築する関数。状況に応じて呼ぶ関数を分ける。
    /// - Parameters:
    ///   - inputData: 変換対象のInputData。
    ///   - N_best: 計算途中で保存する候補数。実際に得られる候補数とは異なる。
    /// - Returns:
    ///   結果のラティスノードと、計算済みノードの全体
    private func convertToLattice(_ inputData: ComposingText, N_best: Int, zenzaiMode: ConvertRequestOptions.ZenzaiMode) -> (result: LatticeNode, nodes: [[LatticeNode]])? {
        if inputData.convertTarget.isEmpty {
            return nil
        }

        // FIXME: enable cache based zenzai
        if zenzaiMode.enabled, let model = self.getModel(modelURL: zenzaiMode.weightURL) {
            var pool: [Candidate] = []
            let (result, nodes, cache) = withUnsafeMutablePointer(to: &pool) { poolPointer in
                self.converter.all_zenzai(
                    inputData,
                    zenz: model,
                    zenzaiCache: self.zenzaiCache,
                    inferenceLimit: zenzaiMode.inferenceLimit,
                    requestRichCandidates: zenzaiMode.requestRichCandidates,
                    personalizationMode: self.getZenzaiPersonalization(mode: zenzaiMode.personalizationMode),
                    versionDependentConfig: zenzaiMode.versionDependentMode,
                    fusionPool: self.collectFusionPool ? poolPointer : nil
                )
            }
            self.lastZenzaiPool = pool
            self.zenzaiCache = cache
            self.previousInputData = inputData
            self.latticeFromZenzai = true
            return (result, nodes)
        }
        #if os(iOS)
        let needTypoCorrection = true
        #else
        let needTypoCorrection = false
        #endif

        // 前の格子が Zenzai の下書きなら使い回さない (同じ入力・文節確定のあと・打ち足し・消しの、どの道も)
        guard let previousInputData, !latticeFromZenzai else {
            debug("convertToLattice: 新規計算用の関数を呼びますA")
            let result = converter.kana2lattice_all(inputData, N_best: N_best, needTypoCorrection: needTypoCorrection)
            self.previousInputData = inputData
            self.latticeFromZenzai = false
            return result
        }

        debug("convertToLattice: before \(previousInputData) after \(inputData)")

        // 完全一致の場合
        if previousInputData == inputData {
            let result = converter.kana2lattice_no_change(N_best: N_best, previousResult: (inputData: previousInputData, nodes: nodes))
            self.previousInputData = inputData
            return result
        }

        // 文節確定の後の場合
        if let completedData, previousInputData.inputHasSuffix(inputOf: inputData) {
            debug("convertToLattice: 文節確定用の関数を呼びます、確定された文節は\(completedData)")
            let result = converter.kana2lattice_afterComplete(inputData, completedData: completedData, N_best: N_best, previousResult: (inputData: previousInputData, nodes: nodes), needTypoCorrection: needTypoCorrection)
            self.previousInputData = inputData
            self.completedData = nil
            return result
        }

        // TODO: 元々はsuffixになっていないが、文節確定の後であるケースで、確定された文節を考慮できるようにする
        // へんかん|する → 変換 する|　のようなパターンで、previousInputData: へんかん, inputData: する, となることがある

        let diff = inputData.differenceSuffix(to: previousInputData)

        // 一文字消した場合
        if diff.deleted > 0 && diff.addedCount == 0 {
            debug("convertToLattice: 最後尾削除用の関数を呼びます, 消した文字数は\(diff.deleted)")
            let result = converter.kana2lattice_deletedLast(deletedCount: diff.deleted, N_best: N_best, previousResult: (inputData: previousInputData, nodes: nodes))
            self.previousInputData = inputData
            return result
        }

        // 一文字変わった場合
        if diff.deleted > 0 {
            debug("convertToLattice: 最後尾文字置換用の関数を呼びます、差分は\(diff)")
            let result = converter.kana2lattice_changed(inputData, N_best: N_best, counts: (diff.deleted, diff.addedCount), previousResult: (inputData: previousInputData, nodes: nodes), needTypoCorrection: needTypoCorrection)
            self.previousInputData = inputData
            return result
        }

        // 1文字増やした場合
        if diff.deleted == 0 && diff.addedCount != 0 {
            debug("convertToLattice: 最後尾追加用の関数を呼びます、追加文字数は\(diff.addedCount)")
            let result = converter.kana2lattice_added(inputData, N_best: N_best, addedCount: diff.addedCount, previousResult: (inputData: previousInputData, nodes: nodes), needTypoCorrection: needTypoCorrection)
            self.previousInputData = inputData
            return result
        }

        // 一文字増やしていない場合
        if true {
            debug("convertToLattice: 新規計算用の関数を呼びますB")
            let result = converter.kana2lattice_all(inputData, N_best: N_best, needTypoCorrection: needTypoCorrection)
            self.previousInputData = inputData
            return result
        }
    }

    public func getAppropriateActions(_ candidate: Candidate) -> [CompleteAction] {
        if ["[]", "()", "｛｝", "〈〉", "〔〕", "（）", "「」", "『』", "【】", "{}", "<>", "《》", "\"\"", "\'\'", "””"].contains(candidate.text) {
            return [.moveCursor(-1)]
        }
        if ["{{}}"].contains(candidate.text) {
            return [.moveCursor(-2)]
        }
        return []

    }

    /// 2つの連続する`Candidate`をマージする
    public func mergeCandidates(_ left: Candidate, _ right: Candidate) -> Candidate {
        converter.mergeCandidates(left, right)
    }

    /// 外部から呼ばれる変換候補を要求する関数。
    /// - Parameters:
    ///   - inputData: 変換対象のInputData。
    ///   - options: リクエストにかかるパラメータ。
    /// - Returns: `ConversionResult`
    public func requestCandidates(_ inputData: ComposingText, options: ConvertRequestOptions) -> ConversionResult {
        debug("requestCandidates 入力は", inputData)
        // 変換対象が無の場合
        if inputData.convertTarget.isEmpty {
            return ConversionResult(mainResults: [], firstClauseResults: [])
        }

        // DicdataStoreにRequestOptionを通知する
        self.sendToDicdataStore(.setRequestOptions(options))

        guard let result = self.convertToLattice(inputData, N_best: options.N_best, zenzaiMode: options.zenzaiMode) else {
            return ConversionResult(mainResults: [], firstClauseResults: [])
        }

        return self.processResult(inputData: inputData, result: result, options: options)
    }

    /// Recover a dictionary path for the selected text without replacing live neural/lattice caches.
    public func dictionaryCandidate(_ inputData: ComposingText, matching text: String, options: ConvertRequestOptions) -> Candidate? {
        guard !inputData.convertTarget.isEmpty, !text.isEmpty else { return nil }
        self.sendToDicdataStore(.setRequestOptions(options))
        let constraint = Kana2Kanji.PrefixConstraint(Array(text.utf8))
        let result = converter.kana2lattice_all_with_prefix_constraint(inputData, N_best: options.N_best, constraint: constraint)
        return result.result.getCandidateData().map { converter.processClauseCandidate($0) }
            .filter { $0.text == text && $0.correspondingCount == inputData.input.count }
            .max { $0.value < $1.value }
    }

    /// 変換確定後の予測変換候補を要求する関数
    public func requestPostCompositionPredictionCandidates(leftSideCandidate: Candidate, options: ConvertRequestOptions) -> [PostCompositionPredictionCandidate] {
        // ゼロヒント予測変換に基づく候補を列挙
        var zeroHintResults = self.getUniquePostCompositionPredictionCandidate(self.converter.getZeroHintPredictionCandidates(preparts: [leftSideCandidate], N_best: 15))
        do {
            // 助詞は最大3つに制限する
            var joshiCount = 0
            zeroHintResults = zeroHintResults.reduce(into: []) { results, candidate in
                switch candidate.type {
                case .additional(data: let data):
                    if CIDData.isJoshi(cid: data.last?.rcid ?? CIDData.EOS.cid) {
                        if joshiCount < 3 {
                            results.append(candidate)
                            joshiCount += 1
                        }
                    } else {
                        results.append(candidate)
                    }
                case .replacement:
                    results.append(candidate)
                }
            }
        }

        // 予測変換に基づく候補を列挙
        let predictionResults = self.converter.getPredictionCandidates(prepart: leftSideCandidate, N_best: 15)
        // 絵文字を追加
        let replacer = options.textReplacer
        var emojiCandidates: [PostCompositionPredictionCandidate] = []
        for data in leftSideCandidate.data where DicdataStore.includeMMValueCalculation(data) {
            let result = replacer.getSearchResult(query: data.word, target: [.emoji], ignoreNonBaseEmoji: true)
            for emoji in result {
                emojiCandidates.append(PostCompositionPredictionCandidate(text: emoji.text, value: -3, type: .additional(data: [.init(word: emoji.text, ruby: "エモジ", cid: CIDData.記号.cid, mid: MIDData.一般.mid, value: -3)])))
            }
        }
        emojiCandidates = self.getUniquePostCompositionPredictionCandidate(emojiCandidates)

        var results: [PostCompositionPredictionCandidate] = []
        var seenCandidates: Set<String> = []

        results.append(contentsOf: emojiCandidates.suffix(3))
        seenCandidates.formUnion(emojiCandidates.suffix(3).map {$0.text})

        // 残りの半分。ただしzeroHintResultsが足りない場合は全部で10個になるようにする。
        let predictionsCount = max((10 - results.count) / 2, 10 - results.count - zeroHintResults.count)
        let predictions =  self.getUniquePostCompositionPredictionCandidate(predictionResults, seenCandidates: seenCandidates).min(count: predictionsCount, sortedBy: {$0.value > $1.value})
        results.append(contentsOf: predictions)
        seenCandidates.formUnion(predictions.map {$0.text})

        let zeroHints = self.getUniquePostCompositionPredictionCandidate(zeroHintResults, seenCandidates: seenCandidates)
        results.append(contentsOf: zeroHints.min(count: 10 - results.count, sortedBy: {$0.value > $1.value}))
        return results
    }
}
