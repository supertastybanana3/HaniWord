import Foundation
import KanaKanjiConverterModule
import ffi

private struct InputSession {
    var composing = ComposingText()
    var context = ""
    /// 2026-11-07 (仕事 45): カーソルの右の字 (right_context.swift)
    var right = ""
    var plain = false
    /// 2026-11-23 (セキュリティの点検 13): 疑問文の か で替えた文節の もとの表記 (question_ka.swift)。打った接続のものだけを使う
    /// (前はサーバー全体で 1 つで、別の接続 (隔離のアプリを含む) の候補の一覧の頭に入っていた)
    var questionKa: [(reading: String, text: String)] = []
}

@MainActor private var inputSessions: [UInt64: InputSession] = [:]
@MainActor private var activeConnection: UInt64 = 0

// IDs come from authenticated transport connections, never from an RPC field.
@_silgen_name("SelectIpcSession")
@MainActor public func select_ipc_session(_ connection: UInt64) {
    guard connection != activeConnection else { return }
    inputSessions[activeConnection] = InputSession(composing: composingText,
        context: (config["context"] as? String) ?? "", right: rightContextText, plain: plainNotation, questionKa: questionKaOriginals)
    let selected = inputSessions.removeValue(forKey: connection) ?? InputSession()
    composingText = selected.composing
    config["context"] = selected.context
    rightContextText = selected.right
    plainNotation = selected.plain
    questionKaOriginals = selected.questionKa
    activeConnection = connection
    // The model/dictionaries are shared, but incremental input/context caches are not.
    converter.stopComposition()
}

/// 2026-10-20 (Codex の再調査 M3): 今の接続の変換に、学習データとプロフィールを使ってよいか。AppContainer・制限トークン・
/// 低い整合性のアプリ (サンドボックス) からの接続では使わない (変換を問い合わせて、学習した語を引き出せないように)
@MainActor var personalDataAllowed = true

/// 接続を選んだ直後に呼ぶ。切り替えるときは、覚えかけの学習を先に保存し (学習を使わない設定で変換すると、保存前の学習が捨てられる)、
/// 接続をまたいで使い回す仮表示の結果と、変換の途中の結果を捨てる
@_silgen_name("SetPersonalDataAllowed")
@MainActor public func set_personal_data_allowed(_ allowed: Bool) {
    guard allowed != personalDataAllowed else { return }
    if personalDataAllowed {
        save_learning()
    }
    personalDataAllowed = allowed
    clearPreviewMemo()
    // 2026-11-08 (仕事 49): 確定した文節の覚えと左の語の見積もりも消す (left_pos.swift)
    forgetLeftWord()
    converter.stopComposition()
    // 2026-11-04: 単語登録した語も個人のデータなので、使ってよい接続のときだけ入れる (user_dictionary.swift)
    if dynamicDictionaryReady && !userDictionaryElements.isEmpty {
        applyDynamicDictionary()
    }
}

@_silgen_name("ReleaseIpcSession")
@MainActor public func release_ipc_session(_ connection: UInt64) {
    inputSessions.removeValue(forKey: connection)
    if connection == activeConnection {
        composingText = ComposingText()
        config["context"] = ""
        rightContextText = ""
        plainNotation = false
        questionKaOriginals = []
        activeConnection = 0
        converter.stopComposition()
    }
}

@_silgen_name("FreeText")
public func free_text(_ pointer: UnsafeMutablePointer<CChar>?) {
    free(pointer)
}

@_silgen_name("FreeCandidates")
public func free_candidates(_ list: UnsafeMutablePointer<UnsafeMutablePointer<FFICandidate>?>?, _ count: Int32) {
    guard let list else { return }
    for index in 0..<Int(count) {
        if let item = list[index] {
            free(item.pointee.text)
            free(item.pointee.subtext)
            free(item.pointee.hiragana)
            item.deinitialize(count: 1)
            item.deallocate()
        }
    }
    list.deallocate()
}
