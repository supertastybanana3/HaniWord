import Foundation

/// 2026-11-06 (まとめ役 09:57、利用者の頼み「候補に出すのと、設定でも既定オフでオンにできるように」): 「。。。」「・・・」「...」を「…」に。
/// 同じ点が 3 つ並んだ文節 (preservingLiteralSymbols が 1 つにまとめる) の候補の一覧の 2 番目に …、6 つなら ……。
/// 設定 input.ellipsis_auto (初期値オフ) をオンにすると、変換で自動で置き換える (一覧の 2 番目に打ったとおりの形)
let ellipsisDots: Set<Character> = ["。", "・", ".", "．"]

@MainActor func ellipsisAutoEnabled() -> Bool {
    (config["ellipsisAuto"] as? Bool) ?? false
}

/// 同じ点がちょうど 3 つ・6 つ並んだ読みなら、それに当たる … の形 (… か ……)。ほかは nil
func ellipsisForm(_ reading: String) -> String? {
    guard let first = reading.first, ellipsisDots.contains(first), reading.allSatisfy({ $0 == first }) else { return nil }
    switch reading.count {
    case 3: return "…"
    case 6: return "……"
    default: return nil
    }
}

/// 変換の文節のうち、点が 3 つ・6 つ並んだ文節を … に (設定がオンのとき)
func replacingEllipsis(_ segments: [(word: String, rubyCount: Int)], enabled: Bool) -> [(word: String, rubyCount: Int)] {
    guard enabled else { return segments }
    return segments.map { segment in
        guard let form = ellipsisForm(segment.word), segment.rubyCount == segment.word.count else { return segment }
        return (word: form, rubyCount: segment.rubyCount)
    }
}

/// 候補の一覧: 点が 3 つ・6 つ並んだ読みなら、… の形を 2 番目に (設定がオンなら 1 番目に … 、2 番目に打ったとおり)
func placingEllipsis(_ texts: [String], reading: String, auto: Bool) -> [String] {
    guard let form = ellipsisForm(reading) else { return texts }
    var rest = texts.filter { $0 != form && $0 != reading }
    let head = auto ? [form, reading] : [reading, form]
    rest.insert(contentsOf: head, at: 0)
    return rest
}
