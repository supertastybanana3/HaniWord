import Foundation
import KanaKanjiConverterModule

/// 2026-11-06 (仕事 (4) と一緒の品詞の直し): 品詞に合わない漢字を、同じ読みでつながる語に替える (器械な形 → 奇怪な形、高尚し → 交渉し)。
/// 組み合わせとして文法上ありえない形のときだけ (確実なときだけ):
/// - (P1) 名詞 (一般 1285・サ変接続 1283) ＋ な (だ の連体形 454) ＋ 名詞 (な の後ろで名詞を修飾する形)。な に付くのは形容動詞の語幹 (1287) なので、
///   同じ読みの形容動詞の語幹 (奇怪・正確・親切) に替える。な の後ろが の・ので・のに・ん (機会なので・学生なのに) や、文の終わりなら替えない
///   (名詞＋な＋の は正しい形。な の後ろの名詞は 非自立 (こと・の) を除く)
/// - (P2) 形容動詞の語幹 (1287) ＋ し・する (サ変の動詞 610・599)。形容動詞の語幹に する は付かないので、同じ読みのサ変接続の名詞 (交渉) に替える
/// 替える語は、辞書だけの変換で読み全体が 1 語になる語のうち、品詞の合ういちばん点のよい語。無ければ替えない。語ごとの手の調整はしない
let posMismatchEnabled = devEnvironment["AZK_POS_MISMATCH"] != "0"

let generalNounCID = 1285
let sahenNounCID = 1283
let sahenShiCID = 610
let sahenSuruCID = 599

/// な の後ろで修飾される名詞 (一般・サ変接続・形容動詞語幹・固有名詞・人名・地名・数)。非自立 (こと・の 1310〜) と代名詞は数えない
func isModifiedNounCID(_ cid: Int) -> Bool {
    [sahenNounCID, generalNounCID, adjectivalStemCID].contains(cid) || (1288...1295).contains(cid)
}

/// 語の並びの中で、品詞に合わない語の位置と、替えるときに要る品詞 (左の品詞)。替える所が無ければ空
func posMismatches(_ data: [DicdataElement]) -> [(index: Int, wantLcid: Int)] {
    var out: [(index: Int, wantLcid: Int)] = []
    for i in data.indices {
        let e = data[i]
        guard e.word.unicodeScalars.contains(where: { (0x4E00...0x9FFF).contains($0.value) }) else { continue }
        // (P1) 名詞＋な＋名詞。な と名詞が 1 語になった形 (な形/ナカタチ: 左 454・右 1285) も
        if (e.rcid == generalNounCID || e.rcid == sahenNounCID) && e.lcid == e.rcid, i + 1 < data.count, data[i + 1].lcid == adjectivalNaCID {
            let na = data[i + 1]
            let modifies = na.word == "な" ? (i + 2 < data.count && isModifiedNounCID(data[i + 2].lcid))
                : (na.word.hasPrefix("な") && na.word.count > 1 && isModifiedNounCID(na.rcid))
            if modifies { out.append((i, adjectivalStemCID)) }
        }
        // (P2) 形容動詞の語幹＋し・する
        if e.rcid == adjectivalStemCID && e.lcid == adjectivalStemCID, i + 1 < data.count,
           [sahenShiCID, sahenSuruCID].contains(data[i + 1].lcid), ["し", "する"].contains(data[i + 1].word) {
            out.append((i, sahenNounCID))
        }
    }
    return out
}

/// 語の並びの品詞に合わない語を、lookup (読み → 同じ読みで品詞の合う語) で替えた並び。替える所が無ければ nil
func fixingPosMismatches(_ data: [DicdataElement], lookup: (String, Int) -> DicdataElement?) -> [DicdataElement]? {
    var out = data
    var changed = false
    for (i, want) in posMismatches(data) {
        guard let better = lookup(data[i].ruby, want), better.word != data[i].word, better.ruby == data[i].ruby else { continue }
        out[i] = DicdataElement(word: better.word, ruby: data[i].ruby, lcid: better.lcid, rcid: better.rcid, mid: better.mid, value: data[i].value())
        changed = true
    }
    return changed ? out : nil
}

/// 読み (カタカナ) と品詞 → 辞書だけの変換で読み全体が 1 語になる、その品詞 (左右とも) の いちばん点のよい語 (変換の間は覚えておく)
@MainActor var posAlternativeMemo: [String: DicdataElement?] = [:]

@MainActor func posAlternative(ruby: String, lcid: Int) -> DicdataElement? {
    let key = "\(ruby)\t\(lcid)"
    if let hit = posAlternativeMemo[key] { return hit }
    let reading = String(String.UnicodeScalarView(ruby.unicodeScalars.map { scalar in
        (0x30A1...0x30F6).contains(scalar.value) ? Unicode.Scalar(scalar.value - 0x60)! : scalar
    }))
    let composing = directComposingText(reading)
    let count = composing.input.count
    let found = converter.requestCandidates(composing, options: getOptions(context: "", prediction: false, zenzai: false, nBest: 30)).mainResults
        .filter { $0.correspondingCount == count && $0.data.count == 1 && $0.data[0].ruby == ruby && $0.data[0].lcid == lcid && $0.data[0].rcid == lcid }
        .max(by: { $0.value < $1.value })?.data.first
    if posAlternativeMemo.count >= 256 { posAlternativeMemo.removeAll() }
    posAlternativeMemo[key] = found
    return found
}

/// 変換で選んだ候補に fixingPosMismatches をかけた候補 (替える所が無ければ nil)
@MainActor func posMismatchesFixed(_ candidate: Candidate) -> Candidate? {
    guard posMismatchEnabled, !posMismatches(candidate.data).isEmpty,
          let data = fixingPosMismatches(candidate.data, lookup: { posAlternative(ruby: $0, lcid: $1) }) else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
                     lastMid: candidate.lastMid, data: data)
}
