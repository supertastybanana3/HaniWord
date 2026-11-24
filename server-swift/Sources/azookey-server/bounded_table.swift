import Foundation

/// 2026-10-22 (D の 2 回目の調べ D2-2・M-1): 上限つきの表の読み込み。変換サーバーが読む表 (分野辞書の domains.txt・always.txt・
/// 優遇・目印・値の上書き、誤用の表など) は利用者の場所 (%APPDATA%\HaniWord) にあるので、壊れた・大きなファイルでも止まらないようにする。
///   - 読む前にファイルの大きさを確かめ、maxBytes を超えたら何も読まない。UTF-8 で読めなければ何も読まない
///   - # で始まる行と空行は飛ばす。タブ以外の制御文字を含む行は捨てる
///   - 欄 (タブ区切り) の数が columns に入らない行・maxFieldLength 字を超える欄のある行は捨てる
///   - maxLines 行を取ったらそこで止める
/// 表を足すときは、ここを通して読む (表ごとに上限を書き足さない)
enum BoundedTable {
    /// ファイルの中身 (UTF-8)。大きさが maxBytes を超える・読めない・UTF-8 でなければ nil
    static func text(_ url: URL, maxBytes: Int) -> String? {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        guard let size = try? handle.seekToEnd(), size <= UInt64(maxBytes), (try? handle.seek(toOffset: 0)) != nil,
              let data = try? handle.read(upToCount: Int(size) + 1), data.count <= maxBytes else { return nil }
        return String(data: data, encoding: .utf8)
    }

    /// 表の行 (欄の並び)。ファイルが無い・大きすぎる・読めなければ空
    static func rows(_ url: URL, maxBytes: Int = 8 << 20, maxLines: Int = 200_000, columns: ClosedRange<Int>, maxFieldLength: Int = 256) -> [[String]] {
        guard let text = text(url, maxBytes: maxBytes) else { return [] }
        return rows(text: text, maxLines: maxLines, columns: columns, maxFieldLength: maxFieldLength)
    }

    static func rows(text: String, maxLines: Int, columns: ClosedRange<Int>, maxFieldLength: Int) -> [[String]] {
        var out: [[String]] = []
        for line in text.split(whereSeparator: \.isNewline) {
            if out.count >= maxLines { break }
            if line.hasPrefix("#") { continue }
            if line.unicodeScalars.contains(where: { $0.properties.generalCategory == .control && $0 != "\t" }) { continue }
            let cols = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
            guard columns.contains(cols.count), cols.allSatisfy({ $0.count <= maxFieldLength }) else { continue }
            out.append(cols)
        }
        return out
    }

    /// 2026-11-02: 10 万行ほどの大きな表 (難読語の同音語の表) を速く読む。決まりは rows と同じで、字をスカラー値で扱う
    /// (欄の長さもスカラー値で数える)。行ごとに欄 (スカラー値の並び) を body に渡す。rows は字 (書記素) で行と欄を切るので、16 万行で 1 秒を超えていた
    static func scalarRows(text: String, maxLines: Int, columns: ClosedRange<Int>, maxFieldLength: Int,
                           _ body: ([ArraySlice<Unicode.Scalar>]) -> Void) {
        let scalars = Array(text.unicodeScalars)
        func isNewline(_ v: UInt32) -> Bool {
            switch v {
            case 0x0A...0x0D, 0x85, 0x2028, 0x2029: return true
            default: return false
            }
        }
        var taken = 0, start = 0
        var fields: [ArraySlice<Unicode.Scalar>] = []
        while start < scalars.count && taken < maxLines {
            var end = start, control = false
            while end < scalars.count && !isNewline(scalars[end].value) {
                let v = scalars[end].value
                if (v < 0x20 && v != 0x09) || (0x7F...0x9F).contains(v) { control = true }
                end += 1
            }
            if end > start && scalars[start] != "#" && !control {
                fields.removeAll(keepingCapacity: true)
                var fieldStart = start
                for i in start...end where i == end || scalars[i] == "\t" {
                    fields.append(scalars[fieldStart..<i])
                    fieldStart = i + 1
                }
                if columns.contains(fields.count) && fields.allSatisfy({ $0.count <= maxFieldLength }) {
                    body(fields)
                    taken += 1
                }
            }
            start = end + 1
        }
    }

    /// 変換の重みの欄。有限で range に入るときだけ
    static func value(_ field: String, range: ClosedRange<Float> = -60 ... 0) -> Float? {
        guard let v = Float(field.trimmingCharacters(in: .whitespaces)), v.isFinite, range.contains(v) else { return nil }
        return v
    }
}
