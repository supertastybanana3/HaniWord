import Foundation
import KanaKanjiConverterModule

/// 2026-11-04: 文末の わら → 藁 を直す。文末の 藁 (読み わら) を 笑・わら に替えた候補と AI の結果を zenz に文全体で採点させ、
/// 候補の点が AI の結果の点から waraMargin 以上は下がらなければ替える (藁 が本当に正しい文もあるので、元の結果も比べに入れる)。
/// 感動詞の崩れ (ほーん → ホーン、ふう → 風) は、オフにできない追加辞書 core_interjections (extra_dictionaries) の行で減らす。
/// 文の頭の感動詞を替える形も試したが、dev・test で直った数が 0 で、一般の文では 1 字の感動詞の頭 (は・お) で替えにいって悪くしたので入れない
/// 比べるための開発用の切り替え: AZK_KANDOU=0 で替えない。差の線は AZK_WARA_MARGIN
let kandouFix = devEnvironment["AZK_KANDOU"] != "0"
let waraMargin: Float = devEnvironment["AZK_WARA_MARGIN"].flatMap(Float.init) ?? 6.0
/// 測るための開発用の記録: AZK_KANDOU_LOG=<ファイル>
let kandouLogPath = devEnvironment["AZK_KANDOU_LOG"]

@MainActor func writeKandouLog(_ kind: String, _ hiragana: String, _ before: String, _ after: String, _ scores: String, _ started: Date) {
    guard let path = kandouLogPath else { return }
    let esc: (String) -> String = { $0.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") }
    let line = "{\"kind\":\"\(kind)\",\"reading\":\"\(esc(hiragana))\",\"before\":\"\(esc(before))\",\"after\":\"\(esc(after))\",\"scores\":\"\(scores)\",\"ms\":\(Int(-started.timeIntervalSinceNow * 1000))}\n"
    if let data = line.data(using: .utf8), let handle = FileHandle(forWritingAtPath: path) ?? { FileManager.default.createFile(atPath: path, contents: nil); return FileHandle(forWritingAtPath: path) }() {
        handle.seekToEndOfFile(); handle.write(data); handle.closeFile()
    }
}

/// 文末が 藁 (読み わら) の語か (2 語以上の候補の最後の語)
func endsWithWara(_ data: [DicdataElement]) -> Bool {
    guard data.count >= 2, let last = data.last else { return false }
    return last.ruby == "ワラ" && (last.word == "藁" || last.word == "ワラ")
}

/// 文末の 藁 (読み わら) を 笑・わら に替えた候補 (元の結果のほうが良ければ nil)
@MainActor func replacingFinalWara(_ best: Candidate, hiragana: String, context: String, fast: Bool) -> Candidate? {
    guard endsWithWara(best.data), let last = best.data.last else { return nil }
    let started = Date()
    let zenzOptions = getOptions(context: context, prediction: false, zenzai: true, fast: fast)
    guard let original = converter.zenzSequenceScore(hiragana: hiragana, output: best.text, leftSideContext: context, options: zenzOptions) else { return nil }
    var bestScore = -Float.infinity
    var bestData: [DicdataElement]? = nil
    var scores: [String] = ["\(original)"]
    for word in ["笑", "わら"] {
        var data = best.data
        data[data.count - 1] = DicdataElement(word: word, ruby: last.ruby, lcid: last.lcid, rcid: last.rcid, mid: last.mid, value: last.value())
        guard let lp = converter.zenzSequenceScore(hiragana: hiragana, output: data.map(\.word).joined(), leftSideContext: context, options: zenzOptions) else { continue }
        scores.append("\(lp)")
        if lp > bestScore {
            bestScore = lp
            bestData = data
        }
    }
    let replace = bestData != nil && bestScore >= original - waraMargin
    writeKandouLog("wara", hiragana, best.text, replace ? bestData!.map(\.word).joined() : best.text, scores.joined(separator: ","), started)
    guard replace, let data = bestData else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: best.value, correspondingCount: best.correspondingCount, lastMid: best.lastMid, data: data)
}

/// 2026-11-23: 文の頭の 1 語のすぐ後ろが読点 (うん、・はい、・まあ、) で、その語の読みに感動詞 (品詞 3) があるのに、感動詞でない語 (運・肺) を
/// 選んでいたら、感動詞に替える。辞書だけの変換 (AI が間に合わない・休んでいるとき) で、前に 1 語だけで覚えた語 (うん → 運) が文の頭の感動詞に
/// 勝っていた。条件は、文の頭であること・語の読み全体が感動詞の読みと同じこと・すぐ後ろが読点であること (11-04 に読点の条件なしで文の頭の感動詞を
/// 替えたら、1 字の頭 (は・お) で 羽生や → は不や のように悪くしたので入れなかった)。登録した語は替えない。AZK_KANDOU_HEAD=0 で替えない (開発用のビルドだけ)
let kandouHeadFix = devEnvironment["AZK_KANDOU_HEAD"] != "0"
let interjectionCid = 3

/// 左の文脈が文の頭か (空・句点・感嘆符・疑問符・改行・始めのかっこで終わる)
func atSentenceHead(_ context: String) -> Bool {
    guard let last = context.last(where: { $0 != " " && $0 != "　" }) else { return true }
    return "。．！？!?\n「『（(【".contains(last)
}

/// 文の頭の語 (best.data[0]) を、同じ読みの感動詞に替えた候補。follower は読みの終わりのすぐ後ろの字 (記号で分けた部分を変換しているとき)
@MainActor func replacingInitialInterjection(_ best: Candidate, hiragana: String, context: String, follower: Character?) -> Candidate? {
    guard kandouHeadFix, atSentenceHead(context), let first = best.data.first, first.lcid != interjectionCid,
          !first.metadata.contains(.isUserRegistered) else { return nil }
    let count = first.ruby.count
    guard count >= 1, count <= hiragana.count, hiraganaToKatakana(String(hiragana.prefix(count))) == first.ruby else { return nil }
    let next: Character? = count < hiragana.count ? hiragana[hiragana.index(hiragana.startIndex, offsetBy: count)] : follower
    guard let next, "、，,".contains(next) else { return nil }
    // 感動詞は、読みのひらがなのとおりに書くもの (まあ・くっ・ほーん) だけ。辞書には まア・くッ・痛ってー のような感動詞の綴りもあり、
    // 値の順に取ると まあ、→ まア、 のように悪くした (11-23 の測り)。ひらがなの感動詞が無い読み・もうその字なら替えない
    let reading = String(hiragana.prefix(count))
    guard first.word != reading else { return nil }
    let composing = directComposingText(reading)
    let options = getOptions(context: "", prediction: false, zenzai: false, nBest: 50)
    guard let interjection = converter.requestCandidates(composing, options: options).mainResults.first(where: {
        $0.correspondingCount == composing.input.count && $0.data.count == 1 && $0.data[0].lcid == interjectionCid
            && $0.data[0].ruby == first.ruby && $0.data[0].word == reading
    })?.data[0] else { return nil }
    var data = best.data
    data[0] = interjection
    writeKandouLog("head", hiragana, best.text, data.map(\.word).joined(), "", Date())
    return Candidate(text: data.map(\.word).joined(), value: best.value, correspondingCount: best.correspondingCount, lastMid: best.lastMid, data: data)
}
