import Foundation
import KanaKanjiConverterModule

/// 2026-11-03: 連濁の形の読み (ぐるま・ざくら・がみ) の語が、AI の変換の結果で助詞のすぐ後ろに単独で来た所 (あとはぐるまは → あと|は|車|は) を、
/// 助詞と合わせた読み (は＋ぐるま → はぐるま) の 1 語 (歯車) に替える。辞書だけの変換は あと歯車は と正しく選べているが、AI が崩す。
/// 連濁の形の読みの語は表 rendaku_readings.tsv (読み (カタカナ)<TAB>表記。tools/rendaku/make_table.py が作る): 同じ表記に頭の 1 字の濁らない読み
/// (車 くるま) もあり、濁った読みは複合語の中でしか使われないもの (コーパスで単独の語として濁った読みがほとんど出ない。側 がわ は入らない)。
/// 語ごとの決まりは作らない。助詞と合わせた読みを辞書だけで引き、その語で終わる 1 語 (歯車・羽車) を最大 4 つ取り、
/// (助詞の側は AI の結果で あとは のような 1 語になっていることがあるので、その語の末尾 1〜3 字を合わせる: あと|は＋ぐるま → あと|歯車)
/// それぞれに替えた文を zenz に文全体で採点させて、いちばん点の良いものにする。替えられる 1 語が無ければ替えない
@MainActor var rendakuReadings: Set<String>? = nil
/// 表は 1 行 1 組、数千行の想定
let rendakuTableLimits = (maxBytes: 1 << 20, maxLines: 50_000, maxFieldLength: 64)

/// 比べるための開発用の切り替え: AZK_RENDAKU=0 で替えない
let rendakuFix = devEnvironment["AZK_RENDAKU"] != "0"
/// 測るための開発用の記録: AZK_RENDAKU_LOG=<ファイル> に、替える所が見つかった変換ごとに 1 行
let rendakuLogPath = devEnvironment["AZK_RENDAKU_LOG"]

@MainActor func loadRendakuReadings() -> Set<String> {
    if let table = rendakuReadings { return table }
    var table: Set<String> = []
    for cols in BoundedTable.rows(execURL.appendingPathComponent("rendaku_readings.tsv"), maxBytes: rendakuTableLimits.maxBytes,
                                  maxLines: rendakuTableLimits.maxLines, columns: 2...2, maxFieldLength: rendakuTableLimits.maxFieldLength) {
        guard !cols[0].isEmpty, !cols[1].isEmpty else { continue }
        table.insert(hiraganaToKatakana(cols[0]) + "\t" + cols[1])
    }
    rendakuReadings = table
    return table
}

/// AI の結果の語は、辞書の品詞の番号でなく一般名詞 1285 が付いていることがあるので、助詞は番号か字で見る
let rendakuParticles: Set<String> = ["は", "が", "を", "に", "の", "と", "も", "で", "へ", "や", "か", "から", "まで", "より"]

/// 連濁の形の読みの語が、助詞 (ひらがなの語) のすぐ後ろに来ている所 (語の位置)
func rendakuAfterParticle(_ data: [DicdataElement], table: Set<String>) -> [Int] {
    guard data.count >= 2 else { return [] }
    return (1..<data.count).filter { i in
        let e = data[i], prev = data[i - 1]
        return table.contains(e.ruby + "\t" + e.word) && (CIDData.isJoshi(cid: prev.rcid) || rendakuParticles.contains(prev.word))
            && !prev.word.isEmpty && prev.word.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) }
    }
}

/// 替えた候補 (替えられる所が無ければ nil)
@MainActor func replacingRendakuWords(_ best: Candidate, hiragana: String, context: String, fast: Bool) -> Candidate? {
    let table = loadRendakuReadings()
    guard !table.isEmpty else { return nil }
    let spots = rendakuAfterParticle(best.data, table: table)
    guard !spots.isEmpty else { return nil }
    let started = Date()
    var alternativeCount = 0, scored = 0
    let dictOptions = getOptions(context: context, prediction: false, zenzai: false, fast: true)
    let zenzOptions = getOptions(context: context, prediction: false, zenzai: true, fast: fast)
    var current = best.data
    var changed = false
    // 後ろから替える (前の所を替えても後ろの位置がずれないように)
    for i in spots.reversed() {
        let word = current[i].word
        let prev = current[i - 1]
        let prevRuby = Array(prev.ruby.toHiragana())
        // (助詞の側の語の末尾から合わせた字数, 替える 1 語)。助詞 1 字 (は) から
        var alternatives: [(take: Int, data: DicdataElement)] = []
        var seen: Set<String> = []
        for take in 1...min(3, prevRuby.count) {
            let reading = String(prevRuby.suffix(take)) + current[i].ruby.toHiragana()
            let sub = directComposingText(reading)
            for c in converter.requestCandidates(sub, options: dictOptions).mainResults
                where c.correspondingCount == sub.input.count && c.data.count == 1 && c.text.count > word.count && c.text.hasSuffix(word) && seen.insert(c.text).inserted {
                alternatives.append((take, c.data[0]))
            }
        }
        alternativeCount += min(alternatives.count, 4)
        var bestScore = -Float.infinity
        var bestData: [DicdataElement]? = nil
        for alternative in alternatives.prefix(4) {
            // 助詞の側の語の残り (あとは の あと)。ひらがなの語なので表記は読みと同じ。品詞は左の番号のまま (右も同じにする)
            let rest = prevRuby.count - alternative.take
            let head = rest == 0 ? [] : [DicdataElement(word: String(prevRuby.prefix(rest)), ruby: hiraganaToKatakana(String(prevRuby.prefix(rest))),
                                                        lcid: prev.lcid, rcid: prev.lcid, mid: prev.mid, value: prev.value())]
            let data = Array(current[..<(i - 1)]) + head + [alternative.data] + Array(current[(i + 1)...])
            scored += 1
            guard let lp = converter.zenzSequenceScore(hiragana: hiragana, output: data.map(\.word).joined(), leftSideContext: context, options: zenzOptions) else { continue }
            if lp > bestScore {
                bestScore = lp
                bestData = data
            }
        }
        if let data = bestData {
            current = data
            changed = true
        }
    }
    if let path = rendakuLogPath, let line = ("{\"reading\":\"\(jsonEscaped(hiragana))\",\"spots\":\(spots.count),\"alternatives\":\(alternativeCount),\"scored\":\(scored),"
        + "\"ms\":\(Int(-started.timeIntervalSinceNow * 1000)),\"before\":\"\(jsonEscaped(best.text))\",\"after\":\"\(jsonEscaped(current.map(\.word).joined()))\"}\n").data(using: .utf8),
       let handle = FileHandle(forWritingAtPath: path) ?? { FileManager.default.createFile(atPath: path, contents: nil); return FileHandle(forWritingAtPath: path) }() {
        handle.seekToEndOfFile(); handle.write(line); handle.closeFile()
    }
    guard changed else { return nil }
    return Candidate(text: current.map(\.word).joined(), value: best.value, correspondingCount: best.correspondingCount, lastMid: best.lastMid, data: current)
}

func jsonEscaped(_ s: String) -> String {
    s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
}
