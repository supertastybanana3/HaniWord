import Foundation
import XCTest
@testable import KanaKanjiConverterModule
@testable import azookey_server

// 2026-10-21: 英単語をローマ字として読んだかな (see → せえ) が、日本語としてよく使う語になるかを辞書で調べる (AZK_KANA_WORDS_IN を指定したときだけ)。
// 入力は 1 行 1 語 (英単語。タブの後ろは無視)。ローマ字としてかなに読み切れる語だけ、
// 「語<TAB>かな<TAB>辞書だけの 1 位<TAB>その語の数<TAB>1 位の点数<TAB>1 語で読み全体になる語<TAB>その点数<TAB>1 位の語の並び」を書き出す
final class KanaWordsProbeTests: XCTestCase {
    func testKanaWordsProbe() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let stage = env["AZK_TEST_DICTIONARY_STAGE"], let input = env["AZK_KANA_WORDS_IN"], let output = env["AZK_KANA_WORDS_OUT"] else {
            throw XCTSkip("Run with an isolated dictionary stage and a word list")
        }
        try await MainActor.run {
            let root = URL(filePath: stage)
            let converter = KanaKanjiConverter()
            let options = ConvertRequestOptions(requireJapanesePrediction: false, requireEnglishPrediction: false,
                keyboardLanguage: .ja_JP, learningType: .nothing, dictionaryResourceURL: root.appendingPathComponent("Dictionary"),
                memoryDirectoryURL: root.appendingPathComponent("memory"), sharedContainerURL: root.appendingPathComponent("shared"),
                textReplacer: .empty, metadata: nil)
            var lines: [String] = []
            for line in try String(contentsOfFile: input, encoding: .utf8).split(whereSeparator: \.isNewline) {
                let word = String(line.split(separator: "\t").first ?? "")
                var roman = ComposingText()
                roman.insertAtCursorPosition(word, inputStyle: .roman2kana)
                // 末尾の n 1 つは確定の時点で ん (open = おぺん)
                var kana = roman.convertTarget
                if kana.hasSuffix("n") { kana = String(kana.dropLast()) + "ん" }
                if word.isEmpty || kana.unicodeScalars.contains(where: { $0.isASCII }) { continue }
                let parts = [word, kana]
                var composing = ComposingText()
                composing.insertAtCursorPosition(kana, inputStyle: .direct)
                let count = composing.input.count
                let results = converter.requestCandidates(composing, options: options).mainResults.filter { $0.correspondingCount == count }
                let best = results.first
                let single = results.first { $0.data.count == 1 && $0.text != parts[1] }
                // 1 位の語の並び: 表記:点数:助詞なら j
                let elements = (best?.data ?? []).map { "\($0.word):\(String(format: "%.2f", Double($0.value()))):\(CIDData.isJoshi(cid: $0.lcid) ? "j" : "")" }.joined(separator: "|")
                lines.append([parts[0], parts[1], best?.text ?? "", String(best?.data.count ?? 0),
                    String(format: "%.2f", Double(best?.value ?? -99)), single?.text ?? "", String(format: "%.2f", Double(single?.value ?? -99)), elements].joined(separator: "\t"))
                converter.stopComposition()
            }
            try (lines.joined(separator: "\n") + "\n").write(toFile: output, atomically: true, encoding: .utf8)
        }
    }
}
