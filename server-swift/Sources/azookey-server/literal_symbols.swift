import Foundation

func isLiteralSymbol(_ character: Character) -> Bool {
    // Inspect the base scalar, retaining variation selectors and emoji sequences
    // as one Character. Kana iteration/prolongation marks are letters, not symbols.
    guard let scalar = character.unicodeScalars.first else { return false }
    switch scalar.properties.generalCategory {
    case .mathSymbol, .currencySymbol, .modifierSymbol, .otherSymbol,
         .connectorPunctuation, .dashPunctuation, .openPunctuation, .closePunctuation,
         .initialPunctuation, .finalPunctuation, .otherPunctuation:
        return true
    default:
        return false
    }
}

// Explicit symbols bypass learned/AI alternatives; kana readings of symbol names
// still convert normally, and manually selected alternatives remain available.
/// 2026-11-05 (仕事 22 の 2、まとめ役 15:26): digitSlashContext なら、数字の並び (/ が 3 つ以上: 第8/9/10/11) の中の / は、その後ろの部分を
/// 変換するときの左の文脈にだけ ・ を渡す (出す字は / のまま。決め直しは slash_dot.swift)。/ を文脈に渡すと、AI が後ろの助数詞を取り違えた
/// (第8/9/…/15だい → 15大韓国。・ なら 15代)。/ が 1 つ・2 つの並び (1/5と・1/3くらい・2024/11/05) は / のまま渡す
/// (AI ありの 300 文で、/ が 1 つの所まで ・ にすると、/ が正しい所の後ろの変換が 15 文変わり 良 1・悪 5 だった)
/// 数字＋/＋数字 の並び (1/2/3) の中の / の数。chars[index] が数字にはさまれた / でなければ 0
func digitSlashChainLength(_ chars: [Character], at index: Int) -> Int {
    let digit: (Character) -> Bool = { $0.isASCII && $0.isNumber }
    guard chars[index] == "/", index > 0, index + 1 < chars.count, digit(chars[index - 1]), digit(chars[index + 1]) else { return 0 }
    var start = index, end = index
    while start > 0 && (digit(chars[start - 1]) || (chars[start - 1] == "/" && start >= 2 && digit(chars[start - 2]))) { start -= 1 }
    while end + 1 < chars.count && (digit(chars[end + 1]) || (chars[end + 1] == "/" && end + 2 < chars.count && digit(chars[end + 2]))) { end += 1 }
    return chars[start...end].filter { $0 == "/" }.count
}

/// 2026-11-23 (読みを記号で分けない): 読点と括弧。splitOrdinarySymbols が偽なら、ここでは読みを分けずに、前後と一緒に convert に渡す
/// (英数字にはさまれた所 (1,000・f(x)) は今までどおり分ける)。文の終わりの記号 (。！？) や ・ / ほかの記号は今までどおり分ける
/// 2026-11-24 (a/longcomma): 長い読みで部分を終える読点
let longSplitCommas: Set<Character> = ["、", "，"]
let joinableLiteralSymbols: Set<Character> = ["、", "，", ",", "「", "」", "『", "』", "（", "）", "(", ")", "【", "】", "［", "］", "[", "]",
                                              "〔", "〕", "〈", "〉", "《", "》", "“", "”", "‘", "’"]

/// 2026-11-06 (問いの か): attachQuestionKa なら、「(1 字以上)か」のすぐ後ろの ？ ? (続けて打った印も) は切らずに、その前の部分と一緒に convert に渡す
/// (渡した先の変換が、？ で切った形と ？ ごとの形を比べる。question_ka.swift)
/// 2026-11-23: splitOrdinarySymbols が偽なら、読点と括弧 (joinableLiteralSymbols) では分けない。dev 一般 1,000 文 (AI あり、文脈なし) で、
/// 記号で部分に分けると記号より右の文脈が AI に渡らず (いご、→ 伊語、・こうりつは、→ 公立は、・「とうばく」→「討幕」の取り違え)、
/// 全部の記号で分けない形で 厳しい 688 → 714 だった。記号だけの部分 (「」だけ) は変換に渡さず、そのまま出す
func preservingLiteralSymbols(
    reading: String,
    context: String,
    digitSlashContext: Bool = false,
    attachQuestionKa: Bool = false,
    splitOrdinarySymbols: Bool = true,
    /// 2026-11-23 (2 つ目): splitOrdinarySymbols が偽のとき、読点と括弧に加えて分けない記号 (AI ありの道で 。．！？・。呼ぶ側が決める)
    extraJoinable: Set<Character> = [],
    /// 2026-11-24 (a/longcomma): 真なら、、 ， のすぐ後ろで部分を終える (読点は前の部分に入れて一緒に変換し、左の出力を次の部分の左の文脈に渡す)。
    /// 英数字にはさまれた所 (1,000) は今までどおり。呼ぶ側が長い読みの AI ありの道でだけ真にする (longReadingSplit)
    splitCommas: Bool = false,
    convert: (String, String) -> [(word: String, rubyCount: Int)],
    nameBeforeDot: ((String, String, Bool, String) -> String?)? = nil,
    /// 2026-11-19: 部分を変換する前に、その部分のすぐ後ろの字 (読みの終わりなら nil) を知らせる (終助詞の直しを文の終わりの前だけにするため)
    partFollower: ((Character?) -> Void)? = nil
) -> [(word: String, rubyCount: Int)]? {
    guard reading.contains(where: isLiteralSymbol) else { return nil }
    var result: [(word: String, rubyCount: Int)] = []
    // 2026-11-17: 部分ごとの読みと、result の中の位置 (中黒の左の名を見るため。nameBeforeDot)
    var partSpans: [(reading: String, range: Range<Int>)] = []
    // 2026-11-23: 部分が読み全体と同じになった (分ける所が 1 つも無かった) ときの印。変換の口に渡さず、最後に nil を返す
    // (渡すと変換の口がまたここへ来て終わらない。か？ の ？ を前につなぐ所からも来た: 「それはほんとうか？」 で、「 を分けないと部分が読み全体になった)
    var wholeReading = false
    func convertPart(_ text: String, _ left: String, _ follower: Character?) -> [(word: String, rubyCount: Int)] {
        // 2026-11-23: 記号だけの部分 (分けずに残った 「」 など) は変換に渡さず、1 字ずつそのまま
        if text.allSatisfy(isLiteralSymbol) {
            return text.map { (word: String($0), rubyCount: 1) }
        }
        if text == reading {
            wholeReading = true
            return []
        }
        partFollower?(follower)
        let converted = convert(text, left)
        partSpans.append((text, result.count..<(result.count + converted.count)))
        return converted
    }
    var pending = ""
    var left = context
    let chars = Array(reading)
    var attaching = false
    var skip = 0
    for (index, character) in chars.enumerated() {
        if skip > 0 { skip -= 1; continue }
        // 2026-11-06 (まとめ役 09:57): 同じ点 (。 ・ .) が 3 つ以上続く所は 1 つの文節にまとめる (一覧の 2 番目に … を出すため。ellipsis.swift)
        if ellipsisDots.contains(character) {
            var end = index + 1
            while end < chars.count && chars[end] == character { end += 1 }
            if end - index >= 3 {
                if !pending.isEmpty {
                    let parts = convertPart(pending, left, "…")
                    result += parts
                    left += parts.map(\.word).joined()
                    pending = ""
                }
                let run = String(chars[index..<end])
                result.append((word: run, rubyCount: end - index))
                left += run
                skip = end - index - 1
                continue
            }
        }
        // 2026-11-23: ？ を分けない (extraJoinable) ときは、か？ で部分を終えない (下で前後と一緒に変換する)
        if attachQuestionKa && questionMarks.contains(character) && !(!splitOrdinarySymbols && extraJoinable.contains(character))
            && (attaching || (pending.count >= 2 && pending.last == "か")) {
            pending.append(character)
            attaching = true
            if index + 1 < chars.count && questionMarks.contains(chars[index + 1]) { continue }
            let parts = convertPart(pending, left, index + 1 < chars.count ? chars[index + 1] : nil)
            result += parts
            left += parts.map(\.word).joined()
            pending = ""
            attaching = false
            continue
        }
        // 2026-11-24 (a/longcomma): 長い読みでは、読点を前の部分に入れてそこで部分を終える (読点ごとに確定するのと同じ形)
        if splitCommas && !splitOrdinarySymbols && longSplitCommas.contains(character) {
            let alnumAround = index > 0 && index + 1 < chars.count && chars[index - 1].isASCII && (chars[index - 1].isLetter || chars[index - 1].isNumber)
                && chars[index + 1].isASCII && (chars[index + 1].isLetter || chars[index + 1].isNumber)
            if !alnumAround {
                pending.append(character)
                let parts = convertPart(pending, left, index + 1 < chars.count ? chars[index + 1] : nil)
                result += parts
                left += parts.map(\.word).joined()
                pending = ""
                continue
            }
        }
        // 2026-11-23: 読点と括弧は、英数字にはさまれていなければ分けない (前後と一緒に変換する)
        // 2026-11-23 (a/dotext): AI の道だけでつなげる記号 (。．！？・) は、すぐ後ろが英数字なら今までどおり分ける (0366345 と同じ分け方)。
        // かなの後ろの 。 + 小文字の拡張子 を . に決め直す決まり (slash_dot.swift の ②) は、。 だけの文節を前提にしていて、つなげると
        // 資料。|pdf のまま残っていた (資料.pdf にならない)。。 + 大文字で始まる英字・数字 も分ける (天気です。|Googleで)
        if !splitOrdinarySymbols && (joinableLiteralSymbols.contains(character) || extraJoinable.contains(character)) {
            let alnum: (Int) -> Bool = { i in i >= 0 && i < chars.count && chars[i].isASCII && (chars[i].isLetter || chars[i].isNumber) }
            let extraBeforeAlnum = !joinableLiteralSymbols.contains(character) && extraJoinable.contains(character) && alnum(index + 1)
            if !(alnum(index - 1) && alnum(index + 1)) && !extraBeforeAlnum {
                pending.append(character)
                continue
            }
        }
        if isLiteralSymbol(character) {
            if !pending.isEmpty {
                let parts = convertPart(pending, left, character)
                result += parts
                left += parts.map(\.word).joined()
                pending = ""
            }
            let literal = String(character)
            result.append((word: literal, rubyCount: 1))
            left += digitSlashContext && character == "/" && digitSlashChainLength(chars, at: index) >= 3 ? "・" : literal
        } else {
            pending.append(character)
        }
    }
    // 2026-11-23: 分ける所が 1 つも無かった (読点と括弧しか記号が無い) ときは、何もしない (nil。呼ぶ側がふつうに変換する)。
    // 読み全体を 1 つの部分として convert に渡すと、convert がまたここへ来て終わらない (スタックがあふれて落ちた)。記号だけの読みは今までどおりそのまま
    if !pending.isEmpty { result += convertPart(pending, left, nil) }
    if wholeReading {
        return reading.allSatisfy(isLiteralSymbol) ? reading.map { (word: String($0), rubyCount: 1) } : nil
    }
    // 2026-11-23 (2 つ目): ・ を分けずに変換すると、文節の頭に ・ がつく (以前|・以後に)。頭の ・ は、それだけの文節にする (以前|・|以後に)
    if !splitOrdinarySymbols && extraJoinable.contains("・") {
        result = result.flatMap { segment -> [(word: String, rubyCount: Int)] in
            guard segment.word.count > 1, segment.rubyCount > 1, segment.word.first == "・" else { return [segment] }
            return [(word: "・", rubyCount: 1), (word: String(segment.word.dropFirst()), rubyCount: segment.rubyCount - 1)]
        }
    }
    // 2026-11-17: 「・」のすぐ右の部分の頭の語がカタカナなら、左の部分の後ろの語の並び (助詞の形の語の手前まで) を nameBeforeDot に渡し、
    // カタカナが返れば替える (とむ・ヒドルストン → トム・ヒドルストン。後ろの部分から替えて、前の位置をずらさない)。
    // 2026-11-18: 右の頭の語は、カタカナだけの語か、頭がカタカナ 2 字以上の語 (レジーム期・ユーラシア大陸)。どちらかは右の語を渡して nameBeforeDot が決める
    if let nameBeforeDot {
        let particleLike: Set<String> = ["は", "が", "を", "に", "で", "と", "も", "へ", "の", "や", "か", "から", "まで", "より"]
        func isKatakana(_ c: Unicode.Scalar) -> Bool { (0x30A1...0x30FA).contains(c.value) || c.value == 0x30FC }
        func isKatakanaWord(_ w: String) -> Bool { !w.isEmpty && w.unicodeScalars.allSatisfy(isKatakana) }
        func startsWithKatakana(_ w: String) -> Bool { isKatakanaWord(w) || w.unicodeScalars.prefix(while: isKatakana).count >= 2 }
        for (k, part) in partSpans.enumerated().reversed() {
            let dot = part.range.upperBound
            guard dot + 1 < result.count, result[dot].word == "・", k + 1 < partSpans.count, partSpans[k + 1].range.lowerBound == dot + 1,
                  startsWithKatakana(result[dot + 1].word), !part.range.isEmpty else { continue }
            let right = result[dot + 1].word
            // 左の部分全体 (固有名詞の辞書の語としてだけ見る) を先に、だめなら助詞の形の語より後ろの並び (辞書に無いひらがなも)
            var start = part.range.upperBound
            while start > part.range.lowerBound, !particleLike.contains(result[start - 1].word), !isKatakanaWord(result[start - 1].word) { start -= 1 }
            for (from, unknown) in [(part.range.lowerBound, false), (start, true)] where from < part.range.upperBound {
                let rubyCount = result[from..<part.range.upperBound].map { $0.rubyCount }.reduce(0, +)
                guard rubyCount <= part.reading.count else { continue }
                let spanReading = String(part.reading.suffix(rubyCount))
                let chosen = result[from..<part.range.upperBound].map { $0.word }.joined()
                if let katakana = nameBeforeDot(spanReading, chosen, unknown, right) {
                    result.replaceSubrange(from..<part.range.upperBound, with: [(word: katakana, rubyCount: rubyCount)])
                    break
                }
            }
        }
    }
    return result
}

/// 2026-10-21: 打った英字 (読みに入っている英字) の大文字小文字は、変換の結果でも変えない (AI の変換が たて1m62 を 縦1M62 にしていた)。
/// IME 本体が表記直し (github → GitHub) を済ませてから読みに入れるので、ここでは変えない。結果の英字の並びが、大文字小文字を除いて読みと同じときだけ
func preservingTypedLatinCase(reading: String, segments: [(word: String, rubyCount: Int)]) -> [(word: String, rubyCount: Int)] {
    let typed = Array(reading.filter { $0.isASCII && $0.isLetter })
    let output = String(segments.map(\.word).joined().filter { $0.isASCII && $0.isLetter })
    guard !typed.isEmpty, output != String(typed), output.lowercased() == String(typed).lowercased() else { return segments }
    var index = 0
    return segments.map { segment in
        (word: String(segment.word.map { c -> Character in
            guard c.isASCII && c.isLetter else { return c }
            defer { index += 1 }
            return typed[index]
        }), rubyCount: segment.rubyCount)
    }
}

// MARK: - 2026-11-23 (a/symseg): 前後と一緒に変換した記号を、記号だけの文節に分け直す

/// 開発用の切り替え: AZK_SYMBOL_SEGMENTS=0 で分け直さない (a/nosplit の形のまま)
/// (括弧の中のかな 1 字を 1 字だけで変換し直す形も試したが、0.1.0 と同じく 「し」(死)・「不」(ふ) にしたので入れていない。dev 一般 1,000 で悪 1)
let symbolSegmentSplitEnabled = devEnvironment["AZK_SYMBOL_SEGMENTS"] != "0"

/// 文節を分け直す所の記号: 約物 (句読点・括弧・中黒・ダッシュなど)。絵文字・数式の記号・通貨の記号は見ない
/// (表記に絵文字の語 (ございます😭) や覚えた記号の語 (かっこ → ()) が入っていても、読みの約物の数と比べられるように)
func isPunctuationSymbol(_ character: Character) -> Bool {
    guard let scalar = character.unicodeScalars.first else { return false }
    switch scalar.properties.generalCategory {
    case .connectorPunctuation, .dashPunctuation, .openPunctuation, .closePunctuation,
         .initialPunctuation, .finalPunctuation, .otherPunctuation:
        return true
    default:
        return false
    }
}

/// 約物の種類 (全角・半角と、句読点の 、，, ・ 。．. を同じに見る)。打った字と表記の字を組にするのに使う
let punctuationFamilies: [Character: String] = [
    "、": "、", "，": "、", ",": "、", "､": "、", "。": "。", "．": "。", ".": "。", "｡": "。", "・": "・", "･": "・",
    "！": "!", "!": "!", "？": "?", "?": "?", "（": "(", "(": "(", "）": ")", ")": ")", "［": "[", "[": "[", "］": "]", "]": "]",
    "「": "「", "｢": "「", "」": "」", "｣": "」",
]
func punctuationFamily(_ character: Character) -> String {
    punctuationFamilies[character] ?? String(character)
}

/// 1 つの文節 (表記 word・読み reading) を、読みの約物の所で分ける。約物はそれだけの文節にする。
/// 読みの約物と表記の約物は、種類 (punctuationFamily) ごとに前から順に組にする。読みに無い種類の表記の約物 (辞書の トム・ハンクス の ・) は語の中に残す。
/// typedSymbols なら約物の字を読みの字 (打った字) に戻す (AI が 。 を . にしていた: N700けい…しよう。 → 使用.)。偽なら表記の字のまま (学習の側。選んだ字を覚える)。
/// 分けないとき (nil): 読みに約物が無い・読みが約物だけ (。。。 や / の文節はそのまま)・問いの か？ を含む (question_ka.swift が か？ ごとに比べて一覧に出す)・
/// 読みにある種類の約物の数が表記と違う (AI が記号を落とした・足した・別の記号にした)・組にした約物の順が前後する・約物の間の読みと表記の片方だけが空
func splitSegmentAtSymbols(word: String, reading: String, typedSymbols: Bool = true) -> [(word: String, reading: String)]? {
    let r = Array(reading), w = Array(word)
    guard r.contains(where: isPunctuationSymbol), !r.allSatisfy(isPunctuationSymbol) else { return nil }
    if r.indices.contains(where: { questionMarks.contains(r[$0]) && $0 > 0 && r[$0 - 1] == "か" }) { return nil }
    let rs = r.indices.filter { isPunctuationSymbol(r[$0]) }
    let families = Set(rs.map { punctuationFamily(r[$0]) })
    let ws = w.indices.filter { isPunctuationSymbol(w[$0]) && families.contains(punctuationFamily(w[$0])) }
    var byFamily: [String: [Int]] = [:]
    for i in ws { byFamily[punctuationFamily(w[i]), default: []].append(i) }
    var used: [String: Int] = [:]
    var matched: [Int] = []
    for ri in rs {
        let family = punctuationFamily(r[ri])
        let k = used[family, default: 0]
        guard let list = byFamily[family], k < list.count else { return nil }
        matched.append(list[k])
        used[family] = k + 1
    }
    guard matched.count == ws.count, zip(matched, matched.dropFirst()).allSatisfy({ $0 < $1 }) else { return nil }
    var out: [(word: String, reading: String)] = []
    var rp = 0, wp = 0
    for (ri, wi) in zip(rs, matched) {
        let rPiece = String(r[rp..<ri]), wPiece = String(w[wp..<wi])
        guard rPiece.isEmpty == wPiece.isEmpty else { return nil }
        if !rPiece.isEmpty { out.append((word: wPiece, reading: rPiece)) }
        out.append((word: String(typedSymbols ? r[ri] : w[wi]), reading: String(r[ri])))
        rp = ri + 1
        wp = wi + 1
    }
    let rTail = String(r[rp...]), wTail = String(w[wp...])
    guard rTail.isEmpty == wTail.isEmpty else { return nil }
    if !rTail.isEmpty { out.append((word: wTail, reading: rTail)) }
    return out
}

/// 変換の口 (ConvertSegments) の結果の文節を、読みの約物の所で分け直し、約物は打った字に戻す。
/// 2026-11-23 (a/nosplit) から読点と括弧 (AI ありの道では 。．！？・ も) は前後と一緒に変換し、記号が前の文節に付いて返っていた (初代|1形で、|杯)。
/// 記号だけの文節を前提にした決まり (。＋拡張子・・ の選び直しの学習・Space の一覧の読みで引く表・一覧で選んだ語の学習) が効かず、
/// 記号付きの語 (警告、) を覚えるおそれがあった。AI に渡す文脈 (記号より右) は変えない
func splittingJoinedSymbols(_ segments: [(word: String, rubyCount: Int)], reading: String) -> [(word: String, rubyCount: Int)] {
    guard symbolSegmentSplitEnabled else { return segments }
    let chars = Array(reading)
    guard segments.allSatisfy({ $0.rubyCount >= 0 }), segments.reduce(0, { $0 + $1.rubyCount }) == chars.count else { return segments }
    var out: [(word: String, rubyCount: Int)] = []
    var offset = 0
    for segment in segments {
        let segmentReading = String(chars[offset..<(offset + segment.rubyCount)])
        offset += segment.rubyCount
        if let parts = splitSegmentAtSymbols(word: segment.word, reading: segmentReading) {
            out += parts.map { (word: $0.word, rubyCount: $0.reading.count) }
        } else {
            out.append(segment)
        }
    }
    return out
}

/// 学習に渡す文節 (読みと表記の組) を、読みの約物の所で分ける (記号付きの語を覚えない)。表記の約物の字は選んだ字のまま
func splittingJoinedSymbols(readings: [String], texts: [String]) -> (readings: [String], texts: [String]) {
    guard symbolSegmentSplitEnabled, readings.count == texts.count else { return (readings, texts) }
    var outReadings: [String] = [], outTexts: [String] = []
    for (reading, text) in zip(readings, texts) {
        if let parts = splitSegmentAtSymbols(word: text, reading: reading, typedSymbols: false) {
            outReadings += parts.map(\.reading)
            outTexts += parts.map(\.word)
        } else {
            outReadings.append(reading)
            outTexts.append(text)
        }
    }
    return (outReadings, outTexts)
}
