import KanaKanjiConverterModule

func appendingEmojiCandidates(to candidates: [String], reading: String, replacer: TextReplacer) -> [String] {
    let matches = replacer.getSearchResult(query: reading, target: [.emoji])
    let emoji = Set(matches.map(\.text))
    var result = candidates.filter { !emoji.contains($0) }
    var seen = Set(result)
    for match in matches where seen.insert(match.text).inserted {
        result.append(match.text)
    }
    return result
}
