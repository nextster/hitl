import CryptoKit
import Foundation
import Network

/// What the Mac put on this phone by cable: where it is at home, and the key
/// it knows this phone by. Without it the app has no Mac to talk to.
struct MacWord: Codable, Equatable, Sendable {
    var name: String
    var hosts: [String]
    var port: Int
    var token: String
    /// The print of the one certificate the Mac answers with.
    var pin: String
    /// The server's room, where the two meet when the phone is not at home; nil until one is set up.
    var relay: RelayWord?

    static var file: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("hitl/mac.json")
    }

    static func read() -> MacWord? {
        guard let data = try? Data(contentsOf: file) else { return nil }
        return try? JSONDecoder().decode(MacWord.self, from: data)
    }

    /// Keeps what the Mac said at home of its server's room, beside what it put here by cable.
    func keep() {
        guard let data = try? JSONEncoder().encode(self) else { return }
        try? data.write(to: MacWord.file, options: .atomic)
    }
}

/// A way to the Mac that is in use: at home, or through the server's room.
protocol MacWire: AnyObject {
    func say(_ word: [String: Any])
    func close()
}

extension MacLink: MacWire {}
extension RelayLink: MacWire {}

/// One connection to the Mac: lines of JSON each way, over TLS with the Mac's
/// own certificate, known by its print.
final class MacLink {
    enum Ended { case refused, unreachable, lost }

    private let connection: NWConnection
    private var heard = Data()
    private var isOver = false
    private var isUp = false
    /// The connection is up: the key can be said.
    var opened: (() -> Void)?
    var line: ((Data) -> Void)?
    var ended: ((Ended) -> Void)?

    init(host: String, word: MacWord) {
        connection = NWConnection(host: NWEndpoint.Host(host), port: NWEndpoint.Port(rawValue: UInt16(clamping: word.port)) ?? 1,
                                  using: MacLink.parameters(pin: word.pin))
    }

    /// Made outside the main actor: the system checks the certificate on a queue of its own.
    nonisolated private static func parameters(pin: String) -> NWParameters {
        let tls = NWProtocolTLS.Options()
        sec_protocol_options_set_verify_block(tls.securityProtocolOptions, { _, trust, complete in
            let chain = SecTrustCopyCertificateChain(sec_trust_copy_ref(trust).takeRetainedValue()) as? [SecCertificate]
            let leaf = chain?.first.map { SecCertificateCopyData($0) as Data }
            let print = leaf.map { SHA256.hash(data: $0).map { String(format: "%02x", $0) }.joined() }
            complete(print == pin)
        }, DispatchQueue.global(qos: .userInitiated))
        let tcp = NWProtocolTCP.Options()
        tcp.connectionTimeout = 4
        tcp.enableKeepalive = true
        tcp.keepaliveIdle = 15
        tcp.noDelay = true
        return NWParameters(tls: tls, tcp: tcp)
    }

    func start() {
        connection.stateUpdateHandler = { [weak self] state in
            MainActor.assumeIsolated { self?.changed(state) }
        }
        // The network it went by is gone (Wi‑Fi switched off): it is over now, not when the system gives up asking.
        connection.viabilityUpdateHandler = { [weak self] isViable in
            MainActor.assumeIsolated {
                guard let self, !isViable, !self.heard.isEmpty || self.isUp else { return }
                self.end(.lost)
            }
        }
        connection.start(queue: .main)
    }

    private func changed(_ state: NWConnection.State) {
        switch state {
        case .ready:
            isUp = true
            opened?()
            listen()
        case .waiting(let error), .failed(let error):
            // Nobody listens on the Mac's port: the Mac is there, and nothing waits in its queue.
            if case .posix(let code) = error, code == .ECONNREFUSED { end(.refused) } else { end(heard.isEmpty ? .unreachable : .lost) }
        case .cancelled: end(.lost)
        default: break
        }
    }

    private func listen() {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 1 << 20) { [weak self] data, _, isComplete, error in
            MainActor.assumeIsolated {
                guard let self, !self.isOver else { return }
                if let data, !data.isEmpty {
                    self.heard.append(data)
                    while let end = self.heard.firstIndex(of: 0x0A) {
                        let one = self.heard.subdata(in: self.heard.startIndex..<end)
                        self.heard.removeSubrange(self.heard.startIndex...end)
                        if !one.isEmpty { self.line?(one) }
                    }
                }
                if isComplete || error != nil { self.end(.lost) } else { self.listen() }
            }
        }
    }

    func say(_ word: [String: Any]) {
        guard !isOver, let data = try? JSONSerialization.data(withJSONObject: word) else { return }
        connection.send(content: data + Data([0x0A]), completion: .contentProcessed { _ in })
    }

    func close() {
        guard !isOver else { return }
        isOver = true
        connection.stateUpdateHandler = nil
        connection.viabilityUpdateHandler = nil
        connection.cancel()
    }

    private func end(_ how: Ended) {
        guard !isOver else { return }
        isOver = true
        connection.stateUpdateHandler = nil
        connection.viabilityUpdateHandler = nil
        connection.cancel()
        ended?(how)
    }
}
