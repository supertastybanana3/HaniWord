import Foundation

/// 2026-11-06 (仕事 34 の 4、iOS と同じ入力の補助): 1+2= のように式のあとに = を打ったら、候補に答えを出す (設定 input.calc_candidates、初期値オフ)。
/// 式だけの読みは 1 つの文節にまとめ (ConvertSegments)、その一覧の 2 番目に「式＝答え」、3 番目に「答え」を出す (ListCandidates)。
/// かなの入力中は記号が全角・長音になる (+ → ＋・- → ー・/ → ・ など) ので、数字にはさまれた形を半角の演算子として読む
@MainActor func calcCandidatesEnabled() -> Bool {
    (config["calcCandidates"] as? Bool) ?? false
}

/// 読みが「数・演算子・括弧だけの式 ＋ 終わりの =」なら、その答え (整数ならそのまま、ほかは 10 けたまで)。演算子が 1 つも無い・式が読めない・0 で割るなら nil
func calcResult(_ reading: String) -> String? {
    guard reading.count <= 60, let last = reading.last, last == "=" || last == "＝" else { return nil }
    let map: [Character: Character] = ["０": "0", "１": "1", "２": "2", "３": "3", "４": "4", "５": "5", "６": "6", "７": "7", "８": "8", "９": "9",
                                       "＋": "+", "ー": "-", "－": "-", "−": "-", "＊": "*", "×": "*", "／": "/", "÷": "/", "・": "/",
                                       "（": "(", "）": ")", "＾": "^", "．": "."]
    let body = reading.dropLast().compactMap { c -> Character? in
        if c == " " || c == "　" { return nil }
        return map[c] ?? c
    }
    guard !body.isEmpty, body.allSatisfy({ "0123456789.+-*/^()".contains($0) }), body.contains(where: { "+-*/^".contains($0) }),
          body.contains(where: { $0.isNumber }) else { return nil }
    var parser = CalcParser(chars: body)
    guard let value = parser.expression(), parser.at == body.count, value.isFinite else { return nil }
    if abs(value - value.rounded()) < 1e-9 && abs(value) < 1e15 {
        return String(Int64(value.rounded()))
    }
    var text = String(format: "%.10g", value)
    if text.contains("e") { return nil }
    if text.contains(".") { while text.hasSuffix("0") { text.removeLast() } }
    return text
}

/// 足し算・引き算 < 掛け算・割り算 < べき (右から) < 頭の - < 数・括弧
struct CalcParser {
    let chars: [Character]
    var at = 0

    mutating func expression() -> Double? {
        guard var value = term() else { return nil }
        while at < chars.count, chars[at] == "+" || chars[at] == "-" {
            let op = chars[at]; at += 1
            guard let rhs = term() else { return nil }
            value = op == "+" ? value + rhs : value - rhs
        }
        return value
    }

    mutating func term() -> Double? {
        guard var value = power() else { return nil }
        while at < chars.count, chars[at] == "*" || chars[at] == "/" {
            let op = chars[at]; at += 1
            guard let rhs = power() else { return nil }
            if op == "/" && rhs == 0 { return nil }
            value = op == "*" ? value * rhs : value / rhs
        }
        return value
    }

    mutating func power() -> Double? {
        guard let base = unary() else { return nil }
        if at < chars.count, chars[at] == "^" {
            at += 1
            guard let exponent = power(), abs(exponent) <= 64 else { return nil }
            return pow(base, exponent)
        }
        return base
    }

    mutating func unary() -> Double? {
        if at < chars.count, chars[at] == "-" {
            at += 1
            return unary().map { -$0 }
        }
        return primary()
    }

    mutating func primary() -> Double? {
        guard at < chars.count else { return nil }
        if chars[at] == "(" {
            at += 1
            guard let value = expression(), at < chars.count, chars[at] == ")" else { return nil }
            at += 1
            return value
        }
        let start = at
        while at < chars.count, chars[at].isNumber || chars[at] == "." { at += 1 }
        guard at > start else { return nil }
        return Double(String(chars[start..<at]))
    }
}

/// 式だけの読みの文節を 1 つにまとめる (一覧を読み全体で出すため)
func mergingCalcSegments(_ segments: [(word: String, rubyCount: Int)], reading: String, enabled: Bool) -> [(word: String, rubyCount: Int)] {
    guard enabled, segments.count > 1, calcResult(reading) != nil else { return segments }
    return [(word: segments.map(\.word).joined(), rubyCount: segments.reduce(0) { $0 + $1.rubyCount })]
}

/// 式だけの読みの一覧: 2 番目に「式＝答え」、3 番目に「答え」
func placingCalcResult(_ texts: [String], reading: String, enabled: Bool) -> [String] {
    guard enabled, let plain = calcResult(reading) else { return texts }
    let shown = texts.first ?? reading
    // 見えている式の数字が全角 (１＋２＝) なら、答えも全角にする
    let wide = shown.unicodeScalars.contains { (0xFF10...0xFF19).contains($0.value) }
    let result = wide ? String(String.UnicodeScalarView(plain.unicodeScalars.map { scalar in
        switch scalar {
        case "0"..."9": return Unicode.Scalar(scalar.value + 0xFEE0)!
        case ".": return "．"
        case "-": return "－"
        default: return scalar
        }
    })) : plain
    var rest = texts.filter { $0 != shown + result && $0 != result }
    rest.insert(contentsOf: [shown + result, result], at: min(1, rest.count))
    return rest
}
