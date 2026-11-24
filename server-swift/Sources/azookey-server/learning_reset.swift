import Foundation
import KanaKanjiConverterModule

// 2026-11-08: 学習をまっさらにする (設定の画面から、変換サーバーを通して消す。サーバーがメモリに持っている学習が、あとで書き戻さないように)
//
// - 範囲 (a) includeTables = false: 変換の学習 (azooKey の memory.* と、まだ書き出していない学習)
// - 範囲 (b) includeTables = true: (a) に加えて、HaniWord の学習の表
//   - 変換サーバー (memory の中): 表記の選び・・ と / と 。 . の選び・同音語の選びと断り・数え方で算用数字を残した語
//   - IME 本体 (%APPDATA%\HaniWord の直下): 英字の語・母音を補わない綴り・直さない英語の綴り・予測の候補・ローマ字の打ち方の回数・/ で英字にした語
// - 消さないもの: 利用者が書く表 (learning_ng.txt・english_terms_personal.txt)・単語登録・設定
// 消す前に、%APPDATA%\HaniWord\memory_backup_<日時> へ写す (写せなければ何も消さない)。
// IME 本体がメモリに持っている学習 (予測の候補・直さない英語の綴り) は、learning_reset.txt が新しくなったのを見て捨てる

/// IME 本体が書く学習の表 (%APPDATA%\HaniWord の直下)
let clientLearnedFileNames = ["english_learned.txt", "typo_repair_ng.txt", "latin_spell_ng.txt", "phrases.tsv", "romaji_style_stats.json", "slash_commands.txt"]

/// 変換サーバーが書く HaniWord の学習の表 (memory の中)
let serverLearnedTableNames = ["hyouki_learned.txt", "slash_dot_learned.txt", "homophone_learned.txt", "homophone_declined.txt", "counter_numerals_kept.txt"]

/// IME 本体に、学習をまっさらにしたことを知らせる印のファイル (中身は日時だけ。更新の時刻を見る)
let learningResetMarkName = "learning_reset.txt"

/// 学習の控えを base\memory_backup_<stamp> に取り (同じ名前があれば _2 から)、範囲 (b) なら HaniWord の学習の表を消す。
/// azooKey の memory.* は、呼ぶ側が変換器から消す (メモリの学習と一緒に)。写すのに失敗したら throw して、何も消さない。控えの場所を返す
func backUpAndRemoveLearningFiles(base: URL, includeTables: Bool, stamp: String) throws -> URL {
    let fm = FileManager.default
    var backup = base.appendingPathComponent("memory_backup_\(stamp)")
    var suffix = 2
    while fm.fileExists(atPath: backup.path) {
        backup = base.appendingPathComponent("memory_backup_\(stamp)_\(suffix)")
        suffix += 1
    }
    try fm.createDirectory(at: backup, withIntermediateDirectories: true)
    let memory = base.appendingPathComponent("memory")
    if fm.fileExists(atPath: memory.path) {
        try fm.copyItem(at: memory, to: backup.appendingPathComponent("memory"))
    }
    if includeTables {
        for name in clientLearnedFileNames where fm.fileExists(atPath: base.appendingPathComponent(name).path) {
            try fm.copyItem(at: base.appendingPathComponent(name), to: backup.appendingPathComponent(name))
        }
        // 全部を写し終えてから消す
        for name in serverLearnedTableNames {
            try? fm.removeItem(at: memory.appendingPathComponent(name))
        }
        for name in clientLearnedFileNames {
            try? fm.removeItem(at: base.appendingPathComponent(name))
        }
    }
    try? Data((stamp + "\n").utf8).write(to: base.appendingPathComponent(learningResetMarkName))
    return backup
}

/// 控えの場所を、利用者に見せる形 (C:\Users\…) にする
func displayPath(_ url: URL) -> String {
    url.withUnsafeFileSystemRepresentation { pointer in
        pointer.map { String(cString: $0) } ?? url.path
    }
}

/// 学習をまっさらにする。includeTables が偽なら (a)、真なら (b)。控えの場所を返す (控えを取れなかったら空で、何も消していない)
@_silgen_name("ResetLearning")
@MainActor public func reset_learning(_ includeTables: Bool) -> UnsafeMutablePointer<CChar> {
    // まだ書き出していない学習も控えに入るよう、先に書き出す
    save_learning()
    let formatter = DateFormatter()
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.dateFormat = "yyyyMMdd-HHmmss"
    // Windows の Swift では既定の時間帯が世界時になる (試しで 03:17 に消して 20261108-181724 になった)。日付の候補 (dateCandidates) と同じに日本の時刻で
    formatter.timeZone = TimeZone(identifier: "Asia/Tokyo") ?? .current
    let backup: URL
    do {
        backup = try backUpAndRemoveLearningFiles(base: execURL, includeTables: includeTables, stamp: formatter.string(from: Date()))
    } catch {
        return _strdup("")!
    }
    // azooKey の学習 (メモリと memory.*) を消し、学習を引いて作った途中の結果も捨てる
    converter.sendToDicdataStore(.resetMemory)
    converter.stopComposition()
    clearPreviewMemo()
    learnGuardDropped.clear()  // 2026-11-21: L2 で外した学習の語の覚えも消す (learn_guard.swift)
    learnedSinceSave = 0
    if includeTables {
        // 消した表を、次に使うときに読み直す (空から)
        hyoukiLearnedCache = nil
        slashDotLearnedCache = nil
        homophoneLearnedCache = nil
        homophoneLearnedOffered = []
        homophoneDeclinedCache = nil
        counterNumeralsKeptCache = nil
    }
    return _strdup(displayPath(backup))!
}
