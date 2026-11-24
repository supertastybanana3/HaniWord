import KanaKanjiConverterModule
import Foundation
import ffi

@MainActor let converter = KanaKanjiConverter()
@MainActor var composingText = ComposingText()

@MainActor var execURL = URL(filePath: "")
/// Initialize で execURL (辞書・分野辞書の置き場所) が決まったら true
@MainActor var execURLReady = false
@MainActor var config: [String : Any] = [
    "enable": false,
    "profile": "",
    "inferenceLimit": 1,
    "model": "zenz.gguf",
    "modelVersion": "v3",
    "spaceAfterLatin": false,
    "greekLetters": true,
    "chemSubscript": true,
]

@MainActor func zenzaiVersionMode(context: String) -> ConvertRequestOptions.ZenzaiVersionDependentMode {
    let profile = personalDataAllowed ? config["profile"] as! String : ""
    if (config["modelVersion"] as? String) == "v2" {
        return .v2(.init(profile: profile, leftSideContext: context))
    }
    // 話題・文体 (zenz-v3 の条件)。試験用に AZK_TOPIC / AZK_STYLE で指定できる
    let env = devEnvironment
    let topic = env["AZK_TOPIC"].flatMap { $0.isEmpty ? nil : $0 }
    let style = env["AZK_STYLE"].flatMap { $0.isEmpty ? nil : $0 }
    return .v3(.init(profile: profile, topic: topic, style: style, leftSideContext: context))
}

/// 絵文字辞書は getOptions のたびに読み直すと 1 回 20〜30ms かかるので、1 度だけ読んで使い回す (azooKey-Windows)
@MainActor var cachedTextReplacer: (url: URL, replacer: TextReplacer)? = nil
@MainActor func sharedTextReplacer() -> TextReplacer {
    let url = execURL.appendingPathComponent("EmojiDictionary").appendingPathComponent("emoji_all_E15.1.txt")
    if let cached = cachedTextReplacer, cached.url == url {
        return cached.replacer
    }
    let replacer = TextReplacer { url }
    cachedTextReplacer = (url, replacer)
    return replacer
}

@MainActor func getOptions(context: String = "", prediction: Bool = true, zenzai: Bool = true, fast: Bool = false, nBest: Int = 10) -> ConvertRequestOptions {
    // 2026-11-18 (perf の 7): zenz を裏で読み終えていれば変換器に渡す。読んでいる最中は AI を使わない。
    // 起動のときに AI を切っていて、あとで設定でオンにしたときも、ここで裏読みを始める (主のスレッドで読むと数秒止まるため)
    installPreloadedZenzIfReady()
    // 2026-11-18 (perf の 7): 本体の辞書を裏で読み終えていれば覚えに足す (dict_loading.swift の background の形)
    installPreloadedDictionaryIfReady()
    // 2026-11-19 (C の見直し・軽微 4): 裏で読めなかったら、以後は AI を使わない (前は zenzai を .on で返し、AI を使う問い合わせのたびに
    // 変換器の getModel が主のスレッドでモデルを読み直していた。読み込みが遅く失敗する PC では、そのたびに止まる)
    let wantsZenz = zenzai && config["enable"] as! Bool && !aiPausedForRequest && !zenzPreloadFailed
    if wantsZenz && zenzPreloadAutoStart && !zenzPreloadStarted {
        startZenzPreload(marks: false)
    }
    return ConvertRequestOptions(
        N_best: nBest,
        requireJapanesePrediction: prediction,
        requireEnglishPrediction: false,
        keyboardLanguage: .ja_JP,
        learningType: personalDataAllowed && (config["learning"] as? Bool ?? true) ? .inputAndOutput : .nothing,
        dictionaryResourceURL: execURL.appendingPathComponent("Dictionary"),
        memoryDirectoryURL: execURL.appendingPathComponent("memory"),
        sharedContainerURL: execURL.appendingPathComponent("shared"),
        textReplacer: sharedTextReplacer(),
        // zenzai
        zenzaiMode: wantsZenz && !zenzWarming() ? .on(
            weight: execURL.appendingPathComponent((config["model"] as? String) ?? "zenz.gguf"),
            inferenceLimit: fast ? 1 : ((config["inferenceLimit"] as? Int) ?? 1),
            requestRichCandidates: true,
            personalizationMode: nil,
            versionDependentMode: zenzaiVersionMode(context: context)
        ) : .off,
        // 2026-11-18 (perf の 7): 本体の辞書を最初の変換の前に全部読むのは preload の形だけ (dict_loading.swift)
        preloadDictionary: dictionaryLoadingMode == .preload,
        metadata: .init(versionString: "HaniWord for Windows")
    )
}

class SimpleComposingText {
    init(text: String, cursor: Int) {
        self.text = UnsafeMutablePointer<CChar>(mutating: text.utf8String)!
        self.cursor = cursor
    }

    var text: UnsafeMutablePointer<CChar>
    var cursor: Int
}

struct SComposingText {
    var text: UnsafeMutablePointer<CChar>
    var cursor: Int
}

func constructCandidateString(candidate: Candidate, hiragana: String) -> String {
    var remainingHiragana = hiragana
    var result = ""
    
    for data in candidate.data {
        if remainingHiragana.count < data.ruby.count {
            result += remainingHiragana
            break
        }
        remainingHiragana.removeFirst(data.ruby.count)
        result += data.word
    }
    
    return result
}

@_silgen_name("LoadConfig")
@MainActor public func load_config() {
    clearPreviewMemo()
    // 2026-11-19: 設定を読み終えたあと、zenz を読めなかったあとで AI がオンなら、1 回だけ裏読みを始め直す (zenz_preload.swift)
    defer { _ = retryFailedZenzPreloadIfNeeded() }
    if let appDataPath = ProcessInfo.processInfo.environment["APPDATA"] {
        let settingsPath = URL(filePath: appDataPath).appendingPathComponent("HaniWord/settings.json")
        
        do {
            let data = try Data(contentsOf: settingsPath)
            if let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
               let zenzaiDict = json["zenzai"] as? [String: Any] {
                
                if let enableValue = zenzaiDict["enable"] as? Bool {
                    config["enable"] = enableValue
                }
                
                if let modelValue = zenzaiDict["model"] as? String {
                    config["model"] = modelValue
                }

                if let versionValue = zenzaiDict["model_version"] as? String {
                    config["modelVersion"] = versionValue
                }

                if let inputDict = json["input"] as? [String: Any], let space = inputDict["space_after_latin"] as? Bool {
                    config["spaceAfterLatin"] = space
                }
                if let inputDict = json["input"] as? [String: Any] {
                    if let greek = inputDict["greek_letters"] as? Bool { config["greekLetters"] = greek }
                    if let chem = inputDict["chem_subscript"] as? Bool { config["chemSubscript"] = chem }
                    // 2026-11-03: 表記ゆれの学習 (hyouki.swift。初期値はオン)
                    if let hyouki = inputDict["hyouki_learning"] as? Bool { config["hyoukiLearning"] = hyouki }
                    // 2026-11-05 (仕事 22 の 2): 英字の表記直し (。 のあとの拡張子の形。slash_dot.swift)
                    if let caseFix = inputDict["english_case_fix"] as? Bool { config["englishCaseFix"] = caseFix }
                    // 2026-11-05 (仕事 22 の 3): / と . の選び直しの学習 (初期値オン。slash_dot.swift)
                    if let slash = inputDict["slash_learning"] as? Bool { config["slashLearning"] = slash }
                    // 2026-11-06 (まとめ役 09:57): 。。。・・・・... を自動で … に (初期値オフ。ellipsis.swift)
                    config["ellipsisAuto"] = inputDict["ellipsis_auto"] as? Bool
                    // 2026-11-07 (仕事 45): カーソルの右の字も見て変換する (初期値オン (11-08)。辞書だけの変換でだけ使う。right_context.swift)
                    config["rightContext"] = inputDict["right_context"] as? Bool
                    // 2026-11-08 (仕事 52): 重いときは自動で AI を休む (初期値オン) と、休んでいる間の並べ方 ("dictionary" 初期値・"preview")
                    config["aiAutoPause"] = inputDict["ai_auto_pause"] as? Bool
                    config["aiPauseOrder"] = inputDict["ai_pause_order"] as? String
                    // 2026-11-06 (仕事 34 の 4): 式のあとの = で答えを候補に (初期値オフ。calc.swift)
                    config["calcCandidates"] = inputDict["calc_candidates"] as? Bool
                    // 2026-11-06 (仕事 36): かなで打った数詞を漢数字の形で先に出すか ("arabic" 初期値・"kanji"。counter_numerals.swift)
                    config["counterNumerals"] = inputDict["counter_numerals"] as? String
                    // 2026-11-05 (仕事 28): / と . の打ち分け (auto・いつも ・ か 。・いつも / か .)、難読語の同音語の置き換え、数学の式の直し。
                    // 書いていない項目は初期値に戻す (設定アプリで消した値が残らないように)
                    config["slashKey"] = inputDict["slash_key"] as? String ?? "auto"
                    config["periodKey"] = inputDict["period_key"] as? String ?? "auto"
                    config["homophoneReplace"] = inputDict["homophone_replace"] as? Bool ?? true
                    config["mathExpressions"] = inputDict["math_expressions"] as? Bool
                    config["domainsOff"] = (inputDict["domains_off"] as? [String]) ?? []
                    config["domainsFocus"] = (inputDict["domains_focus"] as? [String]) ?? []
                    // 2026-11-06 (仕事 39): 方言の言い方の辞書 (domain\dialects.txt に並ぶ名前のうち選んだもの。初期値は使わない)
                    config["dialects"] = (inputDict["dialects"] as? [String]) ?? []
                    // 2026-11-07 (仕事 43): ネットスラングの辞書 (domain\slang.txt の internet。初期値はオフ)
                    config["internetSlang"] = inputDict["internet_slang"] as? Bool ?? false
                    // 2026-11-06 (仕事 32): ギリシャ文字の大文字・小文字の順 (字の小文字 → "lower" か "upper")。書いていない字は既定 (greekDefaultUpper)
                    config["greekCase"] = greekCaseSetting(inputDict["greek_case"])
                }

                if let learningValue = zenzaiDict["learning"] as? Bool {
                    config["learning"] = learningValue
                }

                if let limitValue = zenzaiDict["inference_limit"] as? Int {
                    config["inferenceLimit"] = limitValue
                }

                if let profileValue = zenzaiDict["profile"] as? String {
                    config["profile"] = profileValue
                }
            }
        } catch {
            print("Failed to read settings: \(error)")
        }
    }
    // 2026-10-20: 分野辞書のオン・オフも設定の変更 (update_config) ですぐ効くようにここで読み直す (Initialize の前は置き場所が決まっていないので何もしない)
    if execURLReady {
        setup_domain_dictionaries()
    }
    // 2026-11-04: 単語登録 (user_dictionary.swift)。設定アプリは登録を保存したあと設定の読み直しを知らせるので、ここで読み直す (変わっていなければ読まない)
    if dynamicDictionaryReady {
        reloadUserDictionary()
    }
}

/// 2026-10-20: 配布する分野辞書の名前 (domain\domains.txt、1 行 1 つ。# で始まる行は説明)。名前は英小文字・数字・_ だけ (ファイル名に使う)
func publishedDomainNames(_ text: String?) -> [String] {
    guard let text else { return [] }
    var names: [String] = []
    for line in text.split(whereSeparator: \.isNewline) {
        let name = line.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty, !name.hasPrefix("#"), name.count <= 32,
              name.unicodeScalars.allSatisfy({ ("a"..."z").contains($0) || ("0"..."9").contains($0) || $0 == "_" }),
              !names.contains(name) else { continue }
        names.append(name)
    }
    return names
}

/// 使う分野辞書の名前 (domain_<名前>)。配布する分野辞書 (domains.txt に並ぶもの) は既定でオン。設定 input.domains_off に入れた分野だけ使わない。
/// domains.txt に無い domain_* のファイルは読まない。試験用のビルドでは環境変数 AZK_DOMAIN_<名前の大文字>=1/0 (例 AZK_DOMAIN_LAW=0) で切り替えられる
func enabledDomainIdentifiers(config: [String: Any], environment: [String: String], published: [String] = []) -> [String] {
    let off = Set((config["domainsOff"] as? [String]) ?? [])
    return published.compactMap { name -> String? in
        let enabled = environment["AZK_DOMAIN_" + name.uppercased()].map { $0 == "1" } ?? !off.contains(name)
        return enabled ? "domain_" + name : nil
    }
}

/// 2026-11-06 (仕事 39): 方言の言い方の辞書の名前 (domain_dialect_<名前>)。domain\dialects.txt に並ぶ名前のうち、設定 input.dialects で選んだものだけ。
/// 分野辞書と違って初期値は使わない (関西の語尾 へん・はる・ねん・ええ などを、標準語で打つ人には足さない)。同音語の優遇も使わない。
/// 試験用のビルドでは環境変数 AZK_DIALECT_<名前の大文字>=1/0 で切り替えられる
func dialectIdentifiers(config: [String: Any], environment: [String: String], published: [String]) -> [String] {
    let chosen = Set((config["dialects"] as? [String]) ?? [])
    return published.filter { !$0.hasPrefix("core_") }.compactMap { name -> String? in
        let on = environment["AZK_DIALECT_" + name.uppercased()].map { $0 == "1" } ?? chosen.contains(name)
        return on ? "domain_dialect_" + name : nil
    }
}

/// 2026-11-07 (仕事 43): くだけた言葉の辞書の名前 (domain_slang_<名前>)。domain\slang.txt に並ぶ名前のうち、設定でオンにしたものだけ。
/// 今は internet (ネットスラング、設定 input.internet_slang) だけ。初期値はオフ (オワコン・ぴえん のような語を、使わない人の変換に混ぜない)。同音語の優遇は使わない。
/// 試験用のビルドでは環境変数 AZK_SLANG_<名前の大文字>=1/0 で切り替えられる
let slangSettings: [String: String] = ["internet": "internetSlang"]
func slangIdentifiers(config: [String: Any], environment: [String: String], published: [String]) -> [String] {
    return published.filter { !$0.hasPrefix("core_") }.compactMap { name -> String? in
        guard let key = slangSettings[name] else { return nil }
        let on = environment["AZK_SLANG_" + name.uppercased()].map { $0 == "1" } ?? ((config[key] as? Bool) ?? false)
        return on ? "domain_slang_" + name : nil
    }
}

/// 2026-10-22: オフにできない追加辞書 (辞書の抜け M3.5: 活用形・成句・語) の名前 (domain_<名前>)。domain\always.txt に並ぶ名前 (core_ で始まる)。
/// 分野辞書と同じ形で置くが、設定 input.domains_off は効かない。試験用のビルドでは環境変数 AZK_ALWAYS_<名前の大文字>=0 で外せる (どれが効いたか測るため)
func alwaysOnIdentifiers(environment: [String: String], names: [String]) -> [String] {
    names.filter { $0.hasPrefix("core_") && environment["AZK_ALWAYS_" + $0.uppercased()] != "0" }.map { "domain_" + $0 }
}

/// 2026-10-31: 読み間違いの本来の形 (ふいんき → 雰囲気) を入れた追加辞書 (always.txt の core_misreadings)。変換器がその語 (と、同じ語の学習) に
/// isReadingCorrection の印を付け、zenz の評価でモデルの点によらず通し、前置きの制約も素通りさせる (融合採点も選び直さない)。辞書に入れただけだと、辞書だけの変換では一番上でも、AI の変換では 178 組のうち 99 組、
/// 候補の一覧の 1 番では 59 組だった (モデルが読みどおりの字を好む)。試験用のビルドでは AZK_READING_CORRECTIONS=0 で切れる (直しの前後を同じビルドで測るため)
func readingCorrectionIdentifiers(environment: [String: String], always: [String]) -> Set<String> {
    environment["AZK_READING_CORRECTIONS"] == "0" ? [] : Set(always.filter { $0 == "domain_core_misreadings" })
}

/// 2026-10-20: 分野の文脈での同音語の優遇。分野辞書の隣の
///   domain_<名前>_boost.tsv  : 優遇する語 (表記<TAB>読み)。分野辞書の語と、変換器の辞書にある分野の語 (債権・刑法・施行)
///   domain_<名前>_signals.txt: 文脈に出たらその分野の文とみなす語 (1 行 1 語)。優遇する語もこれに入る
/// 文脈 (直前の文と、いま変換している文) に分野の目印の語が fusion.domainSignals 種類 (既定 1) 以上出ていれば、その分野は有効。
/// 2026-10-30: よく打つ分野 (設定 input.domains_focus) は、目印の語が無くてもいつも有効 (選んだ分野は強める、選ばない分野は今のまま)
/// 2026-10-21 の測定 (法律の中間版): 法律の文 935 文 719→725、一般の文 3,675 文 2,458→2,457、日本史・世界史・生物の文は ±1
/// 有効なときだけ、融合採点で同じ読みの語を分野の語に差し替えた候補を足し、分野の語 1 つにつき fusion.domainBonus を足す。
/// 一般の文 (文脈に分野の語が無い) では何も変わらない
struct DomainBoost {
    var byReading: [String: [String]] = [:]   // 読み (カタカナ) → 表記
    var pairs: Set<String> = []                // "読み\t表記"
    var signals: Set<String> = []
    /// 目印の語の頭の字 → その字で始まる目印の語の長さ (長い順)。文脈の 1 字ごとに、ありうる長さだけを試す
    /// (2026-10-22: 前は 1 字ごとに最長の語の長さ (医学で 64 字) から 2 字まで全部の長さの文字列を作っていた。D2-3)
    var lengthsByFirst: [Character: [Int]] = [:]
    /// 2026-10-30: よく打つ分野 (設定 input.domains_focus。インストーラーの専門分野の画面で選ぶ)。目印の語が無くても有効にする
    var focused = false

    /// この分野の優遇を効かせるか: よく打つ分野か、文脈に目印の語が minimum 種類以上出ているか
    func isActive(_ text: String, minimum: Int) -> Bool {
        focused || signalCount(text, minimum: minimum) >= minimum
    }

    init?(boost: String?, signals: String?) {
        for line in (boost ?? "").split(whereSeparator: \.isNewline) where !line.hasPrefix("#") {
            let cols = line.split(separator: "\t", omittingEmptySubsequences: false)
            guard cols.count >= 2, !cols[0].isEmpty, !cols[1].isEmpty, cols[0].count <= 64, cols[1].count <= 64 else { continue }
            let word = String(cols[0]), ruby = hiraganaToKatakana(String(cols[1]))
            if pairs.insert(ruby + "\t" + word).inserted {
                byReading[ruby, default: []].append(word)
            }
            self.signals.insert(word)
        }
        for line in (signals ?? "").split(whereSeparator: \.isNewline) where !line.hasPrefix("#") {
            let word = line.trimmingCharacters(in: .whitespaces)
            if !word.isEmpty && word.count <= 32 { self.signals.insert(word) }
        }
        self.signals = self.signals.filter { $0.count >= 2 }
        var lengths: [Character: Set<Int>] = [:]
        for word in self.signals {
            lengths[word.first!, default: []].insert(word.count)
        }
        lengthsByFirst = lengths.mapValues { $0.sorted(by: >) }
        if pairs.isEmpty && self.signals.isEmpty { return nil }
    }

    /// 文脈に出た分野の語の種類の数 (minimum に届いたら数えるのをやめる)
    func signalCount(_ text: String, minimum: Int) -> Int {
        let chars = Array(text)
        var found: Set<String> = []
        for i in chars.indices {
            for len in lengthsByFirst[chars[i]] ?? [] where i + len <= chars.count {
                let w = String(chars[i..<(i + len)])
                if signals.contains(w) {
                    found.insert(w)
                    if found.count >= minimum { return found.count }
                    break
                }
            }
        }
        return found.count
    }
}

@MainActor var domainBoosts: [DomainBoost] = []

/// 2026-10-31: 語の並びの、差し替えられる所 (1 語か隣り合う 2 語。助詞・助動詞を含まず、もう分野の語でなく、同じ読みの分野の語があり、
/// 送り仮名だけの違いでないもの) を左から全部差し替えた並び。差し替えが 2 か所未満なら nil (1 か所の候補は別に足している)
func combinedDomainVariant(_ elems: [DicdataElement], boosts: [DomainBoost]) -> [DicdataElement]? {
    var out: [DicdataElement] = []
    var replaced = 0
    var i = 0
    while i < elems.count {
        var taken = false
        for len in [2, 1] where !taken && i + len <= elems.count {
            let span = elems[i..<(i + len)]
            if span.contains(where: { (147...549).contains($0.lcid) || (147...549).contains($0.rcid) }) { continue }
            let ruby = span.map(\.ruby).joined()
            let current = span.map(\.word).joined()
            if boosts.contains(where: { $0.pairs.contains(ruby + "\t" + current) || $0.signals.contains(current) }) { continue }
            guard let word = boosts.lazy.compactMap({ $0.byReading[ruby]?.first(where: { $0 != current && notationSkeleton($0) != notationSkeleton(current) }) }).first else { continue }
            out.append(DicdataElement(word: word, ruby: ruby, lcid: span.first!.lcid, rcid: span.last!.rcid, mid: span.last!.mid,
                                      value: span.reduce(0) { $0 + $1.value() }))
            replaced += 1
            i += len
            taken = true
        }
        if !taken {
            out.append(elems[i])
            i += 1
        }
    }
    return replaced >= 2 ? out : nil
}

/// 2026-10-31: 分野の語に差し替えた語の数 (候補の語のうち分野の優遇の語 (読みと表記の組) の数から、もとの並びにあった数を引いたもの)
func replacedDomainWords(_ data: [DicdataElement], base: [DicdataElement], boosts: [DomainBoost]) -> Int {
    func count(_ d: [DicdataElement]) -> Int { d.filter { e in boosts.contains { $0.pairs.contains(e.ruby + "\t" + e.word) } }.count }
    return max(0, count(data) - count(base))
}

/// 2026-10-30: よく打つ分野 (設定 input.domains_focus) の名前 (domain_<名前>)。名前の決まりは domains.txt と同じ (ファイル名に使える英小文字・数字・_ だけ)
func focusedDomainIdentifiers(config: [String: Any]) -> Set<String> {
    Set(publishedDomainNames(((config["domainsFocus"] as? [String]) ?? []).joined(separator: "\n")).map { "domain_" + $0 })
}

/// 送り仮名 (ひらがな) を除き、算用数字を漢数字にそろえた形。書き方だけの違いを見分ける (取り消し/取消、第1号/第一号)
func notationSkeleton(_ text: String) -> String {
    let digits: [Character: Character] = ["0": "〇", "1": "一", "2": "二", "3": "三", "4": "四", "5": "五", "6": "六", "7": "七", "8": "八", "9": "九"]
    return String(text.compactMap { c -> Character? in
        if let d = digits[c] { return d }
        return c.unicodeScalars.allSatisfy({ (0x3041...0x3096).contains($0.value) }) ? nil : c
    })
}

/// 分野辞書と語ごとの値の上書き (value_overrides.tsv) を読む。
/// 試験用に環境変数 AZK_DOMAIN_<名前>=1/0・AZK_VALUE_OVERRIDES=0 で切り替えられる。辞書のファイルは最初の検索で読み込まれる
@MainActor func setup_domain_dictionaries() {
    let env = devEnvironment
    let domainDir = execURL.appendingPathComponent("domain")
    // 2026-10-22 (D の調べ D2-2): 表は上限つきで読む (BoundedTable)。名前の一覧は 64KB、優遇・目印は 8MB まで (配っているのは 0.5MB ほど)
    let published = publishedDomainNames(BoundedTable.text(domainDir.appendingPathComponent("domains.txt"), maxBytes: 64 << 10))
    let identifiers = enabledDomainIdentifiers(config: config, environment: env, published: published.filter { !$0.hasPrefix("core_") })
    let always = alwaysOnIdentifiers(environment: env, names: publishedDomainNames(BoundedTable.text(domainDir.appendingPathComponent("always.txt"), maxBytes: 64 << 10)))
    let focused = focusedDomainIdentifiers(config: config)
    let dialects = dialectIdentifiers(config: config, environment: env,
                                      published: publishedDomainNames(BoundedTable.text(domainDir.appendingPathComponent("dialects.txt"), maxBytes: 64 << 10)))
    let slang = slangIdentifiers(config: config, environment: env,
                                 published: publishedDomainNames(BoundedTable.text(domainDir.appendingPathComponent("slang.txt"), maxBytes: 64 << 10)))
    // 2026-11-05 (まとめ役 14:51): 数学の式の直しがオンのときだけ、数学の文の見分けに数学の目印の語を読む (slash_dot.swift)。
    // 仕事 28 の 6: 設定 input.math_expressions。書いていなければ (前の版の設定)、よく打つ分野に数学か物理があればオン
    mathSentenceSignals = mathExpressionsOn(config: config, focused: focused)
        ? DomainBoost(boost: nil, signals: BoundedTable.text(domainDir.appendingPathComponent("domain_math_signals.txt"), maxBytes: 8 << 20))
        : nil
    domainBoosts = identifiers.compactMap { id in
        guard var boost = DomainBoost(boost: BoundedTable.text(domainDir.appendingPathComponent("\(id)_boost.tsv"), maxBytes: 8 << 20),
                                      signals: BoundedTable.text(domainDir.appendingPathComponent("\(id)_signals.txt"), maxBytes: 8 << 20)) else { return nil }
        boost.focused = focused.contains(id)
        return boost
    }
    DicdataStore.domainDirectoryURL = domainDir
    // ファイルが無ければ辞書の読み込みが空振りするだけ (FileManager.fileExists は Windows の URL.path "/C:/..." で偽になるので使わない)
    DicdataStore.domainIdentifiers = always + identifiers + dialects + slang
    // 読み間違いの本来の形 (core_misreadings) は AI の変換でも落とさない
    DicdataStore.readingCorrectionDomains = readingCorrectionIdentifiers(environment: env, always: always)
    // 2026-11-08 (仕事 42 の直し): 常用外の字の表記の表 (core_rare_kanji) の語に印を付け、候補の一覧で下の候補にする (rare_spelling.swift)。
    // AZK_RARE_DEMOTE=0 で印も付けない (開発用のビルドだけ。直す前と同じ動きで測るため)
    DicdataStore.rareSpellingDomains = rareSpellingIdentifiers(environment: env, always: always)
    // 設定の読み直し (分野のオンオフ・ファイルの置き換え) のたびに、変換器が持っている分野辞書の中身を捨てて読み直させる (D の調べ R2-3・D2-6)
    DicdataStore.domainGeneration += 1
    // 1 行 = 読み(カタカナかひらがな) TAB 表記 TAB 値。# で始まる行は説明。値は有限で -60〜0 のときだけ (D2-2)
    var overrides: [String: PValue] = [:]
    func load(_ url: URL) {
        for cols in BoundedTable.rows(url, columns: 3 ... 8, maxFieldLength: 64) {
            guard !cols[0].isEmpty, !cols[1].isEmpty, let v = BoundedTable.value(cols[2]) else { continue }
            overrides[hiraganaToKatakana(cols[0]) + "\t" + cols[1]] = PValue(v)
        }
    }
    // 2026-10-31: 配る値の調整 (value_adjustments.tsv。ふち → 縁・はし → 端 の並び) を先に読み、利用者が自分で書く value_overrides.tsv をあとから読む (同じ語なら利用者の値が勝つ)。
    // value_overrides.tsv は個人のファイルなので配らない (package.py が止める)。配る表を同じ名前にすると、入れるたびに利用者の表を上書きしてしまう
    if env["AZK_VALUE_ADJUSTMENTS"] != "0" { load(execURL.appendingPathComponent("value_adjustments.tsv")) }
    if env["AZK_VALUE_OVERRIDES"] != "0" { load(execURL.appendingPathComponent("value_overrides.tsv")) }
    for identifier in identifiers { load(domainDir.appendingPathComponent("\(identifier)_overrides.tsv")) }
    DicdataStore.valueOverrides = overrides
}

@_silgen_name("Initialize")
@MainActor public func initialize(
    path: UnsafePointer<CChar>,
    use_zenzai: Bool
) {
    let path = String(cString: path)
    execURL = URL(filePath: path)
    execURLReady = true
    // 2026-11-23 (セキュリティの点検 12): 壊れた学習のファイルを memory_corrupt_<日時> に退けたら、標準エラーに 1 行 (launcher の記録に残る)。
    // 書くのは壊れ方の種類・ファイルの数・退けた先の名前だけで、学習の中身は書かない
    LearningCorruption.report = { reason, count, folder in
        FileHandle.standardError.write(Data("haniword-server: learning files were corrupt (\(reason)); moved \(count) files to \(folder.isEmpty ? "(not moved)" : folder)\n".utf8))
    }
    // 2026-11-18 (perf の 7、開発用のビルドだけ): 起動の段ごとの時間を AZK_DEBUG に 1 行で書く (冷えた起動の内訳を測るため)
    var clock = Date()
    var times: [String] = []
    func note(_ name: String) {
        guard devEnvironment["AZK_DEBUG"] != nil else { return }
        times.append("\(name) \(Int(-clock.timeIntervalSinceNow * 1000))ms")
        clock = Date()
    }
    startupMark("started")
    // 2026-11-18 (perf の 7): 本体の辞書の読み捨て (lazy) か裏読み (background) を始める (dict_loading.swift)
    startDictionaryLoading(dictionaryURL: execURL.appendingPathComponent("Dictionary"))
    for name in ["memory", "shared"] {
        try? FileManager.default.createDirectory(at: execURL.appendingPathComponent(name), withIntermediateDirectories: true)
    }

    load_config()  // 分野辞書 (setup_domain_dictionaries) もここで読む
    note("config")
    load_extra_dictionary()
    // 2026-11-04: 追加辞書の語と、単語登録 (%APPDATA%\HaniWord\user_dictionary.tsv) の語を合わせて入れる (user_dictionary.swift)
    reloadUserDictionary(force: true)
    dynamicDictionaryReady = true
    japaneseVocabulary = .load(from: execURL.appendingPathComponent("japanese_vocabulary.json"))
    note("extra_user_vocabulary")
    startLoadingHomophoneTables()  // 難読語の同音語の表 (16 万行) は、別のスレッドで読みはじめる (homophones.swift)
    startLoadingHyoukiTable()  // 表記ゆれの組の表 (hyouki.swift) も
    startLoadingCollocTable()  // 2026-11-06: 組み合わせの回数の表 (colloc.swift) も
    // 2026-11-20 (学習の効きすぎ): 学習の場所から危ない要素を引かない (learn_guard.swift。表は裏で読む)
    installLearnGuard()
    note("start_tables")

    composingText.insertAtCursorPosition("a", inputStyle: .roman2kana)
    if zenzPreloadEnabled {
        // 2026-11-18 (perf の 7): 温めの変換は辞書だけにして、すぐ返す (受け口を先に開く)。zenz の読み込みと最初の推論は別のスレッドで
        // (zenz_preload.swift。起動の段の印 loading・ready もそのスレッドが出す)
        // 辞書の用意 (charID・連接・学習・本体の辞書の先読み) と、最初の変換を分けて測る
        converter.sendToDicdataStore(.setRequestOptions(getOptions(zenzai: false)))
        note("dictionary_setup")
        // 2026-11-19 (仕事 3「あ」の 1 秒の probe): 試験用のビルドだけ AZK_WARMUP_STEPS=1 で、温めの変換を段に分けて印を出す
        // (表の置き換え・予測なしの変換・同じ読みの予測ありの変換・別の頭の字の読み)。配る版は devEnvironment が空なので、今までどおり 1 回
        if devEnvironment["AZK_WARMUP_STEPS"] == "1" {
            _ = sharedTextReplacer()
            note("warmup_text_replacer")
            _ = converter.requestCandidates(composingText, options: getOptions(prediction: false, zenzai: false, nBest: 1))
            note("warmup_no_prediction")
            _ = converter.requestCandidates(composingText, options: getOptions(zenzai: false))
            note("warmup_prediction")
            var other = ComposingText()
            other.insertAtCursorPosition("ka", inputStyle: .roman2kana)
            _ = converter.requestCandidates(other, options: getOptions(zenzai: false))
            note("warmup_other_head")
        } else {
            // 2026-11-19 (仕事 3): 温めは予測なしでする。打っている間の変換 (ConvertSegments) は予測なしなので、予測ありで温めると、冷えた起動で
            // 「あ」で始まる語を最大 700 件引くぶん (0.6〜1.0 秒) Initialize が遅れていた。予測を使う問い合わせの最初の 1 回は、そのぶん遅くなりうる。
            // 試験用のビルドだけ AZK_WARMUP_PREDICTION=1 で前の形
            let warmPrediction = devEnvironment["AZK_WARMUP_PREDICTION"] == "1"
            converter.requestCandidates(composingText, options: getOptions(prediction: warmPrediction, zenzai: false))
            note("warmup_dictionary")
        }
        startZenzPreloadAtStartup()
    } else {
        // 2026-11-10: この温めの変換で zenz の文脈 (GPU の準備) を作る。前後に印を出し、launcher はこの間に落ちたときだけ GPU (AVX2) の版のせいとみなす
        startupMark("loading")
        converter.requestCandidates(composingText, options: getOptions())
        note("warmup_ai")
        startupMark("ready")
    }
    composingText = ComposingText()
    // 学習から外す語 (learning_ng.txt、1 行 1 語。# で始まる行は説明)。次の保存 (すぐ下の closeKeyboard) で消える
    if let text = try? String(contentsOf: execURL.appendingPathComponent("learning_ng.txt"), encoding: .utf8) {
        LearningDiscard.words = text.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty && !$0.hasPrefix("#") }
    }
    // 変換器は設定を受け取る前に学習データ (memory) を探しに行って見つけられず、以後読み直さない。
    // 設定を渡したあとで読み直させる (closeKeyboard = 保存して読み直し)
    converter.sendToDicdataStore(.closeKeyboard)
    note("learning")
    if !times.isEmpty { debugWrite("startup: \(times.joined(separator: ", "))\n".data(using: .utf8)!) }
}

/// 2026-11-10: 起動の段の印を標準エラーに 1 行出す (launcher が読む。バッファを通さないので、直後に落ちても消えない)。
/// 開発用のビルドだけ: AZK_TEST_CRASH=<段> でその印のあとに落とす (launcher の版の替え方を試すため。配る版は AZK_* を読まない)
func startupMark(_ stage: String) {
    FileHandle.standardError.write(Data("haniword-server: \(stage)\n".utf8))
    if devEnvironment["AZK_TEST_CRASH"] == stage {
        fatalError("AZK_TEST_CRASH \(stage)")
    }
}

/// 2026-10-13: 本体の辞書に足す語 (extra_dictionary.tsv: 語<TAB>カタカナの読み<TAB>品詞番号<TAB>意味番号<TAB>重み、# で始まる行は説明)。
/// 本体の辞書に無い品詞の語 (負な・正なら の 負・正 = 形容動詞 1287) や固有名詞 (GeoGuessr)。少数の語だけ (動的な辞書は引き方が遅い)
@MainActor func load_extra_dictionary() {
    var elements: [DicdataElement] = []
    // 2026-10-22 (D の 2 回目の調べ D2-2): 上限つきで読む (BoundedTable。1 MB・2,000 行・欄 64 字まで、制御文字の行は捨てる)
    for parts in BoundedTable.rows(execURL.appendingPathComponent("extra_dictionary.tsv"), maxBytes: 1 << 20, maxLines: 2000, columns: 5...5, maxFieldLength: 64) {
        // 2026-10-19 (security review #6): 意味番号は辞書の表の範囲 (0..<502)、重みは有限の値だけ (範囲外の mid は格子で配列の外を引いて落ちる)
        guard !parts[0].isEmpty, !parts[1].isEmpty, let cid = Int(parts[2]), let mid = Int(parts[3]),
              let value = BoundedTable.value(parts[4]), (0..<1319).contains(cid), (0..<502).contains(mid) else { continue }
        elements.append(DicdataElement(word: parts[0], ruby: parts[1], cid: cid, mid: mid, value: PValue(value)))
    }
    // 2026-11-04: 入れるのは単語登録の語と合わせて (applyDynamicDictionary)
    extraDictionaryElements = elements
}

@_silgen_name("AppendText")
@MainActor public func append_text(
    input: UnsafePointer<CChar>,
    cursorPtr: UnsafeMutablePointer<Int32>
) -> UnsafeMutablePointer<CChar> {
    let inputString = String(cString: input)
    composingText.insertAtCursorPosition(inputString, inputStyle: .roman2kana)

    cursorPtr.pointee = Int32(composingText.convertTargetCursorPosition)
    return _strdup(composingText.convertTarget)!
}

/// いま打たれている文字の列 (ローマ字・英字・かな)。英語の自動判別に使う
@_silgen_name("GetRawInput")
@MainActor public func get_raw_input() -> UnsafeMutablePointer<CChar> {
    return _strdup(composingText.input.map { String($0.character) }.joined())!
}

/// ローマ字変換をせずにそのまま入れる (英語と判定した語)
@_silgen_name("AppendTextDirect")
@MainActor public func append_text_direct(
    input: UnsafePointer<CChar>,
    cursorPtr: UnsafeMutablePointer<Int32>
) -> UnsafeMutablePointer<CChar> {
    let inputString = String(cString: input)
    composingText.insertAtCursorPosition(inputString, inputStyle: .direct)
    cursorPtr.pointee = Int32(composingText.convertTargetCursorPosition)
    return _strdup(composingText.convertTarget)!
}

@_silgen_name("RemoveText")
@MainActor public func remove_text(
    cursorPtr: UnsafeMutablePointer<Int32>
) -> UnsafeMutablePointer<CChar> {
    composingText.deleteBackwardFromCursorPosition(count: 1)

    cursorPtr.pointee = Int32(composingText.convertTargetCursorPosition)
    return _strdup(composingText.convertTarget)!
}

@_silgen_name("MoveCursor")
@MainActor public func move_cursor(
    offset: Int32,
    cursorPtr: UnsafeMutablePointer<Int32>
) -> UnsafeMutablePointer<CChar> {
    _ = composingText.moveCursorFromCursorPosition(count: Int(offset))
    cursorPtr.pointee = Int32(composingText.convertTargetCursorPosition)
    return _strdup(composingText.convertTarget)!
}

@_silgen_name("ClearText")
@MainActor public func clear_text() {
    composingText = ComposingText()
    clearOfferedHomophones()
}

func to_list_pointer(_ list: [FFICandidate]) -> UnsafeMutablePointer<UnsafeMutablePointer<FFICandidate>?> {
    let pointer = UnsafeMutablePointer<UnsafeMutablePointer<FFICandidate>?>.allocate(capacity: list.count)
    for (i, item) in list.enumerated() {
        let candidate = UnsafeMutablePointer<FFICandidate>.allocate(capacity: 1)
        candidate.initialize(to: item)
        pointer.advanced(by: i).initialize(to: candidate)
    }
    return pointer
}

@_silgen_name("GetComposedText")
@MainActor public func get_composed_text(lengthPtr: UnsafeMutablePointer<Int32>) -> UnsafeMutablePointer<UnsafeMutablePointer<FFICandidate>?> {
    refreshUserDictionaryIfChanged()
    let hiragana = composingText.convertTarget
    let contextString = (config["context"] as? String) ?? ""
    let options = getOptions(context: contextString)
    let converted = converter.requestCandidates(composingText, options: options)
    var result: [FFICandidate] = []

    for i in 0..<converted.mainResults.count {
        let candidate = converted.mainResults[i]

        let text = strdup(constructCandidateString(candidate: candidate, hiragana: hiragana))
        let hiragana = strdup(hiragana)
        let correspondingCount = candidate.correspondingCount

        var afterComposingText = composingText
        afterComposingText.prefixComplete(correspondingCount: correspondingCount)
        let subtext = strdup(afterComposingText.convertTarget)

        result.append(FFICandidate(text: text, subtext: subtext, hiragana: hiragana, correspondingCount: Int32(correspondingCount)))        
    }

    lengthPtr.pointee = Int32(result.count)

    return to_list_pointer(result)
}

@_silgen_name("ShrinkText")
@MainActor public func shrink_text(
    offset: Int32
) -> UnsafeMutablePointer<CChar>  {
    var afterComposingText = composingText
    afterComposingText.prefixComplete(correspondingCount: max(0, Int(offset)))
    _ = afterComposingText.moveCursorFromCursorPosition(count: 0)
    composingText = afterComposingText

    return _strdup(composingText.convertTarget)!
}

@_silgen_name("SetContext")
@MainActor public func set_context(
    context: UnsafePointer<CChar>
) {
    let contextString = String(cString: context)
    config["context"] = contextString
}

// MARK: - Google日本語入力風の変換 (ライブ変換を使わず、Spaceで文節ごとに変換する)

/// 未確定の末尾の n を「ん」にし、ローマ字の残りを除いた変換対象を作る
func normalizeForConversion(_ hiragana: String) -> String {
    var text = hiragana
    let chars = Array(text)
    let previousIsLatin = chars.count >= 2 && chars[chars.count - 2].isASCII && chars[chars.count - 2].isLetter
    if (text.hasSuffix("n") || text.hasSuffix("ｎ")) && !previousIsLatin {
        text.removeLast()
        text.append("ん")
    }
    return text
}

func hiraganaToKatakana(_ text: String) -> String {
    String(String.UnicodeScalarView(text.unicodeScalars.map { scalar in
        (0x3041...0x3096).contains(scalar.value) ? Unicode.Scalar(scalar.value + 0x60)! : scalar
    }))
}

/// 2026-10-15: カタカナ語のあとの助詞を、辞書にあるカタカナ表記の助詞 (デ・ヲ・ガ) で出したもの (フーシデ・ガバナンスデ) を、ひらがなに戻す。
/// 読みはひらがなで打っているので、助詞だけカタカナにしたいことはまず無い。文の頭の助詞はそのまま。戻すものが無ければ nil
func hiraganizingKatakanaParticles(_ candidate: Candidate) -> Candidate? {
    var data = candidate.data
    var changed = false
    func isKatakana(_ c: Character?) -> Bool {
        guard let c else { return false }
        return c.unicodeScalars.allSatisfy { (0x30A1...0x30F6).contains($0.value) || $0.value == 0x30FC }
    }
    for i in data.indices where i > 0 {
        let element = data[i]
        guard !element.word.isEmpty, element.word == element.ruby, element.word.allSatisfy({ isKatakana($0) }) else { continue }
        // 助詞として辞書にあるカタカナ (デ/158)。名詞として入っている デ・ヲ (デ/1285) も、カタカナ語の直後で後ろがカタカナでなければ助詞とみなす
        // (ニ・ガ・モ は人名の終わり (カルティ|ニ) にもなるので見ない)
        let looseParticle = ["デ", "ヲ"].contains(element.word) && isKatakana(data[i - 1].word.last)
            && !(i + 1 < data.count && isKatakana(data[i + 1].word.first))
        guard CIDData.isJoshi(cid: element.lcid) || looseParticle else { continue }
        data[i] = DicdataElement(word: element.ruby.toHiragana(), ruby: element.ruby, lcid: element.lcid, rcid: element.rcid,
            mid: element.mid, value: element.value())
        changed = true
    }
    guard changed else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
        lastMid: candidate.lastMid, data: data)
}

/// 2026-10-15: かなで打ったときに出てくる辞書の英字の語 (LINE・NVIDIA・Oh) の扱い (latin_kana_policy.tsv: 表記<TAB>格付け[<TAB>読み[<TAB>変換]])。
/// 格付けは候補一覧での英字の形の位置: high = 2 番目・mid = 5 番目・low = ひらがな・カタカナの手前・drop = 出さない。
/// 変換は kana = 読みのカタカナにする (らいん → ライン。high・mid・low の既定)・next = 次の候補を使う (あい の AI → 愛。drop の既定)・
/// keep = 英字のまま (わうわう → WOWOW。英字でしか書かない名前。2026-10-15)。
/// 読み (カタカナ) を書いた行は、辞書のその語 (表記と読みの組) に効く: 変換と候補の位置。歌手の AI/アイ は あい のときだけで、えーあい の AI はそのまま。
/// 読みの無い行は、候補の文字列の中にその英字の語があるとき (LINEで) の位置だけに使う。1 字ずつ読む略語 (NHK・DVD) は載せない
struct LatinKanaRule { let place: String; let convert: String }
@MainActor var latinKanaPolicyTable: [String: LatinKanaRule]? = nil
@MainActor var latinKanaPolicyPhrases: [(String, LatinKanaRule)] = []

@MainActor func latinKanaPolicy() -> [String: LatinKanaRule] {
    if let table = latinKanaPolicyTable { return table }
    var table: [String: LatinKanaRule] = [:]
    var phrases: [(String, LatinKanaRule)] = []
    if let content = try? String(contentsOf: execURL.appendingPathComponent("latin_kana_policy.tsv"), encoding: .utf8) {
        for line in content.split(whereSeparator: \.isNewline) where !line.hasPrefix("#") {
            let parts = line.split(separator: "\t", omittingEmptySubsequences: false).map { String($0).trimmingCharacters(in: .whitespaces) }
            guard parts.count >= 2, !parts[0].isEmpty, ["high", "mid", "low", "drop"].contains(parts[1]) else { continue }
            let convert = parts.count >= 4 && ["kana", "next", "keep"].contains(parts[3]) ? parts[3] : (parts[1] == "drop" ? "next" : "kana")
            let rule = LatinKanaRule(place: parts[1], convert: convert)
            if parts.count >= 3 && !parts[2].isEmpty {
                table[parts[0] + "\t" + parts[2]] = rule
            } else {
                table[parts[0]] = rule
                if parts[0].contains(" ") { phrases.append((parts[0], rule)) }
            }
        }
    }
    latinKanaPolicyTable = table
    latinKanaPolicyPhrases = phrases
    return table
}

/// 辞書の語 1 つ (表記と読み) の扱い。読みを限った行が先
@MainActor func latinKanaRule(word: String, ruby: String) -> LatinKanaRule? {
    let table = latinKanaPolicy()
    guard !table.isEmpty, word.unicodeScalars.contains(where: { $0.isASCII && CharacterSet.letters.contains($0) }) else { return nil }
    let kana = hiraganaToKatakana(ruby.toHiragana())
    guard !kana.unicodeScalars.contains(where: { $0.isASCII }) else { return nil }   // 英字で打った語はそのまま
    return table[word + "\t" + kana]
}

/// 候補の文字列 (読みはその候補全体の読み) の中の英字の語の格付けのうち、いちばん下のもの。打った英字は見ない
@MainActor func latinKanaPlace(of text: String, reading: String) -> String? {
    let table = latinKanaPolicy()
    guard !table.isEmpty, text.unicodeScalars.contains(where: { $0.isASCII && CharacterSet.letters.contains($0) }) else { return nil }
    let typed = Set(reading.split(whereSeparator: { !($0.isASCII && $0.isLetter) }).map { $0.lowercased() })
    let order = ["high": 0, "mid": 1, "low": 2, "drop": 3]
    var worst: String? = nil
    func consider(_ word: String, _ rule: LatinKanaRule?) {
        guard let rule, rule.convert != "keep", !typed.contains(word.lowercased()) else { return }   // 英字のまま残す語は並べ替えない
        if worst == nil || order[rule.place]! > order[worst!]! { worst = rule.place }
    }
    for run in text.split(whereSeparator: { !$0.isASCII || $0 == " " }).map(String.init) { consider(run, table[run]) }
    for (phrase, rule) in latinKanaPolicyPhrases where text.contains(phrase) { consider(phrase, rule) }
    // 候補全体が 1 語のとき (あい → AI) は、読みを限った行も見る
    if !reading.isEmpty { consider(text, table[text + "\t" + hiraganaToKatakana(reading)]) }
    return worst
}

/// 2026-10-15: 辞書の絵文字・記号の語 (🇹🇭/タイ・😭/ナキガオ・🫣/ハズカシイ・×/カケル・☆☆☆/ミツボシ) が変換に選ばれたら、その文節は次の候補を使う
/// (カンボジア🇹🇭 → カンボジアタイ)。絵文字 (U+1F000 以降・国旗) を含む語か、かな・漢字・英字・数字の無い記号だけの語で、読みが記号そのものでないもの。
/// 名前に入った ☆ (らき☆すた・まどか☆マギカ) はそのまま。候補一覧では下のほうへ回す
/// 2026-10-20: 記号だけの語は、利用者が選んで覚えたもの (3 回選ぶと覚える。learning_admission.swift) だけ、ふつうの語と同じに変換に使う
func isPictographElement(_ e: DicdataElement) -> Bool {
    guard e.word != e.ruby else { return false }
    if e.word.unicodeScalars.contains(where: { (0x1F000...0x1FAFF).contains($0.value) }) { return true }
    return isSymbolOnlyElement(e) && !e.metadata.contains(.isLearned)
}

/// 2026-10-20: 記号だけの語 (カッコ→()・ハテナ→(?)・カケル→×・ヤジルシ→→)。かな・漢字・英字・数字が無く、記号を含む語。
/// 前は矢印・図形など (U+2190〜2BFF) と × ÷ ※ 〒 だけを見ていて、かっこ が () になっていた。
/// 〇 だけの語 (レイ→〇・マルマル→〇〇) は漢数字の字だが、○ と同じ形なので記号に数える。絵文字 (U+1F000 以降) は含めない
func isSymbolOnlyWord(_ word: String) -> Bool {
    let scalars = word.unicodeScalars
    if scalars.contains(where: { (0x1F000...0x1FAFF).contains($0.value) }) { return false }
    if !word.isEmpty && word.allSatisfy({ $0 == "〇" }) { return true }
    let hasText = scalars.contains { (0x3041...0x30FA).contains($0.value) || (0x4E00...0x9FFF).contains($0.value) || ($0.isASCII && ($0.properties.isAlphabetic || ("0"..."9").contains(Character($0)))) }
    return !hasText && word.contains(where: isLiteralSymbol)
}

/// かなの読みに対する、辞書の記号だけの語 (読みが記号そのものの語は除く)
func isSymbolOnlyElement(_ e: DicdataElement) -> Bool {
    e.word != e.ruby && isSymbolOnlyWord(e.word) && e.ruby.unicodeScalars.contains { (0x30A1...0x30FA).contains($0.value) }
}

/// Zenzai の変換に絵文字の語 (カンボジア🇹🇭) が入ったとき、同じ読みの辞書の語 (タイ・対・鯛・隊) とその読みのカタカナ・ひらがなを入れてみて、
/// 文全体として Zenzai がいちばん自然とみるものに替える。絵文字が出たときだけ (まれ) なので、ふだんの速さには関わらない
@MainActor func replacingPictographs(_ candidate: Candidate, hiragana: String, context: String, fast: Bool) -> Candidate? {
    let dictOptions = getOptions(context: context, prediction: false, zenzai: false, fast: true)
    let zenzOptions = getOptions(context: context, prediction: false, zenzai: true, fast: fast)
    var data = candidate.data
    var changed = false
    for i in data.indices where isPictographElement(data[i]) {
        let reading = data[i].ruby.toHiragana()
        let sub = directComposingText(reading)
        var options: [String] = []
        var placeName: String? = nil
        for c in converter.requestCandidates(sub, options: dictOptions).mainResults
            where c.correspondingCount == sub.input.count && c.data.count == 1 && !c.data.contains(where: isPictographElement) && !options.contains(c.text) {
            if placeName == nil && (1293...1294).contains(c.data[0].lcid) { placeName = c.text }
            if options.count < 6 { options.append(c.text) }
        }
        // 国旗 (🇹🇭/タイ・🇨🇳/チュウゴク) は国名を表すので、同じ読みの国名・地名 (タイ・中国) をそのまま使う (Zenzai に選ばせると カンボジア対 になる)
        let isFlag = data[i].word.unicodeScalars.contains { (0x1F1E6...0x1F1FF).contains($0.value) }
        if isFlag {
            let word = placeName ?? hiraganaToKatakana(reading)
            data[i] = DicdataElement(word: word, ruby: data[i].ruby, lcid: data[i].lcid, rcid: data[i].rcid, mid: data[i].mid, value: data[i].value())
            changed = true
            continue
        }
        // 2026-10-20: 文脈が無く、記号の語が読み全体 (かっこ・れい だけを打った) なら、Zenzai の点数は当て推量になる (れい → レイ) ので辞書の順で選ぶ。
        // 辞書の順の先頭が読みをカタカナにしただけの語 (カッコ) で、漢字の語 (括弧) もあるなら漢字の語を先に (カタカナはいつでも F7・一覧の末尾で選べる)
        if context.isEmpty && data.count == 1 && isSymbolOnlyElement(data[i]), var first = options.first {
            let hasKanji = { (word: String) in word.unicodeScalars.contains { (0x4E00...0x9FFF).contains($0.value) } }
            if first == hiraganaToKatakana(reading), let kanji = options.first(where: hasKanji) { first = kanji }
            data[i] = DicdataElement(word: first, ruby: data[i].ruby, lcid: data[i].lcid, rcid: data[i].rcid, mid: data[i].mid, value: data[i].value())
            changed = true
            continue
        }
        for form in [hiraganaToKatakana(reading), reading] where !options.contains(form) { options.append(form) }
        var bestScore = -Float.infinity
        var bestWord: String? = nil
        for word in options {
            var trial = data
            trial[i] = DicdataElement(word: word, ruby: data[i].ruby, lcid: data[i].lcid, rcid: data[i].rcid, mid: data[i].mid, value: data[i].value())
            guard let score = converter.zenzSequenceScore(hiragana: hiragana, output: trial.map(\.word).joined(), leftSideContext: context, options: zenzOptions) else { continue }
            if score > bestScore { bestScore = score; bestWord = word }
        }
        guard let word = bestWord else { continue }
        data[i] = DicdataElement(word: word, ruby: data[i].ruby, lcid: data[i].lcid, rcid: data[i].rcid, mid: data[i].mid, value: data[i].value())
        changed = true
    }
    guard changed else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
        lastMid: candidate.lastMid, data: data)
}

func hasPictograph(_ text: String) -> Bool {
    text.unicodeScalars.contains { (0x1F000...0x1FAFF).contains($0.value) } || isSymbolOnlyWord(text)
}

/// 2026-10-15: 変換に出さない「語と読みの組」(reading_suppress.tsv: 表記<TAB>読み (カタカナ)[<TAB>head])。本来の読みとは違う、崩れた読みで辞書に入っている語
/// (決して/ケシテ: けして が 消して でなく 決して になっていた)。選ばれたらその文節は次の候補を使う。候補一覧からは消さない。
/// head の行は熟語の中でだけ濁る読み (表紙/ビョウシ: 紙表紙)。変換の先頭か助詞の直後に来たときだけ避ける (びょうし → 病死)
@MainActor var readingSuppressTable: [String: Bool]? = nil
@MainActor func readingSuppress() -> [String: Bool] {
    if let table = readingSuppressTable { return table }
    var table: [String: Bool] = [:]
    if let content = try? String(contentsOf: execURL.appendingPathComponent("reading_suppress.tsv"), encoding: .utf8) {
        for line in content.split(whereSeparator: \.isNewline) where !line.hasPrefix("#") {
            let parts = line.split(separator: "\t").map(String.init)
            if parts.count >= 2 { table[parts[0] + "\t" + parts[1]] = parts.count >= 3 && parts[2] == "head" }
        }
    }
    readingSuppressTable = table
    return table
}
/// data[i] が避ける語か (head の語は、先頭か助詞の直後のときだけ)
@MainActor func isSuppressedReading(_ data: [DicdataElement], at i: Int) -> Bool {
    let table = readingSuppress()
    guard !table.isEmpty, let headOnly = table[data[i].word + "\t" + hiraganaToKatakana(data[i].ruby.toHiragana())] else { return false }
    return !headOnly || i == 0 || CIDData.isJoshi(cid: data[i - 1].rcid)
}
@MainActor func hasSuppressedReading(_ data: [DicdataElement]) -> Bool {
    data.indices.contains { isSuppressedReading(data, at: $0) }
}

/// 変換の候補に「次の候補を使う」英字の語 (あい の AI) が入っているか
@MainActor func hasNextLatin(_ candidate: Candidate) -> Bool {
    candidate.data.contains { latinKanaRule(word: $0.word, ruby: $0.ruby)?.convert == "next" }
}

/// 変換の候補の中の「カタカナにする」英字の語を、読みのカタカナにする (LINEで → ラインで)。戻すものが無ければ nil
@MainActor func katakanizingPolicyLatin(_ candidate: Candidate) -> Candidate? {
    var data = candidate.data
    var changed = false
    for i in data.indices {
        let element = data[i]
        guard let rule = latinKanaRule(word: element.word, ruby: element.ruby), rule.convert == "kana" else { continue }
        data[i] = DicdataElement(word: hiraganaToKatakana(element.ruby.toHiragana()), ruby: element.ruby, lcid: element.lcid, rcid: element.rcid,
            mid: element.mid, value: element.value())
        changed = true
    }
    guard changed else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
        lastMid: candidate.lastMid, data: data)
}

/// 候補一覧の中の格付け表の英字の語を、格付けの位置に並べ直す (同じ格付けどうしは元の順)
@MainActor func placingPolicyLatin(_ texts: [String], reading: String) -> [String] {
    var rest: [String] = []
    var high: [String] = [], mid: [String] = [], low: [String] = []
    for text in texts {
        switch latinKanaPlace(of: text, reading: reading) {
        case "high": high.append(text)
        case "mid": mid.append(text)
        case "low": low.append(text)
        case "drop": continue
        default: rest.append(text)
        }
    }
    if rest.isEmpty && !(high.isEmpty && mid.isEmpty && low.isEmpty) {
        rest.append(hiraganaToKatakana(reading))   // 英字の語しか無いときも、カタカナを先に
    }
    rest.insert(contentsOf: high, at: min(1, rest.count))
    rest.insert(contentsOf: mid, at: min(4, rest.count))
    return rest + low
}

/// 2026-10-15: カタカナの固有名詞のすぐ後で、普通の語のカタカナ表記 (カッカ・バンザイ) が選ばれたとき、同じ語でより強い漢字 (閣下・万歳) に戻す
/// (アルトーカッカバンザイ → アルトー閣下万歳)。表は katakana_variants.tsv (外来語の読み・語彙素の違う組は除いてある)。マジ・ホント のように
/// カタカナのほうが強い語は表に無いので変わらない
@MainActor var katakanaVariantTable: [String: (word: String, value: PValue)]? = nil
@MainActor func kanjiAfterKatakanaNames(_ candidate: Candidate) -> Candidate? {
    if katakanaVariantTable == nil {
        var table: [String: (word: String, value: PValue)] = [:]
        if let content = try? String(contentsOf: execURL.appendingPathComponent("katakana_variants.tsv"), encoding: .utf8) {
            for line in content.split(whereSeparator: \.isNewline) where !line.hasPrefix("#") {
                let parts = line.split(separator: "\t").map(String.init)
                if parts.count >= 3, let v = Float(parts[2]) { table[parts[0]] = (parts[1], PValue(v)) }
            }
        }
        katakanaVariantTable = table
    }
    guard let table = katakanaVariantTable, !table.isEmpty, candidate.data.count >= 2 else { return nil }
    func isKatakanaWord(_ w: String) -> Bool { !w.isEmpty && w.unicodeScalars.allSatisfy { (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC } }
    var data = candidate.data
    var changed = false
    // カタカナの固有名詞から続くカタカナの語の並びの中だけを見る (アルトー|カッカ|バンザイ の カッカ と バンザイ)
    var afterName = false
    for i in 0..<data.count {
        let element = candidate.data[i]
        defer {
            if isKatakanaWord(element.word) && (1288...1294).contains(element.rcid) { afterName = true }
            else if !isKatakanaWord(element.word) { afterName = false }
        }
        guard i > 0, afterName, element.word == element.ruby, isKatakanaWord(element.word), [1283, 1285, 1287].contains(element.lcid),
              let kanji = table[element.ruby], kanji.value > element.value() else { continue }
        data[i] = DicdataElement(word: kanji.word, ruby: element.ruby, lcid: element.lcid, rcid: element.rcid, mid: element.mid, value: element.value())
        changed = true
    }
    guard changed else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
        lastMid: candidate.lastMid, data: data)
}

/// 2026-11-17 (A → C、「/」の中黒の見本 11-09 で 50 語中 6): 「/」で打った中黒の左の名が、かな・漢字のまま残る (とむ・ヒドルストン・らら・クロフト・亜嵐・スミシー)。
/// 読みは記号の所で切って部分ごとに変換する (preservingLiteralSymbols) ので、「・」の左の名は短い読みだけで変換され、辞書のかな・漢字の形
/// (富む のかな・名の 亜嵐) が選ばれる。kanjiAfterKatakanaNames の鏡の形で、「・」のすぐ右の部分の頭がカタカナの語のとき、左の部分の後ろの語の並び
/// (助詞の形の語の手前まで) を、次のどちらかなら読みのカタカナにする:
/// 1. 読みのカタカナが、辞書だけの変換で 1 語の名のカタカナ語 (品詞 1289〜1294: トム・ララ・アラン・アガサ) として出て、同じ読みのふつうの語と
///    左に選ばれた語のどれも、それより nakaguroNameMargin 以上強くない。左の部分全体 (あめでお) を先に見て、だめなら助詞の形の語より後ろだけ
/// 2. 助詞の形の語より後ろだけを見るとき: 左がひらがなだけで、その読みの語が辞書に 1 つも無い (こーびん)。辞書だけで変換して活用する語を含むか
///    助詞で終わる句 (きたよ・すごいね) は除く (readsAsPhrase、2026-11-19)
/// 左の語がカタカナ語より強い (東京・ニューヨーク の 東京)・カタカナ語が固有名詞でない (ごはん・パン の ゴハン)・右がカタカナでない (きゃりー・ぱみゅぱみゅ) ときは替えない。
/// 試験用のビルドでは AZK_NAKAGURO_NAMES=0 で切れる (前と後を同じビルドで測るため)
let nakaguroNameMargin: PValue = 1.0

/// 読み (カタカナ) → 辞書だけの変換で読み全体が 1 語になる語 (変換の間は覚えておく。collocDictionaryWord と同じ引き方)
@MainActor var dictionarySingleWordMemo: [String: [DicdataElement]] = [:]
@MainActor func dictionarySingleWords(ruby: String) -> [DicdataElement] {
    if let hit = dictionarySingleWordMemo[ruby] { return hit }
    let reading = String(String.UnicodeScalarView(ruby.unicodeScalars.map { scalar in
        (0x30A1...0x30F6).contains(scalar.value) ? Unicode.Scalar(scalar.value - 0x60)! : scalar
    }))
    let composing = directComposingText(reading)
    let count = composing.input.count
    let found = converter.requestCandidates(composing, options: getOptions(context: "", prediction: false, zenzai: false, nBest: 30)).mainResults
        .filter { $0.correspondingCount == count && $0.data.count == 1 && $0.data[0].ruby == ruby }.map { $0.data[0] }
    if dictionarySingleWordMemo.count >= 256 { dictionarySingleWordMemo.removeAll() }
    dictionarySingleWordMemo[ruby] = found
    return found
}

/// 「・」の左の語の並び (読み・選ばれた表記) を、読みのカタカナにするなら、そのカタカナ (しないなら nil)。
/// 1. 読みのカタカナの名 (品詞 1289〜1294: 人名・組織・地域) が辞書にあり、同じ読みのふつうの名詞 (豆腐・寿司・箸) がそれと同じか強くなく、ほかの語 (富む) と
///    選ばれた語がそれより nakaguroNameMargin 以上強くない (すし・はし・とうふ を人名などの スシ・ハシ・トウフ には替えない)
/// 2. unknown のときだけ: 左がひらがなだけで、その読みの語が辞書に 1 つも無い (こーびん。変換エンジンが足す読みのカタカナ・ひらがなの候補は数えない)
/// right は「・」のすぐ右の語。カタカナだけ (ヒドルストン) でなく頭がカタカナ 2 字以上 (ユーラシア大陸・クリスティ作) のときは、頭のカタカナの並びが
/// 辞書でふつうの名詞として名より強くないときの 1 だけ (2026-11-18。辞書に無いひらがなの 2 は、右がカタカナだけのとき。
/// 試験用のビルドでは AZK_NAKAGURO_RIGHT=whole で右をカタカナだけの語に限る前の形)
@MainActor func katakanaNameBeforeNakaguro(reading: String, chosen: String, unknown: Bool, right: String) -> String? {
    guard devEnvironment["AZK_NAKAGURO_NAMES"] != "0" else { return nil }
    func isKatakanaWord(_ w: String) -> Bool { !w.isEmpty && w.unicodeScalars.allSatisfy { (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC } }
    let rightWhole = isKatakanaWord(right)
    if !rightWhole {
        if devEnvironment["AZK_NAKAGURO_RIGHT"] == "whole" { return nil }
        // 右の頭のカタカナの並び (ユーラシア大陸 の ユーラシア) が、辞書でふつうの名詞 (品詞 1283〜1287) として名 (1289〜1294) より強ければ、
        // 「・」はふつうの語の並べ方 (毬・ボール遊び・すし・ラーメン屋) と見て替えない。辞書に無い並びは名と見る
        let head = String(String.UnicodeScalarView(right.unicodeScalars.prefix { (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC }))
        let headWords = dictionarySingleWords(ruby: head)
        let headName = headWords.filter { $0.word == head && (1289...1294).contains($0.lcid) }.map { $0.value() }.max()
        if let noun = headWords.filter({ (1283...1287).contains($0.lcid) }).map({ $0.value() }).max(), noun >= (headName ?? -.infinity) { return nil }
    }
    func isHiraganaWord(_ w: String) -> Bool { !w.isEmpty && w.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) || $0.value == 0x30FC } }
    let ruby = hiraganaToKatakana(reading)
    guard ruby.count >= 2, isHiraganaWord(reading), isKatakanaWord(ruby), !isKatakanaWord(chosen) else { return nil }
    // 変換エンジンが読み全体に足すカタカナ・ひらがなの候補 (品詞 固有名詞一般・意味 一般。getAdditionalCandidate) は辞書の語に数えない
    let words = dictionarySingleWords(ruby: ruby).filter {
        !($0.lcid == CIDData.固有名詞.cid && $0.mid == MIDData.一般.mid && ($0.word == ruby || $0.word == reading))
    }
    // 名の語は人名・組織・地域の品詞 (1289〜1294。トム・ララ・アラン・アガサ・アメデオ)
    if let name = words.filter({ $0.word == ruby && (1289...1294).contains($0.lcid) }).max(by: { $0.value() < $1.value() }) {
        // ふつうの名詞 (品詞 1283〜1287: 豆腐・寿司・箸) は、カタカナの名と同じか強ければ替えない。動詞など (富む) と選ばれた語は幅 nakaguroNameMargin
        if let noun = words.filter({ (1283...1287).contains($0.lcid) }).map({ $0.value() }).max(), noun >= name.value() { return nil }
        let rivals = words.filter { !(1289...1294).contains($0.lcid) || $0.word == chosen }.map { $0.value() }
        if let strongest = rivals.max(), strongest > name.value() + nakaguroNameMargin { return nil }
        return ruby
    }
    if unknown, rightWhole, isHiraganaWord(chosen), chosen == reading, words.isEmpty,
       devEnvironment["AZK_NAKAGURO_PHRASE"] == "0" || !readsAsPhrase(reading) { return ruby }
    return nil
}

/// 2026-11-19 (まとめ役の rc-next 234b9b9 の見直しの 4): 辞書に無いひらがなの左 (2 の決まり) が、名でなく句か。読みを辞書だけで変換した 1 番目が、
/// 活用する語 (動詞・形容詞・助動詞。すごいね・だよね・がんばれ。isInflectingCid) を含むか、助詞 (終助詞も) で終わる (きた|よ) なら句とみる
/// (くだけた「/」の打ち方の きたよ・メール が キタヨ・メール になっていた)。名 (こー|びん) は名詞の切れ端に分かれるので替えない。
/// 試験用のビルドでは AZK_NAKAGURO_PHRASE=0 で前の形
@MainActor func readsAsPhrase(_ reading: String) -> Bool {
    let composing = directComposingText(reading)
    let count = composing.input.count
    guard let best = converter.requestCandidates(composing, options: getOptions(context: "", prediction: false, zenzai: false, nBest: 5)).mainResults
        .first(where: { $0.correspondingCount == count }) else { return false }
    return best.data.contains { isInflectingCid($0.lcid) } || best.data.last.map { CIDData.isJoshi(cid: $0.lcid) } == true
}

/// 2026-10-15: 辞書にない外来語・海外の人名が、辞書の短い語の切れ端に崩れた所 (グロー|馬|リスト・ル|岸|スク・ヘ|グセ|す) を、読みのカタカナにする。
/// 名詞 (接頭語・接尾語を含む) が 2 つ以上続くまとまりで、カタカナを含み、切れ端 (カタカナ 1 字・漢字 1 字・ひらがな 1〜2 字・カタカナと漢字/かなの混ざり) があり、
/// 読み 1 字あたりの重みが弱い (-4.5 より下) ときだけ。助詞・助動詞では切るので「ロシア」と「ウクライナ」はまとめない。
/// 正しい複合語 (ロシア軍・トランプ氏・猫ミーム) は語の重みが強いので当たらない (リポジトリの外の評価 (2026-10-15) で 0/231)
let brokenLoanwordThreshold: Double = -4.5
func katakanizingBrokenLoanwords(_ candidate: Candidate) -> Candidate? {
    func isKatakanaWord(_ w: String) -> Bool { !w.isEmpty && w.unicodeScalars.allSatisfy { (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC } }
    func hasKatakana(_ w: String) -> Bool { w.unicodeScalars.contains { (0x30A1...0x30FA).contains($0.value) } }
    func isKanji(_ s: Unicode.Scalar) -> Bool { (0x4E00...0x9FFF).contains(s.value) || s.value == 0x3005 }
    func isHiragana(_ s: Unicode.Scalar) -> Bool { (0x3041...0x3096).contains(s.value) || s.value == 0x30FC }
    // 助詞と同じ形のかな (名詞として入っている で・を) は区切りにする (ヘグセスで が ヘグセスデ にならないように)
    let particleLike: Set<String> = ["で", "を", "が", "に", "は", "と", "も", "へ", "の", "や", "か", "デ", "ヲ", "ガ", "ニ", "ハ", "ト", "モ", "ヘ", "ノ", "ヤ", "カ"]
    func isNoun(_ e: DicdataElement) -> Bool {
        ((1283...1310).contains(e.lcid) || e.lcid == 560) && !CIDData.isJoshi(cid: e.rcid) && !(particleLike.contains(e.word) && e.word != "ヘ")
            && e.word.unicodeScalars.contains { isKanji($0) || isHiragana($0) || (0x30A1...0x30FA).contains($0.value) }
    }
    func isFragment(_ e: DicdataElement) -> Bool {
        let w = e.word.unicodeScalars
        if isKatakanaWord(e.word) { return e.ruby.count == 1 }
        if w.count == 1 && w.allSatisfy(isKanji) { return true }
        if w.count <= 2 && w.allSatisfy(isHiragana) { return true }
        return hasKatakana(e.word) && w.contains { isKanji($0) || isHiragana($0) }
    }
    var data: [DicdataElement] = []
    var changed = false
    var i = 0
    let elements = candidate.data
    while i < elements.count {
        guard isNoun(elements[i]) else { data.append(elements[i]); i += 1; continue }
        var j = i
        while j < elements.count && isNoun(elements[j]) { j += 1 }
        let run = Array(elements[i..<j])
        let ruby = run.map(\.ruby).joined()
        let mora = ruby.count
        let perMora = run.reduce(0.0) { $0 + Double($1.value()) } / Double(max(mora, 1))
        // カタカナ語＋漢字 1 字 (トスカナ語・アラム人・ロシア軍・クマ餌) は正しい複合語の形なので、切れ端がそれだけのときは見ない。
        // 2 字以上の漢字の語 (箇所・主義) が入るまとまりも見ない
        let fragments = run.indices.filter { isFragment(run[$0]) }
        let onlySuffixes = fragments.allSatisfy { k in
            k > 0 && run[k].word.unicodeScalars.count == 1 && run[k].word.unicodeScalars.allSatisfy(isKanji)
                && isKatakanaWord(run[k - 1].word) && run[k - 1].ruby.count >= 2
        }
        let hasKanjiWord = run.contains { $0.word.unicodeScalars.filter(isKanji).count >= 2 }
        if run.count >= 2 && mora >= 3 && run.contains(where: { hasKatakana($0.word) }) && !fragments.isEmpty && !onlySuffixes && !hasKanjiWord
            && run.contains(where: { !isKatakanaWord($0.word) }) && perMora < brokenLoanwordThreshold
            && ruby.unicodeScalars.allSatisfy({ (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC }) {
            // 最後の語がひらがなで、助詞の形で終わるとき (ヘ|グセ|すで の で) は、その助詞をひらがなのまま残す
            var head = ruby
            var tail = ""
            if let last = run.last, last.word.unicodeScalars.allSatisfy(isHiragana), last.word.count >= 2, let c = last.word.last,
               ["で", "を", "が", "に", "は", "と", "も", "へ", "の"].contains(String(c)) {
                head = String(ruby.dropLast())
                tail = String(c)
            }
            data.append(DicdataElement(word: head + tail, ruby: ruby, lcid: run[0].lcid, rcid: run[run.count - 1].rcid, mid: run[0].mid,
                value: run.reduce(PValue(0)) { $0 + $1.value() }))
            changed = true
        } else {
            data += run
        }
        i = j
    }
    guard changed else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
        lastMid: candidate.lastMid, data: data)
}

/// 2026-10-15: かなで打った読みの候補に、辞書の英字の語 (iPad/アイパッド・iPhone/アイフォン) が入っていたら、その語だけを読みのカタカナに戻す。
/// 助詞などほかの語はそのまま (iPadで → アイパッドで)。打った英字 (読みに英字のまま入っている語) は戻さない。戻すものが無ければ nil
func katakanizingLatinWords(_ candidate: Candidate, reading: String) -> String? {
    var text = ""
    var changed = false
    for element in candidate.data {
        let hasLatin = element.word.unicodeScalars.contains { $0.isASCII && CharacterSet.letters.contains($0) }
        let ruby = element.ruby.toHiragana()
        if hasLatin && !ruby.isEmpty && !latinMatchesReading(element.word, reading: ruby) {
            text += hiraganaToKatakana(ruby)
            changed = true
        } else {
            text += element.word
        }
    }
    guard changed, latinMatchesReading(text, reading: reading), !isPartialKatakana(text, reading: reading) else { return nil }
    return text
}

@MainActor func directComposingText(_ text: String) -> ComposingText {
    var composing = ComposingText()
    composing.insertAtCursorPosition(text, inputStyle: .direct)
    return composing
}

@MainActor func leftContext(_ extra: String) -> String {
    let base = (config["context"] as? String) ?? ""
    return base + extra
}

/// 融合採点の重み (環境変数 AZK_FUSION_* で変えられる)。
/// score = logP_zenz(候補 | 読み, 左文脈) + lat·(辞書の値の Zenzai 最終候補との差のうち latDeadZone を超えた分。語数が Zenzai より少ない候補だけ) + words·語数 + learn·学習語数 + zbest·[Zenzai の最終候補] + kata·[読み全体のカタカナ] + frag·漢字1字の切れ端数
struct FusionWeights {
    var enabled: Bool
    var lat: Float
    var latDeadZone: Float
    var latFewerOnly: Bool
    var words: Float
    var learn: Float
    var zbest: Float
    var kata: Float
    var frag: Float
    var maxPool: Int
    var dumpPath: String?
    var domainBonus: Float
    var domainSignals: Int
    /// 2026-10-31: 文の頭で、頭の語が助詞の候補 (も|ビール) から引く点
    var headParticle: Float
}

/// 2026-10-31: 文の頭か (変換サーバーが知っている左の文脈が空か、文の終わりの字 (。！？.!? と改行) で終わる)。空白は読み飛ばす
func isSentenceHead(_ left: String) -> Bool {
    guard let last = left.last(where: { !$0.isWhitespace || $0.isNewline }) else { return true }
    return "。！？.!?\n\r".contains(last)
}

/// 2026-10-31: 融合採点で、文の頭の助詞で始まる候補を下げるか (頭の語は読みのある最初の語。助詞の品詞は CIDData.isJoshi)。
/// 文の頭の「も」で始まる読み (もびーるをかざる) を、AI が「も|ビール|を|飾る」にしていた (モデルの点の差は 1 点ほど)。
/// 文の途中 (私|も|ビール) は左の文脈で見分ける。左の文脈を読めない欄では、IME 本体が直前に確定した字を送る
func startsWithParticleAtSentenceHead(_ data: [DicdataElement], left: String) -> Bool {
    isSentenceHead(left) && (data.first(where: { !$0.ruby.isEmpty }).map { CIDData.isJoshi(cid: $0.lcid) } ?? false)
}

/// 2026-10-19 (security review #5): 試験用の環境変数 (AZK_*) は、試験用のビルド (swiftc -D AZK_DEV_KNOBS、build_swift.ps1 -DevKnobs) でだけ読む。
/// 配布する版では空 (利用者の環境変数で、変換の重み・分野辞書・書き出し先のパス (AZK_DEBUG・AZK_FUSION_DUMP・AZK_DUMP_DATA) を変えられないように)
let devEnvironment: [String: String] = {
#if AZK_DEV_KNOBS
    return ProcessInfo.processInfo.environment.filter { $0.key.hasPrefix("AZK_") }
#else
    return [:]
#endif
}()

let fusionProf = devEnvironment["AZK_FUSION_PROF"] == "1"

let fusion: FusionWeights = {
    let env = devEnvironment
    func f(_ key: String, _ d: Float) -> Float { env[key].flatMap { Float($0) } ?? d }
    return FusionWeights(
        enabled: (env["AZK_FUSION"] ?? "1") != "0",
        lat: f("AZK_FUSION_LAT", 0.3),
        latDeadZone: f("AZK_FUSION_LAT_DEADZONE", 5),
        latFewerOnly: (env["AZK_FUSION_LAT_FEWER"] ?? "1") != "0",
        words: f("AZK_FUSION_WORDS", -0.25),
        learn: f("AZK_FUSION_LEARN", 0),
        zbest: f("AZK_FUSION_ZBEST", 0),
        kata: f("AZK_FUSION_KATA", -3),
        frag: f("AZK_FUSION_FRAG", 0),
        maxPool: Int(f("AZK_FUSION_K", 8)),
        dumpPath: env["AZK_FUSION_DUMP"].flatMap { $0.isEmpty ? nil : $0 },
        domainBonus: f("AZK_DOMAIN_BONUS", 2.0),
        domainSignals: Int(f("AZK_DOMAIN_SIGNALS", 1)),
        // 2 点: もびーるをかざる・もびーるがゆれる (差 0.9・1.2) が モビール になり、一般の文 3,675 文で変わるのは 1 区切り
        // (のうちは → 農地は)、くだけた例文 1,994 文で 0 (2026-10-31。3 点では でなるようになります が 出なる に崩れた)
        headParticle: f("AZK_FUSION_HEAD_PARTICLE", 2)
    )
}()

/// 融合採点: Zenzai が途中で作った候補・辞書だけの上位・切れ端を熟語に差し替えた候補・読み全体のカタカナを集め、
/// zenz の対数確率と辞書の値などを足した点で選び直す。Zenzai の最終候補は必ずプールに入っている
@MainActor func fusionChoose(hiragana: String, context: String, count: Int, zenzBest: Candidate, fast: Bool, isFragmentWord: (String) -> Bool) -> Candidate {
    // 2026-10-29: Zenzai の最終候補に利用者が新しく作った語 (学習した語のうち辞書に無いもの。ユーザ辞書の印つき) があれば選び直さない
    // (選び直しの採点は「辞書:」の条件を付けないので、その語を知らないモデルの点で外してしまう)
    // 2026-10-31: 読み間違いの本来の形 (ふいんき → 雰囲気) があるときも同じ (選び直しの採点はモデルの点が主で、読みどおりの字に戻してしまう)
    // 2026-11-04: 利用者が単語登録した語 (isUserRegistered) があるときも同じ (選び直しの採点で、モデルの知らない登録した語を外さない)
    if zenzBest.data.contains(where: { ($0.metadata.contains(.isLearned) && $0.metadata.contains(.isFromUserDictionary)) || $0.metadata.contains(.isReadingCorrection)
        || $0.metadata.contains(.isUserRegistered) }) {
        return zenzBest
    }
    let t0 = Date()
    struct Entry { var c: Candidate; var src: String }
    var pool: [Entry] = [Entry(c: zenzBest, src: "z")]
    var index: [String: Int] = [zenzBest.text: 0]
    func add(_ c: Candidate, _ src: String) {
        guard c.correspondingCount == count, !c.text.isEmpty, !hasNGWord(c.text) else { return }
        if let i = index[c.text] {
            if i != 0 && c.value > pool[i].c.value { pool[i].c = c }
            if i != 0 && src == "t" { pool[i].src = src }
            return
        }
        index[c.text] = pool.count
        pool.append(Entry(c: c, src: src))
    }
    for c in converter.lastZenzaiPool { add(c, "c") }
    if let coordinated = nounCoordinationAlternative(zenzBest, lookup: { reading, text in
        converter.dictionaryCandidate(directComposingText(reading), matching: text,
            options: getOptions(context: context, prediction: false, zenzai: false, fast: true))
    }) { add(coordinated, "t") }
    func fragments(_ c: Candidate) -> Int { c.data.filter { isFragmentWord($0.word) }.count }
    if fusionProf { FileHandle.standardError.write("FUSIONPROF afterPool=\(Int(-t0.timeIntervalSinceNow * 1000))\n".data(using: .utf8)!) }
    // 切れ端 (漢字 1 字) が多い候補は、切れ端とその前後の読みをまとめて辞書で引き、1 語で書ける熟語に差し替えた候補も作る (張遼/跋/孤 → 跳梁跋扈)
    let kanjiOnly: (String) -> Bool = { $0.unicodeScalars.allSatisfy { (0x4E00...0x9FFF).contains($0.value) || $0.value == 0x3005 } }
    // 辞書を引き直すので重い。Zenzai の最終候補に切れ端が 2 つ以上あるときだけ (それと、辞書の値が最良の候補の区切りでも探す)
    var bases: [Candidate] = []
    if fragments(zenzBest) >= 2 {
        bases.append(zenzBest)
        if let d = pool.dropFirst().max(by: { $0.c.value < $1.c.value })?.c, fragments(d) >= 1 { bases.append(d) }
    }
    var synthetic = 0
    let dictOptions = bases.isEmpty ? nil : getOptions(context: context, prediction: false, zenzai: false, fast: fast)
    for base in bases where synthetic < 6 {
        let elems = base.data
        var spans: Set<[Int]> = []
        for f in elems.indices where isFragmentWord(elems[f].word) {
            for lo in max(0, f - 2)...f {
                for hi in f..<min(elems.count, f + 3) where hi > lo {
                    spans.insert([lo, hi])
                }
            }
        }
        for span in spans.sorted(by: { ($0[1] - $0[0], $0[0]) > ($1[1] - $1[0], $1[0]) }) where synthetic < 6 {
            let (lo, hi) = (span[0], span[1])
            let reading = elems[lo...hi].map { $0.ruby }.joined().toHiragana()
            let sub = directComposingText(reading)
            let subCount = sub.input.count
            for word in converter.requestCandidates(sub, options: dictOptions!).mainResults
                where word.correspondingCount == subCount && word.data.count == 1 && word.text.count >= 2 && kanjiOnly(word.text) && !hasNGWord(word.text) {
                let data = Array(elems[..<lo]) + word.data + Array(elems[(hi + 1)...])
                let text = data.map { $0.word }.joined()
                if index[text] != nil { break }
                // 値は差し替えた語の分だけ入れ替える (連接の項は無視した近似)
                let value = base.value - elems[lo...hi].reduce(0) { $0 + $1.value() } + word.data[0].value()
                add(Candidate(text: text, value: value, correspondingCount: count, lastMid: base.lastMid, data: data), "f")
                synthetic += 1
                break
            }
        }
    }
    if fusionProf { FileHandle.standardError.write("FUSIONPROF afterFrag=\(Int(-t0.timeIntervalSinceNow * 1000))\n".data(using: .utf8)!) }
    // Whole-reading katakana remains an explicit candidate, not a synthetic
    // automatic winner: treating a sentence as one noun discards its grammar.
    // 多すぎるときは Zenzai の最終候補と、辞書の値の上位を残す
    if pool.count > fusion.maxPool {
        let head = pool[0]
        let special = pool.dropFirst().filter { $0.src == "f" || $0.src == "k" || $0.src == "t" }
        let rest = pool.dropFirst().filter { $0.src == "c" }.sorted { $0.c.value > $1.c.value }
        pool = [head] + Array(rest.prefix(max(0, fusion.maxPool - 1 - special.count))) + special
        // 絞ったあとに候補を足す (分野の差し替え) ので、表記 → 位置の表を作り直す (古い位置のままだと配列の外を読んで落ちる)
        index = [:]
        for (i, e) in pool.enumerated() where index[e.c.text] == nil { index[e.c.text] = i }
    }
    // 分野の文脈なら、Zenzai の最終候補の語 (1 語か隣り合う 2 語) を、同じ読みの分野の語に差し替えた候補を足す (再建 → 債権)。
    // プールを絞ったあとで 3 つまで足す (先に足すと Zenzai の途中の候補を押し出し、書き方の選び直しが変わった: 2026-10-21 法律の文 719→693)
    let activeBoosts = domainBoosts.filter { $0.isActive(context + zenzBest.text, minimum: fusion.domainSignals) }
    if !activeBoosts.isEmpty {
        let elems = zenzBest.data
        var variants = 0
        for i in elems.indices where variants < 3 {
            for len in 1...2 where i + len <= elems.count {
                let span = elems[i..<(i + len)]
                // 助詞・助動詞を含む語は差し替えない (型/で → 型出 になった)
                if span.contains(where: { (147...549).contains($0.lcid) || (147...549).contains($0.rcid) }) { continue }
                let ruby = span.map(\.ruby).joined()
                let current = span.map(\.word).joined()
                // 元の語が既に分野の語なら差し替えない (控訴→公訴・勾留→拘留・科料→過料 のように分野の語どうしを入れ替えてしまう)
                if activeBoosts.contains(where: { $0.pairs.contains(ruby + "\t" + current) || $0.signals.contains(current) }) { continue }
                // 送り仮名だけの違い (取り消し/取消し) は差し替えない (法令風の表記を押し付けない)
                for boost in activeBoosts {
                    for word in boost.byReading[ruby] ?? [] where word != current && notationSkeleton(word) != notationSkeleton(current) && variants < 3 {
                        let replaced = DicdataElement(word: word, ruby: ruby, lcid: span.first!.lcid, rcid: span.last!.rcid, mid: span.last!.mid,
                                                      value: span.reduce(0) { $0 + $1.value() })
                        let data = Array(elems[..<i]) + [replaced] + Array(elems[(i + len)...])
                        add(Candidate(text: data.map(\.word).joined(), value: zenzBest.value, correspondingCount: count, lastMid: zenzBest.lastMid, data: data), "d")
                        variants += 1
                    }
                }
            }
        }
        // 2026-10-31: 差し替えられる所を全部まとめて差し替えた候補も 1 つ足す (2 か所以上のときだけ)。1 か所ずつの候補だけでは、
        // 初稿7、交差2 → 初項7、交差2 のように、どれか 1 つしか直らなかった (数学の文の組)。決まりは上と同じで、2 語の並びを先に見る
        if let combined = combinedDomainVariant(elems, boosts: activeBoosts) {
            add(Candidate(text: combined.map(\.word).joined(), value: zenzBest.value, correspondingCount: count, lastMid: zenzBest.lastMid, data: combined), "d")
        }
    }
    if pool.count == 1 && fusion.dumpPath == nil { return zenzBest }
    let t1 = Date()
    if fusionProf { FileHandle.standardError.write("FUSIONPROF beforeScore=\(Int(-t0.timeIntervalSinceNow * 1000))\n".data(using: .utf8)!) }
    let zenzOptions = getOptions(context: context, prediction: false, zenzai: true, fast: fast)
    guard let lms = converter.zenzScoreSequences(hiragana: hiragana, outputs: pool.map(\.c.text), options: zenzOptions), lms.count == pool.count else {
        return zenzBest
    }
    if fusionProf { FileHandle.standardError.write("FUSIONPROF afterScore=\(Int(-t0.timeIntervalSinceNow * 1000))\n".data(using: .utf8)!) }
    var best = 0
    var bestScore = -Float.infinity
    var rows: [String] = []
    // 辞書の値は全候補を同じ式で計算し直す (カタカナ 1 語の候補は語の情報がないので Zenzai の最終候補と同じ値)
    let zValue = converter.fusionPathValue(zenzBest.data)
    let values: [Float] = pool.map { $0.src == "k" || $0.c.data.isEmpty ? zValue : converter.fusionPathValue($0.c.data) }
    let zWords = zenzBest.data.count
    let left = leftContext(context)
    // 2026-10-31: 自動の変換に選ばない語 (記号だけの語 〇・絵文字) を含む候補は、ほかに候補があれば選ばない。選んだあとでその語だけ別の字に替える
    // (replacingPictographs) ので、区切りがその語に合わせたまま残る (れいだい3へ: モデルの点は 〇第3へ が上で、〇 を 令 に替えて 令第3へ になっていた)
    // 2026-11-02: 形容動詞の「な」のすぐ後ろに助詞・句読点が付いた候補 (まず高価なと思う) も、ほかに候補があれば選ばない (adjectival_na.swift)
    let selectable = pool.map { !$0.c.data.contains(where: isPictographElement) && !(adjectivalNaFilter && hasDanglingAdjectivalNa($0.c.data)) }
    let anySelectable = selectable.contains(true)
    for (i, e) in pool.enumerated() {
        let learned = e.c.data.filter { $0.metadata.contains(.isLearned) }
        let learnedChars = learned.reduce(0) { $0 + $1.ruby.count }
        let fr = fragments(e.c)
        let words = e.src == "k" ? 1 : e.c.data.count
        // 辞書の値は、Zenzai の最終候補より語数が少ない候補 (切れ端を熟語にまとめた形) にだけ、差が latDeadZone を超えた分を効かせる。
        // 語数が同じ候補どうし (至難の業/至難の技、追及/追求) では辞書の値の壊れ (わざ→業 −30) や小差に引っ張られるので使わない
        let dv = values[i] - zValue
        var dvEffective = dv > fusion.latDeadZone ? dv - fusion.latDeadZone : (dv < -fusion.latDeadZone ? dv + fusion.latDeadZone : 0)
        if fusion.latFewerOnly && !(words < zWords) { dvEffective = 0 }
        // 分野の文脈なら、Zenzai の最終候補の語を分野の語に差し替えた候補 (src "d") にだけ domainBonus を足す。
        // ほかの候補に足すと、分野の語と一緒にほかの書き方 (とき/時・いう/言う) まで選び直してしまう (2026-10-21 法律の文 725→693)
        // 2026-10-31: 差し替えた語 1 つにつき足す (まとめて差し替えた候補 初項・公差 が、1 か所だけの候補 初稿・公差 と同じ加点で負けていた)
        let boosted = e.src == "d" ? max(1, replacedDomainWords(e.c.data, base: zenzBest.data, boosts: activeBoosts)) : 0
        let score = lms[i] + fusion.lat * dvEffective + fusion.words * Float(words) + fusion.learn * Float(learned.count)
            + (e.src == "z" ? fusion.zbest : 0) + (e.src == "k" ? fusion.kata : 0) + fusion.frag * Float(fr)
            + (e.src == "t" ? 4.0 : 0) + fusion.domainBonus * Float(boosted)
            - (startsWithParticleAtSentenceHead(e.c.data, left: left) ? fusion.headParticle : 0)
        if score > bestScore && (selectable[i] || !anySelectable) {
            bestScore = score
            best = i
        }
        if fusion.dumpPath != nil {
            func esc(_ s: String) -> String { s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"").replacingOccurrences(of: "\n", with: "\\n").replacingOccurrences(of: "\r", with: "\\r").replacingOccurrences(of: "\t", with: "\\t") }
            rows.append("{\"t\":\"\(esc(e.c.text))\",\"src\":\"\(e.src)\",\"lm\":\(lms[i]),\"v\":\(values[i]),\"v0\":\(e.c.value),\"w\":\(words),\"l\":\(learned.count),\"lc\":\(learnedChars),\"fr\":\(fr)}")
        }
    }
    if let path = fusion.dumpPath {
        func esc(_ s: String) -> String { s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"").replacingOccurrences(of: "\n", with: "\\n").replacingOccurrences(of: "\r", with: "\\r").replacingOccurrences(of: "\t", with: "\\t") }
        let ms = "[\(Int(t1.timeIntervalSince(t0) * 1000)),\(Int(-t1.timeIntervalSinceNow * 1000))]"
        let line = "{\"ms\":\(ms),\"h\":\"\(esc(hiragana))\",\"ctx\":\"\(esc(context))\",\"cands\":[\(rows.joined(separator: ","))]}\n"
        if let handle = FileHandle(forWritingAtPath: path) {
            handle.seekToEndOfFile()
            handle.write(line.data(using: .utf8)!)
            try? handle.close()
        } else {
            FileManager.default.createFile(atPath: path, contents: line.data(using: .utf8))
        }
    }
    return pool[best].c
}

/// 入力全体を変換し、最良候補を文節に区切って返す (text=表記, correspondingCount=読みの文字数)
/// 比べ直しで Zenzai の結果に足す点 (AZK_RESCORE_MARGIN で試せる)
let rescoreMargin: Float = {
    if let v = devEnvironment["AZK_RESCORE_MARGIN"], let f = Float(v) { return f }
    return 0
}()

/// 漢字 1 字の切れ端 1 つあたりの減点 (対数確率)。AZK_FRAGMENT_PENALTY で試せる
let fragmentPenalty: Float = {
    if let v = devEnvironment["AZK_FRAGMENT_PENALTY"], let f = Float(v) { return f }
    return 0.7
}()

@_silgen_name("ConvertSegments")
@MainActor public func convert_segments(
    hiraganaPtr: UnsafePointer<CChar>,
    contextPtr: UnsafePointer<CChar>,
    fast: Bool,
    dictionaryOnly: Bool,
    lengthPtr: UnsafeMutablePointer<Int32>
) -> UnsafeMutablePointer<UnsafeMutablePointer<FFICandidate>?> {
    refreshUserDictionaryIfChanged()
    refreshAfterLearnGuardLoaded()  // 2026-11-22: 学習の直しの表を読み終えた直後に 1 回、前の覚えを捨てる (learn_guard.swift)
    let hiragana = normalizeForConversion(String(cString: hiraganaPtr))
    let context = leftContext(String(cString: contextPtr))
    rightContextReading = hiragana
    // 2026-11-08 (仕事 52): 重いので AI を休んでいるなら、この変換は辞書だけ (並べ方は設定 input.ai_pause_order。辞書だけ向けは仕事 54 の直しをかける)。
    // 休んでいる間の清書は、30 秒に 1 回だけ AI の試しにする。AI でした変換は、かかった時間を数える (ai_load_guard.swift)
    let ai = beginAIConversion(dictionaryOnly: dictionaryOnly)
    lastConversionAIPaused = ai.paused
    // 2026-11-18 (perf の 7): zenz を裏で読んでいる間は、AI の変換も辞書だけ (休みの印は立てない。かかった時間も数えない)。
    // 読み終えていれば、この変換から AI を使う (先に変換器に渡す)
    installPreloadedZenzIfReady()
    // 2026-11-19 (軽微 4): 読めなかったあとも、読んでいる間と同じく辞書だけ (休みの印は立てない。かかった時間も数えない)
    let warming = !dictionaryOnly && (zenzWarming() || zenzPreloadFailed)
    aiPausedForRequest = ai.paused || warming
    defer { aiPausedForRequest = false }
    let dictionaryOnly = dictionaryOnly || ai.paused || warming
    let fast = ai.paused && aiPauseOrderPreview() ? true : fast
    let aiStarted = Date()
    // 2026-11-02: 難読語の同音語 (当局の商用 → 当局の慫慂) は、仮表示・清書・Space のどれでも返す前に替える (homophones.swift)。
    // AI の変換では、利用者が選んだ難読語の一覧の語 (蹉跌) も、zenz の点の差が小さければ戻す (homophone_words.swift)
    // 2026-11-04: 登録した語は替えない (直す前の文にあった登録した語が消えるなら、直す前のまま。user_dictionary.swift)
    // 2026-11-23 (a/symseg): 前後と一緒に変換した記号 (読点・括弧、AI ありの道では 。．！？・ も) を記号だけの文節に分け直し、打った字に戻す (literal_symbols.swift)
    let converted = splittingJoinedSymbols(conversionSegments(hiragana: hiragana, context: context, fast: fast, dictionaryOnly: dictionaryOnly), reading: hiragana)
    var segments = keepingUserRegisteredText(converted, replacingHomophones(converted, reading: hiragana, context: context, ai: !dictionaryOnly, fast: fast))
    // 2026-11-06: 。。。・・・・... の文節を … に (設定 input.ellipsis_auto がオンのときだけ。ellipsis.swift)
    segments = replacingEllipsis(segments, enabled: ellipsisAutoEnabled())
    // 2026-11-05 (仕事 22 の 2): 数字/数字 のあとの ・ の形の語 (1・2年生) と、かなのあとの 。＋拡張子 (資料.pdf) を決め直す。
    // 打っている途中の仮表示 (速い・辞書だけ) では決め直さない (slash_dot.swift)
    if devEnvironment["AZK_SLASH_DOT"] != "0" {
        let beforeSlashDot = segments
        // 2026-11-05 (まとめ役 14:51): 数学の文 (よく打つ分野に数学か物理があり、目印の語か式の記号がある) では英字 1 字・数字・閉じ括弧のあとを /
        let math = isMathSentence(context: context, text: segments.map(\.word).joined(), signals: mathSentenceSignals)
        // 仕事 28 の 1: いつも ・ か / (slash_key)・いつも 。 か . (period_key) のときは、その記号を決め直さない
        segments = decidingSlashDot(segments, reading: hiragana, context: context, decide: !(fast && dictionaryOnly),
                                    caseFix: englishCaseFixEnabled(), partOfSpeech: slashDotPartOfSpeech, math: math,
                                    slashes: slashKeyAuto(), periods: periodKeyAuto())
        // 2026-11-05 (仕事 22 の 3): 同じ組で前に選び直した形 (賛成/反対・資料。PDF) をかける。打っている途中の仮表示ではかけない。数学の文は別に覚えた形
        if !(fast && dictionaryOnly) {
            segments = slashDotLearnedApplied(segments, reading: hiragana, math: math)
        }
        // 2026-11-05 (まとめ役、単語登録 12e1038 と仕事 22 を合わせたとき): 登録した語は ・ / 。 . の決め直しと選び直しの学習でも替えない
        segments = keepingUserRegisteredText(beforeSlashDot, segments)
    }
    // 2026-11-06 (仕事 34 の 4): 式だけの読み (1+2=) は 1 つの文節に (一覧に答えを出すため。設定 input.calc_candidates。calc.swift)
    segments = mergingCalcSegments(segments, reading: hiragana, enabled: calcCandidatesEnabled())
    if !dictionaryOnly && aiSettingEnabled() {
        endAIConversion(ms: -aiStarted.timeIntervalSinceNow * 1000, chars: hiragana.count, refine: ai.refine, probe: ai.probe)
    }
    let result = segments.map { segment in
        FFICandidate(text: strdup(segment.word), subtext: strdup(""), hiragana: strdup(""), correspondingCount: Int32(segment.rubyCount))
    }
    lengthPtr.pointee = Int32(result.count)
    return to_list_pointer(result)
}

/// 2026-10-15: ライブ変換の仮表示 (辞書だけ・fast) の結果を覚えておく。読み・前の文脈・表記の設定が同じなら、同じ結果を返す。
/// 「、」「。」で区切った前の部分は打鍵のたびに同じ読みで呼ばれる (preservingLiteralSymbols) ので、変換し直さずに返す。
/// 学習・設定・表記の切り替えで結果が変わるので、そのときは捨てる (clearPreviewMemo)
@MainActor var previewMemo: [String: [(word: String, rubyCount: Int)]] = [:]
@MainActor var previewMemoOrder: [String] = []
let previewMemoLimit = 64

@MainActor func clearPreviewMemo() {
    // 2026-11-08 (仕事 49): 左の語の見積もりも捨てる (学習・設定で候補が変わるため。left_pos.swift)
    leftTailRcidMemo.removeAll(keepingCapacity: true)
    leftWordDecisionMemo.removeAll(keepingCapacity: true)
    previewMemo.removeAll(keepingCapacity: true)
    previewMemoOrder.removeAll(keepingCapacity: true)
    scopeLookupMemo.removeAll(keepingCapacity: true)
}

/// 2026-10-15: 文節の区切りを確かめるための辞書の引き直し (読み・表記を指定して、その表記になる辞書の語の並びを探す) を覚えておく。
/// 打鍵のたびに前の文節は同じ読み・同じ表記で引き直されるので、2 回目からは引かずに返す。学習・設定で変わるので clearPreviewMemo で捨てる
@MainActor var scopeLookupMemo: [String: Candidate?] = [:]
let scopeLookupMemoLimit = 4096

@MainActor func scopeLookup(reading: String, text: String, context: String) -> Candidate? {
    let key = "\(plainNotation ? 1 : 0)\u{1}\(context)\u{1}\(reading)\u{1}\(text)"
    if let hit = scopeLookupMemo[key] {
        return hit
    }
    let found = converter.dictionaryCandidate(directComposingText(reading), matching: text,
        options: getOptions(context: context, prediction: false, zenzai: false, fast: true))
    if scopeLookupMemo.count >= scopeLookupMemoLimit { scopeLookupMemo.removeAll(keepingCapacity: true) }
    scopeLookupMemo[key] = found
    return found
}

/// 2026-11-20: 連語の助詞の切り方 (開発用のビルドの AZK_CLAUSE_COMPOUND で測るため。配る版はいつも before = 前で切るだけ)
@MainActor func compoundParticleSplitMode() -> CompoundParticleSplit {
    switch devEnvironment["AZK_CLAUSE_COMPOUND"] {
    case "0": return .off
    case "noverb": return .nonVerbTail
    case "word": return .wordTail
    default: return .before
    }
}

/// 2026-11-08 (仕事 54): AI の変換を使う設定か (zenzai.enable)。切っていれば、Space・清書も辞書だけの変換と同じ直しをかける
@MainActor func aiSettingEnabled() -> Bool {
    if let knob = devEnvironment["AZK_AI_FIXES_AS_ON"] { return knob == "1" }
    return (config["enable"] as? Bool) == true
}

/// この問い合わせで AI を使うか: AI の設定がオンで、重いので休んでいない (仕事 52。ai_load_guard.swift)
@MainActor func aiConversionEnabled() -> Bool {
    aiSettingEnabled() && !aiPausedForRequest && !zenzWarming() && !zenzPreloadFailed
}

/// 2026-11-23: 変換の口 (conversionSegmentsUncached) の呼び返しの守り。記号で読みを分ける所・問いの か・読みの頭で分ける所・化学式の所は、
/// 読みを部分に分けて変換の口を呼び返す。部分が読み全体と同じになると同じ読みで呼び返して終わらず、スタックがあふれて変換サーバーが落ちた
/// (11-23 a/nosplit の 1 つ目・2 つ目で 2 回。どちらも分け方の誤り)。どんな入力でも落ちないよう、同じ読み・同じ形 (問いの か の段・辞書だけか・仮表示か) で
/// 入り直したときと、深さが conversionDepthLimit に達したときは、読み全体の 1 番の候補をそのまま返す (plainConversionSegments)。ふつうの変換では起きない
@MainActor var conversionsInProgress: Set<String> = []
@MainActor var conversionDepth = 0
let conversionDepthLimit = 24

/// 呼び返しの守りのときの変換: 読み全体を azooKey にそのまま渡し、読み全体の 1 番の候補を、その文節の区切りで返す (分ける所・直しを通さない)
@MainActor func plainConversionSegments(hiragana: String, context: String, fast: Bool, dictionaryOnly: Bool) -> [(word: String, rubyCount: Int)] {
    let composing = directComposingText(hiragana)
    let count = composing.input.count
    converter.collectFusionPool = false
    let converted = converter.requestCandidates(composing, options: getOptions(context: context, prediction: false, zenzai: !dictionaryOnly, fast: fast))
    guard let best = converted.mainResults.first(where: { $0.correspondingCount == count }) else {
        return [(word: hiragana, rubyCount: count)]
    }
    let split = best.clauseSplit()
    if !split.isEmpty && split.map({ $0.word }).joined() == best.text && split.reduce(0, { $0 + $1.rubyCount }) == count {
        return split
    }
    return [(word: best.text, rubyCount: count)]
}

/// 辞書だけ向けの直しをかけるか: 辞書だけの変換か、AI を切った設定
func usesDictionaryFixes(dictionaryOnly: Bool, aiEnabled: Bool) -> Bool {
    dictionaryOnly || !aiEnabled
}

@MainActor func conversionSegments(hiragana: String, context: String, fast: Bool, dictionaryOnly: Bool) -> [(word: String, rubyCount: Int)] {
    guard fast && dictionaryOnly else {
        return conversionSegmentsUncached(hiragana: hiragana, context: context, fast: fast, dictionaryOnly: dictionaryOnly)
    }
    // 2026-11-19: 部分のすぐ後ろの字も鍵に入れる (終助詞の直しが、文の終わりの記号の前だけでかかるので)
    let key = "\(plainNotation ? 1 : 0)\u{1}\(context)\u{1}\(hiragana)\u{1}\(rightContextEnabled() ? rightContextText : "")\u{1}\(conversionPartFollower.map(String.init) ?? "")"
    if let hit = previewMemo[key] {
        return hit
    }
    let segments = conversionSegmentsUncached(hiragana: hiragana, context: context, fast: fast, dictionaryOnly: dictionaryOnly)
    if previewMemoOrder.count >= previewMemoLimit {
        previewMemo.removeValue(forKey: previewMemoOrder.removeFirst())
    }
    previewMemo[key] = segments
    previewMemoOrder.append(key)
    return segments
}

/// 2026-11-23: 読点と括弧 (joinableLiteralSymbols) でも読みを分けるか (前の形)。既定は分けない。開発用のビルドの AZK_LITERAL_SPLIT=all で分ける
let literalSplitOrdinary = devEnvironment["AZK_LITERAL_SPLIT"] == "all"

/// 2026-11-23 (2 つ目): AI ありの道では、文の終わりの記号 (。．！？) と ・ でも読みを分けない。文の終わりの 。 まで一緒に渡すと AI がよく当たり
/// (dev 一般 1,000 で、読点と括弧だけ分けない形 702 → どの記号でも分けない形 714 の差の 24 文は、どれも文の終わりの 。 で分けていた)、
/// ・ の左のカタカナの名前 (アラン・ケイ・ビル・ジョイ) がひとりで変換されてかなに残らない。
/// 辞書だけの道 (仮表示・AI を切った設定) は分けたまま: 文の終わりの終助詞の直し (部分のすぐ後ろの字を見る) と、・ の前の名前 (やまだ・たろう) を守る。
/// 問いの か を ？ で切って比べる側 (.split) では ？ で分ける。開発用のビルドの AZK_LITERAL_JOIN_EXTRA=0 で、AI ありの道も読点と括弧だけ
/// 2026-11-23 (a/dotext): AI を切った設定・AI の休み・起動直後の先読みの間・読めなかったとき (aiConversionEnabled が偽) は、Space・清書で
/// dictionaryOnly が偽でも辞書だけで変換するので、辞書だけの道と同じに分ける (つなげると、部分のすぐ後ろの字を見る文末の終助詞の直し・
/// 。だけの文節を見る 。 + 拡張子 の決め直しが効かなかった)
@MainActor func literalJoinExtra(dictionaryOnly: Bool, aiEnabled: Bool = true, questionKa: QuestionKaMode, long: LongReadingSplit = .none) -> Set<Character> {
    guard !usesDictionaryFixes(dictionaryOnly: dictionaryOnly, aiEnabled: aiEnabled), devEnvironment["AZK_LITERAL_JOIN_EXTRA"] != "0" else { return [] }
    var extra: Set<Character> = ["・"]
    // 長い読みで 。 で分ける (longReadingSplit) ときは、文の終わりの記号を足さない
    if long == .none || long == .comma {
        extra.formUnion(["。", "．", "！", "!"])
        if questionKa != .split { extra.formUnion(["？", "?"]) }
    }
    return extra
}

/// 2026-11-23 (調べ用、開発用のビルドだけ): 読みが長いときに分ける形。分けない形は読み全体を 1 回で AI に渡すので、長い文の 1 回の時間が延びた
/// (長い文 平均 58 字の p50: 今の形 516 ms → 読点と括弧だけ分けない 775 ms)。正しさと速さの釣り合いを決めるため。
/// AZK_SPLIT_LONG=<字数> で、読みがその字数以上なら、AZK_SPLIT_LONG_MODE=period (既定) は 文の終わりの記号 (。．！？) で分け、
/// all は前の形 (どの記号でも分ける)。字数は部分ごとではなく、変換の口に来た読みの字数で見る
/// 2026-11-24 (a/longcomma): 既定は comma (読みが 50 字以上なら、AI ありの道で 、 (，) のすぐ後ろで読みを分け、左の部分の出力を右の部分の左の文脈に渡す。
/// 括弧と 。 は今までどおりつなげる)。文全体を 1 回で zenz に渡すと、長い文で 1 回の推論が延び (zenzai 1 文 204 → 572 ms)、0366345 より長い文の p50 が 22% 遅かった。
/// dev 一般 1,000 (読点ごとに確定をまねた測り t121): 読みが 50 字以上で読点を含む 272 文は 厳しい 176 → 175、p50 604 → 262 ms。短い文は分けると正しさが落ちるので分けない。
/// 辞書だけの道 (仮表示・AI を切った設定) は分けない (速いので要らない。分けると呼ぶ回数の分だけ遅く、正しさも少し落ちた)。
/// 開発用のビルドでは AZK_SPLIT_LONG=0 で分けない (11-23 の形)、AZK_SPLIT_LONG=<字数>、AZK_SPLIT_LONG_MODE=period|all|comma で替えられる
enum LongReadingSplit { case none, period, all, comma }
let splitLongThreshold = Int(devEnvironment["AZK_SPLIT_LONG"] ?? "") ?? 50
let splitLongMode: LongReadingSplit = {
    switch devEnvironment["AZK_SPLIT_LONG_MODE"] {
    case "all": return .all
    case "period": return .period
    default: return .comma
    }
}()
/// aiPath: AI ありで変換する道か (comma は AI ありの道だけ)
func longReadingSplit(_ reading: String, aiPath: Bool = true) -> LongReadingSplit {
    guard splitLongThreshold > 0 && reading.count >= splitLongThreshold else { return .none }
    return splitLongMode == .comma && !aiPath ? .none : splitLongMode
}

@MainActor func conversionSegmentsUncached(hiragana: String, context: String, fast: Bool, dictionaryOnly: Bool,
                                            questionKa: QuestionKaMode = .decide) -> [(word: String, rubyCount: Int)] {
    // 2026-11-23: 呼び返しの守り (conversionGuard)。同じ読み・同じ形で入り直したか、深さが上限を超えたら、分ける所・直しを通さずに返す
    let guardKey = "\(questionKa)\u{1F}\(dictionaryOnly)\u{1F}\(fast)\u{1F}\(hiragana)"
    if conversionsInProgress.contains(guardKey) || conversionDepth >= conversionDepthLimit {
        debugWrite("conversion guard: depth \(conversionDepth) reentered \(conversionsInProgress.contains(guardKey)) \(hiragana)\n".data(using: .utf8)!)
        return plainConversionSegments(hiragana: hiragana, context: context, fast: fast, dictionaryOnly: dictionaryOnly)
    }
    conversionsInProgress.insert(guardKey)
    conversionDepth += 1
    defer {
        conversionsInProgress.remove(guardKey)
        conversionDepth -= 1
    }
    // 2026-11-06 (問いの か): 「…か？」は ？ で切った変換と ？ ごとの変換を比べる (question_ka.swift)
    if questionKa == .decide && questionKaEnabled, let marks = questionKaMarks(hiragana) {
        let split = conversionSegmentsUncached(hiragana: hiragana, context: context, fast: fast, dictionaryOnly: dictionaryOnly, questionKa: .split)
        let whole = conversionSegmentsUncached(hiragana: hiragana, context: context, fast: fast, dictionaryOnly: dictionaryOnly, questionKa: .whole)
        let chosen = choosingQuestionKaNoting(split: split, whole: whole, reading: hiragana, marks: marks) { reading, text in
            let composing = directComposingText(reading)
            let count = composing.input.count
            return converter.requestCandidates(composing, options: getOptions(context: "", prediction: false, zenzai: false, nBest: 50)).mainResults
                .contains { $0.correspondingCount == count && $0.data.count == 1 && $0.text == text }
        }
        noteQuestionKaOriginal(chosen.original)
        return chosen.segments
    }
    if let request = nominalRequest(reading: hiragana) {
        let prefix = request.prefix.isEmpty ? [] : conversionSegments(hiragana: request.prefix,
            context: context, fast: fast, dictionaryOnly: dictionaryOnly)
        return prefix + request.segments
    }
    if let prefix = japaneseVocabulary.prefixOverride(for: hiragana) {
        let tail = String(hiragana.dropFirst(prefix.count))
        return prefix.segments + (tail.isEmpty ? [] : conversionSegments(hiragana: tail,
            context: context + prefix.segments.map(\.word).joined(), fast: fast, dictionaryOnly: dictionaryOnly))
    }
    let protected = chemEnabled() ? chemicalLiteralSpans(hiragana) : []
    if !protected.isEmpty {
        let chars = Array(hiragana)
        var segments: [(word: String, rubyCount: Int)] = []
        var offset = 0
        var left = context
        for span in protected {
            if offset < span.range.lowerBound {
                let part = conversionSegments(hiragana: String(chars[offset..<span.range.lowerBound]), context: left, fast: fast, dictionaryOnly: dictionaryOnly)
                segments += part
                left += part.map { $0.word }.joined()
            }
            segments.append((word: span.text, rubyCount: span.range.count))
            left += span.text
            offset = span.range.upperBound
        }
        if offset < chars.count {
            segments += conversionSegments(hiragana: String(chars[offset...]), context: left, fast: fast, dictionaryOnly: dictionaryOnly)
        }
        return segments
    }
    // 2026-11-05 (仕事 22 の 2、まとめ役 15:26): 数字＋/＋数字 の後ろの部分は、左の文脈に ・ を渡して変換する (/ の打ち分けが auto のとき。
    // 試験のビルドでは AZK_SLASH_CONTEXT=0 で前の形)。出す字は / のまま
    let digitSlashContext = slashKeyAuto() && devEnvironment["AZK_SLASH_CONTEXT"] != "0"
    // 2026-11-06 (問いの か): whole のときは ？ を切らない。ふだん (decide) は、文の途中の「…か？」の部分を ？ ごと渡す
    // 2026-11-19: 部分を変換する間は、そのすぐ後ろの字を conversionPartFollower に置く (読みの終わりの部分は、外側の後ろの字のまま)。済んだら戻す
    let outerFollower = conversionPartFollower
    defer { conversionPartFollower = outerFollower }
    // 2026-11-23: 読点と括弧では読みを分けない (literalSplitOrdinary が偽、既定)。開発用のビルドの AZK_LITERAL_SPLIT=all で前の形 (どの記号でも分ける)
    // 2026-11-24 (a/longcomma): 長い読み (50 字以上) は、AI ありの道で 、 で分ける (longReadingSplit)
    let longSplit = longReadingSplit(hiragana, aiPath: !usesDictionaryFixes(dictionaryOnly: dictionaryOnly, aiEnabled: aiConversionEnabled()))
    if questionKa != .whole, let literal = preservingLiteralSymbols(reading: hiragana, context: context, digitSlashContext: digitSlashContext,
        attachQuestionKa: questionKa == .decide && questionKaEnabled,
        splitOrdinarySymbols: literalSplitOrdinary || (longSplit == .all),
        extraJoinable: literalJoinExtra(dictionaryOnly: dictionaryOnly, aiEnabled: aiConversionEnabled(), questionKa: questionKa, long: longSplit),
        splitCommas: longSplit == .comma, convert: { reading, left in
        conversionSegments(hiragana: reading, context: left, fast: fast, dictionaryOnly: dictionaryOnly)
    }, nameBeforeDot: { reading, chosen, unknown, right in
        katakanaNameBeforeNakaguro(reading: reading, chosen: chosen, unknown: unknown, right: right)
    }, partFollower: { conversionPartFollower = $0 ?? outerFollower }) { return literal }
    let composing = directComposingText(hiragana)
    let count = composing.input.count
    // 2026-11-06: 読み全体がくだけた一言 (だる・りょ・わろた) なら、表のよい形の 1 番目 (casual.swift)
    if let forms = casualWordForms(hiragana) {
        return [(word: forms[0], rubyCount: count)]
    }
    converter.collectFusionPool = fusion.enabled && !dictionaryOnly
    // 2026-11-08 (仕事 54): 辞書だけ向けの直し (外来語の崩れのカタカナ・数字の後ろの助数詞・colloc の 1 字の語と外側の文脈) は、
    // 辞書だけの変換 (ライブ変換の仮表示) に加えて、AI を切った設定の Space・清書でもかける (IME 本体は AI の設定を知らないので、
    // AI を切っていても Space は dictionaryOnly=false で届き、今まではこの直しが抜けていた)
    let dictionaryFixes = usesDictionaryFixes(dictionaryOnly: dictionaryOnly, aiEnabled: aiConversionEnabled())
    // 2026-11-23: 文の途中に差し込んだ短い読みは、左の終わりのかなとつなげて引き、切れ目をまたぐ語なら読みの所をかなのまま (insertion_join.swift)
    if dictionaryFixes, let inserted = insertedKanaSegments(hiragana: hiragana, context: context, fast: fast) {
        converter.collectFusionPool = false
        return inserted
    }
    let tConv = Date()
    defer { if fusionProf { FileHandle.standardError.write("FUSIONPROF convertAll=\(Int(-tConv.timeIntervalSinceNow * 1000))\n".data(using: .utf8)!) } }
    // 2026-11-08 (仕事 49): 辞書だけの変換 (打っている途中の仮表示・AI を切った設定・AI を休んでいる間) は、左の文脈の終わりの語の品詞からつなぐ
    // (格子の始点を文頭から左の語に替える。文の頭に来やすい語で始まるときは文頭から変換し直す。left_pos.swift)
    let converted = dictionaryFixes
        ? requestCandidatesFromLeftWord(composing, count: count, context: context) { getOptions(context: context, prediction: false, zenzai: !dictionaryOnly, fast: fast) }
        : converter.requestCandidates(composing, options: getOptions(context: context, prediction: false, zenzai: !dictionaryOnly, fast: fast))
    converter.collectFusionPool = false
    if fusionProf { FileHandle.standardError.write("FUSIONPROF zenzai=\(Int(-tConv.timeIntervalSinceNow * 1000)) pool=\(converter.lastZenzaiPool.count)\n".data(using: .utf8)!) }

    var segments: [(word: String, rubyCount: Int)] = [(word: hiragana, rubyCount: count)]
    var chosen = converted.mainResults.first(where: { $0.correspondingCount == count })
    // Zenzai は 1 トークンずつ貪欲に直すので、モデルが知らない語 (跳梁跋扈) を漢字 1 字の切れ端に崩すことがある (張遼/跋/孤)。
    // 切れ端が 2 つ以上あるときだけ、辞書だけの変換の上位候補も zenz に文全体で採点させ、切れ端 1 つにつき少し減点して比べ直す
    // 漢数字 (三線 の 三、二礼 の 二) は 1 字でふつうなので数えない
    func isFragmentWord(_ w: String) -> Bool {
        w.count == 1 && !"〇一二三四五六七八九十百千万億".contains(w) && w.unicodeScalars.allSatisfy { (0x4E00...0x9FFF).contains($0.value) }
    }
    func fragments(_ c: Candidate) -> Int {
        c.data.filter { isFragmentWord($0.word) }.count
    }
    if !dictionaryOnly && fusion.enabled, let zenzBest = chosen {
        chosen = fusionChoose(hiragana: hiragana, context: context, count: count, zenzBest: zenzBest, fast: fast, isFragmentWord: isFragmentWord)
    } else if !dictionaryOnly, let zenzBest = chosen, fragments(zenzBest) >= 2 {
        let dictOptions = getOptions(context: context, prediction: false, zenzai: false, fast: fast)
        let zenzOptions = getOptions(context: context, prediction: false, zenzai: true, fast: fast)
        var seen: Set<String> = [zenzBest.text]
        var pool: [Candidate] = [zenzBest]
        // 辞書だけの 1 位は、区切り方を借りるだけで候補には入れない (入れると 三線→参戦 のように辞書が勝ってしまう)
        var bases: [Candidate] = [zenzBest]
        if let c = converter.requestCandidates(composing, options: dictOptions).mainResults.first(where: { $0.correspondingCount == count }),
           c.text != zenzBest.text {
            bases.append(c)
        }
        // 切れ端とその前後 1 語までの読みをまとめて辞書で引き、1 語で書ける熟語があれば差し替えた候補を作る (張遼/跋/孤 → 跳梁跋扈)
        func isFragment(_ e: DicdataElement) -> Bool { isFragmentWord(e.word) }
        // 区切り方は候補ごとに違う (超良/抜/擦る と 張遼/跋/孤/する) ので、上位 4 つの区切りでそれぞれ探す
        let kanjiOnly: (String) -> Bool = { $0.unicodeScalars.allSatisfy { (0x4E00...0x9FFF).contains($0.value) || $0.value == 0x3005 } }
        for base in bases where pool.count < 24 {
            let elems = base.data
            var spans: Set<[Int]> = []
            for f in elems.indices where isFragment(elems[f]) {
                for lo in max(0, f - 2)...f {
                    for hi in f..<min(elems.count, f + 3) where hi > lo {
                        spans.insert([lo, hi])
                    }
                }
            }
            for span in spans.sorted(by: { ($0[1] - $0[0], $0[0]) > ($1[1] - $1[0], $1[0]) }) where pool.count < 24 {
                let (lo, hi) = (span[0], span[1])
                let reading = elems[lo...hi].map { $0.ruby }.joined().toHiragana()
                let sub = directComposingText(reading)
                let subCount = sub.input.count
                for word in converter.requestCandidates(sub, options: dictOptions).mainResults
                    where word.correspondingCount == subCount && word.data.count == 1 && word.text.count >= 2 && kanjiOnly(word.text) && !hasNGWord(word.text) {
                    let data = Array(elems[..<lo]) + word.data + Array(elems[(hi + 1)...])
                    let text = data.map { $0.word }.joined()
                    if seen.contains(text) { break }
                    seen.insert(text)
                    pool.append(Candidate(text: text, value: base.value, correspondingCount: count, lastMid: base.lastMid, data: data))
                    break
                }
            }
        }
        var bestScore = -Float.infinity
        for c in pool {
            guard let lp = converter.zenzSequenceScore(hiragana: hiragana, output: c.text, leftSideContext: context, options: zenzOptions) else { continue }
            let score = lp - fragmentPenalty * Float(fragments(c)) + (c.text == zenzBest.text ? rescoreMargin : 0)
            debugWrite("rescore \(hiragana) \(c.text) lp=\(lp) score=\(score)\n".data(using: .utf8)!)
            if score > bestScore {
                bestScore = score
                chosen = c
            }
        }
    }
    // 2026-11-07 (仕事 45 の 2): カーソルの右の字があれば、AI の候補の上位を、候補のあとに右の字が続く zenz の確率で並べ直す
    // (開発用の切り替え AZK_RCTX_AI=1 のときだけ。0.1.0 では使わない: 時間が延びすぎる。読み全体の終わりの部分だけ。right_context.swift)
    // 並べ直した候補でも、今の 1 位の登録した語 (isUserRegistered) は書き換えない (keepingUserRegisteredWords。user_dictionary.swift)
    if !dictionaryOnly, rightContextEnabled(), devEnvironment["AZK_RCTX_AI"] == "1", let best = chosen,
       let better = keepingUserRegisteredWords(best, rerankingByRightContext(chosen: best, others: converted.mainResults, hiragana: hiragana, count: count,
                                                                            left: context, right: rightContextFor(hiragana), fast: fast)) {
        chosen = better
    }
    // 2026-11-04: 読み全体が登録した読みと同じなら、登録した語にする (user_dictionary.swift)
    if let word = wholeReadingUserWord(hiragana, count: count, chosen: chosen) {
        chosen = word
    }
    // 2026-11-02: AI の選んだ候補に、形容動詞の「な」のすぐ後ろの助詞・句読点 (まず高価なと思う) が残っていれば、
    // 辞書だけの変換 (この形を含まなければ) にする (まず|こう|かなと思う。adjectival_na.swift)
    if !dictionaryOnly, adjectivalNaFilter, let best = chosen, hasDanglingAdjectivalNa(best.data),
       let dictionaryBest = converter.requestCandidates(composing, options: getOptions(context: context, prediction: false, zenzai: false, fast: fast))
           .mainResults.first(where: { $0.correspondingCount == count }), !hasDanglingAdjectivalNa(dictionaryBest.data),
       let kept = keepingUserRegisteredWords(best, dictionaryBest) {
        debugWrite("adjectival na \(hiragana) \(best.text) -> \(kept.text)\n".data(using: .utf8)!)
        chosen = kept
    }
    // 2026-11-03: AI の選んだ候補で、連濁の形の読みの語が助詞のすぐ後ろに単独で来た所 (あと|は|車|は) を、助詞と合わせた 1 語 (歯車) に替える (rendaku.swift)
    // 2026-11-04: ここから下の語の単位の直しは、登録した語 (isUserRegistered) を書き換えない (keepingUserRegisteredWords。user_dictionary.swift)
    if !dictionaryOnly, rendakuFix, let best = chosen,
       let fixed = keepingUserRegisteredWords(best, replacingRendakuWords(best, hiragana: hiragana, context: context, fast: fast)) {
        debugWrite("rendaku \(hiragana) \(best.text) -> \(fixed.text)\n".data(using: .utf8)!)
        chosen = fixed
    }
    // 2026-11-04: AI の選んだ候補で、文末の 藁 を 笑・わら に直す (kandou.swift。zenz で元の結果と比べる)
    if !dictionaryOnly, kandouFix, let best = chosen,
       let fixed = keepingUserRegisteredWords(best, replacingFinalWara(best, hiragana: hiragana, context: context, fast: fast)) {
        debugWrite("wara \(hiragana) \(best.text) -> \(fixed.text)\n".data(using: .utf8)!)
        chosen = fixed
    }
    // 2026-11-23: 文の頭の 1 語 + 読点 (うん、) は、その読みの感動詞を選ぶ (AI ありでも辞書だけでも。kandou.swift)
    if let best = chosen,
       let fixed = keepingUserRegisteredWords(best, replacingInitialInterjection(best, hiragana: hiragana, context: context, follower: conversionPartFollower)) {
        debugWrite("interjection \(hiragana) \(best.text) -> \(fixed.text)\n".data(using: .utf8)!)
        chosen = fixed
    }
    if let best = chosen, let path = devEnvironment["AZK_DUMP_DATA"] {
        // 調べる用: 選んだ語の並び (表記・読み・品詞・重み) を 1 行ずつ書き出す
        let elements = best.data.map { "[\"\($0.word)\",\"\($0.ruby)\",\($0.lcid),\($0.rcid),\(String(format: "%.3f", Double($0.value())))]" }.joined(separator: ",")
        let line = "{\"reading\":\"\(hiragana)\",\"dict\":\(dictionaryOnly),\"data\":[\(elements)]}\n"
        if let handle = FileHandle(forWritingAtPath: path) ?? { FileManager.default.createFile(atPath: path, contents: nil); return FileHandle(forWritingAtPath: path) }() {
            handle.seekToEndOfFile(); handle.write(line.data(using: .utf8)!); handle.closeFile()
        }
    }
    if let best = chosen, let fixed = keepingUserRegisteredWords(best, hiraganizingKatakanaParticles(best)) {
        chosen = fixed
    }
    if let best = chosen, let fixed = keepingUserRegisteredWords(best, katakanizingPolicyLatin(best)) {
        chosen = fixed
    }
    // Zenzai はこの崩れをほぼ自分で直すので、辞書だけの変換 (ライブ変換の仮表示・AI を切った設定) のときだけ
    if dictionaryFixes, let best = chosen, let fixed = keepingUserRegisteredWords(best, katakanizingBrokenLoanwords(best)) {
        chosen = fixed
    }
    if let best = chosen, let fixed = keepingUserRegisteredWords(best, kanjiAfterKatakanaNames(best)) {
        chosen = fixed
    }
    // 2026-11-03: 表記ゆれの語を、利用者が選ぶ向きの形に (hyouki.swift。覚えた記録が無ければ替えない。仮表示・清書・Space の同じ所でかける)
    if let best = chosen, let fixed = keepingUserRegisteredWords(best, hyoukiApplied(best)) {
        chosen = fixed
    }
    // 2026-11-06 (数字＋かい): 数字のすぐ後ろは助数詞を先に (辞書だけ)、文の頭の 数字＋助数詞＋助詞だけ は使われる回数の多い助数詞に (counters.swift)
    if digitCounterEnabled {
        // 2026-11-22 (数字＋がつ): 数字のすぐ後ろのカタカナだけの助数詞 (11|ガツ) を、同じ読みのいちばん使われる助数詞の字 (月) に (辞書だけ。counters.swift)
        if dictionaryFixes, katakanaCounterFixEnabled, let best = chosen,
           let fixed = keepingUserRegisteredWords(best, replacingKatakanaCounterAfterDigit(best, topCounter: { bestSameReadingCounter($0, usage: counterCounts()) })) {
            chosen = fixed
        }
        if dictionaryFixes, let best = chosen,
           let fixed = keepingUserRegisteredWords(best, preferringCounterAfterDigit(best, results: converted.mainResults, usage: counterCounts())) {
            chosen = fixed
        }
        if let best = chosen, let fixed = keepingUserRegisteredWords(best, choosingCounterByUsage(best, context: context)) {
            chosen = fixed
        }
    }
    // 2026-11-06 (仕事 36): 漢数字を先にする設定なら、かなで打った数詞の語 (1回/イッカイ) を漢数字に (仮表示・清書・Space の同じ所。counter_numerals.swift)
    if let best = chosen, let fixed = kanjiNumeralsApplied(best) {
        chosen = fixed
    }
    // 2026-11-06 (仕事 (4) と一緒の品詞の直し): 品詞に合わない漢字 (器械な形・高尚し) を、同じ読みでつながる語に (pos_mismatch.swift)
    // 2026-11-17 (迷うね → 迷う値): 辞書だけの変換で、終止形のすぐ後ろの文の終わりの終助詞の読みを名詞にしていたら、終助詞に戻す (sentence_final.swift)
    if dictionaryFixes, let best = chosen,
       let fixed = keepingUserRegisteredWords(best, sentenceFinalParticleFixed(best, context: context,
           atSentenceEnd: finalParticleAtSentenceEnd(follower: conversionPartFollower, rightContext: rightContextEnabled() ? rightContextText : ""),
           particle: dictionaryFinalParticle)) {
        chosen = fixed
    }
    if let best = chosen, let fixed = posMismatchesFixed(best) {
        chosen = fixed
    }
    // 2026-11-06 (仕事 (4)): 前後の語との組み合わせの回数で同音の字を選び直す (よく聞く薬 → よく効く薬。colloc.swift)
    if let best = chosen, let fixed = collocChosen(best, dictionaryOnly: dictionaryFixes, left: context, right: rightContextFor(hiragana)) {
        chosen = fixed
    }
    // 2026-11-06 (仕事 10c): 数学の式の直しがオンの人だけ、数学の名詞のすぐ後ろの母音 1 字 (ベクトルあ) を変数の英字に (math_variables.swift)
    // 英字にした母音の所は、下の「ひらがなで打った文節が英字になっていたら変換し直す」の見比べで、読みの側も英字として扱う
    var latinCheckReading = Array(hiragana)
    if mathVariableVowelsEnabled && mathSentenceSignals != nil, let best = chosen, let fixed = keepingUserRegisteredWords(best, latinizingVariableVowels(best)) {
        chosen = fixed
        var at = 0
        for (old, new) in zip(best.data, fixed.data) {
            if old.word != new.word, old.ruby.count == 1, at < latinCheckReading.count, let letter = new.word.first { latinCheckReading[at] = letter }
            at += old.ruby.count
        }
    }
    #if AZK_DEV_KNOBS
    hyoukiLastElements = chosen?.data ?? []
    #endif
    if !dictionaryOnly, let best = chosen, best.data.contains(where: isPictographElement),
       let fixed = keepingUserRegisteredWords(best, replacingPictographs(best, hiragana: hiragana, context: context, fast: fast)) {
        chosen = fixed
    }
    if let best = chosen {
        let split = best.clauseSplit()
        let joined = split.map { $0.word }.joined()
        let total = split.reduce(0) { $0 + $1.rubyCount }
        if !split.isEmpty && joined == best.text && total == count {
            segments = split
        } else {
            segments = [(word: best.text, rubyCount: count)]
        }
    }

    // 打った英字 1 文字 (x線 の x) は、読み (えっくす) に置き換えて変換してみる。
    // 辞書が英字入りの形 (X線・Tシャツ・C言語) を返せば、その大文字・小文字を使う。知らなければ打ったとおり (x軸)
    do {
        let chars = Array(hiragana)
        func isLatin(_ i: Int) -> Bool { i >= 0 && i < chars.count && chars[i].isASCII && chars[i].isLetter }
        let singles = chars.indices.filter { isLatin($0) && !isLatin($0 - 1) && !isLatin($0 + 1) }
        if !singles.isEmpty {
            var alt = ""
            var origAt: [Int] = []     // alt の各文字が元の読みの何文字目に当たるか
            var insideName: [Bool] = []
            for (i, c) in chars.enumerated() {
                if singles.contains(i), let name = letterNames[Character(c.lowercased())] {
                    for (k, n) in name.enumerated() {
                        alt.append(n)
                        origAt.append(i)
                        insideName.append(k > 0)
                    }
                } else {
                    alt.append(c)
                    origAt.append(i)
                    insideName.append(false)
                }
            }
            let altComposing = directComposingText(alt)
            let altCount = altComposing.input.count
            let altConverted = converter.requestCandidates(altComposing, options: getOptions(context: context, prediction: false, zenzai: !dictionaryOnly, fast: fast))
            func latin(_ s: String) -> String { String(s.filter { $0.isASCII && $0.isLetter }).lowercased() }
            if let altBest = altConverted.mainResults.first(where: { $0.correspondingCount == altCount }),
               latin(altBest.text) == latin(hiragana) {
                let split = altBest.clauseSplit()
                if split.map({ $0.word }).joined() == altBest.text && split.reduce(0, { $0 + $1.rubyCount }) == altCount {
                    var mapped: [(word: String, rubyCount: Int)] = []
                    var altOffset = 0
                    var ok = true
                    for segment in split {
                        let start = altOffset
                        let end = altOffset + segment.rubyCount
                        if (end < altCount && insideName[end]) { ok = false; break }   // 名前の途中で切れている
                        let origStart = origAt[start]
                        let origEnd = end < altCount ? origAt[end] : chars.count
                        mapped.append((word: segment.word, rubyCount: origEnd - origStart))
                        altOffset = end
                    }
                    if ok { segments = mapped }
                }
            }
        }
    }

    // 文節ごとの手直し:
    //  ・「チェこ」のような半端な変換は、読み全体をカタカナにする (チェコ)
    //  ・ひらがなで打った文節が英語 (ぷっしゅ → push) になっていたら、その文節だけ変換し直す
    var offset = 0
    var leftText = ""
    let readingChars = Array(hiragana)
    // 格付け表で「次の候補を使う」英字の語 (あい の AI・えぇ の A) が入っている文節も、変換し直す
    var nextLatinRanges: [Range<Int>] = []
    var pictographRanges: [Range<Int>] = []
    // 2026-11-04: 登録した語のある文節は、文節の手直し・NG の語・ギリシャ文字・化学式の下付きをかけない (書いたとおりに出す)
    var registeredRanges: [Range<Int>] = []
    var suppressedRanges: [Range<Int>] = []
    if let best = chosen, segments.map({ $0.word }).joined() == best.text {
        var at = 0
        for (i, element) in best.data.enumerated() {
            let n = element.ruby.count
            if latinKanaRule(word: element.word, ruby: element.ruby)?.convert == "next" { nextLatinRanges.append(at..<(at + n)) }
            if isSuppressedReading(best.data, at: i) { suppressedRanges.append(at..<(at + n)) }
            if isPictographElement(element) { pictographRanges.append(at..<(at + n)) }
            if element.metadata.contains(.isUserRegistered) { registeredRanges.append(at..<(at + n)) }
            at += n
        }
    }
    var registeredSegments = Set<Int>()
    for k in segments.indices {
        let end = min(readingChars.count, offset + segments[k].rubyCount)
        let segmentReading = String(readingChars[offset..<end])
        if registeredRanges.contains(where: { $0.overlaps(offset..<end) }) {
            registeredSegments.insert(k)
            leftText += segments[k].word
            offset = end
            continue
        }
        let pictographSegment = pictographRanges.contains(where: { $0.overlaps(offset..<end) })
        let suppressedSegment = suppressedRanges.contains(where: { $0.overlaps(offset..<end) })
        let latinSegmentReading = offset < end && end <= latinCheckReading.count ? String(latinCheckReading[offset..<end]) : segmentReading
        if !latinMatchesReading(segments[k].word, reading: latinSegmentReading) || nextLatinRanges.contains(where: { $0.overlaps(offset..<end) }) || pictographSegment
            || suppressedSegment {
            let sub = directComposingText(segmentReading)
            let subCount = sub.input.count
            let options = getOptions(context: context + leftText, prediction: false, zenzai: !dictionaryOnly, fast: fast)
            // 上位から順に、そのまま使える候補か、英字の語だけを読みのカタカナに戻した形 (iPadで → アイパッドで) を使う。
            // Zenzai の候補に無ければ辞書だけの候補からも探し (ございます😭 → ございます泣き顔)、どちらにも無いときだけ読み全体をカタカナにする
            // (2026-10-15: 前は助詞までカタカナになっていた アイパッドデ)
            // 絵文字の語 (ございます😭) を直す長い文節では、読み全体のカタカナ (ゴザイマスナキガオ) を最後の手 (下の ??) に回す。
            // 短い語 (タイ) はカタカナが正しいことが多いのでそのまま
            let wholeKatakana = hiraganaToKatakana(segmentReading)
            let skipWholeKatakana = pictographSegment && segmentReading.count >= 6
            // 崩れた読みの語 (決して/ケシテ) を避けたときは、読みそのままのひらがな (けして) より漢字の候補 (消して) を先に
            let skipWholeHiragana = suppressedSegment
            func usable(_ alternatives: [Candidate]) -> String? {
                alternatives.lazy.filter { $0.correspondingCount == subCount && !(skipWholeKatakana && $0.text == wholeKatakana)
                    && !(skipWholeHiragana && ($0.text == segmentReading || $0.text == wholeKatakana)) }.compactMap { alternative -> String? in
                    if latinMatchesReading(alternative.text, reading: segmentReading) && !isPartialKatakana(alternative.text, reading: segmentReading)
                        && !hasNextLatin(alternative) && !alternative.data.contains(where: isPictographElement)
                        && !hasSuppressedReading(alternative.data) {
                        return alternative.text
                    }
                    return katakanizingLatinWords(alternative, reading: segmentReading)
                }.first
            }
            // 崩れた読みの語を避けた文節は、辞書だけの順位から先に選ぶ (いつわ: Zenzai の次の候補は 五和、辞書なら 逸話)
            var replacement = suppressedSegment && !dictionaryOnly
                ? usable(converter.requestCandidates(sub, options: getOptions(context: context + leftText, prediction: false, zenzai: false, fast: fast)).mainResults)
                : nil
            if replacement == nil {
                replacement = usable(converter.requestCandidates(sub, options: options).mainResults)
            }
            if replacement == nil && !dictionaryOnly {
                replacement = usable(converter.requestCandidates(sub, options: getOptions(context: context + leftText, prediction: false, zenzai: false, fast: fast)).mainResults)
            }
            // 2026-10-20: 文脈が無く読み全体が記号の語だった (かっこ だけを打った) なら、読みのカタカナ (カッコ) より漢字の語 (括弧) を先に (replacingPictographs と同じ)
            if pictographSegment && segments.count == 1 && context.isEmpty && replacement == wholeKatakana {
                let alternatives = converter.requestCandidates(sub, options: getOptions(context: context, prediction: false, zenzai: false, fast: fast)).mainResults
                replacement = usable(alternatives.filter { $0.text.unicodeScalars.contains { (0x4E00...0x9FFF).contains($0.value) } }) ?? replacement
            }
            if devEnvironment["AZK_DEBUG"] != nil {
                debugWrite("clausefix \(segmentReading) count=\(subCount) -> \(replacement ?? "(katakana)")\n".data(using: .utf8)!)
            }
            segments[k].word = replacement ?? (suppressedSegment ? segmentReading : wholeKatakana)
        }
        segments[k].word = replacingNGWords(segments[k].word)
        if isPartialKatakana(segments[k].word, reading: segmentReading) {
            segments[k].word = hiraganaToKatakana(segmentReading)
        }
        leftText += segments[k].word
        offset = end
    }
    let typedLatin: Set<String> = Set(hiragana.split(whereSeparator: { !($0.isASCII && $0.isLetter) }).map { $0.lowercased() })
    if (config["spaceAfterLatin"] as? Bool) ?? false, !typedLatin.isEmpty {
        for k in segments.indices {
            let next = k + 1 < segments.count ? segments[k + 1].word.first : nil
            segments[k].word = insertSpaceAfterLatin(segments[k].word, next: next, typed: typedLatin)
        }
    }
    // ギリシャ文字 (アルファ → α) と化学式の下付き (H2O → H₂O)。設定で切れる
    if greekEnabled() {
        let autoUpper = greekAutoUpper()
        var previous: Character? = nil
        for k in segments.indices {
            let next = k + 1 < segments.count ? segments[k + 1].word.first : nil
            let original = segments[k].word
            if !registeredSegments.contains(k) { segments[k].word = replacingGreek(original, before: previous, after: next, upper: autoUpper) }
            previous = original.last
        }
    }
    if chemEnabled() {
        // 文節をまたぐ化学式 (H | 2O) もあるので、つなげて置き換えてから同じ文字数で切り直す (下付きにしても文字数は同じ)
        let joined = Array(replacingChemicalFormulas(segments.map { $0.word }.joined()))
        var at = 0
        for k in segments.indices {
            let n = segments[k].word.count
            if at + n <= joined.count && !registeredSegments.contains(k) {
                segments[k].word = String(joined[at..<(at + n)])
            }
            at += n
        }
    }
    // Candidate scopes are refined only after notation fixes, preserving the selected output.
    // 2026-11-20: 連語の助詞の文節の番号 (下の splitAttachedParticles で割らない。と共に → と共＋に にしない)
    var compoundKeep: Set<Int> = []
    // 2026-11-21: 名詞の後ろの助詞は前につなげ、内容語＋後ろの機能語を 1 文節にする (教室に／陥る・必要が／ある)
    // (ふつうの IME の文節)。英字の語の後ろの助詞 (githubを) は今までどおり別の文節 (IME 本体が英字を含む文節の一覧を綴りの違いから並べるため)。
    // 開発用のビルドの AZK_CLAUSE_NOUN_PARTICLE=separate で前の形 (教室／に／陥る)
    let latinOnly = devEnvironment["AZK_CLAUSE_NOUN_PARTICLE"] != "separate"
    if let best = chosen, segments.map({ $0.word }).joined() == best.text {
        // 辞書だけの変換 (ライブ変換の仮表示) の 1 位は、はじめから辞書の語の並びなので引き直さない。
        // 引き直しは読み全体の格子を毎回作り直すので、60 字で 100ms ほどかかっていた (2026-10-15)
        let aligned = dictionaryOnly ? nil
            : converter.dictionaryCandidate(composing, matching: best.text, options: getOptions(context: context, prediction: false, zenzai: false, fast: true))
        if devEnvironment["AZK_DEBUG"] != nil {
            debugWrite("scope \(hiragana) aligned=\(aligned?.data.map { "\($0.word)/\($0.ruby)/\($0.lcid)/\($0.rcid)" } ?? [])\n".data(using: .utf8)!)
        }
        var scoped = aligned ?? best
        scoped.data = splitDictionaryParticleAffixes(scoped.data, particleLedClauses: latinOnly) { reading, text in
            scopeLookup(reading: reading, text: text, context: context)
        }
        // 2026-11-20: 連語の助詞 (とともに・によって・として・につれ) の前で切る (起こること／とともに、守ろう／として。segmentation.swift)。
        // つないだ字か読みの字数が合わなければ、前の区切り (連語の助詞を切らない) に戻す
        // 2026-11-21 (案 1): 辞書の「複数の語を 1 つにした行」(ことが多い・影響を受けた) を中の語に分けてから区切る (segmentation.swift)。
        // 区切りの確かめに通らなければ、分ける前の区切りに戻す
        var fused = scoped
        if devEnvironment["AZK_CLAUSE_FUSED"] != "0" {
            fused.data = splitFusedEntries(scoped.data) { reading, text in
                scopeLookup(reading: reading, text: text, context: context)
            }
        }
        let compound = compoundParticleSplitMode()
        var withCompound = fused
        withCompound.data = splitCompoundParticles(fused.data, mode: compound) { reading, text in
            scopeLookup(reading: reading, text: text, context: context)
        }
        // 2026-11-21 (案 2): 動詞のすぐ後ろの ため・とき・結果 の前と、連体詞 (この・その) の後ろでも切る。開発用のビルドの AZK_CLAUSE_HEAD=0 で切らない
        let withKeep = compoundParticleClauses(withCompound, mode: compound, latinParticlesOnly: latinOnly, heads: devEnvironment["AZK_CLAUSE_HEAD"] != "0")
        for (split, keep) in [(withKeep.segments, withKeep.keep), (fused.clauseSplit(separateParticles: true, latinParticlesOnly: latinOnly), Set<Int>()),
                              (scoped.clauseSplit(separateParticles: true, latinParticlesOnly: latinOnly), Set<Int>())] {
            if !split.isEmpty, split.map({ $0.word }).joined() == best.text, split.reduce(0, { $0 + $1.rubyCount }) == count {
                segments = split
                compoundKeep = keep
                break
            }
        }
    }
    segments = splitAttachedParticles(reading: hiragana, segments: segments, keep: compoundKeep) { reading, text in
        if reading.unicodeScalars.allSatisfy({ $0.isASCII && CharacterSet.letters.contains($0) })
            && reading.lowercased() == text.lowercased() { return true }
        // 2026-11-21: 名詞の後ろの助詞を前につなげる形 (既定) では、英字の語の後ろの助詞だけを分ける
        guard !latinOnly else { return false }
        return scopeLookup(reading: reading, text: text, context: context)?.data.last
            .map { (1283..<1310).contains($0.rcid) } == true
    }
    // 2026-10-22: 代わりの字の成句 (勘に障る) は本来の書き方 (癇に障る) にする (variant_spellings.swift)
    let refined = preservingTypedLatinCase(reading: hiragana, segments: japaneseVocabulary.refiningScopes(reading: hiragana, segments: segments))
    return keepingUserRegisteredText(refined, replacingVariantSpellings(refined, reading: hiragana, table: variantSpellings()))
}

/// 文節の読みに対する通常の候補一覧 (「単漢字…」は含まない)
/// 2026-11-08: 候補の一覧の本体。texts と、作るのに使った候補 (AI と辞書だけの変換の結果。語の並びを見るため)
@MainActor func buildCandidatesCore(hiragana: String, context: String) -> (texts: [String], pool: [Candidate]) {
    let composing = directComposingText(hiragana)
    let count = composing.input.count
    let katakana = hiraganaToKatakana(hiragana)
    var texts: [String] = []
    let readingHasLatin = hiragana.unicodeScalars.contains { $0.isASCII && CharacterSet.letters.contains($0) }
    func add(_ text: String, dictionaryWord: Bool = false) {
        let notated = notation(text)
        if notated != text && !texts.contains(notated) {
            texts.append(notated)
        }
        if !text.isEmpty && text != hiragana && text != katakana && !texts.contains(text)
            && (dictionaryWord || !hasIsolatedKatakana(text)) && !hasNGWord(text)
            && (readingHasLatin || !isMixedLatinKana(text)) && !isPartialKatakana(text, reading: hiragana)
            && latinMatchesReading(text, reading: hiragana) {
            texts.append(text)
        }
    }
    // 0. 読み全体がくだけた一言 (だる → だる・ダル、わろた → わろた・ワロタ) なら、表のよい形を先頭に (casual.swift)
    for form in casualWordForms(hiragana) ?? [] where !texts.contains(form) {
        texts.append(form)
    }
    // 1. Zenzai が文として自然と判断した候補 (数個)
    let neural = converter.requestCandidates(composing, options: getOptions(context: context, prediction: false))
    // (Zenzai の結果の 3 番目以降は「期しゃが」のような半端な変換が多いので上位 2 つだけ)
    let neuralFull = neural.mainResults.filter { $0.correspondingCount == count }
    // (助詞から始まる崩れた形 (はしを → は詩を) は入れない)
    for candidate in neuralFull.prefix(1) where !hasSuppressedReading(candidate.data)
        && !(count > 1 && candidate.data.first.map { CIDData.isJoshi(cid: $0.lcid) } ?? false) {
        // 助詞だけカタカナのもの (フーシデ) はひらがなに戻した形を、格付け表の英字の語 (LINE) は読みのカタカナ (ライン) を先に置く
        let particleFixed = hiraganizingKatakanaParticles(candidate)
        if let fixed = particleFixed { add(fixed.text) }
        if let fixed = kanjiAfterKatakanaNames(particleFixed ?? candidate) { add(fixed.text) }
        if let kana = katakanizingPolicyLatin(particleFixed ?? candidate) {
            if kana.text == katakana {
                if !texts.contains(katakana) { texts.append(katakana) }
            } else {
                add(kana.text)
            }
        }
        // Zenzai が上位に選んだカタカナ語 (チェコ) は、カタカナ変換として末尾に回さず先頭に置く
        if particleFixed == nil && candidate.text == katakana && !texts.contains(katakana) {
            texts.append(katakana)
        }
        add(candidate.text)
        if let kana = katakanizingLatinWords(candidate, reading: hiragana) { add(kana) }
    }
    // 1.4 ギリシャ文字 (ぱい → π・Π)。紛らわしい名前もここで候補に出す
    if greekEnabled(), let letters = greekCandidateLetters(hiragana) {
        for letter in letters where !texts.contains(letter) {
            texts.append(letter)
        }
    }
    // 1.5 日付・時刻 (きょう → 2026/10/09 など。Google 日本語入力・ATOK と同じ)
    for text in dateCandidates(for: hiragana) {
        add(text)
    }
    // 2. 頭の語を入れ替えた候補 (かき→書き/描き/掻き… ＋ 残りは一番良い変換で固定)。Google 日本語入力と同じ並べ方
    let dictionary = converter.requestCandidates(composing, options: getOptions(context: context, prediction: false, zenzai: false, nBest: 50))
    // 2026-11-02: 辞書だけで読み全体を変換した最良を、AI の最良のすぐ後ろに (AI が読みをかなのまま返したときの逃げ道。あーねりかい → あーね理解。
    // 仮表示に出ていた形でもある)。AZK_DICT_BEST_IN_LIST=0 で切れる (開発用のビルドだけ)
    if devEnvironment["AZK_DICT_BEST_IN_LIST"] != "0", let dictionaryBest = dictionary.mainResults.first(where: { $0.correspondingCount == count }) {
        add(dictionaryBest.text)
    }
    var remainders: [Int: String] = [:]
    var combos: [(text: String, length: Int, value: PValue, nominal: Bool)] = []
    // 区切りは Zenzai の一番良い候補の最初の単語の切れ目 (かき|続けている、きょう|は)
    // 2026-10-15: Zenzai が漢字 1 字の切れ端 2 つ以上に崩したとき (せいふ → 正/負) と、助詞から始まるとき (はしを → は/詩/を) は、
    // 辞書だけの 1 位 (政府・橋を) の切れ目を使う。変換 (conversionSegments) では切れ端を直しているのに、
    // 一覧では 正 を入れ替えた せい不・性不… が 40 個並んでいた
    func isFragmentHead(_ w: String) -> Bool {
        w.count == 1 && !"〇一二三四五六七八九十百千万億".contains(w) && w.unicodeScalars.allSatisfy { (0x4E00...0x9FFF).contains($0.value) }
    }
    let neuralBest = neural.mainResults.first(where: { $0.correspondingCount == count })
    let brokenBest = (neuralBest?.data.filter { isFragmentHead($0.word) }.count ?? 0) >= 2
        || (neuralBest?.data.first.map { CIDData.isJoshi(cid: $0.lcid) } ?? false)
    let best = brokenBest
        ? dictionary.mainResults.first(where: { $0.correspondingCount == count }) ?? neuralBest
        : neuralBest
    if let best, let coordinated = nounCoordinationAlternative(best, lookup: { reading, text in
        converter.dictionaryCandidate(directComposingText(reading), matching: text,
            options: getOptions(context: context, prediction: false, zenzai: false, fast: true))
    }) { add(coordinated.text) }
    var headLength = best?.data.first(where: { !$0.ruby.isEmpty })?.ruby.count ?? 0
    var preferNominalHead = false
    let listParticles = ["には", "では", "とは", "での", "への", "との", "にも", "でも", "から", "まで", "より",
                         "は", "が", "を", "の", "に", "で", "と", "も", "へ", "や", "か", "ね", "よ"]
    if headLength >= count {
        // 「今日は」「自身の」のように 1 語で登録されているときは、末尾の助詞を切り離した位置で入れ替える
        if let particle = listParticles.first(where: { hiragana.hasSuffix($0) && hiragana.count > $0.count }) {
            headLength = count - particle.count
            // A katakana noun also admits noun + particle (リスト / リスと).
            // Do not let a rare verb head (利すと) outrank those noun alternatives.
            preferNominalHead = best?.data.count == 1 && best?.data[0].lcid == 1285 && best?.text == katakana
        }
    }
    if devEnvironment["AZK_DEBUG"] != nil {
        debugWrite("\(hiragana) head=\(headLength) data=\(best?.data.map { "\($0.word)/\($0.ruby)/\($0.lcid)" } ?? [])\n".data(using: .utf8)!)
    }
    // 残りが助詞・助動詞だけ (書き|ます・食べ|たい・効く|から) なら、一番良い変換のその部分をそのまま使う
    // (読みだけ変換し直すと ます が マス になり 欠きマス・下記マス が並んでいた)。それ以外 (方|出し・線|く) は変換し直す
    var bestRemainder: String? = nil
    if let best {
        var at = 0
        for (i, element) in best.data.enumerated() {
            if at == headLength {
                let rest = best.data[i...]
                if rest.allSatisfy({ (147...549).contains($0.lcid) }) { bestRemainder = rest.map { $0.word }.joined() }
                break
            }
            at += element.ruby.count
        }
    }
    // 2026-11-23: 読みの頭から末尾の助詞の前までを 1 語で覆う辞書の語 (のわき|とは の 野分、だげん|が の 打弦) があり、それが頭の語 (1 番の
    // 最初の語。の|脇|とは の の、打|現|が の 打) より長いときは、その長さでも頭を入れ替える。長い頭が先に並ぶので、短い頭の語 × 残り
    // (之脇とは・乗脇とは…、だ現が・那現が…) より前に 1 語＋助詞 が来る (前は 野分とは が一覧の 10 番目までに無かった。AI の 1 番が
    // 人名 + さん のように短い頭のときは、単漢字 40 個 × 残り が上を埋め、1 語＋助詞 は 40 番目より後ろだった)。
    // AZK_LIST_LONG_HEAD=0 で前の並べ方 (開発用のビルドだけ。測るため)
    var longHeadLength: Int? = nil
    if devEnvironment["AZK_LIST_LONG_HEAD"] != "0", headLength < count,
       let particle = listParticles.first(where: { hiragana.hasSuffix($0) && hiragana.count > $0.count }),
       count - particle.count > headLength {
        longHeadLength = count - particle.count
    }
    // 頭は辞書の 1 語だけ (「期しゃ」「か学」のような語の途中までの変換は除く)
    for head in dictionary.mainResults where (head.correspondingCount == headLength || head.correspondingCount == longHeadLength) && head.correspondingCount < count && head.data.count == 1 && head.data[0].ruby.count == head.correspondingCount && head.text == head.data[0].word && isNaturalHead(head.text) {
        let length = head.correspondingCount
        let remainder: String
        if let cached = remainders[length] {
            remainder = cached
        } else if length == headLength, let fixed = bestRemainder {
            remainder = fixed
            remainders[length] = remainder
        } else {
            let restReading = String(hiragana.dropFirst(length))
            let rest = converter.requestCandidates(directComposingText(restReading), options: getOptions(context: context + head.text, prediction: false, zenzai: false))
            remainder = rest.mainResults.first(where: { $0.correspondingCount == restReading.count })?.text ?? restReading
            remainders[length] = remainder
        }
        // 残りが漢字 1 字 (〜科・〜勢・〜伯) なら、頭を入れ替えた形は熟語の当て推量にしかならないので並べない
        // (効果 の後に こう科・高科…、脅迫 の前に 今日伯・強伯…。末尾の か を助詞とみなして切ると 科 になる)
        // (長い頭も入れ替えるので、その長さの頭だけを飛ばす。残りは長さごとに 1 つ)
        if isFragmentHead(remainder) { continue }
        combos.append((text: head.text + remainder, length: length, value: head.value,
            nominal: (1283..<1310).contains(head.data[0].lcid) && (1283..<1310).contains(head.data[0].rcid)))
        if devEnvironment["AZK_DEBUG"] != nil {
            debugWrite("  head \(head.text) data=\(head.data.map { "\($0.word)/\($0.ruby)/lcid\($0.lcid)" }) rem=\(remainder)\n".data(using: .utf8)!)
        }
    }
    combos.sort {
        if $0.length != $1.length { return $0.length > $1.length }
        if preferNominalHead && $0.nominal != $1.nominal { return $0.nominal }
        return $0.value > $1.value
    }
    // 頭の語の長さごとに上位だけを交互に混ぜず、長い (=内容語まるごと) 区切りを優先して並べる
    for combo in combos.prefix(40) {
        add(combo.text)
    }
    // 3. 辞書に 1 語として載っている候補はすべて (「き」→ 木 気 期 機 …)。単漢字は後ろにまとめる
    // 漢字・カタカナだけの語を先に、ひらがな混じりの語 (乞う・斯う) は後に (それぞれの中では辞書の順)。
    // 2026-10-18: ただし辞書に頻度の無い語 (栄・夐・迥。読み込み時の既定値 -30) は、ひらがな混じりの語よりも後ろ。
    // 前は はる で 貼る が 31 番目 (頻度の無い漢字 1 字 24 個の後ろ)、とく で 解く が 25 番目だった
    // 2026-11-10: 動詞・形容詞の自立語の基本形 (乞う・問う・書く・濃い) は、漢字だけの語と同じ前の段に入れ、辞書の順で単漢字と混ぜる。
    // 前は送り仮名があるので後ろの段に回り、こう で 乞う が単漢字 70 個の後ろ (72 番目)、とう で 問う が 44 番目だった。
    // 活用の途中の形 (来い・漕い・問い) と副詞 (斯う) と、辞書の値が -12 以下の基本形 (爲す・舊い・高価い のような旧字・異体の綴り) は後ろの段のまま。
    // 開発用の切り替え: AZK_LIST_VERB_TIER=0 で前の並べ方
    let wholeWordsAll = dictionary.mainResults.filter { $0.correspondingCount == count && $0.data.count == 1 }
    func hasHiragana(_ text: String) -> Bool {
        text.unicodeScalars.contains { (0x3041...0x3096).contains($0.value) }
    }
    func wholeWordTier(_ candidate: Candidate) -> Int {
        if candidate.data[0].value() <= -30 { return 2 }
        if listVerbTier && isDictionaryFormWord(candidate.data[0]) && candidate.data[0].value() > listVerbMinValue { return 0 }
        return hasHiragana(candidate.text) ? 1 : 0
    }
    let wholeWords = (0...2).flatMap { tier in wholeWordsAll.filter { wholeWordTier($0) == tier } }
    var singleKanji: [String] = []
    var latin: [String] = []
    var singleKanjiShown = 0
    for candidate in wholeWords where isUsableWord(candidate.text) {
        // 単漢字は通常の一覧にはよく使う順に 70 字まで (MS-IME の「き」と同程度)。残りは「単漢字…」から
        if isSingleKanji(candidate.text) {
            singleKanjiShown += 1
            if singleKanjiShown > 70 { continue }
        }
        // 単漢字も辞書の順のまま並べる (「こう」→ 高 行 公 … は普通の語として前に出す)
        if candidate.text.unicodeScalars.allSatisfy({ $0.isASCII }) {
            latin.append(candidate.text)
        } else {
            // Exact dictionary words can contain single kana abbreviations, e.g. 独ソ.
            add(candidate.text, dictionaryWord: true)
        }
    }
    // 4. 複数の語を組み合わせた残りの候補 (上位だけ)
    // 2026-11-02: 数詞＋助数詞 (二|丁) には、同じ読みのほかの助数詞に替えた形 (二挺・二梃・二張) を足す (counters.swift。値の低い 挺 は上位に入らない)
    // 辞書に 1 語で載る 一丁 (AI の 1 番目・3. の語) も同じに
    // 2026-11-03: 替えた形は、4. の組み合わせの上位のあとにまとめて足す。前は各候補のすぐ後ろに足していて、2 番目の 二章 が 4 番目に下がった
    // (にしょう の 2章 の後ろに 2賞 …)。AZK_COUNTER_PLACE=inline で前の置き方 (開発用のビルドだけ。測るため)
    let counters = dictionaryCounters(context: context)
    let countersInline = devEnvironment["AZK_COUNTER_PLACE"] == "inline"
    var counterTexts: [String] = []
    func addCounterAlternatives(_ data: [DicdataElement]) {
        for alternative in counterAlternatives(data, counters: counters) {
            if countersInline { add(alternative) } else { counterTexts.append(alternative) }
        }
    }
    for candidate in [neuralBest].compactMap({ $0 }) + wholeWords {
        addCounterAlternatives(candidate.data)
    }
    for candidate in dictionary.mainResults.filter({ $0.correspondingCount == count && $0.data.count > 1 }).prefix(10) {
        if let kana = katakanizingPolicyLatin(candidate) { add(kana.text) }
        add(candidate.text)
        addCounterAlternatives(candidate.data)
        if let kana = katakanizingLatinWords(candidate, reading: hiragana) { add(kana) }
    }
    counterTexts.forEach { add($0) }
    // 5. 単漢字 (読み全体が 1 文字の漢字になるもの)。Google 日本語入力と同じく後ろの方に
    for kanji in singleKanji {
        add(kanji)
    }
    for word in latin {
        add(word)
    }
    // An AI runner-up is not automatically better than dictionary words and head alternatives.
    for candidate in neuralFull.dropFirst().prefix(1) { add(candidate.text) }
    texts = japaneseVocabulary.insertingAlternatives(reading: hiragana, into: texts)
    texts = placingPolicyLatin(texts, reading: hiragana)
    // 2026-11-06 (仕事 32): 単位の Ω (おーむ → Ω、10おーむ → 10Ω)。ギリシャ文字ではなく単位として、ふつうの候補の後ろ (ひらがな・カタカナの前) に
    // 足す (オウム・オーム社 などを押し出さない)
    if let ohm = ohmCandidate(hiragana), !texts.contains(ohm) {
        texts.append(ohm)
    }
    // 6. ひらがな・カタカナは最後。カタカナ語なら元の英単語をその後ろに (プレイ → play / Play。MS-IME と同じ)
    // (くだけた一言で先頭に出したひらがなは重ねない)
    if !texts.contains(hiragana) {
        texts.append(hiragana)
    }
    if !texts.contains(katakana) {
        texts.append(katakana)
    }
    for word in englishWords(for: hiragana) {
        for form in [word, word.prefix(1).uppercased() + word.dropFirst()] where !texts.contains(form) {
            texts.append(form)
        }
    }
    // 2026-10-15: 崩れた読みで辞書に入っている語 (けして の 決して) は、候補の後ろへ (消さない)
    let suppressTable = readingSuppress()
    if !suppressTable.isEmpty {
        let readingKatakana = hiraganaToKatakana(hiragana)
        let suppressed = texts.filter { suppressTable[$0 + "\t" + readingKatakana] != nil }
        if !suppressed.isEmpty { texts = texts.filter { !suppressed.contains($0) } + suppressed }
    }
    // 2026-10-15: 辞書の絵文字・記号の候補 (カンボジア🇹🇭・🫶) は、ふつうの候補の後ろへ (消さない)
    // 2026-10-20: 覚えていない記号の語を含む候補 (かっこのなかを の ()の中を) も後ろへ。覚えた記号の語 (3 回選んだ かっこ → ()) は、ふつうの候補の中に残す
    if !hasPictograph(hiragana) {
        let symbols = (neural.mainResults + dictionary.mainResults).flatMap(\.data).filter(isSymbolOnlyElement)
        let learnedSymbols = Set(symbols.filter { $0.metadata.contains(.isLearned) }.map(\.word))
        let avoidedSymbols = Set(symbols.map(\.word)).subtracting(learnedSymbols)
        let demoted = { (text: String) in
            (hasPictograph(text) && !learnedSymbols.contains(text)) || avoidedSymbols.contains { text.contains($0) }
        }
        texts = texts.filter { !demoted($0) } + texts.filter(demoted)
    }
    return (texts, neural.mainResults + dictionary.mainResults)
}

/// 読み全体に対応する候補を並べて返す (絵文字は通常候補と「単漢字…」の後)
@_silgen_name("ListCandidates")
@MainActor public func list_candidates(
    hiraganaPtr: UnsafePointer<CChar>,
    contextPtr: UnsafePointer<CChar>,
    lengthPtr: UnsafeMutablePointer<Int32>
) -> UnsafeMutablePointer<UnsafeMutablePointer<FFICandidate>?> {
    refreshUserDictionaryIfChanged()
    refreshAfterLearnGuardLoaded()  // 2026-11-22: 学習の直しの表を読み終えた直後に 1 回、前の覚えを捨てる (learn_guard.swift)
    let hiragana = normalizeForConversion(String(cString: hiraganaPtr))
    let context = leftContext(String(cString: contextPtr))
    // 2026-11-08 (仕事 52): 重いので AI を休んでいる間は、候補の一覧も辞書だけ
    aiPausedForRequest = aiPausedNow()
    defer { aiPausedForRequest = false }
    // 2026-10-22: 代わりの字の成句 (論を待たない) より前に、本来の書き方 (論を俟たない) とかな書き (論をまたない) を置く (variant_spellings.swift)。
    // 音の誤りの本来の形 (てんかのほうとう → 伝家の宝刀) は、一覧に無ければ後ろに足す (misuse.swift。打った読みと違う語は先に出さない)
    var texts = appendingMisreadings(orderingVariantSpellings(buildCandidates(hiragana: hiragana, context: context), reading: hiragana, table: variantSpellings()),
                                     reading: hiragana, table: misreadingCandidates())
    // 2026-11-02: 1 番目の候補の難読語の同音語を替えたら、替えた形を 1 番に、元の形を 2 番に (変換と同じ形を先に。Space で元に戻せる。homophones.swift)
    texts = orderingHomophones(texts, reading: hiragana, context: context)
    // 2026-11-03: 表記ゆれの語は、利用者が選ぶ向きの形を同じ語のほかの形の前に (hyouki.swift)
    texts = orderingHyouki(texts, reading: hiragana)
    // 2026-11-21 (学習の効きすぎ): L2 で 1 番目に使わなくなった学習の語 (狂疾・賀) を 2 番目に (learn_guard.swift)
    // 2026-11-23 (セキュリティの点検 14): 学習を使ってはいけない接続 (隔離のアプリ) では空 (droppedLearnedForList)
    texts = promotingDroppedLearned(texts, dropped: droppedLearnedForList(hiragana: hiragana))
    // 2026-11-02: 1 番目が形容動詞の語幹＋な で終わり (まずこうかな → まず高価な)、終助詞の かな で終わる候補 (まずこうかな) があれば 2 番目に
    // (難読語の一覧の語を 2 番目に上げたあとにかけるので、両方当てはまるときは かな が 2 番目。adjectival_na.swift)
    texts = orderingFinalKana(texts, reading: hiragana, context: context)
    // 2026-11-06 (問いの か): ？ ごとの形に替えた文節 (長野か) の一覧では、もとの形 (長野家) を先頭に (IME 本体が見えている形を上に入れ直すので 2 番目になる)
    texts = placingQuestionKaOriginal(texts, reading: hiragana)
    // 2026-11-06: 。。。・・・・... の文節の一覧に … (設定がオフなら 2 番目、オンなら 1 番目。ellipsis.swift)
    texts = placingEllipsis(texts, reading: hiragana, auto: ellipsisAutoEnabled())
    // 2026-11-06 (仕事 34 の 4): 式だけの読みの一覧に「式＝答え」と「答え」(設定 input.calc_candidates。calc.swift)
    texts = placingCalcResult(texts, reading: hiragana, enabled: calcCandidatesEnabled())
    // 2026-11-06 (仕事 36): 漢数字を先にする設定なら、1 番目の 1回 を 2 番目にして 一回 を 1 番目に (counter_numerals.swift)
    texts = placingKanjiNumerals(texts, reading: hiragana, enabled: counterNumeralsKanji(), kept: counterNumeralsKept())
    // 2026-11-05 (仕事 22 の 2): 。 のあとの小文字で打った拡張子 (pdfは) の文節は、変換と同じ表の形 (PDFは) を先に (slash_dot.swift)
    if devEnvironment["AZK_SLASH_DOT"] != "0" {
        if periodKeyAuto() {
            texts = dotExtensionCandidates(texts, reading: hiragana, context: context, caseFix: englishCaseFixEnabled())
        }
        // 2026-11-05 (仕事 22 の 3): 記号 1 字の文節 (・ / ／、。 .) は、ほかの形をいつも一覧に並べる
        texts = appendingSlashDotAlternatives(texts, reading: hiragana)
    }
    // 2026-11-20: 読み間違いの本来の形 (ごちゅう → 御中) は、一覧に無ければ 4 番目に足す (1 番目には出さない。注記「もしかして: おんちゅう」は下で付ける。misuse.swift)。
    // 並べ替えをみなかけたあとに足すので、4 番目から動かない
    let misreadings = misreadingCandidates()
    texts = insertingMisreadingCorrections(texts, reading: hiragana, table: misreadings)
    // 「単漢字…」(選んで Enter すると、通常の一覧に無い単漢字の一覧に切り替わる。MS-IME と同じ)
    if !singleKanjiList(hiragana: hiragana, context: context, excluding: Set(texts)).isEmpty {
        texts.append(singleKanjiMarker)
    }
    texts = appendingEmojiCandidates(to: texts, reading: hiragana, replacer: sharedTextReplacer())
    // 2026-11-04: 表記ゆれの学習で、この一覧の 1 番目と違う形を確定したときだけ選んだ形として数える (hyouki.swift)
    noteHyoukiListed(reading: hiragana, first: texts.first)
    // 候補窓の注記 (「本来は〜」・異字同訓。subtext に「種類<TAB>注記」。出すかどうかは IME 本体の設定)
    // 注記の設定がオフなら作らない (SetCandidateNotes)
    let notes = candidateNotesWanted ? misuseNotes() : MisuseNotes()
    // 2026-11-08: 学習した語を含む候補 (Ctrl+Delete で忘れられる候補) は correspondingCount を 1 にする (一覧では長さを使わない)。
    // IME 本体が、その候補を選んでいる間だけ「Ctrl+Delete で忘れる」を出す
    let learned = learnedCandidateTexts(hiragana: hiragana)
    // 2026-11-20: 読み間違いの本来の形の注記「もしかして: 正しい読み」は、設定に関係なく付ける (IME 本体は種類 読み誤り をいつも出す)。誤用の注記より先
    let result = texts.map { text in
        let misreading = misreadingNote(for: text, reading: hiragana, table: misreadings)
        let note = !misreading.isEmpty ? misreading : candidateNotesWanted ? misuseNote(for: text, reading: hiragana, notes: notes) : ""
        return FFICandidate(text: strdup(text), subtext: strdup(note), hiragana: strdup(""), correspondingCount: learned.contains(text) ? 1 : 0)
    }
    lengthPtr.pointee = Int32(result.count)
    return to_list_pointer(result)
}

/// 2026-11-08: 読み hiragana の一覧のうち、学習した語を含む候補の表記 (ForgetLearned が忘れられるものと同じ決め: 読み全体の変換の上位 30 で、
/// 同じ表記の候補に学習の語があるもの)。個人のデータを使わない接続・学習を使わない設定では空 (学習を引かない)
@MainActor func learnedCandidateTexts(hiragana: String) -> Set<String> {
    guard !hiragana.isEmpty, personalDataAllowed, config["learning"] as? Bool ?? true else {
        return []
    }
    let composing = directComposingText(hiragana)
    let count = composing.input.count
    let converted = converter.requestCandidates(composing, options: getOptions(prediction: false, zenzai: false, nBest: 30))
    return Set(converted.mainResults.filter { candidate in
        candidate.correspondingCount == count && candidate.data.contains { $0.metadata.contains(.isLearned) }
    }.map(\.text))
}

/// カーソル位置の後ろの文字を消す (Delete キー)
@_silgen_name("DeleteForward")
@MainActor public func delete_forward(
    cursorPtr: UnsafeMutablePointer<Int32>
) -> UnsafeMutablePointer<CChar> {
    composingText.deleteForwardFromCursorPosition(count: 1)

    cursorPtr.pointee = Int32(composingText.convertTargetCursorPosition)
    return _strdup(composingText.convertTarget)!
}


/// 頭の語として不自然なもの (「期しゃ」= 漢字＋小さい仮名、「き者」「か学」= ひらがな始まりで途中から漢字) を除く
func isNaturalHead(_ word: String) -> Bool {
    func isKanji(_ c: Character) -> Bool {
        guard let v = c.unicodeScalars.first?.value else { return false }
        return (0x4E00...0x9FFF).contains(v) || (0x3400...0x4DBF).contains(v) || v == 0x3005
    }
    func isHiragana(_ c: Character) -> Bool {
        guard let v = c.unicodeScalars.first?.value else { return false }
        return (0x3041...0x3096).contains(v)
    }
    let chars = Array(word)
    let hasKanji = chars.contains(where: isKanji)
    if hasKanji && chars.contains(where: { "ゃゅょぁぃぅぇぉゎ".contains($0) }) {
        return false
    }
    if let first = chars.first, hasKanji, isHiragana(first), first != "お", first != "ご" {
        return false
    }
    return true
}

let singleKanjiMarker = "単漢字…"

/// 英字 1 文字の読み (x → えっくす)。X線・Tシャツ・C言語 を辞書に決めさせるのに使う
let letterNames: [Character: String] = [
    "a": "えー", "b": "びー", "c": "しー", "d": "でぃー", "e": "いー", "f": "えふ", "g": "じー", "h": "えいち",
    "i": "あい", "j": "じぇい", "k": "けー", "l": "える", "m": "えむ", "n": "えぬ", "o": "おー", "p": "ぴー",
    "q": "きゅー", "r": "あーる", "s": "えす", "t": "てぃー", "u": "ゆー", "v": "ぶい", "w": "だぶりゅー",
    "x": "えっくす", "y": "わい", "z": "ぜっと",
]

/// 英字のすぐ後ろに日本語 (かな・漢字) が続くところに半角スペースを入れる (Windows でコンピューター)。iOS と同じ
func insertSpaceAfterLatin(_ text: String, next: Character?, typed: Set<String>) -> String {
    func isLatin(_ c: Character) -> Bool { c.isASCII && c.isLetter }
    func isJapanese(_ c: Character) -> Bool {
        c.unicodeScalars.allSatisfy { (0x3041...0x30FF).contains($0.value) || (0x3400...0x9FFF).contains($0.value) || (0xF900...0xFAFF).contains($0.value) }
    }
    // 打った英字 (typed) の語の後ろだけ。辞書から出た X線・45km などには入れない
    var out = ""
    var run = ""
    let chars = Array(text)
    for (i, c) in chars.enumerated() {
        out.append(c)
        run = isLatin(c) ? run + String(c) : ""
        let following: Character? = i + 1 < chars.count ? chars[i + 1] : next
        if isLatin(c), let f = following, isJapanese(f), run.count >= 2, typed.contains(run.lowercased()) {
            out.append(" ")
        }
    }
    return out
}

/// 「チェこ」「スロバキあ」のように、頭だけカタカナで残りがひらがなの半端な変換か。
/// 後ろが助詞 (チェコの・ガーナは) のときは半端とみなさない
/// 読みが「した」などのありふれた語と重なり、文の途中に出ると誤変換にしかならない珍しい語 (地名の 姿陀)。
/// 変換結果に出たら読みのひらがなに戻す (見学姿陀 → 見学した)。候補一覧からは外す
let ngWords: [String: String] = ["姿陀": "した"]

func replacingNGWords(_ text: String) -> String {
    ngWords.reduce(text) { $0.replacingOccurrences(of: $1.key, with: $1.value) }
}

func hasNGWord(_ text: String) -> Bool {
    ngWords.keys.contains { text.contains($0) }
}

func isPartialKatakana(_ text: String, reading: String) -> Bool {
    let particles: Set<String> = ["は", "が", "を", "の", "に", "で", "と", "も", "へ", "や", "か", "ね", "よ",
                                  "から", "まで", "より", "には", "では", "とは", "での", "への", "との", "にも", "でも", "って"]
    func isKatakana(_ c: Character) -> Bool { c.unicodeScalars.allSatisfy { (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC } }
    func isHiragana(_ c: Character) -> Bool { c.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) || $0.value == 0x30FC } }
    let chars = Array(text)
    guard let split = chars.firstIndex(where: { !isKatakana($0) }), split >= 2 else { return false }
    let tail = chars[split...]
    guard tail.allSatisfy(isHiragana) else { return false }
    let head = String(chars[..<split])
    // 頭のカタカナが読みの頭をそのままカタカナにしたもので、後ろが 1 文字だけ、
    // しかも助詞や活用語尾 (ダウンロードした・クリックして・パンだ) でないとき (チェこ・スロバキあ・ウズベキスタん)
    guard hiraganaToKatakana(reading).hasPrefix(head), tail.count == 1 else { return false }
    let endings: Set<String> = ["だ", "し", "た", "て", "る", "い", "く", "う", "な", "さ", "わ", "ぞ", "ぜ", "ず",
                                "せ", "れ", "ろ", "め", "け", "ぬ", "じ", "ち", "つ", "ん"]
    let t = String(tail)
    if t == "ん" {
        return true   // ウズベキスタん
    }
    return !particles.contains(t) && !endings.contains(t)
}

/// 「きょう」「あした」「いま」「ことし」などを日付・時刻に変換した候補
func dateCandidates(for reading: String) -> [String] {
    let dayOffsets: [String: Int] = ["きょう": 0, "ほんじつ": 0, "あした": 1, "あす": 1, "みょうにち": 1,
                                     "あさって": 2, "きのう": -1, "さくじつ": -1, "おととい": -2, "おとつい": -2]
    let yearOffsets: [String: Int] = ["ことし": 0, "ほんねん": 0, "らいねん": 1, "きょねん": -1, "さくねん": -1]
    let monthOffsets: [String: Int] = ["こんげつ": 0, "らいげつ": 1, "せんげつ": -1]
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: "Asia/Tokyo") ?? .current
    let now = Date()
    let weekdays = ["日", "月", "火", "水", "木", "金", "土"]
    func reiwa(_ year: Int) -> String {
        let r = year - 2018
        return r == 1 ? "令和元年" : "令和\(r)年"
    }
    if let offset = dayOffsets[reading], let date = calendar.date(byAdding: .day, value: offset, to: now) {
        let c = calendar.dateComponents([.year, .month, .day, .weekday], from: date)
        guard let y = c.year, let m = c.month, let d = c.day, let w = c.weekday else { return [] }
        let mm = String(format: "%02d", m), dd = String(format: "%02d", d)
        return ["\(y)/\(mm)/\(dd)", "\(y)-\(mm)-\(dd)", "\(y)年\(m)月\(d)日", "\(reiwa(y))\(m)月\(d)日",
                "\(m)月\(d)日(\(weekdays[w - 1]))", "\(m)月\(d)日", "\(m)/\(d)", "\(weekdays[w - 1])曜日"]
    }
    if reading == "いま" || reading == "げんざい" {
        let c = calendar.dateComponents([.hour, .minute], from: now)
        guard let h = c.hour, let mi = c.minute else { return [] }
        let h12 = h % 12 == 0 ? 12 : h % 12
        return [String(format: "%d:%02d", h, mi), "\(h)時\(mi)分", "\(h < 12 ? "午前" : "午後")\(h12)時\(mi)分"]
    }
    if let offset = yearOffsets[reading], let date = calendar.date(byAdding: .year, value: offset, to: now) {
        let y = calendar.component(.year, from: date)
        return ["\(y)年", reiwa(y), "\(y)"]
    }
    if let offset = monthOffsets[reading], let date = calendar.date(byAdding: .month, value: offset, to: now) {
        let c = calendar.dateComponents([.year, .month], from: date)
        guard let y = c.year, let m = c.month else { return [] }
        return ["\(m)月", "\(y)年\(m)月", "\(reiwa(y))\(m)月"]
    }
    return []
}

/// ひらがなで打った部分が小文字の英単語 (ぷっしゅ → push, すとあ → store) に変わっていないか。
/// 大文字だけの略語 (WOWOW, X線) や 2 文字以下の単位 (km, cm) は、読みから出てきても認める。
/// ただし 2 文字以下でも、ローマ字として読むと打った読みになるもの (おん → on, いん → in, ご → go) は使わない
func latinMatchesReading(_ text: String, reading: String) -> Bool {
    func latinRuns(_ s: String) -> [String] {
        var runs: [String] = []
        var current = ""
        for c in s {
            if c.isASCII && c.isLetter {
                current.append(c)
            } else if !current.isEmpty {
                runs.append(current)
                current = ""
            }
        }
        if !current.isEmpty { runs.append(current) }
        return runs
    }
    let typed = Set(latinRuns(reading).map { $0.lowercased() })
    for run in latinRuns(text) where run.count >= 3 && run.contains(where: { $0.isLowercase }) {
        if !typed.contains(run.lowercased()) {
            return false
        }
    }
    for run in latinRuns(text) where run.count <= 2 && run.contains(where: { $0.isLowercase }) && !typed.contains(run.lowercased()) {
        var romaji = ComposingText()
        romaji.insertAtCursorPosition(run.lowercased(), inputStyle: .roman2kana)
        var kana = romaji.convertTarget
        if kana.hasSuffix("n") { kana = String(kana.dropLast()) + "ん" }
        if !kana.isEmpty && kana.unicodeScalars.allSatisfy({ (0x3041...0x309F).contains($0.value) }) && reading.contains(kana) {
            return false
        }
    }
    // かなで打った読みをローマ字にしただけの大文字 (つばき → TSUBAKI、さくら → SAKURA) も使わない。略語 (WOWOW・NHK) は残す
    for run in latinRuns(text) where run.count >= 3 && !typed.contains(run.lowercased()) {
        var romaji = ComposingText()
        romaji.insertAtCursorPosition(run.lowercased(), inputStyle: .roman2kana)
        let kana = romaji.convertTarget
        if kana.unicodeScalars.allSatisfy({ (0x3041...0x309F).contains($0.value) || $0.value == 0x30FC }) && reading.contains(kana) {
            return false
        }
    }
    return true
}

/// 英字と仮名が混ざった候補 (「pureイ」のようなローマ字の打ちかけ混じり)
func isMixedLatinKana(_ text: String) -> Bool {
    let hasLatin = text.unicodeScalars.contains { $0.isASCII && CharacterSet.letters.contains($0) }
    let hasNonLatin = text.unicodeScalars.contains { !$0.isASCII }
    return hasLatin && hasNonLatin   // 「para具合」「pureイ」のように英字と日本語が混ざったもの
}

/// カタカナ語の読み (ひらがな) → 元の英単語。english_words.tsv (JMdict + bep-eng.dic から作成) を最初に使うときに読む
@MainActor var englishWordTable: [String: [String]]? = nil

@MainActor func englishWords(for hiragana: String) -> [String] {
    if englishWordTable == nil {
        var table: [String: [String]] = [:]
        // 2026-10-13: english_words_extra.tsv は手で足す語 (固有名詞の GeoGuessr など。自動で作る english_words.tsv を作り直しても消えない)。前に置いた候補が先に出る
        for name in ["english_words_extra.tsv", "english_words.tsv"] {
            guard let content = try? String(contentsOf: execURL.appendingPathComponent(name), encoding: .utf8) else { continue }
            for line in content.split(whereSeparator: \.isNewline) where !line.hasPrefix("#") {
                let parts = line.split(separator: "\t")
                if parts.count == 2 {
                    let words = parts[1].split(separator: "|").map(String.init)
                    let reading = String(parts[0])
                    table[reading] = (table[reading] ?? []) + words.filter { !(table[reading] ?? []).contains($0) }
                }
            }
        }
        englishWordTable = table
    }
    return englishWordTable?[hiragana] ?? []
}

/// 2026-11-10: 動詞・形容詞の自立語の基本形の品詞の番号。IPADIC の left-id.def の「動詞,自立」と「形容詞,自立」のうち活用形が「基本形」のもの 46 個と、
/// 綴りが基本形と同じになる「五段・ワ行ウ音便,連用タ接続」(問うた・言うた の 問う・言う) の 2 個 (azooKey の品詞の番号はこの並びと同じ)。
/// 辞書の 問う は 基本形 (817) より 連用タ接続 (810) の値が高く、変換の候補には 810 の方が残る。
/// 候補の一覧で、送り仮名のある語でも漢字だけの語と同じ段に置く (buildCandidatesCore の 3.)
let dictionaryFormInflectableCids: Set<Int> = [
    19, 20, 37, 43, 52, 563, 573, 583, 592, 599, 612, 619, 628, 637, 639, 645, 651, 657, 663, 669, 679, 680, 695,
    696, 711, 712, 723, 731, 738, 746, 754, 762, 772, 773, 792, 802, 803, 817, 818, 819, 836, 841, 846, 851, 856, 863,
    810, 811,
]
/// 前の段に入れる基本形の辞書の値の下限。11-10 の測り (読み 149) で 1 ページ目に入った基本形 79 語のうち、-12 より上は 書く・勝つ・通う・看る など
/// ふつうの語、-12 以下は 爲す・舊い・高価い・気勢う・視張る など旧字・異体の綴りで、名詞 (茄子 など) を 1 ページ目から押し出していた
let listVerbMinValue: PValue = -12
/// 開発用の切り替え: AZK_LIST_VERB_TIER=0 で、基本形も後ろの段に (前の並べ方。測るため)
let listVerbTier = devEnvironment["AZK_LIST_VERB_TIER"] != "0"

/// 動詞・形容詞の自立語の基本形か (乞う・問う・濃い。乞う の連用タ接続・濃う・斯う (副詞) は違う)
func isDictionaryFormWord(_ e: DicdataElement) -> Bool {
    dictionaryFormInflectableCids.contains(e.lcid)
}

func isSingleKanji(_ text: String) -> Bool {
    guard text.count == 1, let v = text.unicodeScalars.first?.value else { return false }
    return (0x3400...0x9FFF).contains(v) || (0xF900...0xFAFF).contains(v)
}

/// 文節の読みの頭 (末尾の助詞を除いた部分) を 1 文字の漢字にした候補。助詞はそのまま後ろに付ける
@MainActor func singleKanjiList(hiragana: String, context: String, excluding: Set<String> = []) -> [String] {
    let particles = ["には", "では", "とは", "での", "への", "との", "にも", "でも", "から", "まで", "より",
                     "は", "が", "を", "の", "に", "で", "と", "も", "へ", "や", "か", "ね", "よ"]
    var targets: [(head: String, suffix: String)] = [(hiragana, "")]
    if let particle = particles.first(where: { hiragana.hasSuffix($0) && hiragana.count > $0.count }) {
        targets.append((String(hiragana.dropLast(particle.count)), particle))
    }
    var result: [String] = []
    for target in targets {
        let composing = directComposingText(target.head)
        let count = composing.input.count
        let converted = converter.requestCandidates(composing, options: getOptions(context: context, prediction: false, zenzai: false, nBest: 10))
        for candidate in converted.mainResults where candidate.correspondingCount == count && candidate.data.count == 1 && isSingleKanji(candidate.text) && isUsableWord(candidate.text) {
            let text = candidate.text + target.suffix
            if !result.contains(text) && !excluding.contains(text) {
                result.append(text)
            }
        }
        if !result.isEmpty { break }
    }
    // MS-IME と同じく部首・画数の順 (Unicode の漢字の並びがほぼ部首・画数順)
    return result.sorted { ($0.unicodeScalars.first?.value ?? 0) < ($1.unicodeScalars.first?.value ?? 0) }
}

@_silgen_name("ListSingleKanji")
@MainActor public func list_single_kanji(
    hiraganaPtr: UnsafePointer<CChar>,
    contextPtr: UnsafePointer<CChar>,
    lengthPtr: UnsafeMutablePointer<Int32>
) -> UnsafeMutablePointer<UnsafeMutablePointer<FFICandidate>?> {
    let hiragana = normalizeForConversion(String(cString: hiraganaPtr))
    let context = leftContext(String(cString: contextPtr))
    let main = buildCandidates(hiragana: hiragana, context: context)
    let texts = singleKanjiList(hiragana: hiragana, context: context, excluding: Set(main))
    let result = texts.map { text in
        FFICandidate(text: strdup(text), subtext: strdup(""), hiragana: strdup(""), correspondingCount: 0)
    }
    lengthPtr.pointee = Int32(result.count)
    return to_list_pointer(result)
}

/// 候補として出さない語: 半濁点などの結合文字つき (キ゚)、動詞の撥音便の途中形 (噛ん・駆ん)
func isUsableWord(_ word: String) -> Bool {
    if word.unicodeScalars.contains(where: { (0x3099...0x309C).contains($0.value) }) {
        return false
    }
    let chars = Array(word)
    if chars.count >= 2, chars.last == "ん",
       chars.dropLast().allSatisfy({ c in c.unicodeScalars.allSatisfy { (0x3400...0x9FFF).contains($0.value) } }) {
        return false
    }
    return true
}

/// 「書き続けていル」「嗅がクの」のように、仮名 1 文字だけがカタカナになった不自然な候補か
func hasIsolatedKatakana(_ text: String) -> Bool {
    let chars = Array(text)
    func isKatakana(_ c: Character) -> Bool {
        guard let v = c.unicodeScalars.first?.value else { return false }
        return (0x30A1...0x30FA).contains(v) || v == 0x30FC
    }
    // 2026-11-02: 長音の ー は、ひらがなの語にも使う (あーね・すごーい・そーなんだ)。ー は直前の字と同じ字種とみなす (あ の後ろなら ひらがな、
    // コ の後ろなら カタカナ)。前は ー をいつもカタカナと見ていて、あーねりかい の一覧から辞書の最良の あーね理解 が消えていた。
    // ー そのものは離れたカタカナに数えない (あーネ・すごーイ の ネ イ は、前の ー がひらがななので離れたカタカナ)
    var katakana: [Bool] = []
    for c in chars {
        katakana.append(c == "ー" ? (katakana.last ?? true) : isKatakana(c))
    }
    for (i, c) in chars.enumerated() where katakana[i] && c != "ー" {
        let prev = i > 0 ? katakana[i - 1] : false
        let next = i + 1 < chars.count ? katakana[i + 1] : false
        if !prev && !next && chars.count > 1 {
            return true
        }
    }
    return false
}

func debugWrite(_ data: Data) {
    guard let path = devEnvironment["AZK_DEBUG"],
          let handle = FileHandle(forWritingAtPath: path) ?? {
              FileManager.default.createFile(atPath: path, contents: nil)
              return FileHandle(forWritingAtPath: path)
          }() else { return }
    handle.seekToEndOfFile()
    handle.write(data)
    handle.closeFile()
}

// MARK: - 学習

@MainActor var learnedSinceSave = 0
@MainActor var learningAdmission = ConservativeLearningAdmission()

/// 読み → 確定した表記 を 1 つ覚える。辞書の候補に同じ表記があればそれ (品詞つき) を覚え、
/// なければ allowNewWord のときだけ名詞として覚える
@MainActor func learnOne(reading: String, text: String, context: String, allowNewWord: Bool) -> Bool {
    let reading = normalizeForConversion(reading)
    // 候補一覧の「単漢字…」は項目名なので覚えない (以前は キ→単漢字… を覚えて「聞き」が崩れた)
    if reading.isEmpty || text.isEmpty || text.contains("単漢字…") || hasNGWord(text) {
        return false
    }
    let composing = directComposingText(reading)
    let count = composing.input.count
    let converted = converter.requestCandidates(composing, options: getOptions(context: context, prediction: false, zenzai: false))
    if let candidate = converted.mainResults.first(where: { $0.correspondingCount == count && $0.text == text }) {
        // 2026-11-04: 単語登録した語 (isUserRegistered) は学習に書かない (登録を消したら出なくなるように。登録した語は辞書にあるので学習しなくても出る)
        if candidate.data.contains(where: { $0.metadata.contains(.isUserRegistered) }) { return false }
        guard learningAdmission.admit(reading: reading, text: text, symbolWord: candidate.data.contains(where: isSymbolOnlyElement)) else { return false }
        // 2026-11-03: 表記ゆれの語の形を覚える (hyouki.swift)。2026-11-04: 選んだ形として数えるのは、Space の一覧を出した読みで一覧の 1 番目と違う形を
        // 確定したときだけ (前は辞書の 1 位と比べていて、AI の変換のまま確定した漢字まで選んだ形に数えた)
        let listedFirst = hyoukiListedFirst.removeValue(forKey: reading) ?? text
        noteHyoukiChoices(candidate, defaultText: listedFirst)
        // 2026-11-06 (仕事 36): 漢数字を先にする設定で、一覧から算用数字の形 (1回) を選んだら、その語は次から漢数字にしない (counter_numerals.swift)
        noteCounterNumeralChoices(candidate, defaultText: listedFirst)
        converter.updateLearningData(candidate)
        return true
    }
    guard allowNewWord, text != reading, text.count <= 12 else {
        return false
    }
    guard learningAdmission.admit(reading: reading, text: text, symbolWord: isSymbolOnlyWord(text)) else { return false }
    let element = DicdataElement(word: text, ruby: hiraganaToKatakana(reading), cid: CIDData.一般名詞.cid, mid: MIDData.一般.mid, value: -10)
    let candidate = Candidate(text: text, value: -10, correspondingCount: count, lastMid: MIDData.一般.mid, data: [element])
    converter.updateLearningData(candidate)
    return true
}

/// 確定した文節列を覚える。読みと表記は \u{1F} 区切り。戻り値は覚えた個数
@_silgen_name("LearnSegments")
@MainActor public func learn_segments(
    readingsPtr: UnsafePointer<CChar>,
    textsPtr: UnsafePointer<CChar>,
    contextPtr: UnsafePointer<CChar>,
    allowNewWords: Bool,
    saveNow: Bool
) -> Int32 {
    if !(config["learning"] as? Bool ?? true) {
        return 0
    }
    clearPreviewMemo()
    // 2026-11-23 (a/symseg): 記号が付いた文節 (警告、・「象は」) は、記号の所で分けてから覚える。記号付きの語を覚えない
    // (変換の口は分け直して返すが、文節を伸ばした・縮めた一覧で選んだ文節には記号が付きうる。literal_symbols.swift)
    let (readings, texts) = splittingJoinedSymbols(
        readings: String(cString: readingsPtr).split(separator: "\u{1F}", omittingEmptySubsequences: false).map(String.init),
        texts: String(cString: textsPtr).split(separator: "\u{1F}", omittingEmptySubsequences: false).map(String.init))
    var context = leftContext(String(cString: contextPtr))
    // 2026-11-02: 難読語の同音語を替えた所で、利用者が元の語を選んで確定したら、その組は以後替えない (homophones.swift)
    noteHomophoneChoices(readings: readings.map(normalizeForConversion), texts: texts)
    // 2026-11-05 (仕事 22 の 3): ・ / ／ と 。 . の文節で、表の決めと違う形を選んで確定したら覚える (slash_dot.swift)
    noteSlashDotChoices(readings: readings.map(normalizeForConversion), texts: texts, context: context)
    var learned: Int32 = 0
    // 2026-11-20 (学習の効きすぎ、L1): 危ない文節 (助詞の読みの 1 語・難読語のまれな側) は 1 文節では覚えず、となりとつないで覚える (learn_guard.swift)
    // 2026-11-22: 難読語の表を読み終える前 (起動の直後) の確定は、azooKey の学習に覚えない (待たない。危ない形で 1 語で覚えるより安全。まとめ役の決め)
    let learnGuardReady = !learnGuardEnabled || learnGuardTables.loaded
    for unit in learnGuardReady ? learningUnits(readings: readings.map(normalizeForConversion), texts: texts) : [] {
        if learnOne(reading: unit.reading, text: unit.text, context: context, allowNewWord: allowNewWords && !unit.merged) {
            learned += Int32(unit.count)
        } else if unit.joinable {
            // 2026-11-21: 難読語を右へつないだ単位 (と、つなげない難読語の 1 文節 狂疾に) で、字が候補に無いときは、文節ごとの候補の語を並べて覚える
            // (名詞＋助詞の文節は、助詞を外した語と助詞に分けて引く)
            if learnJoinedParts(readings: unit.partReadings, texts: unit.partTexts, context: context) { learned += Int32(unit.count) }
        } else if unit.merged {
            // 2026-11-21: 助詞の読みの 1 語を左とつないだ単位 (規則賀) が候補に無いときは、文節ごとに覚える (危ない 1 語は L2 で 1 番目に出ない)
            var left = context
            for (reading, text) in zip(unit.partReadings, unit.partTexts) {
                if learnOne(reading: reading, text: text, context: left, allowNewWord: allowNewWords) { learned += 1 }
                left += text
            }
        }
        context += unit.text
    }
    // 2026-11-08 (仕事 49): 最後に確定した文節の最後の語を覚える (次の辞書だけの変換が、その語の品詞からつなぐ。メモリに 1 つ持つだけ。left_pos.swift)
    if let reading = readings.last, let text = texts.last {
        noteCommittedLeft(reading: reading, text: text)
    }
    if learned > 0 && texts.contains(where: { text in
        text.unicodeScalars.contains { (0xE0100...0xE01EF).contains($0.value) }
    }) {
        // Do not reuse a same-reading lattice that predates the newly learned IVS word.
        converter.stopComposition()
    }
    learnedSinceSave += Int(learned)
    if saveNow || learnedSinceSave >= 5 {
        converter.sendToDicdataStore(.closeKeyboard)
        learnedSinceSave = 0
    }
    return learned
}

/// 2026-10-29: 候補の一覧で選んでいる候補 (読み → 表記) が学習した語なら忘れる (Ctrl+Delete)。辞書の語は何もしない。忘れた学習の語の数を返す。
/// 学習の表はその場で書き直す (forgetMemory)。学習の表から読んだ語はメタデータ無しで、DicdataElement の == はメタデータも比べるので、
/// 印を外したもの・学習の印・ユーザ辞書の印つき (利用者が新しく作った語) を全部渡す
@_silgen_name("ForgetLearned")
@MainActor public func forget_learned(readingPtr: UnsafePointer<CChar>, textPtr: UnsafePointer<CChar>) -> Int32 {
    let reading = normalizeForConversion(String(cString: readingPtr))
    let text = String(cString: textPtr)
    if reading.isEmpty || text.isEmpty {
        return 0
    }
    clearPreviewMemo()
    // 2026-11-02: 難読語の同音語で「元の語に戻した」記録も、その候補の分は消す (homophones.swift。戻り値の数には入れない)
    forgetHomophoneChoices(reading: reading, text: text)
    // 2026-11-03: 表記ゆれの形の記録も (hyouki.swift)
    forgetHyoukiChoices(text: text)
    // 2026-11-06 (仕事 36): 算用数字のまま出す記録も (counter_numerals.swift)
    forgetCounterNumeralChoices(text: text)
    let composing = directComposingText(reading)
    let count = composing.input.count
    let converted = converter.requestCandidates(composing, options: getOptions(prediction: false, zenzai: false, nBest: 30))
    var learned: [DicdataElement] = []
    for candidate in converted.mainResults where candidate.correspondingCount == count && candidate.text == text {
        for element in candidate.data where element.metadata.contains(.isLearned) && !learned.contains(where: { $0.word == element.word && $0.ruby == element.ruby }) {
            learned.append(element)
        }
    }
    if learned.isEmpty {
        return 0
    }
    var targets: [DicdataElement] = []
    for element in learned {
        for metadata: DicdataElementMetadata in [.empty, .isLearned, [.isLearned, .isFromUserDictionary]] {
            var target = element
            target.metadata = metadata
            targets.append(target)
        }
    }
    converter.sendToDicdataStore(.forgetMemory(Candidate(text: text, value: 0, correspondingCount: count, lastMid: MIDData.一般.mid, data: targets)))
    converter.stopComposition()
    return Int32(learned.count)
}

@_silgen_name("SaveLearning")
@MainActor public func save_learning() {
    clearPreviewMemo()
    converter.sendToDicdataStore(.closeKeyboard)
    learnedSinceSave = 0
}
