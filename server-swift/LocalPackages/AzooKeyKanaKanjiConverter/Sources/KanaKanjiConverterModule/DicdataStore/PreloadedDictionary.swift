import Foundation

/// azooKey-Windows (2026-11-18、perf の 7 の 2b): 本体の辞書 (Dictionary/louds の LOUDS と .loudstxt3 の写像) を、変換の主のスレッドの外で読んだもの。
/// KanaKanjiConverter.installPreloadedDictionary で変換器の覚えに足す。足すまでは読んだスレッドだけが触り、足したあとは主のスレッドだけが触る
public final class PreloadedDictionary: @unchecked Sendable {
    let loudses: [String: LOUDS]
    let loudstxts: [String: Data]
    public var loudsCount: Int { loudses.count }
    public var loudstxtCount: Int { loudstxts.count }

    init(loudses: [String: LOUDS], loudstxts: [String: Data]) {
        self.loudses = loudses
        self.loudstxts = loudstxts
    }

    /// dictionaryResourceURL (Dictionary) の louds の中を全部読む。主のスレッドの外から呼ぶ
    public static func load(dictionaryResourceURL: URL) -> PreloadedDictionary {
        let directory = dictionaryResourceURL.appendingPathComponent("louds", isDirectory: true)
        let urls = (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? []
        var loudses: [String: LOUDS] = [:]
        var loudstxts: [String: Data] = [:]
        for url in urls {
            let identifier = url.deletingPathExtension().lastPathComponent
            switch url.pathExtension {
            case "louds":
                if let louds = LOUDS.loadMainDictionary(identifier, directory: directory) {
                    loudses[identifier] = louds
                }
            case "loudstxt3":
                if let data = try? Data(contentsOf: url, options: DicdataStore.mapPreloadedDictionary ? [.alwaysMapped] : []) {
                    loudstxts[identifier] = data
                }
            default:
                continue
            }
        }
        return PreloadedDictionary(loudses: loudses, loudstxts: loudstxts)
    }
}
