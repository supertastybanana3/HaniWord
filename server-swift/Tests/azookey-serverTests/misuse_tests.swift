import XCTest
@testable import azookey_server

final class MisuseTests: XCTestCase {
    func testMisreadingsAreAppendedOnlyWhenMissing() {
        let table = parseMisreadingCandidates("""
        # 読み\t本来の形
        てんかのほうとう\t伝家の宝刀
        せろん\t世論
        """)
        // 打った読みと違う語は先に出さない: 一覧の後ろに足す
        XCTAssertEqual(appendingMisreadings(["天下の宝刀", "添加の放蕩"], reading: "てんかのほうとう", table: table), ["天下の宝刀", "添加の放蕩", "伝家の宝刀"])
        // もう一覧にあれば何もしない
        XCTAssertEqual(appendingMisreadings(["セロン", "世論"], reading: "せろん", table: table), ["セロン", "世論"])
        XCTAssertEqual(appendingMisreadings(["天下"], reading: "てんか", table: table), ["天下"])
    }

    func testTablesDropControlCharactersAndShiftedColumns() {
        // 制御文字 (候補窓の区切り U+001F) のある行と、欄の数が違う行 (空の欄も 1 欄と数える) は捨てる (D の 2 回目の調べ M-1・M-3)
        let table = parseMisreadingCandidates("あ\t正しい\nい\t見える\u{1F}選ばれる\nう\t\tずれ\n")
        XCTAssertEqual(table.back, ["あ": ["正しい"]])
        XCTAssertEqual(table.corrections, [:])
        let notes = parseMisuseNotes("誤用\t含む\t的\t一字の鍵は使わない\n誤用\t含む\t的を得\t本来は 的を射る\n")
        XCTAssertEqual(notes.contains.map(\.key), ["的を得"])
    }

    func testMisreadingCorrectionsGoFourthWithANote() {
        // 2026-11-20 (Mozc の reading_correction.tsv): 3 列目 (正しい読み) のある行は、一覧に無ければ 4 番目に足し、注記「もしかして: 正しい読み」
        let table = parseMisreadingCandidates("""
        # 読み\t本来の形\t正しい読み
        ごちゅう\t御中\tおんちゅう
        せろん\t世論
        いちよう\t一応\tいちおう
        かいし\t壊死\tえし\t後
        """)
        XCTAssertEqual(table.back, ["せろん": ["世論"]])
        XCTAssertEqual(table.corrections["ごちゅう"], [.init(form: "御中", correct: "おんちゅう")])
        XCTAssertEqual(insertingMisreadingCorrections(["語注", "五柱", "ごちゅう", "ゴチュウ"], reading: "ごちゅう", table: table),
                       ["語注", "五柱", "ごちゅう", "御中", "ゴチュウ"])
        // 一覧が短ければ後ろ。1 番目には出さない
        XCTAssertEqual(insertingMisreadingCorrections(["ごちゅう", "ゴチュウ"], reading: "ごちゅう", table: table), ["ごちゅう", "ゴチュウ", "御中"])
        XCTAssertEqual(insertingMisreadingCorrections(["ごちゅう"], reading: "ごちゅう", table: table), ["ごちゅう", "御中"])
        // もう一覧にあれば動かさない (1 番目でも)
        XCTAssertEqual(insertingMisreadingCorrections(["一様", "一応", "一葉", "いちよう"], reading: "いちよう", table: table), ["一様", "一応", "一葉", "いちよう"])
        // 後ろの助詞は外して引き、形の後ろに付け直す
        XCTAssertEqual(insertingMisreadingCorrections(["ごちゅうへ", "ゴチュウヘ"], reading: "ごちゅうへ", table: table), ["ごちゅうへ", "ゴチュウヘ", "御中へ"])
        XCTAssertEqual(misreadingNote(for: "御中へ", reading: "ごちゅうへ", table: table), "読み誤り\tもしかして: おんちゅう")
        XCTAssertEqual(misreadingNote(for: "御中", reading: "ごちゅう", table: table), "読み誤り\tもしかして: おんちゅう")
        XCTAssertEqual(misreadingNote(for: "一応", reading: "いちよう", table: table), "読み誤り\tもしかして: いちおう")
        // ほかの候補・正しい読みで打ったとき・3 列目の無い行 (読みのゆれ) には注記を付けない。読みのゆれは今までどおり後ろに足す
        XCTAssertEqual(misreadingNote(for: "語注", reading: "ごちゅう", table: table), "")
        XCTAssertEqual(misreadingNote(for: "御中", reading: "おんちゅう", table: table), "")
        XCTAssertEqual(misreadingNote(for: "世論", reading: "せろん", table: table), "")
        XCTAssertEqual(insertingMisreadingCorrections(["セロン"], reading: "せろん", table: table), ["セロン"])
        XCTAssertEqual(appendingMisreadings(["セロン", "せろん"], reading: "せろん", table: table), ["セロン", "せろん", "世論"])
        // 4 列目が「後」の行 (誤った読みが強い語の読みでもある) は、4 番目でなく後ろに注記つきで足す
        XCTAssertEqual(table.corrections["かいし"], [.init(form: "壊死", correct: "えし", atEnd: true)])
        XCTAssertEqual(insertingMisreadingCorrections(["開始", "海士", "怪死", "懐紙", "かいし", "カイシ"], reading: "かいし", table: table),
                       ["開始", "海士", "怪死", "懐紙", "かいし", "カイシ", "壊死"])
        XCTAssertEqual(misreadingNote(for: "壊死", reading: "かいし", table: table), "読み誤り\tもしかして: えし")
        // 助詞だけの違いでない読み (ごちゅうい) は引かない
        XCTAssertEqual(insertingMisreadingCorrections(["ご注意"], reading: "ごちゅうい", table: table), ["ご注意"])
    }

    func testPublishedMisreadingTableIsReadWithinLimits() throws {
        // 2026-11-20: 配る misreading_candidates.tsv (今までの 27 行 + Mozc の reading_correction.tsv) を上限つきで読み、捨てられる行が無いこと
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let limits = misreadingLimits
        let url = root.appendingPathComponent("misreading_candidates.tsv")
        let rows = BoundedTable.rows(url, maxBytes: limits.maxBytes, maxLines: limits.maxLines, columns: limits.columns, maxFieldLength: limits.maxFieldLength)
        let lines = try String(contentsOf: url, encoding: .utf8).split(separator: "\n").filter { !$0.isEmpty && !$0.hasPrefix("#") }
        XCTAssertEqual(rows.count, lines.count)
        let table = misreadingCandidates(rows: rows)
        XCTAssertEqual(table.corrections["ごちゅう"]?.first, .init(form: "御中", correct: "おんちゅう"))
        XCTAssertEqual(table.corrections["てんかのほうとう"]?.first, .init(form: "伝家の宝刀", correct: "でんかのほうとう"))
        XCTAssertEqual(table.back["せろん"], ["世論"])
        XCTAssertNil(table.corrections["せろん"])
        // 強い語 (開始) と読みが重なる行は後ろに
        XCTAssertEqual(table.corrections["かいし"]?.first, .init(form: "壊死", correct: "えし", atEnd: true))
        XCTAssertEqual(table.corrections["ごちゅう"]?.first?.atEnd, false)
    }

    func testNotesByContainedFormAndByReading() {
        let notes = parseMisuseNotes("""
        誤用\t含む\t的を得\t本来は 的を射る
        誤用\t含む\t上や下への大騒\t本来は 上を下への大騒ぎ
        異字同訓\t読み\tあう:合う\t一致する
        """)
        XCTAssertEqual(misuseNote(for: "的を得た", reading: "まとをえた", notes: notes), "誤用\t本来は 的を射る")
        XCTAssertEqual(misuseNote(for: "上や下への大騒ぎ", reading: "うえやしたへのおおさわぎ", notes: notes), "誤用\t本来は 上を下への大騒ぎ")
        XCTAssertEqual(misuseNote(for: "合う", reading: "あう", notes: notes), "異字同訓\t一致する")
        // 読みが違えば異字同訓の注記は出さない (合わせる は 合う の使い分けの組ではない)。本来の形には注記を付けない
        XCTAssertEqual(misuseNote(for: "合う", reading: "あわせる", notes: notes), "")
        XCTAssertEqual(misuseNote(for: "的を射る", reading: "まとをいる", notes: notes), "")
    }

    func testContainedFormsSkipTheirExceptions() {
        // 2026-11-06 (仕事 40): 5 列目の除く語を含む候補には、その行の注記を付けない。鍵を含まない除く語 (と、鍵より短い語) は読まない。4 列の行は今までどおり
        let notes = parseMisuseNotes("""
        差別\t含む\tめくら\t注意: 差別の語とされます\tめくらまし|目くらまし|くら
        言い換え\t含む\tあいうえ\t注意\t
        誤用\t含む\t的を得\t本来は 的を射る
        """)
        XCTAssertEqual(notes.contains.first(where: { $0.key == "めくら" })?.except, ["めくらまし"])
        XCTAssertEqual(misuseNote(for: "めくらだ", reading: "めくらだ", notes: notes), "差別\t注意: 差別の語とされます")
        XCTAssertEqual(misuseNote(for: "めくらまし", reading: "めくらまし", notes: notes), "")
        XCTAssertEqual(misuseNote(for: "あいうえお", reading: "あいうえお", notes: notes), "言い換え\t注意")
        XCTAssertEqual(misuseNote(for: "的を得た", reading: "まとをえた", notes: notes), "誤用\t本来は 的を射る")
    }

    func testPublishedCautionNotesAreReadWithinLimits() throws {
        // 2026-11-07 (仕事 40): 配る misuse_notes.tsv を上限つきで読み、注意の注記の行 (差別・言い換え) が 1 つも捨てられないこと、除く語が効くこと
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let limits = misuseNoteLimits
        let rows = BoundedTable.rows(root.appendingPathComponent("misuse_notes.tsv"), maxBytes: limits.maxBytes, maxLines: limits.maxLines,
                                     columns: limits.columns, maxFieldLength: limits.maxFieldLength)
        let text = try String(contentsOf: root.appendingPathComponent("extra_dictionaries").appendingPathComponent("caution_notes.txt"), encoding: .utf8)
        let published = text.split(whereSeparator: \.isNewline).filter { !$0.hasPrefix("#") && !$0.isEmpty }.count
        XCTAssertEqual(rows.filter { $0[0] == "差別" || $0[0] == "言い換え" }.count, published)
        let notes = misuseNotes(rows: rows)
        XCTAssertTrue(misuseNote(for: "気違いだ", reading: "きちがいだ", notes: notes).hasPrefix("差別\t"))
        XCTAssertEqual(misuseNote(for: "不具合", reading: "ふぐあい", notes: notes), "")
        XCTAssertTrue(misuseNote(for: "支那そば", reading: "しなそば", notes: notes).hasPrefix("言い換え\t"))
        XCTAssertEqual(misuseNote(for: "的を射る", reading: "まとをいる", notes: notes), "")
    }
}
