import Foundation
import KanaKanjiConverterModule

/// 2026-11-08 (仕事 42 の直し、まとめ役 11-07 23:38「まれな字の表記は下の候補として」):
/// 常用外の字の表記の表 (extra_dictionaries/core_rare_kanji.tsv、印 isRareSpelling) の語は、ふつうの書き方より下の候補にする。
/// AI (Zenzai) は辞書の値を見ずに字を選ぶ (前置きの制約を満たす道が辞書にあれば、値が低くても選ぶ) ので、表の値を下げても一覧の 1 番目は戻らない。
/// そこで、一覧の 1 番目が表の語を含むときだけ、表を隠して一覧の 1 番目を作り直し (表が無いときの 1 番目 = 前の 1 番目)、それを見る:
/// - 前の 1 番目が今の 1 番目と同じなら、表は 1 番目を変えていないので何もしない (死屍累々 は表にもあるが、表が無くても 1 番目)
/// - 前の 1 番目が辞書の 1 語 (編む・隠して)、辞書の語＋送り仮名・助詞・助動詞だけ (ソレ|とも)、同じ語の一部をかなにした形 (阿弥陀くじ・キツネうどん:
///   漢字が全部今の 1 番目にもある) なら、前の 1 番目を 1 番目に戻す (表の語の形はすぐ後ろ)
/// - 前の 1 番目がばらばらの語をつないだ崩れた形 (消し|かけ|て → 嗾けて・秋|済す → 秋茄子) なら、表の語の形を 1 番目のままにする
/// 利用者が選んだ語 (学習した語) は表を隠しても残るので、選んだ表の語は前の 1 番目と同じになり、そのまま 1 番目に出る。
/// 変換 (仮表示・Space) は変えない (一般の文 3,675 で表の語に替わった文は 0)。変換し直すのは一覧を開いたとき (ListCandidates) だけ
struct RareSpellingWord: Equatable {
    var word: String
    var lcid: Int
}

/// 2026-11-08: 常用外の字の表記の表 (core_rare_kanji) の分野辞書の名前。引いた語に isRareSpelling の印を付ける。
/// AZK_RARE_DEMOTE=0 なら付けない (開発用のビルドだけ。直す前と同じ動きで測るため)
func rareSpellingIdentifiers(environment: [String: String], always: [String]) -> Set<String> {
    environment["AZK_RARE_DEMOTE"] == "0" ? [] : Set(always.filter { $0 == "domain_core_rare_kanji" })
}

func isRareSpellingKanji(_ c: Character) -> Bool {
    c.unicodeScalars.contains { (0x4E00...0x9FFF).contains($0.value) || (0x3400...0x4DBF).contains($0.value) || $0.value == 0x3005 }
}

/// 前の 1 番目 previous がふつうの書き方か (words はその語の並び。分からなければ nil)
func isPlainRareAlternative(_ text: String, words: [RareSpellingWord]?, top: String) -> Bool {
    // 助詞・助動詞 (品詞の番号 147〜549) でない語が 1 つまで = 辞書の 1 語、辞書の語＋送り仮名・助詞・助動詞
    if let words, words.filter({ !(147...549).contains($0.lcid) }).count <= 1 { return true }
    // 同じ語の一部をかなにした形: 漢字が全部今の 1 番目にもある (かなだけの形も入る)
    return text.allSatisfy { !isRareSpellingKanji($0) || top.contains($0) }
}

/// 一覧 texts (1 番目 top) と、表が無いときの 1 番目 previous (語の並び words) から並べ直す。
/// previous がふつうの書き方なら previous を 1 番目に (一覧に無ければ足す)、top はすぐ後ろ
func orderingRareSpelling(_ texts: [String], previous: String, words: [RareSpellingWord]?) -> [String] {
    guard let top = texts.first, previous != top, isPlainRareAlternative(previous, words: words, top: top) else { return texts }
    return [previous] + texts.filter { $0 != previous }
}

/// 候補の一覧 (ListCandidates・単漢字の一覧の除外に使う)。1 番目が表の語を含むときだけ、表を隠して一覧の本体を作り直し、その 1 番目と比べる。
/// 作り直しの前と後は、変換器の格子と AI の制約だけを捨てる (前は表の語が残らないように、後は表の語が戻るように)。
/// 3,174 行の試し (2026-11-08、AI あり): 作り直した行 (317 行、1 番目に表の語が入ったときだけ) の一覧を開く時間は 90% で 143 → 201ms、
/// 作り直さない行は 144 → 138ms (変わらない)。AI の会話まで捨てる (stopComposition) と、作り直した行が 90% で 576ms になり、
/// あとの読みが遅くなって結果も順番で揺れた (作り直していない 8 読みの 1 番目が替わった)。
/// 軽くする形 (AI の変換 1 回と辞書だけ 1 回・本体を 1 番目が決まるところまで) は、表が無いときの 1 番目と合わなかった
/// (AI が崩れた形を返す・かなのままの読みは本体の後ろの段で 1 番目が決まる)
@MainActor func buildCandidates(hiragana: String, context: String) -> [String] {
    let (texts, pool) = buildCandidatesCore(hiragana: hiragana, context: context)
    guard !DicdataStore.hideRareSpelling, let first = texts.first else { return texts }
    let rareWords = Set(pool.flatMap(\.data).filter { $0.metadata.contains(.isRareSpelling) }.map(\.word))
    guard rareWords.contains(where: { first.contains($0) }) else { return texts }
    converter.resetLatticeCache()
    DicdataStore.hideRareSpelling = true
    let plain = buildCandidatesCore(hiragana: hiragana, context: context)
    DicdataStore.hideRareSpelling = false
    converter.resetLatticeCache()
    guard let previous = plain.texts.first else { return texts }
    let count = directComposingText(hiragana).input.count
    let words = plain.pool.first { $0.text == previous && $0.correspondingCount == count }?.data.map { RareSpellingWord(word: $0.word, lcid: $0.lcid) }
    if devEnvironment["AZK_DEBUG"] != nil {
        debugWrite("rare \(hiragana) top=\(first) previous=\(previous) words=\(words?.map { "\($0.word)/\($0.lcid)" } ?? [])\n".data(using: .utf8)!)
    }
    return orderingRareSpelling(texts, previous: previous, words: words)
}
