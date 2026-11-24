import XCTest
@testable import azookey_server

/// 2026-11-08: 学習をまっさらにする (learning_reset.swift) の、控えの取り方と消し方。
/// azooKey の memory.* とメモリの学習は変換器 (DicdataStore の resetMemory) が消すので、ここでは控えと HaniWord の表だけを見る
final class LearningResetTests: XCTestCase {
    private let fm = FileManager.default
    private let azooKeyFiles = ["memory.loudstxt3", "memory.loudschars2", "memory.memorymetadata", "memory.louds"]
    private let userWritten = ["learning_ng.txt", "english_terms_personal.txt", "settings.json"]

    private func makeBase() throws -> URL {
        let base = fm.temporaryDirectory.appendingPathComponent("haniword_reset_\(UUID().uuidString)")
        let memory = base.appendingPathComponent("memory")
        try fm.createDirectory(at: memory, withIntermediateDirectories: true)
        for name in azooKeyFiles + serverLearnedTableNames {
            try Data("x\n".utf8).write(to: memory.appendingPathComponent(name))
        }
        for name in clientLearnedFileNames + userWritten {
            try Data("x\n".utf8).write(to: base.appendingPathComponent(name))
        }
        return base
    }

    private func exists(_ url: URL) -> Bool {
        fm.fileExists(atPath: url.path)
    }

    /// (a) 変換の学習だけ: memory の中身を全部控えに写し、HaniWord の表は消さない
    func testConversionLearningOnlyBacksUpMemoryAndKeepsTheTables() throws {
        let base = try makeBase()
        defer { try? fm.removeItem(at: base) }
        let backup = try backUpAndRemoveLearningFiles(base: base, includeTables: false, stamp: "20261108-230000")
        XCTAssertEqual(backup.lastPathComponent, "memory_backup_20261108-230000")
        for name in azooKeyFiles + serverLearnedTableNames {
            XCTAssertTrue(exists(backup.appendingPathComponent("memory").appendingPathComponent(name)), name)
            XCTAssertTrue(exists(base.appendingPathComponent("memory").appendingPathComponent(name)), name)
        }
        for name in clientLearnedFileNames + userWritten {
            XCTAssertTrue(exists(base.appendingPathComponent(name)), name)
        }
        XCTAssertTrue(exists(base.appendingPathComponent(learningResetMarkName)))
    }

    /// (b) HaniWord の学習の表も: 控えに写してから消す。利用者が書く表と設定は残す
    func testTablesAreBackedUpThenRemovedButUserWrittenFilesStay() throws {
        let base = try makeBase()
        defer { try? fm.removeItem(at: base) }
        let backup = try backUpAndRemoveLearningFiles(base: base, includeTables: true, stamp: "20261108-230100")
        for name in serverLearnedTableNames {
            XCTAssertTrue(exists(backup.appendingPathComponent("memory").appendingPathComponent(name)), name)
            XCTAssertFalse(exists(base.appendingPathComponent("memory").appendingPathComponent(name)), name)
        }
        for name in clientLearnedFileNames {
            XCTAssertTrue(exists(backup.appendingPathComponent(name)), name)
            XCTAssertFalse(exists(base.appendingPathComponent(name)), name)
        }
        for name in userWritten {
            XCTAssertTrue(exists(base.appendingPathComponent(name)), name)
        }
        // 同じ日時でもう一度消すと、控えは別の名前 (_2) になる (前の控えを上書きしない)
        let again = try backUpAndRemoveLearningFiles(base: base, includeTables: true, stamp: "20261108-230100")
        XCTAssertEqual(again.lastPathComponent, "memory_backup_20261108-230100_2")
        XCTAssertTrue(exists(backup.appendingPathComponent("memory").appendingPathComponent(serverLearnedTableNames[0])))
    }
}
