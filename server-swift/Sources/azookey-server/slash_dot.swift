import Foundation
import KanaKanjiConverterModule

/// 2026-11-05 (仕事 22 の 2): かなの入力中に打った / と . の、変換するときの決め直し。
/// IME 本体は、数字・英字の語のすぐ後ろの / を半角の / のまま読みに入れ (打った時点で左の字で決める)、かなのあとの / は ・、. は 。 で入れる。
/// ここでは、Space・句読点の変換・清書のときに前後を見て次の 2 つを決め直す。打っている途中の仮表示 (速い・辞書だけの変換) では決め直さない:
///  ① 数字＋/＋数字 (1/2/3 のような並びも) のすぐ後ろ (変換した表記) が、・ で並べる形の語 (1・2年生・2・26事件・1・2・3丁目) で始まるなら、/ を ・ に。
///     語の表 (slashDotFollowWords) は Wikipedia 日本語版で 数字・数字 と 数字/数字 のすぐ後ろを数えたもの (tools/slash_dot)。迷う形 (年・号・回・月) は / のまま
///  ② かなのすぐ後ろの 。 に、小文字で打った拡張子 (pdf・docx・png) が続き、その後ろが助詞・空白・記号・読みの終わりのとき、
///     。 の前の文節が名詞 (資料・議事録・レポート) で終わり、その文節の前の文節が は・が・も で終わっていなければ、。 を . に (資料.pdfは)。
///     前が文の終わりの形 (助詞・助動詞: だよ・です・ました・ね) や動詞・形容詞なら 。 のまま (〜だよ。pdfは)。
///     体言止めの文 (結論は以上。pdfは) は名詞の文節の前が は なので 迷う形 として 。 のまま。品詞は変換サーバーの辞書の語の右の番号で見る。
///     。 のままの所の拡張子は、表の形 (PDF・DOCX) にする (IME 本体はこの並びの拡張子だけ表記直しをせずに渡す。表記直しの設定がオフなら小文字のまま)
/// ② で見る拡張子 (小文字で打ったもの) と、。 のままのときの形 (english_case.txt の表記。表に無いものは小文字のまま)。
/// IME 本体の auto_english::dot_extensions と同じ並び
let dotExtensionForms: [String: String] = [
    "pdf": "PDF", "doc": "doc", "docx": "DOCX", "xls": "xls", "xlsx": "XLSX", "ppt": "ppt", "pptx": "PPTX",
    "png": "PNG", "jpg": "JPG", "jpeg": "JPEG", "gif": "GIF", "bmp": "bmp", "svg": "svg", "webp": "webp", "heic": "heic", "tiff": "tiff",
    "txt": "txt", "md": "md", "csv": "CSV", "zip": "zip", "exe": "exe", "mp3": "mp3", "mp4": "mp4", "wav": "wav", "mov": "mov",
    "html": "HTML", "json": "JSON",
]

/// ① の、数字＋/＋数字 のすぐ後ろ (変換した表記) がこの語で始まれば ・ にする語 (37 語)。Wikipedia 日本語版 (試験の記事を外した本文) で、
/// 数字＋区切り＋数字 のすぐ後ろの漢字・カタカナの並びの頭ごとに ・ と / (／) を数え、合計 100 以上・・ の割合 0.9 以上のもの
/// (tools/slash_dot/scan_slash_dot.py・follow_table.py。試験の記事では ・ に直す 3,523 回が正しく 146 回が / の所)
let slashDotFollowWords: [String] = ["日", "番", "話", "年生", "合併号", "丁目", "代", "列車", "階", "系統", "位", "巻", "時", "号車", "編成", "週", "号機", "期", "年修了者", "両", "次車", "月合併号", "月度", "歳", "作", "塁", "軍", "年次", "号線", "事件", "回戦", "制", "学年", "条", "勝", "年代", "モード"]

/// ①' の、英字の語どうしの区切りを / にする組 (左/右、打ったとおりの大文字小文字)。Wikipedia 日本語版 (試験の記事を外した本文) で、
/// 英字の語＋区切り (・ / ／)＋英字の語 の組ごとに数え、合計 100 以上・/ の割合 0.9 以上のもの (66 組。左が小文字の語の組は除く:
/// IME 本体が打った時点で / にする)。試験の記事では / に直す 2,888 回が正しく 40 回が ・ の所 (tools/slash_dot/scan_latin_pairs.py・pair_table.py)
let latinSlashPairs: Set<String> = ["ISO/IEC", "I/O", "F/A", "PC/AT", "Fate/stay", "Fate/Grand", "TCP/IP", "AUTUMN/WINTER", "Ki/oon", "EHESS/Cassini", "SPRING/SUMMER", "CP/M", "RED/THEATER", "X/S", "ON/OFF", "TRACE/FBI", "AC/DC", "Fate/Zero", "DOS/V", "AN/SPS", "IC/BS", "GNU/Linux", "AN/APG", "CBS/SONY", "HIV/AIDS", "MSX/MSX", "Fate/kaleid", "Z/X", "AN/SQS", "Fate/EXTRA", "HR/HM", "GHQ/SCAP", "ZUKKON/BAKKON", "Ldh/EHESS", "kJ/mol", "ISO/TS", "PA/SIC", "CDU/CSU", "Epic/Sony", "Words/Music", "XP/Vista", "J/FPS", "IC/PA", "Fate/hollow", "IC/TB", "CAD/CAM", "ISO/TR", "EPIC/SONY", "POP/THE", "AN/M", "APC/C", "AN/AAQ", "AN/UYK", "Fate/Apocrypha", "Mbit/s", "CBS/Sony", "NIP/TUCK", "SA/SIC", "AN/SPY", "Arrangement/Music", "THEATER/TOPS", "eDP/DP", "DP/HDMI", "MHX/MHXX", "V/STOL", "PL/I"]

/// 2026-11-05 (仕事 28 の 1): / と . の打ち分けの設定 (input.slash_key: auto・dot・slash、input.period_key: auto・kuten・period。初期値 auto)。
/// auto でないときは、その記号を決め直さず、選び直しも覚えない (IME 本体が いつも ・ か / ・ いつも 。 か . で入れる)
@MainActor func slashKeyAuto() -> Bool { (config["slashKey"] as? String ?? "auto") == "auto" }
@MainActor func periodKeyAuto() -> Bool { (config["periodKey"] as? String ?? "auto") == "auto" }

/// 2026-11-05 (仕事 28 の 6): 数学の式の直し (input.math_expressions)。書いていなければ、よく打つ分野に数学か物理があればオン (前の版と同じ)
func mathExpressionsOn(config: [String: Any], focused: Set<String>) -> Bool {
    (config["mathExpressions"] as? Bool) ?? (focused.contains("domain_math") || focused.contains("domain_physics"))
}

/// 数学の文で / を先にする左の文節の終わり: 英字 1 字 (その前が英字でない)・数字・閉じ括弧
func mathSlashLeft(_ left: String) -> Bool {
    guard let last = left.last else { return false }
    if last.isASCII && last.isNumber || last == ")" || last == "）" { return true }
    guard last.isASCII && last.isLetter else { return false }
    let before = left.dropLast().last
    return !(before.map { $0.isASCII && $0.isLetter } ?? false)
}

/// 数学の文か: 数学の式の直しがオン (input.math_expressions。signals が nil でない) のうえで、今の文と直前の文 (文脈の終わりの 2 文と、変換した文) に
/// 数学の目印の語 (domain_math_signals.txt) か式の記号 (= + ^ < > と英字か数字) がある
func isMathSentence(context: String, text: String, signals: DomainBoost?) -> Bool {
    guard let signals else { return false }
    var sentences = context.split(whereSeparator: { "。！？\n".contains($0) }).suffix(2).joined()
    sentences += text
    if signals.signalCount(sentences, minimum: 1) >= 1 { return true }
    let chars = Array(text)
    return chars.indices.contains { k in "=+^<>".contains(chars[k]) && ((k > 0 && (chars[k - 1].isASCII && (chars[k - 1].isLetter || chars[k - 1].isNumber) || chars[k - 1] == ")"))
        || (k + 1 < chars.count && chars[k + 1].isASCII && (chars[k + 1].isLetter || chars[k + 1].isNumber))) }
}

/// 数学の式の直しがオンのときの、数学の目印の語 (setup_domain_dictionaries で読む。オフなら nil)
@MainActor var mathSentenceSignals: DomainBoost? = nil

/// 文節ごとの品詞 (辞書の語の並びの、最後の語の左右の番号)。引けなければ nil
typealias SlashDotPartOfSpeech = @MainActor (_ reading: String, _ word: String, _ left: String) -> (lcid: Int, rcid: Int)?

func isDotNounCID(_ cid: Int) -> Bool {
    // 名詞 (一般・サ変接続・固有名詞・数・接尾) の番号。形容動詞語幹 (1287: 静か)・代名詞 (1306: 彼)・非自立 (1310: こと・1313: 以上) は除く。
    // 辞書で確かめた番号: 資料・写真 1285、レポート・申請 1283、議事録・報告書 1288 (slash_dot_tests の testPrintPartOfSpeechForProbeWords)
    (1283...1286).contains(cid) || (1288...1305).contains(cid)
}

func isDotFunctionCID(_ cid: Int) -> Bool {
    // 助詞 147...368・助動詞 369...554
    (147...554).contains(cid)
}

private func isDotParticleHead(_ c: Character) -> Bool {
    "はがをにでともへやのかまよ".contains(c)
}

private func dotBoundary(_ rest: ArraySlice<Character>) -> Bool {
    guard let c = rest.first else { return true }
    if c.isASCII && (c.isLetter || c.isNumber) { return false }
    if ("ぁ"..."ゖ").contains(c) { return isDotParticleHead(c) }
    // カタカナ・漢字が続く (pdfファイル) ときは決めない
    if ("ァ"..."ヺ").contains(c) || c == "ー" { return false }
    if let scalar = c.unicodeScalars.first, (0x4E00...0x9FFF).contains(scalar.value) { return false }
    return true
}

/// 変換の結果の文節 (表記, 読みの文字数) と読みから、/ と 。 を決め直す。decide = false (打っている途中の仮表示) なら ① ② は決め直さず、② の拡張子の形だけ直す
@MainActor func decidingSlashDot(_ segments: [(word: String, rubyCount: Int)], reading: String, context: String, decide: Bool,
                      caseFix: Bool, partOfSpeech: SlashDotPartOfSpeech, math: Bool = false,
                      slashes: Bool = true, periods: Bool = true) -> [(word: String, rubyCount: Int)] {
    let chars = Array(reading)
    guard chars.contains("/") || chars.contains("。") || chars.contains("・") else { return segments }
    var out = segments
    var starts: [Int] = []
    var offset = 0
    for s in segments { starts.append(offset); offset += s.rubyCount }
    guard offset == chars.count else { return segments }
    // 2026-11-06 (まとめ役 10:17): 今の文と直前の文に「内積」か「ベクトル」があれば、英字 1 字どうしの間の ・ は / にしない (高校数学の内積 a・b。
    // ベクトルどうしの割り算は無い)。分数の a/b はベクトルを含まない数学の文で今までどおり /
    let vectorSentence = math && {
        let window = context.split(whereSeparator: { "。！？\n".contains($0) }).suffix(2).joined() + segments.map(\.word).joined()
        return window.contains("内積") || window.contains("ベクトル")
    }()
    func singleLetterEnd(_ text: String) -> Bool {
        guard let last = text.last, last.isASCII && last.isLetter else { return false }
        return !(text.dropLast().last.map { $0.isASCII && $0.isLetter } ?? false)
    }
    func singleLetterStart(_ text: String) -> Bool {
        guard let first = text.first, first.isASCII && first.isLetter else { return false }
        return !(text.dropFirst().first.map { $0.isASCII && $0.isLetter } ?? false)
    }
    for i in out.indices where out[i].rubyCount == 1 {
        let p = starts[i]
        if chars[p] == "/" && out[i].word == "/" && decide && slashes {
            // 左: 1〜4 けたの数字 (その前は数字・/・. でない)。右: 1〜4 けたの数字の並び (1/2/3 のように / でつないでよい) のあとに表の語
            var l = p
            while l > 0 && chars[l - 1].isASCII && chars[l - 1].isNumber { l -= 1 }
            guard (1...4).contains(p - l), !(l > 0 && "/.,・".contains(chars[l - 1])) else { continue }
            var slashes = [p]
            var r = p + 1
            while true {
                let start = r
                while r < chars.count && chars[r].isASCII && chars[r].isNumber { r += 1 }
                guard (1...4).contains(r - start) else { r = -1; break }
                if r < chars.count && chars[r] == "/" { slashes.append(r); r += 1; continue }
                break
            }
            guard r > 0, !(r < chars.count && ".,".contains(chars[r])) else { continue }
            // 並びの / がどれも 1 字の文節 (変換が / のまま) で、最後の数字のあとは変換した表記で照らす (読みの かい は 階 か 回 か分からない)。
            // 数字は打ったとおりに残っているときだけ
            let indices = slashes.compactMap { q in starts.firstIndex(of: q) }
            guard indices.count == slashes.count, indices.allSatisfy({ out[$0].word == "/" && out[$0].rubyCount == 1 }) else { continue }
            let last = indices.last!
            let after = out[(last + 1)...].map(\.word).joined()
            let digits = String(chars[(slashes.last! + 1)..<r])
            guard after.hasPrefix(digits) else { continue }
            let rest = after.dropFirst(digits.count)
            if slashDotFollowWords.contains(where: { rest.hasPrefix($0) }) {
                for k in indices { out[k].word = "・" }
            }
        } else if math && decide && slashes && "・/".contains(chars[p]) && out[i].word == String(chars[p]) && i > 0 && mathSlashLeft(out[i - 1].word)
                    && !(vectorSentence && chars[p] == "・" && i + 1 < out.count && singleLetterEnd(out[i - 1].word) && singleLetterStart(out[i + 1].word)) {
            // ①'' 数学の文 (数学の式の直しがオンで、今の文と直前の文に数学の目印の語か式の記号がある) では、英字 1 字・数字・閉じ括弧の
            // すぐ後ろの区切りを / に (a/b・2/3・(x+1)/2)。内積の a・b は選び直しの学習 (math: の組) に任せる (まとめ役 14:51 の案 3)
            out[i].word = "/"
        } else if chars[p] == "・" && out[i].word == "・" && decide && slashes && i > 0 && i + 1 < out.count {
            // ①' 英字の語どうしの決まった組 (TCP/IP・I/O・A/B): 左の文節の終わりの英字の並びと、右の文節の頭の英字の並びが表の組なら / に。
            // IME 本体は略語・大文字 1 字のあとの / を ・ で入れる (Wikipedia で ・ と / が割れる型)。組の表は Wikipedia の本文で数えたもの
            let left = String(out[i - 1].word.reversed().prefix { $0.isASCII && $0.isLetter }.reversed())
            let right = String(out[i + 1].word.prefix { $0.isASCII && $0.isLetter })
            guard !left.isEmpty, !right.isEmpty else { continue }
            if let c = out[i + 1].word.dropFirst(right.count).first, c.isASCII && c.isNumber { continue }
            if latinSlashPairs.contains(left + "/" + right) {
                out[i].word = "/"
            }
        } else if chars[p] == "。" && out[i].word == "。" && periods && i > 0 && i + 1 < out.count {
            // 左はかな (読みのかな。英字・数字のすぐ後ろは IME 本体が決める)
            guard p > 0, ("ぁ"..."ゖ").contains(chars[p - 1]) || ("ァ"..."ヺ").contains(chars[p - 1]) || chars[p - 1] == "ー" else { continue }
            let next = out[i + 1].word
            guard let ext = dotExtensionForms.keys.filter({ next.hasPrefix($0) }).max(by: { $0.count < $1.count }) else { continue }
            let afterExt = starts[i + 1] + ext.count
            guard afterExt <= chars.count, String(chars[(p + 1)..<afterExt]) == ext else { continue }
            // 拡張子の後ろは変換した表記で見る (読みの か は から の頭か 回収 の頭か分からない)。文節が拡張子で終われば次の文節の頭
            let rest = Array(next.dropFirst(ext.count)) + (next.count == ext.count && i + 2 < out.count ? Array(out[i + 2].word) : [])
            // 英字・数字が続けば拡張子の語ではない (pdfs・mp34)
            if let c = rest.first, c.isASCII && (c.isLetter || c.isNumber) { continue }
            var filename = false
            if decide && dotBoundary(rest[...]) {
                let prev = out[i - 1]
                let prevReading = String(chars[starts[i - 1]..<p])
                let left = context + out[..<(i - 1)].map(\.word).joined()
                if let pos = partOfSpeech(prevReading, prev.word, left), isDotNounCID(pos.rcid), !isDotFunctionCID(pos.rcid) {
                    // 名詞の文節の前の文節が は・が・も で終わる (結論は以上) のは体言止めの文かもしれないので 迷う形
                    let before = i >= 2 ? out[i - 2].word : context
                    filename = !(before.hasSuffix("は") || before.hasSuffix("が") || before.hasSuffix("も"))
                }
            }
            if filename {
                out[i].word = "."
            } else if caseFix, let form = dotExtensionForms[ext], form != ext {
                out[i + 1].word = form + next.dropFirst(ext.count)
            }
        }
    }
    return out
}

/// 変換サーバーの辞書で、文節 (読み・表記) になる語の並びを引き、最後の語の左右の番号を返す (scopeLookup。引けなければ nil)
@MainActor func slashDotPartOfSpeech(reading: String, word: String, left: String) -> (lcid: Int, rcid: Int)? {
    guard let last = scopeLookup(reading: reading, text: word, context: left)?.data.last else { return nil }
    return (last.lcid, last.rcid)
}

/// 英字の表記直しの設定 (input.english_case_fix、初期値オン)。オフなら ② の拡張子を表の形にしない
@MainActor func englishCaseFixEnabled() -> Bool {
    config["englishCaseFix"] as? Bool ?? true
}

/// ② の文節の候補の一覧 (ListCandidates): 前の文脈が 。 で終わり、読みの頭が小文字で打った拡張子 (pdfは) なら、候補の頭の拡張子を表の形に
/// (変換と同じ PDFは を先に。同じ形になった候補は 1 つに)。. で終わる文脈 (資料.) のあとは打ったとおりの小文字のまま
func dotExtensionCandidates(_ texts: [String], reading: String, context: String, caseFix: Bool) -> [String] {
    guard caseFix, context.hasSuffix("。"),
          let ext = dotExtensionForms.keys.filter({ reading.hasPrefix($0) }).max(by: { $0.count < $1.count }),
          let form = dotExtensionForms[ext], form != ext else { return texts }
    if let c = reading.dropFirst(ext.count).first, c.isASCII && (c.isLetter || c.isNumber) { return texts }
    var seen = Set<String>()
    return texts.map { $0.hasPrefix(ext) ? form + $0.dropFirst(ext.count) : $0 }.filter { seen.insert($0).inserted }
}

// MARK: - 3. 候補の一覧と選び直しの学習 (仕事 22 の 3)

/// 読みが記号 1 字の文節 (・ / ／、。 .) の、候補の一覧にいつも並べる形。ほかの読みは nil
func slashDotAlternatives(reading: String) -> [String]? {
    switch reading {
    case "・", "/", "／": return ["・", "/", "／"]
    case "。", ".": return ["。", "."]
    default: return nil
    }
}

/// 候補の一覧 (ListCandidates) の頭に、記号のほかの形を並べる (IME 本体は今の表記を 1 番目に置き、同じ形は 1 つにまとめる)
func appendingSlashDotAlternatives(_ texts: [String], reading: String) -> [String] {
    guard let alternatives = slashDotAlternatives(reading: reading) else { return texts }
    var seen = Set<String>()
    return (alternatives + texts).filter { seen.insert($0).inserted }
}

/// 選び直しを覚える組。記号の文節 index の前後の文節の表記から作る (どれでもなければ nil)。
///  数字どうし (1/2年): dd + 後ろの数字のあとの語 / 英字どうし (TCP・IP): ll + 左右の英字の並び / かなどうし (賛成・反対): jj + 左右の文節 (8 字まで) /
///  。 か . のあとの拡張子 (資料.pdf): dot + 前の文節 (8 字まで) + 拡張子。数学の文 (math) は別に覚える (数学で / を選んでも ふつうの文には効かない)
func slashDotLearningKey(texts: [String], index i: Int, reading: Character, math: Bool) -> String? {
    guard i > 0, i + 1 < texts.count else { return nil }
    let left = texts[i - 1], right = texts[i + 1]
    let isDigit: (Character) -> Bool = { $0.isASCII && $0.isNumber || ("０"..."９").contains($0) }
    let isLatin: (Character) -> Bool = { $0.isASCII && $0.isLetter }
    var key: String
    if "・/／".contains(reading) {
        if let l = left.last, let r = right.first, isDigit(l), isDigit(r) {
            key = "dd\t" + String(right.drop(while: isDigit).prefix(6))
        } else if let l = left.last, let r = right.first, isLatin(l), isLatin(r) {
            key = "ll\t" + String(left.reversed().prefix(while: isLatin).reversed()) + "\t" + String(right.prefix(while: isLatin))
        } else {
            key = "jj\t" + String(left.suffix(8)) + "\t" + String(right.prefix(8))
        }
    } else if "。.".contains(reading) {
        let lower = right.lowercased()
        guard let ext = dotExtensionForms.keys.filter({ lower.hasPrefix($0) }).max(by: { $0.count < $1.count }) else { return nil }
        key = "dot\t" + String(left.suffix(8)) + "\t" + ext
    } else {
        return nil
    }
    return (math ? "math:" : "") + key
}

/// 覚えた組 (組 → 選んだ形) と、覚えた順 (古いものから消す)
struct SlashDotLearned: Equatable {
    var forms: [String: String] = [:]
    var order: [String] = []
}

let slashDotLearnedLimit = 5_000

func parseSlashDotLearned(_ text: String?) -> SlashDotLearned {
    var learned = SlashDotLearned()
    guard let text else { return learned }
    for line in text.split(whereSeparator: \.isNewline).prefix(slashDotLearnedLimit) {
        let parts = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
        guard parts.count >= 3, let form = parts.last, slashDotAlternatives(reading: form) != nil else { continue }
        let key = parts.dropLast().joined(separator: "\t")
        if learned.forms[key] == nil { learned.order.append(key) }
        learned.forms[key] = form
    }
    return learned
}

func formatSlashDotLearned(_ learned: SlashDotLearned) -> String {
    learned.order.compactMap { key in learned.forms[key].map { "\(key)\t\($0)\n" } }.joined()
}

/// 確定した形 chosen を、今の決め (表で決めた形) defaultForm と比べて覚える。同じなら忘れる (決めに戻した)。変わったら true
func noteSlashDotChoice(_ learned: inout SlashDotLearned, key: String, chosen: String, defaultForm: String) -> Bool {
    if chosen == defaultForm {
        guard learned.forms.removeValue(forKey: key) != nil else { return false }
        learned.order.removeAll { $0 == key }
        return true
    }
    guard learned.forms[key] != chosen else { return false }
    if learned.forms[key] == nil { learned.order.append(key) }
    learned.forms[key] = chosen
    if learned.order.count > slashDotLearnedLimit {
        let drop = learned.order.count - slashDotLearnedLimit
        for old in learned.order.prefix(drop) { learned.forms.removeValue(forKey: old) }
        learned.order.removeFirst(drop)
    }
    return true
}

/// 決め直しのあとの文節に、覚えた形をかける (同じ組で前に選び直した形)。かけた結果と、かける前 (表の決め) の記号の形 (読みの中の位置ごと) を返す
func applyingSlashDotLearned(_ segments: [(word: String, rubyCount: Int)], reading: String, learned: SlashDotLearned, math: Bool,
                             families: String = "・/／。.") -> (segments: [(word: String, rubyCount: Int)], defaults: [Int: String]) {
    let chars = Array(reading)
    var out = segments
    var defaults: [Int: String] = [:]
    var offset = 0
    let texts = segments.map(\.word)
    for i in segments.indices {
        defer { offset += segments[i].rubyCount }
        guard segments[i].rubyCount == 1, offset < chars.count, slashDotAlternatives(reading: String(chars[offset])) != nil,
              slashDotAlternatives(reading: segments[i].word) != nil, families.contains(chars[offset]) else { continue }
        defaults[offset] = segments[i].word
        guard let key = slashDotLearningKey(texts: texts, index: i, reading: chars[offset], math: math),
              let form = learned.forms[key], slashDotAlternatives(reading: String(chars[offset]))?.contains(form) == true else { continue }
        out[i].word = form
    }
    return (out, defaults)
}

/// 設定 input.slash_learning (初期値オン)。開発用のビルドでは AZK_SLASH_LEARNING=0 でも切れる。
/// 学習データを使ってはいけない接続 (personalDataAllowed が偽) では、記録を使わない・覚えない
@MainActor func slashDotLearningEnabled() -> Bool {
    personalDataAllowed && (config["slashLearning"] as? Bool ?? true) && devEnvironment["AZK_SLASH_LEARNING"] != "0"
}

/// 選び直しを覚える記号 (仕事 28 の 1: auto の記号だけ)
@MainActor func slashDotLearningFamilies() -> String {
    (slashKeyAuto() ? "・/／" : "") + (periodKeyAuto() ? "。." : "")
}

@MainActor var slashDotLearnedCache: SlashDotLearned? = nil

@MainActor func slashDotLearnedURL() -> URL { execURL.appendingPathComponent("memory").appendingPathComponent("slash_dot_learned.txt") }

@MainActor func slashDotLearned() -> SlashDotLearned {
    guard personalDataAllowed else { return SlashDotLearned() }
    if let learned = slashDotLearnedCache { return learned }
    let stored = parseSlashDotLearned(BoundedTable.text(slashDotLearnedURL(), maxBytes: 1 << 20))
    slashDotLearnedCache = stored
    return stored
}

@MainActor func saveSlashDotLearned(_ learned: SlashDotLearned) {
    guard personalDataAllowed else { return }
    slashDotLearnedCache = learned
    try? formatSlashDotLearned(learned).data(using: .utf8)?.write(to: slashDotLearnedURL(), options: .atomic)
}

/// 直近の決め直し (Space・清書) の、読み → 記号の読みの中の位置 → 表で決めた形。確定のとき (LearnSegments)、選んだ形と比べる
@MainActor var slashDotDefaults: [String: [Int: String]] = [:]

/// 変換の結果に、覚えた形をかけ、表の決めを覚えておく (打っている途中の仮表示ではかけない)
@MainActor func slashDotLearnedApplied(_ segments: [(word: String, rubyCount: Int)], reading: String, math: Bool = false) -> [(word: String, rubyCount: Int)] {
    guard slashDotLearningEnabled(), reading.contains(where: { slashDotAlternatives(reading: String($0)) != nil }) else { return segments }
    let applied = applyingSlashDotLearned(segments, reading: reading, learned: slashDotLearned(), math: math, families: slashDotLearningFamilies())
    if slashDotDefaults.count >= 64 { slashDotDefaults.removeAll() }
    slashDotDefaults[reading] = applied.defaults
    return applied.segments
}

/// 確定した文節 (読み・表記) の記号の形を覚える (LearnSegments)
@MainActor func noteSlashDotChoices(readings: [String], texts: [String], context: String = "") {
    guard slashDotLearningEnabled(), readings.count == texts.count,
          readings.contains(where: { slashDotAlternatives(reading: $0) != nil }) else { return }
    let reading = readings.joined()
    let math = isMathSentence(context: context, text: texts.joined(), signals: mathSentenceSignals)
    let defaults = slashDotDefaults.removeValue(forKey: reading) ?? [:]
    var learned = slashDotLearned()
    var changed = false
    var offset = 0
    for i in readings.indices {
        defer { offset += readings[i].count }
        guard let r = readings[i].first, readings[i].count == 1, slashDotAlternatives(reading: readings[i])?.contains(texts[i]) == true,
              slashDotLearningFamilies().contains(r),
              let key = slashDotLearningKey(texts: texts, index: i, reading: r, math: math) else { continue }
        // 一覧を出さずに確定した所 (決めを覚えていない) は、読みの字そのものを決めとみる (・ は ・、/ は /)
        let defaultForm = defaults[offset] ?? readings[i]
        if noteSlashDotChoice(&learned, key: key, chosen: texts[i], defaultForm: defaultForm) { changed = true }
    }
    if changed { saveSlashDotLearned(learned) }
}
