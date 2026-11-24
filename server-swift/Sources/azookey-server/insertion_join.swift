import Foundation
import KanaKanjiConverterModule

/// 2026-11-23: かなで書いた語の途中に短い読みを差し込んだときの、辞書だけの変換 (ライブ変換の 6 字以下の仮表示・AI を切った設定)。
/// 前は読みだけを文頭として変換し、1 字・2 字の読みが 目・編・前 のような字になっていた (AI の変換は左の文脈を見るので、かなのまま)。
/// 条件: 読みが 3 字以下のひらがな、左の文脈の終わりのかなの並び (ひらがなだけかカタカナだけ) が 2 字以上、右の文脈 (IME が渡すカーソルの右の字) が
/// 空か左と同じ字の種類のかなで始まる (語の途中に差し込んだ形。漢字の送り仮名・1 字の助詞のあと・右が漢字の所は今までどおり)。
/// - カタカナ: 右もカタカナなら、読みをカタカナで出す (カタカナの語の途中)。右がカタカナでなければ、左のかなと読みをつなげて引き、
///   左のまま始まって後ろがカタカナなら、それを出す (バナ + な → ナ)
/// - ひらがな: 左のかなと読みをつなげて辞書で引き、左のかなのまま始まって後ろがひらがななら、それを出す (かなの語の続き)。後ろに漢字が出るなら
///   (この + ときに → 時に) 今までどおり。左のかなが別の字になったなら (かいめい + した → 改名した、とも + だち → 友達)、語の途中とみて読みをかなで出す。
///   ただし左のかなだけを変換して終わりが助詞・助動詞なら (から + てを → 空手を)、助詞と読みがつながって別の語になった形なので今までどおり
/// つなげた変換の要素は句ごとにまとまることがある (ている場合が・は友達) ので、要素の切れ目ではなく字で見る。開発用の切り替え AZK_INSERT_JOIN=0 で前の形
let insertJoinEnabled = devEnvironment["AZK_INSERT_JOIN"] != "0"
/// 読みの長さの上限 (差し込みの組の測りで 2 字と 3 字は同じ数。6 字は文節の頭の差し込みを悪くした)
let insertJoinMaxReading = 3
/// 左の終わりのかなの並びの長さの上限と下限 (1 字は漢字の送り仮名か助詞のことが多い)
let insertJoinMaxTail = 8
let insertJoinMinTail = 2

enum KanaScript: Equatable { case hiragana, katakana }

func isHiraganaLetter(_ c: Character) -> Bool {
    c.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) || $0.value == 0x30FC }
}

func isKatakanaLetter(_ c: Character) -> Bool {
    c.unicodeScalars.allSatisfy { (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC }
}

/// 左の文脈の終わりの、ひらがなだけかカタカナだけの並び (長音の ー はどちらにも入る。字の種類は ー でない最後の字で決める)
func trailingKanaRun(_ context: String) -> (text: String, script: KanaScript)? {
    guard let last = context.last(where: { $0 != "ー" }) else { return nil }
    let script: KanaScript
    if last.unicodeScalars.allSatisfy({ (0x3041...0x3096).contains($0.value) }) {
        script = .hiragana
    } else if last.unicodeScalars.allSatisfy({ (0x30A1...0x30FA).contains($0.value) }) {
        script = .katakana
    } else {
        return nil
    }
    let hiraganaRun = script == .hiragana
    let belongs: (Character) -> Bool = { hiraganaRun ? isHiraganaLetter($0) : isKatakanaLetter($0) }
    var characters: [Character] = []
    for character in context.reversed() {
        guard belongs(character), characters.count < insertJoinMaxTail else { break }
        characters.append(character)
    }
    let run = String(characters.reversed())
    return run.isEmpty ? nil : (run, script)
}

func katakanaToHiraganaText(_ text: String) -> String {
    String(String.UnicodeScalarView(text.unicodeScalars.map { s in
        (0x30A1...0x30F6).contains(s.value) ? Unicode.Scalar(s.value - 0x60)! : s
    }))
}

/// 差し込んだ短い読みの、辞書だけの変換 (条件に合わなければ nil。今までどおり変換する)
@MainActor func insertedKanaSegments(hiragana: String, context: String, fast: Bool) -> [(word: String, rubyCount: Int)]? {
    guard insertJoinEnabled, !hiragana.isEmpty, hiragana.count <= insertJoinMaxReading, hiragana.allSatisfy(isHiraganaLetter),
          let (tail, script) = trailingKanaRun(context), tail.count >= insertJoinMinTail else { return nil }
    let right = rightContextFor(hiragana)
    let rightKatakana = right.first.map { isKatakanaLetter($0) && $0 != "ー" } ?? false
    let rightHiragana = right.first.map { isHiraganaLetter($0) && $0 != "ー" } ?? false
    // 右が空でなく、左と同じ字の種類のかなで始まらないなら、語の途中ではない
    if !right.isEmpty && !(script == .katakana ? rightKatakana : rightHiragana) && !(script == .katakana && rightHiragana) { return nil }
    if script == .katakana && rightKatakana {
        let kana = hiraganaToKatakana(hiragana)
        debugWrite("insertion join (katakana word) \(tail)+\(hiragana)+\(right.prefix(3)) -> \(kana)\n".data(using: .utf8)!)
        return [(word: kana, rubyCount: hiragana.count)]
    }
    let tailReading = script == .katakana ? katakanaToHiraganaText(tail) : tail
    let joined = tailReading + hiragana
    let composing = directComposingText(joined)
    let count = composing.input.count
    guard count == joined.count else { return nil }
    let options = getOptions(context: String(context.dropLast(tail.count)), prediction: false, zenzai: false, fast: fast)
    guard let best = converter.requestCandidates(composing, options: options).mainResults.first(where: { $0.correspondingCount == count }) else { return nil }
    if best.text.hasPrefix(tail) {
        // 左のかなのまま始まる: 後ろが左と同じ字の種類のかななら、それを読みの変換にする (かなの語の続き)。漢字が出るなら今までどおり
        let part = String(best.text.dropFirst(tail.count))
        let sameScript = !part.isEmpty && part.allSatisfy { script == .katakana ? isKatakanaLetter($0) : isHiraganaLetter($0) }
        debugWrite("insertion join (kept) \(tail)+\(hiragana) -> \(sameScript ? part : "-") (joined \(best.text))\n".data(using: .utf8)!)
        return sameScript ? [(word: part, rubyCount: hiragana.count)] : nil
    }
    // 左のかなが別の字になった (かなで書いた語の途中)。ただしカタカナは、右がカタカナでなければ今までどおり (上で見た形だけ)
    guard script == .hiragana else { return nil }
    // 左のかなだけの変換の終わりが助詞・助動詞なら、助詞と読みがつながって別の語になった形なので、つなげない
    let tailComposing = directComposingText(tailReading)
    if let tailBest = converter.requestCandidates(tailComposing, options: options).mainResults.first(where: { $0.correspondingCount == tailComposing.input.count }),
       let last = tailBest.data.last, (147...549).contains(last.lcid) {
        debugWrite("insertion join (particle) \(tail)+\(hiragana): joined \(best.text), tail \(tailBest.text)\n".data(using: .utf8)!)
        return nil
    }
    debugWrite("insertion join (kana) \(tail)+\(hiragana) -> \(hiragana) (joined \(best.text))\n".data(using: .utf8)!)
    return [(word: hiragana, rubyCount: hiragana.count)]
}
