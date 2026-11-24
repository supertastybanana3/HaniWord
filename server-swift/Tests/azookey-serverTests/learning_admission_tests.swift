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

/// 2026-11-24: パスワードらしい形の文節を含む確定からは何も覚えない (looksLikeSecret)
final class SecretLikeCommitTests: XCTestCase {
    func testPasswordLikeTextsAreNotLearned() {
        // A: 英字と数字がどちらもあり、合わせて 6 字以上
        for text in ["Pass1234", "abc12345", "ｐａｓｓ１２３４", "X68000"] { XCTAssertTrue(looksLikeSecret(text), text) }
        // B: パスワードによく使う記号と、英字・数字 3 字以上
        for text in ["p@ss", "桜2024！", "abc#", "Tokyo#88", "qq＊１２"] { XCTAssertTrue(looksLikeSecret(text), text) }
        // C: かなのすぐ後ろに、かなに読めなかった小文字が 1〜2 字残る
        for text in ["ぱっsうぉrd", "あいうk", "かきxyく"] { XCTAssertTrue(looksLikeSecret(text), text) }
        // 端末のパスワードの問いで、英字の判定が test12 を英字に残し、そのあとの a が あ になった形 (文節が分かれても、どちらも当たる)
        for text in ["test12あb", "test12", "あb"] { XCTAssertTrue(looksLikeSecret(text), text) }
        // 数式に見えない形は今までどおり (記号が * だけ・英字が 3 字以上続く・式のしるしが無い)
        for text in ["a1b2c3d4", "xy*12345", "pass+1234", "Abc^123"] { XCTAssertTrue(looksLikeSecret(text), text) }
    }

    /// 数式の形 (式のしるしか上付き・下付きの字があり、英字は 2 字までの変数だけ。後ろの助詞は除く) は覚える
    func testFormulasAreLearned() {
        for text in ["x²+1", "x²＋y²＝1", "3x²+2x+1=0", "2x+3=7を", "f(x)=2x+1", "a²+b²=c²", "y=ax+b", "2*3=6", "3x²−2x+1=0", "(a+b)²", "H₂O"] {
            XCTAssertFalse(looksLikeSecret(text), text)
        }
    }

    func testOrdinaryTextsAreLearned() {
        for text in ["今日は", "GitHubに", "iPhone", "PDFで", "N700系", "2024年", "100％", "50%", "C++", "Q&A", "3D",
                     "5mmの", "aは", "Tシャツ", "LINEで", "x²+1", "第3章", "COVIDの", "100%の", "100％の力"] {
            XCTAssertFalse(looksLikeSecret(text), text)
        }
    }
}
