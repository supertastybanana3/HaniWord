package enum ZenzBatchLimits {
    package static func allocation(tokenCount: Int, from: Int, contextCapacity: Int, decodeCapacity: Int) -> Int? {
        guard tokenCount > 0, tokenCount <= Int(Int32.max), from >= 0, from < tokenCount,
              tokenCount <= contextCapacity, contextCapacity > 0, decodeCapacity > 0 else { return nil }
        let remaining = tokenCount - from
        guard remaining <= decodeCapacity, remaining <= Int(Int32.max) else { return nil }
        return remaining
    }

    package static func canAppend(count: Int32, capacity: Int, sequences: Int, sequenceCapacity: Int) -> Bool {
        count >= 0 && Int(count) < capacity && capacity <= Int(Int32.max)
            && sequences > 0 && sequences <= sequenceCapacity
    }
}
