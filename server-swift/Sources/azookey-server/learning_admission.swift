import Foundation

func isNumericNotation(_ text: String) -> Bool {
    let units = ["kg", "km", "cm", "mm", "ml", "mL", "mol", "年", "月", "日", "時", "分", "秒",
                 "個", "人", "本", "枚", "回", "歳", "円", "%", "％", "g", "m", "L"]
    var core = text
    if let unit = units.first(where: { text.hasSuffix($0) }) { core.removeLast(unit.count) }
    let numerals = Set("〇零一二三四五六七八九十百千万億兆壱弐参拾佰仟萬")
    let separators = Set(".,，．、/／:：+-＋−－%％ ")
    return !core.isEmpty && core.contains(where: { $0.isNumber || numerals.contains($0) })
        && core.allSatisfy { $0.isNumber || numerals.contains($0) || separators.contains($0) }
}

struct ConservativeLearningAdmission {
    private var repetitions: [String: Int] = [:]

    /// symbolWord: かなの読みから記号だけの語 (かっこ → ()) を選んだ。数字と同じく 3 回選んで 1 回だけ覚える
    /// (2026-10-20。1 回選んだだけで、その読みの変換がずっと記号になるのを避ける。覚えた記号は変換にも出る)
    mutating func admit(reading: String, text: String, symbolWord: Bool = false) -> Bool {
        if !reading.isEmpty && reading.allSatisfy(isLiteralSymbol) { return false }
        guard symbolWord || isNumericNotation(reading) || isNumericNotation(text) else { return true }
        // Exact numeric identity carries no spelling preference worth learning.
        guard reading != text else { return false }
        let key = reading + "\u{1F}" + text
        if repetitions[key] == nil && repetitions.count >= 256 { repetitions.removeAll(keepingCapacity: true) }
        let count = (repetitions[key] ?? 0) + 1
        if count < 3 {
            repetitions[key] = count
            return false
        }
        repetitions.removeValue(forKey: key)
        return true
    }
}

/// 2026-11-24: パスワードらしい形の文節。確定した文にこの形の文節が 1 つでもあれば、その確定からは何も覚えない (learn_segments)。
/// 欄の種類を IME に知らせないアプリ (Flutter で作られたアプリのパスワードの欄) で、パスワードがかなに化けて学習に入った。
/// IME 本体はその枠組みの窓を覚えない欄にするが、ほかに見分けられない欄があるときの網。次のどれか:
/// - A: 英字と数字がどちらもあり、合わせて 6 字以上 (Pass1234・abc12345)
/// - B: パスワードによく使う記号 (! # $ & * + = ? @ ^ _ | ~、全角も。% はふつうの文に多い (100%の力) ので見ない) があり、英字と数字が合わせて 3 字以上
/// - C: かなのすぐ後ろに、かなに読めなかった小文字の英字が 1〜2 字だけ残っている (ぱっsうぉrd の s・rd)
/// 数式の形 (x²+1・2x+3=7を・f(x)=2x+1。+ = ^ < > などの記号か上付き・下付きの字があり、英字は 2 字までの変数だけ) は A・B に当てない
/// 一般の文 3,675 文の出力では、当たったのは A の 2 文 (81Dojo・X68000) だけ (その確定は覚えない)
func looksLikeSecret(_ text: String) -> Bool {
    let symbols: Set<Character> = ["!", "#", "$", "&", "*", "+", "=", "?", "@", "^", "_", "|", "~",
                                   "！", "＃", "＄", "＆", "＊", "＋", "＝", "？", "＠", "＾", "＿", "｜", "～"]
    func folded(_ c: Character) -> Character {
        let s = String(c).precomposedStringWithCompatibilityMapping
        return s.count == 1 ? s.first! : c
    }
    func isLetter(_ c: Character) -> Bool { let f = folded(c); return f.isASCII && f.isLetter }
    func isDigit(_ c: Character) -> Bool { let f = folded(c); return f.isASCII && f.isNumber }
    func isKana(_ c: Character) -> Bool {
        guard let s = c.unicodeScalars.first, c.unicodeScalars.count == 1 else { return false }
        return (0x3041...0x30FF).contains(s.value) || c == "ー"
    }
    // 数式のしるし。- / は型番・日付に、* はパスワードに多い (qq＊１２) ので、しるしにはしない (式の中にあるのはよい)
    let formulaMarks: Set<Character> = ["+", "=", "^", "<", ">", "＋", "＝", "＾", "＜", "＞", "×", "÷", "≦", "≧", "≤", "≥", "±"]
    let formulaOthers: Set<Character> = ["-", "−", "－", "/", "／", "*", "＊", "(", ")", "（", "）", ".", " "]
    let scripts = Set("⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁿⁱ₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎")
    func isFormula() -> Bool {
        var core = Array(text)
        while let last = core.last, let s = last.unicodeScalars.first, last.unicodeScalars.count == 1, (0x3041...0x309F).contains(s.value) {
            core.removeLast()  // 後ろの助詞 (2x+3=7を)
        }
        guard core.contains(where: { formulaMarks.contains($0) || scripts.contains($0) }) else { return false }
        var run = 0
        for c in core {
            if scripts.contains(c) || formulaMarks.contains(c) || formulaOthers.contains(c) || isDigit(c) { run = 0; continue }
            guard isLetter(c) else { return false }
            run += 1
            if run > 2 { return false }
        }
        return true
    }
    let chars = Array(text)
    let letters = chars.filter(isLetter).count, digits = chars.filter(isDigit).count
    if letters >= 1 && digits >= 1 && letters + digits >= 6 && !isFormula() { return true }
    if chars.contains(where: { symbols.contains($0) }) && letters + digits >= 3 && !isNumericNotation(text) && !isFormula() { return true }
    // C: かなの並びの中に残った小文字の英字 (前の字がかなで、英字の並びが 1〜2 字)
    var i = 0
    while i < chars.count {
        guard chars[i].isASCII && chars[i].isLowercase else { i += 1; continue }
        var j = i
        while j < chars.count && chars[j].isASCII && chars[j].isLetter { j += 1 }
        if j - i <= 2 && i > 0 && isKana(chars[i - 1]) && chars[i..<j].allSatisfy({ $0.isLowercase }) { return true }
        i = j
    }
    return false
}
