import Foundation
import KanaKanjiConverterModule

private let scopeParticles = ["として", "について", "に対して", "から", "まで", "より", "には", "では", "とは", "にも", "へは", "のは", "のも",
                              "が", "は", "を", "に", "で", "と", "の", "へ", "も", "や"]

/// 2026-11-21: particleLedClauses なら、右の品詞も助詞の行 (を信じて 156→307・が集まって 148→307・をして。助詞＋動詞＋て を 1 つにした行) も、
/// 左右の品詞が違い (連語の助詞 について 199→199 は分けない)、残りを辞書で引けて、残りの頭の語が助詞でないときに 頭の助詞＋残り に分ける。
/// 名詞＋助詞を 1 文節にする形 (既定) で、本願を信じて・官公庁が集まっている が 1 つの文節にならないように (本願を／信じて)
func splitDictionaryParticleAffixes(
    _ elements: [DicdataElement],
    particleLedClauses: Bool = false,
    lookup: (String, String) -> Candidate?
) -> [DicdataElement] {
    var result: [DicdataElement] = []
    for element in elements {
        let ruby = element.ruby.toHiragana()
        var split = false
        let particleLed = particleLedClauses && CIDData.isJoshi(cid: element.rcid) && element.lcid != element.rcid
            && !compoundParticleCids.contains(element.lcid)
        // Fused dictionary entries retain the first and last word's grammatical IDs.
        if CIDData.isJoshi(cid: element.lcid) && (!CIDData.isJoshi(cid: element.rcid) || particleLed) {
            for particle in scopeParticles where ruby.hasPrefix(particle) && element.word.hasPrefix(particle) {
                let tailReading = String(ruby.dropFirst(particle.count))
                let tailText = String(element.word.dropFirst(particle.count))
                guard !tailReading.isEmpty, let tail = lookup(tailReading, tailText),
                      tail.data.map(\.word).joined() == tailText,
                      tail.data.reduce(0, { $0 + $1.ruby.count }) == tailReading.count,
                      !CIDData.isJoshi(cid: element.rcid) || tail.data.first.map({ !CIDData.isJoshi(cid: $0.lcid) }) == true else { continue }
                result.append(DicdataElement(word: particle, ruby: hiraganaToKatakana(particle), lcid: element.lcid,
                    rcid: element.lcid, mid: element.mid, value: element.value()))
                result += splitDictionaryParticleAffixes(tail.data, particleLedClauses: particleLedClauses, lookup: lookup)
                split = true
                break
            }
        } else if (1283..<1310).contains(element.lcid) && CIDData.isJoshi(cid: element.rcid) {
            for particle in scopeParticles where ruby.hasSuffix(particle) && element.word.hasSuffix(particle) {
                let headReading = String(ruby.dropLast(particle.count))
                let headText = String(element.word.dropLast(particle.count))
                guard !headReading.isEmpty, let head = lookup(headReading, headText),
                      head.data.last.map({ (1283..<1310).contains($0.rcid) }) == true,
                      head.data.map(\.word).joined() == headText,
                      head.data.reduce(0, { $0 + $1.ruby.count }) == headReading.count else { continue }
                result += splitDictionaryParticleAffixes(head.data, particleLedClauses: particleLedClauses, lookup: lookup)
                result.append(DicdataElement(word: particle, ruby: hiraganaToKatakana(particle), lcid: element.rcid,
                    rcid: element.rcid, mid: element.mid, value: element.value()))
                split = true
                break
            }
        }
        if !split { result.append(element) }
    }
    return result
}

/// 2026-11-20: keep の番号の文節 (連語の助詞の文節。と共に・によって) は割らない (と共に → と共＋に にしない)
func splitAttachedParticles(
    reading: String,
    segments: [(word: String, rubyCount: Int)],
    keep: Set<Int> = [],
    isNominalHead: (String, String) -> Bool
) -> [(word: String, rubyCount: Int)] {
    let chars = Array(reading)
    var offset = 0
    var result: [(word: String, rubyCount: Int)] = []
    for (index, segment) in segments.enumerated() {
        let end = offset + segment.rubyCount
        guard segment.rubyCount > 0, end <= chars.count else { return segments }
        let ruby = String(chars[offset..<end])
        offset = end
        if keep.contains(index) {
            result.append(segment)
            continue
        }
        var split = false
        for particle in scopeParticles where ruby.hasSuffix(particle) && segment.word.hasSuffix(particle) {
            let headReading = String(ruby.dropLast(particle.count))
            let headText = String(segment.word.dropLast(particle.count))
            guard !headReading.isEmpty, !headText.isEmpty else { continue }
            // A dictionary-verified noun boundary, not a suffix guess inside an inflected verb.
            if isNominalHead(headReading, headText) {
                result += [(word: headText, rubyCount: headReading.count), (word: particle, rubyCount: particle.count)]
                split = true
                break
            }
        }
        if !split { result.append(segment) }
    }
    return offset == chars.count ? result : segments
}


/// 2026-11-20: Space・清書の文節を、連語の助詞の前で切る (起こることとともに が 1 つの文節になり、一覧が読み全体の言い換えだけになっていた)。
/// 連語の助詞は IPADIC の 助詞,格助詞,連語 のうち として〜を通じまして (179..<255。とともに・において・について・によって・に対して・を通じて など 76 個)。
/// 引用・くだけた形 (じゃ・って・て・という・といった・とかいう。165..<179) は切らない。
/// 前は とともに・こと (名詞,非自立) が後置機能語として前の動詞につながり、起こることとともに が 1 つの文節だった
let compoundParticleCids = 179..<255
private let compoundParticleHeads = ["と", "に", "を"]

/// 連語の助詞を分けるか。既定 (before) は前で切るだけ: 起こること／とともに、守ろう／として、近づく／につれ (まとめ役 11-20 23:2x の決め)。
/// 開発用のビルドの AZK_CLAUSE_COMPOUND (測るため): 0 = 切らない (前の形)、noverb = 残りが動詞でない辞書の 1 語なら頭の助詞と分ける、word = 残りが辞書の 1 語なら分ける。
/// noverb・word を既定にしない訳 (辞書だけの dev 1,000 文): 残りを辞書で引くと同じ字の動詞でない語 (よって 接続詞・より 副詞・あたり 辺り) が返って
/// に／よって・に／より が多く出るうえ、分けた ともに を splitAttachedParticles が とも＋に に割る (と／とも／に)
enum CompoundParticleSplit: Equatable {
    case off, before, nonVerbTail, wordTail
}

/// 連語の助詞を、頭の助詞 (と・に・を) と残りに分ける。残りを辞書で引いて 1 語のときだけ (とともに → と＋ともに、について → に＋ついて)。
/// 頭の助詞には元の連語の品詞を付ける (文節の頭の印)。nonVerbTail なら、残りが動詞 (561...1280) のときは分けない
func splitCompoundParticles(_ elements: [DicdataElement], mode: CompoundParticleSplit, lookup: (String, String) -> Candidate?) -> [DicdataElement] {
    guard mode == .nonVerbTail || mode == .wordTail else { return elements }
    var result: [DicdataElement] = []
    for element in elements {
        guard compoundParticleCids.contains(element.lcid) else {
            result.append(element)
            continue
        }
        let ruby = element.ruby.toHiragana()
        var split = false
        for head in compoundParticleHeads where ruby.hasPrefix(head) && element.word.hasPrefix(head) {
            let tailReading = String(ruby.dropFirst(head.count))
            let tailText = String(element.word.dropFirst(head.count))
            guard !tailReading.isEmpty, let tail = lookup(tailReading, tailText), tail.data.count == 1,
                  tail.data[0].word == tailText, tail.data[0].ruby.count == tailReading.count,
                  mode == .wordTail || !(561...1280).contains(tail.data[0].lcid) else { continue }
            result.append(DicdataElement(word: head, ruby: hiraganaToKatakana(head), lcid: element.lcid,
                rcid: element.lcid, mid: element.mid, value: element.value()))
            result.append(tail.data[0])
            split = true
            break
        }
        if !split { result.append(element) }
    }
    return result
}

/// 連語の助詞 (と、splitCompoundParticles で分けた頭の助詞) の前で語の並びを区切り、区切りごとに clauseSplit(separateParticles: true) をかける。
/// off なら今までどおり 1 回だけ
func clauseSplitAtCompoundParticles(_ candidate: Candidate, mode: CompoundParticleSplit, latinParticlesOnly: Bool = false) -> [(word: String, rubyCount: Int)] {
    compoundParticleClauses(candidate, mode: mode, latinParticlesOnly: latinParticlesOnly).segments
}

/// clauseSplitAtCompoundParticles と同じ区切りと、連語の助詞で始まる文節の番号 (splitAttachedParticles の keep に渡し、と共に を と共＋に に割らない)。
/// 頭の助詞と残りに分ける形 (noverb・word) では、分けた頭の助詞の文節も入る (1 字なので割られない)
/// latinParticlesOnly: 名詞の後ろの助詞を前につなげる (教室に／陥る。clauseSplit の同じ名前の口)
/// 2026-11-21 (まとめ役 09:2x、案 2): heads なら、次の所でも文節を切る (ふつうの IME と同じく、ここで 2 つの文節にする)。
///   - 動詞・助動詞 (561...1280・369...554) のすぐ後ろの 名詞,非自立,副詞可能 (1313: ため・とき・ところ) と 名詞,副詞可能 (1314: 結果・後・上) の前
///     (参加する／ための・相談した／結果・考えた／上で)。名詞,非自立,一般 (1310: こと・もの) は前につなげたまま (起こること)
///   - 連体詞 (1315: この・その・ある・小さな) の後ろ (この／不変量・小さな／建物)
/// 前は azooKey の区切りの決まりで、これらが機能語のように前後の語につながっていた
func compoundParticleClauses(_ candidate: Candidate, mode: CompoundParticleSplit, latinParticlesOnly: Bool = false,
                             heads: Bool = false) -> (segments: [(word: String, rubyCount: Int)], keep: Set<Int>) {
    guard mode != .off || heads else { return (candidate.clauseSplit(separateParticles: true, latinParticlesOnly: latinParticlesOnly), []) }
    var chunks: [[DicdataElement]] = [[]]
    var previous: DicdataElement? = nil
    for element in candidate.data {
        let compound = mode != .off && compoundParticleCids.contains(element.lcid)
        let afterVerb = previous.map { (561...1280).contains($0.rcid) || (369...554).contains($0.rcid) } ?? false
        let head = heads && (((1313...1314).contains(element.lcid) && afterVerb) || previous?.rcid == 1315)
        if (compound || head) && !chunks[chunks.count - 1].isEmpty {
            chunks.append([])
        }
        chunks[chunks.count - 1].append(element)
        previous = element
    }
    var segments: [(word: String, rubyCount: Int)] = []
    var keep: Set<Int> = []
    for chunk in chunks where !chunk.isEmpty {
        if compoundParticleCids.contains(chunk[0].lcid) { keep.insert(segments.count) }
        let split = Candidate(text: chunk.map(\.word).joined(), value: candidate.value, correspondingCount: chunk.reduce(0) { $0 + $1.ruby.count },
                              lastMid: candidate.lastMid, data: chunk).clauseSplit(separateParticles: true, latinParticlesOnly: latinParticlesOnly)
        // 2026-11-21: 文節の最後の語の右の品詞が助詞でなければ割らない (語の並びが分かっているので、尻の と・に を助詞と見なさない。
        // 変更したこ／と・時間ととも／に・最／も・互い／に・特／に にしない。名詞＋助詞を 1 つにした語 (蒸し暑さを) は右が助詞なので今までどおり割る)
        var end = 0, at = 0, index = segments.count
        for segment in split {
            end += segment.rubyCount
            var last: DicdataElement? = nil
            while at < chunk.count, end > 0 {
                last = chunk[at]
                let consumed = chunk[...at].reduce(0) { $0 + $1.ruby.count }
                at += 1
                if consumed >= end { break }
            }
            if let last, !CIDData.isJoshi(cid: last.rcid) { keep.insert(index) }
            index += 1
        }
        segments += split
    }
    return (segments, keep)
}

/// 2026-11-21 (まとめ役 09:2x、案 1): azooKey の辞書の「複数の語を 1 つにした行」(ことが多い 1310→19・ことができる・必要があります・影響を受けた) を、
/// 中の助詞 (が・を・に・は・も・へ) の所で 前＋助詞＋後ろ の 3 つに分ける。前と後ろと助詞がどれも辞書で同じ字として引け、つないだ字と字数が元と同じときだけ。
/// 前と後ろは引き直した語の並びを使わず、それぞれ 1 語のまま (左の品詞は元の行の左、右の品詞は元の行の右) にして、助詞の所に切れ目を 1 つ入れるだけにする
/// (引き直した並びの中に区切りが入ると 欠かさ／ないように のようになる)。
/// で (である・ので)・と (こと・とする)・の (この・〜のこと) は語の中や助動詞にも出るので使わない (こ／とである・こ／の／年 になった)。
/// 前か後ろが 1 字のかな、左右の品詞が同じ行 (天の川・手がかり)、頭が助詞の行 (splitDictionaryParticleAffixes が受け持つ) は分けない。
/// 前は 1 語として引かれて文節の区切りの決まりが中に入れず、異なることが多い・影響を受けた が 1 つの文節だった
/// (辞書だけの dev 1,000 文の 8 字以上の文節で、ふつうの IME なら分ける 172 のうち 102 がこの行を含んでいた)
private let fusedParticles: [Character] = ["が", "を", "に", "は", "も", "へ"]

/// 連語の助詞 (compoundParticleCids の 76 個) の字と読み。IPADIC の left-id.def の 179..<255 の字に、漢字の入った形 (に対して・と共に・を通じて) の読みを足した
let compoundParticleReadings: [String: String] = [
    "として": "として",
    "としましたら": "としましたら",
    "としまして": "としまして",
    "とともに": "とともに",
    "と共に": "とともに",
    "にあたって": "にあたって",
    "にあたり": "にあたり",
    "にあたりまして": "にあたりまして",
    "にあたります": "にあたります",
    "にあたる": "にあたる",
    "において": "において",
    "におきまして": "におきまして",
    "における": "における",
    "にかけ": "にかけ",
    "にかけて": "にかけて",
    "にかけまして": "にかけまして",
    "にたいして": "にたいして",
    "にたいしまして": "にたいしまして",
    "にたいします": "にたいします",
    "にたいする": "にたいする",
    "について": "について",
    "につき": "につき",
    "につきまして": "につきまして",
    "につけ": "につけ",
    "につれ": "につれ",
    "につれて": "につれて",
    "にとって": "にとって",
    "にとり": "にとり",
    "にとりまして": "にとりまして",
    "にまつわります": "にまつわります",
    "にまつわる": "にまつわる",
    "によって": "によって",
    "により": "により",
    "によりまして": "によりまして",
    "によります": "によります",
    "による": "による",
    "にわたって": "にわたって",
    "にわたり": "にわたり",
    "にわたりまして": "にわたりまして",
    "にわたります": "にわたります",
    "にわたる": "にわたる",
    "に関し": "にかんし",
    "に関して": "にかんして",
    "に関しまして": "にかんしまして",
    "に関します": "にかんします",
    "に関する": "にかんする",
    "に際し": "にさいし",
    "に際して": "にさいして",
    "に際しまして": "にさいしまして",
    "に従い": "にしたがい",
    "に従いまして": "にしたがいまして",
    "に従います": "にしたがいます",
    "に従う": "にしたがう",
    "に従って": "にしたがって",
    "に対し": "にたいし",
    "に対して": "にたいして",
    "に対しまして": "にたいしまして",
    "に対します": "にたいします",
    "に対する": "にたいする",
    "に当たって": "にあたって",
    "に当たり": "にあたり",
    "に当たりまして": "にあたりまして",
    "に当たります": "にあたります",
    "に当たる": "にあたる",
    "をめぐって": "をめぐって",
    "をめぐりまして": "をめぐりまして",
    "をめぐります": "をめぐります",
    "をめぐる": "をめぐる",
    "をもちまして": "をもちまして",
    "をもって": "をもって",
    "を以て": "をもって",
    "を通して": "をとおして",
    "を通しまして": "をとおしまして",
    "を通じ": "をつうじ",
    "を通じて": "をつうじて",
    "を通じまして": "をつうじまして"
]

func splitFusedEntries(_ elements: [DicdataElement], depth: Int = 0, lookup: (String, String) -> Candidate?) -> [DicdataElement] {
    guard depth < 3 else { return elements }
    func isSingleKana(_ s: String) -> Bool { s.count == 1 && s.unicodeScalars.allSatisfy { (0x3041...0x309F).contains($0.value) } }
    var result: [DicdataElement] = []
    for element in elements {
        let word = Array(element.word)
        let ruby = Array(element.ruby.toHiragana())
        guard element.lcid != element.rcid, word.count >= 3, ruby.count >= 3, !CIDData.isJoshi(cid: element.lcid) else {
            result.append(element)
            continue
        }
        let text = String(word), reading = String(ruby)
        // 右の品詞が連語の助詞の行 (ことにより 1310→211・したことにより) は、尻の連語の助詞を表で見つけて 前の語＋連語の助詞 に分ける (変更したこと／により)
        if compoundParticleCids.contains(element.rcid),
           let found = compoundParticleReadings.filter({ text.hasSuffix($0.key) && reading.hasSuffix($0.value) }).max(by: { $0.key.count < $1.key.count }) {
            let surface = found.key, compoundReading = found.value
            let headText = String(text.dropLast(surface.count)), headReading = String(reading.dropLast(compoundReading.count))
            if !headText.isEmpty, !isSingleKana(headText), let head = lookup(headReading, headText)?.data, let headLast = head.last,
               head.map(\.word).joined() == headText, head.reduce(0, { $0 + $1.ruby.count }) == headReading.count {
                let headElement = DicdataElement(word: headText, ruby: hiraganaToKatakana(headReading), lcid: element.lcid, rcid: headLast.rcid,
                                                 mid: element.mid, value: element.value())
                result += splitFusedEntries([headElement], depth: depth + 1, lookup: lookup)
                result.append(DicdataElement(word: surface, ruby: hiraganaToKatakana(compoundReading), lcid: element.rcid, rcid: element.rcid,
                                             mid: element.mid, value: element.value()))
                continue
            }
        }
        var parts: [DicdataElement]? = nil
        search: for i in 1..<(word.count - 1) where fusedParticles.contains(word[i]) {
            for j in 1..<(ruby.count - 1) where ruby[j] == word[i] {
                let headText = String(word[..<i]), headReading = String(ruby[..<j])
                let tailText = String(word[(i + 1)...]), tailReading = String(ruby[(j + 1)...])
                // 助詞から先が連語の助詞 (により・について) なら、その途中では切らない (ことに＋より にしない)
                if compoundParticleReadings.contains(where: { String(word[i...]).hasPrefix($0.key) && String(ruby[j...]).hasPrefix($0.value) }) { continue }
                guard !isSingleKana(headText), !isSingleKana(tailText),
                      let head = lookup(headReading, headText)?.data, let headLast = head.last,
                      head.map(\.word).joined() == headText, head.reduce(0, { $0 + $1.ruby.count }) == headReading.count,
                      let tail = lookup(tailReading, tailText)?.data, let tailFirst = tail.first,
                      tail.map(\.word).joined() == tailText, tail.reduce(0, { $0 + $1.ruby.count }) == tailReading.count,
                      let particle = lookup(String(word[i]), String(word[i]))?.data, particle.count == 1,
                      particle[0].word == String(word[i]), CIDData.isJoshi(cid: particle[0].lcid) else { continue }
                let headElement = DicdataElement(word: headText, ruby: hiraganaToKatakana(headReading), lcid: element.lcid, rcid: headLast.rcid,
                                                 mid: element.mid, value: element.value())
                let tailElement = DicdataElement(word: tailText, ruby: hiraganaToKatakana(tailReading), lcid: tailFirst.lcid, rcid: element.rcid,
                                                 mid: element.mid, value: element.value())
                parts = [headElement] + particle + splitFusedEntries([tailElement], depth: depth + 1, lookup: lookup)
                break search
            }
        }
        result += parts ?? [element]
    }
    return result
}
