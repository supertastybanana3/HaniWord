import Foundation
import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-06 (仕事 10c): 数学の名詞のすぐ後ろの母音 1 字を変数の英字にする (math_variables.swift)
final class MathVariablesTests: XCTestCase {
    func e(_ word: String, _ ruby: String, _ cid: Int) -> DicdataElement {
        DicdataElement(word: word, ruby: ruby, lcid: cid, rcid: cid, mid: 501, value: -10)
    }
    func words(_ data: [DicdataElement]?) -> [String]? { data?.map(\.word) }

    func testVowelAfterMathNounBecomesALetter() {
        // ベクトルあ (終わり)・ベクトル亜の・点お、
        XCTAssertEqual(words(latinizingVariableVowels([e("ベクトル", "ベクトル", 1285), e("あ", "ア", 1285)])), ["ベクトル", "a"])
        XCTAssertEqual(words(latinizingVariableVowels([e("ベクトル", "ベクトル", 1285), e("亜", "ア", 1298), e("の", "ノ", 368), e("大きさ", "オオキサ", 1285)])),
                       ["ベクトル", "a", "の", "大きさ"])
        // 点・集合・行列・数学 のあとは、数えた英字の 8 割以上が大文字なので大文字 (点O・数学A)
        XCTAssertEqual(words(latinizingVariableVowels([e("点", "テン", 1285), e("お", "オ", 1285), e("、", "、", 1317)])), ["点", "O", "、"])
        XCTAssertEqual(words(latinizingVariableVowels([e("数学", "スウガク", 1285), e("あ", "ア", 1285)])), ["数学", "A"])
    }

    func testOtherFormsAreLeftAlone() {
        // 後ろが助詞・記号でない (ベクトル＋あ＋る)、表に無い名詞 (本あ)、母音 1 字でない語 (点＋いる)、別の語になった読み (繊維の)
        XCTAssertNil(latinizingVariableVowels([e("ベクトル", "ベクトル", 1285), e("あ", "ア", 1285), e("る", "ル", 700)]))
        XCTAssertNil(latinizingVariableVowels([e("本", "ホン", 1285), e("あ", "ア", 1285)]))
        XCTAssertNil(latinizingVariableVowels([e("点", "テン", 1285), e("いる", "イル", 700)]))
        XCTAssertNil(latinizingVariableVowels([e("繊維", "センイ", 1285), e("の", "ノ", 368)]))
        // 頭の母音 (前に名詞が無い)
        XCTAssertNil(latinizingVariableVowels([e("あ", "ア", 1285), e("の", "ノ", 368)]))
    }
}
