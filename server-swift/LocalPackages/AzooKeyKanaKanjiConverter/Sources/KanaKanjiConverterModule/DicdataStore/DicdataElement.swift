//
//  DicdataElement.swift
//  Keyboard
//
//  Created by ensan on 2020/09/10.
//  Copyright © 2020 ensan. All rights reserved.
//

import Foundation

public struct DicdataElement: Equatable, Hashable, Sendable {
    static let BOSData = Self(word: "", ruby: "", cid: CIDData.BOS.cid, mid: MIDData.BOS.mid, value: 0, adjust: 0)
    static let EOSData = Self(word: "", ruby: "", cid: CIDData.EOS.cid, mid: MIDData.EOS.mid, value: 0, adjust: 0)

    public init(word: String, ruby: String, lcid: Int, rcid: Int, mid: Int, value: PValue, adjust: PValue = .zero, metadata: DicdataElementMetadata = .empty) {
        self.word = word
        self.ruby = ruby
        self.lcid = lcid
        self.rcid = rcid
        self.mid = mid
        self.baseValue = value
        self.adjust = adjust
        self.metadata = metadata
    }

    public init(word: String, ruby: String, cid: Int, mid: Int, value: PValue, adjust: PValue = .zero, metadata: DicdataElementMetadata = .empty) {
        self.word = word
        self.ruby = ruby
        self.lcid = cid
        self.rcid = cid
        self.mid = mid
        self.baseValue = value
        self.adjust = adjust
        self.metadata = metadata
    }

    public init(ruby: String, cid: Int, mid: Int, value: PValue, adjust: PValue = .zero, metadata: DicdataElementMetadata = .empty) {
        self.word = ruby
        self.ruby = ruby
        self.lcid = cid
        self.rcid = cid
        self.mid = mid
        self.baseValue = value
        self.adjust = adjust
        self.metadata = metadata
    }

    public consuming func adjustedData(_ adjustValue: PValue) -> Self {
        self.adjust += adjustValue
        return self
    }

    public var word: String
    public var ruby: String
    public var lcid: Int
    public var rcid: Int
    public var mid: Int
    var baseValue: PValue
    public var adjust: PValue
    public var metadata: DicdataElementMetadata

    public func value() -> PValue {
        min(.zero, self.baseValue + self.adjust)
    }

    public static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.word == rhs.word && lhs.ruby == rhs.ruby && lhs.lcid == rhs.lcid && lhs.mid == rhs.mid && lhs.rcid == rhs.rcid && lhs.metadata == rhs.metadata
    }

    public func hash(into hasher: inout Hasher) {
        hasher.combine(word)
        hasher.combine(ruby)
        hasher.combine(lcid)
        hasher.combine(rcid)
        hasher.combine(metadata)
    }
}

extension DicdataElement: CustomDebugStringConvertible {
    public var debugDescription: String {
        "("
        + "ruby: \(self.ruby), "
        + "word: \(self.word), "
        + "cid: (\(self.lcid), \(self.rcid)), "
        + "mid: \(self.mid), "
        + "value: \(self.baseValue)+\(self.adjust)=\(self.value()), "
        + "metadata: ("
        + "isLearned: \(self.metadata.contains(.isLearned)), "
        + "isFromUserDictionary: \(self.metadata.contains(.isFromUserDictionary)), "
        + "isReadingCorrection: \(self.metadata.contains(.isReadingCorrection))"
        + ")"
        + ")"
    }
}

public struct DicdataElementMetadata: OptionSet, Sendable, Hashable, Equatable {
    public let rawValue: UInt32
    public init(rawValue: UInt32) {
        self.rawValue = rawValue
    }

    public static let empty: Self = []
    /// 学習データから得られた候補にはこのフラグを立てる
    public static let isLearned = DicdataElementMetadata(rawValue: 1 << 0) // 1
    /// ユーザ辞書から得られた候補にはこのフラグを立てる
    public static let isFromUserDictionary = DicdataElementMetadata(rawValue: 1 << 1) // 2
    /// azooKey-Windows: 読み間違いの本来の形 (ふいんき → 雰囲気。DicdataStore.readingCorrectionDomains の分野辞書の語) にはこのフラグを立てる
    public static let isReadingCorrection = DicdataElementMetadata(rawValue: 1 << 2) // 4
    /// azooKey-Windows (2026-11-04): 利用者が単語登録で登録した語 (%APPDATA%\HaniWord\user_dictionary.tsv) にはこのフラグを立てる。
    /// ユーザ辞書の印 (isFromUserDictionary) は配る追加辞書 (extra_dictionary.tsv) の語にも付くので、登録した語だけを見分けるのに使う
    /// (Zenzai で先頭のトークンを学習した語と同じ優先で判断し、以降は表記どおり残す。上流 AzooKeyKanaKanjiConverter #370 の 3 つ目に当たる)
    public static let isUserRegistered = DicdataElementMetadata(rawValue: 1 << 3) // 8
    /// azooKey-Windows (2026-11-08): 常用外の字の表記 (嗾ける・阿弥陀籤。DicdataStore.rareSpellingDomains の分野辞書の語) にはこのフラグを立てる。
    /// 変換サーバーは候補の一覧で、表が無いときの 1 番目に当たる候補がふつうの書き方なら、この語の形をその下に置く
    public static let isRareSpelling = DicdataElementMetadata(rawValue: 1 << 4) // 16
}
