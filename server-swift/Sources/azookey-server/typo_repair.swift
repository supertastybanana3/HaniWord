import Foundation
import KanaKanjiConverterModule

/// 2026-10-28: ローマ字の母音の抜け (そういうkとです = そういうことです) を直すための採点。
/// どの字を直すか (ローマ字として読めない子音 1 字だけ。英字にした語は直さない) と読みの候補 (母音を補った読み) は IME 本体が作って渡す (auto_english.rs の vowel_repairs)。
/// ここでは候補ごとに、辞書だけの変換の 1 位の点数と、読みだけを zenz が書く確率 (対数) を返す (試験用には、読みとその表記をいっしょに書く確率も)。
/// 読みが違う候補どうしを比べるので、読みを条件にした表記の確率 (zenzSequenceScore) は使えない
@MainActor func readingScores(_ reading: String, context: String, zenz: Bool, joint: Bool) -> (dict: PValue, joint: Float?, readingOnly: Float?, text: String)? {
    let composing = directComposingText(reading)
    let count = composing.input.count
    guard let best = converter.requestCandidates(composing, options: getOptions(context: context, prediction: false, zenzai: false, fast: true))
        .mainResults.first(where: { $0.correspondingCount == count }) else { return nil }
    let zenzOptions = getOptions(context: context, prediction: false, zenzai: true, fast: true)
    let jointScore = zenz && joint ? converter.zenzJointScore(hiragana: reading, output: best.text, leftSideContext: context, options: zenzOptions) : nil
    let readingOnly = zenz ? converter.zenzJointScore(hiragana: reading, output: "", leftSideContext: context, options: zenzOptions) : nil
    return (best.value, jointScore, readingOnly, best.text)
}

/// 読みの候補 ("\u{1}" 区切り、16 個まで) ごとに「辞書の点数<TAB>読みだけの zenz の点数」を "\u{1}" 区切りで返す (取れなかった欄は空。AI を切っていれば zenz の欄は空)
@_silgen_name("ScoreReadings")
@MainActor public func score_readings(readingsPtr: UnsafePointer<CChar>, contextPtr: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar> {
    let readings = String(cString: readingsPtr).components(separatedBy: "\u{1}").prefix(16)
    let context = leftContext(String(cString: contextPtr))
    let rows = readings.map { reading -> String in
        guard let s = readingScores(reading, context: context, zenz: true, joint: false) else { return "\t" }
        return "\(s.dict)\t\(s.readingOnly.map { String($0) } ?? "")"
    }
    return _strdup(rows.joined(separator: "\u{1}"))!
}

// 次の 2 つは測るための入口 (リポジトリの外の測りの道具が呼ぶ)。配る版には入れない (AZK_DEV_KNOBS のビルドだけ)
#if AZK_DEV_KNOBS
/// 読みの候補 ("\u{1}" 区切り) ごとに「辞書の点数<TAB>読みと表記をいっしょに書く zenz の点数<TAB>読みだけの zenz の点数<TAB>表記」を "\u{1}" 区切りで返す (取れなかった欄は空)
@_silgen_name("ScoreReadingsDetail")
@MainActor public func score_readings_detail(readingsPtr: UnsafePointer<CChar>, contextPtr: UnsafePointer<CChar>, zenz: Bool) -> UnsafeMutablePointer<CChar> {
    let readings = String(cString: readingsPtr).components(separatedBy: "\u{1}")
    let context = leftContext(String(cString: contextPtr))
    let rows = readings.map { reading -> String in
        guard let s = readingScores(reading, context: context, zenz: zenz, joint: true) else { return "\t\t\t" }
        return "\(s.dict)\t\(s.joint.map { String($0) } ?? "")\t\(s.readingOnly.map { String($0) } ?? "")\t\(s.text)"
    }
    return _strdup(rows.joined(separator: "\u{1}"))!
}

/// 2026-10-28 (0.1.1 の調べ、試作): ライブラリの予測変換 (requireJapanesePrediction) が何を出すかを見る。
/// 読みの候補の一覧 (mainResults) を「表記<TAB>候補の読み<TAB>点数」で "\u{1}" 区切りで返す。候補の読みが打った読みより長いものが予測
@_silgen_name("PredictCandidates")
@MainActor public func predict_candidates(readingPtr: UnsafePointer<CChar>, contextPtr: UnsafePointer<CChar>, zenz: Bool) -> UnsafeMutablePointer<CChar> {
    let composing = directComposingText(String(cString: readingPtr))
    let context = leftContext(String(cString: contextPtr))
    let converted = converter.requestCandidates(composing, options: getOptions(context: context, prediction: true, zenzai: zenz, fast: true))
    let rows = converted.mainResults.map { c in "\(c.text)\t\(c.data.map(\.ruby).joined().toHiragana())\t\(c.value)" }
    return _strdup(rows.joined(separator: "\u{1}"))!
}
#endif
