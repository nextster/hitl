import CryptoKit
import Foundation

/// The seal of what is said in the server's room, the phone's half; the Mac's is `src/hitl/relay.py`,
/// where a frame is laid out. The server between the two is handed frames it can neither read nor make:
/// the key lies on the Mac and on this phone, and nowhere else.
struct RelaySeal {
    /// How much of a line goes in one frame.
    static let partBytes = 96 * 1024
    /// A frame older than this is not taken: what was said once is not said again by somebody who kept it.
    static let freshSeconds = 600.0

    private let cipher: SymmetricKey
    private let tag: SymmetricKey
    private let mine: UInt8
    private let theirs: UInt8
    private let launch: [UInt8]
    private var count: UInt64 = 0
    /// The last number heard of each launch of the other side, and when.
    private var heard: [[UInt8]: (count: UInt64, at: Double)] = [:]
    private var parts: [UInt8] = []
    private var speaker: [UInt8] = []

    /// `mine` and `theirs` are who speaks, "P" the phone and "M" the Mac: a frame is opened only as the other
    /// side's.
    init?(key hex: String, mine: Character, theirs: Character) {
        var key: [UInt8] = []
        var rest = Substring(hex)
        while rest.count >= 2, let byte = UInt8(rest.prefix(2), radix: 16) {
            key.append(byte)
            rest = rest.dropFirst(2)
        }
        guard rest.isEmpty, key.count >= 32, let mine = mine.asciiValue, let theirs = theirs.asciiValue else { return nil }
        let master = SymmetricKey(data: key)
        cipher = SymmetricKey(data: Data(HMAC<SHA256>.authenticationCode(for: Data("hitl relay: cipher".utf8), using: master)))
        tag = SymmetricKey(data: Data(HMAC<SHA256>.authenticationCode(for: Data("hitl relay: tag".utf8), using: master)))
        self.mine = mine
        self.theirs = theirs
        launch = (0..<8).map { _ in UInt8.random(in: .min ... .max) }
    }

    private static func bytes<Number: FixedWidthInteger>(_ number: Number) -> [UInt8] {
        withUnsafeBytes(of: number.bigEndian) { Array($0) }
    }

    /// Mixes bytes with the stream of this nonce; done twice, it gives them back.
    private func mixed(_ bytes: [UInt8], nonce: [UInt8]) -> [UInt8] {
        var out = bytes
        var block: UInt32 = 0
        var at = 0
        out.withUnsafeMutableBufferPointer { out in
            while at < out.count {
                let stream = HMAC<SHA256>.authenticationCode(for: nonce + Self.bytes(block), using: cipher)
                stream.withUnsafeBytes { stream in
                    for place in 0..<min(32, out.count - at) { out[at + place] ^= stream[place] }
                }
                at += 32
                block += 1
            }
        }
        return out
    }

    private func signed(_ speaker: UInt8, _ nonce: [UInt8], _ sealed: [UInt8]) -> [UInt8] {
        Array(HMAC<SHA256>.authenticationCode(for: [speaker] + nonce + sealed, using: tag))
    }

    /// A line as the frames to say it in, in their order.
    mutating func frames(_ line: Data, now: Double = Date().timeIntervalSince1970) -> [Data] {
        let whole = [UInt8](line)
        var frames: [Data] = []
        var at = 0
        repeat {
            let piece = Array(whole[at..<min(at + Self.partBytes, whole.count)])
            at += Self.partBytes
            count += 1
            let plain = launch + Self.bytes(count) + Self.bytes(UInt64(now * 1000)) + [at < whole.count ? 1 : 0] + piece
            let nonce = (0..<12).map { _ in UInt8.random(in: .min ... .max) }
            let sealed = mixed(plain, nonce: nonce)
            frames.append(Data(nonce + sealed + signed(mine, nonce, sealed)))
        } while at < whole.count
        return frames
    }

    /// The line a frame ends; nil for a frame that is a part of one, was not sealed by the other side with
    /// this key, is old, or was heard before.
    mutating func line(_ frame: Data, now: Double = Date().timeIntervalSince1970) -> Data? {
        let frame = [UInt8](frame)
        guard frame.count >= 12 + 25 + 32 else { return nil }
        let nonce = Array(frame[..<12]), sealed = Array(frame[12..<(frame.count - 32)]), said = Array(frame[(frame.count - 32)...])
        let expected = signed(theirs, nonce, sealed)
        // Compared whole, so that the time it takes says nothing of where two tags differ.
        guard zip(said, expected).reduce(UInt8(0), { $0 | ($1.0 ^ $1.1) }) == 0 else { return nil }
        let plain = mixed(sealed, nonce: nonce)
        let from = Array(plain[..<8])
        let number = plain[8..<16].reduce(UInt64(0)) { $0 << 8 | UInt64($1) }
        let moment = Double(plain[16..<24].reduce(UInt64(0)) { $0 << 8 | UInt64($1) }) / 1000
        guard abs(now - moment) <= Self.freshSeconds, number > (heard[from]?.count ?? 0) else { return nil }
        heard[from] = (number, now)
        if heard.count > 8 { heard = heard.filter { now - $0.value.at < 2 * Self.freshSeconds } }
        if from != speaker {
            // Another launch of the other side: what the earlier one left half said is not a part of this.
            speaker = from
            parts = []
        }
        parts += plain[25...]
        guard plain[24] == 0 else { return nil }
        defer { parts = [] }
        return Data(parts)
    }
}
