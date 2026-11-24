//
//  extension Data.swift
//  Keyboard
//
//  Created by ensan on 2020/09/30.
//  Copyright © 2020 ensan. All rights reserved.
//

import Foundation
import SwiftUtils

extension LOUDS {
    private static func loadLOUDSBinary(from url: URL) -> [UInt64]? {
        do {
            let binaryData = try Data(contentsOf: url, options: [.uncached]) // 2度読み込むことはないのでキャッシュ不要
            let ui64array = binaryData.toArray(of: UInt64.self)
            return ui64array
        } catch {
            debug(error)
            return nil
        }
    }

    private static func getLOUDSURL(_ identifier: String, option: ConvertRequestOptions) -> (chars: URL, louds: URL) {

        if identifier == "user"{
            return (
                option.sharedContainerURL.appendingPathComponent("user.loudschars2", isDirectory: false),
                option.sharedContainerURL.appendingPathComponent("user.louds", isDirectory: false)
            )
        }
        if identifier.hasPrefix("domain_"), let dir = DicdataStore.domainDirectoryURL {
            return (
                dir.appendingPathComponent("\(identifier).loudschars2", isDirectory: false),
                dir.appendingPathComponent("\(identifier).louds", isDirectory: false)
            )
        }
        if identifier == "memory"{
            return (
                option.memoryDirectoryURL.appendingPathComponent("memory.loudschars2", isDirectory: false),
                option.memoryDirectoryURL.appendingPathComponent("memory.louds", isDirectory: false)
            )
        }
        return (
            option.dictionaryResourceURL.appendingPathComponent("louds/\(identifier).loudschars2", isDirectory: false),
            option.dictionaryResourceURL.appendingPathComponent("louds/\(identifier).louds", isDirectory: false)
        )
    }

    /// azooKey-Windows (2026-11-18、perf の 7): 本体の辞書の LOUDS を、置き場 (Dictionary/louds) を直接渡して読む。
    /// 別のスレッドで読むため (ConvertRequestOptions を渡さない)。読み方は load(_:option:) と同じ
    package static func loadMainDictionary(_ identifier: String, directory: URL) -> LOUDS? {
        let charsURL = directory.appendingPathComponent("\(identifier).loudschars2", isDirectory: false)
        let loudsURL = directory.appendingPathComponent("\(identifier).louds", isDirectory: false)
        guard let nodeIndex2ID = try? Array(Data(contentsOf: charsURL, options: [.uncached])),
              let bytes = LOUDS.loadLOUDSBinary(from: loudsURL) else { return nil }
        return LOUDS(bytes: bytes.map {$0.littleEndian}, nodeIndex2ID: nodeIndex2ID)
    }

    /// azooKey-Windows (2026-11-18、perf の 7): 本体の辞書の .loudstxt3 を、先読みと同じく写像して返す (DicdataStore.mapPreloadedDictionary が偽なら読み込む)
    package static func mapLoudstxt3(_ identifier: String, option: ConvertRequestOptions) -> Data? {
        try? Data(contentsOf: getLoudstxt3URL(identifier, option: option), options: DicdataStore.mapPreloadedDictionary ? [.alwaysMapped] : [])
    }

    /// azooKey-Windows: .loudstxt3 を丸ごと読む (分野辞書のファイルを DicdataStore が持っておくため)。
    /// 読む前に大きさを確かめ、maxBytes を超えるファイルは読まない (2026-10-22、D の調べ R2-3)
    package static func loadLoudstxt3(_ identifier: String, option: ConvertRequestOptions, maxBytes: Int) -> Data? {
        readBounded(getLoudstxt3URL(identifier, option: option), maxBytes: maxBytes)
    }

    /// azooKey-Windows: 大きさが maxBytes 以下のファイルだけを読む (読む前に大きさを確かめる)
    package static func readBounded(_ url: URL, maxBytes: Int) -> Data? {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        guard let size = try? handle.seekToEnd(), size <= UInt64(maxBytes), (try? handle.seek(toOffset: 0)) != nil,
              let data = try? handle.read(upToCount: Int(size) + 1), data.count <= maxBytes else { return nil }
        return data
    }

    /// azooKey-Windows: ファイルの大きさが maxBytes 以下か (開いて終わりへ移るだけで、中身は読まない)
    package static func sizeAtMost(_ url: URL, _ maxBytes: Int) -> Bool {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return false }
        defer { try? handle.close() }
        guard let size = try? handle.seekToEnd() else { return false }
        return size <= UInt64(maxBytes)
    }

    /// azooKey-Windows: 分野辞書・追加辞書 (domain_) の .louds・.loudschars2・.loudstxt3 の 1 ファイルの大きさの上限。配っているのは最大 0.2MB ほど
    package static let domainFileMaxBytes = 8 << 20

    private static func getLoudstxt3URL(_ identifier: String, option: ConvertRequestOptions) -> URL {
        if identifier.hasPrefix("user") {
            return option.sharedContainerURL.appendingPathComponent("\(identifier).loudstxt3", isDirectory: false)
        }
        if identifier.hasPrefix("domain_"), let dir = DicdataStore.domainDirectoryURL {
            return dir.appendingPathComponent("\(identifier).loudstxt3", isDirectory: false)
        }
        if identifier.hasPrefix("memory") {
            return option.memoryDirectoryURL.appendingPathComponent("\(identifier).loudstxt3", isDirectory: false)
        }
        return option.dictionaryResourceURL.appendingPathComponent("louds/\(identifier).loudstxt3", isDirectory: false)
    }

    /// LOUDSをファイルから読み込む関数
    /// - Parameter identifier: ファイル名
    /// - Returns: 存在すればLOUDSデータを返し、存在しなければ`nil`を返す。
    package static func load(_ identifier: String, option: ConvertRequestOptions) -> LOUDS? {
        let (charsURL, loudsURL) = getLOUDSURL(identifier, option: option)
        // azooKey-Windows: 分野辞書・追加辞書は利用者の場所に置くので、大きすぎるファイルは読まない (2026-10-22、D の調べ R2-3)
        if identifier.hasPrefix("domain_"), !(sizeAtMost(charsURL, domainFileMaxBytes) && sizeAtMost(loudsURL, domainFileMaxBytes)) {
            return nil
        }
        let nodeIndex2ID: [UInt8]
        do {
            nodeIndex2ID = try Array(Data(contentsOf: charsURL, options: [.uncached]))   // 2度読み込むことはないのでキャッシュ不要
        } catch {
            debug("Error: \(identifier)に対するLOUDSファイルが存在しません。このエラーは無視できる可能性があります。 Description: \(error)")
            return nil
        }

        if let bytes = LOUDS.loadLOUDSBinary(from: loudsURL) {
            let louds = LOUDS(bytes: bytes.map {$0.littleEndian}, nodeIndex2ID: nodeIndex2ID)
            return louds
        }
        return nil
    }

    @inlinable
    static func parseBinary(binary: Data) -> [DicdataElement] {
        // azooKey-Windows (2026-11-23、セキュリティの点検 12): 切れた項目を範囲の外まで読まない
        guard binary.count >= 2 else { return [] }
        // 最初の2byteがカウント
        let count = binary[binary.startIndex ..< binary.startIndex + 2].toArray(of: UInt16.self)[0]
        guard binary.count >= 2 + 10 * Int(count) else { return [] }
        var index = binary.startIndex + 2
        var dicdata: [DicdataElement] = []
        dicdata.reserveCapacity(Int(count))
        for _ in 0 ..< count {
            let ids = binary[index ..< index + 6].toArray(of: UInt16.self)
            let value = binary[index + 6 ..< index + 10].toArray(of: Float32.self)[0]
            dicdata.append(DicdataElement(word: "", ruby: "", lcid: Int(ids[0]), rcid: Int(ids[1]), mid: Int(ids[2]), value: PValue(value)))
            index += 10
        }

        let substrings = binary[index...].split(separator: UInt8(ascii: "\t"), omittingEmptySubsequences: false)
        guard let ruby = String(data: substrings[0], encoding: .utf8) else {
            debug("getDataForLoudstxt3: failed to parse", dicdata)
            return []
        }
        for (index, substring) in zip(dicdata.indices, substrings[1...]) {
            guard let word = String(data: substring, encoding: .utf8) else {
                debug("getDataForLoudstxt3: failed to parse", ruby)
                continue
            }
            withMutableValue(&dicdata[index]) {
                $0.ruby = ruby
                $0.word = word.isEmpty ? ruby : word
            }
        }
        return dicdata

    }

    static func getDataForLoudstxt3(_ identifier: String, indices: [Int], cache: Data? = nil, option: ConvertRequestOptions) -> [DicdataElement] {
        let binary: Data
        
        if let cache {
            binary = cache
        } else {
            do {
                let url = getLoudstxt3URL(identifier, option: option)
                binary = try Data(contentsOf: url)
            } catch {
                debug("getDataForLoudstxt3: \(error)")
                return []
            }
        }

        // azooKey-Windows (2026-11-23、セキュリティの点検 12): 頭・位置の表・項目の範囲を確かめてから切り出す (壊れた学習のファイルで落ちないように)
        guard binary.startIndex == 0, binary.count >= 2 else { return [] }
        let lc = binary[0..<2].toArray(of: UInt16.self)[0]
        let header_endIndex: UInt32 = 2 + UInt32(lc) * UInt32(MemoryLayout<UInt32>.size)
        guard binary.count >= Int(header_endIndex) else { return [] }
        let ui32array = binary[2..<header_endIndex].toArray(of: UInt32.self)

        let result: [DicdataElement] = indices.flatMap {(index: Int) -> [DicdataElement] in
            guard index >= 0, index < Int(lc) else { return [] }
            let startIndex = Int(ui32array[index])
            let endIndex = index == (lc - 1) ? binary.endIndex : Int(ui32array[index + 1])
            guard Int(header_endIndex) <= startIndex, startIndex <= endIndex, endIndex <= binary.endIndex else { return [] }
            return parseBinary(binary: binary[startIndex ..< endIndex])
        }
        return result
    }

    /// indexとの対応を維持したバージョン
    static func getDataForLoudstxt3(_ identifier: String, indices: [(trueIndex: Int, keyIndex: Int)], cache: Data? = nil, option: ConvertRequestOptions) -> [(loudsNodeIndex: Int, dicdata: [DicdataElement])] {
        let binary: Data

        if let cache {
            binary = cache
        } else {
            do {
                let url = getLoudstxt3URL(identifier, option: option)
                binary = try Data(contentsOf: url)
            } catch {
                debug("getDataForLoudstxt3: \(error)")
                return []
            }
        }

        // azooKey-Windows (2026-11-23、セキュリティの点検 12): 上と同じに、範囲を確かめてから切り出す
        guard binary.startIndex == 0, binary.count >= 2 else { return [] }
        let lc = binary[0..<2].toArray(of: UInt16.self)[0]
        let header_endIndex: UInt32 = 2 + UInt32(lc) * UInt32(MemoryLayout<UInt32>.size)
        guard binary.count >= Int(header_endIndex) else { return [] }
        let ui32array = binary[2..<header_endIndex].toArray(of: UInt32.self)
        var result: [(loudsNodeIndex: Int, dicdata: [DicdataElement])] = []
        for (trueIndex, keyIndex) in indices {
            guard keyIndex >= 0, keyIndex < Int(lc) else { continue }
            let startIndex = Int(ui32array[keyIndex])
            let endIndex = keyIndex == (lc - 1) ? binary.endIndex : Int(ui32array[keyIndex + 1])
            guard Int(header_endIndex) <= startIndex, startIndex <= endIndex, endIndex <= binary.endIndex else { continue }
            result.append((trueIndex, parseBinary(binary: binary[startIndex ..< endIndex])))
        }
        return result
    }
}
