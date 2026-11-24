import Foundation

/// 2026-11-18 (azooKey-Windows、perf の 7: 受け口を先に開き、zenz は裏で読む): zenz のモデルの読み込みと最初の推論 (GPU の準備) を、
/// 変換の主のスレッドの外で済ませた文脈。KanaKanjiConverter.installPreloadedZenz で変換器に渡す。
/// 渡すまでは読み込んだスレッドだけが触り、渡したあとは変換器 (主のスレッド) だけが触る (同時には触らないので @unchecked Sendable)
public final class ZenzPreload: @unchecked Sendable {
    let resourceURL: URL
    let context: ZenzContext

    init(resourceURL: URL, context: ZenzContext) {
        self.resourceURL = resourceURL
        self.context = context
    }

    /// モデルを読み、1 回 decode して温める。主のスレッドの外から呼ぶ。読めなければ nil (変換器は AI を使わない)
    public static func load(resourceURL: URL) -> ZenzPreload? {
#if canImport(Darwin)
        let path = resourceURL.path(percentEncoded: false)
#else
        // this is not percent-encoded
        let path = resourceURL.path
#endif
        guard let context = try? ZenzContext.createContext(path: path) else { return nil }
        context.warmUp()
        return ZenzPreload(resourceURL: resourceURL, context: context)
    }
}
