//
//  ClauseSplit.swift
//
//  azooKey-Windows (Google日本語入力風の文節変換) のための追加。
//  変換候補の単語列を、ラティス構築時と同じ規則 (DicdataStore.isClause) で文節に区切る。
//

import Foundation

extension Candidate {
    /// 文節ごとの表記と、その読みの文字数を返す。
    /// 2026-11-21: latinParticlesOnly なら、separateParticles で助詞を別の文節にするのは英字の語の後ろ (githubを) だけにし、
    /// 名詞の後ろの助詞は前につなげる (教室に／陥る・必要が／ある。ふつうの IME の文節)
    public func clauseSplit(separateParticles: Bool = false, latinParticlesOnly: Bool = false) -> [(word: String, rubyCount: Int)] {
        var result: [(word: String, rubyCount: Int)] = []
        var prevRcid = CIDData.BOS.cid
        var previousIsLatin = false
        var previousIsParticle = false
        for element in self.data {
            if element.word.isEmpty && element.ruby.isEmpty {
                continue
            }
            // 英字 (英語と判定してそのまま入れた語) は 1 文字ずつの要素になっているので、続く限り 1 つの文節にまとめる。
            // 英字の語は名詞として扱う。候補選択では助詞を別の範囲にできる。
            let isLatin = !element.word.isEmpty && element.word.unicodeScalars.allSatisfy { $0.isASCII && CharacterSet.letters.contains($0) }
            let isParticle = CIDData.isJoshi(cid: element.lcid)
            let merge: Bool
            if result.isEmpty {
                merge = false
            } else if separateParticles && isParticle
                && (previousIsParticle || previousIsLatin || (!latinParticlesOnly && (1283..<1310).contains(prevRcid))) {
                merge = previousIsParticle
            } else if isLatin && previousIsLatin {
                merge = true
            } else if isLatin {
                merge = false
            } else {
                merge = !DicdataStore.isClause(previousIsLatin ? CIDData.固有名詞.cid : prevRcid, element.lcid)
            }
            if merge {
                result[result.count - 1].word += element.word
                result[result.count - 1].rubyCount += element.ruby.count
            } else {
                result.append((word: element.word, rubyCount: element.ruby.count))
            }
            prevRcid = element.rcid
            previousIsLatin = isLatin
            previousIsParticle = isParticle
        }
        return result
    }
}
