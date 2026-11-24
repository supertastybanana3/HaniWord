import KanaKanjiConverterModule
import XCTest
@testable import azookey_server

final class NounCoordinationTests: XCTestCase {
    private func candidate(_ words: [(String, String, Int)]) -> Candidate {
        let data = words.map { DicdataElement(word: $0.0, ruby: $0.1, cid: $0.2, mid: 501, value: -10) }
        return Candidate(text: data.map(\.word).joined(), value: -10,
            correspondingCount: data.reduce(0) { $0 + $1.ruby.count }, lastMid: 501, data: data)
    }

    func testBothNounsAndFullDictionaryPathAreRequired() {
        let input = candidate([("リスト", "リスト", 1285), ("キウイ", "キウイ", 1285)])
        let noun = candidate([("リス", "リス", 1285)])
        let output = candidate([("リス", "リス", 1285), ("と", "ト", 147), ("キウイ", "キウイ", 1285)])
        let result = nounCoordinationAlternative(input) { reading, text in
            if reading == "りす" && text == "リス" { return noun }
            if reading == "りすときうい" && text == "リスとキウイ" { return output }
            return nil
        }
        XCTAssertEqual(result?.text, "リスとキウイ")
        XCTAssertEqual(result?.correspondingCount, input.correspondingCount)
        XCTAssertNil(nounCoordinationAlternative(input) { _, _ in nil })
        XCTAssertNil(nounCoordinationAlternative(input) { reading, _ in reading == "りす" ? noun : nil })
    }

    func testNoSplitInsideWholeWordsOrBeforeNonNouns() {
        for input in [candidate([("トマトソース", "トマトソース", 1285)]),
                      candidate([("リスト", "リスト", 1285), ("を", "ヲ", 156)]),
                      candidate([("利す", "リス", 584), ("と", "ト", 147)])] {
            XCTAssertNil(nounCoordinationAlternative(input) { _, _ in XCTFail("Not a noun coordination boundary"); return nil })
        }
    }

    func testDictionaryProbesAreBoundedForLongInput() {
        let input = candidate(Array(repeating: ("リスト", "リスト", 1285), count: 10))
        var calls = 0
        XCTAssertNil(nounCoordinationAlternative(input) { _, _ in calls += 1; return nil })
        XCTAssertEqual(calls, 2)
        let long = candidate(Array(repeating: ("リスト", "リスト", 1285), count: 50))
        XCTAssertNil(nounCoordinationAlternative(long) { _, _ in XCTFail("Long input must not incur extra lattice searches"); return nil })
    }
}
