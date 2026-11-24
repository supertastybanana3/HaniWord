import Foundation
import SwiftUtils
import EfficientNGram

@MainActor package final class Zenz {
    package var resourceURL: URL
    private var zenzContext: ZenzContext?
    /// 2026-11-04 (azooKey-Windows): 採点用の文脈 (ZenzContext.makeScoringSibling。初めて採点するときに作る)。変換の文脈の KV を採点で変えないため。
    /// 試験用のビルド (-D AZK_DEV_KNOBS) では AZK_ZENZ_SHARED_SCORING=1 で前と同じく変換の文脈で採点し、AZK_ZENZ_SCORING_FRESH=1 で採点のたびに採点用の KV を空にする
    private var scoringSibling: ZenzContext? = nil
    private var scoringSiblingTried = false
    private static let sharedScoring = azkDevEnvironment["AZK_ZENZ_SHARED_SCORING"] == "1"
    /// 2026-11-19 (azooKey-Windows、メモリの案 1): AZK_ZENZ_SCORING_SWAP=1 なら採点用の文脈を作らず、採点の前後で変換の文脈の状態と採点の状態を丸ごと入れ替える
    /// (採点用の文脈は KV・計算の場所・GPU の確保で 65〜77MB。採点の状態は写し (数 MB) で持ち、次の採点で戻して、別の文脈のときと同じ使い回しにする)
    /// 2026-11-19 (まとめ役の決め): 既定は、GPU に載せない llama の版 (CPU の版) のときだけ入れ替える。CPU では写しはメモリの中だけで
    /// 速さが変わらず (1 文の p50 454 → 456ms)、「メモリ」(private のワーキングセット) が約 −26MB。GPU (Vulkan) では写しが GPU とホストを行き来して
    /// 1 文が 20〜26% 遅くなるので、2 つ目の文脈のまま。開発用のつまみ AZK_ZENZ_SCORING_SWAP=1 / 0 で、版によらず入れる・切る
    private static let swapScoring: Bool = {
        if sharedScoring { return false }
        if let knob = azkDevEnvironment["AZK_ZENZ_SCORING_SWAP"] { return knob == "1" }
        return !ZenzContext.supportsGPUOffload
    }()
    private var scoringState: ZenzContext.SavedState? = nil

    /// 採点を 1 回する。入れ替えの形なら、変換の状態を写して採点の状態に替え、済んだら戻す
    private func scoring<T>(_ body: (ZenzContext) -> T?) -> T? {
        guard Self.swapScoring else {
            guard let context = scoringContext else { return nil }
            return body(context)
        }
        guard let context = zenzContext else { return nil }
        let conversion = context.saveState()
        context.restoreState(scoringState)
        let result = body(context)
        scoringState = context.saveState()
        context.restoreState(conversion)
        return result
    }
    private static let freshScoring = azkDevEnvironment["AZK_ZENZ_SCORING_FRESH"] == "1"
    private var scoringContext: ZenzContext? {
        if Self.sharedScoring { return zenzContext }
        if !scoringSiblingTried {
            scoringSiblingTried = true
            scoringSibling = zenzContext?.makeScoringSibling()
        }
        guard let sibling = scoringSibling else { return zenzContext }
        if Self.freshScoring { try? sibling.reset_context() }
        return sibling
    }
    init(resourceURL: URL) throws {
        self.resourceURL = resourceURL
        do {
#if canImport(Darwin)
            if #available(iOS 16, macOS 13, *) {
                self.zenzContext = try ZenzContext.createContext(path: resourceURL.path(percentEncoded: false))
            } else {
                // this is not percent-encoded
                self.zenzContext = try ZenzContext.createContext(path: resourceURL.path)
            }
#else
            // this is not percent-encoded
            self.zenzContext = try ZenzContext.createContext(path: resourceURL.path)
#endif
            debug("Loaded model \(resourceURL.lastPathComponent)")
        } catch {
            throw error
        }
    }

    /// 2026-11-18 (azooKey-Windows、perf の 7): 別のスレッドで読み込んで温めた文脈 (ZenzPreload) を受け取る
    init(resourceURL: URL, preloadedContext: ZenzContext) {
        self.resourceURL = resourceURL
        self.zenzContext = preloadedContext
    }

    package func endSession() {
        try? self.zenzContext?.reset_context()
        try? self.scoringSibling?.reset_context()
        scoringState = nil
    }

    func candidateEvaluate(
        convertTarget: String,
        candidates: [Candidate],
        requestRichCandidates: Bool,
        personalizationMode: (mode: ConvertRequestOptions.ZenzaiMode.PersonalizationMode, base: EfficientNGram, personal: EfficientNGram)?,
        versionDependentConfig: ConvertRequestOptions.ZenzaiVersionDependentMode
    ) -> ZenzContext.CandidateEvaluationResult {
        guard let zenzContext else {
            return .error
        }
        for candidate in candidates {
            let result = zenzContext.evaluate_candidate(
                input: convertTarget.toKatakana(),
                candidate: candidate,
                requestRichCandidates: requestRichCandidates,
                personalizationMode: personalizationMode,
                versionDependentConfig: versionDependentConfig
            )
            return result
        }
        return .error
    }

    func predictNextCharacter(leftSideContext: String, count: Int) -> [(character: Character, value: Float)] {
        guard let zenzContext else {
            return []
        }
        let result = zenzContext.predict_next_character(leftSideContext: leftSideContext, count: count)
        return result
    }

    /// prompt に続けて output を書く確率 (対数、EOS まで)。azooKey-Windows で候補を比べ直すのに使う
    package func sequenceLogProb(prompt: String, output: String) -> Float? {
        let v = scoring { $0.evaluate(text: prompt + output, ignorePrompt: prompt) }
        return v.flatMap { $0.isNaN ? nil : $0 }
    }

    /// azooKey-Windows (2026-11-07、仕事 45): prompt＋prefix に続けて continuation を書く確率 (対数、EOS は足さない)。
    /// 候補 (prefix) のあとにカーソルの右の字 (continuation) が続くかを測る
    package func continuationLogProb(prompt: String, prefix: String, continuation: String) -> Float? {
        guard !continuation.isEmpty else { return nil }
        let v = scoring { $0.evaluate(text: prompt + prefix + continuation, ignorePrompt: prompt + prefix, addEOS: false) }
        return v.flatMap { $0.isNaN ? nil : $0 }
    }

    /// 融合採点: 読み input (カタカナ) に対して outputs それぞれの対数確率 (EOS まで)。azooKey-Windows
    package func scoreSequences(input: String, outputs: [String], versionDependentConfig: ConvertRequestOptions.ZenzaiVersionDependentMode) -> [Float]? {
        scoring { (zenzContext: ZenzContext) -> [Float]? in
            let prompt = zenzContext.fusionPrompt(input: input, versionDependentConfig: versionDependentConfig)
            if azkDevEnvironment["AZK_FUSION_SERIAL"] == "1" {
                return zenzContext.scoreSequences(prompt: prompt, outputs: outputs)
            }
            return zenzContext.scoreSequencesBatched(prompt: prompt, outputs: outputs) ?? zenzContext.scoreSequences(prompt: prompt, outputs: outputs)
        }
    }

    package func pureGreedyDecoding(pureInput: String, maxCount: Int = .max) -> String {
        return self.zenzContext?.pure_greedy_decoding(leftSideContext: pureInput, maxCount: maxCount) ?? ""
    }
}
