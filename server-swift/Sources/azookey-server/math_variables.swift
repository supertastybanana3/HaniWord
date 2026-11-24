import Foundation
import KanaKanjiConverterModule

/// 2026-11-06 (仕事 10c): 数学の名詞のすぐ後ろの変数 1 字が母音 (a・i・u・e・o) のとき、ローマ字の母音として かな に入ってしまう
/// (bekutorua → ベクトルあ・ベクトル亜。b・x のような子音の字は英字のまま残る)。数学の式の直しがオン (よく打つ分野に数学・物理があり、
/// input.math_expressions がオン = mathSentenceSignals がある) の人だけ、変換が選んだ語の並びで
/// 「表の名詞 ＋ 読みが母音 1 字だけの語 (あ・い・亜・胃 …) ＋ 助詞・記号・終わり」のとき、その 1 字を打った母音の英字にする (ベクトルa・点aの)。
/// 読みの側では替えない (せんいの → 繊維の・こういの → 行為の のように、短い名詞＋母音が別の語になる読みを壊さないため。変換がその語を選べばここに来ない)
let mathVariableVowelsEnabled = devEnvironment["AZK_MATH_VARIABLE_VOWEL"] != "0"

/// 変数 1 字が続く数学の名詞。tools/math_vars/count_noun_letter.py で、Wikipedia の数学の記事 (C の data_ext\math2 の材料) と
/// Wikibooks の数学の本で「名詞＋英字 1 字＋区切り」を数え、どちらかで 15 回以上のもの (11-06)。語の切れ端 (然数・率変数・様体・分集合 など、
/// 長い語の後ろの部分) と、数学でない語 (人・者・手・学者・ドア・プログラム・タイプ・ワシントン) は除いた
let mathVariableNouns: Set<String> = [
    "数", "点", "集合", "関数", "列", "変数", "線", "行列", "整数", "群", "体", "空間", "量", "半径", "自然数", "時刻", "頂点", "ベクトル",
    "定数", "グラフ", "曲線", "直線", "状態", "値", "積", "実数", "確率変数", "素数", "円", "面積", "事象", "関手", "多様体", "問題", "集団",
    "係数", "角", "像", "部分集合", "対象", "写像", "位置", "速度", "交点", "パラメータ", "辺", "位相空間", "図式", "中心", "指数", "命題",
    "圏", "時間", "面", "要素", "距離", "正整数", "記号", "文字", "構造群", "数学", "式", "容器",
]

/// 同じ数え方で、後ろの英字 1 字の 8 割以上が大文字だった名詞 (集合 0.98・行列 1.00・点 0.82・数学 1.00 …)。これらのあとは大文字にする
/// (ほかは打ったとおりの小文字: ベクトル 0.28・関数 0.19・整数 0.06。0.5〜0.8 の 頂点・曲線・命題・式 なども小文字のまま)
let mathUpperVariableNouns: Set<String> = [
    "点", "集合", "列", "行列", "群", "体", "空間", "グラフ", "積", "確率変数", "円", "面積", "事象", "関手", "多様体", "問題", "集団",
    "角", "部分集合", "交点", "位相空間", "図式", "中心", "圏", "構造群", "数学", "容器",
]

let vowelLetters: [String: String] = ["ア": "a", "イ": "i", "ウ": "u", "エ": "e", "オ": "o"]

/// 語の並びの、表の名詞のすぐ後ろの 読みが母音 1 字だけの語 (後ろが助詞・記号・終わり) を英字にした並び。替える所が無ければ nil
func latinizingVariableVowels(_ data: [DicdataElement]) -> [DicdataElement]? {
    var out = data
    var changed = false
    for i in data.indices.dropFirst() {
        let element = data[i]
        guard mathVariableNouns.contains(data[i - 1].word), let letter = vowelLetters[element.ruby], element.word.count == 1 else { continue }
        if i + 1 < data.count {
            let next = data[i + 1]
            guard (147...368).contains(next.lcid) || next.word.allSatisfy(isLiteralSymbol) else { continue }
        }
        let shown = mathUpperVariableNouns.contains(data[i - 1].word) ? letter.uppercased() : letter
        out[i] = DicdataElement(word: shown, ruby: element.ruby, lcid: element.lcid, rcid: element.rcid, mid: element.mid, value: element.value())
        changed = true
    }
    return changed ? out : nil
}

/// 候補の語の並びに latinizingVariableVowels をかけた候補 (替える所が無ければ nil)
func latinizingVariableVowels(_ candidate: Candidate) -> Candidate? {
    guard let data = latinizingVariableVowels(candidate.data) else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
                     lastMid: candidate.lastMid, data: data)
}
