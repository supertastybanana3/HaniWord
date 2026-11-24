import Foundation
import KanaKanjiConverterModule

/// 2026-11-18 (perf の 7 の続き、まとめ役 20:1x): 本体の辞書の読み方。冷えた起動 (入れた直後・写し直した直後) を測ると、前の形の温めの変換 5.7〜7.1 秒のうち、
/// 本体の辞書の先読み (preloadDictionary。Dictionary/louds の LOUDS 160 と .loudstxt3 438 を、最初の変換の前に主のスレッドで全部読む) が 3.8〜4.9 秒だった
/// (t64/cold2.jsonl。ファイルを初めて開くときのディスクの読みと Defender の調べ)。3 つの形を測って決める (開発用のビルドだけ AZK_DICT_MODE で替える):
/// - preload: 前の形
/// - lazy: 先読みせず、引いた頭の字の LOUDS と .loudstxt3 をそのとき読んで覚える (DicdataStore)。別のスレッドが辞書のファイルを読み捨てて
///   OS のキャッシュに載せる (読み捨てなので private のメモリには数えない。AZK_DICT_READAHEAD=0 で読み捨てなし)
/// - background (2026-11-21 から既定、まとめ役の決め): lazy に加えて、別のスレッドが全部を読み (PreloadedDictionary)、主のスレッドが次の問い合わせの頭で覚えに足す
/// 2026-11-21 (t64/outn、冷えた起動 2 回ずつ): Initialize は preload 7.2〜9.8 秒、background 2.2〜2.3 秒 (Vulkan・CPU の AVX2 の版)。打っている 120 秒の 1 キーは同じ
/// (Vulkan p50/p90 6/13ms)。Vulkan の出力の字は 300 文とも preload と同じ。起動直後の private のメモリは小さいが、読み終えたあとは preload と同じになる見込み
enum DictionaryLoadingMode: String {
    case preload, lazy, background
}

let dictionaryLoadingMode = DictionaryLoadingMode(rawValue: devEnvironment["AZK_DICT_MODE"] ?? "") ?? .background
let dictionaryReadAheadEnabled = dictionaryLoadingMode == .lazy && devEnvironment["AZK_DICT_READAHEAD"] != "0"

/// 裏のスレッドと主のスレッドの受け渡し (錠で守る)
final class DictionaryPreloadSlot: @unchecked Sendable {
    private let lock = NSLock()
    private var loaded: PreloadedDictionary? = nil

    func finish(_ value: PreloadedDictionary) {
        lock.lock(); defer { lock.unlock() }
        loaded = value
    }

    /// 読み終えていれば渡す (1 度だけ)
    func take() -> PreloadedDictionary? {
        lock.lock(); defer { lock.unlock() }
        let value = loaded
        loaded = nil
        return value
    }
}

let dictionaryPreloadSlot = DictionaryPreloadSlot()
@MainActor var dictionaryPreloadPending = false

/// Initialize の頭で呼ぶ: 形に合わせて、読み捨て (lazy) か全部の裏読み (background) を始める
@MainActor func startDictionaryLoading(dictionaryURL: URL) {
    switch dictionaryLoadingMode {
    case .preload:
        return
    case .lazy:
        guard dictionaryReadAheadEnabled else { return }
        let directory = dictionaryURL.appendingPathComponent("louds")
        let thread = Thread {
            let started = Date()
            let urls = (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? []
            var bytes = 0
            for url in urls.sorted(by: { $0.lastPathComponent < $1.lastPathComponent }) {
                guard let handle = try? FileHandle(forReadingFrom: url) else { continue }
                while let chunk = try? handle.read(upToCount: 1 << 20), !chunk.isEmpty {
                    bytes += chunk.count
                }
                try? handle.close()
            }
            debugWrite("dictionary read-ahead: \(urls.count) files, \(bytes >> 20)MB in \(Int(-started.timeIntervalSinceNow * 1000))ms\n".data(using: .utf8)!)
        }
        thread.qualityOfService = .utility
        thread.start()
    case .background:
        dictionaryPreloadPending = true
        let slot = dictionaryPreloadSlot
        let thread = Thread {
            let started = Date()
            let preloaded = PreloadedDictionary.load(dictionaryResourceURL: dictionaryURL)
            slot.finish(preloaded)
            debugWrite("dictionary background: \(preloaded.loudsCount) louds, \(preloaded.loudstxtCount) loudstxt3 in \(Int(-started.timeIntervalSinceNow * 1000))ms\n".data(using: .utf8)!)
        }
        thread.qualityOfService = .utility
        thread.start()
    }
}

/// 主のスレッドで、裏読みが済んでいれば覚えに足す (getOptions の頭で呼ぶ)
@MainActor func installPreloadedDictionaryIfReady() {
    guard dictionaryPreloadPending, let preloaded = dictionaryPreloadSlot.take() else { return }
    dictionaryPreloadPending = false
    converter.installPreloadedDictionary(preloaded)
}
