import XCTest
import KanaKanjiConverterModule
@testable import azookey_server

/// 2026-11-08 (仕事 49): 辞書だけの変換で、左の文脈の終わりの語の品詞からつなぐ (left_pos.swift) の決まり
final class LeftPosTests: XCTestCase {
    func testNonStarterClassesFollowLeftIdDef() {
        // left-id.def の番号: 307 助詞 て・369 助動詞・870 動詞,接尾 れる・919 動詞,非自立 いる・1297 名詞,接尾・1310 名詞,非自立
        for cid in [53, 146, 147, 307, 368, 369, 554, 868, 870, 919, 1280, 1297, 1305, 1309, 1313] {
            XCTAssertTrue(isNonStarterCID(cid), "\(cid)")
        }
        // 文の頭に来る語: 記号・形容詞,自立・接続詞・動詞,自立 (言う 818・謂う 817・一段 625)・副詞・名詞,一般 1285・固有名詞 1288・代名詞 1306
        for cid in [0, 8, 11, 52, 555, 561, 625, 817, 818, 867, 1281, 1283, 1285, 1288, 1296, 1306, 1314, 1315] {
            XCTAssertFalse(isNonStarterCID(cid), "\(cid)")
        }
    }

    func testMajorPartOfSpeech() {
        XCTAssertEqual(majorPOS(817), majorPOS(818))  // 謂う と 言う はどちらも動詞
        XCTAssertEqual(majorPOS(561), majorPOS(1280))
        XCTAssertNotEqual(majorPOS(818), majorPOS(1285))  // 動詞 と 名詞
        XCTAssertNotEqual(majorPOS(307), majorPOS(369))  // 助詞 と 助動詞
        XCTAssertEqual(majorPOS(1283), majorPOS(1314))
        XCTAssertNotEqual(majorPOS(1315), majorPOS(1314))  // 連体詞 は名詞と別
    }

    /// 止める口 (試験用のビルドの AZK_LEFT_POS): 0 で使わない、tail で見積もりだけ、それ以外・無しは both
    func testOffSwitch() async {
        XCTAssertEqual(leftWordModeOf(["AZK_LEFT_POS": "0"]), "0")
        XCTAssertEqual(leftWordModeOf(["AZK_LEFT_POS": "tail"]), "tail")
        XCTAssertEqual(leftWordModeOf([:]), "both")
        XCTAssertEqual(leftWordModeOf(["AZK_LEFT_POS": "1"]), "both")
        await MainActor.run {
            // 0 なら、左の文脈があっても始点は文頭のまま (変換器を引かない)
            XCTAssertNil(leftStartRcid(context: "手紙を書い", mode: "0"))
            XCTAssertNil(leftStartRcid(context: "京都市は、", mode: "0"))
        }
    }

    func testTailKey() {
        XCTAssertNil(leftTailKey(""))
        XCTAssertNil(leftTailKey("今日は晴れ。"))  // 文の終わりのあとは文頭のまま
        XCTAssertNil(leftTailKey("報告\n"))
        XCTAssertNil(leftTailKey("京都府南部"))  // 漢字で終わる左は見積もらない
        XCTAssertEqual(leftTailKey("京都市は、"), "、")
        XCTAssertEqual(leftTailKey("手紙を書い"), "い")
        XCTAssertEqual(leftTailKey("勉強している"), "している")
        XCTAssertEqual(leftTailKey("あいうえおかきくけこ"), "おかきくけこ")  // 6 字まで
    }

    /// 個人のデータを許さない接続では、確定した文節も左の語の見積もりも残さない・使わない
    func testNothingKeptWithoutPersonalData() async {
        await MainActor.run {
            let saved = (personalDataAllowed, lastCommittedLeft, leftTailRcidMemo)
            defer {
                personalDataAllowed = saved.0
                lastCommittedLeft = saved.1
                leftTailRcidMemo = saved.2
            }
            personalDataAllowed = false
            lastCommittedLeft = nil
            leftTailRcidMemo = [:]
            noteCommittedLeft(reading: "かい", text: "書い")
            XCTAssertNil(lastCommittedLeft)
            rememberLeftTail("して", 300)
            XCTAssertTrue(leftTailRcidMemo.isEmpty)
            // 前の接続で覚えたものが残っていても使わない
            lastCommittedLeft = (text: "書い", rcid: 625)
            XCTAssertNil(committedLeftRcid(context: "手紙を書い"))
            // 許す接続なら使う (表記が左の文脈の終わりに合うときだけ)
            personalDataAllowed = true
            XCTAssertEqual(committedLeftRcid(context: "手紙を書い"), 625)
            XCTAssertNil(committedLeftRcid(context: "手紙を読み"))
            rememberLeftTail("して", 300)
            XCTAssertEqual(leftTailRcidMemo["して"], .some(300))
            // 接続を切り替えたとき (SetPersonalDataAllowed) は全部消える
            forgetLeftWord()
            XCTAssertNil(lastCommittedLeft)
            XCTAssertTrue(leftTailRcidMemo.isEmpty)
        }
    }

    func testGate() {
        let none: () -> (lcid: Int, ruby: String, value: PValue)? = { XCTFail("文頭からの結果は要らない"); return nil }
        // 文の頭に来ない語 (助詞 て) で始まるなら、左の語からの結果を使う (文頭から変換し直さない)
        XCTAssertTrue(leftWordGate(leftFirst: (307, "テ", -3), starterConnection: -9.5, bosFirst: none))
        // 文の頭に来やすい語 (名詞。文頭からの点 -1.4) で始まるなら、文頭からの結果
        XCTAssertFalse(leftWordGate(leftFirst: (1285, "イカ", -8), starterConnection: -1.4, bosFirst: none))
        // 同じ読み・同じ大きな品詞 (動詞) なら、語そのものの点の良いほう: 謂う (817) より 言う (818) が良ければ文頭からの結果 (と言う)
        XCTAssertFalse(leftWordGate(leftFirst: (817, "イウ", -9), starterConnection: -7.1, bosFirst: { (818, "イウ", -3) }))
        // 行う (左の語から) が おこなう (文頭から) より良ければ、左の語からの結果 (を行う)
        XCTAssertTrue(leftWordGate(leftFirst: (818, "オコナウ", -4), starterConnection: -9.3, bosFirst: { (817, "オコナウ", -7) }))
        // 点が同じなら左の語からの結果
        XCTAssertTrue(leftWordGate(leftFirst: (818, "オコナウ", -5), starterConnection: -9.3, bosFirst: { (817, "オコナウ", -5) }))
        // 動詞 し (する) で、文頭からの結果が名詞 下 なら、左の語からの結果 (品詞の選び分け: 完成|した)
        XCTAssertTrue(leftWordGate(leftFirst: (650, "シ", -2), starterConnection: -9.0, bosFirst: { (1285, "シタ", -1) }))
        // 文頭からの結果の最初の語の読みが違えば、左の語からの結果
        XCTAssertTrue(leftWordGate(leftFirst: (625, "カエル", -6), starterConnection: -9.0, bosFirst: { (619, "カ", -1) }))
    }
}
