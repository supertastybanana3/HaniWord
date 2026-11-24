import Foundation
import KanaKanjiConverterModule

/// 2026-11-06 (仕事 (4)): 前後の語との組み合わせの回数で、同音の字を選び直す (よく聞く薬 → よく効く薬、鑑賞に浸る → 感傷に浸る)。
/// 表 (colloc_choices.tsv。tools/colloc の build_colloc_table.py v2): 日本語版ウィキペディアと青空文庫で、同じ読みの語が同じ文脈に来る回数を数え、
/// いちばん多い表記 (10 回以上) と、その 1/10 より多く出る近い表記 (どちらも正しい書き方: 効く と 利く)。どの語にも付く文脈の語 (し・日 など、
/// 組になる読みが 1 万を超える語) は表に入れない。文脈は次の 4 つ:
///   R1 すぐ後ろの内容語 (効く|薬)・R2 すぐ後ろの格助詞＋内容語 (感傷|に|浸る)・L1 すぐ前の内容語・L2 前の内容語＋格助詞 (薬|が|効く)
/// 変換で選んだ語の並びで、語の前後がこの文脈に当たり、当たった文脈のどれも今の語を支えず (選ぶ表記でも近い表記でもない)、選ぶ表記がどれも同じで、
/// 辞書に同じ読みのその表記の語があるときだけ替える。
/// 替えない語: 学習した語 (利用者が選んだ表記)、よく打つ分野の組の語 (分野の優遇が選ぶ語。替える前の語・替えたあとの語のどちらか)、
/// 固有名詞の品詞の語 (1288〜1293。替える前・替えたあと。鳴沢村 の 鳴・長野行き → 長野由紀 にしていた)
let collocEnabled = devEnvironment["AZK_COLLOC"] != "0"
/// 開発用の切り替え: AZK_COLLOC_SINGLE=0 で 1 字の語を替えない (v1 の測りで、1 字の語は良 (膜・期・制) も悪 (語・線・城・役) も多かった)
let collocSingleCharacter = devEnvironment["AZK_COLLOC_SINGLE"] != "0"
/// 2026-11-07 (v2 の測り): 1 字の語は、辞書だけの変換でだけ替え、AI ありの変換では替えない。開発用の切り替え: AZK_COLLOC_SINGLE_AI=1 で AI ありでも替える。
/// 一般の文 3,675 の AI ありで、1 字の語も替えると 良 26・悪 14 (悪 12 が複合語の中の 1 字: 市境 → 市橋・野球盤 → 野球版・独立語 → 独立後)、替えないと 良 12・悪 2。
/// 数える Sudachi は 市境・野球盤 を 1 語のまま切るので表に 市|境 の組が無く、AI が正しく選んだ複合語の 1 字を替えていた。
/// 辞書だけの変換では、1 字の語も替えると 良 85・悪 1、替えないと 良 46・悪 0 (1 字の良が多い。辞書だけの出す 1 字はもとから違うことが多い)
let collocSingleCharacterWithAI = devEnvironment["AZK_COLLOC_SINGLE_AI"] == "1"
/// 開発用の切り替え: AZK_COLLOC_AI=0 で、AI ありの変換では表を使わない (辞書だけの変換でだけ使う)
let collocWithAI = devEnvironment["AZK_COLLOC_AI"] != "0"

struct CollocChoice: Equatable {
    var word: String
    var count: Int
    /// 近い表記 (いちばん多い表記の 1/10 より多く出る別の表記。替えない)
    var close: Set<String> = []
}

/// 2026-11-07: 表 (v2 は 93 万行・31.6MB) は読み込まずに、字のまま持って二分探索で引く (前は起動のたびに 1〜1.5 分かけて読み込み、引き当ての表にしていた。
/// メモリも 200MB 近かった)。行は 読み<TAB>文脈の種類<TAB>文脈 の字の並び (UTF-8 のバイト) の順に並んでいること (build_colloc_table.py が並べる)
/// 2026-11-10: 字は Data で持つ。配る表は、ファイルを写像した Data (CollocTableLoad) のまま引く
struct CollocTable {
    /// 表の字 (UTF-8)。0 から数える
    private(set) var data = Data()
    /// 頭の # の行を飛ばした、本文の始まり
    private(set) var bodyStart = 0
    var isEmpty: Bool { bodyStart >= data.count }

    init(bytes: [UInt8] = []) {
        self.init(data: Data(bytes))
    }

    init(data: Data) {
        // 切り出した Data は startIndex が 0 でないので、0 から数える Data にする (ファイルから作った Data は 0 から)
        self.data = data.startIndex == 0 ? data : Data(data)
        bodyStart = self.data.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) -> Int in
            var at = 0
            while at < bytes.count && bytes[at] == UInt8(ascii: "#") {
                guard let newline = bytes[at...].firstIndex(of: UInt8(ascii: "\n")) else { return bytes.count }
                at = newline + 1
            }
            return at
        }
    }

    /// 読み<TAB>文脈の種類<TAB>文脈 の行の選ぶ表記 (無ければ nil)
    func choice(_ key: String) -> CollocChoice? {
        let want = Array(key.utf8) + [UInt8(ascii: "\t")]
        return data.withUnsafeBytes { bytes -> CollocChoice? in
            var lo = bodyStart, hi = bytes.count
            while lo < hi {
                let mid = (lo + hi) / 2
                var start = mid
                while start > lo && bytes[start - 1] != UInt8(ascii: "\n") { start -= 1 }
                var end = start
                while end < bytes.count && bytes[end] != UInt8(ascii: "\n") { end += 1 }
                // 行の頭の 3 つの欄 (と次の TAB) を、探す鍵と比べる
                var tabs = 0, keyEnd = start
                while keyEnd < end && tabs < 3 { if bytes[keyEnd] == UInt8(ascii: "\t") { tabs += 1 }; keyEnd += 1 }
                let lineKey = bytes[start..<keyEnd]
                if lineKey.elementsEqual(want) {
                    return parseCollocChoice(ArraySlice(bytes[keyEnd..<end]))
                }
                if lineKey.lexicographicallyPrecedes(want) { lo = end + 1 } else { hi = start }
            }
            return nil
        }
    }
}

/// 行の後ろの 3 つの欄 (選ぶ表記<TAB>回数<TAB>近い表記)
func parseCollocChoice(_ tail: ArraySlice<UInt8>) -> CollocChoice? {
    let f = String(decoding: tail, as: UTF8.self).split(separator: "\t", omittingEmptySubsequences: false)
    guard f.count == 3, !f[0].isEmpty, let n = Int(f[1]) else { return nil }
    // 近い表記: 表記:回数 を , でつないだもの (空なら無し)
    let close = Set(f[2].split(separator: ",").compactMap { item in item.split(separator: ":").first.map(String.init) })
    return CollocChoice(word: String(f[0]), count: n, close: close)
}

/// 字の表から作る (試験と、ほかの形の表のため)。形の合わない行を捨て、鍵の順に並べ直す
func parseCollocTable(_ text: String?) -> CollocTable {
    var lines: [[UInt8]] = []
    for line in (text ?? "").split(whereSeparator: \.isNewline) where !line.hasPrefix("#") {
        let f = line.split(separator: "\t", omittingEmptySubsequences: false)
        guard f.count == 6, ["R1", "R2", "L1", "L2"].contains(f[1]), Int(f[4]) != nil,
              !f[0].isEmpty, !f[2].isEmpty, !f[3].isEmpty else { continue }
        lines.append(Array(line.utf8))
    }
    func key(_ line: [UInt8]) -> ArraySlice<UInt8> {
        var tabs = 0, at = 0
        while at < line.count && tabs < 3 { if line[at] == UInt8(ascii: "\t") { tabs += 1 }; at += 1 }
        return line[..<at]
    }
    lines.sort { key($0).lexicographicallyPrecedes(key($1)) }
    return CollocTable(bytes: Array(lines.joined(separator: [UInt8(ascii: "\n")])) + (lines.isEmpty ? [] : [UInt8(ascii: "\n")]))
}

let collocCaseParticles: Set<String> = ["を", "に", "が", "で", "と", "へ", "から", "より"]

/// 文脈にする内容語か (助詞・助動詞・記号・数字・英字でない、漢字かかなを含む語)
func isCollocContent(_ e: DicdataElement) -> Bool {
    guard !CIDData.isJoshi(cid: e.lcid), !(369...554).contains(e.lcid), e.lcid != 1317, !isNumeralOrSuffixElement(e) else { return false }
    return e.word.unicodeScalars.contains { (0x3041...0x30FA).contains($0.value) || (0x4E00...0x9FFF).contains($0.value) || $0.value == 0x3005 }
}

/// 語の並びの i 番目の語の文脈 (表の鍵の 文脈の種類<TAB>文脈 の部分)
func collocContexts(_ data: [DicdataElement], at i: Int) -> [String] {
    var out: [String] = []
    if i + 1 < data.count {
        let a = data[i + 1]
        if collocCaseParticles.contains(a.word) && CIDData.isJoshi(cid: a.lcid) {
            if i + 2 < data.count, isCollocContent(data[i + 2]) { out.append("R2\t\(a.word)|\(data[i + 2].word)") }
        } else if isCollocContent(a) {
            out.append("R1\t\(a.word)")
        }
    }
    if i >= 1 {
        let b = data[i - 1]
        if collocCaseParticles.contains(b.word) && CIDData.isJoshi(cid: b.lcid) {
            if i >= 2, isCollocContent(data[i - 2]) { out.append("L2\t\(data[i - 2].word)|\(b.word)") }
        } else if isCollocContent(b) {
            out.append("L1\t\(b.word)")
        }
    }
    return out
}

/// 2026-11-07: 辞書には、助詞と語が 1 語になった語がある (を買う/ヲカウ: 左 156 (助詞)・右 817、の関心/ノカンシン: 左 368・右 1285、上司の: 右 368)。
/// 文脈と狙いの語は、その語を助詞と語に分けた かけら で見る (かんしんをかう が 関心|を買う で、後ろの を|買う の文脈が作れていなかった)
struct CollocPiece {
    /// かけらの語 (分けたときは、助詞の側は左右とも助詞の品詞、語の側は元の語の 語の側の品詞)
    var element: DicdataElement
    /// 元の語の並びの何番目か
    var index: Int
    /// 元の語の中で、このかけらの前と後ろの字 (分けていなければ空)
    var head = ""
    var tail = ""
}

let collocSplitParticles = ["から", "より", "を", "に", "が", "で", "と", "へ", "の", "は", "も"]

func collocKatakana(_ text: String) -> String {
    String(String.UnicodeScalarView(text.unicodeScalars.map { (0x3041...0x3096).contains($0.value) ? Unicode.Scalar($0.value + 0x60)! : $0 }))
}

/// 語の並びを かけら に分ける。頭が助詞 (左の品詞が助詞) で後ろに語が続く 1 語は 助詞 | 語、終わりが助詞 (右の品詞が助詞) の 1 語は 語 | 助詞
func collocPieces(_ data: [DicdataElement]) -> [CollocPiece] {
    var out: [CollocPiece] = []
    for (i, e) in data.enumerated() {
        if CIDData.isJoshi(cid: e.lcid), !CIDData.isJoshi(cid: e.rcid),
           let p = collocSplitParticles.first(where: { e.word.hasPrefix($0) && e.word.count > $0.count && e.ruby.hasPrefix(collocKatakana($0)) }) {
            let ruby = collocKatakana(p)
            out.append(CollocPiece(element: DicdataElement(word: p, ruby: ruby, lcid: e.lcid, rcid: e.lcid, mid: e.mid, value: e.value()), index: i, tail: String(e.word.dropFirst(p.count))))
            out.append(CollocPiece(element: DicdataElement(word: String(e.word.dropFirst(p.count)), ruby: String(e.ruby.dropFirst(ruby.count)), lcid: e.rcid, rcid: e.rcid, mid: e.mid, value: e.value()),
                                   index: i, head: p))
            continue
        }
        if CIDData.isJoshi(cid: e.rcid), !CIDData.isJoshi(cid: e.lcid),
           let p = collocSplitParticles.first(where: { e.word.hasSuffix($0) && e.word.count > $0.count && e.ruby.hasSuffix(collocKatakana($0)) }) {
            let ruby = collocKatakana(p)
            out.append(CollocPiece(element: DicdataElement(word: String(e.word.dropLast(p.count)), ruby: String(e.ruby.dropLast(ruby.count)), lcid: e.lcid, rcid: e.lcid, mid: e.mid, value: e.value()),
                                   index: i, tail: p))
            out.append(CollocPiece(element: DicdataElement(word: p, ruby: ruby, lcid: e.rcid, rcid: e.rcid, mid: e.mid, value: e.value()), index: i, head: String(e.word.dropLast(p.count))))
            continue
        }
        out.append(CollocPiece(element: e, index: i))
    }
    return out
}

/// 語の並びの同音の語を、表の文脈で選ぶ表記に替えた並び。lookup は 読み・表記 → 辞書の語 (無ければ nil)、keep は替えない語 (読み・表記)。替える所が無ければ nil
/// outer: 候補の外側の文脈 (仕事 45。左の文脈の終わりの語・カーソルの右の字。right_context.swift)。外側の文脈の候補は、表にある最初のものを使う
func choosingByColloc(_ data: [DicdataElement], table: CollocTable, lookup: (String, String) -> DicdataElement?,
                      keep: (DicdataElement) -> Bool, singleCharacter: Bool = true, outer: CollocOuterContexts = CollocOuterContexts()) -> [DicdataElement]? {
    var out = data
    var changed = false
    let pieces = collocPieces(data)
    let pieceElements = pieces.map(\.element)
    for (k, piece) in pieces.enumerated() {
        let e = piece.element
        let i = piece.index
        guard e.word.unicodeScalars.contains(where: { (0x4E00...0x9FFF).contains($0.value) }),
              singleCharacter || e.word.count > 1, !isProperNounElement(e) else { continue }
        var choices = collocContexts(pieceElements, at: k).compactMap { table.choice("\(e.ruby)\t\($0)") }
        for alternatives in collocOuterKeys(outer, pieces: pieceElements, at: k) {
            if let hit = alternatives.lazy.compactMap({ table.choice("\(e.ruby)\t\($0)") }).first { choices.append(hit) }
        }
        // 当たった文脈のどれかが今の語を支える (選ぶ表記か近い表記) なら替えない
        guard !choices.isEmpty, !choices.contains(where: { $0.word == e.word || $0.close.contains(e.word) }) else { continue }
        let words = Set(choices.map(\.word))
        guard words.count == 1, let word = words.first, !keep(e), !keep(data[i]) else { continue }
        guard singleCharacter || word.count > 1,
              let better = lookup(e.ruby, word), better.word == word, better.ruby == e.ruby, !keep(better), !isProperNounElement(better) else { continue }
        if piece.head.isEmpty && piece.tail.isEmpty {
            out[i] = DicdataElement(word: word, ruby: e.ruby, lcid: better.lcid, rcid: better.rcid, mid: better.mid, value: e.value())
        } else {
            // 助詞と 1 語になった語の中の語を替えたときは、元の 1 語の形のまま (の関心 → の歓心)
            let original = data[i]
            out[i] = DicdataElement(word: piece.head + word + piece.tail, ruby: original.ruby, lcid: original.lcid, rcid: original.rcid, mid: original.mid, value: original.value())
        }
        changed = true
    }
    return changed ? out : nil
}

/// 2026-11-07 (v2 の夜の悪 2 つ): 数詞 (1295) と接尾の語 (接尾・助数詞・接尾の特殊など 1297〜1305)。文脈の語にしない (isCollocContent)。
/// 文章を数えた Sudachi は 一台・包み方 を 1 語か、数詞と接尾の語に切るので、表には 一|台・包み|方 の組が無い。
/// それを「出ていない語」とみて、ほかの組 (ダイ L1 一 → 第 11・ツツミ R1 方 → 堤 27) で替えていた (一台の楽器 → 一第の・包み方 → 堤方)。
/// 狙いの語としては外さない (v2.1 で狙いの語からも外すと、辞書だけの一般の文で 細胞膜 の 膜・対策 の 策・量・期・制 などの良が 32 減った)
func isNumeralOrSuffixElement(_ e: DicdataElement) -> Bool {
    [e.lcid, e.rcid].contains { $0 == CIDData.数.cid || (1297...1305).contains($0) }
}

/// 固有名詞の品詞の語 (固有名詞・人名・組織・地名: 1288〜1293)
func isProperNounElement(_ e: DicdataElement) -> Bool {
    (1288...1293).contains(e.lcid) || (1288...1293).contains(e.rcid)
}

/// 2026-11-10: 表はファイルを写像して持つ (.alwaysMapped)。読み込むと変換サーバーの private のメモリが表の大きさ (30MB) だけ増えていた
/// (11-09 の重さの測り。重さの目安はサーバーのメモリ 200MB)。写像は OS が引いたページだけを読み、ほかのプロセスと共有できるので private に数えない。
/// 写像のあいだ表のファイルは消せない・書き換えられない (配る表は変換サーバーを止めてから置き換える。DLL と同じ)。
/// 開発用の切り替え: AZK_MAPPED=0 で前と同じに全部を読み込む
let collocMapped = devEnvironment["AZK_MAPPED"] != "0"

/// 表 (数十万行) は読むのに時間がかかるので、Initialize で別のスレッドに読ませ、はじめて使うときに読み終わるのを待つ (hyouki.swift と同じ)
final class CollocTableLoad: @unchecked Sendable {
    private let done = DispatchSemaphore(value: 0)
    private var table = CollocTable()

    init(url: URL, mapped: Bool = collocMapped) {
        DispatchQueue.global(qos: .utility).async { [self] in
            // 字のまま持つ (並べ直さない。表づくりの道具が鍵の順に並べてある)
            if let attributes = try? FileManager.default.attributesOfItem(atPath: url.path), let size = attributes[.size] as? Int, size <= 64 << 20,
               let data = try? Data(contentsOf: url, options: mapped ? [.alwaysMapped] : []) {
                table = CollocTable(data: data)
            }
            done.signal()
        }
    }

    func wait() -> CollocTable {
        done.wait()
        done.signal()
        return table
    }
}

@MainActor var collocTableLoad: CollocTableLoad? = nil
@MainActor var collocTableCache: CollocTable? = nil

/// 開発用のビルドでは AZK_COLLOC_TABLE で別の表を読める
@MainActor func startLoadingCollocTable() {
    guard collocEnabled, collocTableLoad == nil, collocTableCache == nil else { return }
    let url = devEnvironment["AZK_COLLOC_TABLE"].map { URL(fileURLWithPath: $0) } ?? execURL.appendingPathComponent("colloc_choices.tsv")
    collocTableLoad = CollocTableLoad(url: url)
}

@MainActor func collocTable() -> CollocTable {
    if let loaded = collocTableCache { return loaded }
    startLoadingCollocTable()
    let loaded = collocTableLoad?.wait() ?? CollocTable()
    collocTableCache = loaded
    collocTableLoad = nil
    return loaded
}

/// 読み (カタカナ)・表記 → 辞書だけの変換で読み全体が 1 語になるその表記の語 (変換の間は覚えておく)
@MainActor var collocWordMemo: [String: DicdataElement?] = [:]

@MainActor func collocDictionaryWord(ruby: String, word: String) -> DicdataElement? {
    let key = "\(ruby)\t\(word)"
    if let hit = collocWordMemo[key] { return hit }
    let reading = String(String.UnicodeScalarView(ruby.unicodeScalars.map { scalar in
        (0x30A1...0x30F6).contains(scalar.value) ? Unicode.Scalar(scalar.value - 0x60)! : scalar
    }))
    let composing = directComposingText(reading)
    let count = composing.input.count
    let found = converter.requestCandidates(composing, options: getOptions(context: "", prediction: false, zenzai: false, nBest: 50)).mainResults
        .first { $0.correspondingCount == count && $0.data.count == 1 && $0.data[0].word == word && $0.data[0].ruby == ruby }?.data.first
    if collocWordMemo.count >= 512 { collocWordMemo.removeAll() }
    collocWordMemo[key] = found
    return found
}

/// 変換で選んだ候補に choosingByColloc をかけた候補 (替える所が無ければ nil)
/// left: 左の文脈、right: カーソルの右の字 (仕事 45。設定 input.right_context がオンのときだけ外側の文脈に使う)
@MainActor func collocChosen(_ candidate: Candidate, dictionaryOnly: Bool, left: String = "", right: String = "") -> Candidate? {
    guard collocEnabled, dictionaryOnly || collocWithAI else { return nil }
    let table = collocTable()
    guard !table.isEmpty else { return nil }
    let focused = domainBoosts.filter(\.focused)
    let keep: (DicdataElement) -> Bool = { e in
        e.metadata.contains(.isLearned) || focused.contains { $0.pairs.contains(e.ruby + "\t" + e.word) }
    }
    // 2026-11-08 (仕事 45): 外側の文脈 (左の文脈の終わりの語・カーソルの右の字) は、辞書だけの変換 (仮表示・AI を切った人・清書が切れたとき) でだけ使う。
    // 左はいつも、右は設定 input.right_context がオンのときだけ。AI ありの変換では使わない (dev・test の 15,712 件で、AI ありの欄の左だけは 良 8・悪 1
    // (新しい|菓子 → 歌詞)、左右は dev で 良 9・悪 2 と小さく、AI がもう正しく選んでいた所を替えることがあるため)。
    // 開発用の切り替え: AZK_OUTER_AI=1 で AI ありでも使う、AZK_OUTER_LEFT=0・AZK_OUTER_RIGHT=0 で片方を使わない
    let useOuter = dictionaryOnly || devEnvironment["AZK_OUTER_AI"] == "1"
    let outer = useOuter
        ? collocOuterContexts(left: left, right: right, useLeft: devEnvironment["AZK_OUTER_LEFT"] != "0",
                              useRight: rightContextEnabled() && devEnvironment["AZK_OUTER_RIGHT"] != "0")
        : CollocOuterContexts()
    guard let data = choosingByColloc(candidate.data, table: table, lookup: { collocDictionaryWord(ruby: $0, word: $1) }, keep: keep,
                                      singleCharacter: collocSingleCharacter && (dictionaryOnly || collocSingleCharacterWithAI), outer: outer) else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
                     lastMid: candidate.lastMid, data: data)
}
