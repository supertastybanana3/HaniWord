import XCTest
@testable import azookey_server

/// 2026-11-08 (仕事 54): 辞書だけ向けの直し (外来語の崩れのカタカナ・数字の後ろの助数詞・colloc の 1 字の語と外側の文脈) をかけるのは、
/// 辞書だけの変換 (ライブ変換の仮表示) と、AI を切った設定の Space・清書
final class DictionaryFixesTests: XCTestCase {
    func testDictionaryFixesFollowTheAISetting() {
        XCTAssertTrue(usesDictionaryFixes(dictionaryOnly: true, aiEnabled: true))
        XCTAssertTrue(usesDictionaryFixes(dictionaryOnly: true, aiEnabled: false))
        // AI を切った設定の Space (dictionaryOnly=false で届く) にもかける
        XCTAssertTrue(usesDictionaryFixes(dictionaryOnly: false, aiEnabled: false))
        // AI ありの Space・清書にはかけない (今までどおり)
        XCTAssertFalse(usesDictionaryFixes(dictionaryOnly: false, aiEnabled: true))
    }
}
