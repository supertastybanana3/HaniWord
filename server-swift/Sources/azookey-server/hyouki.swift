import Foundation
import KanaKanjiConverterModule

/// 2026-11-03: 表記ゆれ (仮名でも漢字でもよい所・送り仮名のあるなし・交ぜ書き) を、利用者が選ぶ向きに合わせる。
/// 表: hyouki_groups.tsv (同じ読み・同じ品詞の番号で意味の同じ書き分けの組。data_ext\hyouki の README)。読み・左右の番号・形の 3 つで当てる。
///   同じ読み・番号の行のうち、JMdict の項目か漢字を含む形を共有する行 (いう|言う と いう|云う) は、1 つの語にまとめる。
///   かなの形だけを共有する行 (まく|膜 と まく|幕) は別の語のまま。2 つ以上の語に入るかなの形は、どの語か分からないので替えも学習もしない
/// 覚えるもの (memory\hyouki_learned.txt。個人のデータで、配らない・git に入れない): 確定した語が表の語なら、その語で選んだ形の数と、
///   種類 (補助動詞・名詞のかな書き …) ごとの向き (語の形のうちいちばんかなの多い形を選んだ = かな寄り、いちばん少ない形 = 漢字寄り) の数。
///   一覧で選び直した形は 1、今の表示のまま確定した形は hyoukiWeights.accepted (0 も選べる。今の既定の形が強まり続けないように)。
/// 使い方: 変換の結果 (仮表示・清書・Space) の語を、点 (コーパスの回数 + その語で選んだ数 + 種類の向き) で今の形より十分よい形に替え、
///   一覧ではその形を同じ語のほかの形の前に置く。覚えた記録が無い語・種類では何も変えない。
///   かなが同じ読み・品詞の別の語のかなでもある語 (いる = 居る・要る) は、かな → 漢字 に替えない。
///   種類の向きは同じ種類の中だけで効かせる (使い分けの種類 = 形式名詞・補助動詞・補助形容詞 の向きを、名詞・動詞のかな書きに使わない。逆も)。
///   記録は確定のときに変わり、次の変換から効く (入力の途中では変わらない)。設定 input.hyouki_learning で切れる (初期値はオン)
struct HyoukiLexeme: Equatable {
    var forms: [String] = []
    /// コーパスの回数 (Wikipedia + 青空文庫)。形の順
    var counts: [Double] = []
    var category = ""
    /// かなの形が、同じ読みの別の語のかなでもある (表の印「かなは別の語と共有」と、同じ読みのほかの番号の行に同じかなの形があるもの)
    var sharedKana = false
}

struct HyoukiTable {
    /// 読み<TAB>左<TAB>右 → その読み・番号の語 (ふつうは 1 つ。かなの形だけを共有する別の語があれば 2 つ以上)
    var lexemes: [String: [HyoukiLexeme]] = [:]
    /// 読み → その読みの語の鍵 (一覧を並べるとき)
    var byReading: [String: [String]] = [:]
    var isEmpty: Bool { lexemes.isEmpty }
    static func key(reading: String, lcid: Int, rcid: Int) -> String { "\(reading)\t\(lcid)\t\(rcid)" }

    /// 形 form がちょうど 1 つの語に入るなら、その語 (2 つ以上の語に入るかなの形は nil)
    func lexeme(_ key: String, containing form: String) -> HyoukiLexeme? {
        let found = (lexemes[key] ?? []).filter { $0.forms.contains(form) }
        return found.count == 1 ? found[0] : nil
    }
}

/// 全部の組 (7.4 万行) で 15 MB
let hyoukiTableLimits = (maxBytes: 24 << 20, maxLines: 200_000, maxFieldLength: 1024)

/// 表の行: 読み<TAB>左<TAB>右<TAB>形 (| 区切り)<TAB>種類<TAB>ゆれ<TAB>印<TAB>Wikipedia の回数 (形の順)<TAB>青空文庫の回数 (形の順)<TAB>…<TAB>JMdict の項目 (15 欄目、, 区切り)
func parseHyoukiTable(_ text: String?) -> HyoukiTable {
    var table = HyoukiTable()
    guard let text else { return table }
    struct Row { var forms: [String]; var counts: [Double]; var category: String; var mark: String; var entries: Set<String> }
    var rows: [String: [Row]] = [:]
    var order: [String] = []
    for row in BoundedTable.rows(text: text, maxLines: hyoukiTableLimits.maxLines, columns: 9...32, maxFieldLength: hyoukiTableLimits.maxFieldLength) {
        guard !row[0].isEmpty, let lcid = Int(row[1]), let rcid = Int(row[2]) else { continue }
        let forms = row[3].split(separator: "|").map(String.init)
        guard forms.count >= 2 else { continue }
        let wikipedia = row[7].split(separator: "|").map { Double($0) ?? 0 }
        let aozora = row[8].split(separator: "|").map { Double($0) ?? 0 }
        let counts = forms.indices.map { (($0 < wikipedia.count) ? wikipedia[$0] : 0) + (($0 < aozora.count) ? aozora[$0] : 0) }
        let entries = row.count > 14 ? Set(row[14].split(separator: ",").map(String.init)) : []
        let key = HyoukiTable.key(reading: row[0], lcid: lcid, rcid: rcid)
        if rows[key] == nil {
            order.append(key)
            table.byReading[row[0], default: []].append(key)
        }
        rows[key, default: []].append(Row(forms: forms, counts: counts, category: row[4], mark: row[6], entries: entries))
    }
    for key in order {
        let group = rows[key]!
        // JMdict の項目か漢字を含む形を共有する行をつなぐ
        var parent = Array(group.indices)
        func root(_ i: Int) -> Int { parent[i] == i ? i : root(parent[i]) }
        for i in group.indices {
            for j in group.indices where j > i {
                let sharesEntry = !group[i].entries.isDisjoint(with: group[j].entries)
                let sharesKanji = group[i].forms.contains { !hyoukiAllHiragana($0) && group[j].forms.contains($0) }
                if sharesEntry || sharesKanji {
                    let a = root(i), b = root(j)
                    parent[b] = a
                }
            }
        }
        var merged: [Int: HyoukiLexeme] = [:]
        var marks: [Int: [String]] = [:]
        var roots: [Int] = []
        for (i, row) in group.enumerated() {
            let r = root(i)
            var lexeme = merged[r] ?? HyoukiLexeme(forms: [], counts: [], category: row.category, sharedKana: false)
            if merged[r] == nil { roots.append(r) }
            for (k, form) in row.forms.enumerated() where !lexeme.forms.contains(form) {
                lexeme.forms.append(form)
                lexeme.counts.append(row.counts[k])
            }
            merged[r] = lexeme
            marks[r, default: []].append(row.mark)
        }
        table.lexemes[key] = roots.map { r in
            var lexeme = merged[r]!
            lexeme.sharedKana = marks[r]!.contains { hyoukiKanaShared(mark: $0, forms: lexeme.forms) }
            return lexeme
        }
    }
    // 2026-11-04: 同じ読みのほかの番号 (品詞の違う語) の行に同じかなの形があれば、それも共有とみる (名詞の および (お呼び) と接続詞の および (及び)。
    // 表の印は同じ番号の中だけで付いていて、AI が および に名詞の番号を付けて選ぶと、漢字の向きで お呼び に替えていた)
    for keys in table.byReading.values where keys.count > 1 {
        for key in keys {
            let otherKana = Set(keys.filter { $0 != key }.flatMap { table.lexemes[$0] ?? [] }.flatMap { $0.forms.filter(hyoukiAllHiragana) })
            guard !otherKana.isEmpty, var lexemes = table.lexemes[key] else { continue }
            for i in lexemes.indices where lexemes[i].forms.contains(where: { hyoukiAllHiragana($0) && otherKana.contains($0) }) {
                lexemes[i].sharedKana = true
            }
            table.lexemes[key] = lexemes
        }
    }
    return table
}

/// 印「かなは別の語と共有 (X・Y・…)」の語のうち、まとめた語の形に入らないものがあるか (… で切れていれば、あるとみる)
func hyoukiKanaShared(mark: String, forms: [String]) -> Bool {
    guard let start = mark.range(of: "かなは別の語と共有") else { return false }
    let rest = mark[start.upperBound...]
    guard let open = rest.firstIndex(of: "("), let close = rest[open...].firstIndex(of: ")") else { return true }
    let listed = rest[rest.index(after: open)..<close].trimmingCharacters(in: .whitespaces)
    if listed.hasSuffix("…") { return true }
    return listed.split(separator: "・").contains { !forms.contains(String($0)) }
}

/// 種類ごとの向きの数
struct HyoukiDirection: Equatable {
    var kana = 0.0
    var kanji = 0.0
}

struct HyoukiLearned: Equatable {
    /// 語の鍵 (読み<TAB>左<TAB>右) → 形 → 選んだ数 (重みつき)
    var forms: [String: [String: Double]] = [:]
    /// 種類 → 向きの数
    var directions: [String: HyoukiDirection] = [:]
    var isEmpty: Bool { forms.isEmpty && directions.isEmpty }
}

/// 点の重み。開発用のビルドでは AZK_HYOUKI_ACCEPTED・_PRIOR・_CHOSEN・_DIRECTION・_MARGIN・_MINIMUM で試せる (測りで決める)
struct HyoukiWeights {
    /// 今の表示のまま確定した形の数え方 (一覧で選び直した形は 1)
    var accepted = 0.0
    /// コーパスの回数 (log(1 + 回数)) の重み
    var prior = 1.0
    /// その語で選んだ数の重み
    var chosen = 1.0
    /// 種類の向き (−1〜1) × 形のかなの割合 の重み
    var direction = 4.0
    /// 今の形からこれ以上よいときだけ替える
    var margin = 0.5
    /// 種類の向きは、数がこれ以上になってから使う
    var minimum = 2.0
}

let hyoukiWeights: HyoukiWeights = {
    var w = HyoukiWeights()
    func value(_ name: String) -> Double? { devEnvironment[name].flatMap(Double.init).flatMap { $0.isFinite ? $0 : nil } }
    if let v = value("AZK_HYOUKI_ACCEPTED") { w.accepted = v }
    if let v = value("AZK_HYOUKI_PRIOR") { w.prior = v }
    if let v = value("AZK_HYOUKI_CHOSEN") { w.chosen = v }
    if let v = value("AZK_HYOUKI_DIRECTION") { w.direction = v }
    if let v = value("AZK_HYOUKI_MARGIN") { w.margin = v }
    if let v = value("AZK_HYOUKI_MINIMUM") { w.minimum = v }
    return w
}()

/// 形の字のうち、ひらがな (と長音) の割合
func hyoukiKanaRatio(_ form: String) -> Double {
    guard !form.isEmpty else { return 0 }
    let kana = form.filter { c in c.unicodeScalars.allSatisfy { (0x3041...0x3096).contains($0.value) || $0.value == 0x30FC } }.count
    return Double(kana) / Double(form.count)
}

func hyoukiAllHiragana(_ form: String) -> Bool { !form.isEmpty && hyoukiKanaRatio(form) == 1 }

/// 確定した語を覚える (読み・左右の番号・形が表の語のときだけ)。覚えたら true
func noteHyoukiChoice(_ learned: inout HyoukiLearned, table: HyoukiTable, reading: String, lcid: Int, rcid: Int, form: String, weight: Double) -> Bool {
    guard weight > 0 else { return false }
    let key = HyoukiTable.key(reading: reading, lcid: lcid, rcid: rcid)
    guard let lexeme = table.lexeme(key, containing: form) else { return false }
    learned.forms[key, default: [:]][form, default: 0] += weight
    let ratio = hyoukiKanaRatio(form)
    let others = lexeme.forms.filter { $0 != form }.map(hyoukiKanaRatio)
    if others.allSatisfy({ $0 < ratio }) {
        learned.directions[lexeme.category, default: HyoukiDirection()].kana += weight
    } else if others.allSatisfy({ $0 > ratio }) {
        learned.directions[lexeme.category, default: HyoukiDirection()].kanji += weight
    }
    return true
}

/// 今の形 current を替えるなら、その形。その語で選んだ数か種類の向きが無ければ替えない。
/// 点 = prior × log(1 + コーパスの回数) + chosen × 選んだ数 + direction × 種類の向き × かなの割合。替えるのは、利用者の分の点が今の形より高く、
/// 点の合計が margin 以上高い形 (いちばん高いもの)
func hyoukiPreferredForm(_ lexeme: HyoukiLexeme, current: String, chosen: [String: Double]?, direction: HyoukiDirection?, weights: HyoukiWeights) -> String? {
    guard let ci = lexeme.forms.firstIndex(of: current) else { return nil }
    let total = (direction?.kana ?? 0) + (direction?.kanji ?? 0)
    let tendency = total >= weights.minimum && total > 0 ? ((direction?.kana ?? 0) - (direction?.kanji ?? 0)) / (total + 1) : 0
    let chosenTotal = chosen?.values.reduce(0, +) ?? 0
    guard chosenTotal > 0 || tendency != 0 else { return nil }
    func user(_ i: Int) -> Double {
        weights.chosen * (chosen?[lexeme.forms[i]] ?? 0) + weights.direction * tendency * hyoukiKanaRatio(lexeme.forms[i])
    }
    func score(_ i: Int) -> Double { weights.prior * log1p(max(0, lexeme.counts[i])) + user(i) }
    var best: Int? = nil
    for i in lexeme.forms.indices where i != ci {
        // かなが別の語と共有の語では、かな → 漢字 に替えない (いる を 居る にしない。要る・射る のこともある)
        if lexeme.sharedKana && hyoukiAllHiragana(current) && !hyoukiAllHiragana(lexeme.forms[i]) { continue }
        guard user(i) > user(ci), score(i) - score(ci) >= weights.margin else { continue }
        if best.map({ score(i) > score($0) }) ?? true { best = i }
    }
    return best.map { lexeme.forms[$0] }
}

/// 学習を消す (Ctrl+Delete の ForgetLearned と learning_ng.txt): 消す語 (words) を含む形の記録を消す。種類の向きの数はそのまま
func forgettingHyouki(_ learned: HyoukiLearned, words: [String]) -> HyoukiLearned {
    var kept = learned
    for (key, forms) in learned.forms {
        let left = forms.filter { form, _ in !words.contains(where: { !$0.isEmpty && form.contains($0) }) }
        kept.forms[key] = left.isEmpty ? nil : left
    }
    return kept
}

// 記録のファイル: F<TAB>読み<TAB>左<TAB>右<TAB>形<TAB>数 と D<TAB>種類<TAB>かな寄り<TAB>漢字寄り
func formatHyoukiLearned(_ learned: HyoukiLearned) -> String {
    var lines = ["# 表記ゆれの組で、利用者が確定した形の数と種類ごとの向き (memory\\hyouki_learned.txt)。個人のデータ (配らない)"]
    for key in learned.forms.keys.sorted() {
        for (form, count) in learned.forms[key]!.sorted(by: { $0.key < $1.key }) { lines.append("F\t\(key)\t\(form)\t\(count)") }
    }
    for (category, d) in learned.directions.sorted(by: { $0.key < $1.key }) { lines.append("D\t\(category)\t\(d.kana)\t\(d.kanji)") }
    return lines.joined(separator: "\n") + "\n"
}

func parseHyoukiLearned(_ text: String?) -> HyoukiLearned {
    var learned = HyoukiLearned()
    guard let text else { return learned }
    for row in BoundedTable.rows(text: text, maxLines: hyoukiLearnedLimit, columns: 4...6, maxFieldLength: 64) {
        if row[0] == "F", row.count == 6, let count = Double(row[5]), count.isFinite, count > 0, Int(row[2]) != nil, Int(row[3]) != nil {
            learned.forms[[row[1], row[2], row[3]].joined(separator: "\t"), default: [:]][row[4]] = count
        } else if row[0] == "D", row.count == 4, let kana = Double(row[2]), let kanji = Double(row[3]), kana.isFinite, kanji.isFinite {
            learned.directions[row[1]] = HyoukiDirection(kana: max(0, kana), kanji: max(0, kanji))
        }
    }
    return learned
}

let hyoukiLearnedLimit = 20_000

// MARK: - 変換サーバーの中の状態

/// 設定 input.hyouki_learning (初期値はオン)。開発用のビルドでは AZK_HYOUKI=0 でも切れる。
/// 学習データを使ってはいけない接続 (personalDataAllowed が偽: サンドボックスのアプリなど) では、記録を使わない・覚えない
@MainActor func hyoukiLearningEnabled() -> Bool {
    personalDataAllowed && (config["hyoukiLearning"] as? Bool ?? true) && devEnvironment["AZK_HYOUKI"] != "0"
}

/// 表 (7.4 万行) は読むのに時間がかかるので、Initialize で別のスレッドに読ませ、はじめて使うときに読み終わるのを待つ
final class HyoukiTableLoad: @unchecked Sendable {
    private let done = DispatchSemaphore(value: 0)
    private var table = HyoukiTable()

    init(url: URL) {
        DispatchQueue.global(qos: .utility).async { [self] in
            table = parseHyoukiTable(BoundedTable.text(url, maxBytes: hyoukiTableLimits.maxBytes))
            done.signal()
        }
    }

    func wait() -> HyoukiTable {
        done.wait()
        done.signal()
        return table
    }
}

@MainActor var hyoukiTableLoad: HyoukiTableLoad? = nil
@MainActor var hyoukiTableCache: HyoukiTable? = nil

/// 開発用のビルドでは AZK_HYOUKI_TABLE で別の表 (よく出る組だけ、など) を読める
@MainActor func startLoadingHyoukiTable() {
    guard hyoukiTableLoad == nil, hyoukiTableCache == nil else { return }
    let url = devEnvironment["AZK_HYOUKI_TABLE"].map { URL(fileURLWithPath: $0) } ?? execURL.appendingPathComponent("hyouki_groups.tsv")
    hyoukiTableLoad = HyoukiTableLoad(url: url)
}

@MainActor func hyoukiTable() -> HyoukiTable {
    if let loaded = hyoukiTableCache { return loaded }
    startLoadingHyoukiTable()
    let loaded = hyoukiTableLoad?.wait() ?? HyoukiTable()
    hyoukiTableCache = loaded
    hyoukiTableLoad = nil
    return loaded
}

@MainActor var hyoukiLearnedCache: HyoukiLearned? = nil

@MainActor func hyoukiLearnedURL() -> URL { execURL.appendingPathComponent("memory").appendingPathComponent("hyouki_learned.txt") }

@MainActor func hyoukiLearned() -> HyoukiLearned {
    guard personalDataAllowed else { return HyoukiLearned() }
    if let learned = hyoukiLearnedCache { return learned }
    let stored = forgettingHyouki(parseHyoukiLearned(BoundedTable.text(hyoukiLearnedURL(), maxBytes: 4 << 20)), words: LearningDiscard.words)
    hyoukiLearnedCache = stored
    return stored
}

/// 記録を書き直す (学習を覚えるとき・消すときだけ)
@MainActor func saveHyoukiLearned(_ learned: HyoukiLearned) {
    guard personalDataAllowed else { return }
    let kept = forgettingHyouki(learned, words: LearningDiscard.words)
    guard kept.forms.values.map(\.count).reduce(0, +) + kept.directions.count <= hyoukiLearnedLimit else { return }
    hyoukiLearnedCache = kept
    try? formatHyoukiLearned(kept).data(using: .utf8)?.write(to: hyoukiLearnedURL(), options: .atomic)
}

/// 変換の結果の候補の語を、覚えた向きの形に替える (替える所が無ければ nil)。仮表示・清書・Space のどれにも同じにかける
@MainActor func hyoukiApplied(_ candidate: Candidate) -> Candidate? {
    guard hyoukiLearningEnabled() else { return nil }
    let learned = hyoukiLearned()
    guard !learned.isEmpty else { return nil }
    let table = hyoukiTable()
    guard !table.isEmpty else { return nil }
    var data = candidate.data
    var changed = false
    for i in data.indices {
        let e = data[i]
        let key = HyoukiTable.key(reading: e.ruby.toHiragana(), lcid: e.lcid, rcid: e.rcid)
        guard let lexeme = table.lexeme(key, containing: e.word),
              let form = hyoukiPreferredForm(lexeme, current: e.word, chosen: learned.forms[key], direction: learned.directions[lexeme.category], weights: hyoukiWeights)
        else { continue }
        data[i] = DicdataElement(word: form, ruby: e.ruby, lcid: e.lcid, rcid: e.rcid, mid: e.mid, value: e.value())
        changed = true
    }
    guard changed else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
        lastMid: candidate.lastMid, data: data)
}

/// Space の一覧 (読み全体が 1 語の表記ゆれの語のとき): 覚えた向きの形を、同じ語のほかの形のうちいちばん前にある所へ
@MainActor func orderingHyouki(_ texts: [String], reading: String) -> [String] {
    guard hyoukiLearningEnabled() else { return texts }
    let learned = hyoukiLearned()
    guard !learned.isEmpty else { return texts }
    let table = hyoukiTable()
    guard let keys = table.byReading[reading] else { return texts }
    var out = texts
    for key in keys {
        for lexeme in table.lexemes[key] ?? [] {
            // いちばん前にある形を今の形とみる (2 つ以上の語に入るかなの形なら見ない)
            guard let first = out.firstIndex(where: { lexeme.forms.contains($0) }), table.lexeme(key, containing: out[first]) != nil,
                  let preferred = hyoukiPreferredForm(lexeme, current: out[first], chosen: learned.forms[key], direction: learned.directions[lexeme.category], weights: hyoukiWeights),
                  let at = out.firstIndex(of: preferred), at > first else { continue }
            out.remove(at: at)
            out.insert(preferred, at: first)
        }
    }
    return out
}

/// 2026-11-04: Space の一覧を出した読みと、その 1 番目 (IME 本体は一覧が来ると 1 番目を表示にする)。確定した形がこの 1 番目と違うときだけ、
/// 利用者が選んだ形として数える (一覧を開かずに確定した形と、1 番目のまま確定した形は今の表示のまま)。前は辞書だけの変換の 1 位と比べていて、
/// AI の変換 (文脈つき) の漢字をそのまま確定した分まで選んだ形に数え、かなを好む人でも漢字の向きが溜まった (打ち手のまね 11-04 で かな 25.9 → 23.3%)
@MainActor var hyoukiListedFirst: [String: String] = [:]

@MainActor func noteHyoukiListed(reading: String, first: String?) {
    if hyoukiListedFirst.count >= 256 { hyoukiListedFirst.removeAll() }
    hyoukiListedFirst[reading] = first
}

/// 確定した文節 (learnOne で表記の合った候補) の語を覚える。defaultText は今の表示 (一覧を出した読みならその 1 番目、そうでなければ確定した表記そのもの)。
/// 今の表示のまま確定した形は accepted の重み (初期値 0) で、選び直した形は 1 で数える
@MainActor func noteHyoukiChoices(_ candidate: Candidate, defaultText: String?) {
    guard hyoukiLearningEnabled() else { return }
    let table = hyoukiTable()
    guard !table.isEmpty else { return }
    let weight = candidate.text == defaultText ? hyoukiWeights.accepted : 1
    var learned = hyoukiLearned()
    var changed = false
    for e in candidate.data {
        if noteHyoukiChoice(&learned, table: table, reading: e.ruby.toHiragana(), lcid: e.lcid, rcid: e.rcid, form: e.word, weight: weight) { changed = true }
    }
    if changed { saveHyoukiLearned(learned) }
}

/// 候補の一覧で学習した語を忘れる (Ctrl+Delete。ForgetLearned) とき、その表記の形の記録も消す
@MainActor func forgetHyoukiChoices(text: String) {
    let learned = hyoukiLearned()
    let kept = forgettingHyouki(learned, words: [text])
    if kept != learned { saveHyoukiLearned(kept) }
}

#if AZK_DEV_KNOBS
/// 測るための入口 (リポジトリの外の測りの台本が呼ぶ。配る版には入れない): 直近の変換 (conversionSegmentsUncached) で選んだ候補の語の並び
@MainActor var hyoukiLastElements: [DicdataElement] = []

/// 読みを変換して (覚えの memo を使わない)、文節 (表記<TAB>読みの字数) と、選んだ候補の語 (形<TAB>読み<TAB>左<TAB>右) を返す。
/// 2 つの間は空行。表記ゆれの学習の測りで、決まった好みの打ち手をまねるため (語ごとに表の語かを見て、好みの形に直して確定する)
@_silgen_name("ConvertElements")
@MainActor public func convert_elements(readingPtr: UnsafePointer<CChar>, contextPtr: UnsafePointer<CChar>, fast: Bool, dictionaryOnly: Bool) -> UnsafeMutablePointer<CChar> {
    let reading = normalizeForConversion(String(cString: readingPtr))
    let context = leftContext(String(cString: contextPtr))
    hyoukiLastElements = []
    let segments = conversionSegmentsUncached(hiragana: reading, context: context, fast: fast, dictionaryOnly: dictionaryOnly)
    let head = segments.map { "\($0.word)\t\($0.rubyCount)" }
    let words = hyoukiLastElements.map { "\($0.word)\t\($0.ruby.toHiragana())\t\($0.lcid)\t\($0.rcid)" }
    return _strdup((head + [""] + words).joined(separator: "\n"))!
}
#endif
