import Foundation
import KanaKanjiConverterModule

/// 2026-11-07 (仕事 45): 文の途中に打ったとき、カーソルの右の字 (右の文脈) と左の文脈の終わりの語も見て、組み合わせの数の表 (colloc.swift) で同音の字を選ぶ。
/// 右の文脈は IME が SetRightContext で渡す (左の SetContext と並べて、入力中の範囲の終わりから 20 字・最初の改行まで)。空なら今までと同じ。
/// 2026-11-08 (dev・test の測り): 使うのは辞書だけの変換だけ (colloc.swift の collocChosen)。左の文脈の終わりの語はいつも、右の字は設定 input.right_context
/// (初期値オン) がオンのときだけ。開発用の切り替え: AZK_RIGHT_CONTEXT=1/0 で右の字を設定によらずオン/オフ、AZK_OUTER_RIGHT=0 で右の字を、
/// AZK_OUTER_LEFT=0 で左の文脈の終わりの語を使わない、AZK_OUTER_AI=1 で AI ありの変換でも使う (比べるため)
@MainActor var rightContextText = ""

let rightContextMaxCharacters = 20

/// ConvertSegments に渡された読み全体。変換サーバーは読みを記号・化学式などで分けて前から変換することがあるので、
/// 右の字は、変換する部分が読み全体の終わりに当たるときだけ使う (rightContextFor)
@MainActor var rightContextReading = ""

@MainActor func rightContextFor(_ hiragana: String) -> String {
    rightContextReading.hasSuffix(hiragana) ? rightContextText : ""
}

@_silgen_name("SetRightContext")
@MainActor public func set_right_context(context: UnsafePointer<CChar>) {
    rightContextText = trimmedRightContext(String(cString: context))
}

/// 右の文脈は最初の改行より前の、頭から 20 字まで
func trimmedRightContext(_ text: String) -> String {
    String(text.prefix { $0 != "\n" && $0 != "\r" }.prefix(rightContextMaxCharacters))
}

@MainActor func rightContextEnabled() -> Bool {
    if let knob = devEnvironment["AZK_RIGHT_CONTEXT"] { return knob == "1" }
    return (config["rightContext"] as? Bool) ?? true
}

/// 組み合わせの数の表に足す、外側の文脈 (左の文脈の終わり・右の字) の候補。表の鍵の 文脈の種類<TAB>文脈 の候補を、先に試すものから並べたもの
struct CollocOuterContexts: Equatable {
    /// 候補の最初の語の前 (L1 の語・L2 の 語|格助詞)
    var left: [String] = []
    /// 左の文脈の終わりの語 (候補の最初が格助詞のとき、その次の語の L2 に使う)
    var leftWords: [String] = []
    /// 候補の最後の語の後ろ (R1 の語・R2 の 格助詞|語)
    var right: [String] = []
    /// 右の字の頭の語 (候補の最後が格助詞のとき、その前の語の R2 に使う)
    var rightWords: [String] = []

    var isEmpty: Bool { left.isEmpty && leftWords.isEmpty && right.isEmpty && rightWords.isEmpty }
}

/// 字の種類 (読みが無いので、語の切れ目は字の種類が変わる所だけで決める。例ごとの決まりは作らない)
private func isKanjiCharacter(_ c: Character) -> Bool {
    c.unicodeScalars.allSatisfy { (0x4E00...0x9FFF).contains($0.value) || (0x3400...0x4DBF).contains($0.value) || [0x3005, 0x3006, 0x30F6].contains($0.value) }
}
private func isHiraganaCharacter(_ c: Character) -> Bool { c.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) } }
private func isKatakanaCharacter(_ c: Character) -> Bool { c.unicodeScalars.allSatisfy { (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC } }
private let kanjiNumeralCharacters = Set("〇一二三四五六七八九十百千万億兆")

/// 字の並びの頭の語の候補 (長いほうから): 漢字の並び＋送り仮名 2・1・0 字、カタカナの並び。かなで始まる・数詞だけの漢字の並びは無し
func collocLeadingWords(_ text: Substring) -> [String] {
    guard let first = text.first else { return [] }
    if isKatakanaCharacter(first) {
        return [String(text.prefix(while: isKatakanaCharacter))]
    }
    guard isKanjiCharacter(first) else { return [] }
    let kanji = text.prefix(while: isKanjiCharacter)
    if kanji.allSatisfy({ kanjiNumeralCharacters.contains($0) }) { return [] }
    let kana = text.dropFirst(kanji.count).prefix(while: isHiraganaCharacter)
    return (0...min(2, kana.count)).reversed().map { String(kanji) + String(kana.prefix($0)) }
}

/// 字の並びの終わりの語の候補: 漢字の並び＋その後ろのかな (3 字まで) 全部、カタカナの並び。数詞だけの漢字の並びは無し
func collocTrailingWords(_ text: Substring) -> [String] {
    guard let last = text.last else { return [] }
    if isKatakanaCharacter(last) {
        return [String(text.reversed().prefix(while: isKatakanaCharacter).reversed())]
    }
    let kana = text.reversed().prefix(while: isHiraganaCharacter)
    guard kana.count <= 3 else { return [] }
    let kanji = text.dropLast(kana.count).reversed().prefix(while: isKanjiCharacter)
    guard !kanji.isEmpty, !kanji.allSatisfy({ kanjiNumeralCharacters.contains($0) }) else { return [] }
    return [String(kanji.reversed()) + String(kana.reversed())]
}

private let caseParticlesLongestFirst = collocCaseParticles.sorted { $0.count > $1.count }

/// 左の文脈・右の字から、外側の文脈の候補を作る
func collocOuterContexts(left: String, right: String, useLeft: Bool, useRight: Bool) -> CollocOuterContexts {
    var out = CollocOuterContexts()
    if useRight, !right.isEmpty {
        let r = Substring(right)
        if let p = caseParticlesLongestFirst.first(where: { r.hasPrefix($0) }) {
            out.right = collocLeadingWords(r.dropFirst(p.count)).map { "R2\t\(p)|\($0)" }
        } else {
            out.right = collocLeadingWords(r).map { "R1\t\($0)" }
        }
        out.rightWords = collocLeadingWords(r)
    }
    if useLeft, !left.isEmpty {
        let l = Substring(left)
        if let p = caseParticlesLongestFirst.first(where: { l.hasSuffix($0) }), !collocTrailingWords(l.dropLast(p.count)).isEmpty {
            out.left = collocTrailingWords(l.dropLast(p.count)).map { "L2\t\($0)|\(p)" }
        } else {
            out.left = collocTrailingWords(l).map { "L1\t\($0)" }
        }
        out.leftWords = collocTrailingWords(l)
    }
    return out
}

/// 候補の k 番目のかけらに足す外側の文脈 (表の鍵の 文脈の種類<TAB>文脈 の候補。表にある最初のものを使う)
func collocOuterKeys(_ outer: CollocOuterContexts, pieces: [DicdataElement], at k: Int) -> [[String]] {
    var keys: [[String]] = []
    let isCaseParticle: (DicdataElement) -> Bool = { collocCaseParticles.contains($0.word) && CIDData.isJoshi(cid: $0.lcid) }
    if k == pieces.count - 1, !outer.right.isEmpty {
        keys.append(outer.right)
    }
    if k == pieces.count - 2, isCaseParticle(pieces[k + 1]), !outer.rightWords.isEmpty {
        keys.append(outer.rightWords.map { "R2\t\(pieces[k + 1].word)|\($0)" })
    }
    if k == 0, !outer.left.isEmpty {
        keys.append(outer.left)
    }
    if k == 1, isCaseParticle(pieces[0]), !outer.leftWords.isEmpty {
        keys.append(outer.leftWords.map { "L2\t\($0)|\(pieces[0].word)" })
    }
    return keys
}

// MARK: - 仕事 45 の 2: AI ありの変換の並べ直し

/// AI ありの変換の最後の候補の上位 N 個を、候補のあとにカーソルの右の頭 k 字が続く zenz の確率で並べ直す。
/// zenz は左の文脈だけで学習しているので、右の字はプロンプトに入れず、候補の続きとして測る (KanaKanjiConverter.zenzRightContextScore)。
/// 点 = zenz の候補の確率 (読み・左の文脈から、EOS まで) + w × 右の字が続く確率。1 位を替えるのは、点の差が幅を超えるときだけ。
/// 2026-11-08: 0.1.0 では使わない (Vulkan で 1 回の変換が 90% で +804ms 延び、目安の 100ms を大きく超えた。dev の点の書き出しで選んだ字だけの形 b・N=5・k=16・w=1・
/// 幅 4 でも 良 29・悪 0 (0.37%) と小さい)。開発用の切り替え: AZK_RCTX_AI=1 で並べ直す、AZK_RCTX_N・AZK_RCTX_K・AZK_RCTX_W・AZK_RCTX_MARGIN、AZK_RCTX_MODE=a (読みつき)・b (字だけ)。
/// AZK_RCTX_DUMP=<ファイル> で、候補ごとの点 (N は 5 まで、k = 4・8・16、a・b) を 1 行ずつ書き出す (dev で N・k・w・幅を決めるため。重い)
struct RightRerankConfig {
    var n = 3
    var k = 8
    var w: Float = 1
    var margin: Float = 0
    var readingConditioned = true

    static let current: RightRerankConfig = {
        var c = RightRerankConfig()
        let env = devEnvironment
        if let v = env["AZK_RCTX_N"].flatMap({ Int($0) }) { c.n = max(1, v) }
        if let v = env["AZK_RCTX_K"].flatMap({ Int($0) }) { c.k = max(1, v) }
        if let v = env["AZK_RCTX_W"].flatMap({ Float($0) }) { c.w = v }
        if let v = env["AZK_RCTX_MARGIN"].flatMap({ Float($0) }) { c.margin = v }
        if env["AZK_RCTX_MODE"] == "b" { c.readingConditioned = false }
        return c
    }()
}

/// 並べ直す候補: 今の 1 位と、読み全体を覆うほかの候補 (表記の違うもの) を上から n 個まで
func rightRerankPool(chosen: Candidate, others: [Candidate], count: Int, n: Int) -> [Candidate] {
    var seen: Set<String> = [chosen.text]
    var pool = [chosen]
    for c in others where c.correspondingCount == count && pool.count < n && !seen.contains(c.text) {
        seen.insert(c.text)
        pool.append(c)
    }
    return pool
}

/// 点の並べ直し: 今の 1 位 (pool の頭) より点が幅を超えて高い候補があれば、その番号
func rightRerankChoice(base: [Float], continuation: [Float], w: Float, margin: Float) -> Int? {
    guard base.count == continuation.count, base.count >= 2 else { return nil }
    let total = zip(base, continuation).map { $0 + w * $1 }
    guard let best = total.indices.max(by: { total[$0] < total[$1] }), best != 0, total[best] - total[0] > margin else { return nil }
    return best
}

@MainActor func rerankingByRightContext(chosen: Candidate, others: [Candidate], hiragana: String, count: Int, left: String, right: String, fast: Bool) -> Candidate? {
    guard !right.isEmpty else { return nil }
    let config = RightRerankConfig.current
    let options = getOptions(context: left, prediction: false, zenzai: true, fast: fast)
    let dump = devEnvironment["AZK_RCTX_DUMP"]
    let pool = rightRerankPool(chosen: chosen, others: others, count: count, n: dump == nil ? config.n : max(config.n, 5))
    guard pool.count >= 2 else { return nil }
    var base: [Float] = []
    var continuation: [Float] = []
    for c in pool {
        guard let b = converter.zenzSequenceScore(hiragana: hiragana, output: c.text, leftSideContext: left, options: options),
              let r = converter.zenzRightContextScore(hiragana: config.readingConditioned ? hiragana : nil, output: c.text,
                                                      right: String(right.prefix(config.k)), leftSideContext: left, options: options) else { return nil }
        base.append(b)
        continuation.append(r)
    }
    if let path = dump {
        // dev で決めるための書き出し: 候補ごとに、k = 4・8・16 と a・b の続きの確率
        var rows: [String] = []
        for (i, c) in pool.enumerated() {
            var cols = ["\"text\":\"\(rightContextJSONEscaped(c.text))\"", "\"base\":\(base[i])"]
            for k in [4, 8, 16] {
                for (mode, reading) in [("a", Optional(hiragana)), ("b", nil)] {
                    let v = converter.zenzRightContextScore(hiragana: reading, output: c.text, right: String(right.prefix(k)), leftSideContext: left, options: options)
                    cols.append("\"\(mode)\(k)\":\(v.map { "\($0)" } ?? "null")")
                }
            }
            rows.append("{" + cols.joined(separator: ",") + "}")
        }
        let line = "{\"reading\":\"\(rightContextJSONEscaped(hiragana))\",\"right\":\"\(rightContextJSONEscaped(right))\",\"candidates\":[\(rows.joined(separator: ","))]}\n"
        if let handle = FileHandle(forWritingAtPath: path) ?? { FileManager.default.createFile(atPath: path, contents: nil); return FileHandle(forWritingAtPath: path) }() {
            handle.seekToEndOfFile(); handle.write(line.data(using: .utf8)!); handle.closeFile()
        }
    }
    let n = min(config.n, pool.count)
    guard let pick = rightRerankChoice(base: Array(base.prefix(n)), continuation: Array(continuation.prefix(n)), w: config.w, margin: config.margin) else { return nil }
    debugWrite("right rerank \(hiragana) \(chosen.text) -> \(pool[pick].text)\n".data(using: .utf8)!)
    return pool[pick]
}

private func rightContextJSONEscaped(_ s: String) -> String {
    s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"").replacingOccurrences(of: "\n", with: "\\n")
}
