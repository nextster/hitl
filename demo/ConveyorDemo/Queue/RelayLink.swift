import Foundation

/// The room of the server where this phone and the Mac meet when they are not in one network: where it is,
/// which room, the word the server lets in by, and the key that seals what is said there. The Mac puts it
/// on the phone by cable with the rest of its word, or says it at home.
struct RelayWord: Codable, Equatable, Sendable {
    var url: String
    var room: String
    var token: String
    var key: String
}

/// The way to the Mac through the server's room: a WebSocket to the server, lines of JSON sealed each way.
/// The server says in the open only whether the Mac is in the room.
@MainActor
final class RelayLink {
    enum Mac: Equatable {
        /// The server has not said yet.
        case unknown
        case here
        /// The Mac left by itself with nothing in its queue.
        case resting
        /// The Mac is not in the room and did not say why: it sleeps, is off, or has no network.
        case gone
    }

    private(set) var mac: Mac = .unknown
    var macChanged: ((Mac) -> Void)?
    /// The server holds what this phone says while the Mac is away, and hands it to the Mac when it comes: it says
    /// so as the phone comes in. A server of before that carries only to a Mac that is there.
    private(set) var holds = false
    /// The server's word of each frame said while the Mac was away: held for it, or not, in the order they were said.
    var held: ((Bool) -> Void)?
    var line: ((Data) -> Void)?
    /// The way to the server is lost, or there never was one: why, in the system's own words.
    var ended: ((String) -> Void)?

    private let task: URLSessionWebSocketTask
    private var seal: RelaySeal
    private var listening: Task<Void, Never>?
    private var pinging: Task<Void, Never>?
    private var sending: Task<Void, Never>?
    private var lastHeard = Date()
    private var isOver = false

    init?(word: RelayWord) {
        guard let base = URL(string: word.url), let seal = RelaySeal(key: word.key, mine: "P", theirs: "M") else { return nil }
        var request = URLRequest(url: base.appendingPathComponent("room").appendingPathComponent(word.room).appendingPathComponent("phone"),
                                 timeoutInterval: 10)
        request.setValue("Bearer " + word.token, forHTTPHeaderField: "Authorization")
        task = URLSession.shared.webSocketTask(with: request)
        self.seal = seal
    }

    func start() {
        task.resume()
        listening = Task { [weak self, task] in
            while !Task.isCancelled {
                do {
                    let message = try await task.receive()
                    self?.heard(message)
                } catch {
                    self?.end(RelayLink.words(error))
                    return
                }
            }
        }
        // A word now and then keeps the way open, and a server that has answered nothing for a while is gone.
        pinging = Task { [weak self, task] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(20))
                guard !Task.isCancelled, let self else { return }
                if Date().timeIntervalSince(self.lastHeard) > 70 { return self.end("сервер молчит больше минуты") }
                do { try await task.send(.string("ping")) } catch { return self.end(RelayLink.words(error)) }
            }
        }
    }

    private func heard(_ message: URLSessionWebSocketTask.Message) {
        guard !isOver else { return }
        lastHeard = Date()
        switch message {
        case .string(let text):
            // The server's own words: whether the Mac is in the room, whether it holds, and what it held.
            guard let word = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else { return }
            if let kept = word["held"] as? Int { return held?(kept == 1) ?? () }
            if word["holds"] as? Bool == true { holds = true }
            guard let state = word["mac"] as? String else { return }
            mac = state == "here" ? .here : state == "resting" ? .resting : .gone
            macChanged?(mac)
        case .data(let frame):
            if let whole = seal.line(frame) { line?(whole) }
        @unknown default:
            break
        }
    }

    func say(_ word: [String: Any]) {
        sayCounting(word)
    }

    /// Says a word, and in how many frames: the server says of each whether it held it.
    @discardableResult
    func sayCounting(_ word: [String: Any]) -> Int {
        guard !isOver, let data = try? JSONSerialization.data(withJSONObject: word) else { return 0 }
        let frames = seal.frames(data)
        // One after another, in the order they were said.
        sending = Task { [weak self, task, earlier = sending] in
            await earlier?.value
            for frame in frames {
                do { try await task.send(.data(frame)) } catch {
                    self?.end(RelayLink.words(error))
                    return
                }
            }
        }
        return frames.count
    }

    func close() {
        guard !isOver else { return }
        isOver = true
        listening?.cancel()
        pinging?.cancel()
        task.cancel(with: .goingAway, reason: nil)
    }

    private func end(_ why: String) {
        guard !isOver else { return }
        close()
        ended?(why)
    }

    /// What the system says of a failure, with its number: «нет интернета» and «сервер не найден» are told apart by it.
    private static func words(_ error: Error) -> String {
        let error = error as NSError
        return "\(error.localizedDescription) (\(error.domain == NSURLErrorDomain ? "" : error.domain + " ")\(error.code))"
    }
}
