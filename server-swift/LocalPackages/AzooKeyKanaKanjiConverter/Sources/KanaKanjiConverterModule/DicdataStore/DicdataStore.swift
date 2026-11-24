//
//  DicdataStore.swift
//  Keyboard
//
//  Created by ensan on 2020/09/17.
//  Copyright © 2020 ensan. All rights reserved.
//

import Foundation
import SwiftUtils

public final class DicdataStore {
    public init(convertRequestOptions: ConvertRequestOptions) {
        self.requestOptions = convertRequestOptions
        self.setup()
    }

    init(requestOptions: ConvertRequestOptions = .default) {
        self.requestOptions = requestOptions
        debug("DicdataStoreが初期化されました")
        self.setup()
    }

    private var ccParsed: [Bool] = .init(repeating: false, count: 1319)
    private var ccLines: [[Int: PValue]] = []
    private var mmValue: [PValue] = []

    private var loudses: [String: LOUDS] = [:]
    private var loudstxts: [String: Data] = [:]    
    private var importedLoudses: Set<String> = []
    /// azooKey-Windows (2026-11-18、perf の 7): 引いたときに写像しようとして無かった本体の辞書の .loudstxt3 (何度も開き直さない)
    private var mainLoudstxtMissing: Set<String> = []
    private var charsID: [Character: UInt8] = [:]
    private var learningManager = LearningManager()

    private var dynamicUserDict: [DicdataElement] = []
    /// azooKey-Windows (2026-11-04): 動的ユーザ辞書の読み → 語 (単語登録で数千語になるので、読みごとに引く。前は毎回すべてをなめた)
    private var dynamicUserDictByRuby: [String: [DicdataElement]] = [:]

    /// azooKey-Windows: 分野辞書 (例: 法律)。identifier は "domain_law" のように "domain_" で始め、
    /// ファイルは `domainDirectoryURL` に置く (形式は学習データと同じ LOUDS)。空なら何もしない
    nonisolated(unsafe) public static var domainIdentifiers: [String] = []
    nonisolated(unsafe) public static var domainDirectoryURL: URL? = nil
    /// azooKey-Windows: 読み間違いの本来の形を入れた分野辞書 (例: "domain_core_misreadings")。引いた語に isReadingCorrection の印を付ける (2026-10-31)
    nonisolated(unsafe) public static var readingCorrectionDomains: Set<String> = []
    /// azooKey-Windows: 常用外の字の表記を入れた分野辞書 (例: "domain_core_rare_kanji")。引いた語に isRareSpelling の印を付ける (2026-11-08)
    nonisolated(unsafe) public static var rareSpellingDomains: Set<String> = []
    /// azooKey-Windows: true のあいだ、rareSpellingDomains の分野辞書を引かない (表が無いときの候補の一覧を作り直すため。2026-11-08)
    nonisolated(unsafe) public static var hideRareSpelling = false
    /// azooKey-Windows (2026-11-20、学習の効きすぎ): 学習の場所 (memory) から引いた要素のうち、使わないものを決める。真を返した要素は格子に入れない。
    /// 2 つ目の引数は「読み (カタカナ) と字が、本体の辞書に 1 語で載っているか」を引く関数。変換サーバーが起動のときに置く。nil なら全部使う (上流と同じ)
    nonisolated(unsafe) public static var learnedElementGuard: (@Sendable (DicdataElement, (String, String) -> Bool) -> Bool)? = nil
    /// azooKey-Windows: 分野辞書の設定を読み直す (分野のオンオフ・ファイルの置き換え) たびに変換サーバーが増やす。
    /// 変わっていたら、分野辞書の持ち物 (LOUDS・.loudstxt3・読めなかった印) を捨てて読み直す (2026-10-22、D の調べ R2-3・D2-6)
    nonisolated(unsafe) public static var domainGeneration: Int = 0
    /// azooKey-Windows: 持っておく分野辞書の .loudstxt3 の合計の上限 (配っている 10 分野と追加辞書の全部で 6MB ほど)
    public static let domainLoudstxtTotalMaxBytes = 128 << 20
    private var seenDomainGeneration: Int = 0
    private var domainLoudstxtBytes: Int = 0
    private var domainLoudstxtSkipped: Set<String> = []
    /// azooKey-Windows: 語ごとの値の上書き ("読み(カタカナ)\t表記" → 値)。辞書の値が壊れている語の修正用。学習データには効かせない
    nonisolated(unsafe) public static var valueOverrides: [String: PValue] = [:]

    /// 辞書のエントリの最大長さ
    ///  - TODO: make this value as an option
    public let maxlength: Int = 20
    /// この値以下のスコアを持つエントリは積極的に無視する
    ///  - TODO: make this value as an option
    public let threshold: PValue = -17
    private let midCount = 502
    private let cidCount = 1319

    private var requestOptions: ConvertRequestOptions = .default

    private let numberFormatter = NumberFormatter()
    /// 初期化時のセットアップ用の関数。プロパティリストを読み込み、連接確率リストを読み込んで行分割し保存しておく。
    /// azooKey-Windows (2026-11-08 仕事 49): 格子の始点の右の品詞。nil なら文頭 (BOS)。KanaKanjiConverter.setStartRcid で入れる
    package var startRcid: Int? = nil
    private var startNode: RegisteredNode { self.startRcid.map { RegisteredNode.startNode(rcid: $0) } ?? RegisteredNode.BOSNode() }

    private func setup() {
        numberFormatter.numberStyle = .spellOut
        numberFormatter.locale = .init(identifier: "ja-JP")
        self.ccLines = [[Int: PValue]].init(repeating: [:], count: CIDData.totalCount)

        do {
            let string = try String(contentsOf: self.requestOptions.dictionaryResourceURL.appendingPathComponent("louds/charID.chid", isDirectory: false), encoding: String.Encoding.utf8)
            charsID = [Character: UInt8].init(uniqueKeysWithValues: string.enumerated().map {($0.element, UInt8($0.offset))})
        } catch {
            debug("Error: louds/charID.chidが存在しません。このエラーは深刻ですが、テスト時には無視できる場合があります。Description: \(error)")
        }
        do {
            let url = requestOptions.dictionaryResourceURL.appendingPathComponent("mm.binary", isDirectory: false)
            do {
                let binaryData = try Data(contentsOf: url, options: [.uncached])
                self.mmValue = binaryData.toArray(of: Float.self).map {PValue($0)}
            } catch {
                debug("Error: mm.binaryが存在しません。このエラーは深刻ですが、テスト時には無視できる場合があります。Description: \(error)")
                self.mmValue = [PValue].init(repeating: .zero, count: self.midCount * self.midCount)
            }
        }
        _ = self.loadLOUDS(query: "user")
        _ = self.loadLOUDS(query: "memory")

        if requestOptions.preloadDictionary {
            self.preloadDictionary()
        }
    }

    /// azooKey-Windows (2026-11-10): 先読みする本体の辞書の .loudstxt3 (438 ファイル・19MB) は、読み込まずにファイルを写像して持つ。
    /// 読み込むと変換サーバーの private のメモリにそのまま数えられていた (重さの目安はサーバーのメモリ 200MB)。写像は OS が引いたページだけを読む。
    /// 分野辞書 (domain_*) は実行中に置き換わる (domainGeneration) ので写像しない (Windows では写像のあいだファイルを置き換えられない)。
    /// 開発用の切り替え: AZK_MAPPED=0 で前と同じに読み込む (試験では、辞書の置き場を読み込む前に替えて比べる)
    nonisolated(unsafe) public static var mapPreloadedDictionary = azkDevEnvironment["AZK_MAPPED"] != "0"

    /// ファイルI/Oの遅延を減らすために、辞書を事前に読み込む関数。
    private func preloadDictionary() {
        guard let fileURLs = try? FileManager.default.contentsOfDirectory(
            at: self.requestOptions.dictionaryResourceURL.appendingPathComponent("louds", isDirectory: true),
            includingPropertiesForKeys: nil
        ) else { return }

        for url in fileURLs {
            let identifier = url.deletingPathExtension().lastPathComponent
            let pathExt = url.pathExtension
            
            switch pathExt {
                case "louds":
                    // userやmemoryは実行中に更新される場合があるため、キャッシュから除外
                    if identifier == "user" || identifier == "memory" {
                        continue
                    }
                    loudses[identifier] = LOUDS.load(identifier, option: self.requestOptions)
                case "loudstxt3":
                    if let data = try? Data(contentsOf: url, options: Self.mapPreloadedDictionary ? [.alwaysMapped] : []) {
                        loudstxts[identifier] = data
                    } else {
                        debug("Error: Could not load loudstxt3 file at \(url)")
                    }
                default:
                    continue
            }
        }
    }

    public enum Notification {
        /// use `importDynamicUserDict` for data that cannot be obtained statically.
        /// - warning: Too many dynamic user dictionary will damage conversion performance, as dynamic user dictionary uses inefficent algorithms for looking up. If your entries can be listed up statically, then use normal user dictionaries.
        case importDynamicUserDict([DicdataElement])
        @available(*, deprecated, renamed: "importDynamicUserDict", message: "it will be removed in AzooKeyKanaKanjiConverter v1.0")
        case importOSUserDict([DicdataElement])
        case setRequestOptions(ConvertRequestOptions)
        case forgetMemory(Candidate)
        case closeKeyboard
        /// azooKey-Windows (2026-11-08): 学習をまっさらにする (設定の画面の「学習をまっさらにする」。控えは呼ぶ側が先に取る)
        case resetMemory
    }

    func sendToDicdataStore(_ data: Notification) {
        switch data {
        case .closeKeyboard:
            self.closeKeyboard()
        case .importOSUserDict(let dicdata), .importDynamicUserDict(let dicdata):
            self.dynamicUserDict = dicdata
            // azooKey-Windows (2026-11-04): ほかの印 (単語登録の isUserRegistered) は残す (前は印を上書きしていた)
            self.dynamicUserDict.mutatingForeach {
                $0.metadata.insert(.isFromUserDictionary)
            }
            self.dynamicUserDictByRuby = Dictionary(grouping: self.dynamicUserDict, by: \.ruby)
        case let .forgetMemory(candidate):
            self.learningManager.forgetMemory(data: candidate.data)
            // loudsの処理があるので、リセットを実施する
            self.reloadMemory()
        case .resetMemory:
            self.learningManager.reset()
            self.reloadMemory()
        case let .setRequestOptions(value):
            // bundleURLが変わる場合はsetupを再実行する
            if value.dictionaryResourceURL != self.requestOptions.dictionaryResourceURL {
                self.requestOptions = value
                self.setup()
            } else {
                self.requestOptions = value
            }
            let shouldReset = self.learningManager.setRequestOptions(options: value)
            if shouldReset {
                self.reloadMemory()
            }
        }
    }

    func character2charId(_ character: Character) -> UInt8 {
        self.charsID[character, default: .max]
    }

    private func reloadMemory() {
        self.loudses.removeValue(forKey: "memory")
        self.importedLoudses.remove("memory")
    }

    private func reloadUser() {
        self.loudses.removeValue(forKey: "user")
        self.importedLoudses.remove("user")
    }

    private func closeKeyboard() {
        self.learningManager.save()
        // saveしたあとにmemoryのキャッシュされたLOUDSを使い続けないよう、キャッシュから削除する。
        self.reloadMemory()
        self.reloadUser()
    }

    /// ペナルティ関数。文字数で決める。
    @inlinable static func getPenalty(data: borrowing DicdataElement) -> PValue {
        -2.0 / PValue(data.word.count)
    }

    /// 計算時に利用。無視すべきデータかどうか。
    private func shouldBeRemoved(value: PValue, wordCount: Int) -> Bool {
        let d = value - self.threshold
        if d < 0 {
            return true
        }
        // dは正
        return -2.0 / PValue(wordCount) < -d
    }

    /// 計算時に利用。無視すべきデータかどうか。
    @inlinable func shouldBeRemoved(data: borrowing DicdataElement) -> Bool {
        let d = data.value() - self.threshold
        if d < 0 {
            return true
        }
        return Self.getPenalty(data: data) < -d
    }

    /// azooKey-Windows: 設定が読み直されていたら、分野辞書の持ち物を捨てる
    private func dropStaleDomainCaches() {
        guard self.seenDomainGeneration != Self.domainGeneration else { return }
        self.seenDomainGeneration = Self.domainGeneration
        for key in Array(self.loudses.keys) where key.hasPrefix("domain_") { self.loudses[key] = nil }
        self.importedLoudses = self.importedLoudses.filter { !$0.hasPrefix("domain_") }
        for key in Array(self.loudstxts.keys) where key.hasPrefix("domain_") { self.loudstxts[key] = nil }
        self.domainLoudstxtBytes = 0
        self.domainLoudstxtSkipped = []
    }

    /// azooKey-Windows (2026-11-18、perf の 7 の 2b): 別のスレッドで読んだ本体の辞書 (PreloadedDictionary) を覚えに足す。主のスレッドから呼ぶ。
    /// もう引いて読んだ頭の字 (importedLoudses) はそのまま。足した頭の字には読んだ印を付ける (付けないと、最初に引いたときにディスクから読み直す。
    /// 前の先読み (preloadDictionary) は印を付けていなかったので、LOUDS は最初に引いたときに読み直していた)。
    /// エスケープした名前 ([0A] など) は引くときの名前と違うので、LOUDS は足さない (引いたときに読む)
    func installPreloadedDictionary(_ preloaded: PreloadedDictionary) {
        for (identifier, louds) in preloaded.loudses where !identifier.hasPrefix("[") && !self.importedLoudses.contains(identifier) {
            self.loudses[identifier] = louds
            self.importedLoudses.insert(identifier)
        }
        for (shard, data) in preloaded.loudstxts where self.loudstxts[shard] == nil {
            self.loudstxts[shard] = data
        }
    }

    func loadLOUDS(query: String) -> LOUDS? {
        if query.hasPrefix("domain_") {
            self.dropStaleDomainCaches()
        }
        if importedLoudses.contains(query) {
            return self.loudses[query]
        }
        // LOUDSが読み込めたか否かにかかわらず、importedLoudsesは更新する
        importedLoudses.insert(query)
        // azooKey-Windows (2026-11-23、セキュリティの点検 12): 学習は、範囲の外を読まない形か確かめてから読む (壊れていれば退けて、空の学習で続ける)
        if query == "memory", !LongTermLearningMemory.prepareForReading(directoryURL: self.requestOptions.memoryDirectoryURL) {
            return nil
        }
        // 一部のASCII文字はエスケープする
        let identifier = [
            "\\n": "[0A]",
            " ": "[20]",
            "\"": "[22]",
            "\'": "[27]",
            "*": "[2A]",
            "+": "[2B]",
            ".": "[2E]",
            "/": "[2F]",
            ":": "[3A]",
            "<": "[3C]",
            ">": "[3E]",
            "\\": "[5C]",
            "|": "[7C]",
        ][query, default: query]
        if let louds = LOUDS.load(identifier, option: self.requestOptions) {
            self.loudses[query] = louds
            return louds
        } else {
            if identifier == "user" || identifier == "memory" {
                debug("Error: IDが「\(identifier) (query: \(query))」のloudsファイルの読み込みに失敗しましたが、このエラーは深刻ではありません。")
            } else {
                debug("Error: IDが「\(identifier) (query: \(query))」のloudsファイルの読み込みに失敗しました。IDに対する辞書データが存在しないことが想定される場合はこのエラーは深刻ではありませんが、そうでない場合は深刻なエラーの可能性があります。")
            }
            return nil
        }
    }

    private func perfectMatchLOUDS(query: String, charIDs: [UInt8]) -> [Int] {
        guard let louds = self.loadLOUDS(query: query) else {
            return []
        }
        return [louds.searchNodeIndex(chars: charIDs)].compactMap {$0}
    }

    private func throughMatchLOUDS(query: String, charIDs: [UInt8], depth: Range<Int>) -> [Int] {
        guard let louds = self.loadLOUDS(query: query) else {
            return []
        }
        let result = louds.byfixNodeIndices(chars: charIDs)
        // result[1]から始まるので、例えば3..<5 (3文字と4文字)の場合は1文字ずつずらして4..<6の範囲をもらう
        return Array(result[min(depth.lowerBound + 1, result.endIndex) ..< min(depth.upperBound + 1, result.endIndex)])
    }

    private func prefixMatchLOUDS(query: String, charIDs: [UInt8], depth: Int = .max, maxCount: Int = .max) -> [Int] {
        guard let louds = self.loadLOUDS(query: query) else {
            return []
        }
        return louds.prefixNodeIndices(chars: charIDs, maxDepth: depth, maxCount: maxCount)
    }

    /// azooKey-Windows (2026-11-20、学習の効きすぎ): 学習の要素を learnedElementGuard で選り分ける (置かれていなければそのまま)
    func filterLearned(_ data: [DicdataElement]) -> [DicdataElement] {
        guard let guard_ = Self.learnedElementGuard, !data.isEmpty else { return data }
        return data.filter { !guard_($0, { ruby, word in self.mainDictionaryContains(ruby: ruby, word: word) }) }
    }

    /// azooKey-Windows (2026-11-20): 読み (カタカナ) と字が、本体の辞書に 1 語で載っているか (学習・利用者の辞書・分野辞書は見ない)
    func mainDictionaryContains(ruby: String, word: String) -> Bool {
        guard let first = ruby.first else { return false }
        let charIDs = ruby.map(self.character2charId)
        let indices = self.perfectMatchLOUDS(query: String(first), charIDs: charIDs)
        return self.getDicdataFromLoudstxt3(identifier: String(first), indices: indices).contains { $0.word == word }
    }

    package func getDicdataFromLoudstxt3(identifier: String, indices: some Sequence<Int>) -> [DicdataElement] {
        if Self.hideRareSpelling && Self.rareSpellingDomains.contains(identifier) {
            return []
        }
        // split = 2048
        let dict = [Int: [Int]].init(grouping: indices, by: {$0 >> 11})
        var data: [DicdataElement] = []
        for (key, value) in dict {
            let shard = identifier + "\(key)"
            // azooKey-Windows: 分野辞書の .loudstxt3 は本体の辞書と違って先読みされないので、最初に引いたときに読み込んで持っておく。
            // 持たずに引くたびにファイルを開いていたため、法律・生物・医学・化学の 4 分野で 1 キーあたりのファイルの読み出しが 0.3 回→37 回になり、
            // 写経で 1 キーの待ちが延びた (2026-10-22、生物の文の 99% 103→545ms)
            // 読む前に大きさを確かめ、1 ファイル・合計の上限を超えるもの・読めないものは引かない (上流の読み方は大きさを確かめずに丸ごと読むので、そちらへは回さない。R2-3)
            if identifier.hasPrefix("domain_") {
                self.dropStaleDomainCaches()
                if self.loudstxts[shard] == nil {
                    if self.domainLoudstxtSkipped.contains(shard) { continue }
                    guard let loaded = LOUDS.loadLoudstxt3(shard, option: self.requestOptions, maxBytes: LOUDS.domainFileMaxBytes),
                          self.domainLoudstxtBytes + loaded.count <= Self.domainLoudstxtTotalMaxBytes else {
                        self.domainLoudstxtSkipped.insert(shard)
                        continue
                    }
                    self.loudstxts[shard] = loaded
                    self.domainLoudstxtBytes += loaded.count
                }
            } else if self.loudstxts[shard] == nil, !identifier.hasPrefix("user"), !identifier.hasPrefix("memory"), !self.mainLoudstxtMissing.contains(shard) {
                // azooKey-Windows (2026-11-18、perf の 7): 本体の辞書を先読みしないとき (preloadDictionary が偽) も、.loudstxt3 は最初に引いたときに写像して覚える。
                // 覚えないと引くたびにファイルを開き直す (分野辞書で 10-22 に起きたのと同じ)。先読みと同じ写像なので private のメモリには数えない。
                // 学習 (memory) と利用者の辞書 (user) は実行中に書き換わるので覚えない (前と同じ)
                if let mapped = LOUDS.mapLoudstxt3(shard, option: self.requestOptions) {
                    self.loudstxts[shard] = mapped
                } else {
                    self.mainLoudstxtMissing.insert(shard)
                }
            }
            data.append(contentsOf: LOUDS.getDataForLoudstxt3(shard, indices: value.map {$0 & 2047}, cache: self.loudstxts[shard], option: self.requestOptions))
        }
        if identifier == "memory" {
            data.mutatingForeach {
                $0.metadata = .isLearned
            }
            // azooKey-Windows (2026-11-20、学習の効きすぎ): 使わない学習の要素を外す (learnedElementGuard)
            data = self.filterLearned(data)
        } else if !Self.valueOverrides.isEmpty {
            data.mutatingForeach {
                if let v = Self.valueOverrides[$0.ruby + "\t" + $0.word] {
                    $0.baseValue = v
                }
            }
        }
        if identifier == "user" {
            data.mutatingForeach {
                $0.metadata = .isFromUserDictionary
            }
        }
        if Self.readingCorrectionDomains.contains(identifier) {
            data.mutatingForeach {
                $0.metadata.insert(.isReadingCorrection)
            }
        }
        if Self.rareSpellingDomains.contains(identifier) {
            data.mutatingForeach {
                $0.metadata.insert(.isRareSpelling)
            }
        }
        return data
    }

    /// 2026-10-29 (azooKey-Windows): 学習した語のうち、同じ読み・同じ表記の辞書の語が無いもの (利用者が新しく作った語。空ネコ のような語) に、ユーザ辞書の印も付ける。
    /// Zenzai はユーザ辞書の語を評価のプロンプトの「辞書:」で知り、前置きの制約も素通りさせる (学習の語のままだと、モデルが別の字を好むと毎回消えた)。
    /// 辞書にある語の学習 (技 など) と、読みが 2 字までの語 (こ → 娘 のように、文のどこにでも出る読み) は今までどおり (ほかの文が学習に引っ張られないように)。
    /// AZK_NEW_WORDS_AS_USER=0 で切れる
    static let newWordsAsUserDictionary = azkDevEnvironment["AZK_NEW_WORDS_AS_USER"] != "0"

    /// 2026-11-08 (azooKey-Windows、ただし伊豆の件): 辞書の語を助詞・助動詞でつないだ学習の語 (文節のつながりの学習。伊豆を ＋ 見せて →「伊豆を見せて」) には
    /// 新しい語の印を付けない。辞書に 1 語としては無いが、利用者が新しく作った語ではない。印が付くと、評価の優先が ×3 になり融合採点の選び直しもしないので、
    /// 辞書だけの表示を 1 回確定しただけで、AI の判断 (正しい図を見せて) をその形 (ただし伊豆を見せて) で上書きしていた。
    /// AZK_NEW_WORDS_COMPOSED=mark で前と同じに付ける (比べるとき)
    static let markComposedLearnedWords = azkDevEnvironment["AZK_NEW_WORDS_COMPOSED"] == "mark"
    /// 辞書の語だけで切り分けられるかの覚え ((読み<TAB>表記) → 切り分けられるか)
    private var composedOfDictionaryWordsCache: [String: Bool] = [:]
    /// 切り分けを調べている間の辞書の引きでは、印を付け直さない (同じ調べが入れ子にならないように)
    private var checkingComposedWords = false

    private func markNewLearnedWords(_ dicdata: inout [DicdataElement]) {
        guard Self.newWordsAsUserDictionary, !self.checkingComposedWords, dicdata.contains(where: { $0.metadata.contains(.isLearned) }) else {
            return
        }
        var known = Set<String>()
        for data in dicdata where !data.metadata.contains(.isLearned) {
            known.insert(data.ruby + "\t" + data.word)
        }
        for i in dicdata.indices where dicdata[i].metadata.contains(.isLearned) && dicdata[i].ruby.count >= 3
            && !known.contains(dicdata[i].ruby + "\t" + dicdata[i].word)
            && (Self.markComposedLearnedWords || !self.isComposedOfDictionaryWords(ruby: dicdata[i].ruby, word: dicdata[i].word)) {
            dicdata[i].metadata.insert(.isFromUserDictionary)
        }
    }

    /// 2026-11-08 (azooKey-Windows): 読み ruby (カタカナ) と表記 word を、頭から辞書の語 (学習でない語。単語登録した語は含む) だけで切り分けられ、
    /// しかもその中に助詞か助動詞 (品詞の番号 147〜554、azooKey の penaltyRatio と同じ範囲) があるか (文節のつながり: 伊豆 ＋ を ＋ 見せ ＋ て)。
    /// 空ネコ (空 ＋ ネコ) のように、利用者が内容語を並べて作った語は、辞書の語で切り分けられても偽 (今までどおり新しい語の印を付ける)。
    /// 辞書に無い語を含むとき・読みを入力の字に直せないときも偽
    private func isComposedOfDictionaryWords(ruby: String, word: String) -> Bool {
        let key = ruby + "\t" + word
        if let cached = self.composedOfDictionaryWordsCache[key] {
            return cached
        }
        self.checkingComposedWords = true
        defer { self.checkingComposedWords = false }
        let rubyChars = Array(ruby)
        let wordChars = Array(word)
        var composing = ComposingText()
        composing.insertAtCursorPosition(ruby.toHiragana(), inputStyle: .direct)
        var result = false
        if composing.input.count == rubyChars.count && !rubyChars.isEmpty {
            // reachable[i]: 読みの i 字目までを切り分けたとき、来られる (表記の字の位置 × 2 + 助詞・助動詞を使ったか)
            var reachable: [Set<Int>] = Array(repeating: [], count: rubyChars.count + 1)
            reachable[0] = [0]
            for i in 0 ..< rubyChars.count where !reachable[i].isEmpty {
                for j in i + 1 ... rubyChars.count {
                    let part = String(rubyChars[i ..< j])
                    // 読みから作った形 (1 字のかな・カタカナ・数字・記号) は辞書の語ではないので数えない
                    for node in self.getLOUDSData(inputData: composing, from: i, to: j - 1, needTypoCorrection: false, includeWiseData: false)
                    where !node.data.metadata.contains(.isLearned) && node.data.ruby == part {
                        let piece = Array(node.data.word)
                        let function = 147 ... 554 ~= node.data.lcid ? 1 : 0
                        for state in reachable[i] {
                            let p = state / 2
                            if p + piece.count <= wordChars.count && Array(wordChars[p ..< p + piece.count]) == piece {
                                reachable[j].insert((p + piece.count) * 2 + max(state % 2, function))
                            }
                        }
                    }
                }
            }
            result = reachable[rubyChars.count].contains(wordChars.count * 2 + 1)
        }
        if self.composedOfDictionaryWordsCache.count > 4096 {
            self.composedOfDictionaryWordsCache.removeAll()
        }
        self.composedOfDictionaryWordsCache[key] = result
        return result
    }

    /// azooKey-Windows: 学習した語が読み間違いの本来の形 (isReadingCorrection の辞書の語と同じ読み・表記) なら、学習の語にも同じ印を付ける。
    /// 付けないと、本来の形を選んで確定する (学習する) と、次から学習の語が辞書の語より先に選ばれ、AI の変換で 不インキ に戻った (2026-10-31)
    private func markLearnedReadingCorrections(_ dicdata: inout [DicdataElement]) {
        guard !Self.readingCorrectionDomains.isEmpty else {
            return
        }
        let corrections = Set(dicdata.lazy.filter { $0.metadata.contains(.isReadingCorrection) }.map { $0.ruby + "\t" + $0.word })
        guard !corrections.isEmpty else {
            return
        }
        for i in dicdata.indices where dicdata[i].metadata.contains(.isLearned) && corrections.contains(dicdata[i].ruby + "\t" + dicdata[i].word) {
            dicdata[i].metadata.insert(.isReadingCorrection)
        }
    }

    /// kana2latticeから参照する。
    /// - Parameters:
    ///   - inputData: 入力データ
    ///   - from: 起点
    ///   - toIndexRange: `from ..< (toIndexRange)`の範囲で辞書ルックアップを行う。
    public func getLOUDSDataInRange(inputData: ComposingText, from fromIndex: Int, toIndexRange: Range<Int>? = nil, needTypoCorrection: Bool = true) -> [LatticeNode] {
        if !needTypoCorrection {
            return self.getFrozenLOUDSDataInRange(inputData: inputData, from: fromIndex, toIndexRange: toIndexRange)
        }
        let toIndexLeft = toIndexRange?.startIndex ?? fromIndex
        let toIndexRight = min(toIndexRange?.endIndex ?? inputData.input.count, fromIndex + self.maxlength)
        if fromIndex > toIndexLeft || toIndexLeft >= toIndexRight {
            debug("getLOUDSDataInRange: index is wrong")
            return []
        }

        let segments = (fromIndex ..< toIndexRight).reduce(into: []) { (segments: inout [String], rightIndex: Int) in
            segments.append((segments.last ?? "") + String(inputData.input[rightIndex].character.toKatakana()))
        }
        // MARK: 誤り訂正の対象を列挙する。非常に重い処理。
        var stringToInfo = inputData.getRangesWithTypos(fromIndex, rightIndexRange: toIndexLeft ..< toIndexRight)
        // MARK: 検索対象を列挙していく。
        let stringSet = stringToInfo.keys.map {($0, $0.map(self.character2charId))}
        let (minCharIDsCount, maxCharIDsCount) = stringSet.lazy.map {$0.1.count}.minAndMax() ?? (0, -1)
        // 先頭の文字: そこで検索したい文字列の集合
        let group = [Character: [([Character], [UInt8])]].init(grouping: stringSet, by: {$0.0.first!})

        let depth = minCharIDsCount - 1 ..< maxCharIDsCount
        var indices: [(String, Set<Int>)] = group.map {dic in
            let key = String(dic.key)
            let set = dic.value.flatMapSet {(_, charIDs) in self.throughMatchLOUDS(query: key, charIDs: charIDs, depth: depth)}
            return (key, set)
        }
        indices.append(("user", stringSet.flatMapSet {self.throughMatchLOUDS(query: "user", charIDs: $0.1, depth: depth)}))
        for domain in Self.domainIdentifiers {
            indices.append((domain, stringSet.flatMapSet {self.throughMatchLOUDS(query: domain, charIDs: $0.1, depth: depth)}))
        }
        if learningManager.enabled {
            indices.append(("memory", stringSet.flatMapSet {self.throughMatchLOUDS(query: "memory", charIDs: $0.1, depth: depth)}))
        }
        // MARK: 検索によって得たindicesから辞書データを実際に取り出していく
        var dicdata: [DicdataElement] = []
        for (identifier, value) in indices {
            let result: [DicdataElement] = self.getDicdataFromLoudstxt3(identifier: identifier, indices: value).compactMap { (data) -> DicdataElement? in
                let rubyArray = Array(data.ruby)
                let penalty = stringToInfo[rubyArray, default: (0, .zero)].penalty
                if penalty.isZero {
                    return data
                }
                let ratio = Self.penaltyRatio[data.lcid]
                let pUnit: PValue = Self.getPenalty(data: data) / 2   // 負の値
                let adjust = pUnit * penalty * ratio
                if self.shouldBeRemoved(value: data.value() + adjust, wordCount: rubyArray.count) {
                    return nil
                }
                return data.adjustedData(adjust)
            }
            dicdata.append(contentsOf: result)
        }
        // temporalな学習結果にpenaltyを加えて追加する
        for (_, charIds) in consume stringSet {
            for data in self.filterLearned(self.learningManager.temporaryThroughMatch(charIDs: consume charIds, depth: depth)) {
                let rubyArray = Array(data.ruby)
                let penalty = stringToInfo[rubyArray, default: (0, .zero)].penalty
                if penalty.isZero {
                    dicdata.append(data)
                }
                let ratio = Self.penaltyRatio[data.lcid]
                let pUnit: PValue = Self.getPenalty(data: data) / 2   // 負の値
                let adjust = pUnit * penalty * ratio
                if self.shouldBeRemoved(value: data.value() + adjust, wordCount: rubyArray.count) {
                    continue
                }
                dicdata.append(data.adjustedData(adjust))
            }
        }

        for i in toIndexLeft ..< toIndexRight {
            do {
                let result = self.getWiseDicdata(convertTarget: segments[i - fromIndex], inputData: inputData, inputRange: fromIndex ..< i + 1)
                for item in result {
                    stringToInfo[Array(item.ruby)] = (i, 0)
                }
                dicdata.append(contentsOf: result)
            }
            do {
                let result = self.getMatchDynamicUserDict(segments[i - fromIndex])
                for item in result {
                    stringToInfo[Array(item.ruby)] = (i, 0)
                }
                dicdata.append(contentsOf: result)
            }
        }
        self.markNewLearnedWords(&dicdata)
        self.markLearnedReadingCorrections(&dicdata)
        if fromIndex == .zero {
            let result: [LatticeNode] = dicdata.compactMap {
                guard let endIndex = stringToInfo[Array($0.ruby)]?.endIndex else {
                    return nil
                }
                let node = LatticeNode(data: $0, inputRange: fromIndex ..< endIndex + 1)
                node.prevs.append(self.startNode)
                return node
            }
            return result
        } else {
            let result: [LatticeNode] = dicdata.compactMap {
                guard let endIndex = stringToInfo[Array($0.ruby)]?.endIndex else {
                    return nil
                }
                return LatticeNode(data: $0, inputRange: fromIndex ..< endIndex + 1)
            }
            return result
        }
    }

    /// kana2latticeから参照する。
    /// - Parameters:
    ///   - inputData: 入力データ
    ///   - from: 起点
    ///   - toIndexRange: `from ..< (toIndexRange)`の範囲で辞書ルックアップを行う。
    private func getFrozenLOUDSDataInRange(inputData: ComposingText, from fromIndex: Int, toIndexRange: Range<Int>? = nil) -> [LatticeNode] {
        let toIndexLeft = toIndexRange?.startIndex ?? fromIndex
        let toIndexRight = min(toIndexRange?.endIndex ?? inputData.input.count, fromIndex + self.maxlength)
        debug("getLOUDSDataInRange", fromIndex, toIndexRange?.description ?? "nil", toIndexLeft, toIndexRight)
        if fromIndex > toIndexLeft || toIndexLeft >= toIndexRight {
            debug("getLOUDSDataInRange: index is wrong")
            return []
        }

        let segments = (fromIndex ..< toIndexRight).reduce(into: []) { (segments: inout [String], rightIndex: Int) in
            segments.append((segments.last ?? "") + String(inputData.input[rightIndex].character.toKatakana()))
        }
        let character = String(inputData.input[fromIndex].character.toKatakana())
        let characterNode = LatticeNode(data: DicdataElement(word: character, ruby: character, cid: CIDData.一般名詞.cid, mid: MIDData.一般.mid, value: -10), inputRange: fromIndex ..< fromIndex + 1)
        if fromIndex == .zero {
            characterNode.prevs.append(self.startNode)
        }

        // MARK: 誤り訂正なし
        let stringToEndIndex = inputData.getRanges(fromIndex, rightIndexRange: toIndexLeft ..< toIndexRight)
        // MARK: 検索対象を列挙していく。
        guard let (minString, maxString) = stringToEndIndex.keys.minAndMax(by: {$0.count < $1.count}) else {
            return [characterNode]
        }
        let maxIDs = maxString.map(self.character2charId)
        var keys = [String(stringToEndIndex.keys.first!.first!), "user"] + Self.domainIdentifiers
        if learningManager.enabled {
            keys.append("memory")
        }
        // MARK: 検索によって得たindicesから辞書データを実際に取り出していく
        var dicdata: [DicdataElement] = []
        let depth = minString.count - 1 ..< maxString.count
        for identifier in keys {
            dicdata.append(contentsOf: self.getDicdataFromLoudstxt3(identifier: identifier, indices: self.throughMatchLOUDS(query: identifier, charIDs: maxIDs, depth: depth)))
        }
        if learningManager.enabled {
            // temporalな学習結果にpenaltyを加えて追加する
            dicdata.append(contentsOf: self.filterLearned(self.learningManager.temporaryThroughMatch(charIDs: consume maxIDs, depth: depth)))
        }
        for i in toIndexLeft ..< toIndexRight {
            dicdata.append(contentsOf: self.getWiseDicdata(convertTarget: segments[i - fromIndex], inputData: inputData, inputRange: fromIndex ..< i + 1))
        }
        for item in stringToEndIndex {
            dicdata.append(contentsOf: self.getMatchDynamicUserDict(String(item.key)))
        }
        self.markNewLearnedWords(&dicdata)
        self.markLearnedReadingCorrections(&dicdata)
        if fromIndex == .zero {
            return dicdata.compactMap {
                guard let endIndex = stringToEndIndex[Array($0.ruby)] else {
                    return nil
                }
                let node = LatticeNode(data: $0, inputRange: fromIndex ..< endIndex + 1)
                node.prevs.append(self.startNode)
                return node
            } + [characterNode]
        } else {
            return dicdata.compactMap {
                guard let endIndex = stringToEndIndex[Array($0.ruby)] else {
                    return nil
                }
                return LatticeNode(data: $0, inputRange: fromIndex ..< endIndex + 1)
            } + [characterNode]
        }
    }

    /// kana2latticeから参照する。louds版。
    /// - Parameters:
    ///   - inputData: 入力データ
    ///   - from: 始点
    ///   - to: 終点
    /// azooKey-Windows (2026-11-09): includeWiseData が偽なら、読みから作った形 (1 字のかな・カタカナ・数字・記号) を入れない (辞書の語だけを見るとき)
    public func getLOUDSData(inputData: ComposingText, from fromIndex: Int, to toIndex: Int, needTypoCorrection: Bool, includeWiseData: Bool = true) -> [LatticeNode] {
        if toIndex - fromIndex > self.maxlength || fromIndex > toIndex {
            return []
        }
        let segment = inputData.input[fromIndex...toIndex].reduce(into: "") {$0.append($1.character)}.toKatakana()

        // TODO: 最適化の余地あり
        let string2penalty = inputData.getRangeWithTypos(fromIndex, toIndex).filter {
            needTypoCorrection || $0.value == 0.0
        }

        // MARK: 検索によって得たindicesから辞書データを実際に取り出していく
        // 先頭の文字: そこで検索したい文字列の集合
        let strings = string2penalty.keys.map {
            (key: $0, charIDs: $0.map(self.character2charId))
        }
        let group = [Character: [(key: [Character], charIDs: [UInt8])]].init(grouping: strings, by: {$0.key.first!})

        var indices: [(String, Set<Int>)] = group.map {dic in
            let head = String(dic.key)
            let set = dic.value.flatMapSet { (_, charIDs) in
                self.perfectMatchLOUDS(query: head, charIDs: charIDs)
            }
            return (head, set)
        }
        do {
            let set = strings.flatMapSet { (_, charIDs) in
                self.perfectMatchLOUDS(query: "user", charIDs: charIDs)
            }
            indices.append(("user", set))
        }
        for domain in Self.domainIdentifiers {
            let set = strings.flatMapSet { (_, charIDs) in
                self.perfectMatchLOUDS(query: domain, charIDs: charIDs)
            }
            indices.append((domain, set))
        }
        if learningManager.enabled {
            let set = strings.flatMapSet { (_, charIDs) in
                self.perfectMatchLOUDS(query: "memory", charIDs: charIDs)
            }
            indices.append(("memory", set))
        }
        var dicdata: [DicdataElement] = []
        for (identifier, value) in indices {
            let result: [DicdataElement] = self.getDicdataFromLoudstxt3(identifier: identifier, indices: value).compactMap { (data) -> DicdataElement? in
                let rubyArray = Array(data.ruby)
                let penalty = string2penalty[rubyArray, default: .zero]
                if penalty.isZero {
                    return data
                }
                let ratio = Self.penaltyRatio[data.lcid]
                let pUnit: PValue = Self.getPenalty(data: data) / 2   // 負の値
                let adjust = pUnit * penalty * ratio
                if self.shouldBeRemoved(value: data.value() + adjust, wordCount: rubyArray.count) {
                    return nil
                }
                return data.adjustedData(adjust)
            }
            dicdata.append(contentsOf: result)
        }
        // temporalな学習結果にpenaltyを加えて追加する
        for (characters, charIds) in consume strings {
            for data in self.filterLearned(self.learningManager.temporaryPerfectMatch(charIDs: consume charIds)) {
                // perfect matchなので、Array(data.ruby)はcharactersに等しい
                let penalty = string2penalty[characters, default: .zero]
                if penalty.isZero {
                    dicdata.append(data)
                }
                let ratio = Self.penaltyRatio[data.lcid]
                let pUnit: PValue = Self.getPenalty(data: data) / 2   // 負の値
                let adjust = pUnit * penalty * ratio
                if self.shouldBeRemoved(value: data.value() + adjust, wordCount: characters.count) {
                    continue
                }
                dicdata.append(data.adjustedData(adjust))
            }
        }

        if includeWiseData {
            dicdata.append(contentsOf: self.getWiseDicdata(convertTarget: segment, inputData: inputData, inputRange: fromIndex ..< toIndex + 1))
        }
        for segment in string2penalty.keys {
            dicdata.append(contentsOf: self.getMatchDynamicUserDict(String(segment)))
        }

        if fromIndex == .zero {
            let result: [LatticeNode] = dicdata.map {
                let node = LatticeNode(data: $0, inputRange: fromIndex ..< toIndex + 1)
                node.prevs.append(self.startNode)
                return node
            }
            return result
        } else {
            let result: [LatticeNode] = dicdata.map {LatticeNode(data: $0, inputRange: fromIndex ..< toIndex + 1)}
            return result
        }
    }

    func getZeroHintPredictionDicdata(lastRcid: Int) -> [DicdataElement] {
        do {
            let csvString = try String(contentsOf: requestOptions.dictionaryResourceURL.appendingPathComponent("p/pc_\(lastRcid).csv", isDirectory: false), encoding: .utf8)
            let csvLines = csvString.split(separator: "\n")
            let csvData = csvLines.map {$0.split(separator: ",", omittingEmptySubsequences: false)}
            let dicdata: [DicdataElement] = csvData.map {self.parseLoudstxt2FormattedEntry(from: $0)}
            return dicdata
        } catch {
            debug("Error: 右品詞ID\(lastRcid)のためのZero Hint Predictionのためのデータの読み込みに失敗しました。このエラーは深刻ですが、テスト時には無視できる場合があります。 Description: \(error.localizedDescription)")
            return []
        }
    }

    /// 辞書から予測変換データを読み込む関数
    /// - Parameters:
    ///   - head: 辞書を引く文字列
    /// - Returns:
    ///   発見されたデータのリスト。
    func getPredictionLOUDSDicdata(key: some StringProtocol) -> [DicdataElement] {
        let count = key.count
        if count == .zero {
            return []
        }
        // 最大700件に絞ることによって低速化を回避する。
        let maxCount = 700
        var result: [DicdataElement] = []
        let first = String(key.first!)
        let charIDs = key.map(self.character2charId)
        // 1, 2文字に対する予測変換は候補数が大きいので、depth（〜文字数）を制限する
        let depth = if count == 1 {
            3
        } else if count == 2 {
            5
        } else {
            Int.max
        }
        let prefixIndices = self.prefixMatchLOUDS(query: first, charIDs: charIDs, depth: depth, maxCount: maxCount)

        result.append(
            contentsOf: self.getDicdataFromLoudstxt3(identifier: first, indices: Set(prefixIndices))
                .filter { Self.predictionUsable[$0.rcid] }
        )
        let userDictIndices = self.prefixMatchLOUDS(query: "user", charIDs: charIDs, maxCount: maxCount)
        result.append(contentsOf: self.getDicdataFromLoudstxt3(identifier: "user", indices: Set(consume userDictIndices)))
        for domain in Self.domainIdentifiers {
            let domainIndices = self.prefixMatchLOUDS(query: domain, charIDs: charIDs, maxCount: maxCount)
            result.append(contentsOf: self.getDicdataFromLoudstxt3(identifier: domain, indices: Set(consume domainIndices)).filter { Self.predictionUsable[$0.rcid] })
        }
        if learningManager.enabled {
            let memoryDictIndices = self.prefixMatchLOUDS(query: "memory", charIDs: charIDs, maxCount: maxCount)
            result.append(contentsOf: self.getDicdataFromLoudstxt3(identifier: "memory", indices: Set(consume memoryDictIndices)))
            result.append(contentsOf: self.filterLearned(self.learningManager.temporaryPrefixMatch(charIDs: charIDs)))
        }
        return result
    }

    private func parseLoudstxt2FormattedEntry(from dataString: [some StringProtocol]) -> DicdataElement {
        let ruby = String(dataString[0])
        let word = dataString[1].isEmpty ? ruby:String(dataString[1])
        let lcid = Int(dataString[2]) ?? .zero
        let rcid = Int(dataString[3]) ?? lcid
        let mid = Int(dataString[4]) ?? .zero
        let value: PValue = PValue(dataString[5]) ?? -30.0
        return DicdataElement(word: word, ruby: ruby, lcid: lcid, rcid: rcid, mid: mid, value: value)
    }

    /// 補足的な辞書情報を得る。
    ///  - parameters:
    ///     - convertTarget: カタカナ変換済みの文字列
    /// - note
    ///     - 入力全体をカタカナとかひらがなに変換するやつは、Converter側でやっているので注意。
    func getWiseDicdata(convertTarget: String, inputData: ComposingText, inputRange: Range<Int>) -> [DicdataElement] {
        var result: [DicdataElement] = []
        result.append(contentsOf: self.getJapaneseNumberDicdata(head: convertTarget))
        if inputData.input[..<inputRange.startIndex].last?.character.isNumber != true && inputData.input[inputRange.endIndex...].first?.character.isNumber != true, let number = Int(convertTarget) {
            result.append(DicdataElement(ruby: convertTarget, cid: CIDData.数.cid, mid: MIDData.小さい数字.mid, value: -14))
            if Double(number) <= 1E12 && -1E12 <= Double(number), let kansuji = self.numberFormatter.string(from: NSNumber(value: number)) {
                result.append(DicdataElement(word: kansuji, ruby: convertTarget, cid: CIDData.数.cid, mid: MIDData.小さい数字.mid, value: -16))
            }
        }

        // convertTargetを英単語として候補に追加する
        if requestOptions.keyboardLanguage == .en_US && convertTarget.onlyRomanAlphabet {
            result.append(DicdataElement(ruby: convertTarget, cid: CIDData.固有名詞.cid, mid: MIDData.英単語.mid, value: -14))
        }

        // ローマ字入力の場合、単体でひらがな・カタカナ化した候補も追加
        if requestOptions.keyboardLanguage != .en_US && inputData.input[inputRange].allSatisfy({$0.inputStyle == .roman2kana}) {
            if let katakana = Roman2Kana.katakanaChanges[convertTarget], let hiragana = Roman2Kana.hiraganaChanges[Array(convertTarget)] {
                result.append(DicdataElement(word: String(hiragana), ruby: katakana, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -13))
                result.append(DicdataElement(ruby: katakana, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -14))
            }
        }

        // 入力を全てひらがな、カタカナに変換したものを候補に追加する
        if convertTarget.count == 1 {
            let katakana = convertTarget.toKatakana()
            let hiragana = convertTarget.toHiragana()
            if convertTarget == katakana {
                result.append(DicdataElement(ruby: katakana, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -14))
            } else {
                result.append(DicdataElement(word: hiragana, ruby: katakana, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -13))
                result.append(DicdataElement(ruby: katakana, cid: CIDData.固有名詞.cid, mid: MIDData.一般.mid, value: -14))
            }
        }

        // 記号変換
        if convertTarget.count == 1, let first = convertTarget.first {
            var value: PValue = -14
            let hs = Self.fullwidthToHalfwidth[first, default: first]

            if hs != first {
                result.append(DicdataElement(word: convertTarget, ruby: convertTarget, cid: CIDData.記号.cid, mid: MIDData.一般.mid, value: value))
                value -= 5.0
                result.append(DicdataElement(word: String(hs), ruby: convertTarget, cid: CIDData.記号.cid, mid: MIDData.一般.mid, value: value))
                value -= 5.0
            }
            if let fs = Self.halfwidthToFullwidth[first], fs != first {
                result.append(DicdataElement(word: convertTarget, ruby: convertTarget, cid: CIDData.記号.cid, mid: MIDData.一般.mid, value: value))
                value -= 5.0
                result.append(DicdataElement(word: String(fs), ruby: convertTarget, cid: CIDData.記号.cid, mid: MIDData.一般.mid, value: value))
                value -= 5.0
            }
            for group in Self.weakRelatingSymbolGroups where group.contains(hs) {
                for symbol in group where symbol != hs {
                    result.append(DicdataElement(word: String(symbol), ruby: convertTarget, cid: CIDData.記号.cid, mid: MIDData.一般.mid, value: value))
                    value -= 5.0
                    if let fs = Self.halfwidthToFullwidth[symbol] {
                        result.append(DicdataElement(word: String(fs), ruby: convertTarget, cid: CIDData.記号.cid, mid: MIDData.一般.mid, value: value))
                        value -= 5.0
                    }
                }
            }
        }
        return result
    }

    // 記号に対する半角・全角変換
    private static let (fullwidthToHalfwidth, halfwidthToFullwidth) = zip(
        "＋ー＊＝・！＃％＆＇＂〜｜￡＄￥＠｀；：＜＞，．＼／＿￣－",
        "＋ー＊＝・！＃％＆＇＂〜｜￡＄￥＠｀；：＜＞，．＼／＿￣－".applyingTransform(.fullwidthToHalfwidth, reverse: false)!
    )
    .reduce(into: ([Character: Character](), [Character: Character]())) { (results: inout ([Character: Character], [Character: Character]), values: (Character, Character)) in
        results.0[values.0] = values.1
        results.1[values.1] = values.0
    }

    // 弱い類似(矢印同士のような関係)にある記号をグループにしたもの
    // 例えば→に対して⇒のような記号はより類似度が強いため、上位に出したい。これを実現する必要が生じた場合はstrongRelatingSymbolGroupsを新設する。
    // 宣言順不同
    // 1つを入れると他が出る、というイメージ
    // 半角と全角がある場合は半角のみ
    private static let weakRelatingSymbolGroups: [[Character]] = [
        // 異体字セレクト用 (試験実装)
        ["高", "髙"], // ハシゴダカ
        ["斎", "斉", "齋", "齊"],
        ["澤", "沢"],
        ["気", "氣"],
        ["澁", "渋"],
        ["対", "對"],
        ["辻", "辻󠄀"],
        ["禰󠄀", "禰"],
        ["煉󠄁", "煉"],
        ["崎", "﨑"], // タツザキ
        ["栄", "榮"],
        ["吉", "𠮷"], // ツチヨシ
        ["橋", "𣘺", "槗", "𫞎"],
        ["浜", "濱", "濵"],
        ["鴎", "鷗"],
        ["学", "學"],
        ["角", "⻆"],
        ["亀", "龜"],
        ["桜", "櫻"],
        ["真", "眞"],

        // 記号変換
        ["☆", "★", "♡", "☾", "☽"],  // 星
        ["^", "＾"],  // ハット
        ["¥", "$", "¢", "€", "£", "₿"], // 通貨
        ["%", "‰"], // パーセント
        ["°", "℃", "℉"],
        ["◯"], // 図形
        ["*", "※", "✳︎", "✴︎"],   // こめ
        ["・", "…", "‥", "•"],
        ["+", "±", "⊕"],
        ["×", "❌", "✖️"],
        ["÷", "➗" ],
        ["<", "≦", "≪", "〈", "《", "‹", "«"],
        [">", "≧", "≫", "〉", "》", "›", "»"],
        ["=", "≒", "≠", "≡"],
        [":", ";"],
        ["!", "❗️", "❣️", "‼︎", "⁉︎", "❕", "‼️", "⁉️", "¡"],
        ["?", "❓", "⁉︎", "⁇", "❔", "⁉️", "¿"],
        ["〒", "〠", "℡", "☎︎"],
        ["々", "ヾ", "ヽ", "ゝ", "ゞ", "〃", "仝", "〻"],
        ["〆", "〼", "ゟ", "ヿ"], // 特殊仮名
        ["♂", "♀", "⚢", "⚣", "⚤", "⚥", "⚦", "⚧", "⚨", "⚩", "⚪︎", "⚲"], // ジェンダー記号
        ["→", "↑", "←", "↓", "↙︎", "↖︎", "↘︎", "↗︎", "↔︎", "↕︎", "↪︎", "↩︎", "⇆"], // 矢印
        ["♯", "♭", "♪", "♮", "♫", "♬", "♩", "𝄞", "𝄞"],  // 音符
        ["√", "∛", "∜"]  // 根号
    ]

    private func loadCCBinary(url: URL) -> [(Int32, Float)] {
        do {
            let binaryData = try Data(contentsOf: url, options: [.uncached])
            return binaryData.toArray(of: (Int32, Float).self)
        } catch {
            debug("Error: 品詞連接コストデータの読み込みに失敗しました。このエラーは深刻ですが、テスト時には無視できる場合があります。 Description: \(error.localizedDescription)")
            return []
        }
    }

    /// 動的ユーザ辞書からrubyに等しい語を返す。
    func getMatchDynamicUserDict(_ ruby: some StringProtocol) -> [DicdataElement] {
        self.dynamicUserDictByRuby[String(ruby)] ?? []
    }

    /// 動的ユーザ辞書からrubyに先頭一致する語を返す。
    func getPrefixMatchDynamicUserDict(_ ruby: some StringProtocol) -> [DicdataElement] {
        self.dynamicUserDict.filter {$0.ruby.hasPrefix(ruby)}
    }

    // 学習を反映する
    // TODO: previousの扱いを改善したい
    func updateLearningData(_ candidate: Candidate, with previous: DicdataElement?) {
        if let previous {
            self.learningManager.update(data: [previous] + candidate.data)
        } else {
            self.learningManager.update(data: candidate.data)
        }
    }
    // 予測変換に基づいて学習を反映する
    // TODO: previousの扱いを改善したい
    func updateLearningData(_ candidate: Candidate, with predictionCandidate: PostCompositionPredictionCandidate) {
        switch predictionCandidate.type {
        case .additional(data: let data):
            self.learningManager.update(data: candidate.data, updatePart: data)
        case .replacement(targetData: let targetData, replacementData: let replacementData):
            self.learningManager.update(data: candidate.data.dropLast(targetData.count), updatePart: replacementData)
        }
    }
    /// class idから連接確率を得る関数
    /// - Parameters:
    ///   - former: 左側の語のid
    ///   - latter: 右側の語のid
    /// - Returns:
    ///   連接確率の対数。
    /// - 要求があった場合ごとにファイルを読み込んで
    /// 速度: ⏱0.115224 : 変換_処理_連接コスト計算_CCValue
    public func getCCValue(_ former: Int, _ latter: Int) -> PValue {
        if !ccParsed[former] {
            let url = requestOptions.dictionaryResourceURL.appendingPathComponent("cb/\(former).binary", isDirectory: false)
            let values = loadCCBinary(url: url)
            ccLines[former] = [Int: PValue].init(uniqueKeysWithValues: values.map {(Int($0.0), PValue($0.1))})
            ccParsed[former] = true
        }
        let defaultValue = ccLines[former][-1, default: -25]
        return ccLines[former][latter, default: defaultValue]
    }

    /// meaning idから意味連接尤度を得る関数
    /// - Parameters:
    ///   - former: 左側の語のid
    ///   - latter: 右側の語のid
    /// - Returns:
    ///   意味連接確率の対数。
    /// - 要求があった場合ごとに確率値をパースして取得する。
    public func getMMValue(_ former: Int, _ latter: Int) -> PValue {
        if former == 500 || latter == 500 {
            return 0
        }
        return self.mmValue[former * self.midCount + latter]
    }

    private static let possibleLOUDS: Set<Character> = [
        "　", "￣", "‐", "―", "〜", "・", "、", "…", "‥", "。", "‘", "’", "“", "”", "〈", "〉", "《", "》", "「", "」", "『", "』", "【", "】", "〔", "〕", "‖", "*", "′", "〃", "※", "´", "¨", "゛", "゜", "←", "→", "↑", "↓", "─", "■", "□", "▲", "△", "▼", "▽", "◆", "◇", "○", "◎", "●", "★", "☆", "々", "ゝ", "ヽ", "ゞ", "ヾ", "ー", "〇", "ァ", "ア", "ィ", "イ", "ゥ", "ウ", "ヴ", "ェ", "エ", "ォ", "オ", "ヵ", "カ", "ガ", "キ", "ギ", "ク", "グ", "ヶ", "ケ", "ゲ", "コ", "ゴ", "サ", "ザ", "シ", "ジ", "〆", "ス", "ズ", "セ", "ゼ", "ソ", "ゾ", "タ", "ダ", "チ", "ヂ", "ッ", "ツ", "ヅ", "テ", "デ", "ト", "ド", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "バ", "パ", "ヒ", "ビ", "ピ", "フ", "ブ", "プ", "ヘ", "ベ", "ペ", "ホ", "ボ", "ポ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ョ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ヮ", "ワ", "ヰ", "ヱ", "ヲ", "ン", "仝", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "！", "？", "(", ")", "#", "%", "&", "^", "_", "'", "\""
    ]

    // 誤り訂正候補の構築の際、ファイルが存在しているか事前にチェックし、存在していなければ以後の計算を打ち切ることで、計算を減らす。
    static func existLOUDS(for character: Character) -> Bool {
        Self.possibleLOUDS.contains(character)
    }

    /*
     文節の切れ目とは

     * 後置機能語→前置機能語
     * 後置機能語→内容語
     * 内容語→前置機能語
     * 内容語→内容語

     となる。逆に文節の切れ目にならないのは

     * 前置機能語→内容語
     * 内容語→後置機能語

     の二通りとなる。

     */
    /// class idから、文節かどうかを判断する関数。
    /// - Parameters:
    ///   - c_former: 左側の語のid
    ///   - c_latter: 右側の語のid
    /// - Returns:
    ///   そこが文節の境界であるかどうか。
    @inlinable static func isClause(_ former: Int, _ latter: Int) -> Bool {
        // EOSが基本多いので、この順の方がヒット率が上がると思われる。
        let latter_wordtype = Self.wordTypes[latter]
        if latter_wordtype == 3 {
            return false
        }
        let former_wordtype = Self.wordTypes[former]
        if former_wordtype == 3 {
            return false
        }
        if latter_wordtype == 0 {
            return former_wordtype != 0
        }
        if latter_wordtype == 1 {
            return former_wordtype != 0
        }
        return false
    }

    /// wordTypesの初期化時に使うのみ。
    private static let BOS_EOS_wordIDs: Set<Int> = [CIDData.BOS.cid, CIDData.EOS.cid]
    /// wordTypesの初期化時に使うのみ。
    private static let PREPOSITION_wordIDs: Set<Int> = [1315, 6, 557, 558, 559, 560]
    /// wordTypesの初期化時に使うのみ。
    private static let INPOSITION_wordIDs: Set<Int> = Set<Int>(
        Array(561..<868).chained(1283..<1297).chained(1306..<1310).chained(11..<53).chained(555..<557).chained(1281..<1283)
    ).union([1314, 3, 2, 4, 5, 1, 9])

    /*
     private static let POSTPOSITION_wordIDs: Set<Int> = Set<Int>((7...8).map{$0}
     + (54..<555).map{$0}
     + (868..<1281).map{$0}
     + (1297..<1306).map{$0}
     + (1310..<1314).map{$0}
     ).union([10])
     */

    /// - Returns:
    ///   - 3 when BOS/EOS
    ///   - 0 when preposition
    ///   - 1 when core
    ///   - 2 when postposition
    /// - データ1つあたり1Bなので、1.3KBくらいのメモリを利用する。
    public static let wordTypes = (0...1319).map(_judgeWordType)

    /// wordTypesの初期化時に使うのみ。
    private static func _judgeWordType(cid: Int) -> UInt8 {
        if Self.BOS_EOS_wordIDs.contains(cid) {
            return 3    // BOS/EOS
        }
        if Self.PREPOSITION_wordIDs.contains(cid) {
            return 0    // 前置
        }
        if Self.INPOSITION_wordIDs.contains(cid) {
            return 1 // 内容
        }
        return 2   // 後置
    }

    @inlinable static func includeMMValueCalculation(_ data: DicdataElement) -> Bool {
        // 非自立動詞
        if 895...1280 ~= data.lcid || 895...1280 ~= data.rcid {
            return true
        }
        // 非自立名詞
        if 1297...1305 ~= data.lcid || 1297...1305 ~= data.rcid {
            return true
        }
        // 内容語かどうか
        return wordTypes[data.lcid] == 1 || wordTypes[data.rcid] == 1
    }

    /// - データ1つあたり2Bなので、2.6KBくらいのメモリを利用する。
    static let penaltyRatio = (0...1319).map(_getTypoPenaltyRatio)

    /// penaltyRatioの初期化時に使うのみ。
    static func _getTypoPenaltyRatio(_ lcid: Int) -> PValue {
        // 助詞147...368, 助動詞369...554
        if 147...554 ~= lcid {
            return 2.5
        }
        return 1
    }

    /// 予測変換で終端になれない品詞id
    static let predictionUsable = (0...1319).map(_getPredictionUsable)
    /// penaltyRatioの初期化時に使うのみ。
    static func _getPredictionUsable(_ rcid: Int) -> Bool {
        // 連用タ接続
        // 次のコマンドにより機械的に生成`cat cid.txt | grep 連用タ | awk '{print $1}' | xargs -I {} echo -n "{}, "`
        if Set([33, 34, 50, 86, 87, 88, 103, 127, 128, 144, 397, 398, 408, 426, 427, 450, 457, 480, 687, 688, 703, 704, 727, 742, 750, 758, 766, 786, 787, 798, 810, 811, 829, 830, 831, 893, 973, 974, 975, 976, 977, 1007, 1008, 1009, 1010, 1063, 1182, 1183, 1184, 1185, 1186, 1187, 1188, 1189, 1190, 1191, 1192, 1193, 1194, 1240, 1241, 1242, 1243, 1268, 1269, 1270, 1271]).contains(rcid) {
            return false
        }
        // 仮定縮約
        // cat cid.txt | grep 仮定縮約 | awk '{print $1}' | xargs -I {} echo -n "{}, "
        if Set([15, 16, 17, 18, 41, 42, 59, 60, 61, 62, 63, 64, 94, 95, 109, 110, 111, 112, 135, 136, 379, 380, 381, 382, 402, 412, 413, 442, 443, 471, 472, 562, 572, 582, 591, 598, 618, 627, 677, 678, 693, 694, 709, 710, 722, 730, 737, 745, 753, 761, 770, 771, 791, 869, 878, 885, 896, 906, 917, 918, 932, 948, 949, 950, 951, 952, 987, 988, 989, 990, 1017, 1018, 1033, 1034, 1035, 1036, 1058, 1078, 1079, 1080, 1081, 1082, 1083, 1084, 1085, 1086, 1087, 1088, 1089, 1090, 1212, 1213, 1214, 1215]).contains(rcid) {
            return false
        }
        // 未然形
        // cat cid.txt | grep 未然形 | awk '{print $1}' | xargs -I {} echo -n "{}, "
        if Set([372, 406, 418, 419, 431, 437, 438, 455, 462, 463, 464, 495, 496, 504, 533, 534, 540, 551, 567, 577, 587, 595, 606, 614, 622, 630, 641, 647, 653, 659, 665, 672, 683, 684, 699, 700, 715, 716, 725, 733, 740, 748, 756, 764, 780, 781, 794, 806, 807, 823, 824, 825, 837, 842, 847, 852, 859, 865, 873, 881, 890, 901, 911, 925, 935, 963, 964, 965, 966, 967, 999, 1000, 1001, 1002, 1023, 1024, 1045, 1046, 1047, 1048, 1061, 1143, 1144, 1145, 1146, 1147, 1148, 1149, 1150, 1151, 1152, 1153, 1154, 1155, 1224, 1225, 1226, 1227, 1260, 1261, 1262, 1263, 1278]).contains(rcid) {
            return false
        }
        // 未然特殊
        // cat cid.txt | grep 未然特殊 | awk '{print $1}' | xargs -I {} echo -n "{}, "
        if Set([420, 421, 631, 782, 783, 795, 891, 936, 1156, 1157, 1158, 1159, 1160, 1161, 1162, 1163, 1164, 1165, 1166, 1167, 1168, 1228, 1229, 1230, 1231]).contains(rcid) {
            return false
        }
        // 未然ウ接続
        // cat cid.txt | grep 未然ウ接続 | awk '{print $1}' | xargs -I {} echo -n "{}, "
        if Set([25, 26, 46, 74, 75, 76, 99, 119, 120, 140, 389, 390, 405, 416, 417, 447, 476, 493, 494, 566, 576, 585, 594, 603, 621, 629, 671, 681, 682, 697, 698, 713, 714, 724, 732, 739, 747, 755, 763, 778, 779, 793, 804, 805, 820, 821, 822, 872, 880, 889, 900, 910, 923, 924, 934, 958, 959, 960, 961, 962, 995, 996, 997, 998, 1021, 1022, 1041, 1042, 1043, 1044, 1060, 1130, 1131, 1132, 1133, 1134, 1135, 1136, 1137, 1138, 1139, 1140, 1141, 1142, 1220, 1221, 1222, 1223, 1256, 1257, 1258, 1259]).contains(rcid) {
            return false
        }
        // 未然ヌ接続
        // cat cid.txt | grep 未然ヌ接続 | awk '{print $1}' | xargs -I {} echo -n "{}, "
        if Set([27, 28, 47, 77, 78, 79, 100, 121, 122, 141, 391, 392, 448, 477, 604]).contains(rcid) {
            return false
        }
        // 体言接続特殊
        // cat cid.txt | grep 体言接続特殊 | awk '{print $1}' | xargs -I {} echo -n "{}, "
        if Set([404, 564, 565, 574, 575, 600, 601, 620, 774, 775, 776, 777, 871, 887, 888, 898, 899, 908, 909, 921, 922, 1104, 1105, 1106, 1107, 1108, 1109, 1110, 1111, 1112, 1113, 1114, 1115, 1116, 1117, 1118, 1119, 1120, 1121, 1122, 1123, 1124, 1125, 1126, 1127, 1128, 1129]).contains(rcid) {
            return false
        }
        // 仮定形
        // cat cid.txt | grep 仮定形 | awk '{print $1}' | xargs -I {} echo -n "{}, "
        if Set([13, 14, 40, 56, 57, 58, 93, 107, 108, 134, 369, 377, 378, 401, 410, 411, 433, 434, 441, 452, 470, 483, 489, 490, 527, 528, 537, 542, 548, 561, 571, 581, 590, 597, 611, 617, 626, 636, 638, 644, 650, 656, 662, 668, 675, 676, 691, 692, 707, 708, 721, 729, 736, 744, 752, 760, 768, 769, 790, 800, 801, 814, 815, 816, 835, 840, 845, 850, 855, 862, 868, 877, 884, 895, 905, 915, 916, 931, 941, 943, 944, 945, 946, 947, 983, 984, 985, 986, 1015, 1016, 1029, 1030, 1031, 1032, 1057, 1065, 1066, 1067, 1068, 1069, 1070, 1071, 1072, 1073, 1074, 1075, 1076, 1077, 1208, 1209, 1210, 1211, 1248, 1249, 1250, 1251, 1276]).contains(rcid) {
            return false
        }
        // 「食べよ」のような命令形も除外する
        // 命令ｙｏ
        // cat cid.txt | grep 命令ｙｏ | awk '{print $1}' | xargs -I {} echo -n "{}, "
        if Set([373, 553, 569, 579, 589, 596, 609, 624, 634, 642, 648, 654, 660, 666, 673, 860, 866, 875, 903, 913, 928, 929, 939]).contains(rcid) {
            return false
        }
        return true
    }

    // 学習を有効にする語彙を決める。
    @inlinable static func needWValueMemory(_ data: DicdataElement) -> Bool {
        // 助詞、助動詞
        if 147...554 ~= data.lcid {
            return false
        }
        // 接頭辞
        if 557...560 ~= data.lcid {
            return false
        }
        // 接尾名詞を除去
        if 1297...1305 ~= data.lcid {
            return false
        }
        // 記号を除去
        if 6...9 ~= data.lcid {
            return false
        }
        if 0 == data.lcid || 1316 == data.lcid {
            return false
        }

        return true
    }

    static let possibleNexts: [String: [String]] = [
        "x": ["ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヮ"],
        "l": ["ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヮ"],
        "xt": ["ッ"],
        "lt": ["ッ"],
        "xts": ["ッ"],
        "lts": ["ッ"],
        "xy": ["ャ", "ュ", "ョ"],
        "ly": ["ャ", "ュ", "ョ"],
        "xw": ["ヮ"],
        "lw": ["ヮ"],
        "v": ["ヴ"],
        "k": ["カ", "キ", "ク", "ケ", "コ"],
        "q": ["クァ", "クィ", "クゥ", "クェ", "クォ"],
        "qy": ["クャ", "クィ", "クュ", "クェ", "クョ"],
        "qw": ["クヮ", "クィ", "クゥ", "クェ", "クォ"],
        "ky": ["キャ", "キィ", "キュ", "キェ", "キョ"],
        "g": ["ガ", "ギ", "グ", "ゲ", "ゴ"],
        "gy": ["ギャ", "ギィ", "ギュ", "ギェ", "ギョ"],
        "s": ["サ", "シ", "ス", "セ", "ソ"],
        "sy": ["シャ", "シィ", "シュ", "シェ", "ショ"],
        "sh": ["シャ", "シィ", "シュ", "シェ", "ショ"],
        "z": ["ザ", "ジ", "ズ", "ゼ", "ゾ"],
        "zy": ["ジャ", "ジィ", "ジュ", "ジェ", "ジョ"],
        "j": ["ジ"],
        "t": ["タ", "チ", "ツ", "テ", "ト"],
        "ty": ["チャ", "チィ", "チュ", "チェ", "チョ"],
        "ts": ["ツ"],
        "th": ["テャ", "ティ", "テュ", "テェ", "テョ"],
        "tw": ["トァ", "トィ", "トゥ", "トェ", "トォ"],
        "cy": ["チャ", "チィ", "チュ", "チェ", "チョ"],
        "ch": ["チ"],
        "d": ["ダ", "ヂ", "ヅ", "デ", "ド"],
        "dy": ["ヂャ", "ヂィ", "ヂュ", "ヂェ", "ヂョ"],
        "dh": ["デャ", "ディ", "デュ", "デェ", "デョ"],
        "dw": ["ドァ", "ドィ", "ドゥ", "ドェ", "ドォ"],
        "n": ["ナ", "ニ", "ヌ", "ネ", "ノ", "ン"],
        "ny": ["ニャ", "ニィ", "ニュ", "ニェ", "ニョ"],
        "h": ["ハ", "ヒ", "フ", "ヘ", "ホ"],
        "hy": ["ヒャ", "ヒィ", "ヒュ", "ヒェ", "ヒョ"],
        "hw": ["ファ", "フィ", "フェ", "フォ"],
        "f": ["フ"],
        "b": ["バ", "ビ", "ブ", "ベ", "ボ"],
        "by": ["ビャ", "ビィ", "ビュ", "ビェ", "ビョ"],
        "p": ["パ", "ピ", "プ", "ペ", "ポ"],
        "py": ["ピャ", "ピィ", "ピュ", "ピェ", "ピョ"],
        "m": ["マ", "ミ", "ム", "メ", "モ"],
        "my": ["ミャ", "ミィ", "ミュ", "ミェ", "ミョ"],
        "y": ["ヤ", "ユ", "イェ", "ヨ"],
        "r": ["ラ", "リ", "ル", "レ", "ロ"],
        "ry": ["リャ", "リィ", "リュ", "リェ", "リョ"],
        "w": ["ワ", "ウィ", "ウェ", "ヲ"],
        "wy": ["ヰ", "ヱ"]
    ]
}
