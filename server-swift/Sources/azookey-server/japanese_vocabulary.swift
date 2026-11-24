import Foundation

struct JapaneseVocabulary: Decodable {
    enum Category: String, Decodable {
        case slang, admission, subject
    }

    enum PartOfSpeech: String, Decodable {
        case noun, verb, interjection
        case suruNoun = "suru_noun"
        case iAdjective = "i_adjective"

        var isNominal: Bool { self == .noun || self == .suruNoun }
    }

    struct Entry: Decodable {
        let reading: String
        let word: String
        let category: Category
        var variants: [String] = []
        var preferStandalone: Bool = false
        var recentAuthors: Int = 0
        /// 2026-11-22: 表の段 (slot。前に来る候補の数 5・10・18)。表は回数でなく段を持つ (公開の表に検索の回数を置かない)。無ければ recentAuthors から決める
        var slot: Int? = nil
        var ambiguous: Bool = false
        var partOfSpeech: PartOfSpeech = .noun

        var candidateSlot: Int {
            max(slot ?? (recentAuthors >= 30 ? 5 : (recentAuthors >= 10 ? 10 : 18)), ambiguous ? 16 : 5)
        }

        enum CodingKeys: String, CodingKey {
            case reading, word, category, variants
            case preferStandalone = "prefer_standalone"
            case recentAuthors = "recent_authors"
            case slot
            case ambiguous
            case partOfSpeech = "part_of_speech"
        }

        init(reading: String, word: String, category: Category, preferStandalone: Bool = false, recentAuthors: Int = 0, ambiguous: Bool = false, partOfSpeech: PartOfSpeech = .noun, variants: [String] = []) {
            self.reading = reading
            self.word = word
            self.category = category
            self.variants = variants
            self.preferStandalone = preferStandalone
            self.recentAuthors = recentAuthors
            self.ambiguous = ambiguous
            self.partOfSpeech = partOfSpeech
        }

        init(from decoder: any Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            reading = try values.decode(String.self, forKey: .reading)
            word = try values.decode(String.self, forKey: .word)
            category = try values.decode(Category.self, forKey: .category)
            variants = try values.decodeIfPresent([String].self, forKey: .variants) ?? []
            preferStandalone = try values.decodeIfPresent(Bool.self, forKey: .preferStandalone) ?? false
            recentAuthors = try values.decodeIfPresent(Int.self, forKey: .recentAuthors) ?? 0
            slot = try values.decodeIfPresent(Int.self, forKey: .slot).map { min(max($0, 0), 50) }
            ambiguous = try values.decodeIfPresent(Bool.self, forKey: .ambiguous) ?? false
            partOfSpeech = try values.decodeIfPresent(PartOfSpeech.self, forKey: .partOfSpeech) ?? .noun
        }
    }

    struct Override {
        let segments: [(word: String, rubyCount: Int)]
        var count: Int { segments.reduce(0) { $0 + $1.rubyCount } }
    }

    let schemaVersion: Int
    let entries: [Entry]

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case entries
    }

    static let empty = JapaneseVocabulary(schemaVersion: 1, entries: [])

    static func load(from url: URL) -> JapaneseVocabulary {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return .empty }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: 256_001), data.count <= 256_000,
              let decoded = try? JSONDecoder().decode(Self.self, from: data),
              decoded.schemaVersion == 1, decoded.entries.count <= 512 else { return .empty }
        var seen: Set<String> = []
        let valid = decoded.entries.filter { entry in
            (2...40).contains(entry.reading.count) && !entry.word.isEmpty && entry.word.count <= 60
                && entry.reading.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) || $0.value == 0x30FC }
                && !entry.word.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
                && seen.insert(entry.reading + "\t" + entry.word).inserted
        }
        let bounded = valid.map { entry in
            var entry = entry
            var forms: Set<String> = [entry.word]
            entry.variants = Array(entry.variants.prefix(4)).filter {
                !$0.isEmpty && $0.count <= 60
                    && !$0.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
                    && forms.insert($0).inserted
            }
            return entry
        }
        return JapaneseVocabulary(schemaVersion: 1, entries: bounded)
    }

    func alternatives(for reading: String) -> [String] {
        entries.filter { $0.reading == reading }.flatMap { [$0.word] + $0.variants }
    }

    func insertingAlternatives(reading: String, into candidates: [String]) -> [String] {
        let extras = entries.filter { $0.reading == reading }
            .sorted { $0.candidateSlot < $1.candidateSlot }
        var result = candidates
        var inserted = 0
        for entry in extras {
            let forms = [entry.word] + entry.variants
            if let anchor = result.firstIndex(where: forms.contains) {
                // A learned/model-ranked spelling keeps its place; equivalent forms follow it.
                let existing = result.filter(forms.contains)
                let missing = forms.filter { !existing.contains($0) }
                result.removeAll(where: forms.contains)
                result.insert(contentsOf: existing + missing, at: anchor)
                inserted += missing.count
            } else {
                result.insert(contentsOf: forms, at: min(min(entry.candidateSlot, candidates.count) + inserted, result.count))
                inserted += forms.count
            }
        }
        return result
    }

    // Match complete words and bounded continuations, never arbitrary substrings.
    func prefixOverride(for reading: String) -> Override? {
        for entry in entries.sorted(by: { $0.reading.count > $1.reading.count }) where reading.hasPrefix(entry.reading) {
            let tail = String(reading.dropFirst(entry.reading.count))
            if entry.category == .admission {
                for (ruby, text) in [("しぼう", "志望"), ("にいきたい", "に行きたい")] where tail.hasPrefix(ruby) {
                    let remainder = String(tail.dropFirst(ruby.count))
                    guard isAdmissionEnding(remainder, aspiration: ruby == "にいきたい") else { continue }
                    var segments = [(word: entry.word, rubyCount: entry.reading.count)]
                    if ruby == "にいきたい" {
                        segments += [(word: "に", rubyCount: 1), (word: "行きたい", rubyCount: 4)]
                    } else {
                        segments.append((word: text, rubyCount: ruby.count))
                    }
                    return Override(segments: segments)
                }
            } else if entry.category == .slang && entry.preferStandalone {
                if tail.isEmpty || entry.partOfSpeech.isNominal && isNominalEnding(tail, word: entry.word) {
                    return Override(segments: [(word: entry.word, rubyCount: entry.reading.count)])
                }
            }
        }
        return nil
    }

    private func isAdmissionEnding(_ tail: String, aspiration: Bool) -> Bool {
        if tail.isEmpty || tail.first.map({ "。！？!?、,」』\"".contains($0) }) == true { return true }
        if ["です", "でした", "だ", "だった", "だよ", "だね"].contains(tail) { return true }
        if aspiration { return ["とおもう", "けど", "ので", "から"].contains(where: tail.hasPrefix) }
        return false
    }

    private func isNominalEnding(_ tail: String, word: String) -> Bool {
        if tail.first.map({ "。！？!?、,」』\"".contains($0) }) == true { return true }
        if ["だ", "です", "だった", "でした", "だよ", "だね"].contains(tail) { return true }
        return ["の", "は", "を", "も"].contains(where: tail.hasPrefix)
            || word != "運ガキ" && ["が", "に"].contains(where: tail.hasPrefix)
    }

    func refiningScopes(reading: String, segments: [(word: String, rubyCount: Int)]) -> [(word: String, rubyCount: Int)] {
        guard segments.allSatisfy({ $0.rubyCount > 0 }),
              segments.reduce(0, { $0 + $1.rubyCount }) == reading.count else { return segments }
        let characters = Array(reading)
        let longestFirst = entries.sorted(by: { $0.reading.count > $1.reading.count })
        var result: [(word: String, rubyCount: Int)] = []
        var index = 0
        var offset = 0
        while index < segments.count {
            var merged: (word: String, rubyCount: Int, end: Int)?
            for entry in longestFirst {
                let end = offset + entry.reading.count
                guard end <= characters.count, String(characters[offset..<end]) == entry.reading else { continue }
                var length = 0
                var text = ""
                var next = index
                while next < segments.count && length < entry.reading.count {
                    length += segments[next].rubyCount
                    text += segments[next].word
                    next += 1
                }
                let particleFollows = next < segments.count && ["は", "が", "を", "の", "に", "も", "だ", "です"].contains(segments[next].word)
                if length == entry.reading.count,
                   text == entry.word || offset == 0 && end == characters.count
                    || entry.category == .slang && entry.partOfSpeech.isNominal && entry.word != "運ガキ" && particleFollows {
                    merged = (text, length, next)
                    break
                }
            }
            if let merged {
                result.append((word: merged.word, rubyCount: merged.rubyCount))
                index = merged.end
                offset += merged.rubyCount
            } else {
                result.append(segments[index])
                offset += segments[index].rubyCount
                index += 1
            }
        }
        return result
    }
}

@MainActor var japaneseVocabulary = JapaneseVocabulary.empty
