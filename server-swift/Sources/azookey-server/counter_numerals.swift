import Foundation
import KanaKanjiConverterModule

/// 2026-11-06 (仕事 36): かなで打った数詞 (いっかい・ふたり・にじゅうさんにん) を、漢数字の形 (一回・二人・二十三人) を先にして出す設定 (input.counter_numerals)。
/// "arabic" (初期値) は今のまま (azooKey の辞書は 1回・2人・3日 を算用数字の 1 語で持っていて、それが先に出る)。"kanji" のときは:
/// - 変換 (仮表示・清書・Space) で選んだ語の並びの、読みがかなだけで算用数字を含む語 (1回/イッカイ・222/ニヒャクニジュウニ) を漢数字に直す
/// - 一覧では、1 番目がそういう形なら漢数字の形を 1 番目に、算用数字の形を 2 番目にする
/// 変えないもの: 数字のキーで打った数字 (読みに数字がある語)、英字・記号の入る語 (4K・5G・3.5・50%・1/3)、もともと漢字の語 (一緒・一部・四十九日)。
/// 数字の全角の設定 (fullwidth_digits) は IME 本体が算用数字にかけるので、2 番目の形 (１回) にはそのまま効く。
/// 一覧で算用数字の形を選んで確定したら、その語は漢数字にしない (memory\counter_numerals_kept.txt。個人のデータで、配らない・git に入れない)。
/// 漢数字の形を選び直したら、その記録を消す
@MainActor func counterNumeralsKanji() -> Bool { (config["counterNumerals"] as? String) == "kanji" }

let kanjiDigits: [Character] = ["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"]

/// 位取りの漢数字 (23 → 二十三、100 → 百、1000 → 千、10000 → 一万、1980 → 千九百八十)。0 は 〇。
/// 頭に 0 のある並び (007)・16 桁を超える並びは nil
func kanjiNumeral<S: StringProtocol>(_ digits: S) -> String? {
    guard !digits.isEmpty, digits.count <= 16, digits.allSatisfy({ $0.isASCII && $0.isNumber }) else { return nil }
    if digits == "0" { return "〇" }
    guard digits.first != "0" else { return nil }
    let values = digits.compactMap { $0.wholeNumberValue }
    let small: [String] = ["", "十", "百", "千"]
    let large: [String] = ["", "万", "億", "兆"]
    var out = ""
    for (index, value) in values.enumerated() {
        let place = values.count - 1 - index
        let inGroup = place % 4
        if value != 0 {
            // 十・百・千 の前の 一 は書かない (十・百・千)。万・億・兆 の前の 一 は書く (一万)
            if !(value == 1 && inGroup > 0) { out.append(kanjiDigits[value]) }
            out += small[inGroup]
        }
        if inGroup == 0 && place > 0 {
            let start = max(0, index - 3)
            if values[start...index].contains(where: { $0 != 0 }) { out += large[place / 4] }
        }
    }
    return out
}

/// 桁を並べた漢数字 (1980 → 一九八〇)
func kanjiDigitsInRow<S: StringProtocol>(_ digits: S) -> String? {
    guard !digits.isEmpty, digits.allSatisfy({ $0.isASCII && $0.isNumber }) else { return nil }
    return String(digits.compactMap { $0.wholeNumberValue.map { kanjiDigits[$0] } })
}

/// 語の中の算用数字を漢数字に直した形。数字・漢字・かな・ー・々 だけでできた語のときだけ (英字・記号の入る語は nil)。数字が無ければ nil。
/// 4 桁の 100 の倍数でない数＋年 (1980年・2024年度) は桁を並べる (一九八〇年)。ほかは位取り (二十三人・二千年・千九百年・七百九十四年・千九百八十円)。
/// 数え (tools/counters/count_kanji_numerals.py と同じ切り方。1000〜2999 の 4 桁＋年、後ろに漢字が続かない所): 100 の倍数でない数は
/// 日本語版ウィキペディアで 桁を並べる 740・位取り 70、青空文庫で 5,100・169。100 の倍数は ウィキペディアで 位取り 469・桁を並べる 57、青空文庫で 232・109。
/// ほかの助数詞は、どの桁でも位取りが多い (ウィキペディアの 4 桁の 円 は 位取り 872・桁を並べる 5)
func kanjifyingNumerals(_ word: String) -> String? {
    guard word.contains(where: { $0.isASCII && $0.isNumber }) else { return nil }
    var out = ""
    var run = ""
    func flush(before next: Character?) -> Bool {
        guard !run.isEmpty else { return true }
        let year = next == "年" && run.count == 4 && run.first != "0" && (Int(run).map { $0 % 100 != 0 } ?? false)
        guard let kanji = year ? kanjiDigitsInRow(run) : kanjiNumeral(run) else { return false }
        out += kanji
        run = ""
        return true
    }
    for c in word {
        if c.isASCII && c.isNumber {
            run.append(c)
            continue
        }
        guard isNumeralWordLetter(c), flush(before: c) else { return nil }
        out.append(c)
    }
    guard flush(before: nil) else { return nil }
    return out
}

/// 数詞の語に入ってよい字 (漢字・ひらがな・カタカナ・ー・々・ヶ)
func isNumeralWordLetter(_ c: Character) -> Bool {
    guard c.unicodeScalars.count == 1, let s = c.unicodeScalars.first else { return false }
    switch s.value {
    case 0x3041...0x3096, 0x30A1...0x30FA, 0x30FC, 0x3005, 0x30F6, 0x4E00...0x9FFF, 0x3400...0x4DBF, 0xF900...0xFAFF:
        return true
    default:
        return false
    }
}

/// 読みがかなだけか (数字のキーで打った数字・英字・記号を含まない)
func isKanaOnlyReading(_ reading: String) -> Bool {
    !reading.isEmpty && reading.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) || (0x30A1...0x30FA).contains($0.value) || $0.value == 0x30FC }
}

/// 語の並びの、読みがかなだけで算用数字を含む語を漢数字にした並び (kept の語はそのまま)。替える所が無ければ nil
func kanjifyingNumeralElements(_ data: [DicdataElement], kept: Set<String>) -> [DicdataElement]? {
    var out = data
    var changed = false
    for (i, e) in data.enumerated() where !kept.contains(e.word) && isKanaOnlyReading(e.ruby) {
        guard let kanji = kanjifyingNumerals(e.word) else { continue }
        out[i] = DicdataElement(word: kanji, ruby: e.ruby, lcid: e.lcid, rcid: e.rcid, mid: e.mid, value: e.value())
        changed = true
    }
    return changed ? out : nil
}

/// 変換で選んだ候補に kanjifyingNumeralElements をかけた候補 (設定が "kanji" でなければ・替える所が無ければ nil)
@MainActor func kanjiNumeralsApplied(_ candidate: Candidate) -> Candidate? {
    guard counterNumeralsKanji(), let data = kanjifyingNumeralElements(candidate.data, kept: counterNumeralsKept()) else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
                     lastMid: candidate.lastMid, data: data)
}

/// 一覧: 読みがかなだけなら、いちばん前の算用数字を含む形 (1回・第1回) のすぐ前に、その漢数字の形 (一回・第一回) を置く
/// (ふつうは 1 番目が算用数字の形なので、漢数字の形が 1 番目、もとの形が 2 番目になる。学習した語などが前にあれば、その後ろで同じ並びに)。
/// 漢数字の形が一覧の下にあれば、そこから上げる。その形に kept の語が入っていれば変えない
func placingKanjiNumerals(_ texts: [String], reading: String, enabled: Bool, kept: Set<String>) -> [String] {
    guard enabled, isKanaOnlyReading(reading),
          let at = texts.firstIndex(where: { kanjifyingNumerals($0) != nil }),
          !kept.contains(where: { texts[at].contains($0) }), let kanji = kanjifyingNumerals(texts[at]) else { return texts }
    var out = texts
    if let old = out.firstIndex(of: kanji), old < at { return texts }
    out.removeAll { $0 == kanji }
    out.insert(kanji, at: out.firstIndex(of: texts[at])!)
    return out
}

/// 確定した候補の語で、算用数字の形を一覧で選び直したものを覚え、漢数字の形を選び直したものの記録は消す。
/// defaultText は今の表示 (一覧を出した読みならその 1 番目)。今の表示のまま確定したものは数えない
func notingCounterNumerals(_ kept: Set<String>, data: [DicdataElement], text: String, defaultText: String?) -> Set<String> {
    guard text != defaultText else { return kept }
    var out = kept
    for e in data where isKanaOnlyReading(e.ruby) && kanjifyingNumerals(e.word) != nil {
        out.insert(e.word)
    }
    for word in kept where kanjifyingNumerals(word).map({ text.contains($0) }) == true {
        out.remove(word)
    }
    return out
}

func parseCounterNumeralsKept(_ text: String?) -> Set<String> {
    guard let text else { return [] }
    return Set(text.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }
        .filter { !$0.isEmpty && !$0.hasPrefix("#") && kanjifyingNumerals($0) != nil })
}

func formatCounterNumeralsKept(_ kept: Set<String>) -> String {
    (["# 漢数字を先にする設定で、利用者が算用数字の形を選んだ語 (memory\\counter_numerals_kept.txt)。個人のデータ (配らない)"] + kept.sorted())
        .joined(separator: "\n") + "\n"
}

@MainActor var counterNumeralsKeptCache: Set<String>? = nil

@MainActor func counterNumeralsKeptURL() -> URL { execURL.appendingPathComponent("memory").appendingPathComponent("counter_numerals_kept.txt") }

@MainActor func counterNumeralsKept() -> Set<String> {
    guard personalDataAllowed else { return [] }
    if let kept = counterNumeralsKeptCache { return kept }
    let stored = parseCounterNumeralsKept(BoundedTable.text(counterNumeralsKeptURL(), maxBytes: 1 << 20))
    counterNumeralsKeptCache = stored
    return stored
}

@MainActor func saveCounterNumeralsKept(_ kept: Set<String>) {
    guard personalDataAllowed else { return }
    counterNumeralsKeptCache = kept
    clearPreviewMemo()
    try? formatCounterNumeralsKept(kept).data(using: .utf8)?.write(to: counterNumeralsKeptURL(), options: .atomic)
}

/// 確定した文節 (learnOne で表記の合った候補) の数詞の形を覚える (設定が "kanji" のときだけ)
@MainActor func noteCounterNumeralChoices(_ candidate: Candidate, defaultText: String?) {
    guard counterNumeralsKanji() else { return }
    let kept = counterNumeralsKept()
    let noted = notingCounterNumerals(kept, data: candidate.data, text: candidate.text, defaultText: defaultText)
    if noted != kept { saveCounterNumeralsKept(noted) }
}

/// 候補の一覧で学習した語を忘れる (Ctrl+Delete。ForgetLearned) とき、その語を算用数字のまま出す記録も消す
@MainActor func forgetCounterNumeralChoices(text: String) {
    let kept = counterNumeralsKept()
    let left = kept.filter { !text.contains($0) }
    if left != kept { saveCounterNumeralsKept(left) }
}
