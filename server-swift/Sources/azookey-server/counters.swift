import Foundation
import KanaKanjiConverterModule

/// 2026-11-02: 数詞＋助数詞の候補 (二|丁・二|十|丁) に、同じ読みのほかの助数詞 (挺・梃・張) に替えた形を足す (候補の一覧)。
/// 一覧の 4. (複数の語を組み合わせた候補) は辞書の上位 10 個だけなので、値の低い助数詞 (挺 −16.1) を使った 二挺 は出なかった
/// (にちょう の一覧は 二兆 二鳥 二丁 … だけ。拳銃を数える 挺 が選べない)。助数詞は品詞 1300、数詞は品詞 1295 (CIDData.数)
let counterCID = 1300

/// 比べるための開発用の切り替え: AZK_COUNTER_ALTERNATIVES=0 で足さない
let counterAlternativesEnabled = devEnvironment["AZK_COUNTER_ALTERNATIVES"] != "0"

/// 足す助数詞は、助数詞として実際に使われる回数 (counter_counts.tsv: 表記<TAB>回数。日本語版ウィキペディアで、数字か漢数字のすぐ後ろに来て
/// 後ろに漢字が続かない所。tools/counters/count_counters.py) が counterMinCount 以上のものだけ、回数の多い順に。
/// 辞書の値では、挺 (−16.1) が旧字体・異体字 (卷 −13.6・罐 −15.9・疊 −16.1・輛 −16.3・艚 −17.1) と同じ帯にあって分けられなかった
/// (回数では 挺 2,155・輌 4,044 に対して 罐 6・點 17・疊 2・艚 0)。表に無い表記は足さない。AZK_COUNTER_MIN_COUNT で試せる (開発用のビルドだけ)
let counterMinCount: Int = {
    if let v = devEnvironment["AZK_COUNTER_MIN_COUNT"], let n = Int(v) { return n }
    return 1000
}()

/// 表は 1,000 行ほど (上限つきで読む)
@MainActor var counterCountsCache: [String: Int]? = nil

@MainActor func counterCounts() -> [String: Int] {
    if let counts = counterCountsCache { return counts }
    var counts: [String: Int] = [:]
    for row in BoundedTable.rows(execURL.appendingPathComponent("counter_counts.tsv"), maxBytes: 1 << 20, maxLines: 20_000, columns: 2...2, maxFieldLength: 16) {
        if let n = Int(row[1]), n >= 0, !row[0].isEmpty { counts[row[0]] = n }
    }
    counterCountsCache = counts
    return counts
}

/// 漢数字 (一丁 の 一。辞書に 1 語で載る 一丁 は 数詞＋助数詞 に分かれていない)
let kanjiNumerals: Set<Character> = Set("〇一二三四五六七八九十百千万億兆壱弐参拾")

/// 数の字だけで、頭が 万・億・兆 でない (辞書で品詞が数の 冬・硬・○ や、兆乗・万冠 のような形は使わない)
func isNumeralText(_ text: String) -> Bool {
    guard let first = text.first, !"万億兆".contains(first) else { return false }
    return text.allSatisfy { kanjiNumerals.contains($0) || ("0"..."9").contains($0) || ("０"..."９").contains($0) }
}

/// 語の並びが、数詞 (1 語以上) と、最後の助数詞 1 語だけでできている
func isCounterPhrase(_ data: [DicdataElement]) -> Bool {
    guard data.count >= 2, let last = data.last, last.lcid == counterCID, last.rcid == counterCID else { return false }
    return data.dropLast().allSatisfy { $0.lcid == CIDData.数.cid && $0.rcid == CIDData.数.cid } && isNumeralText(data.dropLast().map(\.word).joined())
}

/// 数詞＋助数詞の候補の、助数詞だけを同じ読みのほかの助数詞に替えた形 (counters: 助数詞の読み (カタカナ) → 同じ読みの助数詞の表記、辞書の値の高い順)。
/// 辞書に 1 語で載る語 (一丁 イッチョウ) は、字が 漢数字＋1 字 で、その 1 字が読みの終わりと同じ読みの助数詞なら同じに扱う
func counterAlternatives(_ data: [DicdataElement], counters: (String) -> [String]) -> [String] {
    guard counterAlternativesEnabled else { return [] }
    if isCounterPhrase(data), let last = data.last {
        let numerals = data.dropLast().map(\.word).joined()
        return counters(last.ruby).filter { $0 != last.word }.map { numerals + $0 }
    }
    guard data.count == 1, let word = data.first, word.word.count >= 2, let counter = word.word.last,
          isNumeralText(String(word.word.dropLast())) else { return [] }
    let numerals = String(word.word.dropLast())
    for length in stride(from: word.ruby.count - 1, through: 1, by: -1) {
        let found = counters(String(word.ruby.suffix(length)))
        if found.contains(String(counter)) {
            return found.filter { $0 != String(counter) }.map { numerals + $0 }
        }
    }
    return []
}

/// 助数詞の読み (カタカナ) → 同じ読みの助数詞の表記 (使われる回数が counterMinCount 以上のもの、回数の多い順)。一覧を作る間だけ覚える
@MainActor func dictionaryCounters(context: String) -> (String) -> [String] {
    var memo: [String: [String]] = [:]
    let usage = counterCounts()
    return { ruby in
        if let known = memo[ruby] { return known }
        let reading = ruby.toHiragana()
        let composing = directComposingText(reading)
        let count = composing.input.count
        let words = converter.requestCandidates(composing, options: getOptions(context: context, prediction: false, zenzai: false, nBest: 50)).mainResults
            .filter { $0.correspondingCount == count && $0.data.count == 1 && $0.data[0].lcid == counterCID && $0.data[0].rcid == counterCID
                && (usage[$0.text] ?? 0) >= counterMinCount }
            .sorted { (usage[$0.text] ?? 0, $0.data[0].value()) > (usage[$1.text] ?? 0, $1.data[0].value()) }
        var seen: Set<String> = []
        let found = words.map(\.text).filter { seen.insert($0).inserted }
        if devEnvironment["AZK_DEBUG"] != nil {
            debugWrite("counters \(ruby) \(words.map { "\($0.text)(\(String(format: "%.1f", $0.data[0].value())))" }.joined(separator: " "))\n".data(using: .utf8)!)
        }
        memo[ruby] = found
        return found
    }
}

/// 2026-11-06 (数字＋かい): 比べるための開発用の切り替え: AZK_DIGIT_COUNTER=0 で前の形
let digitCounterEnabled = devEnvironment["AZK_DIGIT_COUNTER"] != "0"

/// 数字だけの語 (6・１２)
func isDigitText(_ text: String) -> Bool {
    !text.isEmpty && text.allSatisfy { ("0"..."9").contains($0) || ("０"..."９").contains($0) }
}

/// (2a) 辞書だけの変換で、数字のすぐ後ろが助数詞でない語 (6会・3会にある・あと6会で終わる) になっていたら、同じ候補の並び (results、値の高い順) のうち、
/// 数字までの表記と、助数詞の後ろの表記が同じで、数字のすぐ後ろが助数詞 (品詞 1300) の候補に替える (6回・3回にある・あと6回で終わる)。
/// 辞書では 数字 (品詞 1285 の名詞) ＋ 会 (1298) の値が 数字 (1295 の数) ＋ 回 (1300) より高く、6かい がいつも 6会 だった。会 は助数詞として使われない
/// (counter_counts.tsv に無い)。数字のあとが 1 語の名詞 (2会社・1会議) のときは、同じ形の助数詞の候補が無いので替わらない
/// 替えるのは、替える先の助数詞が助数詞として使われる回数 (usage) が counterMinCount 以上で、元の語の 10 倍以上、値の差が 2 以内のときだけ
/// (10分の1 → 10文の1・4色 → 4食・1型 → 1ガタ・2種類 → 2首類・4門 → 4問 は替えない。分・色・型 は数字のあとでよく使われる)
func preferringCounterAfterDigit(_ chosen: Candidate, results: [Candidate], usage: [String: Int], minimum: Int = counterMinCount) -> Candidate? {
    let data = chosen.data
    guard let i = data.indices.first(where: { k in k + 1 < data.count && isDigitText(data[k].word)
        && data[k + 1].lcid != counterCID && !CIDData.isJoshi(cid: data[k + 1].lcid) }) else { return nil }
    let prefix = data[...i].map(\.word).joined()
    let prefixRuby = data[...i].map(\.ruby).joined()
    let rest = String(chosen.text.dropFirst(prefix.count))
    for candidate in results where candidate.correspondingCount == chosen.correspondingCount && candidate.text != chosen.text {
        let other = candidate.data
        guard let j = other.indices.first(where: { k in k + 1 < other.count && isDigitText(other[k].word) }),
              other[...j].map(\.word).joined() == prefix, other[...j].map(\.ruby).joined() == prefixRuby,
              other[j + 1].lcid == counterCID else { continue }
        let suffix = other[(j + 2)...].map(\.word).joined()
        guard rest.hasSuffix(suffix), rest.count > suffix.count else { continue }
        let counter = usage[other[j + 1].word] ?? 0
        guard counter >= minimum, counter >= 10 * (usage[data[i + 1].word] ?? 0), chosen.value - candidate.value <= 2 else { return nil }
        return candidate
    }
    return nil
}

/// 2026-11-22 (数字＋がつ): 比べるための開発用の切り替え: AZK_KATAKANA_COUNTER=0 で前の形
let katakanaCounterFixEnabled = devEnvironment["AZK_KATAKANA_COUNTER"] != "0"

/// カタカナ (長音を含む) だけの字
func isKatakanaOnly(_ text: String) -> Bool {
    !text.isEmpty && text.unicodeScalars.allSatisfy { (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC }
}

/// (2c) 辞書だけの変換で、数字 (算用数字・漢数字) のすぐ後ろの助数詞 (品詞 1300) が、カタカナだけ (2 字以上) で読みと同じ字 (11|ガツ) なら、
/// 同じ読みの漢字の助数詞 (回数 1,000 以上) が 1 つだけならその字 (topCounter。月) に替える (2023年11ガツには → 2023年11月には)。
/// 字を 1 つ替えるだけで、区切りと品詞は変えない。
/// 辞書では カタカナの ガツ (助数詞 1300、意味の番号 501、点 −15.33) が 月 (1300、意味 228、−13.21) と同じ所に並び、語とつなぎの点は 月 が上なのに、
/// 文節どうしの意味のつなぎで ガツ が勝っていた (年・同年 のあと。学習が空の dev の N月 を含む 80 文で 33 か所、仮表示に出る)
/// 守り (11-22 C の見直し): counter_counts.tsv にカタカナの助数詞の行は無いので、回数ではカタカナの助数詞を守れない。
///   - 替える先の辞書の値 (学習を除く) が、元のカタカナの値より高いときだけ替える (ページ −7.4・メートル −6.4・シリング は 頁・米・志 より高いので替えない)
///   - 1 字のカタカナ (カ・キ・テ・マ) は替えない (3カ所 が カ＋所 に分かれたとき 3日所 にしない)
func replacingKatakanaCounterAfterDigit(_ chosen: Candidate, topCounter: (String) -> (word: String, value: PValue)?) -> Candidate? {
    var replaced = chosen.data
    var changed = false
    for k in replaced.indices where k + 1 < replaced.count && (isDigitText(replaced[k].word) || isNumeralText(replaced[k].word)) {
        let e = replaced[k + 1]
        guard e.lcid == counterCID, e.word == e.ruby, e.word.count >= 2, isKatakanaOnly(e.word),
              let top = topCounter(e.ruby), top.word != e.word, top.value > e.value(),
              top.word.unicodeScalars.contains(where: { (0x4E00...0x9FFF).contains($0.value) }) else { continue }
        replaced[k + 1] = DicdataElement(word: top.word, ruby: e.ruby, lcid: e.lcid, rcid: e.rcid, mid: e.mid, value: e.value())
        changed = true
    }
    guard changed else { return nil }
    return Candidate(text: replaced.map(\.word).joined(), value: chosen.value, correspondingCount: chosen.correspondingCount,
                     lastMid: chosen.lastMid, data: replaced)
}

/// (2c) の替える先: 読み (カタカナ) が同じ助数詞のうち、漢字を含み、助数詞として数えた表 (usage) で minimum 回以上の字が 1 つだけなら、その字と辞書の値。
/// 2 つ以上あれば nil (11-22 C の見直し: ショク → 色 (ほかに 食)・ダン → 弾 (段・団)・カン → 巻 (貫・冠)・タマ → 球 (玉) のように、
/// 回数の 1 番の字は意味の違う字になりうる。替える前のカタカナは「変換しきれていない」と見えるが、替えると違う漢字が正しそうに見える)
func uniqueKanjiCounter(_ values: [String: PValue], usage: [String: Int], minimum: Int = counterMinCount) -> (word: String, value: PValue)? {
    let kanji = values.keys.filter { w in (usage[w] ?? 0) >= minimum && w.unicodeScalars.contains(where: { (0x4E00...0x9FFF).contains($0.value) }) }
    guard kanji.count == 1, let word = kanji.first, let value = values[word] else { return nil }
    return (word: word, value: value)
}

/// (2c) の替える先を引く: 数字を付けた読み (1がつ) を辞書で引き、数字のすぐ後ろに 1 語で来る漢字の字とその辞書の値 (学習の要素は除く) を、
/// 品詞を問わずに集め (助数詞の 巻 と、接尾の名詞などで来る 冠・貫 を両方数えるため)、uniqueKanjiCounter で決める。読みごとに覚える
@MainActor var bestSameReadingCounterMemo: [String: (word: String, value: PValue)?] = [:]

@MainActor func bestSameReadingCounter(_ ruby: String, usage: [String: Int]) -> (word: String, value: PValue)? {
    if let known = bestSameReadingCounterMemo[ruby] { return known }
    let composing = directComposingText("1" + ruby.toHiragana())
    let count = composing.input.count
    var values: [String: PValue] = [:]
    for c in converter.requestCandidates(composing, options: getOptions(context: "", prediction: false, zenzai: false, nBest: 50)).mainResults
    where c.correspondingCount == count && c.data.count == 2 && isDigitText(c.data[0].word)
        && !c.data[1].metadata.contains(.isLearned) {
        values[c.data[1].word] = max(values[c.data[1].word] ?? -.infinity, c.data[1].value())
    }
    let found = uniqueKanjiCounter(values, usage: usage)
    if bestSameReadingCounterMemo.count >= 1024 { bestSameReadingCounterMemo.removeAll(keepingCapacity: true) }
    bestSameReadingCounterMemo[ruby] = .some(found)
    return found
}

/// (2b) 文の頭 (左の文脈が無いか、文の終わりの記号のあと) で、数字＋助数詞 のあとが助詞・助動詞・記号だけ (6階は？・3階・3回です) のとき、
/// 同じ読みの助数詞のうち、使われる回数 (counter_counts.tsv) がいまの字の 3 倍以上のものに替える (回 549,722・階 84,487 → 6回は？・3回)。
/// 後ろに語が続く (3階にある・6階の部屋) か、左に語がある (地下2階・ビルの3階) ときは、変換の選んだ字のまま。替えた字は候補の一覧の 2 番目以降に残る
@MainActor func choosingCounterByUsage(_ chosen: Candidate, context: String) -> Candidate? {
    let data = chosen.data
    let usage = counterCounts()
    // 助数詞は、品詞が助数詞か、助数詞として数えた表にある字 (AI の変換では 6階 の 階 が名詞の 1298 で来ることがある)
    guard data.count >= 2, isDigitText(data[0].word), data[1].lcid == counterCID || usage[data[1].word] != nil,
          context.isEmpty || context.last.map({ "。．.！!？?\n".contains($0) }) == true else { return nil }
    func light(_ e: DicdataElement) -> Bool {
        ((147...554).contains(e.lcid) && (147...554).contains(e.rcid)) || e.word.allSatisfy(isLiteralSymbol)
    }
    guard data.dropFirst(2).allSatisfy(light) else { return nil }
    let current = data[1]
    guard let top = sameReadingCounters(current.ruby, usage: usage).first, top != current.word,
          (usage[top] ?? 0) >= 3 * max(usage[current.word] ?? 0, 1) else { return nil }
    var replaced = data
    replaced[1] = DicdataElement(word: top, ruby: current.ruby, lcid: current.lcid, rcid: current.rcid, mid: current.mid, value: current.value())
    return Candidate(text: replaced.map(\.word).joined(), value: chosen.value, correspondingCount: chosen.correspondingCount,
                     lastMid: chosen.lastMid, data: replaced)
}

/// 読み (カタカナ) が同じ助数詞のうち、助数詞として数えた表 (usage) で counterMinCount 回以上のもの、回数の多い順。読みごとに覚える。
/// 数字を付けた読み (1かい) を辞書で引き、数字のすぐ後ろが助数詞 (品詞 1300) になった字だけを使う
/// (かい を単独で引くと助数詞の 回 が並ばない。助数詞でない字は数えない: 1がた の 方 (数えた 方 の多くは ほう・かた の読み) で 1型 → 1方 にしていた)
@MainActor var sameReadingCountersMemo: [String: [String]] = [:]

@MainActor func sameReadingCounters(_ ruby: String, usage: [String: Int]) -> [String] {
    if let known = sameReadingCountersMemo[ruby] { return known }
    let composing = directComposingText("1" + ruby.toHiragana())
    let count = composing.input.count
    let words = Set(converter.requestCandidates(composing, options: getOptions(context: "", prediction: false, zenzai: false, nBest: 50)).mainResults
        .filter { $0.correspondingCount == count && $0.data.count == 2 && isDigitText($0.data[0].word) && $0.data[1].lcid == counterCID }
        .map(\.data[1].word))
    let found = words.filter { (usage[$0] ?? 0) >= counterMinCount }.sorted { (usage[$0] ?? 0, $0) > (usage[$1] ?? 0, $1) }
    if sameReadingCountersMemo.count >= 1024 { sameReadingCountersMemo.removeAll(keepingCapacity: true) }
    sameReadingCountersMemo[ruby] = found
    return found
}
