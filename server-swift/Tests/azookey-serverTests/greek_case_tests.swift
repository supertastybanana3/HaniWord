import XCTest
@testable import azookey_server

/// 2026-11-06 (仕事 32): ギリシャ文字の大文字・小文字の順 (設定 input.greek_case)・足した読み・単位の Ω (notation.swift)
@MainActor
final class GreekCaseTests: XCTestCase {
    func testDefaultPutsLowercaseFirstForEveryLetter() {
        defer { config["greekCase"] = [String: String]() }
        config["greekCase"] = [String: String]()
        // 数え直すと、小文字の割合が 5 割を切る字は無かった (いちばん低い ψ でも 54%)
        XCTAssertTrue(greekDefaultUpper.isEmpty)
        XCTAssertEqual(greekCandidateLetters("しぐま"), ["σ", "Σ"])
        XCTAssertEqual(greekCandidateLetters("でるた"), ["δ", "Δ"])
        XCTAssertEqual(greekCandidateLetters("おめが"), ["ω", "Ω"])
        XCTAssertNil(greekCandidateLetters("しぐまの"))
    }

    func testSettingPutsTheChosenCaseFirstOnlyForThatLetter() {
        defer { config["greekCase"] = [String: String]() }
        config["greekCase"] = ["σ": "upper", "δ": "upper", "ω": "lower"]
        XCTAssertEqual(greekCandidateLetters("しぐま"), ["Σ", "σ"])
        XCTAssertEqual(greekCandidateLetters("でるた"), ["Δ", "δ"])
        XCTAssertEqual(greekCandidateLetters("おめが"), ["ω", "Ω"])
        XCTAssertEqual(greekCandidateLetters("あるふぁ"), ["α", "Α"])
    }

    func testSettingIsReadWithUppercaseKeysAndUnknownEntriesAreDropped() {
        let read = greekCaseSetting(["σ": "upper", "Δ": "upper", "ω": "lower", "x": "upper", "α": "big", "β": 1])
        XCTAssertEqual(read, ["σ": "upper", "δ": "upper", "ω": "lower"])
        XCTAssertEqual(greekCaseSetting(nil), [:])
        XCTAssertEqual(greekCaseSetting("upper"), [:])
    }

    func testAddedReadings() {
        defer { config["greekCase"] = [String: String]() }
        config["greekCase"] = [String: String]()
        XCTAssertEqual(greekCandidateLetters("えーた"), ["η", "Η"])
        XCTAssertEqual(greekCandidateLetters("いーた"), ["η", "Η"])
        XCTAssertEqual(greekCandidateLetters("てーた"), ["θ", "Θ"])
        XCTAssertEqual(greekCandidateLetters("つぇーた"), ["ζ", "Ζ"])
        XCTAssertEqual(greekCandidateLetters("えぷしろん"), ["ε", "Ε"])
        XCTAssertEqual(greekCandidateLetters("ゆぷしろん"), ["υ", "Υ"])
        XCTAssertEqual(greekCandidateLetters("ぷし"), ["ψ", "Ψ"])
        XCTAssertEqual(greekCandidateLetters("ふぃー"), ["φ", "Φ"])
    }

    func testAutomaticReplacementFollowsTheChosenCase() {
        defer { config["greekCase"] = [String: String]() }
        config["greekCase"] = [String: String]()
        XCTAssertEqual(replacingGreek("シグマ", before: nil, after: nil, upper: greekAutoUpper()), "σ")
        config["greekCase"] = ["σ": "upper"]
        XCTAssertEqual(replacingGreek("シグマ", before: nil, after: nil, upper: greekAutoUpper()), "Σ")
        XCTAssertEqual(replacingGreek("オメガ", before: nil, after: nil, upper: greekAutoUpper()), "ω")
        // カタカナ語の一部は今までどおり変えない
        XCTAssertEqual(replacingGreek("シグマリオン", before: nil, after: nil, upper: greekAutoUpper()), "シグマリオン")
    }

    func testOhmIsOfferedAsAUnitAlsoAfterDigits() {
        XCTAssertEqual(ohmCandidate("おーむ"), "Ω")
        XCTAssertEqual(ohmCandidate("10おーむ"), "10Ω")
        XCTAssertEqual(ohmCandidate("１０おーむ"), "１０Ω")
        XCTAssertEqual(ohmCandidate("2.2おーむ"), "2.2Ω")
        XCTAssertNil(ohmCandidate("おーむしゃ"))
        XCTAssertNil(ohmCandidate("あおーむ"))
        XCTAssertNil(ohmCandidate(".おーむ"))
    }
}
