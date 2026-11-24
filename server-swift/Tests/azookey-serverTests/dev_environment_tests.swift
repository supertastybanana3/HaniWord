import XCTest
@testable import KanaKanjiConverterModule

/// 2026-11-04: 変換器 (AzooKeyKanaKanjiConverter の fork) の試験用の環境変数 (AZK_LEARN_PRIORITY・AZK_FUSION_PROF など) は、
/// 試験用のビルド (-D AZK_DEV_KNOBS) でだけ読む。配る版では空 (security review #5 と同じ)
final class DevEnvironmentTests: XCTestCase {
    func testOnlyDevBuildsReadKnobs() {
        #if AZK_DEV_KNOBS
        XCTAssertTrue(azkDevEnvironment.keys.allSatisfy { $0.hasPrefix("AZK_") })
        #else
        XCTAssertTrue(azkDevEnvironment.isEmpty)
        #endif
    }
}
