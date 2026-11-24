import KanaKanjiConverterModule

// Recover a possible particle hidden by a katakana noun, but require nouns on
// both sides and an exact dictionary path for the alternate spelling.
func nounCoordinationAlternative(_ candidate: Candidate, lookup: (String, String) -> Candidate?) -> Candidate? {
    let elements = candidate.data
    guard elements.count >= 2, candidate.correspondingCount <= 128 else { return nil }
    var attempts = 0
    for index in 0..<(elements.count - 1) {
        let word = elements[index]
        let next = elements[index + 1]
        guard word.lcid == 1285, word.rcid == 1285, next.lcid == 1285,
              word.word == word.ruby, word.word.hasSuffix("ト"), word.ruby.count >= 3 else { continue }
        guard attempts < 2 else { break }
        attempts += 1
        let head = String(word.word.dropLast())
        let headRuby = String(word.ruby.dropLast()).toHiragana()
        guard let noun = lookup(headRuby, head), noun.data.count == 1,
              noun.data[0].lcid == 1285, noun.data[0].rcid == 1285,
              noun.correspondingCount == headRuby.count else { continue }
        let text = elements[..<index].map(\.word).joined() + head + "と" + elements[(index + 1)...].map(\.word).joined()
        let reading = elements.map(\.ruby).joined().toHiragana()
        if let alternative = lookup(reading, text), alternative.correspondingCount == candidate.correspondingCount {
            return alternative
        }
    }
    return nil
}
