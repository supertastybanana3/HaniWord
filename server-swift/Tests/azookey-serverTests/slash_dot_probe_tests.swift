import Foundation
import XCTest
@testable import azookey_server

/// 2026-11-05 (仕事 22 の 2): / と 。 の決め直しを、変換サーバーの変換の道 (ConvertSegments、辞書だけ・速くない = Space の変換と同じ決め直し) で確かめる。
/// AZK_SLASH_DOT_STAGE (辞書の置き場。memory・shared はここに作る)・AZK_SLASH_DOT_CASES (読み<TAB>正しい文 の組のファイル、; で区切って複数)・
/// AZK_SLASH_DOT_OUT (出力のフォルダ) があるときだけ。組ごとに <出力>/<組の名前>.out.tsv に 読み<TAB>正しい文<TAB>変換 を書く。
/// 設定は APPDATA の HaniWord\settings.json (AI をオフにした試験用の置き場を渡す)。AZK_SLASH_DOT=0 なら決め直さない (前の形)
@MainActor
final class SlashDotProbeTests: XCTestCase {
    func testSlashDotConversionProbe() throws {
        let env = ProcessInfo.processInfo.environment
        guard let stage = env["AZK_SLASH_DOT_STAGE"], let cases = env["AZK_SLASH_DOT_CASES"], let out = env["AZK_SLASH_DOT_OUT"] else {
            throw XCTSkip("Run with a dictionary stage and case files")
        }
        stage.withCString { initialize(path: $0, use_zenzai: false) }
        for path in cases.split(separator: ";").map(String.init) where !path.isEmpty {
            var lines: [String] = []
            for line in try String(contentsOfFile: path, encoding: .utf8).split(whereSeparator: \.isNewline) {
                let parts = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
                guard parts.count >= 2, !parts[0].isEmpty else { continue }
                var count: Int32 = 0
                let list = parts[0].withCString { reading in "".withCString { context in
                    convert_segments(hiraganaPtr: reading, contextPtr: context, fast: false, dictionaryOnly: true, lengthPtr: &count)
                } }
                var text = ""
                for i in 0..<Int(count) {
                    if let c = list[i] { text += String(cString: c.pointee.text) }
                }
                lines.append("\(parts[0])\t\(parts[1])\t\(text)")
            }
            let name = URL(filePath: path).lastPathComponent
            try (lines.joined(separator: "\n") + "\n").write(toFile: out + "/" + name + ".out.tsv", atomically: true, encoding: .utf8)
        }
    }
}
