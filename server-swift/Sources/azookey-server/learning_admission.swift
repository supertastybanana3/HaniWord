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
