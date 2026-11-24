import Foundation

/// 2026-10-22 (M3.6 誤用): IME は打ったとおりに従う。打った読みと違う語を先に出さない。
/// - misreading_candidates.tsv (読み<TAB>本来の形): 音の誤りで、誤った形も語の並びになるもの (てんかのほうとう → 伝家の宝刀) と読みのゆれ (せろん → 世論) は、
///   その読みの候補の一覧に本来の形が無ければ後ろに足す (先には出さない)
/// - misuse_notes.tsv (種類<TAB>引き方<TAB>鍵<TAB>注記): 候補窓の注記。「本来は〜」(誤用) と異字同訓の使い分け。出すかどうかは IME 本体の設定 (既定でオフ)
///   引き方 含む: 候補の字が鍵を含む (的を得 → 的を得る・的を得た)。読み: 鍵が 読み:表記 で、一覧の読みと候補の字がその組
/// 2026-10-22 (D の 2 回目の調べ M-1・M-2・M-3): 表は上限つきで読む (BoundedTable。制御文字の行・欄の数が違う行・長すぎる欄の行は捨てる)。
/// 配る表は 30 行・440 行ほど。注記は 1 件 200 字まで、「含む」の鍵は 2 字以上
/// 2026-11-06 (仕事 40): 5 列目 (書かなくてもよい) は「含む」の除く語 (| 区切り)。候補がそのどれかを含めば、その行の注記は付けない
/// (注意の注記の 差別・言い換え の鍵が、普通の語の中に入っているとき。めくら → めくらまし)
/// 2026-11-20 (読み間違いの「もしかして」。Mozc の reading_correction.tsv、Google 日本語入力と同じ形): misreading_candidates.tsv に 3 列目 (正しい読み) を足した。
///   3 列目のある行 (読み間違い。ごちゅう → 御中 (おんちゅう)) は、一覧に無ければ 4 番目に足し (件数が少なければ後ろ。1 番目には出さない)、
///   候補窓に注記「もしかして: 正しい読み」を出す (種類 読み誤り。IME 本体の設定に関係なく出す)。もう一覧にあれば動かさず、注記だけ付ける (Mozc の correction_rewriter と同じ)。
///   打った読みの後ろが助詞 (ごちゅうへ) なら、助詞を除いた読みで引き、形の後ろに助詞を付ける (Mozc の functional_value と同じ)
///   3 列目の無い行 (読みのゆれ。せろん → 世論) は、今までどおり一覧の後ろに足し、注記は付けない
///   4 列目が「後」の行 (誤った読みが強い語の読みでもある。かいし → 壊死 (開始)・じゅうよう → 慫慂 (重要)) は、4 番目でなく一覧の後ろに注記つきで足す
///   (開始・重要 を打った人の一覧の上のほうを汚さない。まとめ役 11-20)
let misreadingLimits = (maxBytes: 1 << 20, maxLines: 10_000, columns: 2...4, maxFieldLength: 100)
let misuseNoteLimits = (maxBytes: 1 << 20, maxLines: 20_000, columns: 4...5, maxFieldLength: 200)

/// 読み誤りの表 (読み → 本来の形)
struct MisreadingTable: Equatable {
    struct Correction: Equatable {
        let form: String
        let correct: String
        /// 4 列目が「後」: 4 番目でなく一覧の後ろに足す
        var atEnd = false
    }
    /// 3 列目の無い行 (読みのゆれ): 一覧に無ければ後ろに足す
    var back: [String: [String]] = [:]
    /// 3 列目のある行 (読み間違い): 一覧に無ければ 4 番目に足し、注記「もしかして: 正しい読み」
    var corrections: [String: [Correction]] = [:]
}

func parseMisreadingCandidates(_ content: String) -> MisreadingTable {
    misreadingCandidates(rows: BoundedTable.rows(text: content, maxLines: misreadingLimits.maxLines, columns: misreadingLimits.columns,
                                                 maxFieldLength: misreadingLimits.maxFieldLength))
}

func misreadingCandidates(rows: [[String]]) -> MisreadingTable {
    var table = MisreadingTable()
    for parts in rows where !parts[0].isEmpty && !parts[1].isEmpty {
        let correct = parts.count > 2 ? parts[2] : ""
        if correct.isEmpty {
            if !(table.back[parts[0]] ?? []).contains(parts[1]) { table.back[parts[0], default: []].append(parts[1]) }
        } else if !(table.corrections[parts[0]] ?? []).contains(where: { $0.form == parts[1] }) {
            table.corrections[parts[0], default: []].append(.init(form: parts[1], correct: correct, atEnd: parts.count > 3 && parts[3] == "後"))
        }
    }
    return table
}

struct MisuseNotes {
    var contains: [(key: String, kind: String, note: String, except: [String])] = []
    var byReading: [String: (kind: String, note: String)] = [:]   // 読み + "\t" + 表記
}

func parseMisuseNotes(_ content: String) -> MisuseNotes {
    misuseNotes(rows: BoundedTable.rows(text: content, maxLines: misuseNoteLimits.maxLines, columns: misuseNoteLimits.columns,
                                        maxFieldLength: misuseNoteLimits.maxFieldLength))
}

func misuseNotes(rows: [[String]]) -> MisuseNotes {
    var notes = MisuseNotes()
    for parts in rows where !parts[2].isEmpty && !parts[3].isEmpty {
        if parts[1] == "含む" && parts[2].count >= 2 {
            let except = parts.count > 4 ? parts[4].split(separator: "|").map(String.init).filter { $0.count > parts[2].count && $0.contains(parts[2]) } : []
            notes.contains.append((key: parts[2], kind: parts[0], note: parts[3], except: except))
        } else if parts[1] == "読み", let colon = parts[2].firstIndex(of: ":") {
            notes.byReading[String(parts[2][..<colon]) + "\t" + String(parts[2][parts[2].index(after: colon)...])] = (kind: parts[0], note: parts[3])
        }
    }
    // 長い鍵を先に (上や下への大騒 と 下への大騒 があれば長いほう)
    notes.contains.sort { $0.key.count > $1.key.count }
    return notes
}

@MainActor var misreadingCandidateTable: MisreadingTable? = nil
@MainActor func misreadingCandidates() -> MisreadingTable {
    if let table = misreadingCandidateTable { return table }
    let limits = misreadingLimits
    let table = misreadingCandidates(rows: BoundedTable.rows(execURL.appendingPathComponent("misreading_candidates.tsv"), maxBytes: limits.maxBytes,
                                                             maxLines: limits.maxLines, columns: limits.columns, maxFieldLength: limits.maxFieldLength))
    misreadingCandidateTable = table
    return table
}

@MainActor var misuseNoteTable: MisuseNotes? = nil
@MainActor func misuseNotes() -> MisuseNotes {
    if let table = misuseNoteTable { return table }
    let limits = misuseNoteLimits
    let table = misuseNotes(rows: BoundedTable.rows(execURL.appendingPathComponent("misuse_notes.tsv"), maxBytes: limits.maxBytes,
                                                    maxLines: limits.maxLines, columns: limits.columns, maxFieldLength: limits.maxFieldLength))
    misuseNoteTable = table
    return table
}

/// 候補の注記を作るか (IME 本体の設定 input.misuse_notes・homophone_notes のどちらかがオンのときだけ。候補の一覧を頼むたびに SetCandidateNotes で決める。
/// D の 2 回目の調べ M-1: 設定がオフなら注記を計算しない)
@MainActor var candidateNotesWanted = false

@_silgen_name("SetCandidateNotes")
@MainActor public func set_candidate_notes(_ wanted: Bool) {
    candidateNotesWanted = wanted
}

/// 候補の一覧の後ろに、音の誤りの本来の形を足す (もう一覧にあれば何もしない)。3 列目の無い行 (読みのゆれ) だけ
func appendingMisreadings(_ texts: [String], reading: String, table: MisreadingTable) -> [String] {
    guard let forms = table.back[reading] else { return texts }
    return texts + forms.filter { !texts.contains($0) }
}

/// 2026-11-20: 読み間違いの行を引くときに、打った読みの後ろから外して形の後ろに付け直す助詞 (長いものから)
let misreadingTails = ["から", "まで", "より", "には", "では", "とは", "への", "での", "との", "は", "が", "を", "に", "へ", "と", "で", "の", "も", "や"]

/// 打った読みに当たる読み間違いの行 (形は助詞を付け直したもの)。読みそのものの行が無ければ、後ろの助詞を外した読みで引く
func misreadingCorrections(reading: String, table: MisreadingTable) -> [MisreadingTable.Correction] {
    if let found = table.corrections[reading] { return found }
    for tail in misreadingTails where reading.count > tail.count && reading.hasSuffix(tail) {
        if let found = table.corrections[String(reading.dropLast(tail.count))] {
            return found.map { .init(form: $0.form + tail, correct: $0.correct, atEnd: $0.atEnd) }
        }
    }
    return []
}

/// 2026-11-20: 読み間違いの本来の形を一覧の 4 番目に足す (件数が 4 より少なければ後ろ。1 番目には出さない)。もう一覧にあれば動かさない。
/// 4 列目が「後」の行は一覧の後ろに足す
let misreadingInsertPosition = 3
func insertingMisreadingCorrections(_ texts: [String], reading: String, table: MisreadingTable) -> [String] {
    var result = texts
    var position = min(max(1, misreadingInsertPosition), result.count)
    var atEnd: [String] = []
    for correction in misreadingCorrections(reading: reading, table: table) where !result.contains(correction.form) && !atEnd.contains(correction.form) {
        if correction.atEnd {
            atEnd.append(correction.form)
        } else {
            result.insert(correction.form, at: min(position, result.count))
            position += 1
        }
    }
    return result + atEnd
}

/// 2026-11-20: 読み間違いの本来の形の候補の注記 (「読み誤り<TAB>もしかして: 正しい読み」。当たらなければ空)。IME 本体の設定に関係なく付ける
func misreadingNote(for text: String, reading: String, table: MisreadingTable) -> String {
    guard let found = misreadingCorrections(reading: reading, table: table).first(where: { $0.form == text }) else { return "" }
    return "読み誤り\tもしかして: " + found.correct
}

/// 候補の注記 (「種類<TAB>注記」。無ければ空)
func misuseNote(for text: String, reading: String, notes: MisuseNotes) -> String {
    if let found = notes.contains.first(where: { row in text.contains(row.key) && !row.except.contains(where: { text.contains($0) }) }) {
        return found.kind + "\t" + found.note
    }
    if let found = notes.byReading[reading + "\t" + text] {
        return found.kind + "\t" + found.note
    }
    return ""
}
