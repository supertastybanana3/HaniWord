import Foundation

/// 2026-11-04 (azooKey-Windows、security review #5 と同じ): 試験用の環境変数 (AZK_*) は、試験用のビルド (swiftc -D AZK_DEV_KNOBS) でだけ読む。
/// 配る版では空 (利用者の環境変数で、学習の優先・融合採点の形・制約の素通しなどを変えられないように)
let azkDevEnvironment: [String: String] = {
#if AZK_DEV_KNOBS
    return ProcessInfo.processInfo.environment.filter { $0.key.hasPrefix("AZK_") }
#else
    return [:]
#endif
}()
