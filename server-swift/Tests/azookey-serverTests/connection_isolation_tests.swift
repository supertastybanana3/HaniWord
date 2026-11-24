import Foundation
import XCTest
@testable import azookey_server

/// 2026-11-23 (セキュリティの点検 13・14): 通常の接続で覚えたものが、別の接続 (隔離のアプリを含む) の候補と順位に出ないこと
final class ConnectionIsolationTests: XCTestCase {
    /// 13: 疑問文の か で替えた文節の もとの表記 (長野家) は、覚えた接続の一覧にだけ入る。接続が切れたら消える
    @MainActor
    func testQuestionKaOriginalStaysWithItsConnection() {
        let saved = questionKaOriginals
        defer {
            release_ipc_session(401)
            release_ipc_session(402)
            questionKaOriginals = saved
        }
        select_ipc_session(401)
        questionKaOriginals = []
        noteQuestionKaOriginal((reading: "ながのか", text: "長野家"))
        XCTAssertEqual(placingQuestionKaOriginal(["長野か", "長野家"], reading: "ながのか"), ["長野家", "長野か"])
        select_ipc_session(402)
        XCTAssertEqual(placingQuestionKaOriginal(["長野か", "中野か"], reading: "ながのか"), ["長野か", "中野か"])
        select_ipc_session(401)
        XCTAssertEqual(placingQuestionKaOriginal(["長野か"], reading: "ながのか"), ["長野家", "長野か"])
        release_ipc_session(401)
        select_ipc_session(401)
        XCTAssertEqual(placingQuestionKaOriginal(["長野か"], reading: "ながのか"), ["長野か"])
    }

    /// 14: L2 で外した学習の語の表は、学習を使ってよい接続でだけ引く
    @MainActor
    func testDroppedLearnedIsOnlyReadWithPersonalData() throws {
        guard learnGuardEnabled else { throw XCTSkip("AZK_LEARN_GUARD=0") }
        let saved = personalDataAllowed
        defer {
            personalDataAllowed = saved
            learnGuardDropped.clear()
        }
        learnGuardDropped.clear()
        learnGuardDropped.note(ruby: "キョウシツ", word: "狂疾")
        personalDataAllowed = true
        XCTAssertEqual(droppedLearnedForList(hiragana: "きょうしつ").map(\.word), ["狂疾"])
        personalDataAllowed = false
        XCTAssertTrue(droppedLearnedForList(hiragana: "きょうしつ").isEmpty)
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ: 本物の変換で、通常の接続で 長野か？ と変換したあとも、
    /// 隔離の接続と別の通常の接続の ながのか の一覧は、変換する前と同じ (もとの表記 長野家 が頭に入らない)
    @MainActor
    func testQuestionKaOriginalDoesNotReachOtherConnections() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        let saved = personalDataAllowed
        defer {
            personalDataAllowed = saved
            clearPreviewMemo()
            for id: UInt64 in [411, 412, 413, 414] { release_ipc_session(id) }
        }
        use(connection: 411, personal: false)
        let isolatedBefore = listed("ながのか")
        use(connection: 412, personal: true)
        let normalBefore = listed("ながのか")

        use(connection: 413, personal: true)
        clearPreviewMemo()  // 前の試験の変換の覚えを使うと、もとの表記を覚える道を通らないため
        XCTAssertEqual(converted("ながのか？"), ["長野か", "？"])
        XCTAssertEqual(listed("ながのか").first, "長野家", "覚えた接続では、もとの表記が頭に入る")

        use(connection: 414, personal: false)
        XCTAssertEqual(listed("ながのか"), isolatedBefore)
        use(connection: 412, personal: true)
        XCTAssertEqual(listed("ながのか"), normalBefore)
    }

    /// 辞書の置き場 (AZK_QUESTION_KA_STAGE) があるときだけ: 通常の接続で外した学習の語 (の覚え) があっても、
    /// 隔離の接続の一覧は、覚えが無いときと候補も順位も同じ。通常の接続では 2 番目に上がる (覚えが効いていることの確かめ)
    @MainActor
    func testDroppedLearnedDoesNotReorderIsolatedLists() throws {
        guard let stage = ProcessInfo.processInfo.environment["AZK_QUESTION_KA_STAGE"] else {
            throw XCTSkip("Run with an isolated dictionary stage")
        }
        guard learnGuardEnabled else { throw XCTSkip("AZK_LEARN_GUARD=0") }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        let saved = personalDataAllowed
        defer {
            personalDataAllowed = saved
            learnGuardDropped.clear()
            clearPreviewMemo()
            for id: UInt64 in [421, 422] { release_ipc_session(id) }
        }
        learnGuardDropped.clear()
        use(connection: 421, personal: false)
        let before = listed("きしゃ")
        XCTAssertGreaterThan(before.count, 3, "\(before)")
        // 1・2 番目のどれも その語で始まらない、3 番目より後ろの語 (通常の接続で外した学習の語の代わり)
        let word = try XCTUnwrap(before.dropFirst(2).first { w in !before.prefix(2).contains { $0.hasPrefix(w) } })
        learnGuardDropped.note(ruby: "キシャ", word: word)

        use(connection: 422, personal: true)
        let promoted = listed("きしゃ")
        XCTAssertTrue(promoted.dropFirst().first?.hasPrefix(word) == true, "通常の接続では 2 番目に上がる: \(word) \(promoted.prefix(4))")
        use(connection: 421, personal: false)
        XCTAssertEqual(listed("きしゃ"), before)
    }

    @MainActor
    private func use(connection: UInt64, personal: Bool) {
        select_ipc_session(connection)
        // 試験では学習を書き出さないよう、SetPersonalDataAllowed (切り替えのときに保存する) を通さずに替える
        if personalDataAllowed != personal {
            personalDataAllowed = personal
            clearPreviewMemo()
            converter.stopComposition()
        }
    }

    @MainActor
    private func listed(_ reading: String) -> [String] {
        var count: Int32 = 0
        let pointer = reading.withCString { r in "".withCString { c in list_candidates(hiraganaPtr: r, contextPtr: c, lengthPtr: &count) } }
        defer { free_candidates(pointer, count) }
        return (0..<Int(count)).compactMap { i in pointer[i].map { String(cString: $0.pointee.text) } }
    }

    @MainActor
    private func converted(_ reading: String) -> [String] {
        var count: Int32 = 0
        let pointer = reading.withCString { r in "".withCString { c in
            convert_segments(hiraganaPtr: r, contextPtr: c, fast: false, dictionaryOnly: true, lengthPtr: &count)
        } }
        defer { free_candidates(pointer, count) }
        return (0..<Int(count)).compactMap { i in pointer[i].map { String(cString: $0.pointee.text) } }
    }
}
