//
//  LearningMemory.swift
//  Keyboard
//
//  Created by ensan on 2021/02/01.
//  Copyright © 2021 ensan. All rights reserved.
//

import Foundation
import SwiftUtils

private struct MetadataElement: CustomDebugStringConvertible {
    init(day: UInt16, count: UInt8) {
        self.lastUsedDay = day
        self.lastUpdatedDay = day
        self.count = count
    }

    var lastUsedDay: UInt16
    var lastUpdatedDay: UInt16
    var count: UInt8

    var debugDescription: String {
        "(lastUsedDay: \(lastUsedDay), lastUpdatedDay: \(lastUpdatedDay), count: \(count))"
    }
}

/// 長期記憶用の構造体
/// azooKey-Windows: 学習から外す語 (サーバーが learning_ng.txt から読む)。保存のときに、この語を含む学習を捨てる
public enum LearningDiscard {
    nonisolated(unsafe) public static var words: [String] = []
}

/// azooKey-Windows (2026-11-23、セキュリティの点検 12): 壊れた学習のファイルを退けたときの知らせ (壊れ方の種類・退けたファイルの数・退けた先のフォルダの名前)。
/// 学習の中身は渡さない。サーバーが記録に 1 行書く
public enum LearningCorruption {
    nonisolated(unsafe) public static var report: ((_ reason: String, _ fileCount: Int, _ folderName: String) -> Void)? = nil
}

private extension UnsafeRawBufferPointer {
    // offset からリトルエンディアンの整数を、バイトを組み立てて読む (汎用の loadUnaligned は 1 回ごとに遅く、3 万項目で 0.4 秒かかった)。範囲は呼ぶ側が確かめる
    func u16(_ o: Int) -> Int { Int(self[o]) | Int(self[o + 1]) << 8 }
    func u32(_ o: Int) -> Int { Int(self[o]) | Int(self[o + 1]) << 8 | Int(self[o + 2]) << 16 | Int(self[o + 3]) << 24 }
    func u64(_ o: Int) -> UInt64 { (0 ..< 8).reduce(UInt64(0)) { $0 | UInt64(self[o + $1]) << UInt64(8 * $1) } }
}

struct LongTermLearningMemory {
    private static func pauseFileURL(directoryURL: URL) -> URL {
        directoryURL.appendingPathComponent(".pause", isDirectory: false)
    }
    private static func loudsFileURL(asTemporaryFile: Bool, directoryURL: URL) -> URL {
        if asTemporaryFile {
            return directoryURL.appendingPathComponent("memory.louds.2", isDirectory: false)
        } else {
            return directoryURL.appendingPathComponent("memory.louds", isDirectory: false)
        }
    }
    private static func metadataFileURL(asTemporaryFile: Bool, directoryURL: URL) -> URL {
        if asTemporaryFile {
            return directoryURL.appendingPathComponent("memory.memorymetadata.2", isDirectory: false)
        } else {
            return directoryURL.appendingPathComponent("memory.memorymetadata", isDirectory: false)
        }
    }
    private static func loudsCharsFileURL(asTemporaryFile: Bool, directoryURL: URL) -> URL {
        if asTemporaryFile {
            return directoryURL.appendingPathComponent("memory.loudschars2.2", isDirectory: false)
        } else {
            return directoryURL.appendingPathComponent("memory.loudschars2", isDirectory: false)
        }
    }
    private static func loudsTxt3FileURL(_ value: String, asTemporaryFile: Bool, directoryURL: URL) -> URL {
        if asTemporaryFile {
            return directoryURL.appendingPathComponent("memory\(value).loudstxt3.2", isDirectory: false)
        } else {
            return directoryURL.appendingPathComponent("memory\(value).loudstxt3", isDirectory: false)
        }
    }
    private static func fileExist(_ url: URL) -> Bool {
        FileManager.default.fileExists(atPath: url.path)
    }
    /// 上書きする関数
    /// - Parameters:
    ///   - fromURL: 上書きする内容を持ったファイル。消去はされない。
    ///   - toURL: 上書きされるファイル。元あったファイルは消去され、`fromURL`で指定された中身になる。
    private static func overwrite(from fromURL: URL, to toURL: URL) throws {
        // これは成功してもしなくても良い
        // - ファイルが存在して削除ができない場合はエラーにしたいが、その後のcopyが失敗するので問題ない。
        // - ファイルが存在せず削除ができない場合はエラーにしたくないが、その後のcopyが成功するので問題ない。
        try? FileManager.default.removeItem(at: toURL)
        // `.2`ファイルは残したままreplaceを実施する。
        try FileManager.default.copyItem(at: fromURL, to: toURL)
    }

    /// 学習が壊れた状態にあるか判定する関数
    ///  - note: 壊れている場合、一時的に学習をオフにすると良い。
    static func memoryCollapsed(directoryURL: URL) -> Bool {
        fileExist(pauseFileURL(directoryURL: directoryURL))
    }

    static var txtFileSplit: Int { 2048 }

    // MARK: azooKey-Windows (2026-11-23、セキュリティの点検 12): 壊れた学習のファイルを、読む前に見つけて退ける
    // 停電や落ちたときに途中で切れたファイル、件数や位置の壊れたファイルを、merge (保存・忘れる) と変換が範囲を確かめずに切り出し、
    // サーバーが落ちていた (落ちたあと起動し直しても、同じファイルを読んでまた落ちる)。読める形でなければ memory_corrupt_<日時> に退け、空の学習で続ける

    /// 学習のファイル 1 つの大きさの上限。ふだんは数百 KB
    static let maxFileBytes = 64 << 20
    /// 項目の数 (memory.memorymetadata の頭の数) の上限。分けたファイルの数は 項目の数 / 2048 + 1
    static let maxEntryCount = 1 << 20

    /// 大きさ (maxFileBytes 以下) を確かめてから丸ごと読む。無ければ・大きすぎれば nil
    private static func readWhole(_ url: URL) -> Data? {
        guard LOUDS.sizeAtMost(url, maxFileBytes) else { return nil }
        return try? Data(contentsOf: url, options: [.uncached])
    }

    /// 学習のファイル一式 (.2 のつかないもの) が、merge と変換が範囲の外を読まずに読める形か。読めるなら nil、読めなければ壊れ方の種類 (中身は入れない)。
    /// 読み込み (起動・保存のあとの読み直し) と保存のときだけ呼ぶ (変換のたびには呼ばない)
    static func corruption(directoryURL: URL) -> String? {
        let metadataURL = metadataFileURL(asTemporaryFile: false, directoryURL: directoryURL)
        let charsURL = loudsCharsFileURL(asTemporaryFile: false, directoryURL: directoryURL)
        let loudsURL = loudsFileURL(asTemporaryFile: false, directoryURL: directoryURL)
        guard fileExist(metadataURL) else {
            // 学習が無い。ほかのファイルだけが残っていると、merge は項目 0 として metadata の外を読む
            return [charsURL, loudsURL, loudsTxt3FileURL("0", asTemporaryFile: false, directoryURL: directoryURL)].contains(where: fileExist) ? "metadata が無い" : nil
        }
        guard let metadata = readWhole(metadataURL) else { return "metadata が読めないか大きすぎる" }
        // 項目ごとの metadata の数 (1 バイト) と、その数 × MetadataElement が、ちょうどファイルの終わりまで並ぶ
        var itemCounts: [Int] = []
        if let reason = metadata.withUnsafeBytes({ metadataCorruption($0, itemCounts: &itemCounts) }) { return reason }
        let n = itemCounts.count
        if n > 0 {
            // 節の字 (1 節 1 バイト) と LOUDS の木 (節 n 個: 0 が n 個、1 が n - 1 個。残りのビットは 1 で埋める)
            guard let chars = readWhole(charsURL) else { return "loudschars2 が読めない" }
            guard chars.count == n else { return "loudschars2 の数が項目の数と合わない" }
            guard let louds = readWhole(loudsURL) else { return "louds が読めない" }
            guard louds.count % 8 == 0, louds.count / 8 == (2 * n - 1 + 63) / 64 else { return "louds の長さが項目の数と合わない" }
            if let reason = louds.withUnsafeBytes({ loudsTreeCorruption($0, nodeCount: n) }) { return reason }
        }
        // 分けたファイル memory<k>.loudstxt3: 項目 k * 2048 から 2048 個ずつ。頭に件数 (UInt16) と各項目の位置 (UInt32)、項目は 件数 (UInt16) + 10 バイト × 件数 + 読みと語
        for k in 0 ..< n / txtFileSplit + 1 {
            let expected = min(txtFileSplit, n - k * txtFileSplit)
            let url = loudsTxt3FileURL("\(k)", asTemporaryFile: false, directoryURL: directoryURL)
            guard fileExist(url) else {
                if expected == 0 { continue }
                return "loudstxt3 が無い"
            }
            guard let txt = readWhole(url) else { return "loudstxt3 が読めないか大きすぎる" }
            let counts = itemCounts[(k * txtFileSplit) ..< (k * txtFileSplit + expected)]
            if let reason = txt.withUnsafeBytes({ loudstxt3Corruption($0, itemCounts: counts) }) { return reason }
        }
        return nil
    }

    /// 測りのときだけ真 (試験の testLargeLearningTimes が、直す前の保存の時間を測るため)。merge の入力の検査を省く
    nonisolated(unsafe) static var skipMergeCheckForMeasurement = false

    /// merge が読む metadata と loudstxt3 (memory<k>、k は metadata の項目の数から) を、大きさの上限つきで読む。metadata が無ければ項目 0
    private static func readMergeInputs(directoryURL: URL) -> (metadata: Data, loudstxts: [Int: Data]) {
        let metadataURL = metadataFileURL(asTemporaryFile: false, directoryURL: directoryURL)
        // 無ければ項目 0 (学習が無い)。そのときも memory0.loudstxt3 は読む (残っていれば、検査 mergeInputCorruption が件数の食い違いで退ける)。
        // 大きすぎて読めないときは空として読み、検査で退ける
        let metadata = LOUDS.readBounded(metadataURL, maxBytes: maxFileBytes) ?? (fileExist(metadataURL) ? Data() : Data([.zero, .zero, .zero, .zero]))
        let n = metadata.count >= 4 ? min(metadata.withUnsafeBytes { $0.u32(0) }, maxEntryCount) : 0
        var loudstxts: [Int: Data] = [:]
        for k in 0 ..< n / txtFileSplit + 1 {
            if let data = LOUDS.readBounded(loudsTxt3FileURL("\(k)", asTemporaryFile: false, directoryURL: directoryURL), maxBytes: maxFileBytes) {
                loudstxts[k] = data
            }
        }
        return (metadata, loudstxts)
    }

    /// merge の入力 (metadata と loudstxt3) が、merge が範囲の外を読まずに読める形か。読めるなら nil
    static func mergeInputCorruption(metadata: Data, loudstxts: [Int: Data]) -> String? {
        var itemCounts: [Int] = []
        if let reason = metadata.withUnsafeBytes({ metadataCorruption($0, itemCounts: &itemCounts) }) { return reason }
        let n = itemCounts.count
        for k in 0 ..< n / txtFileSplit + 1 {
            let expected = min(txtFileSplit, n - k * txtFileSplit)
            guard let txt = loudstxts[k] else {
                if expected == 0 { continue }
                return "loudstxt3 が無いか大きすぎる"
            }
            let counts = itemCounts[(k * txtFileSplit) ..< (k * txtFileSplit + expected)]
            if let reason = txt.withUnsafeBytes({ loudstxt3Corruption($0, itemCounts: counts) }) { return reason }
        }
        return nil
    }

    /// 変換が学習の LOUDS をたどる前の検査: metadata の形と、loudschars2・louds が節の数と木の形に合うか。loudstxt3 は見ない
    /// (変換が引く所 getDataForLoudstxt3 は範囲を確かめてから切り出す。merge は読むときに確かめる)。読めるなら nil。
    /// rebuildable: 合わないのが louds・loudschars2 だけ (か metadata が無いだけ) で、次の保存 (merge) が metadata と loudstxt3 から作り直せる
    static func loadCorruption(directoryURL: URL) -> (reason: String, rebuildable: Bool)? {
        let metadataURL = metadataFileURL(asTemporaryFile: false, directoryURL: directoryURL)
        let charsURL = loudsCharsFileURL(asTemporaryFile: false, directoryURL: directoryURL)
        let loudsURL = loudsFileURL(asTemporaryFile: false, directoryURL: directoryURL)
        guard let metadata = LOUDS.readBounded(metadataURL, maxBytes: maxFileBytes) else {
            if fileExist(metadataURL) { return ("metadata が大きすぎる", false) }
            return [charsURL, loudsURL, loudsTxt3FileURL("0", asTemporaryFile: false, directoryURL: directoryURL)].contains(where: fileExist) ? ("metadata が無い", true) : nil
        }
        var itemCounts: [Int] = []
        if let reason = metadata.withUnsafeBytes({ metadataCorruption($0, itemCounts: &itemCounts) }) { return (reason, false) }
        let n = itemCounts.count
        guard n > 0 else { return nil }
        guard let chars = LOUDS.readBounded(charsURL, maxBytes: maxFileBytes) else { return ("loudschars2 が読めない", true) }
        guard chars.count == n else { return ("loudschars2 の数が項目の数と合わない", true) }
        guard let louds = LOUDS.readBounded(loudsURL, maxBytes: maxFileBytes) else { return ("louds が読めない", true) }
        guard louds.count % 8 == 0, louds.count / 8 == (2 * n - 1 + 63) / 64 else { return ("louds の長さが項目の数と合わない", true) }
        return louds.withUnsafeBytes { loudsTreeCorruption($0, nodeCount: n) }.map { ($0, true) }
    }

    /// memory.memorymetadata: 項目の数 (UInt32)、項目ごとに 数 (UInt8) と 数 × MetadataElement。読めれば itemCounts に項目ごとの数を入れて nil
    private static func metadataCorruption(_ m: UnsafeRawBufferPointer, itemCounts: inout [Int]) -> String? {
        guard m.count >= 4 else { return "metadata の頭が短い" }
        let n = m.u32(0)
        guard n <= maxEntryCount else { return "項目の数が多すぎる" }
        itemCounts.reserveCapacity(n)
        let elementSize = MemoryLayout<MetadataElement>.size
        var offset = 4
        for _ in 0 ..< n {
            guard offset < m.count else { return "metadata が途中で切れている" }
            let count = Int(m[offset])
            offset += 1 + count * elementSize
            guard offset <= m.count else { return "metadata が途中で切れている" }
            itemCounts.append(count)
        }
        guard offset == m.count else { return "metadata の長さが項目の数と合わない" }
        return nil
    }

    /// memory<k>.loudstxt3 の 1 ファイル: 件数が itemCounts.count と合い、位置が範囲の中で逆順でなく、項目ごとの件数が metadata と合うか
    private static func loudstxt3Corruption(_ t: UnsafeRawBufferPointer, itemCounts: ArraySlice<Int>) -> String? {
        guard t.count >= 2 else { return "loudstxt3 の頭が短い" }
        let lc = t.u16(0)
        guard lc == itemCounts.count else { return "loudstxt3 の件数が合わない" }
        let headerEnd = 2 + 4 * lc
        guard t.count >= headerEnd else { return "loudstxt3 の位置の表が切れている" }
        var previous = headerEnd
        for (i, itemCount) in zip(0 ..< lc, itemCounts) {
            let start = t.u32(2 + 4 * i)
            let end = i == lc - 1 ? t.count : t.u32(2 + 4 * (i + 1))
            guard start >= previous, start <= end, end <= t.count else { return "loudstxt3 の位置が範囲の外か逆順" }
            guard end - start >= 2 else { return "loudstxt3 の項目が短い" }
            let elementCount = t.u16(start)
            guard 2 + 10 * elementCount <= end - start else { return "loudstxt3 の項目が切れている" }
            guard elementCount == itemCount else { return "loudstxt3 の件数が metadata と合わない" }
            previous = start
        }
        return nil
    }

    /// LOUDS の木のビット (先頭のビットから。節 0 の子の並び、節 1 の子の並び … を 1 の並びと 0 で書く) が、節 nodeCount 個の木か。
    /// 節 g (g ≥ 1) は g 個目の 1 で作られるので、節 g の並びを読み始める前に 1 が g 個以上あること。0 が nodeCount 個のあとは 1 だけ (埋め草)
    private static func loudsTreeCorruption(_ louds: UnsafeRawBufferPointer, nodeCount n: Int) -> String? {
        var ones = 0
        var zeros = 0
        for w in 0 ..< louds.count / 8 {
            let word = louds.u64(w * 8)
            for b in 0 ..< 64 {
                let bit = (word >> UInt64(63 - b)) & 1
                if zeros == n {
                    guard bit == 1 else { return "louds の終わりの埋め草が合わない" }
                    continue
                }
                if bit == 1 {
                    ones += 1
                } else {
                    zeros += 1
                    guard zeros == n || ones >= zeros else { return "louds の木の形が合わない" }
                }
            }
        }
        guard zeros == n, ones == n - 1 else { return "louds の 0 と 1 の数が合わない" }
        return nil
    }

    /// 学習のファイル (.2・.pause も) を、学習の置き場と同じ階層の memory_corrupt_<日時> に退ける。退けた先と、退けたファイルの数を返す
    static func quarantine(directoryURL: URL) throws -> (folder: URL, count: Int) {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyyMMdd-HHmmss"
        // Windows の Swift では既定の時間帯が世界時になるので、日本の時刻で (learning_reset.swift の控えと同じ)
        formatter.timeZone = TimeZone(identifier: "Asia/Tokyo") ?? .current
        let stamp = formatter.string(from: Date())
        let parent = directoryURL.deletingLastPathComponent()
        var folder = parent.appendingPathComponent("memory_corrupt_\(stamp)", isDirectory: true)
        var suffix = 2
        while FileManager.default.fileExists(atPath: folder.path) {
            folder = parent.appendingPathComponent("memory_corrupt_\(stamp)_\(suffix)", isDirectory: true)
            suffix += 1
        }
        let files = try FileManager.default.contentsOfDirectory(at: directoryURL, includingPropertiesForKeys: nil).filter(isLearningFile)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for file in files {
            try FileManager.default.moveItem(at: file, to: folder.appendingPathComponent(file.lastPathComponent))
        }
        return (folder, files.count)
    }

    /// 退ける学習のファイル: memory で始まる .louds・.loudschars2・.memorymetadata・.loudstxt3 (.2 も) と .pause。
    /// reset と違い、名前が memory で始まるものだけ (同じフォルダに利用者の辞書 user.* などがあっても動かさない)
    private static func isLearningFile(_ file: URL) -> Bool {
        let name = file.lastPathComponent
        if name == ".pause" { return true }
        return file.isFileURL && name.hasPrefix("memory")
            && [".loudstxt3", ".loudschars2", ".memorymetadata", ".louds", ".loudstxt3.2", ".loudschars2.2", ".memorymetadata.2", ".louds.2"].contains { name.hasSuffix($0) }
    }

    /// 壊れていたら退けて、知らせる。退けたら true
    @discardableResult
    static func quarantineIfCorrupt(directoryURL: URL, reason: String? = nil) -> Bool {
        guard let reason = reason ?? corruption(directoryURL: directoryURL) else { return false }
        do {
            let moved = try quarantine(directoryURL: directoryURL)
            LearningCorruption.report?(reason, moved.count, moved.folder.lastPathComponent)
        } catch {
            // 退けられなければ (ファイルを開いているなど)、学習のファイルを読まない (呼ぶ側が空の学習として扱う)
            LearningCorruption.report?(reason + " (退けられなかった)", 0, "")
        }
        return true
    }

    /// 変換が学習の LOUDS を読む前に呼ぶ。読んでよいなら true。
    /// 書きかけ (.pause がある) のときは、元のファイルが読める形なら読み、読めなければ読まない (次の保存で .2 から戻す。そこで戻せなければ退ける)
    /// louds・loudschars2 だけが合わないときは、退けずに今は読まない (起動のときの保存と、次の保存が metadata と loudstxt3 から作り直す。学習を失わない)
    static func prepareForReading(directoryURL: URL) -> Bool {
        guard let found = loadCorruption(directoryURL: directoryURL) else { return true }
        if !found.rebuildable && !fileExist(pauseFileURL(directoryURL: directoryURL)) {
            quarantineIfCorrupt(directoryURL: directoryURL, reason: found.reason)
        }
        return false
    }

    private static func BoolToUInt64(_ bools: [Bool]) -> [UInt64] {
        let unit = 64
        let value = bools.count.quotientAndRemainder(dividingBy: unit)
        let _bools = bools + [Bool].init(repeating: true, count: (unit - value.remainder) % unit)
        var result = [UInt64]()
        for i in 0...value.quotient {
            var value: UInt64 = 0
            for j in 0..<unit {
                value += (_bools[i * unit + j] ? 1:0) << (unit - j - 1)
            }
            result.append(value)
        }
        return result
    }

    /// - note:
    ///   この関数は出現数(`metadata.count`)と単語の長さ(`dicdata.ruby.count`)に基づいてvalueを決める。
    ///   出現数が大きいほどvalueは大きくなり、単語が長いほどvalueは大きくなる。
    ///   特に、単語の長さが1のとき、値域は`[-5, -8]`となる。一方単語の長さが2であれば値域は`[-3, -6]`であり、長さ4ならば`[-2, -5]`となる。
    fileprivate static func valueForData(metadata: MetadataElement, dicdata: DicdataElement, earlierRank: Int = 0, persisted: Bool = true) -> PValue {
        let d = 1 - Double(metadata.count) / 255
        // A tiny tie-breaker for the existing per-reading insertion order, not a recency boost.
        let established = 0.001 / Double(earlierRank + 1) - (persisted ? 0 : 0.001)
        return PValue(-1 - 4 / Double(dicdata.ruby.count) - 3 * pow(d, 3) + established)
    }

    fileprivate struct MetadataBlock {
        var metadata: [MetadataElement]

        func makeBinary() -> Data {
            var data = Data()
            var metadata: [MetadataElement] = self.metadata.map { MetadataElement(day: $0.lastUsedDay, count: $0.count) }
            // エントリのカウントを1byteでエンコード
            var count = UInt8(metadata.count)
            data.append(contentsOf: Data(bytes: &count, count: MemoryLayout<UInt8>.size))
            for i in metadata.indices {
                data.append(contentsOf: Data(bytes: &metadata[i], count: MemoryLayout<MetadataElement>.size))
            }
            return data
        }
    }

    fileprivate struct DataBlock {
        var count: Int {
            data.count
        }
        var ruby: String
        var data: [(word: String, lcid: Int, rcid: Int, mid: Int, score: PValue)]

        init(dicdata: [DicdataElement]) {
            self.ruby = ""
            self.data = []

            for element in dicdata {
                if self.ruby.isEmpty {
                    self.ruby = element.ruby
                }
                self.data.append((element.word, element.lcid, element.rcid, element.mid, element.value()))
            }
        }

        func makeLoudstxt3Entry() -> Data {
            var data = Data()
            // エントリのカウントを2byteでエンコード
            var count = UInt16(self.count)
            data.append(contentsOf: Data(bytes: &count, count: MemoryLayout<UInt16>.size))

            // 数値データ部をエンコード
            // 10byteが1つのエントリに対応するので、10*count byte
            for (_, lcid, rcid, mid, score) in self.data {
                assert(0 <= lcid && lcid <= UInt16.max)
                assert(0 <= rcid && rcid <= UInt16.max)
                assert(0 <= mid && mid <= UInt16.max)
                var lcid = UInt16(lcid)
                var rcid = UInt16(rcid)
                var mid = UInt16(mid)
                data.append(contentsOf: Data(bytes: &lcid, count: MemoryLayout<UInt16>.size))
                data.append(contentsOf: Data(bytes: &rcid, count: MemoryLayout<UInt16>.size))
                data.append(contentsOf: Data(bytes: &mid, count: MemoryLayout<UInt16>.size))
                var score = Float32(score)
                data.append(contentsOf: Data(bytes: &score, count: MemoryLayout<Float32>.size))
            }
            // wordをエンコード
            // 最先頭の要素はrubyになる
            let text = ([self.ruby] + self.data.map { $0.word == self.ruby ? "" : $0.word }).joined(separator: "\t")
            data.append(contentsOf: text.data(using: .utf8, allowLossyConversion: false)!)
            return data
        }
    }

    /// 関連するファイルを全て削除する
    static func reset(directoryURL: URL) throws {
        // 全削除する
        let fileURLs = try FileManager.default.contentsOfDirectory(at: directoryURL, includingPropertiesForKeys: nil)
        for file in fileURLs {
            if file.isFileURL && (
                // 学習データファイル
                file.path.hasSuffix(".loudstxt3")
                    || file.path.hasSuffix(".loudschars2")
                    || file.path.hasSuffix(".memorymetadata")
                    || file.path.hasSuffix(".louds")
                    // 一時ファイル
                    || file.path.hasSuffix(".loudstxt3.2")
                    || file.path.hasSuffix(".loudschars2.2")
                    || file.path.hasSuffix(".memorymetadata.2")
                    || file.path.hasSuffix(".louds.2")
                    // .pauseファイル
                    || file.path.hasSuffix(".pause")
                    // 古い学習機能のデータファイル
                    || file.path.hasSuffix("learningMemory.txt")
            ) {
                try FileManager.default.removeItem(at: file)
            }
        }
    }

    /// 一時記憶と長期記憶の学習データをマージする
    static let discardedWords = ["単漢字…", "姿陀"]

    static func merge(tempTrie: consuming TemporalLearningMemoryTrie, forgetTargets: [DicdataElement] = [], directoryURL: URL, maxMemoryCount: Int, char2UInt8: [Character: UInt8]) throws {
        // MARK: `.pause`ファイルが存在する場合、`merge`を行う前に`.2`ファイルの復活を試み、失敗した場合は`merge`を諦める。
        if fileExist(pauseFileURL(directoryURL: directoryURL)) {
            debug("LongTermLearningMemory merge collapsion detected, trying recovery...")
            do {
                try overwriteTempFiles(
                    directoryURL: directoryURL,
                    loudsFileTemp: nil,
                    loudsCharsFileTemp: nil,
                    metadataFileTemp: nil,
                    loudsTxt3FileCount: nil,
                    removingRead2File: true
                )
            } catch {
                // azooKey-Windows (2026-11-23、セキュリティの点検 12): 書きかけを戻せなければ (.2 が欠けているなど)、前は保存のたびにここで止まり、
                // 学習が二度と保存されなかった。書きかけのファイルごと退けて、空の学習から続ける
                quarantineIfCorrupt(directoryURL: directoryURL, reason: "書きかけの学習を戻せない")
            }
        }
        // MARK: ここで、前回のファイルの更新は問題なく成功していることが確認できる
        let startTime = Date()
        let today = LearningManager.today
        let recentTrie = consume tempTrie
        var newTrie = TemporalLearningMemoryTrie()
        // azooKey-Windows (2026-11-23、セキュリティの点検 12): merge が読む metadata と loudstxt3 を先に全部読み (大きさの上限つき)、
        // 範囲の外を読まない形か確かめてから使う。壊れていれば退けて、空の学習として合わせる (退けられなければ、保存せずにやめる。落ちるよりよい)。
        // louds と loudschars2 は merge では読まない (変換が読む前に prepareForReading で確かめる)
        var (ltMetadata, loudstxts) = readMergeInputs(directoryURL: directoryURL)
        if !skipMergeCheckForMeasurement, let reason = mergeInputCorruption(metadata: ltMetadata, loudstxts: loudstxts) {
            quarantineIfCorrupt(directoryURL: directoryURL, reason: reason)
            (ltMetadata, loudstxts) = readMergeInputs(directoryURL: directoryURL)
            if mergeInputCorruption(metadata: ltMetadata, loudstxts: loudstxts) != nil {
                throw UpdateError.corruptLearning
            }
        }
        // 構造:
        // dataCount(UInt32), count, data*count, count, data*count, ...
        // MARK: 読み出しは、`metadataFile`が存在しなかった場合（学習が一切ない場合）に失敗する。
        var metadataOffset = 0
        // 最初の4byteはentry countに対応する
        let entryCount = ltMetadata[metadataOffset ..< metadataOffset + 4].toArray(of: UInt32.self)[0]
        metadataOffset += 4

        debug("LongTermLearningMemory merge entryCount", entryCount, ltMetadata.count)

        // それぞれのloudstxt3ファイルに対して処理を行う
        for loudstxtIndex in 0 ..< Int(entryCount) / txtFileSplit + 1 {
            let loudstxtData: Data
            do {
                // azooKey-Windows (2026-11-23): 先に読んで確かめたもの (readMergeInputs)
                guard let data = loudstxts[loudstxtIndex] else { throw CocoaError(.fileNoSuchFile) }
                loudstxtData = data
            } catch {
                debug("LongTermLearningMemory merge failed to read \(loudstxtIndex)", error)
                continue
            }
            // loudstxt3の数
            let count = Int(loudstxtData[0 ..< 2].toArray(of: UInt16.self)[0])
            let indices = loudstxtData[2 ..< 2 + 4 * count].toArray(of: UInt32.self)
            for i in 0 ..< count {
                // メタデータの読み取り
                // 1byteで項目数
                let itemCount = Int(ltMetadata[metadataOffset ..< metadataOffset + 1].toArray(of: UInt8.self)[0])
                metadataOffset += 1
                let metadata = (0 ..< itemCount).map {
                    let range = metadataOffset + $0 * MemoryLayout<MetadataElement>.size ..< metadataOffset + ($0 + 1) * MemoryLayout<MetadataElement>.size
                    return ltMetadata[range].toArray(of: MetadataElement.self)[0]
                }
                metadataOffset += itemCount * MemoryLayout<MetadataElement>.size

                // バイナリ内部でのindex
                let startIndex = Int(indices[i])
                let endIndex = i == (indices.endIndex - 1) ? loudstxtData.endIndex : Int(indices[i + 1])
                let elements = LOUDS.parseBinary(binary: loudstxtData[startIndex ..< endIndex])
                // 該当部分を取り出してメタデータに従ってフィルター、trieに追加
                guard let ruby = elements.first?.ruby,
                      let chars = LearningManager.keyToChars(ruby, char2UInt8: char2UInt8) else {
                    continue
                }
                var newDicdata: [DicdataElement] = []
                var newMetadata: [MetadataElement] = []
                assert(elements.count == metadata.count, "elements count and metadata count must be equal.")
                for (dicdataElement, metadataElement) in zip(elements, metadata) {
                    // 忘却対象である場合は弾く
                    // azooKey-Windows: 候補一覧の項目名 (単漢字…) や誤変換にしかならない語を覚えてしまっていたら、保存のときに捨てる
                    if forgetTargets.contains(dicdataElement) || (Self.discardedWords + LearningDiscard.words).contains(where: { dicdataElement.word.contains($0) }) {
                        debug("LongTermLearningMemory merge stopped because it is a forget target", dicdataElement)
                        continue
                    }
                    if ruby != dicdataElement.ruby {
                        debug("LongTermLearningMemory merge stopped because dicdataElement has different ruby", dicdataElement, ruby)
                        continue
                    }
                    var metadataElement = metadataElement
                    if today < metadataElement.lastUpdatedDay || today < metadataElement.lastUsedDay {
                        // 変なデータが入っているとアンダーフローが起こるため、明示的に新しいデータを入れ直す
                        metadataElement = MetadataElement(day: today, count: 1)
                    }
                    guard today - metadataElement.lastUsedDay < 128 else {
                        // 128日以上使っていない単語は除外
                        debug("LongTermLearningMemory merge stopped because metadata is strange", dicdataElement, metadataElement, today)
                        continue
                    }
                    var dicdataElement = dicdataElement
                    // 32日ごとにカウントを半減させる
                    while today - metadataElement.lastUpdatedDay > 32 {
                        metadataElement.count >>= 1
                        metadataElement.lastUpdatedDay += 32
                    }
                    // カウントがゼロになる場合除外
                    guard metadataElement.count > 0 else {
                        debug("LongTermLearningMemory merge stopped because count is zero", dicdataElement, metadataElement)
                        continue
                    }
                    dicdataElement.baseValue = valueForData(metadata: metadataElement, dicdata: dicdataElement)
                    newDicdata.append(dicdataElement)
                    newMetadata.append(metadataElement)
                }
                newTrie.append(dicdata: newDicdata, chars: chars, metadata: newMetadata)
            }
            // メモリ数上限を超過した場合、長いものから捨てる
            if newTrie.dicdata.count > maxMemoryCount {
                break
            }
        }
        // Preserve persisted insertion order; new learning updates counts without jumping the queue.
        let activeIndices = recentTrie.nodes.flatMap(\.dataIndices).sorted()
        for index in activeIndices {
            let element = recentTrie.dicdata[index]
            guard let chars = LearningManager.keyToChars(element.ruby, char2UInt8: char2UInt8) else { continue }
            newTrie.append(dicdata: [element], chars: chars, metadata: [recentTrie.metadata[index]])
        }
        // newTrieのデータからLOUDSを作り書き出す
        try self.update(trie: newTrie, directoryURL: directoryURL)
        debug("LongTermLearningMemory merge ⏰", Date().timeIntervalSince(startTime), newTrie.dicdata.count)
    }

    fileprivate static func make_loudstxt3(lines: [DataBlock]) -> Data {
        let lc = lines.count    // データ数
        let count = Data(bytes: [UInt16(lc)], count: 2) // データ数をUInt16でマップ

        let data = lines.map { $0.makeLoudstxt3Entry() }
        let body = data.reduce(Data(), +)   // データ

        let header_endIndex: UInt32 = 2 + UInt32(lc) * UInt32(MemoryLayout<UInt32>.size)
        let headerArray = data.dropLast().reduce(into: [header_endIndex]) {array, value in // ヘッダの作成
            array.append(array.last! + UInt32(value.count))
        }

        let header = Data(bytes: headerArray, count: MemoryLayout<UInt32>.size * headerArray.count)
        let binary = count + header + body

        return binary
    }

    enum UpdateError: Error {
        /// `.pause`が存在するため更新を停止する場合
        case pauseFileExist
        /// azooKey-Windows (2026-11-23): 壊れた学習のファイルを退けられなかった場合
        case corruptLearning
    }

    /// ファイルを安全に書き出すため、以下の手順を取る
    ///
    /// 1. 各ファイルを`memory.louds.2`のように書き出す
    /// 2. `.pause`を書き出す
    /// 3. それぞれの`.2`を元ファイルの位置にコピーする
    /// 4. `.pause`を削除する
    ///
    /// このとき、読み出し側では
    /// * `.pause`がない場合、`.2`のつかないファイルを読み出す。
    /// * `.pause`がある場合、適当なタイミングで上記ステップの`3`以降を再実行する。また、`.pause`がある場合、学習機能を停止する。
    ///
    /// 上記手順では`.pause`がない間は`.2`のつかないファイルが整合性を保っており、`.pause`がある場合は`.2`のつくファイルが整合性を保っているため、常に整合性を保ったファイルを維持することができる。
    ///
    /// 例えば1のステップの実行中にエラーが生じた場合、次回キーボードを開いた際は単に更新前のファイルを読み込む。
    ///
    /// 3のステップの実行中にエラーが生じた場合、次回キーボードを開いた際は学習を停止状態にする。ついで閉じる際に再度ステップ3を実行することで、安全に全てのファイルを更新することができる。
    static func update(trie: TemporalLearningMemoryTrie, directoryURL: URL) throws {
        // MARK: `.pause`の存在を確認し、存在していれば失敗させる
        // この場合、先に復活作業を実施すべきである
        guard !fileExist(pauseFileURL(directoryURL: directoryURL)) else {
            throw UpdateError.pauseFileExist
        }

        // MARK: 各ファイルを`.2`で書き出す
        var nodes2Characters: [UInt8] = [0x0, 0x0]
        var dicdata: [DataBlock] = [.init(dicdata: []), .init(dicdata: [])]
        var metadata: [MetadataBlock] = [.init(metadata: []), .init(metadata: [])]
        var bits: [Bool] = [true, false]
        var currentNodes: [(UInt8, Int)] = trie.nodes[0].children.sorted(by: {$0.key < $1.key})
        bits += [Bool](repeating: true, count: currentNodes.count) + [false]
        while !currentNodes.isEmpty {
            currentNodes.forEach {char, nodeIndex in
                nodes2Characters.append(char)
                dicdata.append(DataBlock(dicdata: trie.nodes[nodeIndex].dataIndices.map {trie.dicdata[$0]}))
                metadata.append(MetadataBlock(metadata: trie.nodes[nodeIndex].dataIndices.map {trie.metadata[$0]}))

                bits += [Bool](repeating: true, count: trie.nodes[nodeIndex].children.count) + [false]
            }
            currentNodes = currentNodes.flatMap {(_, nodeIndex) in trie.nodes[nodeIndex].children.sorted(by: {$0.key < $1.key})}
        }

        let bytes = Self.BoolToUInt64(bits)
        let loudsFileTemp = loudsFileURL(asTemporaryFile: true, directoryURL: directoryURL)
        do {
            let binary = Data(bytes: bytes, count: bytes.count * 8)
            try binary.write(to: loudsFileTemp)
        }

        let loudsCharsFileTemp = loudsCharsFileURL(asTemporaryFile: true, directoryURL: directoryURL)
        do {
            let binary = Data(bytes: nodes2Characters, count: nodes2Characters.count)
            try binary.write(to: loudsCharsFileTemp)
        }
        let metadataFileTemp = metadataFileURL(asTemporaryFile: true, directoryURL: directoryURL)
        do {
            let binary = Data(bytes: [UInt32(metadata.count)], count: 4) // エントリ数をUInt32でマップ
            let result = metadata.reduce(into: binary) {
                $0.append(contentsOf: $1.makeBinary())
            }
            try result.write(to: metadataFileTemp)
        }

        let loudsTxt3FileCount: Int
        do {
            loudsTxt3FileCount = ((dicdata.count) / txtFileSplit) + 1
            let indiceses: [Range<Int>] = (0..<loudsTxt3FileCount).map {
                let start = $0 * txtFileSplit
                let _end = ($0 + 1) * txtFileSplit
                let end = dicdata.count < _end ? dicdata.count:_end
                return start..<end
            }

            for indices in indiceses {
                do {
                    let start = indices.startIndex / txtFileSplit
                    let binary = make_loudstxt3(lines: Array(dicdata[indices]))
                    try binary.write(to: loudsTxt3FileURL("\(start)", asTemporaryFile: true, directoryURL: directoryURL), options: .atomic)
                }
            }
        }

        // MARK: `.pause`ファイルを書き出す
        try Data().write(to: pauseFileURL(directoryURL: directoryURL))

        // MARK: 各`.2`のファイルで元のファイルを上書きする
        try overwriteTempFiles(
            directoryURL: directoryURL,
            loudsFileTemp: loudsFileTemp,
            loudsCharsFileTemp: loudsCharsFileTemp,
            metadataFileTemp: metadataFileTemp,
            loudsTxt3FileCount: loudsTxt3FileCount,
            // MARK: 成功の場合、`.pause`ファイルも削除する
            removingRead2File: true
        )
    }

    /// - note: 上書きが全て成功するまで、一時ファイルは削除してはいけない。安全のため、`.pause`を除きそもそも一時ファイルを一切削除しないようにする。
    private static func overwriteTempFiles(directoryURL: URL, loudsFileTemp: URL?, loudsCharsFileTemp: URL?, metadataFileTemp: URL?, loudsTxt3FileCount: Int?, removingRead2File: Bool) throws {
        try overwrite(
            from: loudsCharsFileTemp ?? loudsCharsFileURL(asTemporaryFile: true, directoryURL: directoryURL),
            to: loudsCharsFileURL(asTemporaryFile: false, directoryURL: directoryURL)
        )
        try overwrite(
            from: metadataFileTemp ?? metadataFileURL(asTemporaryFile: true, directoryURL: directoryURL),
            to: metadataFileURL(asTemporaryFile: false, directoryURL: directoryURL)
        )
        if let loudsTxt3FileCount {
            for i in  0 ..< loudsTxt3FileCount {
                try overwrite(
                    from: loudsTxt3FileURL("\(i)", asTemporaryFile: true, directoryURL: directoryURL),
                    to: loudsTxt3FileURL("\(i)", asTemporaryFile: false, directoryURL: directoryURL)
                )
            }
        } else {
            let fileURLs = try FileManager.default.contentsOfDirectory(at: directoryURL, includingPropertiesForKeys: nil)
            for file in fileURLs {
                if file.isFileURL && file.path.hasSuffix(".loudstxt3.2") {
                    try overwrite(from: file, to: URL(fileURLWithPath: String(file.path.dropLast(2))))
                }
            }
        }
        // 読み出し側で最初に読み出されるのは`.louds`なので、これを最後に書き出す方が安全
        try overwrite(
            from: loudsFileTemp ?? loudsFileURL(asTemporaryFile: true, directoryURL: directoryURL),
            to: loudsFileURL(asTemporaryFile: false, directoryURL: directoryURL)
        )
        if removingRead2File {
            try FileManager.default.removeItem(at: pauseFileURL(directoryURL: directoryURL))
        }
    }
}

/// 一時記憶用のデータなので、複雑な形状にしない。
struct TemporalLearningMemoryTrie {
    struct Node {
        var dataIndices: [Int] = []      // loudstxt3の中のデータのインデックスリスト
        var children: [UInt8: Int] = [:] // characterのIDからインデックスへのマッピング
    }

    fileprivate var nodes = [Node()]
    fileprivate var dicdata: [DicdataElement] = []
    fileprivate var metadata: [MetadataElement] = []

    /// 同じノードにあることがわかっているデータを一括で追加する場面で利用する関数
    /// 主にマージ時の利用を想定
    fileprivate mutating func append(dicdata: [DicdataElement], chars: [UInt8], metadata: [MetadataElement]) {
        assert(dicdata.count == metadata.count, "count of dicdata and metadata do not match")
        var index = 0
        for char in chars {
            if let nextIndex = nodes[index].children[char] {
                index = nextIndex
            } else {
                let nextIndex = nodes.endIndex
                nodes[index].children[char] = nextIndex
                nodes.append(Node())
                index = nextIndex
            }
        }
        for (dicdataElement, metadataElement) in zip(dicdata, metadata) {
            if let dataIndex = nodes[index].dataIndices.first(where: {Self.sameDicdataIfRubyIsEqual(left: self.dicdata[$0], right: dicdataElement)}) {
                // すでにnodes[index]に同じデータが存在している場合、カウントを加算し、最後に使った日を後の方に変更する
                withMutableValue(&self.metadata[dataIndex]) { currentMetadata in
                    currentMetadata.lastUsedDay = max(currentMetadata.lastUsedDay, metadataElement.lastUsedDay)
                    currentMetadata.lastUpdatedDay = max(currentMetadata.lastUpdatedDay, metadataElement.lastUpdatedDay)
                    currentMetadata.count += min(.max - currentMetadata.count, metadataElement.count)
                }
                self.dicdata[dataIndex] = dicdataElement
                // valueを更新する
                let rank = nodes[index].dataIndices.firstIndex(of: dataIndex) ?? 0
                self.dicdata[dataIndex].baseValue = LongTermLearningMemory.valueForData(metadata: self.metadata[dataIndex], dicdata: dicdataElement, earlierRank: rank)
                self.dicdata[dataIndex].adjust = 0
                self.dicdata[dataIndex].metadata = .isLearned
            } else {
                // まだnodes[index]に同じデータが存在していない場合、data末尾に新しい要素を追加してnodes[index]を更新する
                let dataIndex = self.dicdata.endIndex
                self.dicdata.append(dicdataElement)
                self.metadata.append(metadataElement)
                nodes[index].dataIndices.append(dataIndex)
                let rank = nodes[index].dataIndices.count - 1
                self.dicdata[dataIndex].baseValue = LongTermLearningMemory.valueForData(metadata: metadataElement, dicdata: dicdataElement, earlierRank: rank)
                self.dicdata[dataIndex].adjust = 0
                self.dicdata[dataIndex].metadata = .isLearned
            }
        }
    }

    /// ルビが同じだとわかっている場合に2つのDicdataElementが同じものか判定する関数
    private static func sameDicdataIfRubyIsEqual(left: DicdataElement, right: DicdataElement) -> Bool {
        left.lcid == right.lcid && left.rcid == right.rcid && left.word == right.word
    }

    mutating func memorize(dicdataElement: DicdataElement, chars: [UInt8]) {
        var index = 0
        for char in chars {
            if let nextIndex = nodes[index].children[char] {
                index = nextIndex
            } else {
                let nextIndex = nodes.endIndex
                nodes[index].children[char] = nextIndex
                nodes.append(Node())
                index = nextIndex
            }
        }
        // 雑な設定だが200年くらいは持つのでヨシ。
        let day = LearningManager.today
        if let dataIndex = nodes[index].dataIndices.first(where: {Self.sameDicdataIfRubyIsEqual(left: self.dicdata[$0], right: dicdataElement)}) {
            withMutableValue(&self.metadata[dataIndex]) {
                $0.count += min(.max - $0.count, 1)
                $0.lastUsedDay = day
            }
            // adjustを更新する
            let rank = nodes[index].dataIndices.firstIndex(of: dataIndex) ?? 0
            self.dicdata[dataIndex].adjust = LongTermLearningMemory.valueForData(metadata: self.metadata[dataIndex], dicdata: dicdataElement, earlierRank: rank, persisted: false) - dicdataElement.baseValue
            self.dicdata[dataIndex].metadata = .isLearned
        } else {
            let dataIndex = self.dicdata.endIndex
            var dicdataElement = dicdataElement
            let metadataElement = MetadataElement(day: day, count: 1)
            // adjustを更新する
            let rank = nodes[index].dataIndices.count
            dicdataElement.adjust = LongTermLearningMemory.valueForData(metadata: metadataElement, dicdata: dicdataElement, earlierRank: rank, persisted: false) - dicdataElement.baseValue
            dicdataElement.metadata = .isLearned
            self.dicdata.append(dicdataElement)
            self.metadata.append(metadataElement)
            nodes[index].dataIndices.append(dataIndex)
        }
    }

    @discardableResult
    mutating func forget(dicdataElement: DicdataElement, chars: [UInt8]) -> Bool {
        var index = 0
        for char in chars {
            if let nextIndex = nodes[index].children[char] {
                index = nextIndex
            } else {
                // 存在しない場合
                return false
            }
        }
        // 存在する場合
        // dataIndicesから削除する(dicdataの方は触らない)
        nodes[index].dataIndices.removeAll(where: {self.dicdata[$0] == dicdataElement})
        return true
    }

    func perfectMatch(chars: [UInt8]) -> [DicdataElement] {
        var index = 0
        for char in chars {
            if let nextIndex = nodes[index].children[char] {
                index = nextIndex
            } else {
                return []
            }
        }
        return nodes[index].dataIndices.map {self.dicdata[$0]}
    }

    func throughMatch(chars: [UInt8], depth: Range<Int>) -> [DicdataElement] {
        var index = 0
        var indices: [Int] = []
        for (offset, char) in chars.enumerated() {
            if let nextIndex = nodes[index].children[char] {
                index = nextIndex
                if depth.contains(offset) {
                    indices.append(contentsOf: nodes[index].dataIndices)
                }
            } else {
                return indices.map {self.dicdata[$0]}
            }
        }
        return indices.map {self.dicdata[$0]}
    }

    func prefixMatch(chars: [UInt8]) -> [DicdataElement] {
        var index = 0
        for char in chars {
            if let nextIndex = nodes[index].children[char] {
                index = nextIndex
            } else {
                return []
            }
        }
        var nodeIndices: [Int] = Array(nodes[index].children.values)
        var indices: [Int] = nodes[index].dataIndices
        while let index = nodeIndices.popLast() {
            nodeIndices.append(contentsOf: nodes[index].children.values)
            indices.append(contentsOf: nodes[index].dataIndices)
        }
        return indices.map {self.dicdata[$0]}
    }
}

final class LearningManager {
    private static func updateChar2Int8(bundleURL: URL, target: inout [Character: UInt8]) {
        do {
            let chidURL = bundleURL.appendingPathComponent("louds/charID.chid", isDirectory: false)
            let string = try String(contentsOf: chidURL, encoding: .utf8)
            target = [Character: UInt8].init(uniqueKeysWithValues: string.enumerated().map {($0.element, UInt8($0.offset))})
        } catch {
            debug("Error: louds/charID.chidが存在しません。このエラーは深刻ですが、テスト時には無視できる場合があります。Description: \(error)")
        }
    }
    private var char2UInt8: [Character: UInt8] = [:]

    static var today: UInt16 {
        UInt16(Int(Date().timeIntervalSince1970) / 86400) - 19000
    }

    static func keyToChars(_ key: some StringProtocol, char2UInt8: [Character: UInt8]) -> [UInt8]? {
        var chars: [UInt8] = []
        chars.reserveCapacity(key.count)
        for character in key {
            if let char = char2UInt8[character] {
                chars.append(char)
            } else {
                return nil
            }
        }
        return chars
    }

    private var temporaryMemory: TemporalLearningMemoryTrie = .init()
    private var options: ConvertRequestOptions = .default
    private var memoryCollapsed: Bool = false

    var enabled: Bool {
        (!self.memoryCollapsed) && self.options.learningType.needUsingMemory
    }

    init() {
        self.memoryCollapsed = LongTermLearningMemory.memoryCollapsed(directoryURL: self.options.memoryDirectoryURL)
        if memoryCollapsed {
            // 学習データが壊れている状態であることを警告する
            debug("LearningManager init: Memory Collapsed")
        }
        if !options.learningType.needUsingMemory {
            return
        }
        Self.updateChar2Int8(bundleURL: options.dictionaryResourceURL, target: &char2UInt8)
    }

    /// - Returns: Whether cache should be reseted or not.
    func setRequestOptions(options: ConvertRequestOptions) -> Bool {
        // 変更があったら`char2Int8`を読み込み直す
        if options.dictionaryResourceURL != self.options.dictionaryResourceURL {
            Self.updateChar2Int8(bundleURL: options.dictionaryResourceURL, target: &char2UInt8)
        }
        self.options = options

        switch options.learningType {
        case .inputAndOutput, .onlyOutput: break
        case .nothing:
            self.temporaryMemory = TemporalLearningMemoryTrie()
        }

        // リセットチェックも実施
        if options.shouldResetMemory {
            self.reset()
            self.options.shouldResetMemory = false
            return true
        }
        return false
    }

    func temporaryPerfectMatch(charIDs: [UInt8]) -> [DicdataElement] {
        if !options.learningType.needUsingMemory {
            return []
        }
        return self.temporaryMemory.perfectMatch(chars: charIDs)
    }

    func temporaryThroughMatch(charIDs: [UInt8], depth: Range<Int>) -> [DicdataElement] {
        if !options.learningType.needUsingMemory {
            return []
        }
        return self.temporaryMemory.throughMatch(chars: charIDs, depth: depth)
    }

    func temporaryPrefixMatch(charIDs: [UInt8]) -> [DicdataElement] {
        if !options.learningType.needUsingMemory {
            return []
        }
        return self.temporaryMemory.prefixMatch(chars: charIDs)
    }

    func update(data: [DicdataElement]) {
        self.update(data: [], updatePart: data)
    }

    /// `updatePart`のみを更新する。`data`の部分は更新しない。
    func update(data: [DicdataElement], updatePart: [DicdataElement]) {
        if !options.learningType.needUpdateMemory {
            return
        }
        // 単語単位
        for datum in updatePart where DicdataStore.needWValueMemory(datum) {
            guard let chars = Self.keyToChars(datum.ruby, char2UInt8: char2UInt8) else {
                continue
            }
            self.temporaryMemory.memorize(dicdataElement: datum, chars: chars)
        }

        if data.count + updatePart.count == 1 {
            return
        }
        // 文節単位bigram
        do {
            var firstClause: DicdataElement?
            var secondClause: DicdataElement?
            for (datum, index) in zip(data.chained(updatePart), 0 ..< data.count + updatePart.count) {
                if var newFirstClause = firstClause {
                    if var newSecondClause = secondClause {
                        if DicdataStore.isClause(newFirstClause.rcid, datum.lcid) {
                            // 更新対象のindexでなければcontinueする
                            guard data.endIndex <= index else {
                                continue
                            }
                            // firstClauseとsecondClauseがあって文節境界である場合, bigramを作って学習に入れる
                            let element = DicdataElement(
                                word: newFirstClause.word + newSecondClause.word,
                                ruby: newFirstClause.ruby + newSecondClause.ruby,
                                lcid: newFirstClause.lcid,
                                rcid: newFirstClause.rcid,
                                mid: newSecondClause.mid,
                                value: newFirstClause.baseValue + newSecondClause.baseValue
                            )
                            // firstClauseを押し出す
                            firstClause = secondClause
                            secondClause = datum
                            guard let chars = Self.keyToChars(element.ruby, char2UInt8: char2UInt8) else {
                                continue
                            }
                            debug("LearningManager update first/second", element)
                            self.temporaryMemory.memorize(dicdataElement: element, chars: chars)
                        } else {
                            // firstClauseとsecondClauseがあって文節境界でない場合, secondClauseをアップデート
                            newSecondClause.word.append(contentsOf: datum.word)
                            newSecondClause.ruby.append(contentsOf: datum.ruby)
                            newSecondClause.rcid = datum.rcid
                            if DicdataStore.includeMMValueCalculation(datum) {
                                newSecondClause.mid = datum.mid
                            }
                            newSecondClause.baseValue += datum.baseValue
                            secondClause = newSecondClause
                        }
                    } else {
                        if DicdataStore.isClause(newFirstClause.rcid, datum.lcid) {
                            // firstClauseがあって文節境界である場合, secondClauseを作る
                            secondClause = datum
                        } else {
                            // firstClauseがあって文節境界でない場合, firstClauseをアップデート
                            newFirstClause.word.append(contentsOf: datum.word)
                            newFirstClause.ruby.append(contentsOf: datum.ruby)
                            newFirstClause.rcid = datum.rcid
                            if DicdataStore.includeMMValueCalculation(datum) {
                                newFirstClause.mid = datum.mid
                            }
                            newFirstClause.baseValue += datum.baseValue
                            firstClause = newFirstClause
                        }
                    }
                } else {
                    firstClause = datum
                }
            }
            if let firstClause, let secondClause {
                let element = DicdataElement(
                    word: firstClause.word + secondClause.word,
                    ruby: firstClause.ruby + secondClause.ruby,
                    lcid: firstClause.lcid,
                    rcid: firstClause.rcid,
                    mid: secondClause.mid,
                    value: firstClause.baseValue + secondClause.baseValue
                )
                if let chars = Self.keyToChars(element.ruby, char2UInt8: char2UInt8) {
                    debug("LearningManager update first/second rest", element)
                    self.temporaryMemory.memorize(dicdataElement: element, chars: chars)
                }
            }
        }
        // 全体
        let data = data.chained(updatePart)
        let element = DicdataElement(
            word: data.reduce(into: "") {$0.append(contentsOf: $1.word)},
            ruby: data.reduce(into: "") {$0.append(contentsOf: $1.ruby)},
            lcid: data.first?.lcid ?? CIDData.一般名詞.cid,
            rcid: data.last?.rcid ?? CIDData.一般名詞.cid,
            mid: data.last?.mid ?? MIDData.一般.mid,
            value: data.reduce(into: 0) {$0 += $1.baseValue}
        )
        guard let chars = Self.keyToChars(element.ruby, char2UInt8: char2UInt8) else {
            return
        }
        debug("LearningManager update all", element)
        self.temporaryMemory.memorize(dicdataElement: element, chars: chars)
    }

    /// データに含まれる語彙の学習をリセットする関数
    func forgetMemory(data: [DicdataElement]) {
        // 1. temporary memoryを削除する
        for element in data {
            guard let chars = Self.keyToChars(element.ruby, char2UInt8: char2UInt8) else {
                continue
            }
            self.temporaryMemory.forget(dicdataElement: element, chars: chars)
        }
        // 2. longterm memoryを削除する
        do {
            try LongTermLearningMemory.merge(tempTrie: self.temporaryMemory, forgetTargets: data, directoryURL: self.options.memoryDirectoryURL, maxMemoryCount: options.maxMemoryCount, char2UInt8: char2UInt8)
            // マージが済んだので、temporaryMemoryを空にする
            self.temporaryMemory = TemporalLearningMemoryTrie()
        } catch {
            // アップデートに失敗した場合、そのまま諦める。
            debug("LearningManager resetLearning: Failed to save LongTermLearningMemory", error)
        }
        // 状態を更新する
        self.memoryCollapsed = LongTermLearningMemory.memoryCollapsed(directoryURL: self.options.memoryDirectoryURL)
    }

    func save() {
        if !options.learningType.needUpdateMemory {
            return
        }
        do {
            try LongTermLearningMemory.merge(tempTrie: self.temporaryMemory, directoryURL: self.options.memoryDirectoryURL, maxMemoryCount: options.maxMemoryCount, char2UInt8: char2UInt8)
            // マージが済んだので、temporaryMemoryを空にする
            self.temporaryMemory = TemporalLearningMemoryTrie()
        } catch {
            // アップデートに失敗した場合、そのまま諦める。
            debug("LearningManager save: Failed to save LongTermLearningMemory", error)
        }
        // 状態を更新する
        self.memoryCollapsed = LongTermLearningMemory.memoryCollapsed(directoryURL: self.options.memoryDirectoryURL)
    }

    func reset() {
        self.temporaryMemory = TemporalLearningMemoryTrie()
        do {
            try LongTermLearningMemory.reset(directoryURL: self.options.memoryDirectoryURL)
        } catch {
            debug("LearningManager reset failed", error)
        }
    }
}
