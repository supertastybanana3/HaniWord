import Foundation
import KanaKanjiConverterModule
import XCTest
@testable import azookey_server

/// 2026-11-10: 大きな表と辞書は、読み込まずにファイルを写像して持つ (colloc の表・先読みする本体の辞書の .loudstxt3)。
/// 写像しても読み込んでも、同じに引けること
final class MappedFilesTests: XCTestCase {
    func testCollocTableLoadsTheSameMappedOrRead() throws {
        let header = "# 見出し\n# 2 行目\n"
        let raw = header + "キク\tR1\t薬\t効く\t83\t利く:15\nキクカ\tR1\t薬\t菊花\t12\t\nキクカ\tR2\tを|飾る\t菊花\t20\t\n"
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("hw_colloc_\(UUID().uuidString).tsv")
        try Data(raw.utf8).write(to: url)
        for mapped in [true, false] {
            let table = CollocTableLoad(url: url, mapped: mapped).wait()
            XCTAssertFalse(table.isEmpty, "mapped \(mapped)")
            XCTAssertEqual(table.data.count, raw.utf8.count, "mapped \(mapped)")
            XCTAssertEqual(table.bodyStart, header.utf8.count, "mapped \(mapped)")
            XCTAssertEqual(table.choice("キク\tR1\t薬"), CollocChoice(word: "効く", count: 83, close: ["利く"]), "mapped \(mapped)")
            XCTAssertEqual(table.choice("キクカ\tR2\tを|飾る")?.count, 20, "mapped \(mapped)")
            XCTAssertNil(table.choice("キ\tR1\t薬"), "mapped \(mapped)")
        }
        // 無いファイルは空の表
        XCTAssertTrue(CollocTableLoad(url: url.appendingPathExtension("none"), mapped: true).wait().isEmpty)
        // 写像を離したあとに消す (Windows では写像のあいだ消せない。消せなくても試験は落とさない)
        try? FileManager.default.removeItem(at: url)
    }

    func testCollocTableFromSlicedDataAndHeaderOnly() {
        // 切り出した Data (startIndex が 0 でない) からでも同じに引ける
        let whole = Data(("xyz" + "# 見出し\nキク\tR1\t薬\t効く\t83\t利く:15\n").utf8)
        let sliced = CollocTable(data: whole[3...])
        XCTAssertEqual(sliced.data.startIndex, 0)
        XCTAssertEqual(sliced.choice("キク\tR1\t薬")?.word, "効く")
        // 頭の # の行だけ (改行なしで終わるものも) は空の表
        XCTAssertTrue(CollocTable(bytes: Array("# 見出し\n".utf8)).isEmpty)
        XCTAssertTrue(CollocTable(bytes: Array("# 見出し".utf8)).isEmpty)
        XCTAssertTrue(CollocTable().isEmpty)
    }

    /// 先読みした本体の辞書の .loudstxt3 を写像しても、読み込んだときと同じ候補になる (辞書の置き場があるときだけ)
    @MainActor
    func testPreloadedDictionaryGivesTheSameCandidatesMappedOrRead() throws {
        guard let path = ProcessInfo.processInfo.environment["AZK_TEST_DICTIONARY_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        let root = URL(filePath: path)
        let readings = ["とうきょうでほんをかう", "きょうはいいてんきですね", "かんきょうもんだいについてかんがえる", "しんかんせんのじこくひょう"]
        let saved = DicdataStore.mapPreloadedDictionary
        defer { DicdataStore.mapPreloadedDictionary = saved }
        var results: [[[String]]] = []
        for (preload, mapped) in [(true, true), (true, false), (false, true)] {
            DicdataStore.mapPreloadedDictionary = mapped
            let converter = KanaKanjiConverter()
            let options = ConvertRequestOptions(requireJapanesePrediction: false, requireEnglishPrediction: false,
                keyboardLanguage: .ja_JP, learningType: .nothing, dictionaryResourceURL: root.appendingPathComponent("Dictionary"),
                memoryDirectoryURL: root.appendingPathComponent("memory"), sharedContainerURL: root.appendingPathComponent("shared"),
                textReplacer: .empty, preloadDictionary: preload, metadata: nil)
            results.append(readings.map { reading in
                var input = ComposingText()
                input.insertAtCursorPosition(reading, inputStyle: .direct)
                return Array(converter.requestCandidates(input, options: options).mainResults.prefix(10).map(\.text))
            })
        }
        XCTAssertEqual(results[0].map(\.isEmpty), readings.map { _ in false })
        XCTAssertEqual(results[0], results[1], "写像と読み込み")
        XCTAssertEqual(results[0], results[2], "写像と先読みなし")
    }
}
