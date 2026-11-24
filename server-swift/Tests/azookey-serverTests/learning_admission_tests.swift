import XCTest
@testable import azookey_server

final class LearningAdmissionTests: XCTestCase {
    func testSymbolsNeverLearnAnAutomaticSubstitution() {
        var admission = ConservativeLearningAdmission()
        for _ in 0..<9 {
            XCTAssertFalse(admission.admit(reading: "←", text: "→"))
            XCTAssertFalse(admission.admit(reading: "？", text: "❓"))
            XCTAssertFalse(admission.admit(reading: "±", text: "＋"))
        }
        XCTAssertTrue(admission.admit(reading: "やじるし", text: "←"))
    }

    // 2026-10-20: かなの読みから選んだ記号だけの語 (かっこ → ()) は、数字と同じく 3 回で 1 回だけ覚える
    func testSymbolWordsFromKanaNeedThreeSelections() {
        var admission = ConservativeLearningAdmission()
        for (reading, text) in [("かっこ", "()"), ("やじるし", "←"), ("かっこを", "()を")] {
            for count in 1...6 {
                XCTAssertEqual(admission.admit(reading: reading, text: text, symbolWord: true), count % 3 == 0, "\(reading) \(count)")
            }
        }
        XCTAssertTrue(admission.admit(reading: "かっこ", text: "括弧", symbolWord: false))
    }

    func testNumbersNeedThreeConfirmationsForEachLearningUpdate() {
        for (reading, text) in [("3", "三"), ("3こ", "三個"), ("いち", "一"), ("2026ねん", "２０２６年")] {
            var admission = ConservativeLearningAdmission()
            for count in 1...9 {
                XCTAssertEqual(admission.admit(reading: reading, text: text), count % 3 == 0)
            }
            XCTAssertFalse(admission.admit(reading: "3", text: "３"))
        }
    }

    func testIdentityAndUnrelatedWords() {
        var admission = ConservativeLearningAdmission()
        for _ in 0..<6 { XCTAssertFalse(admission.admit(reading: "2026", text: "2026")) }
        for (reading, text) in [("りさん", "理三"), ("りさん", "理Ⅲ"), ("GPT4", "GPT-4"),
                                ("いち", "位置"), ("たかい", "髙い"), ("みそ", "味噌")] {
            XCTAssertTrue(admission.admit(reading: reading, text: text))
        }
    }
}
