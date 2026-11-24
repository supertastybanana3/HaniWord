import Foundation
import KanaKanjiConverterModule

/// 2026-11-08 (仕事 49): 辞書だけの変換 (打っている途中の仮表示・AI を切った設定・AI を休んでいる間) で、左の文脈の終わりの語の品詞からつなぐ。
/// 今まで辞書だけの変換は読みの頭をいつも文頭 (BOS) からつないでいたので、確定した直後に助詞・助動詞・送り仮名から打つと、文の頭に来ない語が
/// 名詞に負けていた (て→手・た→多・れ→レ・な→名。文の途中の助詞・助動詞を 1 つ打つ組 generalF dev で 18% 間違い)。
/// 格子の始点を、文頭から「左の語の右の品詞 (rcid)」に替える (KanaKanjiConverter.setStartRcid)。
/// 左の語:
///   (A) 直前に確定した文節の最後の語。LearnSegments で受け、読みと表記で辞書だけの候補を引き直して、表記の一致する候補の最後の語を覚える
///       (学習の learnOne と同じ引き方。メモリに 1 つ持つだけで書き出さない)。左の文脈がその表記で終わっているときだけ使う。
///       学習を切った設定・個人のデータを許さない設定では LearnSegments が呼ばれないので (B) だけ (まとめ役 18:48 の決め)
///   (B) (A) が無い・合わないとき: 左の文脈の終わりのひらがなの並び (6 字まで) を辞書だけで変換し、かなのままの候補の最後の語。
///       終わりが 、， ならその記号の語。。！？ と改行のあとは文頭のまま
/// 絞り方 (leftWordGate): 左の語から始めた結果の最初の語が
///   - 文の頭に来ない分け (isNonStarterCID: 助詞・助動詞・接尾・非自立) → 左の語からの結果を使う
///   - 文頭からのつながりの点が -6 より良い (名詞など、文の頭に来やすい語) → 文頭から変換し直す (並ぶ→奈良部・以下の→イカの を出さない)
///   - それ以外 (動詞・形容詞など) → 文頭から変換した結果の最初の語と読みも大きな品詞も同じなら、字の選び分けは語そのものの点
///     (つながりを含まない値。よく使う字) の良いほうで決める。左の語は品詞と区切りを決めるのに使い、同じ読み・同じ品詞の中の字は選ばない
///     (と言う → と謂う を出さず、を行う は をおこなう にしない。まとめ役 19:38 の決め、samepos (文頭からの結果にする形) と比べて
///     といった → と言った の 3 つが残るかわりに、を行う の 16 を直したまま。dev の文節ごと 一般・目隠し、general45)
/// 持つもの (どちらもメモリだけ。ファイルには書かない):
///   - lastCommittedLeft: 最後に確定した文節 1 つの表記と、その最後の語の品詞の番号 (rcid)。次の確定で置き換わる。学習がオンで
///     個人のデータを許す接続のときだけ覚え・使う。個人のデータの接続を切り替えたとき (SetPersonalDataAllowed) に消える
///   - leftTailRcidMemo: 左の文脈の終わりのひらがなの並び (6 字まで) か 、， → その品詞の番号。512 まで (超えたら全部捨てる)。
///     学習・設定・接続が変わったとき (clearPreviewMemo) に消える。個人のデータを許さない接続では覚えない (そのつど引く)
/// 止める口は試験用のビルドだけ (security review #5「配る版は AZK_* を読まない」に合わせた。まとめ役 19:47): AZK_LEFT_POS=0 で使わない、
/// AZK_LEFT_POS=tail で (B) だけ (学習を切った設定の形を測るため)。配る版はいつも both
let leftWordMode = leftWordModeOf(devEnvironment)

func leftWordModeOf(_ environment: [String: String]) -> String {
    switch environment["AZK_LEFT_POS"] {
    case "0": return "0"
    case "tail": return "tail"
    default: return "both"
    }
}

/// 文頭からのつながりの点がこれより良い語は、文の頭に来やすい語として、文頭から変換し直す
let leftWordStarterConnection: PValue = -6

/// 直前に確定した文節の表記と、その最後の語の rcid
@MainActor var lastCommittedLeft: (text: String, rcid: Int)? = nil

/// かなの並び・記号 → その表記のままの候補の最後の語の rcid (学習・設定が変わったら clearPreviewMemo で捨てる)
@MainActor var leftTailRcidMemo: [String: Int?] = [:]
let leftTailRcidMemoLimit = 512

/// 文の頭に来ない語の品詞の番号 (番号は IPADIC の left-id.def と同じ並び。11-08 に left-id.def を開いて確かめた):
/// 形容詞の接尾・非自立 53〜146、助詞 147〜368、助動詞 369〜554、動詞の接尾・非自立 868〜1280、名詞の接尾 1297〜1305、名詞の特殊・非自立 1309〜1313
func isNonStarterCID(_ cid: Int) -> Bool {
    (53...554).contains(cid) || (868...1280).contains(cid) || (1297...1305).contains(cid) || (1309...1313).contains(cid)
}

/// 品詞の大きな分け (left-id.def の 1 つ目の欄の並び): 記号 4〜10・形容詞 11〜146・助詞 147〜368・助動詞 369〜554・接続詞 555〜556・
/// 接頭詞 557〜560・動詞 561〜1280・副詞 1281〜1282・名詞 1283〜1314。それ以外は番号そのもの
func majorPOS(_ cid: Int) -> Int {
    for (k, r) in [4...10, 11...146, 147...368, 369...554, 555...556, 557...560, 561...1280, 1281...1282, 1283...1314].enumerated() where r.contains(cid) {
        return -1 - k
    }
    return cid
}

/// 左の文脈の終わりの語の見積もりに使う部分。nil なら文頭のまま (左が空・。！？・改行のあと・ひらがなでも 、， でもない)
func leftTailKey(_ context: String) -> String? {
    guard let last = context.last else { return nil }
    if "、，,".contains(last) { return String(last) }
    let isHiragana: (Character) -> Bool = { $0.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) || $0.value == 0x30FC } }
    guard isHiragana(last) else { return nil }
    return String(context.reversed().prefix(while: isHiragana).prefix(6).reversed())
}

/// 絞り方の決め。left は左の語から始めた結果の最初の語 (lcid・読み・語そのものの点) と、その文頭からのつながりの点。bos は文頭から変換した
/// 結果の最初の語 (要るときだけ呼ばれる)。true なら左の語からの結果を使う
func leftWordGate(leftFirst: (lcid: Int, ruby: String, value: PValue), starterConnection: PValue,
                  bosFirst: () -> (lcid: Int, ruby: String, value: PValue)?) -> Bool {
    if isNonStarterCID(leftFirst.lcid) { return true }
    if starterConnection > leftWordStarterConnection { return false }
    guard let b = bosFirst() else { return true }
    if b.ruby == leftFirst.ruby && majorPOS(b.lcid) == majorPOS(leftFirst.lcid) {
        return leftFirst.value >= b.value
    }
    return true
}

/// 読みを辞書だけで変換し、表記が text の候補の最後の語の rcid (learnOne と同じ引き方)。無ければ nil
@MainActor func committedRcid(reading: String, text: String) -> Int? {
    let composing = directComposingText(reading)
    let count = composing.input.count
    converter.setStartRcid(nil)
    let converted = converter.requestCandidates(composing, options: getOptions(prediction: false, zenzai: false, nBest: 30))
    return converted.mainResults.first(where: { $0.correspondingCount == count && $0.text == text })?.data.last?.rcid
}

/// 確定した文節を使ってよいか: 学習がオンで、個人のデータを許す接続のとき
@MainActor func committedLeftAllowed() -> Bool {
    leftWordMode == "both" && personalDataAllowed && (config["learning"] as? Bool ?? true)
}

/// 確定した文節 (最後の 1 つ) を覚える (LearnSegments から)
@MainActor func noteCommittedLeft(reading: String, text: String) {
    lastCommittedLeft = nil
    guard committedLeftAllowed() else { return }
    let reading = normalizeForConversion(reading)
    guard !reading.isEmpty, !text.isEmpty, let rcid = committedRcid(reading: reading, text: text) else { return }
    lastCommittedLeft = (text: text, rcid: rcid)
}

/// 左の文脈の終わりの見積もりを覚える (個人のデータを許さない接続では覚えない)
@MainActor func rememberLeftTail(_ key: String, _ rcid: Int?) {
    guard personalDataAllowed else { return }
    if leftTailRcidMemo.count >= leftTailRcidMemoLimit { leftTailRcidMemo.removeAll(keepingCapacity: true) }
    leftTailRcidMemo[key] = rcid
}

/// 覚えたものを全部消す (個人のデータの接続を切り替えたとき)
@MainActor func forgetLeftWord() {
    lastCommittedLeft = nil
    leftTailRcidMemo.removeAll(keepingCapacity: true)
    leftWordDecisionMemo.removeAll(keepingCapacity: true)
}

@MainActor func leftTailRcid(_ key: String) -> Int? {
    if personalDataAllowed, let hit = leftTailRcidMemo[key] { return hit }
    let rcid = committedRcid(reading: key, text: key)
    rememberLeftTail(key, rcid)
    return rcid
}

/// 確定した文節が左の文脈の終わりに合えば、その最後の語の rcid
@MainActor func committedLeftRcid(context: String) -> Int? {
    guard committedLeftAllowed(), let h = lastCommittedLeft, !h.text.isEmpty, context.hasSuffix(h.text) else { return nil }
    return h.rcid
}

/// 左の文脈 → 格子の始点の rcid。nil なら文頭のまま
@MainActor func leftStartRcid(context: String, mode: String = leftWordMode) -> Int? {
    guard mode != "0", !context.isEmpty else { return nil }
    // 確定した文節は、学習がオンで個人のデータを許す接続のときだけ使う (切り替えたあとに前の覚えを使わない)
    if let rcid = committedLeftRcid(context: context) {
        return rcid
    }
    return leftTailKey(context).flatMap { leftTailRcid($0) }
}

/// 試しの測り用の数 (始点を左の語にした変換の数・文頭から変換し直した数・決めの覚えで左の語からの変換をしなかった数)。LeftWordStatsForTest で読む
@MainActor var leftWordStats = (started: 0, retried: 0, skipped: 0)

/// 2026-11-19 (仕事 49 の速さ): 決めの覚え。(始点・左の文脈の終わり・文頭からの結果の最初の語の読みと品詞) → 左の語からの結果を使うか。
/// (鍵に左の文脈の終わり 16 字を含むので、個人のデータを許す接続のときだけ覚え・使う。接続を切り替えたら forgetLeftWord で消す)
/// 前は打ち足すたびに同じ最初の語で決め直し、キーの 83% で変換を 2 回していた (1 キーの時間の p90 が 1.8〜2 倍)。今は文頭からの変換を先にし、
/// 最初の語が同じで前に「文頭からの結果」に決めていれば、左の語からの変換をしない。学習・設定が変わったら clearPreviewMemo で捨てる。
/// 試験用のビルドだけ AZK_LEFT_POS_MEMO=0 で前の形 (左の語から先に変換し、毎回決める)
@MainActor var leftWordDecisionMemo: [String: Bool] = [:]
let leftWordDecisionMemoLimit = 256
let leftWordDecisionMemoEnabled = devEnvironment["AZK_LEFT_POS_MEMO"] != "0"

@MainActor func rememberLeftWordDecision(_ key: String, _ useLeft: Bool) {
    // 鍵に左の文脈の字を含むので、個人のデータを許さない接続では覚えない (メモリだけで、ファイルには書かない)
    guard personalDataAllowed else { return }
    if leftWordDecisionMemo.count >= leftWordDecisionMemoLimit { leftWordDecisionMemo.removeAll(keepingCapacity: true) }
    leftWordDecisionMemo[key] = useLeft
}

/// 辞書だけの変換の、左の語からつなぐ変換。始点を替えて変換し、絞り方で文頭からの結果に替えるときは文頭から変換し直す
@MainActor func requestCandidatesFromLeftWord(_ composing: ComposingText, count: Int, context: String, options: () -> ConvertRequestOptions) -> ConversionResult {
    let startRcid = leftStartRcid(context: context)
    if leftWordDecisionMemoEnabled {
        return requestCandidatesFromLeftWordMemo(composing, count: count, context: context, startRcid: startRcid, options: options)
    }
    converter.setStartRcid(startRcid)
    let converted = converter.requestCandidates(composing, options: options())
    converter.setStartRcid(nil)
    guard startRcid != nil, let first = converted.mainResults.first(where: { $0.correspondingCount == count })?.data.first else { return converted }
    leftWordStats.started += 1
    var bos: ConversionResult? = nil
    let useLeft = leftWordGate(leftFirst: (first.lcid, first.ruby, first.value()), starterConnection: converter.connectionValue(CIDData.BOS.cid, first.lcid)) {
        bos = converter.requestCandidates(composing, options: options())
        return bos?.mainResults.first(where: { $0.correspondingCount == count })?.data.first.map { ($0.lcid, $0.ruby, $0.value()) }
    }
    if useLeft { return converted }
    leftWordStats.retried += 1
    return bos ?? converter.requestCandidates(composing, options: options())
}

/// 2026-11-19: 決めの覚えを使う形 (文頭からの変換を先にする)
@MainActor func requestCandidatesFromLeftWordMemo(_ composing: ComposingText, count: Int, context: String, startRcid: Int?,
                                                  options: () -> ConvertRequestOptions) -> ConversionResult {
    converter.setStartRcid(nil)
    let bos = converter.requestCandidates(composing, options: options())
    guard let startRcid, let bosFirst = bos.mainResults.first(where: { $0.correspondingCount == count })?.data.first else { return bos }
    let key = "\(startRcid)\u{1}\(context.suffix(16))\u{1}\(bosFirst.ruby)\u{1}\(bosFirst.lcid)"
    if personalDataAllowed, leftWordDecisionMemo[key] == false {
        leftWordStats.skipped += 1
        return bos
    }
    converter.setStartRcid(startRcid)
    let converted = converter.requestCandidates(composing, options: options())
    converter.setStartRcid(nil)
    guard let first = converted.mainResults.first(where: { $0.correspondingCount == count })?.data.first else { return bos }
    leftWordStats.started += 1
    let useLeft = leftWordGate(leftFirst: (first.lcid, first.ruby, first.value()), starterConnection: converter.connectionValue(CIDData.BOS.cid, first.lcid)) {
        (bosFirst.lcid, bosFirst.ruby, bosFirst.value())
    }
    rememberLeftWordDecision(key, useLeft)
    if useLeft { return converted }
    leftWordStats.retried += 1
    return bos
}

#if AZK_DEV_KNOBS
/// 試しの測り用 (リポジトリの外の測りの台本): 直前に確定した文節 (読み・表記) を覚える。戻り値はその最後の語の rcid (引けなければ -1)
@_silgen_name("NoteCommittedForTest")
@MainActor public func note_committed_for_test(readingPtr: UnsafePointer<CChar>, textPtr: UnsafePointer<CChar>) -> Int32 {
    noteCommittedLeft(reading: String(cString: readingPtr), text: String(cString: textPtr))
    return Int32(lastCommittedLeft?.rcid ?? -1)
}

/// 試しの測り用 (リポジトリの外の測りの台本): 始点を左の語にした変換の数 × 100000 + 文頭から変換し直した数。reset が真なら 0 に戻す。
/// 2026-11-19: 決めの覚えで左の語からの変換をしなかった数は LeftWordSkippedForTest
@_silgen_name("LeftWordStatsForTest")
@MainActor public func left_word_stats_for_test(reset: Bool) -> Int64 {
    let v = Int64(leftWordStats.started) * 100000 + Int64(leftWordStats.retried)
    if reset { leftWordStats = (0, 0, 0) }
    return v
}

@_silgen_name("LeftWordSkippedForTest")
@MainActor public func left_word_skipped_for_test() -> Int64 {
    Int64(leftWordStats.skipped)
}

/// 試しの測り用: 左の文脈に対して、辞書だけの変換が始点に使う rcid (文頭なら -1)
@_silgen_name("LeftStartRcidForTest")
@MainActor public func left_start_rcid_for_test(contextPtr: UnsafePointer<CChar>) -> Int32 {
    Int32(leftStartRcid(context: leftContext(String(cString: contextPtr))) ?? -1)
}
#endif
