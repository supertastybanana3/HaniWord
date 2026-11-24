import Foundation
import KanaKanjiConverterModule

/// 2026-11-04: 利用者の単語登録 (%APPDATA%\HaniWord\user_dictionary.tsv。個人のデータで、配らない・git に入れない)。
/// 行: 読み (ひらがな)<TAB>語<TAB>品詞<TAB>メモ (品詞とメモは省いてよい)。# で始まる行は説明。設定アプリの「単語登録」が書く (手で書き換えてもよい)。
/// 品詞: 名詞・固有名詞・人名・地名・短縮よみ (顔文字や定型文)。知らない品詞は名詞。読みは 20 字まで (変換器が辞書を引く読みの長さの上限)。
/// 追加辞書 (extra_dictionary.tsv) の語と合わせて動的ユーザ辞書に入れ、登録した語には isUserRegistered の印を付ける:
///   AI の変換で、先頭のトークンを学習した語と同じ優先で判断し、以降は表記どおり残す (ZenzContext)・融合採点で選び直さない・学習に書かない。
/// 学習データを使ってはいけない接続 (personalDataAllowed が偽: サンドボックスのアプリなど) では入れない (登録した語を変換で引き出せないように)。
/// 読み直し: 起動時・設定の読み直し (LoadConfig。設定アプリが保存したあと知らせる)・変換の入口でファイルの更新時刻が変わったとき (2 秒に 1 回まで見る)
@MainActor var extraDictionaryElements: [DicdataElement] = []
@MainActor var userDictionaryElements: [DicdataElement] = []
@MainActor var userDictionaryStamp: Date? = nil
@MainActor var userDictionaryCheckedAt = Date.distantPast
/// 入れている登録した語の表記と、その字数 (文節の並びの直しのあとで、登録した語が消えていないかを見る。keepingUserRegisteredText)
@MainActor var appliedUserWords = Set<String>()
@MainActor var appliedUserWordLengths: [Int] = []
/// 入れている登録した語の読み (カタカナ) → 語 (読み全体が登録した読みと同じ入力のときに使う。wholeReadingUserWord)
@MainActor var appliedUserWordsByRuby: [String: [DicdataElement]] = [:]
/// Initialize で追加辞書と単語登録を入れたあと真 (それより前の設定の読み直しでは入れない)
@MainActor var dynamicDictionaryReady = false
let userDictionaryLimits = (maxBytes: 4 << 20, maxLines: 50_000, maxFieldLength: 256)
/// 読みの長さの上限 (DicdataStore.maxlength。これより長い読みの語は辞書を引く範囲に入らない)
let userDictionaryMaxReading = 20
/// 登録した語の重み (辞書の語はおよそ -8〜-20)。強すぎると、ほかの語の一部を奪う (-5 では めあどれす → <登録したメアド>レス)。
/// 作った 48 文と比べの 20 文で -5・-7・-8・-9 を比べ、登録した語の出方が -5 と変わらず、奪う形が減る -7 にした (2026-11-04)。
/// 試験用のビルドでは AZK_USER_WORD_VALUE で試せる
let userDictionaryValue: PValue = {
    if let v = devEnvironment["AZK_USER_WORD_VALUE"], let f = Float(v) { return PValue(f) }
    return -7
}()

/// 品詞の名前 → 品詞の番号
let userDictionaryPartsOfSpeech: [String: Int] = [
    "名詞": CIDData.一般名詞.cid, "固有名詞": CIDData.固有名詞.cid, "人名": CIDData.人名一般.cid, "地名": CIDData.地名一般.cid,
    "短縮よみ": CIDData.一般名詞.cid,
]

func userDictionaryURL() -> URL? {
    guard let appData = ProcessInfo.processInfo.environment["APPDATA"], !appData.isEmpty else { return nil }
    return URL(filePath: appData).appendingPathComponent("HaniWord/user_dictionary.tsv")
}

/// 読みがひらがな (と ー・ゔ) だけか
func isUserDictionaryReading(_ reading: String) -> Bool {
    !reading.isEmpty && reading.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) || $0 == "ー" || $0 == "ゔ" }
}

/// 表の行 → 動的ユーザ辞書の語 (読みが ひらがなでない・長すぎる・語が空の行は捨てる。同じ読み・語の行は 1 つに)
func parseUserDictionary(_ rows: [[String]]) -> [DicdataElement] {
    var seen = Set<String>()
    var elements: [DicdataElement] = []
    for cols in rows where cols.count >= 2 {
        let reading = cols[0].trimmingCharacters(in: .whitespaces), word = cols[1].trimmingCharacters(in: .whitespaces)
        guard isUserDictionaryReading(reading), reading.count <= userDictionaryMaxReading, !word.isEmpty else { continue }
        let ruby = hiraganaToKatakana(reading)
        guard seen.insert(ruby + "\t" + word).inserted else { continue }
        let cid = cols.count >= 3 ? userDictionaryPartsOfSpeech[cols[2].trimmingCharacters(in: .whitespaces)] ?? CIDData.一般名詞.cid : CIDData.一般名詞.cid
        var element = DicdataElement(word: word, ruby: ruby, cid: cid, mid: MIDData.一般.mid, value: userDictionaryValue)
        element.metadata = .isUserRegistered
        elements.append(element)
    }
    return elements
}

/// 追加辞書の語と (使ってよい接続なら) 登録した語を、動的ユーザ辞書に入れ直す
@MainActor func applyDynamicDictionary() {
    let user = personalDataAllowed ? userDictionaryElements : []
    appliedUserWords = Set(user.map(\.word))
    appliedUserWordLengths = Set(appliedUserWords.map(\.count)).sorted()
    appliedUserWordsByRuby = Dictionary(grouping: user, by: \.ruby)
    converter.sendToDicdataStore(.importDynamicUserDict(extraDictionaryElements + user))
    // 途中の変換の結果 (格子) を捨てる (入れ直した語が次の変換から出るように)
    converter.stopComposition()
    clearPreviewMemo()
}

/// ファイルを読み直す (更新時刻が変わっていなければ読まない。force なら必ず)。変わったら入れ直す
@MainActor func reloadUserDictionary(force: Bool = false) {
    userDictionaryCheckedAt = Date()
    guard let url = userDictionaryURL() else { return }
    let stamp = (try? FileManager.default.attributesOfItem(atPath: url.path))?[.modificationDate] as? Date
    guard force || stamp != userDictionaryStamp else { return }
    userDictionaryStamp = stamp
    let elements = stamp == nil ? [] : parseUserDictionary(BoundedTable.rows(url, maxBytes: userDictionaryLimits.maxBytes, maxLines: userDictionaryLimits.maxLines,
                                                                            columns: 2...4, maxFieldLength: userDictionaryLimits.maxFieldLength))
    guard force || elements != userDictionaryElements else { return }
    userDictionaryElements = elements
    applyDynamicDictionary()
}

/// 変換の入口で呼ぶ。2 秒に 1 回まで、ファイルの更新時刻を見る
@MainActor func refreshUserDictionaryIfChanged() {
    guard -userDictionaryCheckedAt.timeIntervalSinceNow >= 2 else { return }
    reloadUserDictionary()
}

/// 変換のあとの語の単位の直し (カタカナ・英字・表記ゆれ・連濁・絵文字など) が、登録した語 (isUserRegistered) を書き換えないように。
/// 読みの区切りが同じ直しは、登録した語の所だけ元に戻す (ほかの所の直しは残す)。区切りが変わる直し (語をまとめる・候補を選び直す) は、
/// 登録した語が同じ順でみな残るときだけ使う。直しが無い・登録した語の所だけの直しだったときは nil
func keepingUserRegisteredWords(_ best: Candidate, _ fixed: Candidate?) -> Candidate? {
    guard let fixed else { return nil }
    let registered = best.data.filter { $0.metadata.contains(.isUserRegistered) }
    guard !registered.isEmpty else { return fixed }
    if fixed.data.map(\.ruby) == best.data.map(\.ruby) {
        var data = fixed.data
        for i in data.indices where best.data[i].metadata.contains(.isUserRegistered) { data[i] = best.data[i] }
        let text = data.map(\.word).joined()
        guard text != best.text else { return nil }
        return Candidate(text: text, value: fixed.value, correspondingCount: fixed.correspondingCount, lastMid: fixed.lastMid, data: data)
    }
    var rest = registered[...]
    for element in fixed.data where rest.first.map({ $0.word == element.word && $0.ruby == element.ruby }) == true {
        rest = rest.dropFirst()
    }
    return rest.isEmpty ? fixed : nil
}

/// 文節の並びの直し (難読語の同音語・代わりの字の成句) のあとで、直す前の文にあった登録した語の表記が消えていたら、直す前に戻す
/// (文節の並びには語の印が無いので、表記で見る)
@MainActor func keepingUserRegisteredText(_ before: [(word: String, rubyCount: Int)],
                                          _ after: [(word: String, rubyCount: Int)]) -> [(word: String, rubyCount: Int)] {
    guard !appliedUserWords.isEmpty else { return after }
    let beforeText = before.map(\.word).joined(), afterText = after.map(\.word).joined()
    guard beforeText != afterText else { return after }
    let chars = Array(beforeText)
    for i in chars.indices {
        for n in appliedUserWordLengths where i + n <= chars.count {
            let word = String(chars[i..<(i + n)])
            if appliedUserWords.contains(word) && !afterText.contains(word) { return before }
        }
    }
    return after
}

/// 読み全体が登録した読みと同じ入力 (めあど だけを打って変換した) なら、登録した語 (同じ読みが複数あれば表の先の語) の候補。
/// 選んだ候補が既にその読みの登録した語なら nil (AI の変換は、文の中の短縮よみ・顔文字をモデルの知る語 (メアド・顔文字) にすることがあるので、
/// 語だけを打ったときは登録どおりに出す)
@MainActor func wholeReadingUserWord(_ hiragana: String, count: Int, chosen: Candidate?) -> Candidate? {
    guard !appliedUserWordsByRuby.isEmpty, let words = appliedUserWordsByRuby[hiraganaToKatakana(hiragana)], let word = words.first else { return nil }
    if let chosen, chosen.data.count == 1, words.contains(where: { $0.word == chosen.text }) { return nil }
    return Candidate(text: word.word, value: word.value(), correspondingCount: count, lastMid: word.mid, data: [word])
}
