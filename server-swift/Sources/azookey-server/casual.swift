import Foundation

/// 2026-11-06: くだけた一言 (だる・きも・おけ・りょ・わろた・ちょ など。casual_words.tsv) を 1 語だけで打ったときは、表のよい形の 1 番目
/// (多くはひらがな。わろた・がち はカタカナ) を変換の結果に、2 番目を候補の 2 番目に出す。今は だる → ダル・つら → ツラ・りょ → リョ・うける → 受ける・
/// わんちゃん → ワンちゃん のように崩れていた (AI も辞書も、ひらがなの形を持っていても別の語を選ぶ)。文の中 (読みの一部) は今までどおり AI に任せる。
/// 読み全体が登録した語なら登録した語を先にする (user_dictionary.swift)。ひらがなとカタカナのどちらを先にするかは表の よい形 の順で、
/// 語ごとに決めてある (半々の語はひらがなが先で、学習で利用者の選びに寄る)。開発用の切り替え AZK_CASUAL=0 で使わない
let casualFix = devEnvironment["AZK_CASUAL"] != "0"

/// 表 (読み<TAB>よい形 (| 区切り)<TAB>出どころ) を読む。読みはひらがな
func parseCasualWords(_ text: String) -> [String: [String]] {
    var out: [String: [String]] = [:]
    for line in text.split(whereSeparator: \.isNewline) where !line.hasPrefix("#") {
        let cols = line.split(separator: "\t", omittingEmptySubsequences: false)
        guard cols.count >= 2, !cols[0].isEmpty, cols[0].count <= 16 else { continue }
        let forms = cols[1].split(separator: "|").map(String.init).filter { !$0.isEmpty && $0.count <= 16 }
        if !forms.isEmpty { out[String(cols[0])] = forms }
    }
    return out
}

@MainActor var casualWordsTable: [String: [String]]? = nil

/// 読み全体がくだけた一言なら、よい形 (先が 1 番目)。登録した語のある読みや、表に無い読みなら nil
@MainActor func casualWordForms(_ hiragana: String) -> [String]? {
    guard casualFix else { return nil }
    if casualWordsTable == nil {
        casualWordsTable = parseCasualWords(BoundedTable.text(execURL.appendingPathComponent("casual_words.tsv"), maxBytes: 1 << 20) ?? "")
    }
    guard let forms = casualWordsTable?[hiragana] else { return nil }
    if appliedUserWordsByRuby[hiraganaToKatakana(hiragana)] != nil { return nil }
    return forms
}
