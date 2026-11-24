import Foundation

struct NominalRequest {
    let prefix: String
    let segments: [(word: String, rubyCount: Int)]
}

// Contextual homophones, not a global boost for 語呂 over 頃/語録.
func nominalRequest(reading: String) -> NominalRequest? {
    let endings = ["", "ね", "よ", "よね"].flatMap { particle in
        ["", "！", "？", "。", "!", "?"].map { particle + $0 }
    }
    let verbs = [("くれないか", "くれないか"), ("ください", "ください"),
                 ("ちょうだい", "ちょうだい"), ("くれ", "くれ"), ("おしえてくれ", "教えてくれ")]
    for ending in endings where reading.hasSuffix(ending) {
        let body = String(reading.dropLast(ending.count))
        for (ruby, text) in verbs where body.hasSuffix(ruby) {
            let beforeVerb = String(body.dropLast(ruby.count))
            let particle = beforeVerb.hasSuffix("を") ? "を" : ""
            if ruby == "おしえてくれ" && particle.isEmpty { continue }
            let beforeParticle = String(beforeVerb.dropLast(particle.count))
            for (nounRuby, noun) in [("ごろあわせ", "語呂合わせ"), ("ごろ", "語呂")] where beforeParticle.hasSuffix(nounRuby) {
                let prefix = String(beforeParticle.dropLast(nounRuby.count))
                guard prefix.isEmpty || prefix.hasSuffix("の") else { continue }
                var segments = [(word: noun, rubyCount: nounRuby.count)]
                if !particle.isEmpty { segments.append((particle, particle.count)) }
                segments.append((text, ruby.count))
                if !ending.isEmpty { segments.append((ending, ending.count)) }
                return NominalRequest(prefix: prefix, segments: segments)
            }
        }
    }
    return nil
}
