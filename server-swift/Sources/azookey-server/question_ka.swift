import Foundation
import KanaKanjiConverterModule

/// 2026-11-06 (問いの か): 読みが「…か？」で終わるとき、？ を切らずに渡した変換も作って比べる。
/// 変換は記号 (？) の所で読みを切り、前の部分を ？ 抜きで変換する (preservingLiteralSymbols)。すると、か のすぐ後ろが ？ であることを辞書も AI も知らず、
/// 名詞＋問いの か が 名詞＋接尾 (積分家・意味家・正解化) や別の切り方 (微＋文化・新＋華夏) になっていた。
/// ？ を付けたまま辞書で変換すると 1 位は 微分か？・積分か？・これは正解か？ (か は助詞の 359、？ は 1317 につながる)。
/// ？ ごとの変換が か＋？ で終わり、切った変換が か で終わらないときだけ、？ ごとの変換を採る。
/// ただし、切った変換の その部分 が辞書の 1 語 (しんか？ → 進化？・すうがくか？ → 数学科？・さいてきか？ → 最適化？) なら割らない
/// (？ ごとでは 新か？・数学か？ が 1 位になるが、1 語の見出しがある形は どちらとも言えないので、今までどおりにする)。
/// 比べるための開発用の切り替え: AZK_QUESTION_KA=0 で前の形
let questionKaEnabled = devEnvironment["AZK_QUESTION_KA"] != "0"

/// conversionSegmentsUncached の呼び方: decide = 比べて選ぶ (ふだん)、split = ？ で切る (前の形)、whole = ？ を切らずに渡す
enum QuestionKaMode { case decide, split, whole }

let questionMarks: Set<Character> = ["？", "?"]

/// 読みが「(1 字以上)か」＋問いの印 (？ ?。続けて打った印も) で終わり、その前に記号が無いとき、終わりの印の並び。当てはまらなければ nil
func questionKaMarks(_ reading: String) -> String? {
    let chars = Array(reading)
    var end = chars.count
    while end > 0 && questionMarks.contains(chars[end - 1]) { end -= 1 }
    guard end < chars.count, end >= 2, chars[end - 1] == "か", !chars[..<end].contains(where: isLiteralSymbol) else { return nil }
    return String(chars[end...])
}

/// 切った変換 split と ？ ごとの変換 whole のどちらを採るか。single(読み, 表記) は、その表記がその読みの辞書の 1 語か。
/// 採るのは、2 つが 最後の 1 字 (？ の前) だけ違い、split のその字が漢字 (化・家・科・課・下・価 …)、whole のその字が か のときだけ
/// (積分家？ → 積分か？・意味家？ → 意味か？・これは正解化？ → これは正解か？)。
/// 切り方ごと替わる形 (低評価？ → 定評か？・再評価？ → 砕氷か？。Mozc の辞書の 接頭＋熟語＋化 の 500 語で 悪 65 のうち多く) と、
/// 微＋文化？ → 微分か？ (AI はもとから 微分か？) は替えない。名詞＋漢字 1 字 が辞書の 1 語 (進化・数学科・最適化) なら替えない
func choosingQuestionKa(split: [(word: String, rubyCount: Int)], whole: [(word: String, rubyCount: Int)], reading: String, marks: String,
                        single: (String, String) -> Bool) -> [(word: String, rubyCount: Int)] {
    choosingQuestionKaNoting(split: split, whole: whole, reading: reading, marks: marks, single: single).segments
}

/// choosingQuestionKa と同じに選び、替えたときは、替えた文節の読みと もとの形 (ながのか・長野家) も返す (候補の一覧で 2 番目に残すため)
func choosingQuestionKaNoting(split: [(word: String, rubyCount: Int)], whole: [(word: String, rubyCount: Int)], reading: String, marks: String,
                              single: (String, String) -> Bool) -> (segments: [(word: String, rubyCount: Int)], original: (reading: String, text: String)?) {
    let segments = choosingQuestionKaSegments(split: split, whole: whole, reading: reading, marks: marks, single: single)
    guard segments.map(\.word) != split.map(\.word), segments.count >= 2 else { return (segments, nil) }
    let changed = segments[segments.count - 2]
    let body = Array(reading.dropLast(marks.count))
    let start = body.count - changed.rubyCount
    // split の同じ範囲の表記 (選ぶときに 1 語でないことを確かめた所)
    var at = 0
    var span = ""
    for segment in split {
        if at >= start && at < body.count { span += segment.word }
        at += segment.rubyCount
    }
    return (segments, span.isEmpty ? nil : (String(body[start...]), span))
}

func choosingQuestionKaSegments(split: [(word: String, rubyCount: Int)], whole: [(word: String, rubyCount: Int)], reading: String, marks: String,
                                single: (String, String) -> Bool) -> [(word: String, rubyCount: Int)] {
    let splitText = split.map(\.word).joined()
    let wholeText = whole.map(\.word).joined()
    guard splitText != wholeText, wholeText.hasSuffix("か" + marks), splitText.hasSuffix(marks) else { return split }
    let splitCore = Array(splitText.dropLast(marks.count))
    let wholeCore = Array(wholeText.dropLast(marks.count))
    guard splitCore.count == wholeCore.count, splitCore.count >= 2, splitCore.dropLast() == wholeCore.dropLast(),
          let kanji = splitCore.last, kanji.unicodeScalars.allSatisfy({ (0x4E00...0x9FFF).contains($0.value) }) else { return split }
    let body = Array(reading.dropLast(marks.count))
    // whole で、か のすぐ前の字 (名詞の終わり) を含む文節の読みの始まり (か は ？ と一緒に後ろの文節になることが多い: 微分｜か？)
    var offset = 0
    var start: Int? = nil
    var noun = ""
    for segment in whole {
        if start == nil && offset + segment.rubyCount >= body.count - 1 { start = offset }
        if start != nil { noun += segment.word }
        offset += segment.rubyCount
    }
    guard let start, start < body.count - 1, noun.hasSuffix("か" + marks) else { return split }
    noun = String(noun.dropLast(marks.count + 1))
    guard !noun.isEmpty, !single(String(body[start...]), noun + String(kanji)) else { return split }
    // 文節は 名詞＋か｜？ に組み直す (変換の文節は 名詞｜か？ になる)。名詞＋か の文節の一覧は その読み (ながのか) の一覧なので、
    // 1 番目が もとの形 (長野家) になり、IME 本体が見えている形 (長野か) を一番上に入れ直すと、もとの形は 2 番目に残る (Space 1 回で戻せる)
    var out: [(word: String, rubyCount: Int)] = []
    var at = 0
    for segment in whole where at < start {
        out.append(segment)
        at += segment.rubyCount
    }
    guard at == start else { return whole }
    out.append((word: noun + "か", rubyCount: body.count - start))
    out.append((word: marks, rubyCount: marks.count))
    return out
}

/// 2026-11-06 (まとめ役 06:39 の条件): ？ ごとの形に替えた文節の読み → もとの形 (ながのか → 長野家)。その読みの候補の一覧では もとの形を先頭に置く
/// (IME 本体は見えている形 長野か を一番上に入れ直すので、もとの形は 2 番目になる。Space 1 回で戻せる)。新しいものを 32 個まで覚える。
/// 2026-11-23 (セキュリティの点検 13): 今の接続の分。接続を替えると ipc_sessions.swift の InputSession に退けて戻し、接続が切れたら消す
@MainActor var questionKaOriginals: [(reading: String, text: String)] = []

@MainActor func noteQuestionKaOriginal(_ original: (reading: String, text: String)?) {
    guard let original else { return }
    questionKaOriginals.removeAll { $0.reading == original.reading }
    questionKaOriginals.append(original)
    if questionKaOriginals.count > 32 { questionKaOriginals.removeFirst() }
}

/// 候補の一覧 texts の先頭に、その読みで替えた もとの形 を置く (無ければそのまま)
@MainActor func placingQuestionKaOriginal(_ texts: [String], reading: String) -> [String] {
    guard let original = questionKaOriginals.last(where: { $0.reading == reading })?.text else { return texts }
    return [original] + texts.filter { $0 != original }
}
