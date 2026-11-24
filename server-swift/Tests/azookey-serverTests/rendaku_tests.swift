import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

final class RendakuTests: XCTestCase {
    func e(_ word: String, _ ruby: String, _ lcid: Int, _ rcid: Int? = nil) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: lcid, rcid: rcid ?? lcid, mid: 501, value: -10)
    }

    func testRendakuWordAfterParticleIsFound() {
        let table: Set<String> = ["グルマ\t車", "ザクラ\t桜"]
        // あと|は|車(ぐるま)|は: 車 の所
        XCTAssertEqual(rendakuAfterParticle([e("あと", "アト", 1285), e("は", "ハ", 261), e("車", "グルマ", 1285), e("は", "ハ", 261)], table: table), [2])
        // AI の結果では あとは が 1 語 (右が助詞) のことも、は に一般名詞の番号が付いていることもある
        XCTAssertEqual(rendakuAfterParticle([e("あとは", "アトハ", 1285, 261), e("車", "グルマ", 1285)], table: table), [1])
        XCTAssertEqual(rendakuAfterParticle([e("あと", "アト", 1285), e("は", "ハ", 1285), e("車", "グルマ", 1285)], table: table), [2])
        // 車(くるま) は表に無い読み。歯車 は替えない
        XCTAssertEqual(rendakuAfterParticle([e("あと", "アト", 1285), e("は", "ハ", 261), e("車", "クルマ", 1285)], table: table), [])
        XCTAssertEqual(rendakuAfterParticle([e("あと", "アト", 1285), e("歯車", "ハグルマ", 1285)], table: table), [])
        // 助詞でない語のあと (名詞のあと) や、文の頭は見ない
        XCTAssertEqual(rendakuAfterParticle([e("夜", "ヨ", 1285), e("桜", "ザクラ", 1285)], table: table), [])
        XCTAssertEqual(rendakuAfterParticle([e("桜", "ザクラ", 1285), e("は", "ハ", 261)], table: table), [])
    }
}
