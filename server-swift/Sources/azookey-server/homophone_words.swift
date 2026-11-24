import Foundation
import KanaKanjiConverterModule

/// 2026-11-02: 難読語の一覧 (homophone_words.tsv)。azooKey の辞書で読みだけ打つと同じ読みの別の名詞 (勝つ語) に負け、
/// JMdict に同じ読みで載る漢字の名詞 (蹉跌 → 砂鉄、帰趨 → 奇数)。連語と目印の表 (homophones.swift) は決まった所でしか効かないので、
/// 利用者が一度選んだ語を覚えておき、AI に消させないようにする。
///   行: 読み<TAB>語<TAB>勝つ語<TAB>学習<TAB>一覧 (学習・一覧は 0 か 1。どちらも 0 の行は、同じ読みの語の仲間を知るためだけに読む)
/// 学習 (1): 利用者がその語を確定したら、読みごとに覚える (学習の場所の homophone_learned.txt。読みごとに最後に確定した 1 語)。
///   そのあとの AI の変換 (清書・Space) の結果に、同じ読みの仲間の語があれば、その語に替えた文と元の文を zenz に採点させ、
///   点の差が homophoneLearnedBonus までなら替える (会社に行く のように AI がはっきり選べる所は替えない)。
///   仮表示 (辞書だけの変換) にその語が出ている所だけ (清書で表示が替わる所を増やさない)。
///   仮表示 (辞書だけの変換) にはかけない (AI の点が無く文脈で選べない。辞書の学習はもともと仮表示に効いている)。
///   替えた所で利用者が元の語を確定したら、その読みの記録を消す。Ctrl+Delete (ForgetLearned) と learning_ng.txt でも消える
/// 一覧 (1): Space の一覧で、1 番目に勝つ語があり、そこを同じ読みの語に替えた候補が 2 番目に無ければ、3 番目より後ろにある
///   その語の候補を 2 番目へ上げる (1 番目は変えない。IME 本体は表示している文節を 1 番目に置き直す)
/// 一覧の 学習・一覧 の印は、回数などの決まりで作る (tools/homophones/build_words.py)
struct HomophoneWords {
    /// 語 (一覧の語・勝つ語) → (語, その語が載る行の読み)
    var entries: [HomophoneKey: (word: String, readings: [String])] = [:]
    /// 読み → 同じ読みの語 (一覧の語と勝つ語)
    var groups: [String: Set<String>] = [:]
    /// 学習をかける「読み<TAB>語」
    var learnable: Set<String> = []
    /// 勝つ語 → 2 番目に上げてよい (読み, 語) (表の順)
    var promotable: [String: [(reading: String, word: String)]] = [:]
    /// 語の長さ (スカラー値) の最大
    var longestWord = 0
    var isEmpty: Bool { entries.isEmpty }
}

/// 配る一覧は 2 万行・0.7 MB
let homophoneWordLimits = (maxBytes: 4 << 20, maxLines: 100_000, maxFieldLength: 64)

func parseHomophoneWords(_ text: String?) -> HomophoneWords {
    var words = HomophoneWords()
    guard let text else { return words }
    var strings: [HomophoneKey: String] = [:]
    func string(_ scalars: ArraySlice<Unicode.Scalar>) -> String {
        guard let key = HomophoneKey(scalars) else { return homophoneString(scalars) }
        if let known = strings[key] { return known }
        let made = homophoneString(scalars)
        strings[key] = made
        return made
    }
    let one = Array("1".unicodeScalars)
    BoundedTable.scalarRows(text: text, maxLines: homophoneWordLimits.maxLines, columns: 5...5, maxFieldLength: homophoneWordLimits.maxFieldLength) { parts in
        guard !parts[0].isEmpty, !parts[1].isEmpty, !parts[2].isEmpty, !parts[1].elementsEqual(parts[2]),
              let wordKey = HomophoneKey(parts[1]), let winnerKey = HomophoneKey(parts[2]) else { return }
        let reading = string(parts[0]), word = string(parts[1]), winner = string(parts[2])
        for (key, w) in [(wordKey, word), (winnerKey, winner)] {
            if words.entries[key]?.readings.contains(reading) != true {
                words.entries[key, default: (word: w, readings: [])].readings.append(reading)
            }
            words.longestWord = max(words.longestWord, w.unicodeScalars.count)
        }
        words.groups[reading, default: []].formUnion([word, winner])
        if parts[3].elementsEqual(one) { words.learnable.insert(reading + "\t" + word) }
        if parts[4].elementsEqual(one) { words.promotable[winner, default: []].append((reading: reading, word: word)) }
    }
    return words
}

/// text[p..<p+length] の前後に漢字が続かない (長い語の一部でない)
func homophoneIsWholeWord(_ text: [Unicode.Scalar], _ p: Int, _ length: Int) -> Bool {
    !(p > 0 && isHomophoneKanji(text[p - 1])) && !(p + length < text.count && isHomophoneKanji(text[p + length]))
}

/// 字の並び (text の base より後ろ) にある一覧の語 (前後に漢字が続かないもの)。(位置, 長さ, 語の項目)。重なる所は前のほう、同じ所なら長い語
func homophoneWordSpots(_ text: [Unicode.Scalar], from base: Int = 0, words: HomophoneWords)
    -> [(start: Int, length: Int, entry: (word: String, readings: [String]))] {
    var found: [(start: Int, length: Int, entry: (word: String, readings: [String]))] = []
    var end = base
    for i in base..<text.count where i >= end {
        for length in stride(from: min(words.longestWord, text.count - i), through: 1, by: -1) {
            guard let key = HomophoneKey(text[i..<(i + length)]), let entry = words.entries[key], homophoneIsWholeWord(text, i, length) else { continue }
            found.append((start: i, length: length, entry: entry))
            end = i + length
            break
        }
    }
    return found
}

/// 変換した文字列 (output) の中で、学習した一覧の語 (learned: 読み → 語) の仲間の語がある所。打った読みに読みがあるときだけ。
/// 左の文脈 (left) は語の前に漢字が続くかを見るだけ
func learnedHomophoneSpots(left: String, output: String, reading: String, words: HomophoneWords, learned: [String: String]) -> [HomophoneReplacement] {
    guard !words.isEmpty, !learned.isEmpty, !output.isEmpty else { return [] }
    let leftScalars = Array(left.unicodeScalars)
    let text = leftScalars + Array(output.unicodeScalars)
    let typed = Array(reading.unicodeScalars)
    var found: [HomophoneReplacement] = []
    for spot in homophoneWordSpots(text, from: leftScalars.count, words: words) {
        for r in spot.entry.readings {
            guard let chosen = learned[r], chosen != spot.entry.word, words.learnable.contains(r + "\t" + chosen),
                  homophoneContains(typed, Array(r.unicodeScalars)) else { continue }
            found.append(HomophoneReplacement(start: spot.start - leftScalars.count, original: spot.entry.word, replacement: chosen,
                                              reading: r, reason: "学習"))
            break
        }
    }
    return found
}

/// 学習の差し替えのうち、仮表示 (preview: 辞書だけの変換。辞書の学習が効いている) にもう学習した語が語として出ていて、
/// zenz の点 (対数確率) で、替えた文が元の文より bonus を超えて低くないもの。
/// scores[0] は元の文、scores[k + 1] は spots[k] だけを替えた文の点。点が取れなければ替えない。
/// 仮表示に出ていない所は替えない: 清書だけが変わると、仮表示→清書で表示が切り替わる回数が増える (ctx_tune の 16,615 場面で、
/// 点の差だけだと切り替わりが 2,234 → 2,738、仮表示に出ている所だけなら 2,234 → 2,158 (上限 3、決める側の半分))
func acceptedLearnedSpots(_ spots: [HomophoneReplacement], scores: [Float]?, bonus: Float, preview: String?) -> [HomophoneReplacement] {
    guard let scores, scores.count == spots.count + 1, scores[0].isFinite else { return [] }
    let shown = preview.map { Array($0.unicodeScalars) }
    return spots.indices.filter { k in
        scores[k + 1].isFinite && scores[k + 1] + bonus >= scores[0]
            && (shown.map { homophoneHasWord($0, Array(spots[k].replacement.unicodeScalars)) } ?? false)
    }.map { spots[$0] }
}

/// Space の一覧: 1 番目の候補にある一覧の語の仲間を、同じ読みの別の語に替えた候補を 2 番目に置く。
///   - 学習した語 (learned) に替えた候補は、3 番目より後ろにあれば 2 番目へ、無ければ 2 番目に足す (清書で AI の点が足りず替えなかったとき、すぐ選べるように)
///   - そうでなければ、2 番目が同じ所を同じ読みの語に替えた候補でないときだけ、勝つ語を 一覧 (1) の語に替えた候補のうち、
///     一覧で最も前にあるもの (3 番目より後ろ) を 2 番目へ。1 番目は変えない
func promotingHomophoneWords(_ texts: [String], reading: String, words: HomophoneWords, learned: [String: String]) -> [String] {
    guard texts.count >= 2, !words.isEmpty else { return texts }
    let first = Array(texts[0].unicodeScalars)
    let typed = Array(reading.unicodeScalars)
    var learnedForms: [String] = [], promotedForms: Set<String> = [], neighbors: Set<String> = []
    for spot in homophoneWordSpots(first, words: words) {
        func form(_ word: String) -> String {
            var scalars = first
            scalars.replaceSubrange(spot.start..<(spot.start + spot.length), with: word.unicodeScalars)
            return homophoneString(scalars)
        }
        for r in spot.entry.readings where homophoneContains(typed, Array(r.unicodeScalars)) {
            for other in words.groups[r] ?? [] where other != spot.entry.word { neighbors.insert(form(other)) }
            if let chosen = learned[r], chosen != spot.entry.word, words.learnable.contains(r + "\t" + chosen) {
                learnedForms.append(form(chosen))
            }
            for (pr, word) in words.promotable[spot.entry.word] ?? [] where pr == r { promotedForms.insert(form(word)) }
        }
    }
    var result = texts
    if let target = learnedForms.first {
        guard texts[1] != target else { return texts }
        result.removeAll { $0 == target }
        result.insert(target, at: 1)
        return result
    }
    guard !neighbors.contains(texts[1]),
          let index = texts.indices.dropFirst(2).first(where: { promotedForms.contains(texts[$0]) }) else { return texts }
    result.remove(at: index)
    result.insert(texts[index], at: 1)
    return result
}

/// 確定した文節 (読みと字) で、学習の記録 (読み → 語) を直す。
///   - 文節の字に 学習 (1) の語が語としてあり、その文節の読みに語の読みがあれば、その読みはその語にする (前に別の語を覚えていても替える)
///   - この入力で学習の差し替えをした所 (offered: 読み<TAB>元の語<TAB>替えた語) で、利用者が元の語を確定し、替えた語が無ければ、その読みの記録を消す
func learnedHomophoneChoices(readings: [String], texts: [String], words: HomophoneWords, learned: [String: String],
                             offered: Set<String>) -> [String: String] {
    var result = learned
    for (reading, text) in zip(readings, texts) {
        let typed = Array(reading.unicodeScalars)
        for spot in homophoneWordSpots(Array(text.unicodeScalars), words: words) {
            for r in spot.entry.readings where words.learnable.contains(r + "\t" + spot.entry.word) && homophoneContains(typed, Array(r.unicodeScalars)) {
                result[r] = spot.entry.word
            }
        }
    }
    let typed = Array(readings.joined().unicodeScalars), committed = Array(texts.joined().unicodeScalars)
    for entry in offered {
        let parts = entry.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3, result[parts[0]] == parts[2], homophoneContains(typed, Array(parts[0].unicodeScalars)),
              homophoneHasWord(committed, Array(parts[1].unicodeScalars)), !homophoneContains(committed, Array(parts[2].unicodeScalars)) else { continue }
        result.removeValue(forKey: parts[0])
    }
    return result
}

/// 学習の消し方で一緒に消す記録 (読み): 候補の一覧で忘れた候補 (Ctrl+Delete。読み → 表記) に読みと語が入っているものと、
/// 学習から外す語 (learning_ng.txt) を含む語のもの
func forgottenLearnedHomophones(_ learned: [String: String], reading: String? = nil, text: String? = nil, discarded: [String] = []) -> Set<String> {
    let typed = reading.map { Array($0.unicodeScalars) }, chosen = text.map { Array($0.unicodeScalars) }
    return Set(learned.filter { r, word in
        if discarded.contains(where: { !$0.isEmpty && word.contains($0) }) { return true }
        guard let typed, let chosen else { return false }
        return homophoneContains(typed, Array(r.unicodeScalars)) && homophoneHasWord(chosen, Array(word.unicodeScalars))
    }.keys)
}

@MainActor func homophoneWords() -> HomophoneWords { loadedHomophoneTables().words }

/// 学習した語に戻す、zenz の点の差の上限 (対数確率)。AZK_LEARNED_HOMOPHONE_BONUS で試せる (開発用のビルドだけ)。
/// 2026-11-03: 2 (難読語の組の dev で 2・3・4・6 を比べ、直る数と壊す数の差がいちばん大きい値。test でも同じ。切り替わりはどの値でも増えない)
let homophoneLearnedBonus: Float = {
    if let v = devEnvironment["AZK_LEARNED_HOMOPHONE_BONUS"], let f = Float(v), f.isFinite { return f }
    return 2
}()

/// この入力で学習の差し替えをした組 (読み<TAB>元の語<TAB>替えた語)
@MainActor var homophoneLearnedOffered: Set<String> = []
/// 一覧の語を選んだ記録 (読み → 語)。学習の場所 (memory) の homophone_learned.txt に残す
@MainActor var homophoneLearnedCache: [String: String]? = nil
let homophoneLearnedLimit = 10_000

@MainActor func homophoneLearnedURL() -> URL { execURL.appendingPathComponent("memory").appendingPathComponent("homophone_learned.txt") }

/// 2026-11-03: 学習データを使ってはいけない接続 (personalDataAllowed が偽) では、記録を読まない (空) ・書かない (homophoneDeclined と同じ)
@MainActor func homophoneLearned() -> [String: String] {
    guard personalDataAllowed else { return [:] }
    if let learned = homophoneLearnedCache { return learned }
    let rows = BoundedTable.rows(homophoneLearnedURL(), maxBytes: 1 << 20, maxLines: homophoneLearnedLimit, columns: 2...2, maxFieldLength: 64)
    var stored: [String: String] = [:]
    for row in rows where !row[0].isEmpty && !row[1].isEmpty { stored[row[0]] = row[1] }
    for r in forgottenLearnedHomophones(stored, discarded: LearningDiscard.words) { stored.removeValue(forKey: r) }
    homophoneLearnedCache = stored
    return stored
}

/// 記録を書き直す (学習を覚えるとき・消すときだけ。ふだんの変換では書かない)
@MainActor func saveHomophoneLearned(_ learned: [String: String]) {
    guard personalDataAllowed else { return }
    var kept = learned
    for r in forgottenLearnedHomophones(kept, discarded: LearningDiscard.words) { kept.removeValue(forKey: r) }
    guard kept.count <= homophoneLearnedLimit else { return }
    homophoneLearnedCache = kept
    let lines = ["# 難読語の一覧の語で、利用者が最後に確定した語 (読み<TAB>語)。AI の変換でもこの語に戻す。個人のデータ (配らない)"]
        + kept.sorted { $0.key < $1.key }.map { $0.key + "\t" + $0.value }
    try? (lines.joined(separator: "\n") + "\n").data(using: .utf8)?.write(to: homophoneLearnedURL(), options: .atomic)
}

/// AI の変換の結果に、学習した一覧の語の仲間の語があれば、替えた文を zenz に採点させて決める (仮表示にはかけない)
@MainActor func choosingLearnedHomophones(_ segments: [(word: String, rubyCount: Int)], reading: String, context: String,
                                          fast: Bool) -> [(word: String, rubyCount: Int)] {
    let words = homophoneWords()
    guard !words.isEmpty else { return segments }
    let learned = homophoneLearned()
    guard !learned.isEmpty else { return segments }
    let output = segments.map(\.word).joined()
    let spots = learnedHomophoneSpots(left: context, output: output, reading: reading, words: words, learned: learned)
    guard !spots.isEmpty else { return segments }
    // 仮表示 (打っている途中の辞書だけの変換。覚えてあればそれ。表の差し替えも表示と同じにかける) に学習した語が出ていなければ、採点しない
    let rawPreview = conversionSegments(hiragana: reading, context: context, fast: true, dictionaryOnly: true)
    let tables = homophoneTables()
    let preview = (tables.isEmpty ? rawPreview
        : replacingHomophones(rawPreview, reading: reading, context: context, tables: tables, declined: homophoneDeclined()).segments).map(\.word).joined()
    let previewScalars = Array(preview.unicodeScalars)
    guard spots.contains(where: { homophoneHasWord(previewScalars, Array($0.replacement.unicodeScalars)) }) else { return segments }
    let outputs = [output] + spots.map { applyingHomophones(output, [$0]) }
    let scores = converter.zenzScoreSequences(hiragana: reading, outputs: outputs, options: getOptions(context: context, prediction: false, zenzai: true, fast: fast))
    let accepted = acceptedLearnedSpots(spots, scores: scores, bonus: homophoneLearnedBonus, preview: preview)
    if devEnvironment["AZK_DEBUG"] != nil {
        let margins = scores.flatMap { s in s.count == spots.count + 1 ? spots.indices.map { String(format: "%.2f", s[$0 + 1] - s[0]) }.joined(separator: ",") : nil } ?? "nil"
        debugWrite("homophone learned \(reading) \(spots.map { "\($0.original)->\($0.replacement)" }) margin=\(margins) accepted=\(accepted.count)\n".data(using: .utf8)!)
    }
    for r in accepted { homophoneLearnedOffered.insert(r.reading + "\t" + r.original + "\t" + r.replacement) }
    return mergingHomophoneReplacements(segments, accepted)
}

@MainActor func promotingHomophoneWords(_ texts: [String], reading: String) -> [String] {
    let words = homophoneWords()
    return words.isEmpty ? texts : promotingHomophoneWords(texts, reading: reading, words: words, learned: homophoneLearned())
}

/// 確定 (LearnSegments) で、一覧の語を選んだ記録を直す。記録するのは学習と同じとき (noteHomophoneChoices と同じ)
@MainActor func noteLearnedHomophones(readings: [String], texts: [String]) {
    let words = homophoneWords()
    guard !words.isEmpty else { return }
    let learned = homophoneLearned()
    let next = learnedHomophoneChoices(readings: readings, texts: texts, words: words, learned: learned, offered: homophoneLearnedOffered)
    if next != learned { saveHomophoneLearned(next) }
}

@MainActor func forgetLearnedHomophones(reading: String, text: String) {
    var learned = homophoneLearned()
    let forgotten = forgottenLearnedHomophones(learned, reading: reading, text: text)
    guard !forgotten.isEmpty else { return }
    for r in forgotten { learned.removeValue(forKey: r) }
    saveHomophoneLearned(learned)
}

#if AZK_DEV_KNOBS
/// 測るための入口 (リポジトリの外の測りの台本が呼ぶ。配る版には入れない): 読みと左の文脈で、表記の候補 ("\u{1}" 区切り) ごとの zenz の点 (対数確率) を
/// "\u{1}" 区切りで返す (取れなければ空)。学習の差し替えの点の差を、変換の結果とその語に替えた文で測る
@_silgen_name("ScoreOutputs")
@MainActor public func score_outputs(readingPtr: UnsafePointer<CChar>, contextPtr: UnsafePointer<CChar>, outputsPtr: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar> {
    let reading = normalizeForConversion(String(cString: readingPtr))
    let context = leftContext(String(cString: contextPtr))
    let outputs = String(cString: outputsPtr).components(separatedBy: "\u{1}")
    let scores = converter.zenzScoreSequences(hiragana: reading, outputs: outputs, options: getOptions(context: context, prediction: false, zenzai: true))
    return _strdup(scores.map { $0.map { String($0) }.joined(separator: "\u{1}") } ?? "")!
}
#endif
