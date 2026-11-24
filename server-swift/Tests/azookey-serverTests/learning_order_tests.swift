import Foundation
import XCTest
@testable import KanaKanjiConverterModule

final class LearningOrderTests: XCTestCase {
    private func word(_ text: String) -> DicdataElement {
        DicdataElement(word: text, ruby: "タカイ", cid: CIDData.一般名詞.cid,
            mid: MIDData.一般.mid, value: -10)
    }

    func testEqualFrequencyPrefersFirstAndHigherFrequencyStillWins() {
        var trie = TemporalLearningMemoryTrie()
        trie.memorize(dicdataElement: word("高い"), chars: [1, 2, 3])
        trie.memorize(dicdataElement: word("髙い"), chars: [1, 2, 3])
        var entries = trie.perfectMatch(chars: [1, 2, 3])
        XCTAssertGreaterThan(entries[0].value(), entries[1].value())
        trie.memorize(dicdataElement: word("髙い"), chars: [1, 2, 3])
        entries = trie.perfectMatch(chars: [1, 2, 3])
        XCTAssertGreaterThan(entries[1].value(), entries[0].value())
        trie.memorize(dicdataElement: word("高い"), chars: [1, 2, 3])
        entries = trie.perfectMatch(chars: [1, 2, 3])
        XCTAssertGreaterThan(entries[0].value(), entries[1].value())
    }

    private func stored(_ directory: URL) throws -> [DicdataElement] {
        let paths = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
            .filter { $0.lastPathComponent.hasSuffix(".loudstxt3") }
        var entries: [DicdataElement] = []
        for path in paths {
            let data = try Data(contentsOf: path)
            let count = data.withUnsafeBytes { Int($0.loadUnaligned(as: UInt16.self)) }
            let indices: [Int] = (0..<count).map { index in
                data.withUnsafeBytes { Int($0.loadUnaligned(fromByteOffset: 2 + index * 4, as: UInt32.self)) }
            }
            for (index, start) in indices.enumerated() {
                let end = index + 1 < count ? indices[index + 1] : data.count
                entries += LOUDS.parseBinary(binary: data[start..<end])
            }
        }
        return entries.filter { $0.ruby == "タカイ" }
    }

    func testSavingAndReloadingDoesNotPutNewLearningBeforeEstablishedWords() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("azk-learning-order-\(UUID())")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let chars: [Character: UInt8] = ["タ": 1, "カ": 2, "イ": 3]
        func save(_ text: String) throws {
            var recent = TemporalLearningMemoryTrie()
            recent.memorize(dicdataElement: word(text), chars: [1, 2, 3])
            try LongTermLearningMemory.merge(tempTrie: recent, directoryURL: directory,
                maxMemoryCount: 100, char2UInt8: chars)
        }
        try save("高い")
        try save("髙い")
        var entries = try stored(directory)
        XCTAssertEqual(entries.map(\.word), ["高い", "髙い"])
        XCTAssertGreaterThan(entries[0].value(), entries[1].value())
        var fresh = TemporalLearningMemoryTrie()
        fresh.memorize(dicdataElement: word("新しい表記"), chars: [1, 2, 3])
        XCTAssertGreaterThan(entries[1].value(), fresh.perfectMatch(chars: [1, 2, 3])[0].value())
        try save("髙い")
        entries = try stored(directory)
        XCTAssertGreaterThan(entries[1].value(), entries[0].value())
        try save("高い")
        entries = try stored(directory)
        XCTAssertEqual(entries.map(\.word), ["高い", "髙い"])
        XCTAssertGreaterThan(entries[0].value(), entries[1].value())
    }
}
