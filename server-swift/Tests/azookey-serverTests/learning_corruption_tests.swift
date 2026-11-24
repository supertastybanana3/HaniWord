import Foundation
import XCTest
@testable import KanaKanjiConverterModule

/// 2026-11-23 (セキュリティの点検 12): 途中で切れた・件数や位置の壊れた学習のファイルを、merge (保存) と変換が範囲を確かめずに切り出して落ちていた。
/// 読む前に確かめ、壊れていれば memory_corrupt_<日時> に退けて、空の学習から続ける
final class LearningCorruptionTests: XCTestCase {
    private let chars: [Character: UInt8] = ["タ": 1, "カ": 2, "イ": 3, "コ": 4]
    private var parent: URL!
    private var directory: URL!
    private var reports: [(reason: String, count: Int, folder: String)] = []
    private var savedReport: ((String, Int, String) -> Void)?

    override func setUpWithError() throws {
        parent = FileManager.default.temporaryDirectory.appendingPathComponent("azk-learning-corrupt-\(UUID())")
        directory = parent.appendingPathComponent("memory", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        savedReport = LearningCorruption.report
        reports = []
        LearningCorruption.report = { [unowned self] reason, count, folder in self.reports.append((reason, count, folder)) }
    }

    override func tearDownWithError() throws {
        LearningCorruption.report = savedReport
        try? FileManager.default.removeItem(at: parent)
    }

    private func element(_ word: String, _ ruby: String) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, cid: CIDData.一般名詞.cid, mid: MIDData.一般.mid, value: -10)
    }

    private func save(_ word: String, _ ruby: String) throws {
        var recent = TemporalLearningMemoryTrie()
        recent.memorize(dicdataElement: element(word, ruby), chars: ruby.map { chars[$0]! })
        try LongTermLearningMemory.merge(tempTrie: recent, directoryURL: directory, maxMemoryCount: 100, char2UInt8: chars)
    }

    /// 学習のファイルの .loudstxt3 にある語 (読み順)
    private func storedWords() throws -> [String] {
        let paths = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
            .filter { $0.lastPathComponent.hasSuffix(".loudstxt3") }
        var words: [String] = []
        for path in paths {
            let data = try Data(contentsOf: path)
            let count = data.withUnsafeBytes { Int($0.loadUnaligned(as: UInt16.self)) }
            let indices: [Int] = (0..<count).map { i in data.withUnsafeBytes { Int($0.loadUnaligned(fromByteOffset: 2 + i * 4, as: UInt32.self)) } }
            for (i, start) in indices.enumerated() {
                words += LOUDS.parseBinary(binary: data[start..<(i + 1 < count ? indices[i + 1] : data.count)]).map(\.word)
            }
        }
        return words.sorted()
    }

    private func learnThree() throws {
        try save("高い", "タカイ")
        try save("鯛", "タイ")
        try save("蛸", "タコ")
        XCTAssertNil(LongTermLearningMemory.corruption(directoryURL: directory))
        XCTAssertEqual(try storedWords(), ["蛸", "高い", "鯛"].sorted())
    }

    private func file(_ name: String) -> URL { directory.appendingPathComponent(name) }

    private func corruptFolders() throws -> [URL] {
        try FileManager.default.contentsOfDirectory(at: parent, includingPropertiesForKeys: nil)
            .filter { $0.lastPathComponent.hasPrefix("memory_corrupt_") }
    }

    /// 壊したあとの保存で落ちず、壊れたファイルは退け、学習は新しく覚えた語だけから続く。知らせには学習の中身が入らない
    private func assertRecoversFrom(_ corrupt: () throws -> Void, file: StaticString = #filePath, line: UInt = #line) throws {
        try learnThree()
        try corrupt()
        XCTAssertNotNil(LongTermLearningMemory.corruption(directoryURL: directory), file: file, line: line)
        try save("髙い", "タカイ")
        XCTAssertNil(LongTermLearningMemory.corruption(directoryURL: directory), file: file, line: line)
        XCTAssertEqual(try storedWords(), ["髙い"], file: file, line: line)
        let folders = try corruptFolders()
        XCTAssertEqual(folders.count, 1, file: file, line: line)
        guard folders.count == 1 else { return }
        let moved = try FileManager.default.contentsOfDirectory(at: folders[0], includingPropertiesForKeys: nil)
        XCTAssertTrue(moved.contains { $0.lastPathComponent == "memory.memorymetadata" } || moved.contains { $0.lastPathComponent == "memory.louds" },
                      "\(moved.map(\.lastPathComponent))", file: file, line: line)
        XCTAssertEqual(reports.count, 1, file: file, line: line)
        XCTAssertEqual(reports.first?.count, moved.count, file: file, line: line)
        XCTAssertEqual(reports.first?.folder, folders[0].lastPathComponent, file: file, line: line)
        for word in ["高い", "鯛", "蛸", "タカイ", "タイ", "タコ"] {
            XCTAssertFalse(reports.first?.reason.contains(word) ?? true, file: file, line: line)
        }
    }

    private func rewrite(_ name: String, _ change: (inout Data) -> Void) throws {
        var data = try Data(contentsOf: file(name))
        change(&data)
        try data.write(to: file(name))
    }

    func testValidLearningIsKeptAsIs() throws {
        try learnThree()
        try save("髙い", "タカイ")
        XCTAssertEqual(try storedWords(), ["蛸", "高い", "髙い", "鯛"].sorted())
        XCTAssertTrue(try corruptFolders().isEmpty)
        XCTAssertTrue(reports.isEmpty)
    }

    func testTruncatedMetadata() throws {
        try assertRecoversFrom { try rewrite("memory.memorymetadata") { $0 = $0.prefix($0.count - 3) } }
    }

    func testEntryCountTooLargeInMetadata() throws {
        try assertRecoversFrom { try rewrite("memory.memorymetadata") { $0.replaceSubrange(0..<4, with: [0xFF, 0xFF, 0x0F, 0x00]) } }
    }

    func testBrokenCountInLoudstxt3() throws {
        try assertRecoversFrom { try rewrite("memory0.loudstxt3") { $0.replaceSubrange(0..<2, with: [0xFF, 0xFF]) } }
    }

    func testReversedOffsetsInLoudstxt3() throws {
        try assertRecoversFrom {
            try rewrite("memory0.loudstxt3") { data in
                // 3 番目と 4 番目の項目の位置を入れ替える (逆順になる)
                let a = data.subdata(in: 2 + 4 * 2 ..< 2 + 4 * 3)
                let b = data.subdata(in: 2 + 4 * 3 ..< 2 + 4 * 4)
                data.replaceSubrange(2 + 4 * 2 ..< 2 + 4 * 3, with: b)
                data.replaceSubrange(2 + 4 * 3 ..< 2 + 4 * 4, with: a)
            }
        }
    }

    func testTruncatedLoudstxt3() throws {
        try assertRecoversFrom { try rewrite("memory0.loudstxt3") { $0 = $0.prefix($0.count / 2) } }
    }

    /// louds だけが壊れたときは、退けずに今は読まず (変換が LOUDS をたどらない)、次の保存が metadata と loudstxt3 から作り直す (学習を失わない)
    func testBrokenLoudsIsRebuiltWithoutLosingLearning() throws {
        try learnThree()
        try rewrite("memory.louds") { $0.replaceSubrange(0..<8, with: [UInt8](repeating: 0, count: 8)) }
        XCTAssertNotNil(LongTermLearningMemory.corruption(directoryURL: directory))
        XCTAssertFalse(LongTermLearningMemory.prepareForReading(directoryURL: directory))
        XCTAssertTrue(FileManager.default.fileExists(atPath: file("memory.louds").path))
        try save("髙い", "タカイ")
        XCTAssertNil(LongTermLearningMemory.corruption(directoryURL: directory))
        XCTAssertTrue(LongTermLearningMemory.prepareForReading(directoryURL: directory))
        XCTAssertEqual(try storedWords(), ["蛸", "高い", "髙い", "鯛"].sorted())
        XCTAssertTrue(try corruptFolders().isEmpty)
        XCTAssertTrue(reports.isEmpty)
    }

    func testMetadataMissingWhileOtherFilesRemain() throws {
        try assertRecoversFrom { try FileManager.default.removeItem(at: file("memory.memorymetadata")) }
    }

    /// 変換が学習を読む前 (prepareForReading): 壊れていれば退けて読まない。壊れていなければそのまま読む
    func testPrepareForReadingMovesCorruptLearningAside() throws {
        try learnThree()
        XCTAssertTrue(LongTermLearningMemory.prepareForReading(directoryURL: directory))
        try rewrite("memory.memorymetadata") { $0 = $0.prefix($0.count - 3) }
        XCTAssertFalse(LongTermLearningMemory.prepareForReading(directoryURL: directory))
        XCTAssertFalse(FileManager.default.fileExists(atPath: file("memory.memorymetadata").path))
        XCTAssertEqual(try corruptFolders().count, 1)
        // 退けたあとは学習が無い (読んでよい)
        XCTAssertTrue(LongTermLearningMemory.prepareForReading(directoryURL: directory))
    }

    /// 測り (AZK_LEARNING_PERF=<項目の数> があるときだけ): 大きめの学習で、検査 (corruption) 1 回と保存 1 回 (merge。検査を含む) の時間の中央値 (5 回)。
    /// 直す前の保存 ≈ 保存 − 検査。変換が学習を読み直すとき (起動・保存のあと) は、検査がもう 1 回かかる
    func testLargeLearningTimes() throws {
        guard let env = ProcessInfo.processInfo.environment["AZK_LEARNING_PERF"] else { throw XCTSkip("AZK_LEARNING_PERF") }
        let target = Int(env) ?? 30000
        let kana = Array("アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホ")
        var map: [Character: UInt8] = [:]
        for (i, c) in kana.enumerated() { map[c] = UInt8(i + 1) }
        var recent = TemporalLearningMemoryTrie()
        var seen = Set<String>()
        var x: UInt64 = 1123
        func next() -> Int { x = x &* 6364136223846793005 &+ 1442695040888963407; return Int(x >> 33) }
        while seen.count < target {
            let ruby = String((0 ..< 3 + next() % 4).map { _ in kana[next() % kana.count] })
            guard seen.insert(ruby).inserted else { continue }
            recent.memorize(dicdataElement: element("語\(seen.count)", ruby), chars: ruby.map { map[$0]! })
        }
        let t0 = Date()
        try LongTermLearningMemory.merge(tempTrie: recent, directoryURL: directory, maxMemoryCount: 1 << 20, char2UInt8: map)
        let build = Date().timeIntervalSince(t0)
        // 書いたばかりのファイルを最初に開くときだけ遅い (Windows の調べ) ので、各回の頭で全部を 1 回読み (first_read)、そのあとで検査を測る。
        // 保存の 検査あり・なし は、回ごとに先と後を入れ替える
        var firstReads: [Double] = []
        var reads: [Double] = []
        var checks: [Double] = []
        var loads: [Double] = []
        var saves: [Double] = []
        var savesBefore: [Double] = []
        defer { LongTermLearningMemory.skipMergeCheckForMeasurement = false }
        func readAll() throws {
            for url in try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil) where !url.lastPathComponent.hasSuffix(".2") {
                _ = try Data(contentsOf: url)
            }
        }
        for round in 0 ..< 6 {
            var t = Date()
            try readAll()
            firstReads.append(Date().timeIntervalSince(t))
            t = Date()
            try readAll()
            reads.append(Date().timeIntervalSince(t))
            t = Date()
            XCTAssertNil(LongTermLearningMemory.corruption(directoryURL: directory))
            checks.append(Date().timeIntervalSince(t))
            t = Date()
            XCTAssertTrue(LongTermLearningMemory.prepareForReading(directoryURL: directory))
            loads.append(Date().timeIntervalSince(t))
            for skip in round % 2 == 0 ? [false, true] : [true, false] {
                try readAll()
                LongTermLearningMemory.skipMergeCheckForMeasurement = skip
                var one = TemporalLearningMemoryTrie()
                one.memorize(dicdataElement: element("髙い", "タカイ"), chars: "タカイ".map { map[$0]! })
                t = Date()
                try LongTermLearningMemory.merge(tempTrie: one, directoryURL: directory, maxMemoryCount: 1 << 20, char2UInt8: map)
                if skip { savesBefore.append(Date().timeIntervalSince(t)) } else { saves.append(Date().timeIntervalSince(t)) }
                LongTermLearningMemory.skipMergeCheckForMeasurement = false
            }
        }
        let bytes = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: [.fileSizeKey])
            .reduce(0) { $0 + ((try? $1.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0) }
        let ms = { (v: Double) in String(format: "%.1f", v * 1000) }
        let med = { (xs: [Double]) in ms(xs.sorted()[xs.count / 2]) }
        print("LEARNING_PERF entries=\(target) bytes=\(bytes) build_ms=\(ms(build)) first_read_ms=\(med(firstReads)) read_all_ms=\(med(reads)) full_check_ms=\(med(checks)) load_check_ms=\(med(loads)) save_ms=\(med(saves)) save_without_check_ms=\(med(savesBefore)) saves=\(saves.map(ms)) saves_without=\(savesBefore.map(ms))")
        XCTAssertTrue(reports.isEmpty)
    }

    /// 読み出し側の守り: 件数のわりに短い項目は、範囲の外を読まずに空
    func testParseBinaryDoesNotReadPastTheEnd() {
        XCTAssertTrue(LOUDS.parseBinary(binary: Data([5, 0, 1, 2, 3])).isEmpty)
        XCTAssertTrue(LOUDS.parseBinary(binary: Data([1])).isEmpty)
    }
}
