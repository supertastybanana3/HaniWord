import Foundation
import XCTest
@testable import azookey_server

/// 2026-11-06 (仕事 (2) 数字＋かい・(3) 問いの か): 辞書だけの変換の上位の候補を、語ごとの読みと品詞の番号つきで書き出す (直し方を決めるための調べ)。
/// AZK_DUMP_STAGE (辞書の置き場)・AZK_DUMP_CASES (読み<TAB>… の組のファイル、; で区切って複数)・AZK_DUMP_OUT (出力のフォルダ) があるときだけ。
/// 組ごとに <出力>/<組の名前>.dump.txt に、読みごとに ConvertSegments の答えと、上位 8 個の候補 (表記 = 語/読み/左の番号/右の番号 …) を書く
@MainActor
final class CandidateDumpProbeTests: XCTestCase {
    func testCandidateDump() throws {
        let env = ProcessInfo.processInfo.environment
        guard let stage = env["AZK_DUMP_STAGE"], let cases = env["AZK_DUMP_CASES"], let out = env["AZK_DUMP_OUT"] else {
            throw XCTSkip("Run with a dictionary stage and case files")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        for path in cases.split(separator: ";").map(String.init) where !path.isEmpty {
            var lines: [String] = []
            for line in try String(contentsOfFile: path, encoding: .utf8).split(whereSeparator: \.isNewline) {
                let parts = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
                guard !parts.isEmpty, !parts[0].isEmpty, !parts[0].hasPrefix("#") else { continue }
                var count: Int32 = 0
                let list = parts[0].withCString { reading in "".withCString { context in
                    convert_segments(hiraganaPtr: reading, contextPtr: context, fast: false, dictionaryOnly: true, lengthPtr: &count)
                } }
                var text = ""
                for i in 0..<Int(count) {
                    if let c = list[i] { text += String(cString: c.pointee.text) }
                }
                lines.append("== \(parts[0])\t\(parts.count > 1 ? parts[1] : "")\t→ \(text)")
                let normalized = normalizeForConversion(parts[0])
                if let marks = questionKaMarks(normalized) {
                    let split = conversionSegmentsUncached(hiragana: normalized, context: "", fast: false, dictionaryOnly: true, questionKa: .split)
                    let whole = conversionSegmentsUncached(hiragana: normalized, context: "", fast: false, dictionaryOnly: true, questionKa: .whole)
                    let show: ([(word: String, rubyCount: Int)]) -> String = { $0.map { "\($0.word)/\($0.rubyCount)" }.joined(separator: " ") }
                    // ？ ごとの読みの辞書の候補 (上位 50) で、切った形と ？ ごとの形の点数
                    let wholeComposing = directComposingText(normalized)
                    let all = converter.requestCandidates(wholeComposing, options: getOptions(context: "", prediction: false, zenzai: false, nBest: 50)).mainResults
                        .filter { $0.correspondingCount == wholeComposing.input.count }
                    let value: (String) -> String = { text in all.first(where: { $0.text == text }).map { String(format: "%.2f", Double($0.value)) } ?? "-" }
                    let splitText = split.map(\.word).joined(), wholeText = whole.map(\.word).joined()
                    lines.append("  [question] marks=\(marks) split=\(show(split)) whole=\(show(whole)) value_split=\(value(splitText)) value_whole=\(value(wholeText))")
                } else if normalized != parts[0] {
                    lines.append("  [question] normalized=\(normalized)")
                }
                let composing = directComposingText(parts[0])
                let n = composing.input.count
                let results = converter.requestCandidates(composing, options: getOptions(context: "", prediction: false, zenzai: false)).mainResults
                for candidate in results.filter({ $0.correspondingCount == n }).prefix(8) {
                    let words = candidate.data.map { "\($0.word)/\($0.ruby)/\($0.lcid)/\($0.rcid)" }.joined(separator: " ")
                    lines.append("  \(candidate.text)\t\(String(format: "%.2f", Double(candidate.value)))\t\(words)")
                }
            }
            let name = URL(filePath: path).lastPathComponent
            try (lines.joined(separator: "\n") + "\n").write(toFile: out + "/" + name + ".dump.txt", atomically: true, encoding: .utf8)
        }
    }
}
