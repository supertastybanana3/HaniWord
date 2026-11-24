import XCTest
@testable import azookey_server

final class RequestExpressionsTests: XCTestCase {
    func testBoundedRequestsKeepNounVerbScopesAndReadingLengths() {
        for (reading, expected) in [("ごろくれ", "語呂くれ"), ("ごろください", "語呂ください"),
            ("ごろをくれないか", "語呂をくれないか"), ("ごろちょうだい", "語呂ちょうだい"),
            ("ごろをおしえてくれ", "語呂を教えてくれ"), ("ごろあわせくれよ！", "語呂合わせくれよ！")] {
            let match = nominalRequest(reading: reading)
            XCTAssertEqual(match?.segments.map(\.word).joined(), expected)
            XCTAssertEqual(match?.segments.reduce(0) { $0 + $1.rubyCount }, reading.count)
            XCTAssertEqual(match?.prefix, "")
        }
        for reading in ["ごろくれね", "ごろくれ！", "せかいしのごろくれ", "ねんごうのごろください"] {
            let match = nominalRequest(reading: reading)
            XCTAssertNotNil(match)
            XCTAssertEqual((match?.prefix.count ?? 0) + (match?.segments.reduce(0) { $0 + $1.rubyCount } ?? 0), reading.count)
        }
    }

    func testSimilarWordsAndIncompleteInputAreNotRequests() {
        for reading in ["ごろ", "ごろく", "ごろくれびゅー", "ごろくれいをあげる", "ごろくれすと", "ひぐれ", "あきのおとずれ",
                        "くれし", "くれーん", "れいくれ", "ねごろくれ", "にごろください", "ごろくれない", "ごろくれた"] {
            XCTAssertNil(nominalRequest(reading: reading), reading)
        }
    }
}
