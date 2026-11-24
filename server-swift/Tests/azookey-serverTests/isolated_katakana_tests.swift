import Foundation
import XCTest
@testable import azookey_server

final class IsolatedKatakanaTests: XCTestCase {
    func testLongVowelMarkInHiraganaWordsIsNotIsolatedKatakana() {
        // ひらがなの語の長音 (あーね・すごーい) は、離れたカタカナでない (一覧から あーね理解 が消えていた)
        XCTAssertFalse(hasIsolatedKatakana("あーね理解"))
        XCTAssertFalse(hasIsolatedKatakana("すごーい"))
        // 仮名 1 文字だけがカタカナになった候補は今までどおり外す
        XCTAssertTrue(hasIsolatedKatakana("書き続けていル"))
        XCTAssertTrue(hasIsolatedKatakana("嗅がクの"))
        XCTAssertFalse(hasIsolatedKatakana("コーヒーを飲む"))
        // ー は直前の字と同じ字種: ひらがなの後ろの ー のさらに後ろの 1 字だけのカタカナ (あーネ・すごーイ) は外す
        XCTAssertTrue(hasIsolatedKatakana("あーネ理解"))
        XCTAssertTrue(hasIsolatedKatakana("すごーイ"))
        XCTAssertFalse(hasIsolatedKatakana("スーぱー"))
    }
}
