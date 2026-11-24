import Foundation
import XCTest
@testable import azookey_server

/// 2026-11-03: 学習データを使ってはいけない接続 (personalDataAllowed が偽: サンドボックスのアプリなど) では、
/// 難読語の学習の記録と元の語に戻した組を読まない (空) ・書かない (空を読んで書き戻すと本物の記録が消えるので)
final class PersonalDataTests: XCTestCase {
    func testHomophoneRecordsAreNeitherUsedNorWrittenWithoutPermission() async {
        await MainActor.run {
            let saved = (personalDataAllowed, homophoneLearnedCache, homophoneDeclinedCache)
            defer {
                personalDataAllowed = saved.0
                homophoneLearnedCache = saved.1
                homophoneDeclinedCache = saved.2
            }
            homophoneLearnedCache = ["さてつ": "蹉跌"]
            homophoneDeclinedCache = ["さてつ\t砂鉄"]
            personalDataAllowed = false
            XCTAssertTrue(homophoneLearned().isEmpty)
            XCTAssertTrue(homophoneDeclined().isEmpty)
            // 書いても記録は変わらない
            saveHomophoneLearned(["べつ": "別"])
            saveHomophoneDeclined([])
            personalDataAllowed = true
            XCTAssertEqual(homophoneLearned(), ["さてつ": "蹉跌"])
            XCTAssertEqual(homophoneDeclined(), ["さてつ\t砂鉄"])
        }
    }
}
