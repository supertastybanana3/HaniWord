import Foundation
import KanaKanjiConverterModule

/// 2026-11-17 (迷うね → 迷う値): 辞書だけの変換 (ライブ変換の仮表示・清書した前のほうを残したときの後ろの読み・AI を切った変換) で、
/// 文の終わりの 1 字の終助詞の読み (ね・よ・さ・わ・な・ぞ・ぜ・の・か) を、動詞・形容詞・助動詞の終止形のすぐ後ろで名詞 (値・世・差・輪・名・蚊 など) に
/// していたら、終助詞に戻す。AI ありの変換は zenz が終助詞を大きな差で選ぶので触らない。
/// すぐ前が終止形のときだけなので、名詞の後ろ (長さ の さ のような語の中は 1 語) や文の頭の よ (世) は替えない。
/// すぐ前の語の表記 (2 字以上) と、後ろの読みだけを変換するとき (語が 1 つだけ) の左の文脈は、終わりの字が終止形の終わり方 (う段・い・た・だ) かで見る。
/// 開発用の切り替え: AZK_FINAL_PARTICLE=0 で前と同じ
/// 2026-11-19 (新しいエージェントの見直し・まとめ役): 直すのは、文の終わりに当たるときだけ。変換サーバーは読みを 、。「」 などの記号で切って部分ごとに
/// 変換するので、部分のすぐ後ろが文の終わりの記号 (finalParticleFollowers) か、読みの終わり (右の文脈が空か、同じ記号で始まる) のときだけ。
/// 開く括弧・「、」・「・」の前ではかけない (形容詞は終止形と連体形が同じなので、大きい差（約2倍） の 差 を さ にしていた)。
/// すぐ前の語は右の品詞 (rcid、後ろにつながる側) で見る (前は左の品詞を見ていて、辞書の句の語で後ろの形を見ていなかった)
let finalParticleFix = devEnvironment["AZK_FINAL_PARTICLE"] != "0"

/// 部分のすぐ後ろにあれば、文の終わりとみなす字
let finalParticleFollowers: Set<Character> = ["。", "！", "？", "!", "?", "」", "』", "）", ")", "…", "〜", "～"]

/// 部分のすぐ後ろの字 (読みの終わりなら nil) と右の文脈から、文の終わりに当たるか
func finalParticleAtSentenceEnd(follower: Character?, rightContext: String) -> Bool {
    if let follower { return finalParticleFollowers.contains(follower) }
    return rightContext.first.map { finalParticleFollowers.contains($0) } ?? true
}

/// 記号で切った部分を変換している間の、その部分のすぐ後ろの字 (読みの終わりなら nil。preservingLiteralSymbols の partFollower で替える)
@MainActor var conversionPartFollower: Character? = nil

/// 終助詞として戻す読み (カタカナ)
let finalParticleReadings: Set<String> = ["ネ", "ヨ", "サ", "ワ", "ナ", "ゾ", "ゼ", "ノ", "カ"]

/// 動詞・形容詞・助動詞の基本形 (終止形・連体形) の品詞の番号。IPADIC の left-id.def の 141 個 (azooKey の品詞の番号はこの並びと同じ)
let predicativeEndCids: Set<Int> = [
    19, 20, 37, 43, 52, 65, 66, 67, 96, 113, 114, 131, 137, 146, 370, 383, 384, 403, 414, 415, 430, 435, 436, 444,
    453, 459, 460, 461, 473, 484, 491, 492, 503, 506, 507, 508, 509, 510, 511, 512, 513, 514, 515, 516, 519, 521, 522, 529,
    530, 538, 543, 546, 549, 563, 573, 583, 592, 599, 612, 619, 628, 637, 639, 645, 651, 657, 663, 669, 679, 680, 695, 696,
    711, 712, 723, 731, 738, 746, 754, 762, 772, 773, 792, 802, 803, 817, 818, 819, 836, 841, 846, 851, 856, 863, 870, 879,
    886, 897, 907, 919, 920, 933, 942, 953, 954, 955, 956, 957, 991, 992, 993, 994, 1019, 1020, 1037, 1038, 1039, 1040, 1059, 1091,
    1092, 1093, 1094, 1095, 1096, 1097, 1098, 1099, 1100, 1101, 1102, 1103, 1216, 1217, 1218, 1219, 1252, 1253, 1254, 1255, 1277,
]

/// 活用する語の品詞の番号 (IPADIC の 形容詞 11〜146・助動詞 369〜554・動詞 561〜1280)
func isInflectingCid(_ cid: Int) -> Bool {
    (11...146).contains(cid) || (369...554).contains(cid) || (561...1280).contains(cid)
}

/// 終助詞の品詞の番号 (IPADIC の 助詞,終助詞 264〜295 と、助詞,副助詞／並立助詞／終助詞 の か 359)
func isFinalParticleCid(_ cid: Int) -> Bool {
    (264...295).contains(cid) || cid == 359
}

/// 左の文脈の終わりが、動詞・形容詞・助動詞の終止形の終わり方か (迷う・寒い・行った・静かだ・行きます・です)。字だけで見る
func contextEndsPredicative(_ context: String) -> Bool {
    guard let last = context.last else { return false }
    return "うくぐすつぬぶむるいただ".contains(last)
}

/// 直した候補 (直す所が無ければ nil)。particle: 読み (ひらがな) → 辞書の終助詞の語
/// 2026-11-23 (a/symseg F5): 読点と括弧を前後と一緒に変換する (どの道でも) と、」） が同じ候補の中の語になり、終わりの語が終助詞でなくなる
/// (「まようね」と → 「迷う値」と。前は 」 で分けて、部分のすぐ後ろの字で文の終わりと見ていた)。候補の中で、文の終わりの記号だけの語
/// (finalParticleFollowers) のすぐ前の語も、文の終わりの語として見る
func sentenceFinalParticleFixed(_ candidate: Candidate, context: String, atSentenceEnd: Bool = true,
                                 particle: (String) -> DicdataElement?) -> Candidate? {
    guard finalParticleFix else { return nil }
    var data = candidate.data
    var changed = false
    for t in data.indices {
        let beforeFollower = t + 1 < data.count && !data[t + 1].word.isEmpty && data[t + 1].word.allSatisfy { finalParticleFollowers.contains($0) }
        guard beforeFollower || (t == data.count - 1 && atSentenceEnd),
              let replacement = finalParticleReplacement(candidate.data, at: t, context: context, particle: particle) else { continue }
        data[t] = replacement
        changed = true
    }
    guard changed, let last = data.last else { return nil }
    return Candidate(text: data.map(\.word).joined(), value: candidate.value, correspondingCount: candidate.correspondingCount,
                     lastMid: last.mid, data: data)
}

/// data[t] が、終止形のすぐ後ろで名詞などになった終助詞の読みなら、辞書の終助詞の語 (でなければ nil)
func finalParticleReplacement(_ data: [DicdataElement], at t: Int, context: String, particle: (String) -> DicdataElement?) -> DicdataElement? {
    let last = data[t]
    guard finalParticleReadings.contains(last.ruby), !CIDData.isJoshi(cid: last.lcid), !isFinalParticleCid(last.lcid) else { return nil }
    // すぐ前の語が終止形の品詞か、表記の終わりが終止形の終わり方 (辞書に 1 語の句で入っている 仕方がない・なんとかなる は名詞の品詞なので、字で見る)
    let afterPredicate: Bool
    if t >= 1 {
        let previous = data[t - 1]
        // 活用する語 (動詞・形容詞・助動詞) は品詞で見る (迷い の連用形は替えない)。活用しない語 (名詞の句など) だけ字で見る
        afterPredicate = isInflectingCid(previous.rcid)
            ? predicativeEndCids.contains(previous.rcid)
            : previous.word.count >= 2 && contextEndsPredicative(previous.word)
    } else {
        afterPredicate = contextEndsPredicative(context)
    }
    guard afterPredicate else { return nil }
    let reading = String(String.UnicodeScalarView(last.ruby.unicodeScalars.map { s in
        (0x30A1...0x30F6).contains(s.value) ? Unicode.Scalar(s.value - 0x60)! : s
    }))
    return particle(reading)
}

/// 読み (ひらがな 1 字) の終助詞の語。品詞・意味の番号・値は、本体の辞書のその終助詞の項と同じ (辞書から引くと、同じ表記の別の品詞の語
/// (さ のサ変の語など) が先に来て、変換の候補は表記ごとに 1 つにまとまるので、終助詞の項が残らない)
let finalParticleEntries: [String: (cid: Int, mid: Int, value: Float)] = [
    "ね": (279, 47, -2.17), "よ": (288, 17, -2.147), "さ": (268, 441, -3.296), "わ": (291, 36, -2.742), "な": (275, 287, -2.522),
    "ぞ": (270, 420, -3.255), "ぜ": (269, 52, -3.45), "の": (282, 408, -2.812), "か": (359, 468, -1.979),
]

func dictionaryFinalParticle(_ reading: String) -> DicdataElement? {
    guard let entry = finalParticleEntries[reading] else { return nil }
    let ruby = String(String.UnicodeScalarView(reading.unicodeScalars.map { s in
        (0x3041...0x3096).contains(s.value) ? Unicode.Scalar(s.value + 0x60)! : s
    }))
    return DicdataElement(word: reading, ruby: ruby, lcid: entry.cid, rcid: entry.cid, mid: entry.mid, value: PValue(entry.value))
}
