import Foundation
@testable import Bookdrop

/// Pure in-memory `KeyValueStore` for tests — no filesystem or `cfprefsd`
/// interaction at all, so there's nothing to leak or clean up.
final class InMemoryKeyValueStore: KeyValueStore {
    private var storage: [String: Data] = [:]

    func data(forKey key: String) -> Data? { storage[key] }
    func set(_ value: Data?, forKey key: String) { storage[key] = value }
}
