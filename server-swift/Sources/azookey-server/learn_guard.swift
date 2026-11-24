import Foundation
import KanaKanjiConverterModule

/// 2026-11-20 (学習の効きすぎ、まとめ役の決めで 0.1.0 の直しのいちばん上): azooKey の学習 (memory) は、覚えた語を文脈を見ない固定の点
/// (−1 − 4/読みの字数 − 3(1 − 回数/255)³) で格子に足すので、1 回選んだまれな語 (狂疾 −4.73) が、同じ読みのよく使う語 (教室 −9.32) に
/// どの文脈でも勝つ (狂疾に行く)。また、IME が見せた誤り (規則|賀) をそのまま確定すると、つないだ要素 (規則賀 −4.86) が覚えられ、そのあとは自分で強まる。
/// L2 = 学習の場所から引くとき、次の「危ない要素」を格子に入れない (今ある学習にも効く)。となりとつないだ要素 (志賀・狂疾に陥る) は使う:
///   (A) 読みが助詞・助動詞の読みと同じ 1 語で、字がそのかなでない (が → 賀・ね → 値)
///   (B) 難読語の一覧 (homophone_words.tsv) のまれな側の語 (狂疾。同じ読みに勝つ語がある)
///   (C) 字の全体が本体の辞書の 1 語でなく、最後の 1 字が漢字で読みの終わりが助詞の読み、残り (2 字以上) が本体の辞書の 1 語 (規則賀・永田賀)
///   ただし、表記の書き分けの組 (hyouki_groups.tsv) にある字 (事・時・物 など) は (A)・(C) では危なくないとする。
///   2026-11-22: (B) では字だけで決めず、難読語の行の勝つ語が同じ読みの同じ書き分けの組にあるとき (充分・十分) だけ外す (読み込むときに一覧から除く)。
///   字だけで決めていたので、書き分けの組に字が載る 痴呆 (勝つ語 地方 は別の語) などを外していた (11-22 の測りで悪 25〜27 が 9 語に集まった)
/// 開発用のつまみ AZK_LEARN_GUARD=0 で前の形 (全部使う)
let learnGuardFunctionReadings: Set<String> = ["ガ", "ハ", "ニ", "ヲ", "デ", "ト", "モ", "ヘ", "ノ", "ヤ", "カ", "ネ", "ヨ", "ナ", "ダ", "タ", "シ", "テ", "バ", "ゾ", "サ", "ワ", "ゼ",
                                               "ヨリ", "カラ", "マデ", "ノデ", "ノニ", "ケド", "ダケ", "ホド", "ナド", "デス", "マス", "ナイ", "タイ", "ソウ", "ッテ"]
/// (C) の読みの終わりに見る助詞の読み (1 字の漢字にされやすいもの)
let learnGuardTailReadings: [String] = ["ガ", "ハ", "ニ", "ヲ", "デ", "ト", "モ", "ヘ", "ノ", "ヤ", "カ"]

/// 難読語のまれな側 ("読み (ひらがな)\t語") と、表記の書き分けの組の字。起動のあとに裏で読む (読み終わるまでは (B) と書き分けの除外をかけない)
final class LearnGuardTables: @unchecked Sendable {
    private let condition = NSCondition()
    private var rare: Set<String>? = nil
    private var hyouki: Set<String>? = nil
    private var justLoaded = false
    func set(rare: Set<String>, hyouki: Set<String>) {
        condition.lock(); defer { condition.unlock() }
        self.rare = rare
        self.hyouki = hyouki
        justLoaded = true
        condition.broadcast()
    }
    /// 2026-11-22: 読み終えた直後の 1 回だけ真 (読み終える前の変換の覚えを捨てるため。refreshAfterLearnGuardLoaded)
    func consumeJustLoaded() -> Bool {
        condition.lock(); defer { condition.unlock() }
        guard justLoaded else { return false }
        justLoaded = false
        return true
    }
    func lookup() -> (rare: Set<String>?, hyouki: Set<String>?) {
        condition.lock(); defer { condition.unlock() }
        return (rare, hyouki)
    }
    /// 2026-11-22: 読み終えたか。起動の直後に確定すると、L1 が表を読み終える前に覚えて、難読語をつながずに 1 語で覚えていた
    /// (覚えた 狂疾に は L2 で 1 番目に出ず、同じ並びでも効かなかった)。読み終える前の確定は覚えない (待つと主のスレッドが止まって入力が固まるため。まとめ役の決め)
    var loaded: Bool {
        condition.lock(); defer { condition.unlock() }
        return rare != nil
    }
}

func learnGuardHiragana(_ katakana: String) -> String {
    String(String.UnicodeScalarView(katakana.unicodeScalars.map { s in
        (0x30A1...0x30F6).contains(s.value) ? Unicode.Scalar(s.value - 0x60)! : s
    }))
}

/// 2026-11-21 (文節が「名詞＋助詞」になったため): 字と読みの終わりの、同じ助詞のかな (に・が・を・から など) を外した (字, 読み (ひらがな), 外したかな)。
/// 字の終わりのひらがなが読みの終わりと同じで、助詞・助動詞の読みの表に合うときだけ。外したあとが空なら nil
func learnGuardSplitParticles(text: String, reading: String) -> (text: String, reading: String, tail: String)? {
    let hiraganaTail = String(text.reversed().prefix(while: { c in c.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) } }).reversed())
    guard !hiraganaTail.isEmpty, hiraganaTail.count < text.count, reading.hasSuffix(hiraganaTail), hiraganaTail.count < reading.count else { return nil }
    let tailKatakana = String(String.UnicodeScalarView(hiraganaTail.unicodeScalars.map { Unicode.Scalar($0.value + 0x60)! }))
    guard learnGuardFunctionReadings.contains(tailKatakana) else { return nil }
    return (String(text.dropLast(hiraganaTail.count)), String(reading.dropLast(hiraganaTail.count)), hiraganaTail)
}

func learnGuardIsKanji(_ c: Character) -> Bool {
    c.unicodeScalars.count == 1 && c.unicodeScalars.allSatisfy { (0x4E00...0x9FFF).contains($0.value) || $0.value == 0x3005 }
}

/// 学習の要素が危ないか (真なら格子に入れない)。inDictionary は「読み (カタカナ)・字が本体の辞書に 1 語で載っているか」
func isRiskyLearnedElement(word: String, ruby: String, rare: Set<String>?, hyouki: Set<String>?, inDictionary: (String, String) -> Bool) -> Bool {
    if word.isEmpty || ruby.isEmpty { return false }
    let hira = learnGuardHiragana(ruby)
    // (B) 難読語のまれな側。2026-11-21: 後ろに助詞のかなが付いた要素 (狂疾に。文節が「名詞＋助詞」になったため) も。
    // 2026-11-22: 書き分けの除外は、一覧を読むときに行ごと (勝つ語が同じ書き分けの組) にかけてあるので、ここでは字だけで外さない
    if let rare, rare.contains(hira + "\t" + word) { return true }
    if let rare, let split = learnGuardSplitParticles(text: word, reading: hira), rare.contains(split.reading + "\t" + split.text) { return true }
    if let hyouki, hyouki.contains(word) { return false }
    // (A) 助詞・助動詞の読みの 1 語 (が → 賀)
    if learnGuardFunctionReadings.contains(ruby) && word != ruby && word != hira { return true }
    // (C) 規則賀 型
    let chars = Array(word)
    if chars.count >= 3, let last = chars.last, learnGuardIsKanji(last),
       let tail = learnGuardTailReadings.first(where: { ruby.hasSuffix($0) && ruby.count > $0.count }) {
        let headWord = String(chars.dropLast())
        let headRuby = String(ruby.dropLast(tail.count))
        if !inDictionary(ruby, word) && inDictionary(headRuby, headWord) { return true }
    }
    return false
}

/// 難読語の一覧の「読み<TAB>語」。2026-11-22: 語と勝つ語が、同じ読みの表記の書き分けの組に両方載る行 (充分・十分 = 同じ語の書き分け) は除く
func parseLearnGuardRare(_ text: String?, hyoukiGroups: [String: [Set<String>]] = [:]) -> Set<String> {
    var set: Set<String> = []
    guard let text else { return set }
    for line in text.split(separator: "\n", omittingEmptySubsequences: true) where !line.hasPrefix("#") {
        let parts = line.split(separator: "\t", omittingEmptySubsequences: false)
        guard parts.count >= 3, !parts[0].isEmpty, !parts[1].isEmpty, parts[1] != parts[2] else { continue }
        let reading = String(parts[0]), word = String(parts[1]), winner = String(parts[2])
        if hyoukiGroups[reading]?.contains(where: { $0.contains(word) && $0.contains(winner) }) == true { continue }
        set.insert(reading + "\t" + word)
    }
    return set
}

/// 表記の書き分けの組の、読み → 組ごとの形 (2026-11-22)
func parseLearnGuardHyoukiGroups(_ text: String?) -> [String: [Set<String>]] {
    parseLearnGuardHyoukiAll(text).groups
}

func parseLearnGuardHyouki(_ text: String?) -> Set<String> {
    parseLearnGuardHyoukiAll(text).words
}

/// 表記の書き分けの組を 1 回で読む: 組の字の全部と、読み → 組ごとの形 (2026-11-22。前は 2 回読んでいて、起動の直後の読み終わりが遅れた)
func parseLearnGuardHyoukiAll(_ text: String?) -> (words: Set<String>, groups: [String: [Set<String>]]) {
    var words: Set<String> = []
    var groups: [String: [Set<String>]] = [:]
    guard let text else { return (words, groups) }
    for line in text.split(separator: "\n", omittingEmptySubsequences: true) where !line.hasPrefix("#") {
        let parts = line.split(separator: "\t", omittingEmptySubsequences: false)
        guard parts.count >= 4 else { continue }
        let forms = Set(parts[3].split(separator: "|").filter { !$0.isEmpty }.map(String.init))
        words.formUnion(forms)
        if forms.count >= 2, !parts[0].isEmpty { groups[String(parts[0]), default: []].append(forms) }
    }
    return (words, groups)
}

let learnGuardTables = LearnGuardTables()

/// 起動のとき (Initialize) に呼ぶ。表は裏で読む
@MainActor func installLearnGuard() {
    if devEnvironment["AZK_LEARN_GUARD"] == "0" {
        DicdataStore.learnedElementGuard = nil
        return
    }
    let tables = learnGuardTables
    let dropped = learnGuardDropped
    DicdataStore.learnedElementGuard = { element, inDictionary in
        let (rare, hyouki) = tables.lookup()
        // 2026-11-22: 表を読み終えるまで (起動のあと 0.2〜0.5 秒) は、学習の場所の要素を格子に入れない。危ない要素を見分けられず、
        // 覚えた 1 語の 狂疾・規則賀 がそのまま勝っていた (落ちたあと・更新のあとの起動し直しの直後に打つと当たる)。学習が 0.5 秒効かないだけ
        guard rare != nil else { return true }
        let risky = isRiskyLearnedElement(word: element.word, ruby: element.ruby, rare: rare, hyouki: hyouki, inDictionary: inDictionary)
        // 2026-11-21: 格子に入れなかった語を覚えておき、候補の一覧で 2 番目に上げる (promotingDroppedLearned)
        if risky { dropped.note(ruby: element.ruby, word: element.word) }
        return risky
    }
    let homophoneURL = execURL.appendingPathComponent("homophone_words.tsv")
    let hyoukiURL = execURL.appendingPathComponent("hyouki_groups.tsv")
    let started = Date()
    let debugPath = devEnvironment["AZK_DEBUG"]
    DispatchQueue.global(qos: .utility).async {
        let hyouki = parseLearnGuardHyoukiAll(try? String(contentsOf: hyoukiURL, encoding: .utf8))
        let rare = parseLearnGuardRare(try? String(contentsOf: homophoneURL, encoding: .utf8), hyoukiGroups: hyouki.groups)
        tables.set(rare: rare, hyouki: hyouki.words)
        // 2026-11-22: 起動から表を読み終えるまでの時間 (開発用のビルドの AZK_DEBUG だけ。測りの道具が読む)
        if let debugPath {
            let line = "learn guard tables: \(Int(-started.timeIntervalSinceNow * 1000))ms rare \(rare.count) hyouki \(hyouki.words.count)\n"
            if let handle = FileHandle(forWritingAtPath: debugPath) ?? { FileManager.default.createFile(atPath: debugPath, contents: nil); return FileHandle(forWritingAtPath: debugPath) }() {
                handle.seekToEndOfFile(); handle.write(line.data(using: .utf8)!); handle.closeFile()
            }
        }
    }
}

/// 2026-11-20 (学習の効きすぎ、L1): 確定した文節を、覚える単位にまとめる。
///   (A) 助詞・助動詞の読みの 1 語 (し|が → 志|賀): 左の文節が漢字かカタカナで終わればつなぐ (志|賀 → 志賀、規則|賀 → 規則賀)
///   (B) 難読語のまれな側 (狂疾): 右へかなだけの文節 (助詞など) を越えて次の文節までつなぐ (狂疾|に|陥る → 狂疾に陥る)
/// つないだ単位は、変換の候補に同じ字があるときだけ覚える (新しい語として足さない。規則賀 のような IME の見せた誤りをつないで作らないため)。
/// 2026-11-21: つなげない 1 語 (が → 賀・狂疾 だけ) も、1 語として覚える (L2 で 1 番目には出ないが、Space の一覧の 2 番目に上がる。
/// 覚えたのにどこにも見えないと「学習が効かない」と見えるため)。つないだ単位が覚えられなかったときは、(B) は文節ごとの語を並べて (learnJoinedParts)、
/// (A) は文節ごとに覚える
struct LearningUnit: Equatable {
    enum Kind: Equatable { case normal, merged, functionAlone, rareAlone }
    var reading: String
    var text: String
    var kind: Kind
    var count: Int
    /// 2026-11-21: 難読語を右へつないだ単位 (狂疾｜に｜陥る) の、もとの文節の読みと字。つないだ字が変換の候補に無いときは、
    /// 文節ごとの候補の語を並べて覚える (learnJoinedParts)。それ以外の単位では空
    var partReadings: [String] = []
    var partTexts: [String] = []
    /// (B) の難読語を右へつないだ単位なら真 (覚えられなければ learnJoinedParts)。(A) のつないだ単位は偽 (覚えられなければ文節ごと)
    var joinable = false
    var merged: Bool { kind == .merged }
}

func learningUnits(readings: [String], texts: [String], rare: Set<String>? = learnGuardTables.lookup().rare,
                   enabled: Bool = learnGuardEnabled) -> [LearningUnit] {
    let pairs = Array(zip(readings, texts))
    guard enabled else { return pairs.map { LearningUnit(reading: $0.0, text: $0.1, kind: .normal, count: 1) } }
    func katakana(_ hira: String) -> String {
        String(String.UnicodeScalarView(hira.unicodeScalars.map { s in (0x3041...0x3096).contains(s.value) ? Unicode.Scalar(s.value + 0x60)! : s }))
    }
    func isFunction(_ r: String, _ t: String) -> Bool { learnGuardFunctionReadings.contains(katakana(r)) && t != r && t != katakana(r) }
    func isRare(_ r: String, _ t: String) -> Bool {
        guard let rare else { return false }
        if rare.contains(r + "\t" + t) { return true }
        // 2026-11-21: 文節が「名詞＋助詞」(狂疾に) のときは、助詞を外した語で見る
        if let split = learnGuardSplitParticles(text: t, reading: r) { return rare.contains(split.reading + "\t" + split.text) }
        return false
    }
    func kanaOnly(_ t: String) -> Bool { !t.isEmpty && t.unicodeScalars.allSatisfy { (0x3041...0x309F).contains($0.value) } }
    func endsContent(_ t: String) -> Bool {
        guard let c = t.last else { return false }
        return learnGuardIsKanji(c) || c.unicodeScalars.allSatisfy { (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC }
    }
    var units: [LearningUnit] = []
    var i = 0
    while i < pairs.count {
        let (r, t) = pairs[i]
        if isRare(r, t) {
            var reading = r, text = t, j = i + 1
            while j < pairs.count && kanaOnly(pairs[j].1) { reading += pairs[j].0; text += pairs[j].1; j += 1 }
            if j < pairs.count && !isFunction(pairs[j].0, pairs[j].1) && !isRare(pairs[j].0, pairs[j].1) {
                units.append(LearningUnit(reading: reading + pairs[j].0, text: text + pairs[j].1, kind: .merged, count: j - i + 1,
                                          partReadings: pairs[i...j].map(\.0), partTexts: pairs[i...j].map(\.1), joinable: true))
                i = j + 1
            } else {
                // つなげない: 難読語は 1 語で、後ろのかなの文節はそれぞれ覚える
                units.append(LearningUnit(reading: r, text: t, kind: .rareAlone, count: 1, partReadings: [r], partTexts: [t], joinable: true))
                i += 1
            }
            continue
        }
        if isFunction(r, t) {
            if let last = units.last, last.kind != .rareAlone, endsContent(last.text) {
                let parts = last.merged ? (last.partReadings, last.partTexts) : ([last.reading], [last.text])
                units[units.count - 1] = LearningUnit(reading: last.reading + r, text: last.text + t, kind: .merged, count: last.count + 1,
                                                      partReadings: parts.0 + [r], partTexts: parts.1 + [t])
            } else {
                units.append(LearningUnit(reading: r, text: t, kind: .functionAlone, count: 1))
            }
            i += 1
            continue
        }
        units.append(LearningUnit(reading: r, text: t, kind: .normal, count: 1))
        i += 1
    }
    return units
}

/// 開発用のつまみ AZK_LEARN_GUARD=0 で、L1 も前の形
let learnGuardEnabled: Bool = devEnvironment["AZK_LEARN_GUARD"] != "0"

/// 2026-11-21 (学習の効きすぎ、L1 の続き): つないだ単位の字 (狂疾に陥る) が、読み全体の変換の候補 (mainResults) に無いとき、
/// 文節ごとに変換の候補から同じ字の語を引いて並べ、1 つの候補として覚える。azooKey の学習の「全体」の要素が つないだ字 で残る
/// (1 語の 狂疾 は語ごとの学習にも入るが、L2 で引かれない)。どれかの文節の字が候補に無ければ覚えない
@MainActor func learnJoinedParts(readings: [String], texts: [String], context: String) -> Bool {
    guard !readings.isEmpty, readings.count == texts.count else { return false }
    var data: [DicdataElement] = []
    var left = context
    func lookup(_ reading: String, _ text: String, _ left: String) -> [DicdataElement]? {
        let composing = directComposingText(reading)
        let count = composing.input.count
        let converted = converter.requestCandidates(composing, options: getOptions(context: left, prediction: false, zenzai: false))
        guard let candidate = converted.mainResults.first(where: { $0.correspondingCount == count && $0.text == text }),
              !candidate.data.contains(where: { $0.metadata.contains(.isUserRegistered) }) else { return nil }
        return candidate.data
    }
    for (reading, text) in zip(readings, texts) {
        if let found = lookup(reading, text, left) {
            data += found
        } else if let split = learnGuardSplitParticles(text: text, reading: reading),
                  let head = lookup(split.reading, split.text, left), let tail = lookup(split.tail, split.tail, left + split.text) {
            // 2026-11-21: 文節が「名詞＋助詞」(狂疾に) で、その字が候補に無いときは、助詞を外した語 (狂疾) と助詞 (に) に分けて引く
            data += head + tail
        } else {
            return false
        }
        left += text
    }
    let reading = readings.joined(), text = texts.joined()
    guard learningAdmission.admit(reading: reading, text: text, symbolWord: false) else { return false }
    let total = directComposingText(reading).input.count
    let candidate = Candidate(text: text, value: data.reduce(0) { $0 + $1.value() }, correspondingCount: total,
                              lastMid: data.last?.mid ?? MIDData.一般.mid, data: data)
    converter.updateLearningData(candidate)
    return true
}

/// 2026-11-21 (まとめ役の決め): L2 で格子に入れなかった学習の語 (読み (カタカナ) → 語)。1 番目には出さないが、覚えたのにどこにも見えないと
/// 「学習が効かない」と見えるので、Space の一覧では 2 番目に上げる (promotingDroppedLearned)。学習を空にしたら消す
final class LearnGuardDropped: @unchecked Sendable {
    private let lock = NSLock()
    private var byRuby: [String: Set<String>] = [:]
    func note(ruby: String, word: String) {
        lock.lock(); defer { lock.unlock() }
        if byRuby.count >= 4096 && byRuby[ruby] == nil { byRuby.removeAll() }
        byRuby[ruby, default: []].insert(word)
    }
    func clear() {
        lock.lock(); defer { lock.unlock() }
        byRuby.removeAll()
    }
    /// 読み (カタカナ) の頭に合う語。読みが 1 字の語は、読み全体と同じときだけ (が → 賀。がいこく の頭には合わせない)
    func words(matching katakana: String) -> [(ruby: String, word: String)] {
        lock.lock(); defer { lock.unlock() }
        return byRuby.flatMap { ruby, words in
            (ruby == katakana || (ruby.count >= 2 && katakana.hasPrefix(ruby))) ? words.sorted().map { (ruby: ruby, word: $0) } : []
        }
    }
}

let learnGuardDropped = LearnGuardDropped()

/// 候補の一覧で 2 番目に上げる、L2 で外した学習の語 (読み hiragana の頭に合うもの)。
/// 2026-11-23 (セキュリティの点検 14): 学習から作った表なので、学習を使ってはいけない接続 (personalDataAllowed が偽: 隔離のアプリ) では空。
/// 前は共有の表をそのまま引いていて、隔離のアプリの一覧の順位から、ほかのアプリで覚えた語の有無が分かった
@MainActor func droppedLearnedForList(hiragana: String) -> [(ruby: String, word: String)] {
    guard learnGuardEnabled, personalDataAllowed else { return [] }
    return learnGuardDropped.words(matching: hiraganaToKatakana(hiragana))
}

/// 2026-11-21: L2 で 1 番目に使わなくなった学習の語 (狂疾・賀) を、候補の一覧の 2 番目に上げる (1 番目は変えない)。
/// 読みの長い語から、一覧の 3 番目より後ろにある「その語で始まる候補」を 2 番目へ (2 番目までにあればそのまま)
func promotingDroppedLearned(_ texts: [String], dropped: [(ruby: String, word: String)]) -> [String] {
    guard texts.count > 2 else { return texts }
    for (_, word) in dropped.sorted(by: { $0.ruby.count > $1.ruby.count }) {
        guard let i = texts.firstIndex(where: { $0.hasPrefix(word) }) else { continue }
        if i <= 1 { return texts }
        var out = texts
        let text = out.remove(at: i)
        out.insert(text, at: 1)
        return out
    }
    return texts
}

/// 2026-11-22: 表を読み終えた直後の最初の変換・一覧の頭で、読み終える前の変換の覚え (仮表示の覚え・変換器の前の格子) を 1 回捨てる
/// (読み終える前は学習の場所の要素を入れずに作った結果なので、そのまま使うと学習が効かないまま残る)
@MainActor func refreshAfterLearnGuardLoaded() {
    guard learnGuardEnabled, learnGuardTables.consumeJustLoaded() else { return }
    clearPreviewMemo()
    converter.stopComposition()
}
