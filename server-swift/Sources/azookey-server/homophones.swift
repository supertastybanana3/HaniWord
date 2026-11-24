import Foundation
import KanaKanjiConverterModule

/// 2026-11-02: 難読語の同音語の差し替え。AI (zenz) は、知らない難読語より知っている同音語を選ぶ (当局の慫慂 → 当局の商用、蹉跌したら → 砂鉄したら)。
/// 辞書に連語を足しても AI が最後に選び直すので、変換の結果を返す前に、表にある所だけ同音語を難読語に戻す。
/// 表は 2 つ (日本語版ウィキペディアと青空文庫で数えた。出典は extra_dictionaries/LICENSE.txt):
///   homophone_phrases.tsv  連語: 替える前の連語<TAB>替えた後の連語<TAB>語の位置<TAB>替える前の語<TAB>替えた後の語<TAB>読み
///   homophone_triggers.tsv 目印: 「pair<TAB>読み<TAB>難読語<TAB>相手<TAB>事前」の行の下に「feat<TAB>目印<TAB>重み」の行 ($目印 は文脈の終わりだけ)
/// 当てはめ方は、表を作った側の手本 (ref_apply.py) と同じ答えになるようにしてある (字の位置は Unicode のスカラー値で数える):
///   1. 連語: 変換した文字列の中の語 (前後に漢字が続かないもの) が、左の文脈とつないだ文字列で表の連語の一部になっていれば替える。
///      打った読みに語の読みが無ければ替えない。後ろの 1 字だけの行 (蹉跌し) は、その後ろが さえ・さま・さん・しか・しも・すら・せい なら替えない
///   2. 目印: 連語で替えなかった語は、語の前の 120 字にある目印の重みの和に事前を足し、しきい値 (6.0) 以上なら替える
/// 仮表示・清書・Space (ConvertSegments) と候補の一覧 (ListCandidates) の両方で、結果を返す前にかける (表示が切り替わる回数は増やさない)。
/// 替えた所で利用者が元の語を選んで確定したら、その組 (読みと元の語) は以後替えない (学習の場所の homophone_declined.txt)
/// 表を引く鍵: 9 字までの字の並び (スカラー値を 21 ビットずつ詰める)。String の鍵は引くたびに正規化して遅いので使わない。
/// 文字列に NUL は来ないので、詰めた 0 で長さが分かる。目印・語は 9 字まで (今の表は 6 字まで。長いものは読まない)
struct HomophoneKey: Hashable, Comparable {
    private var a: UInt64 = 0, b: UInt64 = 0, c: UInt64 = 0
    static let maxLength = 9

    init?<C: Collection>(_ scalars: C) where C.Element == Unicode.Scalar {
        guard scalars.count <= Self.maxLength else { return nil }
        for (i, s) in scalars.enumerated() {
            let v = UInt64(s.value) << UInt64(21 * (i % 3))
            switch i / 3 {
            case 0: a |= v
            case 1: b |= v
            default: c |= v
            }
        }
    }

    init?(_ text: String) { self.init(text.unicodeScalars) }

    /// 2026-11-19: 目印を並べた配列で二分探索するための順 (意味のある順ではない)
    static func < (l: HomophoneKey, r: HomophoneKey) -> Bool { (l.a, l.b, l.c) < (r.a, r.b, r.c) }
}

/// 2026-11-19 (仕事 1・メモリ): 連語と読みの字は、表の 1 本の配列 (HomophoneTables.scalars) の中の位置で持つ。連語のすぐ後ろに読みを並べる。
/// 前は行ごとに配列を 2 つ持ち、6.4 万行で 10MB を超えていた。字の並びは同じなので、引いた結果は変わらない
struct HomophonePhrase {
    let start: Int32
    let phraseCount: Int16
    let readingCount: Int16
    /// 連語の中で語が始まる位置
    let offset: Int16
    /// 語の後ろ 1 字だけの行 (蹉跌し)
    let suffixOnly: Bool
    let replacement: String
}

/// 2026-11-19 (仕事 1・メモリ): 目印 → (表の中の順, 重み) を、鍵の順に並べた配列で持ち、二分探索で引く。
/// 前は組ごとの Swift の辞書で、10.4 万個で約 8MB。同じ鍵には同じ (順, 重み) が返るので、和は前と同じ
struct HomophoneFeatures {
    private var keys: [HomophoneKey] = []
    private var orders: [Int32] = []
    private var weights: [Double] = []
    var count: Int { keys.count }

    init() {}

    init(_ table: [HomophoneKey: (order: Int, weight: Double)]) {
        let sorted = table.sorted { $0.key < $1.key }
        keys.reserveCapacity(sorted.count); orders.reserveCapacity(sorted.count); weights.reserveCapacity(sorted.count)
        for (key, value) in sorted {
            keys.append(key); orders.append(Int32(value.order)); weights.append(value.weight)
        }
    }

    subscript(key: HomophoneKey) -> (order: Int, weight: Double)? {
        var lo = 0, hi = keys.count
        while lo < hi {
            let mid = (lo + hi) / 2
            if keys[mid] < key { lo = mid + 1 } else { hi = mid }
        }
        guard lo < keys.count, keys[lo] == key else { return nil }
        return (Int(orders[lo]), weights[lo])
    }
}

struct HomophonePair {
    let reading: [Unicode.Scalar]
    let replacement: String
    let prior: Double
    /// 目印 → (表の中の順, 重み)。和は表の順に足す (手本と同じ丸めになるように)
    var features = HomophoneFeatures()
    var suffixes = HomophoneFeatures()
    var longestFeature = 0
    var longestSuffix = 0
}

struct HomophoneTables {
    /// 替える前の語 → (表に初めて出た順, 語, 連語の行)
    var phrases: [HomophoneKey: (order: Int, word: String, rules: [HomophonePhrase])] = [:]
    /// 連語の行の字 (連語、すぐ後ろに読み)。HomophonePhrase は位置だけを持つ
    var scalars: [Unicode.Scalar] = []
    /// 相手の語 → (表に初めて出た順, 語, 組)
    var pairs: [HomophoneKey: (order: Int, word: String, pairs: [HomophonePair])] = [:]
    var threshold = 6.0
    /// 替える前の語の長さ (スカラー値) の最大
    var longestWord = 0
    var isEmpty: Bool { phrases.isEmpty && pairs.isEmpty }

    func phrase(_ rule: HomophonePhrase) -> ArraySlice<Unicode.Scalar> {
        scalars[Int(rule.start)..<(Int(rule.start) + Int(rule.phraseCount))]
    }

    func reading(_ rule: HomophonePhrase) -> ArraySlice<Unicode.Scalar> {
        let start = Int(rule.start) + Int(rule.phraseCount)
        return scalars[start..<(start + Int(rule.readingCount))]
    }

    /// 替える前の語 → 替えた後の語 (利用者が替えた後の語を選んだかを見る)
    func replacements(of word: String) -> Set<String> {
        guard let key = HomophoneKey(word) else { return [] }
        return Set((phrases[key]?.rules.map(\.replacement) ?? []) + (pairs[key]?.pairs.map(\.replacement) ?? []))
    }
}

struct HomophoneReplacement: Equatable {
    /// 変換した文字列の中の位置 (スカラー値)
    let start: Int
    let original: String
    let replacement: String
    /// 語の読み (表の読み)
    let reading: String
    let reason: String
    var length: Int { original.unicodeScalars.count }
}

/// 表は上限つきで読む (BoundedTable.scalarRows)。配る表は 連語 6.5 万行・3.8 MB、目印 10.5 万行・2 MB、1 欄 10 字まで
let homophoneLimits = (maxBytes: 16 << 20, maxLines: 400_000, maxFieldLength: 64)
let homophoneContextLength = 120
let homophoneNotSuru: Set<HomophoneKey> = Set(["さえ", "さま", "さん", "しか", "しも", "すら", "せい"].compactMap { HomophoneKey($0) })

func homophoneString<S: Sequence>(_ scalars: S) -> String where S.Element == Unicode.Scalar {
    var view = String.UnicodeScalarView()
    view.append(contentsOf: scalars)
    return String(view)
}

/// 手本の漢字 ([々〆ヵヶ㐀-䶿一-鿿豈-﫿])。語の前後にこれが続けば長い語の一部とみなす
func isHomophoneKanji(_ s: Unicode.Scalar) -> Bool {
    let v = s.value
    return v == 0x3005 || v == 0x3006 || v == 0x30F5 || v == 0x30F6 || (0x3400...0x4DBF).contains(v) || (0x4E00...0x9FFF).contains(v)
        || (0xF900...0xFAFF).contains(v)
}

func homophoneContains<H: RandomAccessCollection, N: RandomAccessCollection>(_ haystack: H, _ needle: N) -> Bool
    where H.Element == Unicode.Scalar, N.Element == Unicode.Scalar, H.Index == Int, N.Index == Int {
    guard let first = needle.first else { return true }
    if needle.count > haystack.count { return false }
    let base = haystack.startIndex
    for start in base...(haystack.endIndex - needle.count) where haystack[start] == first {
        if haystack[start..<(start + needle.count)].elementsEqual(needle) { return true }
    }
    return false
}

/// Python の sum (3.12 から、浮動小数の和は Neumaier の方法で誤差を補う)。手本と同じ値にして、しきい値ちょうどの所の答えをそろえる
func homophoneSum(_ values: [Double]) -> Double {
    guard var total = values.first else { return 0 }
    var compensation = 0.0
    for x in values.dropFirst() {
        let t = total + x
        compensation += abs(total) >= abs(x) ? (total - t) + x : (x - t) + total
        total = t
    }
    if compensation != 0 && compensation.isFinite { total += compensation }
    return total
}

/// 表の数の欄 (語の位置・重み) を、String を作らずに読む。重みは「-3.2966」の形 (指数なし・有効数字 15 桁まで) なら、
/// 整数を 10 の累乗で割る (どちらも正確に表せるので、割り算の丸め 1 回で Double(String) と同じ値になる)。ほかの形は Double(String) に任せる
func homophoneInt(_ scalars: ArraySlice<Unicode.Scalar>) -> Int? {
    guard !scalars.isEmpty, scalars.count <= 9, scalars.allSatisfy({ ("0"..."9").contains($0) }) else { return nil }
    return scalars.reduce(0) { $0 * 10 + Int($1.value - 0x30) }
}

func homophoneDouble(_ scalars: ArraySlice<Unicode.Scalar>) -> Double? {
    var digits: Int64 = 0, count = 0, decimals = 0, seenPoint = false
    var body = scalars[...]
    let negative = body.first == "-"
    if negative || body.first == "+" { body = body.dropFirst() }
    for s in body {
        if s == "." && !seenPoint { seenPoint = true; continue }
        guard ("0"..."9").contains(s), count < 15 else { return Double(homophoneString(scalars)).flatMap { $0.isFinite ? $0 : nil } }
        digits = digits * 10 + Int64(s.value - 0x30)
        count += 1
        if seenPoint { decimals += 1 }
    }
    guard count > 0 else { return nil }
    let value = Double(digits) / [1, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15][decimals]
    return negative ? -value : value
}

func parseHomophoneTables(phrases: String?, triggers: String?) -> HomophoneTables {
    var tables = HomophoneTables()
    let limits = homophoneLimits
    // 同じ語 (替えた後の語・元の語) の String は 1 回だけ作る
    var strings: [HomophoneKey: String] = [:]
    func string(_ scalars: ArraySlice<Unicode.Scalar>) -> String {
        guard let key = HomophoneKey(scalars) else { return homophoneString(scalars) }
        if let known = strings[key] { return known }
        let made = homophoneString(scalars)
        strings[key] = made
        return made
    }
    if let phrases {
        BoundedTable.scalarRows(text: phrases, maxLines: limits.maxLines, columns: 6...6, maxFieldLength: limits.maxFieldLength) { parts in
            let phrase = Array(parts[0]), word = parts[3]
            guard let offset = homophoneInt(parts[2]), !word.isEmpty, !parts[4].isEmpty, offset + word.count <= phrase.count,
                  phrase[offset..<(offset + word.count)].elementsEqual(word), let key = HomophoneKey(word),
                  // 2026-11-19: 位置は Int16・Int32 で持つので、入らない行は飛ばす (1 欄の字数の上限に頼らない)
                  phrase.count <= Int(Int16.max), parts[5].count <= Int(Int16.max), offset <= Int(Int16.max),
                  tables.scalars.count + phrase.count + parts[5].count <= Int(Int32.max) else { return }
            let rule = HomophonePhrase(start: Int32(tables.scalars.count), phraseCount: Int16(phrase.count), readingCount: Int16(parts[5].count),
                                       offset: Int16(offset), suffixOnly: offset == 0 && phrase.count == word.count + 1, replacement: string(parts[4]))
            tables.scalars.append(contentsOf: phrase)
            tables.scalars.append(contentsOf: parts[5])
            let order = tables.phrases.count
            tables.phrases[key, default: (order: order, word: string(word), rules: [])].rules.append(rule)
            tables.longestWord = max(tables.longestWord, word.count)
        }
    }
    // 伸ばしながら足した配列の余りを返す (字の数ちょうどの配列に写す)
    var packed: [Unicode.Scalar] = []
    packed.reserveCapacity(tables.scalars.count)
    packed.append(contentsOf: tables.scalars)
    tables.scalars = packed
    guard let triggers else { return tables }
    for line in triggers.utf8.split(separator: UInt8(ascii: "\n")) where line.starts(with: "# しきい値".utf8) {
        let fields = String(decoding: line, as: UTF8.self).split(whereSeparator: \.isWhitespace)
        if fields.count >= 3, let value = Double(fields[2]), value.isFinite { tables.threshold = value }
    }
    // 組は作り終えてから表に入れる (表の中の組を直すと、目印を足すたびに目印の表を写し直して遅い)。
    // 目印は読む間だけ辞書で持ち (同じ目印が 2 度出たら、順は初めの、重みは後の)、組を表に入れるときに並べた配列にする
    typealias FeatureTable = [HomophoneKey: (order: Int, weight: Double)]
    var current: (key: HomophoneKey, word: String, pair: HomophonePair, features: FeatureTable, suffixes: FeatureTable)? = nil
    func flush() {
        guard var current else { return }
        current.pair.features = HomophoneFeatures(current.features)
        current.pair.suffixes = HomophoneFeatures(current.suffixes)
        let order = tables.pairs.count
        tables.pairs[current.key, default: (order: order, word: current.word, pairs: [])].pairs.append(current.pair)
        tables.longestWord = max(tables.longestWord, current.word.unicodeScalars.count)
    }
    BoundedTable.scalarRows(text: triggers, maxLines: limits.maxLines, columns: 3...5, maxFieldLength: limits.maxFieldLength) { parts in
        if parts[0].elementsEqual("pair".unicodeScalars) {
            flush()
            current = nil
            guard parts.count == 5, let prior = homophoneDouble(parts[4]), !parts[2].isEmpty, !parts[3].isEmpty,
                  let key = HomophoneKey(parts[3]) else { return }
            current = (key, string(parts[3]), HomophonePair(reading: Array(parts[1]), replacement: string(parts[2]), prior: prior), [:], [:])
        } else if parts[0].elementsEqual("feat".unicodeScalars), parts.count == 3, current != nil, let weight = homophoneDouble(parts[2]) {
            let isSuffix = parts[1].first == "$"
            let feature = isSuffix ? parts[1].dropFirst() : parts[1]
            guard let key = HomophoneKey(feature) else { return }
            if isSuffix {
                current!.suffixes[key] = (order: current!.suffixes[key]?.order ?? current!.suffixes.count, weight: weight)
                current!.pair.longestSuffix = max(current!.pair.longestSuffix, feature.count)
            } else {
                current!.features[key] = (order: current!.features[key]?.order ?? current!.features.count, weight: weight)
                current!.pair.longestFeature = max(current!.pair.longestFeature, feature.count)
            }
        }
    }
    flush()
    return tables
}

/// 目印の和 (事前 + 文脈のどこかにある目印 + 文脈の終わりの目印)。1 つの目印は 1 回だけ数える
func homophoneScore(_ pair: HomophonePair, context: ArraySlice<Unicode.Scalar>) -> Double {
    let empty = HomophoneKey([Unicode.Scalar]())!
    var hits: [Int: Double] = [:]
    if let hit = pair.features[empty] { hits[hit.order] = hit.weight }
    for start in context.indices {
        for length in stride(from: 1, through: min(pair.longestFeature, context.endIndex - start), by: 1) {
            if let hit = pair.features[HomophoneKey(context[start..<(start + length)])!] { hits[hit.order] = hit.weight }
        }
    }
    var suffixHits: [Int: Double] = [:]
    for length in 0...min(pair.longestSuffix, context.count) {
        if let hit = pair.suffixes[HomophoneKey(context.suffix(length))!] { suffixHits[hit.order] = hit.weight }
    }
    return pair.prior + homophoneSum(hits.sorted { $0.key < $1.key }.map(\.value)) + homophoneSum(suffixHits.sorted { $0.key < $1.key }.map(\.value))
}

/// 変換した文字列 (output) の中で替える語。左の文脈 (left) は見るだけで替えない。declined (読み<TAB>元の語) の組は替えない。
/// 重なる所は前のほうだけ (手本は重なりを考えないが、表の語では起きない)
func homophoneReplacements(left: String, output: String, reading: String, tables: HomophoneTables,
                           declined: Set<String> = []) -> [HomophoneReplacement] {
    guard !tables.isEmpty, !output.isEmpty else { return [] }
    let out = Array(output.unicodeScalars)
    let base = left.unicodeScalars.count
    let text = Array(left.unicodeScalars) + out
    let typed = Array(reading.unicodeScalars)
    func wholeWord(_ p: Int, _ length: Int) -> Bool {
        !(p > 0 && isHomophoneKanji(text[p - 1])) && !(p + length < text.count && isHomophoneKanji(text[p + length]))
    }
    var found: [HomophoneReplacement] = []
    var end = 0
    for i in out.indices where i >= end {
        let p = base + i
        let lengths = 1...min(tables.longestWord, out.count - i)
        // 1. 連語。同じ所に当たる語が 2 つあれば、表の後ろの語 (手本は語ごとに上書きする)
        var best: (order: Int, replacement: HomophoneReplacement)? = nil
        for length in lengths {
            guard let entry = tables.phrases[HomophoneKey(out[i..<(i + length)])!], best.map({ entry.order > $0.order }) ?? true,
                  wholeWord(p, length) else { continue }
            for rule in entry.rules where homophoneContains(typed, tables.reading(rule)) {
                let phrase = tables.phrase(rule)
                let start = p - Int(rule.offset)
                guard start >= 0, start + phrase.count <= text.count,
                      text[start..<(start + phrase.count)].elementsEqual(phrase) else { continue }
                let after = start + phrase.count
                if rule.suffixOnly && after < text.count && homophoneNotSuru.contains(HomophoneKey(text[(after - 1)...after])!) { continue }
                let reading = homophoneString(tables.reading(rule))
                if declined.contains(reading + "\t" + entry.word) { continue }
                best = (entry.order, HomophoneReplacement(start: i, original: entry.word, replacement: rule.replacement, reading: reading,
                                                          reason: "連語 " + homophoneString(phrase)))
                break
            }
        }
        if let best {
            found.append(best.replacement)
            end = i + best.replacement.length
            continue
        }
        // 2. 目印。同じ所に当たる語が 2 つあれば、表の前の語
        let entries = lengths.compactMap { length in tables.pairs[HomophoneKey(out[i..<(i + length)])!].map { (length: length, entry: $0) } }
        search: for (length, entry) in entries.sorted(by: { $0.entry.order < $1.entry.order }) where wholeWord(p, length) {
            let context = text[max(0, p - homophoneContextLength)..<p]
            for pair in entry.pairs where homophoneContains(typed, pair.reading) {
                let reading = homophoneString(pair.reading)
                if declined.contains(reading + "\t" + entry.word) { continue }
                let score = homophoneScore(pair, context: context)
                if score >= tables.threshold {
                    found.append(HomophoneReplacement(start: i, original: entry.word, replacement: pair.replacement, reading: reading,
                                                      reason: "目印 " + String(format: "%.1f", score)))
                    end = i + length
                    break search
                }
            }
        }
    }
    return found
}

func applyingHomophones(_ output: String, _ replacements: [HomophoneReplacement]) -> String {
    var scalars = Array(output.unicodeScalars)
    for r in replacements.reversed() {
        scalars.replaceSubrange(r.start..<(r.start + r.length), with: r.replacement.unicodeScalars)
    }
    return homophoneString(scalars)
}

/// 文節の並びに差し替えをかける。文節の読みの字数 (rubyCount) は変えない。
/// 語が文節をまたぐとき (AI が 帰|趨 のように切ったとき) は、またいだ文節を 1 つにまとめる
func replacingHomophones(_ segments: [(word: String, rubyCount: Int)], reading: String, context: String, tables: HomophoneTables,
                         declined: Set<String> = []) -> (segments: [(word: String, rubyCount: Int)], replacements: [HomophoneReplacement]) {
    let found = homophoneReplacements(left: context, output: segments.map(\.word).joined(), reading: reading, tables: tables, declined: declined)
    return (mergingHomophoneReplacements(segments, found), found)
}

/// 文節の並びの中の差し替え (位置は文節をつないだ文字列のスカラー値) をかける。またいだ文節は 1 つにまとめ、読みの字数の和は変えない
func mergingHomophoneReplacements(_ segments: [(word: String, rubyCount: Int)], _ found: [HomophoneReplacement]) -> [(word: String, rubyCount: Int)] {
    guard !found.isEmpty else { return segments }
    let joined = Array(segments.map(\.word).joined().unicodeScalars)
    var starts: [Int] = []
    var at = 0
    for segment in segments {
        starts.append(at)
        at += segment.word.unicodeScalars.count
    }
    func segmentIndex(_ position: Int) -> Int {
        segments.indices.last(where: { starts[$0] <= position && position < starts[$0] + segments[$0].word.unicodeScalars.count }) ?? 0
    }
    var mergeNext = [Bool](repeating: false, count: segments.count)
    for r in found {
        for k in segmentIndex(r.start)..<segmentIndex(r.start + r.length - 1) { mergeNext[k] = true }
    }
    var result: [(word: String, rubyCount: Int)] = []
    var k = 0
    while k < segments.count {
        var j = k
        while j + 1 < segments.count && mergeNext[j] { j += 1 }
        let lo = starts[k], hi = j + 1 < segments.count ? starts[j + 1] : at
        var scalars = Array(joined[lo..<hi])
        for r in found.reversed() where lo <= r.start && r.start + r.length <= hi {
            scalars.replaceSubrange((r.start - lo)..<(r.start - lo + r.length), with: r.replacement.unicodeScalars)
        }
        result.append((word: homophoneString(scalars), rubyCount: segments[k...j].reduce(0) { $0 + $1.rubyCount }))
        k = j + 1
    }
    return result
}

/// 候補の一覧: 1 番目の候補を替えたら、替えた形を 1 番に、元の形を 2 番に置く (Space で元の語に戻せるように)
func orderingHomophones(_ texts: [String], reading: String, context: String, tables: HomophoneTables,
                        declined: Set<String> = []) -> (texts: [String], replacements: [HomophoneReplacement]) {
    guard let first = texts.first else { return (texts, []) }
    let found = homophoneReplacements(left: context, output: first, reading: reading, tables: tables, declined: declined)
    guard !found.isEmpty else { return (texts, []) }
    let replaced = applyingHomophones(first, found)
    return ([replaced] + texts.filter { $0 != replaced }, found)
}

/// 字の並び (確定した文・選んだ候補) に、語が語として (前後に漢字が続かずに) あるか
func homophoneHasWord(_ text: [Unicode.Scalar], _ word: [Unicode.Scalar]) -> Bool {
    guard !word.isEmpty, word.count <= text.count else { return false }
    for start in 0...(text.count - word.count) where text[start..<(start + word.count)].elementsEqual(word) {
        if !(start > 0 && isHomophoneKanji(text[start - 1])) && !(start + word.count < text.count && isHomophoneKanji(text[start + word.count])) {
            return true
        }
    }
    return false
}

/// 確定した文 (読みと字) で、この入力で替えた語を利用者が元の語に戻していたか。
/// offered (読み<TAB>元の語<TAB>替えた語) のうち、読みがあり、元の語が語として残り、替えた語が無いものの「読み<TAB>元の語」を返す
func declinedHomophones(reading: String, text: String, offered: Set<String>) -> Set<String> {
    let typed = Array(reading.unicodeScalars), committed = Array(text.unicodeScalars)
    var result: Set<String> = []
    for entry in offered {
        let parts = entry.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3, homophoneContains(typed, Array(parts[0].unicodeScalars)), homophoneHasWord(committed, Array(parts[1].unicodeScalars)),
              !homophoneContains(committed, Array(parts[2].unicodeScalars)) else { continue }
        result.insert(parts[0] + "\t" + parts[1])
    }
    return result
}

/// 前に元の語に戻した組 (declined) のうち、今度は利用者が替えた後の語を確定したもの
func acceptedHomophones(reading: String, text: String, declined: Set<String>, tables: HomophoneTables) -> Set<String> {
    let typed = Array(reading.unicodeScalars), committed = Array(text.unicodeScalars)
    return declined.filter { entry in
        let parts = entry.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
        return parts.count == 2 && homophoneContains(typed, Array(parts[0].unicodeScalars))
            && tables.replacements(of: parts[1]).contains { homophoneContains(committed, Array($0.unicodeScalars)) }
    }
}

/// 学習の消し方で一緒に消す記録: 候補の一覧で忘れた候補 (Ctrl+Delete。読み → 表記) に読みと元の語が入っている組と、
/// 学習から外す語 (learning_ng.txt。その字を含む語は覚えない) を含む元の語の組
func forgottenHomophones(declined: Set<String>, reading: String? = nil, text: String? = nil, discarded: [String] = []) -> Set<String> {
    let typed = reading.map { Array($0.unicodeScalars) }, chosen = text.map { Array($0.unicodeScalars) }
    return declined.filter { entry in
        let parts = entry.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 2 else { return true }
        if discarded.contains(where: { !$0.isEmpty && parts[1].contains($0) }) { return true }
        guard let typed, let chosen else { return false }
        return homophoneContains(typed, Array(parts[0].unicodeScalars)) && homophoneHasWord(chosen, Array(parts[1].unicodeScalars))
    }
}

/// 表 (16 万行) は読むのに 0.4 秒ほどかかるので、Initialize で別のスレッドに読ませ、はじめて使うときに読み終わるのを待つ
final class HomophoneTableLoad: @unchecked Sendable {
    private let done = DispatchSemaphore(value: 0)
    private var tables = HomophoneTables()
    private var words = HomophoneWords()

    /// words: 難読語の一覧 (homophone_words.swift)。nil なら読まない
    init(phrases: URL, triggers: URL, words wordsURL: URL?) {
        DispatchQueue.global(qos: .utility).async { [self] in
            tables = parseHomophoneTables(phrases: BoundedTable.text(phrases, maxBytes: homophoneLimits.maxBytes),
                                          triggers: BoundedTable.text(triggers, maxBytes: homophoneLimits.maxBytes))
            if let wordsURL { words = parseHomophoneWords(BoundedTable.text(wordsURL, maxBytes: homophoneWordLimits.maxBytes)) }
            done.signal()
        }
    }

    func wait() -> (tables: HomophoneTables, words: HomophoneWords) {
        done.wait()
        done.signal()
        return (tables, words)
    }
}

@MainActor var homophoneTableLoad: HomophoneTableLoad? = nil
@MainActor var homophoneTableCache: (tables: HomophoneTables, words: HomophoneWords)? = nil

@MainActor func startLoadingHomophoneTables() {
    // 比べるための開発用の切り替え: AZK_HOMOPHONES=0 で替えない (難読語の一覧も使わない)。AZK_HOMOPHONE_WORDS=0 で一覧だけ使わない
    guard homophoneTableLoad == nil, homophoneTableCache == nil else { return }
    if devEnvironment["AZK_HOMOPHONES"] == "0" {
        homophoneTableCache = (HomophoneTables(), HomophoneWords())
        return
    }
    homophoneTableLoad = HomophoneTableLoad(phrases: execURL.appendingPathComponent("homophone_phrases.tsv"),
                                            triggers: execURL.appendingPathComponent("homophone_triggers.tsv"),
                                            words: devEnvironment["AZK_HOMOPHONE_WORDS"] == "0" ? nil : execURL.appendingPathComponent("homophone_words.tsv"))
}

@MainActor func loadedHomophoneTables() -> (tables: HomophoneTables, words: HomophoneWords) {
    if let loaded = homophoneTableCache { return loaded }
    startLoadingHomophoneTables()
    let loaded = homophoneTableCache ?? homophoneTableLoad?.wait() ?? (HomophoneTables(), HomophoneWords())
    homophoneTableCache = loaded
    homophoneTableLoad = nil
    return loaded
}

@MainActor func homophoneTables() -> HomophoneTables { loadedHomophoneTables().tables }

/// この入力 (ClearText から確定まで) で替えた組 (読み<TAB>元の語<TAB>替えた語)
@MainActor var homophoneOffered: Set<String> = []
/// 利用者が元の語に戻した組 (読み<TAB>元の語)。学習の場所 (memory) の homophone_declined.txt に残す
@MainActor var homophoneDeclinedCache: Set<String>? = nil
let homophoneDeclinedLimit = 10_000

@MainActor func homophoneDeclinedURL() -> URL { execURL.appendingPathComponent("memory").appendingPathComponent("homophone_declined.txt") }

/// 2026-11-03: 学習データを使ってはいけない接続 (personalDataAllowed が偽: サンドボックスのアプリなど) では、記録を読まない (空) ・書かない
/// (変換を問い合わせて、利用者の戻した組を引き出せないように。空を読んで書き戻すと本物の記録が消えるので、書くほうも止める)
@MainActor func homophoneDeclined() -> Set<String> {
    guard personalDataAllowed else { return [] }
    if let declined = homophoneDeclinedCache { return declined }
    let rows = BoundedTable.rows(homophoneDeclinedURL(), maxBytes: 1 << 20, maxLines: homophoneDeclinedLimit, columns: 2...2, maxFieldLength: 64)
    let stored = Set(rows.filter { !$0[0].isEmpty && !$0[1].isEmpty }.map { $0[0] + "\t" + $0[1] })
    let declined = stored.subtracting(forgottenHomophones(declined: stored, discarded: LearningDiscard.words))
    homophoneDeclinedCache = declined
    return declined
}

/// 記録を書き直す (学習を覚えるとき・消すときだけ。ふだんの変換では書かない)
@MainActor func saveHomophoneDeclined(_ declined: Set<String>) {
    guard personalDataAllowed else { return }
    let kept = declined.subtracting(forgottenHomophones(declined: declined, discarded: LearningDiscard.words))
    guard kept.count <= homophoneDeclinedLimit else { return }
    homophoneDeclinedCache = kept
    let lines = ["# 難読語の同音語の差し替えで、利用者が元の語に戻した組 (読み<TAB>元の語)。この組は替えない。個人のデータ (配らない)"] + kept.sorted()
    try? (lines.joined(separator: "\n") + "\n").data(using: .utf8)?.write(to: homophoneDeclinedURL(), options: .atomic)
}

/// 2026-11-05 (仕事 28 の 4): 難読語の同音語への置き換え (当局の商用 → 当局の慫慂) をするか (設定 input.homophone_replace、初期値オン)。
/// 注記 (homophone_notes) は別の設定のまま
@MainActor func homophoneReplaceEnabled() -> Bool {
    config["homophoneReplace"] as? Bool ?? true
}

/// ai: AI の変換 (清書・Space) の結果のとき。学習した難読語の一覧の語に戻すか (AI の点で決める) も見る (homophone_words.swift)
@MainActor func replacingHomophones(_ segments: [(word: String, rubyCount: Int)], reading: String, context: String,
                                    ai: Bool = false, fast: Bool = false) -> [(word: String, rubyCount: Int)] {
    let tables = homophoneTables()
    var fixed = segments
    // 2026-11-05 (仕事 28 の 4): 設定 input.homophone_replace がオフなら、表で替えない (利用者が学習した難読語に戻すのは今までどおり)
    if !tables.isEmpty && homophoneReplaceEnabled() {
        let found: [HomophoneReplacement]
        (fixed, found) = replacingHomophones(segments, reading: reading, context: context, tables: tables, declined: homophoneDeclined())
        noteOfferedHomophones(found, reading: reading)
    }
    return ai ? choosingLearnedHomophones(fixed, reading: reading, context: context, fast: fast) : fixed
}

@MainActor func orderingHomophones(_ texts: [String], reading: String, context: String) -> [String] {
    let tables = homophoneTables()
    var ordered = texts
    if !tables.isEmpty && homophoneReplaceEnabled() {
        let found: [HomophoneReplacement]
        (ordered, found) = orderingHomophones(texts, reading: reading, context: context, tables: tables, declined: homophoneDeclined())
        noteOfferedHomophones(found, reading: reading)
    }
    // 難読語の一覧の語を 2 番目に (homophone_words.swift)
    return promotingHomophoneWords(ordered, reading: reading)
}

/// 新しい入力 (ClearText) で、この入力で替えた組の覚えを捨てる
@MainActor func clearOfferedHomophones() {
    homophoneOffered.removeAll()
    homophoneLearnedOffered.removeAll()
}

@MainActor func noteOfferedHomophones(_ found: [HomophoneReplacement], reading: String) {
    for r in found {
        homophoneOffered.insert(r.reading + "\t" + r.original + "\t" + r.replacement)
        if devEnvironment["AZK_DEBUG"] != nil {
            debugWrite("homophone \(reading) \(r.original)->\(r.replacement) \(r.reason)\n".data(using: .utf8)!)
        }
    }
}

/// 確定した文節 (LearnSegments) を見て、替えた語を元の語に戻して確定していたら、その組を以後替えない。
/// 前に戻した組でも、替えた後の語を確定したら、また替える。記録するのは学習と同じとき (IME 本体は覚えない欄 (パスワード・PIN・IS_PRIVATE) と
/// 個人のデータを許さない設定では LearnSegments を呼ばない (may_learn)。学習の設定がオフなら learn_segments がここより前に返る)
@MainActor func noteHomophoneChoices(readings: [String], texts: [String]) {
    defer { clearOfferedHomophones() }
    let reading = readings.joined(), text = texts.joined()
    var declined = homophoneDeclined()
    let before = declined
    declined.subtract(acceptedHomophones(reading: reading, text: text, declined: declined, tables: homophoneTables()))
    if !homophoneOffered.isEmpty { declined.formUnion(declinedHomophones(reading: reading, text: text, offered: homophoneOffered)) }
    if declined != before { saveHomophoneDeclined(declined) }
    // 難読語の一覧の語を選んだ記録 (homophone_words.swift)
    noteLearnedHomophones(readings: readings, texts: texts)
}

/// 候補の一覧で学習した語を忘れる (Ctrl+Delete。ForgetLearned) とき、その候補に入っている元の語の記録と、一覧の語を選んだ記録も消す
@MainActor func forgetHomophoneChoices(reading: String, text: String) {
    let declined = homophoneDeclined()
    let forgotten = forgottenHomophones(declined: declined, reading: reading, text: text)
    if !forgotten.isEmpty { saveHomophoneDeclined(declined.subtracting(forgotten)) }
    forgetLearnedHomophones(reading: reading, text: text)
}
