import Foundation
import KanaKanjiConverterModule
import XCTest

final class ZenzSessionTests: XCTestCase {
    func testScoresSurviveRepeatedSessionResets() async throws {
        guard let path = ProcessInfo.processInfo.environment["AZK_TEST_MODEL_PATH"] else {
            throw XCTSkip("Requires an isolated, pinned test model")
        }
        try await MainActor.run {
            let converter = KanaKanjiConverter()
            let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("azk-score-reset-\(UUID())")
            let options = ConvertRequestOptions(requireJapanesePrediction: false, requireEnglishPrediction: false,
                keyboardLanguage: .ja_JP, learningType: .nothing,
                dictionaryResourceURL: scratch.appendingPathComponent("Dictionary"),
                memoryDirectoryURL: scratch.appendingPathComponent("memory"), sharedContainerURL: scratch,
                textReplacer: .empty,
                zenzaiMode: .on(weight: URL(filePath: path), inferenceLimit: 3,
                    requestRichCandidates: true, personalizationMode: nil,
                    versionDependentMode: .v3(.init(profile: "", leftSideContext: ""))), metadata: nil)
            let reading = "ふぉろろまーののふぉろってどういういみ"
            let outputs = ["フォロロマーノのフォロってどういう意味", "フォロロマーノノフォロッテドウイウイミ"]
            let baseline = try XCTUnwrap(converter.zenzScoreSequences(hiragana: reading, outputs: outputs, options: options))
            XCTAssertEqual(baseline.count, 2)
            XCTAssertGreaterThan(baseline[0], baseline[1])
            for _ in 0..<3 {
                converter.stopComposition()
                let scores = try XCTUnwrap(converter.zenzScoreSequences(hiragana: reading, outputs: outputs, options: options))
                XCTAssertEqual(scores.count, baseline.count)
                for (actual, expected) in zip(scores, baseline) {
                    XCTAssertTrue(actual.isFinite)
                    XCTAssertEqual(actual, expected, accuracy: 0.01)
                }
            }
        }
    }
}
