// 理系の表記の自動変換 (設定でオン/オフ)
//   ・ギリシャ文字: アルファ → α、ガンマ線 → γ線。カタカナ語の一部 (プラスアルファ・アルファベット) は変えない
//   ・化学式の下付き: H2O → H₂O、Ca(OH)2 → Ca(OH)₂。元素記号として読み切れるときだけ (PS5・A4・PC98 は変えない)
import Foundation

/// いまの変換で表記の自動変換を止めるか (TeX を書いているとき。依頼ごとに IME から届く)
@MainActor var plainNotation = false

@_silgen_name("SetPlainNotation")
@MainActor public func set_plain_notation(_ plain: Bool) {
    plainNotation = plain
    // 仮表示の覚えは鍵に plainNotation を含むので、ここで捨てなくてよい
}

@MainActor func greekEnabled() -> Bool {
    ((config["greekLetters"] as? Bool) ?? true) && !plainNotation
}

@MainActor func chemEnabled() -> Bool {
    ((config["chemSubscript"] as? Bool) ?? true) && !plainNotation
}

/// 自動で置き換えるギリシャ文字 (カタカナの名前 → 小文字。greekAutoUpper の字は大文字)。普通の語と紛らわしい名前は入れない
let greekAuto: [(name: String, letter: String)] = [
    ("イプシロン", "ε"), ("オミクロン", "ο"), ("アルファ", "α"), ("ベータ", "β"), ("ガンマ", "γ"), ("ゼータ", "ζ"),
    ("シータ", "θ"), ("イオタ", "ι"), ("ラムダ", "λ"), ("ミュー", "μ"), ("グザイ", "ξ"), ("クシー", "ξ"),
    ("シグマ", "σ"), ("プサイ", "ψ"), ("オメガ", "ω"),
]

/// 読み (ひらがな) → 候補に足すギリシャ文字 (小文字・大文字)。紛らわしい名前 (ぱい・ろー・にゅー・かい) は候補だけ。
/// 並べる順 (大文字が先か) は greekPrefersUpper (設定 input.greek_case、無ければ数で決めた既定)。
/// 2026-11-06 (仕事 32): えーた (η)・てーた (θ)・つぇーた (ζ)・えぷしろん (ε)・ゆぷしろん (υ)・ぷし (ψ)・ふぃー (φ) を足した。
/// 出典は日本語版ウィキペディアの各字の記事の冒頭の読み (Η「エータ、イータ」・Θ「テータ」・Ζ「ツェータ」・Ε「エプシロン」・Υ「ユプシロン」・
/// Ψ「プシ」・Φ「フィー」。2026-11-06 に確かめた)。ふつうの語とぶつかるほかの読み (くさい・さい・はい・ぴー) は足さない
let greekCandidates: [String: (lower: String, upper: String)] = [
    "あるふぁ": ("α", "Α"), "べーた": ("β", "Β"), "がんま": ("γ", "Γ"), "でるた": ("δ", "Δ"), "いぷしろん": ("ε", "Ε"),
    "えぷしろん": ("ε", "Ε"), "ぜーた": ("ζ", "Ζ"), "つぇーた": ("ζ", "Ζ"), "いーた": ("η", "Η"), "えーた": ("η", "Η"),
    "しーた": ("θ", "Θ"), "てーた": ("θ", "Θ"), "いおた": ("ι", "Ι"), "かっぱ": ("κ", "Κ"),
    "らむだ": ("λ", "Λ"), "みゅー": ("μ", "Μ"), "にゅー": ("ν", "Ν"), "ぐざい": ("ξ", "Ξ"), "くしー": ("ξ", "Ξ"),
    "おみくろん": ("ο", "Ο"), "ぱい": ("π", "Π"), "ろー": ("ρ", "Ρ"), "しぐま": ("σ", "Σ"), "たう": ("τ", "Τ"),
    "うぷしろん": ("υ", "Υ"), "ゆぷしろん": ("υ", "Υ"), "ふぁい": ("φ", "Φ"), "ふぃー": ("φ", "Φ"), "かい": ("χ", "Χ"),
    "ぷさい": ("ψ", "Ψ"), "ぷしー": ("ψ", "Ψ"), "ぷし": ("ψ", "Ψ"), "おめが": ("ω", "Ω"),
]

/// 大文字を先に出す字 (小文字で書く)。2026-11-06 (仕事 32): 日本語版ウィキペディアの文で、前後にギリシャ文字が無い 1 字のうち、
/// 名前の一部 (すぐ前・すぐ後ろがカタカナか英字・『』「」の中) と単位の Ω (数字のすぐ後ろ) を除いて数え、小文字の割合が 5 割を切った字。
/// 数え直すと 5 割を切る字は無かった (いちばん低いのは ψ 54%・ω 59%・δ 66%。ζ の大文字の多くは名前で、除くと小文字 82%)。
/// どの字も小文字が先 (今までと同じ)。大文字を先にしたい字は設定 input.greek_case で選ぶ
let greekDefaultUpper: Set<String> = []

/// 設定 input.greek_case (字の小文字 → "lower" か "upper") で選んだ順。書いていない字は greekDefaultUpper
@MainActor func greekPrefersUpper(_ lower: String) -> Bool {
    if let choice = (config["greekCase"] as? [String: String])?[lower] {
        return choice == "upper"
    }
    return greekDefaultUpper.contains(lower)
}

/// 読みに足すギリシャ文字を、選んだ順 (先に出す字が前) で返す
@MainActor func greekCandidateLetters(_ hiragana: String) -> [String]? {
    guard let pair = greekCandidates[hiragana] else { return nil }
    return greekPrefersUpper(pair.lower) ? [pair.upper, pair.lower] : [pair.lower, pair.upper]
}

/// 単位の Ω (抵抗の単位、おーむ)。2026-11-06 (仕事 32): ギリシャ文字としてではなく単位として、候補の後ろのほうに足す
/// (オウム・オーム社 などを押し出さない)。数字のすぐ後ろ (10おーむ) でも出す
let ohmReading = "おーむ"
let ohmSign = "Ω"

/// おーむ → Ω、10おーむ・１０おーむ・2.2おーむ → 10Ω・１０Ω・2.2Ω (数字は打ったまま)。それ以外は nil
func ohmCandidate(_ hiragana: String) -> String? {
    guard hiragana.hasSuffix(ohmReading) else { return nil }
    let head = hiragana.dropLast(ohmReading.count)
    func digit(_ c: Character) -> Bool { ("0"..."9").contains(c) || ("０"..."９").contains(c) }
    guard head.isEmpty || (digit(head.first!) && digit(head.last!) && head.allSatisfy { digit($0) || ".,．，".contains($0) }) else { return nil }
    return head + ohmSign
}

/// 小文字 → 大文字 (自動の置き換えで大文字にするとき)
let greekUpperOf: [String: String] = Dictionary(greekCandidates.values.map { ($0.lower, $0.upper) }, uniquingKeysWith: { first, _ in first })

/// 大文字 → 小文字
let greekLowerOf: [String: String] = Dictionary(greekUpperOf.map { ($0.value, $0.key) }, uniquingKeysWith: { first, _ in first })

/// 設定 input.greek_case を読む (字の小文字 → "lower" か "upper")。大文字の字で書いてあれば小文字に直す。知らない字・値は捨てる
func greekCaseSetting(_ value: Any?) -> [String: String] {
    guard let raw = value as? [String: Any] else { return [:] }
    var out: [String: String] = [:]
    for (key, value) in raw {
        guard let choice = value as? String, choice == "lower" || choice == "upper" else { continue }
        if greekUpperOf[key] != nil {
            out[key] = choice
        } else if let lower = greekLowerOf[key] {
            out[lower] = choice
        }
    }
    return out
}

/// 自動の置き換え (シグマ → σ) で大文字にする字。候補の並びと同じ (大文字が先の字は大文字に置き換える。シグマ → Σ)
@MainActor func greekAutoUpper() -> Set<String> {
    Set(greekUpperOf.keys.filter(greekPrefersUpper))
}

func isKatakanaLike(_ c: Character?) -> Bool {
    guard let v = c?.unicodeScalars.first?.value else { return false }
    return (0x30A1...0x30FA).contains(v) || v == 0x30FC || (0xFF66...0xFF9F).contains(v)
}

/// カタカナのギリシャ文字名を文字に置き換える。前後 (文節をまたいだ前後の文字を含む) にカタカナが続くものは語の一部なので変えない
func replacingGreek(_ text: String, before: Character?, after: Character?, upper: Set<String> = []) -> String {
    var chars = Array(text)
    var i = 0
    var out = ""
    while i < chars.count {
        var matched = false
        for (name, letter) in greekAuto {
            let n = Array(name)
            guard i + n.count <= chars.count, Array(chars[i..<(i + n.count)]) == n else { continue }
            let prev: Character? = i == 0 ? before : chars[i - 1]
            let next: Character? = i + n.count == chars.count ? after : chars[i + n.count]
            if !isKatakanaLike(prev) && !isKatakanaLike(next) {
                out += upper.contains(letter) ? (greekUpperOf[letter] ?? letter) : letter
                i += n.count
                matched = true
                break
            }
        }
        if !matched {
            out.append(chars[i])
            i += 1
        }
    }
    chars = []
    return out
}

let elementSymbols: Set<String> = [
    "H", "He", "Li", "Be", "B", "C", "N", "O", "F", "Ne", "Na", "Mg", "Al", "Si", "P", "S", "Cl", "Ar", "K", "Ca",
    "Sc", "Ti", "V", "Cr", "Mn", "Fe", "Co", "Ni", "Cu", "Zn", "Ga", "Ge", "As", "Se", "Br", "Kr", "Rb", "Sr", "Y", "Zr",
    "Nb", "Mo", "Tc", "Ru", "Rh", "Pd", "Ag", "Cd", "In", "Sn", "Sb", "Te", "I", "Xe", "Cs", "Ba", "La", "Ce", "Pr", "Nd",
    "Pm", "Sm", "Eu", "Gd", "Tb", "Dy", "Ho", "Er", "Tm", "Yb", "Lu", "Hf", "Ta", "W", "Re", "Os", "Ir", "Pt", "Au", "Hg",
    "Tl", "Pb", "Bi", "Po", "At", "Rn", "Fr", "Ra", "Ac", "Th", "Pa", "U", "Np", "Pu", "Am", "Cm", "Bk", "Cf", "Es", "Fm",
    "Md", "No", "Lr", "Rf", "Db", "Sg", "Bh", "Hs", "Mt", "Ds", "Rg", "Cn", "Nh", "Fl", "Mc", "Lv", "Ts", "Og",
]

/// 元素 1 つに数字が付いただけでも化学式とみなすもの (O2 H2 N2 …)。B5・F1・U2 のような型番と区別するため
let singleElementFormulas: Set<String> = ["H2", "O2", "O3", "N2", "Cl2", "F2", "Br2", "I2", "S8", "P4"]

/// 型番・製品名で化学式と同じ形になるもの
let formulaLookalikes: Set<String> = ["PS2", "PS3", "PS4", "PS5", "PC98"]

let subscriptDigits: [Character: Character] = [
    "0": "₀", "1": "₁", "2": "₂", "3": "₃", "4": "₄", "5": "₅", "6": "₆", "7": "₇", "8": "₈", "9": "₉",
]
let plainSubscriptDigits: [Character: Character] = Dictionary(uniqueKeysWithValues: subscriptDigits.map { ($0.value, $0.key) })
let chargeDigits: [Character: Character] = [
    "0": "\u{2070}", "1": "\u{00b9}", "2": "\u{00b2}", "3": "\u{00b3}", "4": "\u{2074}",
    "5": "\u{2075}", "6": "\u{2076}", "7": "\u{2077}", "8": "\u{2078}", "9": "\u{2079}",
]
let plainChargeDigits: [Character: Character] = Dictionary(uniqueKeysWithValues: chargeDigits.map { ($0.value, $0.key) })
let biologicalShorthand: Set<String> = [
    "ATP", "ADP", "AMP", "GTP", "GDP", "GMP", "CTP", "CDP", "CMP", "UTP", "UDP", "UMP", "cAMP", "cGMP",
    "NAD", "NADH", "NADP", "NADPH", "FAD", "FADH", "FMN", "CoA", "Pi", "PPi", "DNA", "RNA", "mRNA", "tRNA", "rRNA",
]
let monatomicAutoCharges: [String: Set<Int>] = [
    "H": [1], "Li": [1], "Na": [1], "K": [1], "Ag": [1], "Cu": [1, 2],
    "Mg": [2], "Ca": [2], "Zn": [2], "Fe": [2, 3], "Mn": [2], "Co": [2, 3], "Ni": [2],
    "Sr": [2], "Ba": [2], "Al": [3], "Cr": [2, 3], "Cd": [2], "Pb": [2, 4], "Hg": [1, 2],
]

func isChemicalRunCharacter(_ c: Character) -> Bool {
    (c.isASCII && (c.isLetter || c.isNumber)) || plainSubscriptDigits[c] != nil || c == "(" || c == ")"
}

/// 英数字と括弧のひと続きが化学式なら、添字の数字を下付きにしたものを返す
func chemicalSubscripted(_ run: String, allowNoSubscript: Bool = false, allowSingleElement: Bool = false) -> String? {
    let plain = String(run.map { plainSubscriptDigits[$0] ?? $0 })
    if plain != run { return chemicalSubscripted(plain, allowNoSubscript: allowNoSubscript, allowSingleElement: allowSingleElement) }
    let withoutCoefficient = String(run.drop(while: { $0.isASCII && $0.isNumber }))
    if formulaLookalikes.contains(withoutCoefficient) {
        return nil
    }
    if singleElementFormulas.contains(run) {
        return subscriptAll(run)
    }
    let chars = Array(run)
    var i = 0
    var elements = 0
    var symbolsSeen: Set<String> = []
    var hasSubscript = false
    var hasLargeSubscript = false
    var depth = 0
    var out = ""
    var previousCanTakeNumber = false   // 直前が元素記号か「)」
    var coefficientAllowed = true       // 先頭と「·」の直後の数字は係数 (下付きにしない。2H2O・CuSO4·5H2O)
    while i < chars.count {
        let c = chars[i]
        if c.isNumber && coefficientAllowed {
            while i < chars.count, chars[i].isNumber {
                out.append(chars[i])
                i += 1
            }
            coefficientAllowed = false
            // 係数のあとは元素記号か「(」が続く
            guard i < chars.count, chars[i].isUppercase || chars[i] == "(" else { return nil }
            continue
        }
        coefficientAllowed = false
        if c == "." || c == "·" {
            // 水和物の点 (CuSO4.5H2O → CuSO₄·5H₂O)
            guard previousCanTakeNumber || out.last.map({ subscriptDigits.values.contains($0) }) == true else { return nil }
            out.append("·")
            i += 1
            previousCanTakeNumber = false
            coefficientAllowed = true
            continue
        }
        if c.isUppercase {
            var symbol = String(c)
            if i + 1 < chars.count, chars[i + 1].isLowercase, elementSymbols.contains(symbol + String(chars[i + 1])) {
                symbol += String(chars[i + 1])
            }
            guard elementSymbols.contains(symbol) else { return nil }
            elements += 1
            symbolsSeen.insert(symbol)
            out += symbol
            i += symbol.count
            previousCanTakeNumber = true
        } else if c.isNumber {
            guard previousCanTakeNumber else { return nil }
            var digits = ""
            while i < chars.count, chars[i].isNumber {
                digits.append(chars[i])
                i += 1
            }
            guard digits.first != "0", let value = Int(digits), value >= 2 else { return nil }
            hasLargeSubscript = hasLargeSubscript || value > 20
            out += String(digits.map { subscriptDigits[$0] ?? $0 })
            hasSubscript = true
            previousCanTakeNumber = false
        } else if c == "(" {
            depth += 1
            out.append(c)
            i += 1
            previousCanTakeNumber = false
        } else if c == ")" {
            guard depth > 0 else { return nil }
            depth -= 1
            out.append(c)
            i += 1
            previousCanTakeNumber = true
        } else {
            return nil
        }
    }
    // Organic molecular formulas can have large atom counts; keep the PC98 model-number guard.
    guard !hasLargeSubscript || (symbolsSeen.contains("C") && symbolsSeen.contains("H")) else { return nil }
    guard depth == 0, elements > 0, hasSubscript || allowNoSubscript,
          elements >= 2 || allowSingleElement || singleElementFormulas.contains(withoutCoefficient) else { return nil }
    return out
}

func chemicalBody(_ run: String, allowNoSubscript: Bool = false, allowSingleElement: Bool = false) -> String? {
    let plain = String(run.map { plainSubscriptDigits[$0] ?? $0 })
    if plain == "FADH2" { return "FADH\u{2082}" }
    if biologicalShorthand.contains(run) { return run }
    return chemicalSubscripted(run, allowNoSubscript: allowNoSubscript, allowSingleElement: allowSingleElement)
}

func chemicalIon(_ run: String, charge: String) -> String? {
    let plainRun = String(run.map { plainSubscriptDigits[$0] ?? $0 })
    if formulaLookalikes.contains(String(plainRun.drop(while: { $0.isASCII && $0.isNumber }))) { return nil }
    guard let last = charge.last, ["+", "-", "\u{2212}", "\u{207a}", "\u{207b}"].contains(String(last)) else { return nil }
    let sign = last == "+" || last == "\u{207a}" ? "\u{207a}" : "\u{207b}"
    let digits = String(charge.dropLast().map { plainChargeDigits[$0] ?? $0 })
    func magnitude(_ digits: String) -> String? {
        guard digits.allSatisfy({ $0.isASCII && $0.isNumber }), let value = Int(digits), (1...8).contains(value) else { return nil }
        return String(digits.map { chargeDigits[$0] ?? $0 })
    }
    if !digits.isEmpty {
        guard let exponent = magnitude(digits), let body = chemicalBody(run, allowNoSubscript: true, allowSingleElement: true) else { return nil }
        return body + exponent + sign
    }
    let chars = Array(run)
    var digitStart = chars.count
    while digitStart > 0, chars[digitStart - 1].isASCII, chars[digitStart - 1].isNumber { digitStart -= 1 }
    let head = String(chars[..<digitStart])
    // Ca2+ is an atomic charge, whereas O2+ and NH4+ retain their molecular subscripts.
    if digitStart < chars.count, elementSymbols.contains(head), !singleElementFormulas.contains(plainRun) {
        let number = String(chars[digitStart...])
        guard let value = Int(number), monatomicAutoCharges[head]?.contains(value) == true,
              let exponent = magnitude(number) else { return nil }
        return head + exponent + sign
    }
    if let body = chemicalBody(run, allowNoSubscript: true, allowSingleElement: true) { return body + sign }
    // Compact polyatomic charges: SO42- = SO4 + 2-. Explicit superscripts avoid ambiguity.
    if digitStart < chars.count, !chars[..<digitStart].contains(where: { plainSubscriptDigits[$0] != nil }) {
        for boundary in (digitStart..<chars.count).reversed() where boundary > 0 {
            if let exponent = magnitude(String(chars[boundary...])),
               let body = chemicalBody(String(chars[..<boundary]), allowNoSubscript: true) {
                return body + exponent + sign
            }
        }
    }
    return nil
}

func subscriptAll(_ run: String) -> String {
    String(run.map { subscriptDigits[$0] ?? $0 })
}

struct ChemicalLiteralSpan {
    var range: Range<Int>
    var text: String
}

let scientificQuantityUnits = [
    "mmol", "umol", "nmol", "pmol", "mol", "mM", "uM", "nM", "pM", "M",
    "mL", "ml", "uL", "ul", "nL", "L", "l", "kg", "mg", "ug", "ng", "pg", "g", "ms", "s",
].sorted { $0.count > $1.count }

func scientificUnitEnd(_ chars: [Character], at start: Int) -> Int? {
    func unitEnd(_ start: Int) -> Int? {
        for unit in scientificQuantityUnits {
            let suffix = Array(unit)
            let end = start + suffix.count
            guard end <= chars.count, Array(chars[start..<end]) == suffix else { continue }
            if end < chars.count, chars[end].isASCII, chars[end].isLetter || chars[end].isNumber { continue }
            return end
        }
        return nil
    }
    guard var end = unitEnd(start) else { return nil }
    while end < chars.count, chars[end] == "/", let next = unitEnd(end + 1) { end = next }
    return end
}

func scalarQuantitySpan(_ chars: [Character], at start: Int) -> ChemicalLiteralSpan? {
    guard chars[start].isASCII, chars[start].isNumber else { return nil }
    var end = start
    while end < chars.count, chars[end].isASCII, chars[end].isNumber { end += 1 }
    if end + 1 < chars.count, chars[end] == ".", chars[end + 1].isASCII, chars[end + 1].isNumber {
        end += 1
        while end < chars.count, chars[end].isASCII, chars[end].isNumber { end += 1 }
    }
    while end < chars.count, chars[end] == " " || chars[end] == "\t" { end += 1 }
    guard let unitEnd = scientificUnitEnd(chars, at: end) else { return nil }
    return ChemicalLiteralSpan(range: start..<unitEnd, text: String(chars[start..<unitEnd]))
}

/// 明示した下付きの後の普通の数字は数量。H2O23こ は数量を1桁以上残した最長の式 H2O2 + 3こ と読む。
func chemicalQuantity(_ run: String, counter: String) -> String? {
    let chars = Array(run)
    var digitStart = chars.count
    while digitStart > 0, chars[digitStart - 1].isASCII, chars[digitStart - 1].isNumber { digitStart -= 1 }
    guard digitStart < chars.count else { return nil }
    let explicit = chars.contains { plainSubscriptDigits[$0] != nil }
    let boundaries = explicit ? [digitStart] : Array((digitStart..<chars.count).reversed())
    for boundary in boundaries where boundary > 0 {
        if let fixed = chemicalBody(String(chars[..<boundary]), allowNoSubscript: true) {
            return fixed + String(chars[boundary...]) + counter
        }
    }
    return nil
}

/// Protect molecular formulas, ions and explicit biochemical shorthand before conversion.
func chemicalLiteralSpans(_ text: String, convertCounters: Bool = true) -> [ChemicalLiteralSpan] {
    let chars = Array(text)
    let units = ["mmol", "mol", "mL", "ml", "kg", "mg", "L", "g"]
    let counters = [("ぶんし", "分子"), ("分子", "分子"), ("個", "個"), ("こ", "個")]
    let particles = Set("をにではがものと、。")
    var result: [ChemicalLiteralSpan] = []
    var i = 0
    while i < chars.count {
        if let quantity = scalarQuantitySpan(chars, at: i) {
            result.append(quantity)
            i = quantity.range.upperBound
            continue
        }
        guard isChemicalRunCharacter(chars[i]) else { i += 1; continue }
        let start = i
        while i < chars.count {
            let c = chars[i]
            let hydrateDot = (c == "." || c == "·") && i > start && i + 1 < chars.count
                && (chars[i + 1].isNumber || chars[i + 1].isUppercase)
            guard isChemicalRunCharacter(c) || hydrateDot else { break }
            i += 1
        }
        let run = String(chars[start..<i])
        var matched = false
        for (reading, counter) in counters {
            let suffix = Array(reading)
            let end = i + suffix.count
            guard end <= chars.count, Array(chars[i..<end]) == suffix else { continue }
            // 「こえる」などの動詞を助数詞にしない。
            if reading == "こ", end < chars.count, !particles.contains(chars[end]) { continue }
            if let fixed = chemicalQuantity(run, counter: convertCounters ? counter : reading) {
                result.append(ChemicalLiteralSpan(range: start..<end, text: fixed))
                i = end
                matched = true
                break
            }
        }
        if !matched {
            for unit in units where run.hasSuffix(unit) {
                if let fixed = chemicalQuantity(String(run.dropLast(unit.count)), counter: unit) {
                    result.append(ChemicalLiteralSpan(range: start..<i, text: fixed))
                    matched = true
                    break
                }
            }
        }
        if matched { continue }
        var chargeEnd = i
        while chargeEnd < chars.count, plainChargeDigits[chars[chargeEnd]] != nil { chargeEnd += 1 }
        if chargeEnd < chars.count, ["+", "-", "\u{2212}", "\u{207a}", "\u{207b}"].contains(String(chars[chargeEnd])) {
            chargeEnd += 1
            let next = chargeEnd < chars.count ? chars[chargeEnd] : nil
            // A following operand indicates addition/subtraction, not a charge; also preserve C++.
            let arithmetic = next.map { ($0.isASCII && ($0.isLetter || $0.isNumber)) || $0 == "(" || $0 == "+" || $0 == "-" } ?? false
            if !arithmetic, let fixed = chemicalIon(run, charge: String(chars[i..<chargeEnd])) {
                result.append(ChemicalLiteralSpan(range: start..<chargeEnd, text: fixed))
                i = chargeEnd
                continue
            }
        }
        if let fixed = chemicalBody(run) {
            result.append(ChemicalLiteralSpan(range: start..<i, text: fixed))
        }
    }
    return result
}

/// 文字列の中の化学式を下付きにする (文字数は変わらない)
func replacingChemicalFormulas(_ text: String) -> String {
    let protected = chemicalLiteralSpans(text, convertCounters: false)
    if !protected.isEmpty {
        let chars = Array(text)
        var out = ""
        var offset = 0
        for span in protected {
            out += replacingChemicalFormulas(String(chars[offset..<span.range.lowerBound]))
            out += span.text
            offset = span.range.upperBound
        }
        return out + replacingChemicalFormulas(String(chars[offset...]))
    }
    var out = ""
    var run = ""
    func flush() {
        if !run.isEmpty {
            out += chemicalBody(run) ?? run
            run = ""
        }
    }
    let chars = Array(text)
    for (k, c) in chars.enumerated() {
        // 水和物の点は、前が化学式の途中で、後ろが数字か元素記号のときだけ化学式の一部 (文末の「H2O.」は含めない)
        let hydrateDot = (c == "." || c == "·") && !run.isEmpty && k + 1 < chars.count && (chars[k + 1].isNumber || chars[k + 1].isUppercase)
        if isChemicalRunCharacter(c) || hydrateDot {
            run.append(c)
        } else {
            flush()
            out.append(c)
        }
    }
    flush()
    return out
}

/// 候補 1 つに表記の自動変換を当てる (設定に従う)
@MainActor func notation(_ text: String) -> String {
    var out = text
    if greekEnabled() {
        out = replacingGreek(out, before: nil, after: nil, upper: greekAutoUpper())
    }
    if chemEnabled() {
        out = replacingChemicalFormulas(out)
    }
    return out
}
