import Foundation
import KanaKanjiConverterModule

/// 2026-11-02: 形容動詞の連体形「な」のすぐ後ろに助詞・句読点が付いた形 (まずこうかなとおもう → まず高価なと思う)。
/// AI (zenz) はこの形を選ぶことがあるが、辞書だけの変換は まず|こう|かなと思う と正しく選べている。
/// AI の変換でこの形の候補は使わない: 融合採点ではこの形を含まない候補から選び、ほかでは辞書だけの変換 (この形を含まなければ) にする。
/// 形: 形容動詞の語幹 (品詞 1287) と「な」が 1 語になった語 (高価な。右の品詞 454) か、語幹のすぐ後ろの「な」の語 (無理|な)。その次の語が
///   助詞 (CIDData.isJoshi) で と・が・を・は・で・に・も・か・へ・より で始まる (から は か に入る) か、句読点で始まる。
///   の・ので・のに・ん (静かなのが・無理なので) と、副助詞 (大変なくらい・好きなだけ) は付いてよいので見ない
let adjectivalStemCID = 1287
let adjectivalNaCID = 454
let danglingNaParticles = ["と", "が", "を", "は", "で", "に", "も", "か", "へ", "より"]
let danglingNaPunctuation: Set<Character> = ["。", "、", "？", "！", "?", "!", "，", "．", ",", "."]

/// 比べるための開発用の切り替え: AZK_ADJECTIVAL_NA=0 で見ない
let adjectivalNaFilter = devEnvironment["AZK_ADJECTIVAL_NA"] != "0"

func hasDanglingAdjectivalNa(_ data: [DicdataElement]) -> Bool {
    for i in data.indices.dropLast() {
        let e = data[i]
        guard e.rcid == adjectivalNaCID, e.word.hasSuffix("な"),
              e.lcid == adjectivalStemCID || (i > 0 && e.word == "な" && data[i - 1].rcid == adjectivalStemCID) else { continue }
        let next = data[i + 1]
        if let first = next.word.first, danglingNaPunctuation.contains(first) { return true }
        // 助詞の字のすぐ後ろが小書きの語 (に＋ゃ の にゃ) は、その助詞でない (一般の文 3,660 行の 組織必要なにゃ が ニャ になっていた)
        if CIDData.isJoshi(cid: next.lcid) && danglingNaParticles.contains(where: { particle in
            next.word.hasPrefix(particle) && !(next.word.dropFirst(particle.count).first.map { "ぁぃぅぇぉゃゅょゎ".contains($0) } ?? false)
        }) { return true }
    }
    return false
}

/// 語の並びが、形容動詞の語幹＋な (高価な・無理|な) で終わる
func endsWithAdjectivalNa(_ data: [DicdataElement]) -> Bool {
    guard let last = data.last, last.rcid == adjectivalNaCID, last.word.hasSuffix("な") else { return false }
    return last.lcid == adjectivalStemCID || (data.count >= 2 && last.word == "な" && data[data.count - 2].rcid == adjectivalStemCID)
}

/// 語の並びが、終助詞の かな で終わる。辞書では か|な (どちらも助詞の品詞。静か|か|な) か、前の語とまとまった語 (いいかな。右の品詞が助詞)
func endsWithFinalKana(_ data: [DicdataElement]) -> Bool {
    guard let last = data.last, last.word.hasSuffix("な"), CIDData.isJoshi(cid: last.rcid) else { return false }
    if last.word.hasSuffix("かな") { return true }
    return last.word == "な" && data.count >= 2 && data[data.count - 2].word.hasSuffix("か") && CIDData.isJoshi(cid: data[data.count - 2].rcid)
}

/// 2026-11-02: Space の一覧で、1 番目が形容動詞の語幹＋な で読みが終わり (まずこうかな → まず高価な)、同じ読みで終助詞の かな で終わる候補
/// (まずこうかな) が 3 番目より後ろにあれば、それを 2 番目に置く。1 番目は変えない (最初に出る変換は変えず、表示を増やさない)。
/// 一覧の候補は字だけなので、品詞は辞書を引き直して見る (scopeLookup。打鍵のたびに引き直さないよう覚えておく)
@MainActor func orderingFinalKana(_ texts: [String], reading: String, context: String) -> [String] {
    guard adjectivalNaFilter, texts.count >= 3, reading.hasSuffix("かな"), texts[0].hasSuffix("な"),
          let first = scopeLookup(reading: reading, text: texts[0], context: context), endsWithAdjectivalNa(first.data),
          let index = texts.indices.dropFirst(2).first(where: { texts[$0].hasSuffix("かな")
              && (scopeLookup(reading: reading, text: texts[$0], context: context).map { endsWithFinalKana($0.data) } ?? false) }) else { return texts }
    var result = texts
    result.remove(at: index)
    result.insert(texts[index], at: 1)
    return result
}
