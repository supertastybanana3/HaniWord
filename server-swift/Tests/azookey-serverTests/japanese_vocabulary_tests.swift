import Foundation
import XCTest
@testable import azookey_server

final class JapaneseVocabularyTests: XCTestCase {
    private let vocabulary = JapaneseVocabulary(schemaVersion: 1, entries: [
        .init(reading: "うんがき", word: "運ガキ", category: .slang, preferStandalone: true),
        .init(reading: "ぎゅならし", word: "ギュ鳴らし", category: .slang, preferStandalone: true),
        .init(reading: "ぎゅられる", word: "ギュられる", category: .slang, preferStandalone: true, partOfSpeech: .verb),
        .init(reading: "こみゅの", word: "コミュノ", category: .slang),
        .init(reading: "とうとし", word: "尊死", category: .slang),
        .init(reading: "いんぷぞんび", word: "インプゾンビ", category: .slang),
        .init(reading: "りさん", word: "理三", category: .admission),
        .init(reading: "ぶんに", word: "文二", category: .admission),
        .init(reading: "ぶんいち", word: "文一", category: .admission, recentAuthors: 30)
    ])

    func testCandidatesAreLowPriorityAndExactReadingOnly() {
        let before = ["分一", "文位置", "分位置", "文市", "分市", "ぶんいち"]
        XCTAssertEqual(vocabulary.insertingAlternatives(reading: "ぶんいち", into: before),
                       ["分一", "文位置", "分位置", "文市", "分市", "文一", "ぶんいち"])
        XCTAssertEqual(vocabulary.insertingAlternatives(reading: "ぶんいちをしてい", into: before), before)
        XCTAssertEqual(vocabulary.insertingAlternatives(reading: "ぶんいち", into: ["分一", "文一"]), ["分一", "文一"])
        XCTAssertEqual(vocabulary.insertingAlternatives(reading: "ぶんいち", into: []), ["文一"])
    }

    func testAdmissionCueProducesIndependentAbbreviationAndParticleScopes() throws {
        let wish = try XCTUnwrap(vocabulary.prefixOverride(for: "りさんにいきたい"))
        XCTAssertEqual(wish.segments.map(\.word), ["理三", "に", "行きたい"])
        XCTAssertEqual(wish.segments.map(\.rubyCount), [3, 1, 4])
        XCTAssertEqual(wish.count, 8)
        let target = try XCTUnwrap(vocabulary.prefixOverride(for: "ぶんいちしぼうです"))
        XCTAssertEqual(target.segments.map(\.word), ["文一", "志望"])
        XCTAssertEqual(target.count, 7)
        XCTAssertNil(vocabulary.prefixOverride(for: "りさん"))
    }

    func testDeathAndFatAreNotAdmissionCues() {
        for reading in ["りさんしぼうりつ", "りさんしぼうをへらす", "りさんしぼうした", "りさんしぼうしゃ",
                        "ぶんいちしぼうりつ", "えりさんしぼう", "りさんすうがく", "りさんにいきたいぷ"] {
            XCTAssertNil(vocabulary.prefixOverride(for: reading), reading)
        }
    }

    func testSlangPreferenceDoesNotRewriteOrdinaryLuckSentences() throws {
        for reading in ["うんがき", "うんがきだ", "うんがきのはなし"] {
            XCTAssertEqual(try XCTUnwrap(vocabulary.prefixOverride(for: reading)).segments.map(\.word), ["運ガキ"])
        }
        for reading in ["うんがきになる", "うんがきれた", "うんがきめてだ", "うんがきになった", "うんがきがする"] {
            XCTAssertNil(vocabulary.prefixOverride(for: reading), reading)
        }
        XCTAssertEqual(vocabulary.prefixOverride(for: "ぎゅならしのはなし")?.count, 5)
        XCTAssertNotNil(vocabulary.prefixOverride(for: "ぎゅられる"))
        XCTAssertNil(vocabulary.prefixOverride(for: "ぎゅられるのはなし"))
    }

    func testAmbiguousSlangRemainsAnAlternative() {
        XCTAssertNil(vocabulary.prefixOverride(for: "とうとし"))
        XCTAssertNil(vocabulary.prefixOverride(for: "こみゅのはなし"))
        XCTAssertEqual(vocabulary.refiningScopes(reading: "うんがきになる",
            segments: [("運", 2), ("が", 1), ("気", 1), ("に", 1), ("なる", 2)]).map(\.word), ["運", "が", "気", "に", "なる"])
        XCTAssertEqual(vocabulary.refiningScopes(reading: "こみゅのはなし",
            segments: [("コミュ", 3), ("の", 1), ("話", 3)]).map(\.word), ["コミュ", "の", "話"])
    }

    func testKnownCompoundUsesOneScopeWithoutSwallowingItsParticle() {
        let parts = vocabulary.refiningScopes(reading: "いんぷぞんびがふえた",
            segments: [("インプ", 3), ("ゾンビ", 3), ("が", 1), ("増えた", 3)])
        XCTAssertEqual(parts.map(\.word), ["インプゾンビ", "が", "増えた"])
        XCTAssertEqual(parts.map(\.rubyCount), [6, 1, 3])
    }

    func testNonNounsCannotUseNominalContinuationRules() {
        let inflected = JapaneseVocabulary(schemaVersion: 1, entries: [
            .init(reading: "めろい", word: "メロい", category: .slang, preferStandalone: true, partOfSpeech: .iAdjective),
            .init(reading: "めろがる", word: "メロがる", category: .slang, preferStandalone: true, partOfSpeech: .verb)
        ])
        XCTAssertNotNil(inflected.prefixOverride(for: "めろい"))
        XCTAssertNil(inflected.prefixOverride(for: "めろいだ"))
        XCTAssertNil(inflected.prefixOverride(for: "めろがるです"))
        XCTAssertEqual(inflected.refiningScopes(reading: "めろがるの",
            segments: [("目", 1), ("路", 1), ("がる", 2), ("の", 1)]).count, 4)
    }

    func testStandaloneCandidateScopePreservesExistingText() {
        let parts = vocabulary.refiningScopes(reading: "ぶんに", segments: [("文", 2), ("に", 1)])
        XCTAssertEqual(parts.map(\.word), ["文に"])
        XCTAssertEqual(parts.map(\.rubyCount), [3])
        let invalid = vocabulary.refiningScopes(reading: "ぶんに", segments: [("文", 0), ("に", 1)])
        XCTAssertEqual(invalid.map(\.word), ["文", "に"])
    }

    func testResourceLoadingRejectsUnknownSchemaAndInvalidEntries() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".json")
        defer { try? FileManager.default.removeItem(at: url) }
        let entry: [String: Any] = ["reading": "うんがき", "word": "運ガキ", "category": "slang"]
        try JSONSerialization.data(withJSONObject: ["schema_version": 2, "entries": [entry]]).write(to: url)
        XCTAssertTrue(JapaneseVocabulary.load(from: url).entries.isEmpty)
        let invalid: [String: Any] = ["reading": "unkaki", "word": "運ガキ", "category": "slang"]
        try JSONSerialization.data(withJSONObject: ["schema_version": 1, "entries": [entry, entry, invalid]]).write(to: url)
        let loaded = JapaneseVocabulary.load(from: url)
        XCTAssertEqual(loaded.entries.count, 1)
        XCTAssertFalse(try XCTUnwrap(loaded.entries.first).preferStandalone)
        try Data("broken".utf8).write(to: url)
        XCTAssertTrue(JapaneseVocabulary.load(from: url).entries.isEmpty)
    }

    /// 2026-11-22: 表は回数 (recent_authors) でなく段 (slot) を持つ。段がそのまま並べる位置になり、あいまいな読みは 16 より前に来ない
    func testResourceSlotSetsTheInsertionBand() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".json")
        defer { try? FileManager.default.removeItem(at: url) }
        let entries: [[String: Any]] = [
            ["reading": "えいこみゅ", "word": "英コミュ", "category": "subject", "slot": 5],
            ["reading": "こみえい", "word": "コミ英", "category": "subject", "slot": 18],
            ["reading": "かき", "word": "化基", "category": "subject", "slot": 10, "ambiguous": true],
        ]
        try JSONSerialization.data(withJSONObject: ["schema_version": 1, "entries": entries]).write(to: url)
        let loaded = JapaneseVocabulary.load(from: url)
        XCTAssertEqual(loaded.entries.map(\.word), ["英コミュ", "コミ英", "化基"])
        XCTAssertEqual(loaded.entries.map(\.candidateSlot), [5, 18, 16])
    }

    func testPublicResourceContainsVerifiedRequestedWords() throws {
        guard let path = ProcessInfo.processInfo.environment["AZK_TEST_VOCABULARY"] else {
            throw XCTSkip("Run with the curated vocabulary resource")
        }
        let loaded = JapaneseVocabulary.load(from: URL(filePath: path))
        XCTAssertEqual(loaded.entries.count, 69)
        XCTAssertTrue(loaded.alternatives(for: "どぱがき").isEmpty)
        XCTAssertEqual(loaded.alternatives(for: "うんがき"), ["運ガキ"])
        XCTAssertEqual(loaded.alternatives(for: "りさん"), ["理三", "理Ⅲ"])
        for (reading, word, variant) in [("りいち", "理一", "理Ⅰ"), ("りに", "理二", "理Ⅱ"), ("りさん", "理三", "理Ⅲ"),
                                         ("ぶんいち", "文一", "文Ⅰ"), ("ぶんに", "文二", "文Ⅱ"), ("ぶんさん", "文三", "文Ⅲ")] {
            XCTAssertEqual(loaded.alternatives(for: reading), [word, variant])
            XCTAssertEqual(loaded.insertingAlternatives(reading: reading, into: [word, "別候補"]), [word, variant, "別候補"])
            XCTAssertEqual(loaded.insertingAlternatives(reading: reading, into: [variant, "別候補"]), [variant, word, "別候補"])
            XCTAssertEqual(loaded.prefixOverride(for: reading + "しぼう")?.segments.map(\.word), [word, "志望"])
            XCTAssertEqual(loaded.prefixOverride(for: reading + "にいきたい")?.segments.map(\.word), [word, "に", "行きたい"])
        }
        XCTAssertTrue(loaded.alternatives(for: "かがくだいい").isEmpty)
        XCTAssertEqual(loaded.alternatives(for: "えいこみゅ"), ["英コミュ"])
        XCTAssertEqual(loaded.alternatives(for: "げんぶん"), ["現文", "言文"])
        XCTAssertTrue(loaded.alternatives(for: "きょうて").isEmpty)
        XCTAssertEqual(loaded.entries.filter(\.preferStandalone).count, 3)
        // 公開の表は段だけを持ち、検索の回数は持たない
        XCTAssertTrue(loaded.entries.allSatisfy { $0.slot != nil && $0.recentAuthors == 0 })
    }

    func testFrequencyAndCollisionRiskControlSupplementaryRank() {
        let frequent = JapaneseVocabulary.Entry(reading: "えいこみゅ", word: "英コミュ", category: .subject, recentAuthors: 38)
        let uncommon = JapaneseVocabulary.Entry(reading: "こみえい", word: "コミ英", category: .subject, recentAuthors: 2)
        let colliding = JapaneseVocabulary.Entry(reading: "かき", word: "化基", category: .subject, recentAuthors: 11, ambiguous: true)
        XCTAssertEqual(frequent.candidateSlot, 5)
        XCTAssertEqual(uncommon.candidateSlot, 18)
        XCTAssertEqual(colliding.candidateSlot, 16)
        let ranked = JapaneseVocabulary(schemaVersion: 1, entries: [uncommon, frequent, colliding])
        let before = (0..<25).map(String.init)
        XCTAssertEqual(ranked.insertingAlternatives(reading: "えいこみゅ", into: before)[5], "英コミュ")
        XCTAssertEqual(ranked.insertingAlternatives(reading: "こみえい", into: before)[18], "コミ英")
        XCTAssertEqual(ranked.insertingAlternatives(reading: "かき", into: before)[16], "化基")
        XCTAssertEqual(ranked.insertingAlternatives(reading: "かき", into: before).filter { $0 != "化基" }, before)
    }

    func testEquivalentSpellingsSharePriorityWithoutPromotingUnrelatedHomophones() {
        let entry = JapaneseVocabulary.Entry(reading: "りさん", word: "理三", category: .admission,
            recentAuthors: 38, ambiguous: true, variants: ["理Ⅲ"])
        let vocabulary = JapaneseVocabulary(schemaVersion: 1, entries: [entry])
        let ordinary = ["離散"] + (1..<20).map(String.init)
        let result = vocabulary.insertingAlternatives(reading: "りさん", into: ordinary)
        XCTAssertEqual(Array(result[16...17]), ["理三", "理Ⅲ"])
        XCTAssertEqual(result.first, "離散")
        XCTAssertEqual(result.filter { !["理三", "理Ⅲ"].contains($0) }, ordinary)
        XCTAssertEqual(vocabulary.insertingAlternatives(reading: "りさん", into: ["理三", "離散", "理Ⅲ"]), ["理三", "理Ⅲ", "離散"])
        XCTAssertEqual(vocabulary.insertingAlternatives(reading: "りさんすうがく", into: ["離散数学"]), ["離散数学"])
        XCTAssertNil(vocabulary.prefixOverride(for: "りさんすうがく"))
    }
}
