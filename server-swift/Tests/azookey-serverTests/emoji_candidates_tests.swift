import Foundation
import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

final class EmojiCandidatesTests: XCTestCase {
    func testEmojiAndVariantsFollowOrdinaryCandidatesWithoutDuplicates() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try "👍\tいいね,いいね\t👍🏻,👍🏿\n❤️\tいいね\t\n".write(to: url, atomically: true, encoding: .utf8)
        let replacer = TextReplacer { url }
        let ordinary = ["良いね", "いいね", "イイネ", "単漢字…"]
        let output = appendingEmojiCandidates(to: ["👍"] + ordinary + ["❤️"], reading: "イイネ", replacer: replacer)
        XCTAssertEqual(output, ordinary + ["👍", "👍🏻", "👍🏿", "❤️"])
        XCTAssertEqual(appendingEmojiCandidates(to: ordinary, reading: "みとうろく", replacer: replacer), ordinary)
        XCTAssertEqual(appendingEmojiCandidates(to: ordinary, reading: "いいね", replacer: .empty), ordinary)
    }
}
