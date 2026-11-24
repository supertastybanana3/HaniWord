#if canImport(llama)
import llama
#else
import llama_mock
#endif
import SwiftUtils
import HeapModule
import Algorithms
import Foundation
import EfficientNGram

/// 2026-11-07 (仕事 38 文脈の切り方): zenz に渡す左の文脈の字数の上限 (元の azooKey は 40)。試験用のビルドでだけ AZK_ZENZ_LEFT_MAX で変えられる
let zenzLeftContextMax: Int = azkDevEnvironment["AZK_ZENZ_LEFT_MAX"].flatMap { Int($0) }.map { max(0, $0) } ?? 40

struct FixedSizeHeap<Element: Comparable> {
    private var size: Int
    private var heap: Heap<Element>

    init(size: Int) {
        self.size = size
        self.heap = []
    }

    mutating func removeMax() {
        self.heap.removeMax()
    }

    mutating func removeMin() {
        self.heap.removeMin()
    }

    @discardableResult
    mutating func insertIfPossible(_ element: Element) -> Bool {
        if self.heap.count < self.size {
            self.heap.insert(element)
            return true
        } else if let min = self.heap.min, element > min {
            self.heap.replaceMin(with: element)
            return true
        } else {
            return false
        }
    }

    var unordered: [Element] {
        self.heap.unordered
    }

    var max: Element? {
        self.heap.max
    }

    var min: Element? {
        self.heap.min
    }

    var isEmpty: Bool {
        self.heap.isEmpty
    }
}

enum ZenzError: LocalizedError {
    case couldNotLoadModel(path: String)
    case couldNotLoadContext
    case couldNotLoadVocab

    var errorDescription: String? {
        switch self {
        case .couldNotLoadContext: return "failed to load context"
        case .couldNotLoadModel(path: let path): return "could not load model weight at \(path)"
        case .couldNotLoadVocab: return "failed to load vocab"
        }
    }
}

final class ZenzContext {
    private var model: OpaquePointer
    private var context: OpaquePointer
    private var vocab: OpaquePointer
    private var prevInput: [llama_token] = []
    /// 2026-11-04 (azooKey-Windows): 採点用の兄弟の文脈 (makeScoringSibling) なら、モデルを持つ元の文脈。モデルは元の文脈が解放する
    private var owner: ZenzContext? = nil

    private let n_len: Int32 = 512

    init(model: OpaquePointer, context: OpaquePointer, vocab: OpaquePointer) {
        self.model = model
        self.context = context
        self.vocab = vocab
    }

    deinit {
        llama_free(context)
        if owner == nil {
            llama_model_free(model)
            llama_backend_free()
        }
    }

    /// 2026-11-04 (azooKey-Windows): 採点 (evaluate・scoreSequences) だけに使う、同じモデル (重みは共有) の別の文脈。KV キャッシュと prevInput を
    /// 変換の文脈と分ける。前は採点が変換の文脈の KV を上書きし (evaluate は系列 0 を採点の文に、融合採点は系列 0 をプロンプトの頭に切り詰めて系列 1〜K を残す)、
    /// 次の変換が使い回す頭の長さと KV の並びが変わって、GPU の小数の誤差で僅差の語が入れ替わった (使わない採点を足すと 創設間もなかった → 創設まもなかった)
    func makeScoringSibling() -> ZenzContext? {
        guard let context = llama_init_from_model(model, Self.ctx_params) else { return nil }
        let sibling = ZenzContext(model: model, context: context, vocab: vocab)
        sibling.owner = self
        return sibling
    }

    /// 2026-11-10 (azooKey-Windows): 文脈の長さ (n_ctx) と 1 回の decode の上限 (n_batch)。KV と logits の出力の場所はこれに比例して取られ、
    /// 変換の文脈と採点の文脈 (makeScoringSibling) の 2 つ分ある。前は 1024。512 にしても、一般の文 300・CPU だけ 100・長い読み (130〜170 字) 10 の
    /// 410 文で出力は 1 文も変わらず、private のメモリが 35〜39MB 減った。開発用のビルドだけ AZK_ZENZ_NCTX・AZK_ZENZ_NBATCH で替えられる (測るため)
    static let contextTokens: UInt32 = azkDevEnvironment["AZK_ZENZ_NCTX"].flatMap { UInt32($0) }.flatMap { (64...4096).contains($0) ? $0 : nil } ?? 512
    static let batchTokens: UInt32 = azkDevEnvironment["AZK_ZENZ_NBATCH"].flatMap { UInt32($0) }.flatMap { (64...4096).contains($0) ? $0 : nil } ?? 512

    private static var ctx_params: llama_context_params {
        let n_threads = max(1, min(8, ProcessInfo.processInfo.processorCount - 2))
        debug("Using \(n_threads) threads")
        var ctx_params = llama_context_default_params()
        ctx_params.n_ctx = Self.contextTokens
        ctx_params.n_threads       = Int32(n_threads)
        ctx_params.n_threads_batch = Int32(n_threads)
        ctx_params.n_batch = Self.batchTokens
        // 融合採点で複数の候補を 1 回の decode でまとめて採点するため、系列を複数持てるようにする (KV は共有)。azooKey-Windows
        ctx_params.n_seq_max = UInt32(Self.maxFusionSequences + 1)
        ctx_params.kv_unified = true
        return ctx_params
    }

    static func createContext(path: String) throws -> ZenzContext {
        llama_backend_init()
        var model_params = llama_model_default_params()
        model_params.load_mode = LLAMA_LOAD_MODE_MMAP
        let model = llama_model_load_from_file(path, model_params)
        guard let model else {
            debug("Could not load model at \(path)")
            throw ZenzError.couldNotLoadModel(path: path)
        }

        let context = llama_init_from_model(model, ctx_params)
        guard let context else {
            debug("Could not load context!")
            throw ZenzError.couldNotLoadContext
        }

        let vocab = llama_model_get_vocab(model)
        guard let vocab else {
            debug("Could not load vocab!")
            throw ZenzError.couldNotLoadVocab
        }

        return ZenzContext(model: model, context: context, vocab: vocab)
    }

    /// 2026-11-18 (azooKey-Windows、perf の 7: 受け口を先に開き、zenz は裏で読む): 1 回 decode して、最初の推論にかかる準備 (GPU の
    /// パイプラインなど、冷えた起動で約 7 秒) を先に済ませる。変換の主のスレッドの外 (ZenzPreload.load) から呼ぶ。済んだら KV を空にする
    func warmUp() {
        _ = self.evaluate(text: "\u{EE00}ヘンカンノジュンビ\u{EE01}変換の準備")
        try? self.reset_context()
    }

    /// 2026-11-19 (azooKey-Windows、メモリの案 1): 文脈の状態 (KV のセル・出力の場所) と、使い回しの頭 (prevInput) の写し。
    /// 採点の状態と変換の状態を、1 つの文脈で入れ替えるために使う (Zenz の AZK_ZENZ_SCORING_SWAP)
    struct SavedState {
        let bytes: [UInt8]
        let prevInput: [llama_token]
    }

    /// 2026-11-19: 読み込んだ llama の版が GPU に載せられるか (CPU の版なら偽)。採点の入れ替えを既定にするかに使う
    static var supportsGPUOffload: Bool { llama_supports_gpu_offload() }

    /// 今の状態を写す。写せなければ nil
    func saveState() -> SavedState? {
        let size = llama_state_get_size(context)
        guard size > 0 else { return nil }
        var bytes = [UInt8](repeating: 0, count: size)
        let written = bytes.withUnsafeMutableBufferPointer { llama_state_get_data(context, $0.baseAddress, size) }
        guard written > 0 else { return nil }
        if written < size { bytes.removeLast(size - written) }
        return SavedState(bytes: bytes, prevInput: prevInput)
    }

    /// 写した状態に戻す。nil か戻せなければ空にする (次の評価は頭から計算し直す)
    func restoreState(_ state: SavedState?) {
        if let state {
            let read = state.bytes.withUnsafeBufferPointer { llama_state_set_data(context, $0.baseAddress, $0.count) }
            if read == state.bytes.count {
                prevInput = state.prevInput
                return
            }
        }
        try? reset_context()
    }

    func reset_context() throws {
        // The token-prefix record and KV memory must reset together. Otherwise
        // a new session skips decoding a prefix that no longer exists in memory.
        llama_memory_clear(llama_get_memory(self.context), true)
        self.prevInput.removeAll(keepingCapacity: true)
    }

    private func get_logits(tokens: [llama_token], logits_start_index: Int = 0) -> UnsafeMutablePointer<Float>? {
        let contextCapacity = Int(llama_n_ctx(context))
        let decodeCapacity = Int(llama_n_batch(context))
        guard logits_start_index >= 0, logits_start_index < tokens.count else { return nil }
        // manage kv_cache
        // 前回と共通する先頭部分は KV キャッシュを再利用し、そこから先だけを計算する。
        // logits を付ける位置 (logits_start_index 以降) は必ず計算し直すので、logits の並びは従来と同じ。
        var from = min(self.prevInput.commonPrefix(with: tokens).count, logits_start_index)
        guard ZenzBatchLimits.allocation(tokenCount: tokens.count, from: from,
            contextCapacity: contextCapacity, decodeCapacity: decodeCapacity) != nil else { return nil }
        let memory = llama_get_memory(context)
        if !llama_memory_seq_rm(memory, 0, llama_pos(from), -1) {
            llama_memory_clear(memory, true)
            from = 0
        }
        guard let capacity = ZenzBatchLimits.allocation(tokenCount: tokens.count, from: from,
            contextCapacity: contextCapacity, decodeCapacity: decodeCapacity) else {
            self.prevInput = []
            return nil
        }
        var batch = llama_batch_init(Int32(capacity), 0, 1)
        defer { llama_batch_free(batch) }
        for i in tokens.indices where i >= from {
            guard llama_batch_add(&batch, tokens[i], Int32(i), [0], logits: logits_start_index <= i, capacity: capacity) else {
                self.prevInput = []
                return nil
            }
        }
        // 評価
        if llama_decode(context, batch) != 0 {
            debug("llama_decode() failed")
            self.prevInput = []
            return nil
        }
        self.prevInput = tokens
        return llama_get_logits(context)
    }

    /// azooKey-Windows (2026-11-07、仕事 45): addEOS が false なら、終わりの印 (EOS) の確率を足さない (候補のあとに右の字が続く確率を測るため)
    func evaluate(text: String, ignorePrompt: String = "", addEOS: Bool = true) -> Float {
        let tokens_list = self.tokenize(text: text, add_bos: true, add_eos: addEOS)
        guard let logits = self.get_logits(tokens: tokens_list) else {
            debug("logits unavailable")
            return .nan
        }
        let tokenizedPromptCount = ignorePrompt.isEmpty ? 1 : tokenize(text: ignorePrompt, add_bos: true, add_eos: false).count
        let n_vocab = llama_vocab_n_tokens(vocab)

        var sum: Float = 0
        // 最初のプロンプト部分は無視する
        for (i, token_id) in tokens_list.indexed().dropFirst(tokenizedPromptCount) {
            // FIXME: there can be more efficient implementations, poossibly using Accelerate or other frameworks.
            var log_prob: Float = 0
            for index in ((i - 1) * Int(n_vocab)) ..< (i * Int(n_vocab)) {
                log_prob += expf(logits[index])
            }
            log_prob = logf(log_prob)
            log_prob = logits[Int((i - 1) * Int(n_vocab) + Int(token_id))] - log_prob
            sum += log_prob
        }
        return sum
    }

    enum CandidateEvaluationResult: Sendable, Equatable, Hashable {
        case error
        case pass(score: Float, alternativeConstraints: [AlternativeConstraint])
        case fixRequired(prefixConstraint: [UInt8])
        case wholeResult(String)

        struct AlternativeConstraint: Sendable, Equatable, Hashable {
            var probabilityRatio: Float
            var prefixConstraint: [UInt8]
        }
    }

    /// 学習した語を、モデルの判断よりどれだけ優先するか (azooKey-Windows の調整)。
    /// 1 だと「わざで→技で」を 1 回学習しただけで「至難の業」まで「至難の技」になるので弱める。AZK_LEARN_PRIORITY で試せる
    static let learnedPriorityScale: Float = {
        if let v = azkDevEnvironment["AZK_LEARN_PRIORITY"], let f = Float(v) { return f }
        return 0.25   // 1 / 0.5 / 0.25 / 0 を比べて、一般の文を崩さずに学習が効くところ (2026-10-09)
    }()

    /// 2026-10-29 (azooKey-Windows): 利用者が新しく作った語 (学習した語のうち辞書に無いもの。DicdataStore.markNewLearnedWords でユーザ辞書の印も付く) を
    /// 評価のプロンプトの「辞書:」に入れるか (AZK_NEW_WORD_PROMPT=1。既定は入れない)
    static let newWordPrompt = azkDevEnvironment["AZK_NEW_WORD_PROMPT"] == "1"

    /// 利用者が新しく作った語の優先の強さ (辞書にある語の学習は learnedPriorityScale のまま)。利用者が作った語 (漢字＋カタカナ) を 1 回学習して比べ、1 では文の中で消え、
    /// 3 で語だけでも文の中でも残った (とかく資料・各私立・至難の業 などは変わらない。2026-10-29)。AZK_NEW_WORD_PRIORITY で試せる
    static let newWordPriorityScale: Float = {
        if let v = azkDevEnvironment["AZK_NEW_WORD_PRIORITY"], let f = Float(v) { return f }
        return 3
    }()

    /// 2026-11-04 (azooKey-Windows): 利用者が単語登録した語の先頭のトークンの優先の強さ。利用者が新しく作った語と同じ 3 から。
    /// AZK_USER_WORD_PRIORITY で試せる (試験用のビルド -D AZK_DEV_KNOBS でだけ読む。配る版では利用者の環境変数で変わらない)
    static let userWordPriorityScale: Float = {
        #if AZK_DEV_KNOBS
        if let v = ProcessInfo.processInfo.environment["AZK_USER_WORD_PRIORITY"], let f = Float(v) { return f }
        #endif
        return 3
    }()

    func getLearningPriority(data: DicdataElement) -> Float {
        // 文字数の長い候補ほど優先的に適用されるようにする
        // 積極的な複合語化の効果を期待
        return if 1 <= data.ruby.count && data.ruby.count <= 4 {
            Float(data.ruby.count + 2)
        } else if 5 <= data.ruby.count && data.ruby.count <= 15 {
            Float(data.ruby.count * 2)
        } else {
            30
        }
    }

    /// ピュアな貪欲法による生成を行って返す
    func pure_greedy_decoding(leftSideContext: String, maxCount: Int = .max) -> String {
        var prompt_tokens = self.tokenize(text: leftSideContext, add_bos: false)
        let initial_count = prompt_tokens.count
        let eos_token = llama_vocab_eos(vocab)
        while prompt_tokens.count - initial_count < maxCount {
            let startOffset = prompt_tokens.count - 1
            guard let logits = self.get_logits(tokens: prompt_tokens, logits_start_index: startOffset) else {
                debug("logits unavailable")
                return ""
            }
            let n_vocab = llama_vocab_n_tokens(vocab)
            let startIndex = (prompt_tokens.count - 1 - startOffset) * Int(n_vocab)
            let endIndex = (prompt_tokens.count - startOffset) * Int(n_vocab)
            // Min-Heapを使用してn-bestを計算
            var max_token: llama_token = -1
            var max_value: Float = Float.infinity * -1
            for index in startIndex..<endIndex {
                let token = llama_token(index - startIndex)
                if max_value < logits[index] {
                    max_token = token
                    max_value = logits[index]
                }
            }
            if max_token == eos_token {
                break
            } else {
                prompt_tokens.append(max_token)
            }
        }

        // Heapからソートして結果を取り出す
        let cchars: [CChar] = prompt_tokens.dropFirst(initial_count).flatMap(self.token_to_piece) + [0]
        return String(cString: cchars)
    }

    func predict_next_character(leftSideContext: String, count: Int) -> [(character: Character, value: Float)] {
        struct NextCharacterCandidate: Comparable {
            static func < (lhs: NextCharacterCandidate, rhs: NextCharacterCandidate) -> Bool {
                lhs.value < rhs.value
            }
            var character: Character
            var value: Float
        }

        // 文末を目指して生成するためのプロンプト
        // \u{EE01}を停止トークンとみなせる
        let prompt_tokens = self.tokenize(text: "\u{EE00}。\u{EE02}\(leftSideContext)", add_bos: false)
        let startOffset = prompt_tokens.count - 1

        guard let logits = self.get_logits(tokens: prompt_tokens, logits_start_index: startOffset) else {
            debug("logits unavailable")
            return []
        }

        let n_vocab = llama_vocab_n_tokens(vocab)
        var exp_sum: Float = 0
        let startIndex = (prompt_tokens.count - 1 - startOffset) * Int(n_vocab)
        let endIndex = (prompt_tokens.count - startOffset) * Int(n_vocab)

        // Min-Heapを使用してn-bestを計算
        var minHeap: FixedSizeHeap<NextCharacterCandidate> = .init(size: count)
        let token_to_penalty_weight: [llama_token: Float] = prompt_tokens.indexed().reduce(into: [:]) { dict, item in
            let (index, token) = item
            // 現在位置から遠いほど減衰させる
            dict[token, default: 0] += 2 / Float(prompt_tokens.count - index)
        }

        for index in startIndex..<endIndex {
            let token = llama_token(index - startIndex)
            let repeat_penalty = Float(1.0 + token_to_penalty_weight[token, default: 0])
            let v = expf(logits[index] / repeat_penalty)
            exp_sum += v

            let tokenPieceData = Data((token_to_piece(token: token)).map(UInt8.init))
            let character: Character
            if let validCharacter = String(data: tokenPieceData, encoding: .utf8), let c = validCharacter.first {
                character = c
            } else {
                continue
            }
            minHeap.insertIfPossible(NextCharacterCandidate(character: character, value: v))
        }

        // Heapからソートして結果を取り出す
        return minHeap.unordered.sorted { $0.value > $1.value }.map { ($0.character, $0.value / exp_sum) }
    }

    func evaluate_candidate(
        input: String,
        candidate: Candidate,
        requestRichCandidates: Bool,
        personalizationMode: (mode: ConvertRequestOptions.ZenzaiMode.PersonalizationMode, base: EfficientNGram, personal: EfficientNGram)?,
        versionDependentConfig: ConvertRequestOptions.ZenzaiVersionDependentMode
    ) -> CandidateEvaluationResult {
        debug("Evaluate", candidate)
        // For zenz-v1 model, \u{EE00} is a token used for 'start query', and \u{EE01} is a token used for 'start answer'
        // We assume \u{EE01}\(candidate) is always splitted into \u{EE01}_\(candidate) by zenz-v1 tokenizer
        var userDictionaryPrompt: String = ""
        // 2026-10-29 (azooKey-Windows): 利用者が新しく作った語 (学習した語でユーザ辞書の印つき) は、AZK_NEW_WORD_PROMPT=1 のときだけ「辞書:」に入れる
        // (zenz-v3 ではこの条件で、利用者が作った語の後ろの字がカタカナに崩れた: 〜の問題ヲ解く)
        for item in candidate.data where item.metadata.contains(.isFromUserDictionary) && (Self.newWordPrompt || !item.metadata.contains(.isLearned)) {
            userDictionaryPrompt += "\(item.word)(\(item.ruby.toHiragana()))"
        }
        var conditions: [String] = []
        // ユーザ辞書の内容がある場合はこれを条件に追加
        if !userDictionaryPrompt.isEmpty {
            conditions.append("辞書:\(userDictionaryPrompt)")
        }
        // プロフィールがある場合はこれを条件に追加
        switch versionDependentConfig {
        case .v1: break
        case .v2(let mode):
            if let profile = mode.profile, !profile.isEmpty {
                let pf = profile.suffix(25)
                conditions.append("プロフィール:\(pf)")
            }
        case .v3(let mode):
            if let profile = mode.profile, !profile.isEmpty {
                let pf = profile.suffix(25)
                conditions.append("\u{EE03}\(pf)")
            }
            if let topic = mode.topic, !topic.isEmpty {
                let tp = topic.suffix(25)
                conditions.append("\u{EE04}\(tp)")
            }
            if let style = mode.style, !style.isEmpty {
                let st = style.suffix(25)
                conditions.append("\u{EE05}\(st)")
            }
            if let preference = mode.preference, !preference.isEmpty {
                let pr = preference.suffix(25)
                conditions.append("\u{EE06}\(pr)")
            }
        }
        // 左文脈を取得
        let leftSideContext: String = switch versionDependentConfig {
        case .v1: ""
        case .v2(let mode):
            if let leftSideContext = mode.leftSideContext {
                String(leftSideContext.suffix(zenzLeftContextMax))
            } else {
                ""
            }
        case .v3(let mode):
            if let leftSideContext = mode.leftSideContext {
                String(leftSideContext.suffix(zenzLeftContextMax))
            } else {
                ""
            }
        }
        let inputTag = "\u{EE00}"
        let outputTag = "\u{EE01}"
        let contextTag = "\u{EE02}"
        // プロンプトを作成
        var prompt: String = switch versionDependentConfig {
        case .v1:
            inputTag + input + outputTag
        case .v2:
            if !conditions.isEmpty {
                // 条件がemptyでない場合は「・」でつなぎ、「発言:」を末尾に追加
                inputTag + input + contextTag + conditions.joined(separator: "・") + "・発言:\(leftSideContext)" + outputTag
            } else if !leftSideContext.isEmpty {
                // 条件がemptyの場合、単にleftSideContextを追加
                inputTag + input + contextTag + leftSideContext + outputTag
            } else {
                // そのまま
                inputTag + input + outputTag
            }
        case .v3:
            if !leftSideContext.isEmpty {
                // leftSideContextがEmptyでなければ下記の通り処理
                // contextがinputに前置されるように変更された(KV-cachingの効率化のため)
                conditions.joined(separator: "") + contextTag + leftSideContext + inputTag + input + outputTag
            } else {
                // そのまま
                conditions.joined(separator: "") + inputTag + input + outputTag
            }
        }
        // プロンプトの前処理を適用
        prompt = self.preprocessText(text: prompt)
        // Therefore, tokens = prompt_tokens + candidate_tokens is an appropriate operation.
        let prompt_tokens = self.tokenize(text: prompt, add_bos: true, add_eos: false)
        let candidate_tokens = self.tokenize(text: self.preprocessText(text: candidate.text), add_bos: false, add_eos: false)
        let tokens = prompt_tokens + candidate_tokens
        let startOffset = prompt_tokens.count - 1
        let pos_max = llama_memory_seq_pos_max(llama_get_memory(self.context), 0)
        debug("pos max:", pos_max)
        guard let logits = self.get_logits(tokens: tokens, logits_start_index: startOffset) else {
            debug("logits unavailable")
            return .error
        }
        let n_vocab = llama_vocab_n_tokens(vocab)
        let is_learned_token: [(isLearned: Bool, priority: Float)] = Array(repeating: (false, 0), count: prompt_tokens.count) + candidate.data.flatMap {
            // 2026-10-31 (azooKey-Windows): 読み間違いの本来の形 (ふいんき → 雰囲気。isReadingCorrection) のトークンは、モデルの点によらずそのまま通す。
            // モデルは読みどおりの字 (ふいんき・不インキ) を好み、直しを求めて N-best のかな書きに乗り換えていた (制約の素通りだけでは残らない)
            if $0.metadata.contains(.isReadingCorrection) {
                return Array(repeating: (true, Float.infinity), count: self.tokenize(text: $0.word, add_bos: false).count)
            }
            // 2026-11-04 (azooKey-Windows、上流 AzooKeyKanaKanjiConverter #370 の 3 つ目に当たる): 利用者が単語登録した語 (isUserRegistered) は、
            // 先頭のトークンを学習した語と同じ優先で判断し、2 つ目以降のトークンはモデルの点によらず表記どおり通す
            // (モデルが別の字を好むと直しを求めて崩れていた: 黒崎莉央 → 黒崎梨央)。採るかどうかは先頭のトークンでモデルが判断するので、文脈による選び分けは残る
            if $0.metadata.contains(.isUserRegistered) {
                let count = self.tokenize(text: $0.word, add_bos: false).count
                guard count > 0 else { return [] }
                return [(true, logf(getLearningPriority(data: $0)) * Self.userWordPriorityScale)] + Array(repeating: (true, Float.infinity), count: count - 1)
            }
            // priorityは文字数にする→文字数が長いほど優先される
            return Array(repeating: ($0.metadata.contains(.isLearned), logf(getLearningPriority(data: $0))
                * ($0.metadata.contains(.isFromUserDictionary) ? Self.newWordPriorityScale : Self.learnedPriorityScale)), count: self.tokenize(text: $0.word, add_bos: false).count)
        }

        var score: Float = 0

        struct AlternativeHighProbToken: Comparable {
            static func < (lhs: AlternativeHighProbToken, rhs: AlternativeHighProbToken) -> Bool {
                lhs.probabilityRatioToMaxProb < rhs.probabilityRatioToMaxProb
            }

            var token: llama_token
            var constraint: [UInt8]
            // 最大probabilityに対しての割合
            var probabilityRatioToMaxProb: Float
        }

        var altTokens = FixedSizeHeap<AlternativeHighProbToken>(size: requestRichCandidates ? 5 : 0)
        for (i, token_id) in tokens.indexed().dropFirst(prompt_tokens.count) {
            // それぞれのトークンが、一つ前の予測において最も確率の高いトークンであるかをチェックする
            // softmaxはmaxなので、単にlogitsの中で最も大きいものを選べば良い
            // 一方実用的にはlog_probも得ておきたい。このため、ここでは明示的にsoftmaxも計算している
            struct TokenAndLogprob: Comparable {
                static func < (lhs: TokenAndLogprob, rhs: TokenAndLogprob) -> Bool {
                    lhs.logprob < rhs.logprob
                }
                var token: llama_token
                var logprob: Float
            }
            var sumexp: Float = 0
            let startIndex = (i - 1 - startOffset) * Int(n_vocab)
            let endIndex = (i - startOffset) * Int(n_vocab)
            var tokenHeap = FixedSizeHeap<TokenAndLogprob>(size: requestRichCandidates ? 3 : 1)
            for index in startIndex ..< endIndex {
                sumexp += expf(logits[index])
            }
            let logsumexp = logf(sumexp)

            if let (mode, baseLM, personalLM) = personalizationMode, mode.alpha > 0 {
                let prefix = tokens[..<i].dropFirst(prompt_tokens.count).map(Int.init)
                let baseProb: [Float]
                let personalProb: [Float]
                // SwiftNgramのLMは無条件の場合エラーになるため(Unigram確率はサポートしていない)
                if !prefix.isEmpty {
                    baseProb = baseLM.bulkPredict(prefix).map { logf(Float($0) + 1e-7) }
                    personalProb = personalLM.bulkPredict(prefix).map { logf(Float($0) + 1e-7) }
                } else {
                    baseProb = Array(repeating: 0, count: Int(n_vocab))
                    personalProb = baseProb
                }
                // p = probabilityBuffer / exp_sum
                // p' = p / p_b * p_p
                for (i, (lpb, lpp)) in zip(0 ..< Int(n_vocab), zip(baseProb, personalProb)) {
                    let logp = logits[startIndex + i] - logsumexp
                    let logp_ = logp + mode.alpha * (lpp - lpb) // personalized probability
                    tokenHeap.insertIfPossible(TokenAndLogprob(token: llama_token(i), logprob: logp_))
                }
            } else {
                // p = probabilityBuffer / exp_sum
                for i in startIndex ..< endIndex {
                    let logp = logits[i] - logsumexp
                    tokenHeap.insertIfPossible(TokenAndLogprob(token: llama_token(i - startIndex), logprob: logp))
                }
            }

            guard let maxItem = tokenHeap.max else {
                debug("Max Item could not be found for unknown reason")
                return .error
            }
            // 読み間違いの本来の形のトークンは、モデルが別の字や終わり (EOS) を好んでも直しを求めない (azooKey-Windows)
            // (語ごとに数えたトークンの数は文全体のトークン化と合わないことがあるので、範囲の外は見ない)
            if maxItem.token != token_id && i < is_learned_token.count && is_learned_token[i].priority == .infinity {
                score += maxItem.logprob
                continue
            }
            // ここで最も良い候補であったかをチェックする
            if maxItem.token != token_id {
                if maxItem.token == llama_vocab_eos(vocab) {
                    var cchars = tokens[..<i].reduce(into: []) {
                        $0.append(contentsOf: token_to_piece(token: $1))
                    }
                    // adding "\0"
                    cchars.append(0)
                    let string = String(cString: cchars)
                    // 要求するべき制約を記述する
                    let wholeResult = String(string.dropFirst(prompt.count))
                    return .wholeResult(wholeResult)
                } else {
                    let actual_logp: Float = logits[startIndex + Int(token_id)] - logsumexp
                    // 学習されたトークンであり、なおかつactual_expのある程度大きければ、学習されたトークンを優先する
                    let preferLearnedToken = is_learned_token[i].isLearned && actual_logp + is_learned_token[i].priority > maxItem.logprob
                    if !preferLearnedToken {
                        // adding "\0"
                        let cchars = tokens[..<i].reduce(into: []) {
                            $0.append(contentsOf: token_to_piece(token: $1))
                        } + token_to_piece(token: maxItem.token)
                        return .fixRequired(prefixConstraint: cchars.dropFirst(prompt.utf8.count).map(UInt8.init))
                    }
                }
            } else if !tokenHeap.isEmpty {
                tokenHeap.removeMax()
                let prefix = tokens[..<i].reduce(into: []) {
                    $0.append(contentsOf: token_to_piece(token: $1))
                }.dropFirst(prompt.utf8.count)

                for item in tokenHeap.unordered {
                    altTokens.insertIfPossible(
                        AlternativeHighProbToken(
                            token: item.token,
                            constraint: prefix.map(UInt8.init) + token_to_piece(token: item.token).map(UInt8.init),
                            probabilityRatioToMaxProb: expf(item.logprob - maxItem.logprob)
                        )
                    )
                }
            }
            score += maxItem.logprob
        }
        return .pass(score: score, alternativeConstraints: altTokens.unordered.sorted(by: >).map {.init(probabilityRatio: $0.probabilityRatioToMaxProb, prefixConstraint: $0.constraint)})
    }

    /// 融合採点用のプロンプト (evaluate_candidate と同じ組み立て。ユーザ辞書の条件は付けない)。azooKey-Windows
    func fusionPrompt(input: String, versionDependentConfig: ConvertRequestOptions.ZenzaiVersionDependentMode) -> String {
        var conditions: [String] = []
        var leftSideContext = ""
        switch versionDependentConfig {
        case .v1: break
        case .v2(let mode):
            if let profile = mode.profile, !profile.isEmpty {
                conditions.append("プロフィール:\(profile.suffix(25))")
            }
            leftSideContext = mode.leftSideContext.map { String($0.suffix(zenzLeftContextMax)) } ?? ""
        case .v3(let mode):
            if let profile = mode.profile, !profile.isEmpty { conditions.append("\u{EE03}\(profile.suffix(25))") }
            if let topic = mode.topic, !topic.isEmpty { conditions.append("\u{EE04}\(topic.suffix(25))") }
            if let style = mode.style, !style.isEmpty { conditions.append("\u{EE05}\(style.suffix(25))") }
            if let preference = mode.preference, !preference.isEmpty { conditions.append("\u{EE06}\(preference.suffix(25))") }
            leftSideContext = mode.leftSideContext.map { String($0.suffix(zenzLeftContextMax)) } ?? ""
        }
        let inputTag = "\u{EE00}", outputTag = "\u{EE01}", contextTag = "\u{EE02}"
        let prompt: String = switch versionDependentConfig {
        case .v1:
            inputTag + input + outputTag
        case .v2:
            if !conditions.isEmpty {
                inputTag + input + contextTag + conditions.joined(separator: "・") + "・発言:\(leftSideContext)" + outputTag
            } else if !leftSideContext.isEmpty {
                inputTag + input + contextTag + leftSideContext + outputTag
            } else {
                inputTag + input + outputTag
            }
        case .v3:
            if !leftSideContext.isEmpty {
                conditions.joined(separator: "") + contextTag + leftSideContext + inputTag + input + outputTag
            } else {
                conditions.joined(separator: "") + inputTag + input + outputTag
            }
        }
        return self.preprocessText(text: prompt)
    }

    static let maxFusionSequences = 15
    static let profile = azkDevEnvironment["AZK_FUSION_PROF"] == "1"

    /// 同じプロンプトに続けて outputs のそれぞれを書く確率 (対数、EOS まで) を返す。azooKey-Windows の融合採点用。
    /// プロンプト (最後の 1 トークンを除く) を系列 0 に置き、それを系列 1...K に写して、全候補を 1 回の decode でまとめて計算する
    func scoreSequencesBatched(prompt: String, outputs: [String]) -> [Float]? {
        let tEnter = Date()
        let prompt_tokens = self.tokenize(text: prompt, add_bos: true, add_eos: false)
        let P = prompt_tokens.count
        guard P >= 2 else { return nil }
        let memory = llama_get_memory(context)
        let n_vocab = Int(llama_vocab_n_tokens(vocab))
        let n_ctx = Int(llama_n_ctx(context))
        let n_batch = Int(llama_n_batch(context))
        guard P <= n_ctx, P <= Int(Int32.max) else { return nil }
        let head = Array(prompt_tokens.prefix(P - 1))
        // 系列 0 を head ちょうどにする (前回の入力と共通する部分は計算し直さない)
        let common = min(self.prevInput.commonPrefix(with: head).count, P - 1)
        if !llama_memory_seq_rm(memory, 0, llama_pos(common), -1) {
            return nil
        }
        // 系列 1 以降のごみを掃除
        for s in 1 ... Self.maxFusionSequences { _ = llama_memory_seq_rm(memory, llama_seq_id(s), -1, -1) }
        if common < P - 1 {
            guard let capacity = ZenzBatchLimits.allocation(tokenCount: head.count, from: common,
                contextCapacity: n_ctx, decodeCapacity: n_batch) else { return nil }
            var batch = llama_batch_init(Int32(capacity), 0, 1)
            defer { llama_batch_free(batch) }
            for i in common ..< (P - 1) {
                guard llama_batch_add(&batch, head[i], Int32(i), [0], logits: i == P - 2, capacity: capacity) else { return nil }
            }
            if llama_decode(context, batch) != 0 {
                self.prevInput = []
                llama_memory_clear(memory, true)
                return nil
            }
        }
        self.prevInput = head
        if Self.profile {
            FileHandle.standardError.write("FUSIONPROF prompt common=\(common) P=\(P) prep=\(Int(-tEnter.timeIntervalSinceNow * 1000))\n".data(using: .utf8)!)
        }
        let tails: [[llama_token]] = outputs.map {
            [prompt_tokens[P - 1]] + self.tokenize(text: self.preprocessText(text: $0), add_bos: false, add_eos: true)
        }
        // 候補どうしで共通する先頭部分は 1 回だけ計算する (トークンの木。1 つのトークンを複数の系列に属させる)
        struct Node { var token: llama_token; var pos: Int; var seqs: [llama_seq_id]; var children: [llama_token: Int] }
        var scores = [Float](repeating: -Float.infinity, count: outputs.count)
        var k = 0
        while k < tails.count {
            var nodes: [Node] = []
            var roots: [llama_token: Int] = [:]
            var paths: [(idx: Int, nodes: [Int])] = []
            while k < tails.count && paths.count < Self.maxFusionSequences {
                // 入りきるか試算 (最悪ケース: 共有なし)
                if nodes.count + tails[k].count > n_batch || (P - 1) + nodes.count + tails[k].count > n_ctx {
                    break
                }
                let seq = llama_seq_id(paths.count + 1)
                var path: [Int] = []
                var parent: Int? = nil
                for (t, token) in tails[k].enumerated() {
                    let existing = parent.map { nodes[$0].children[token] } ?? roots[token]
                    let id: Int
                    if let existing {
                        id = existing
                        nodes[id].seqs.append(seq)
                    } else {
                        id = nodes.count
                        nodes.append(Node(token: token, pos: P - 1 + t, seqs: [seq], children: [:]))
                        if let parent { nodes[parent].children[token] = id } else { roots[token] = id }
                    }
                    path.append(id)
                    parent = id
                }
                paths.append((k, path))
                k += 1
            }
            if paths.isEmpty {
                k += 1   // 1 つでも入らない長さ (読みが非常に長い) は採点しない
                continue
            }
            var batch = llama_batch_init(Int32(nodes.count), 0, Int32(Self.maxFusionSequences))
            defer { llama_batch_free(batch) }
            for j in paths.indices {
                llama_memory_seq_cp(memory, 0, llama_seq_id(j + 1), -1, -1)
            }
            var row: [Int] = Array(repeating: -1, count: nodes.count)
            var nRows = 0
            for (i, node) in nodes.enumerated() {
                let needLogits = !node.children.isEmpty
                guard llama_batch_add(&batch, node.token, Int32(node.pos), node.seqs, logits: needLogits,
                    capacity: nodes.count, sequenceCapacity: Self.maxFusionSequences) else { return nil }
                if needLogits { row[i] = nRows; nRows += 1 }
            }
            let tDecode = Date()
            if llama_decode(context, batch) != 0 {
                for j in paths.indices { _ = llama_memory_seq_rm(memory, llama_seq_id(j + 1), -1, -1) }
                return nil
            }
            let tSoftmax = Date()
            defer {
                if Self.profile {
                    FileHandle.standardError.write("FUSIONPROF nodes=\(nodes.count) rows=\(nRows) seqs=\(paths.count) prompt=\(P) decode=\(Int(tSoftmax.timeIntervalSince(tDecode) * 1000)) softmax=\(Int(-tSoftmax.timeIntervalSinceNow * 1000))\n".data(using: .utf8)!)
                }
            }
            // 各行の logsumexp は 1 回だけ計算する
            var lse = [Float](repeating: 0, count: nodes.count)
            for (i, _) in nodes.enumerated() where row[i] >= 0 {
                guard let logits = llama_get_logits_ith(context, Int32(i)) else { return nil }
                var maxv = -Float.infinity
                for v in 0 ..< n_vocab where logits[v] > maxv { maxv = logits[v] }
                var sumexp: Float = 0
                for v in 0 ..< n_vocab { sumexp += expf(logits[v] - maxv) }
                lse[i] = maxv + logf(sumexp)
            }
            for (idx, path) in paths {
                var sum: Float = 0
                for t in 0 ..< (path.count - 1) {
                    guard let logits = llama_get_logits_ith(context, Int32(path[t])) else { return nil }
                    sum += logits[Int(nodes[path[t + 1]].token)] - lse[path[t]]
                }
                scores[idx] = sum
            }
            for j in paths.indices { _ = llama_memory_seq_rm(memory, llama_seq_id(j + 1), -1, -1) }
        }
        return scores
    }

    /// 同じプロンプトに続けて outputs のそれぞれを書く確率 (対数、EOS まで) を返す。azooKey-Windows の融合採点用。
    /// プロンプト部分の KV キャッシュは使い回し、候補の部分だけを計算する (1 本ずつ。検算用)
    func scoreSequences(prompt: String, outputs: [String]) -> [Float] {
        let prompt_tokens = self.tokenize(text: prompt, add_bos: true, add_eos: false)
        let n_vocab = Int(llama_vocab_n_tokens(vocab))
        let startOffset = prompt_tokens.count - 1
        return outputs.map { output in
            let candidate_tokens = self.tokenize(text: self.preprocessText(text: output), add_bos: false, add_eos: true)
            let tokens = prompt_tokens + candidate_tokens
            guard let logits = self.get_logits(tokens: tokens, logits_start_index: startOffset) else {
                return -Float.infinity
            }
            var sum: Float = 0
            for (k, token_id) in candidate_tokens.enumerated() {
                // 位置 startOffset + k の logits が candidate_tokens[k] を予測する
                let base = k * n_vocab
                var maxv = -Float.infinity
                for j in 0 ..< n_vocab where logits[base + j] > maxv { maxv = logits[base + j] }
                var sumexp: Float = 0
                for j in 0 ..< n_vocab { sumexp += expf(logits[base + j] - maxv) }
                sum += logits[base + Int(token_id)] - maxv - logf(sumexp)
            }
            return sum
        }
    }

    private func llama_batch_add(_ batch: inout llama_batch, _ id: llama_token, _ pos: llama_pos, _ seq_ids: [llama_seq_id], logits: Bool,
                                 capacity: Int, sequenceCapacity: Int = 1) -> Bool {
        // Unsafe pointers have no bounds checks; validate before the first write.
        guard ZenzBatchLimits.canAppend(count: batch.n_tokens, capacity: capacity, sequences: seq_ids.count, sequenceCapacity: sequenceCapacity),
              let tokens = batch.token, let positions = batch.pos, let counts = batch.n_seq_id,
              let sequences = batch.seq_id, let logitFlags = batch.logits,
              let sequence = sequences[Int(batch.n_tokens)] else { return false }
        let index = Int(batch.n_tokens)
        tokens[index] = id
        positions[index] = pos
        counts[index] = Int32(seq_ids.count)
        for i in 0..<seq_ids.count {
            sequence[i] = seq_ids[i]
        }
        logitFlags[index] = logits ? 1 : 0
        batch.n_tokens += 1
        return true
    }

    private func preprocessText(text: String) -> String {
        // replace space into ideographic space (\u3000) for zenz tokenizer
        // replace newline into null for zenz tokenizer
        return text.replacingOccurrences(of: " ", with: "\u{3000}").replacingOccurrences(of: "\n", with: "")
    }
    private func tokenize(text: String, add_bos: Bool, add_eos: Bool = false) -> [llama_token] {
        let utf8Count = text.utf8.count
        let n_tokens = utf8Count + (add_bos ? 1 : 0)
        let tokens = UnsafeMutablePointer<llama_token>.allocate(capacity: n_tokens)
        let tokenCount = llama_tokenize(vocab, text, Int32(utf8Count), tokens, Int32(n_tokens), add_bos, false)
        var swiftTokens: [llama_token] = if tokenCount < 0 {
            [llama_vocab_bos(vocab)]
        } else {
            (0..<tokenCount).map {tokens[Int($0)]}
        }
        tokens.deallocate()
        if add_eos {
            swiftTokens.append(llama_vocab_eos(vocab))
        }
        return swiftTokens
    }

    /// - note: The result does not contain null-terminator
    private func token_to_piece(token: llama_token) -> [CChar] {
        let result = UnsafeMutablePointer<Int8>.allocate(capacity: 8)
        result.initialize(repeating: Int8(0), count: 8)
        defer {
            result.deallocate()
        }
        let nTokens = llama_token_to_piece(vocab, token, result, 8, 0, false)

        if nTokens < 0 {
            let newResult = UnsafeMutablePointer<Int8>.allocate(capacity: Int(-nTokens))
            newResult.initialize(repeating: Int8(0), count: Int(-nTokens))
            defer {
                newResult.deallocate()
            }
            let nNewTokens = llama_token_to_piece(vocab, token, newResult, Int32(-nTokens), 0, false)
            let bufferPointer: UnsafeBufferPointer<Int8> = UnsafeBufferPointer(start: newResult, count: Int(nNewTokens))
            return Array(bufferPointer)
        } else {
            let bufferPointer: UnsafeBufferPointer<Int8> = UnsafeBufferPointer(start: result, count: Int(nTokens))
            return Array(bufferPointer)
        }
    }
}
