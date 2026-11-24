import Foundation

/// 2026-10-22: 代わりの字の成句。本来の字 (表外字) を、意味の違う常用漢字で代わりに書く成句 (論を俟たない/論を待たない・身を挺する/身を呈する・癇に障る/勘に障る)。
/// 表は variant_spellings.tsv (読み (ひらがな)<TAB>代わりの字の書き方<TAB>本来の書き方<TAB>かな書き。tools/extra_dict/make_variant_spellings.py が作る)。
/// 変換の結果に代わりの字の書き方が句の読みのまま出たら本来の書き方にし、候補の一覧では本来の書き方とかな書きを代わりの字の書き方より前に置く
struct VariantSpelling: Equatable {
    let original: String
    let kana: String
}

/// 表の引き方 (読みはひらがなにそろえる。チャンスをうかがう も ちゃんすをうかがう で引く)
func variantSpellingKey(reading: String, text: String) -> String {
    let hiragana = String(String.UnicodeScalarView(reading.unicodeScalars.map { scalar in
        (0x30A1...0x30F6).contains(scalar.value) ? Unicode.Scalar(scalar.value - 0x60)! : scalar
    }))
    return hiragana + "\t" + text
}

/// 2026-10-22 (D の 2 回目の調べ M-1・M-3): 表は上限つきで読む (BoundedTable)。配る表は 80 行ほど・1 欄 30 字ほど
let variantSpellingLimits = (maxBytes: 1 << 20, maxLines: 10_000, columns: 3...4, maxFieldLength: 100)

func parseVariantSpellings(_ content: String) -> [String: VariantSpelling] {
    variantSpellings(rows: BoundedTable.rows(text: content, maxLines: variantSpellingLimits.maxLines,
                                             columns: variantSpellingLimits.columns, maxFieldLength: variantSpellingLimits.maxFieldLength))
}

func variantSpellings(rows: [[String]]) -> [String: VariantSpelling] {
    var table: [String: VariantSpelling] = [:]
    for parts in rows where !parts[0].isEmpty && !parts[1].isEmpty && !parts[2].isEmpty {
        table[variantSpellingKey(reading: parts[0], text: parts[1])] = VariantSpelling(original: parts[2], kana: parts.count >= 4 ? parts[3] : "")
    }
    return table
}

@MainActor var variantSpellingTable: [String: VariantSpelling]? = nil
@MainActor func variantSpellings() -> [String: VariantSpelling] {
    if let table = variantSpellingTable { return table }
    let limits = variantSpellingLimits
    let table = variantSpellings(rows: BoundedTable.rows(execURL.appendingPathComponent("variant_spellings.tsv"), maxBytes: limits.maxBytes,
                                                         maxLines: limits.maxLines, columns: limits.columns, maxFieldLength: limits.maxFieldLength))
    variantSpellingTable = table
    return table
}

/// 変換の結果のうち、つながった文節の字と読みが表の代わりの字の書き方と読みに合うところを、本来の書き方の 1 文節にする
/// (勘|に|障る → 癇に障る)。最後の文節が後ろにかなを 4 字まで続けていても合わせる (機会を|伺って → 機会を窺って。表には 機会を伺っ がある)。
/// 文節の読みの長さの合計が読みと合わなければさわらない
func replacingVariantSpellings(_ segments: [(word: String, rubyCount: Int)], reading: String,
                               table: [String: VariantSpelling]) -> [(word: String, rubyCount: Int)] {
    let chars = Array(reading)
    guard !table.isEmpty, segments.reduce(0, { $0 + $1.rubyCount }) == chars.count else { return segments }
    var result: [(word: String, rubyCount: Int)] = []
    var i = 0, start = 0
    while i < segments.count {
        var text = "", count = 0, matched = false
        for j in i..<min(segments.count, i + 8) {
            text += segments[j].word
            count += segments[j].rubyCount
            let span = String(chars[start..<(start + count)])
            // 後ろのかな (読みと字が同じ) を 0〜4 字はずして引く (表記が空の文節でも範囲を作れるように stride。D の 2 回目の調べ R2-7)
            for tail in stride(from: 0, through: min(4, text.count - 1), by: 1) {
                let suffix = String(text.suffix(tail))
                guard suffix.allSatisfy({ ("ぁ"..."ゖ").contains($0) }), span.hasSuffix(suffix),
                      let variant = table[variantSpellingKey(reading: String(span.dropLast(tail)), text: String(text.dropLast(tail)))] else { continue }
                result.append((word: variant.original + suffix, rubyCount: count))
                i = j + 1; start += count; matched = true
                break
            }
            if matched { break }
        }
        if !matched {
            result.append(segments[i]); start += segments[i].rubyCount; i += 1
        }
    }
    return result
}

/// 候補の一覧で、代わりの字の書き方より前に本来の書き方とかな書きを置く (一覧に無ければ入れる)。表に当たる候補が無ければそのまま
func orderingVariantSpellings(_ texts: [String], reading: String, table: [String: VariantSpelling]) -> [String] {
    guard !table.isEmpty, let substitute = texts.first(where: { table[variantSpellingKey(reading: reading, text: $0)] != nil }),
          let variant = table[variantSpellingKey(reading: reading, text: substitute)] else { return texts }
    let front = [variant.original] + (variant.kana.isEmpty ? [] : [variant.kana])
    var result = texts.filter { !front.contains($0) }
    result.insert(contentsOf: front, at: result.firstIndex(of: substitute) ?? 0)
    return result
}
