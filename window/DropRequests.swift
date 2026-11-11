// A live panel decides drops on the same main loop as user edits. No polling
// client may unlink a live check based on an older progress-file snapshot.
import Foundation

struct DropRequests {
    static func process(directory: URL, queued: URL, done: URL,
                        prepare: () -> Void, protected: (String) -> Bool, automaticResults: (String) -> [[String: Any]]? = { _ in nil }, didDrop: (String, Data) -> Void) -> Bool {
        var removed = false
        let fm = FileManager.default
        let folder = directory.appendingPathComponent("drops")
        let files = (try? fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.contentModificationDateKey])) ?? []
        for file in files where file.lastPathComponent.hasSuffix(".reply.json") {
            if let date = try? file.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate,
               Date().timeIntervalSince(date) > 60 { try? fm.removeItem(at: file) }
        }
        let requests = files.filter { $0.lastPathComponent.hasSuffix(".request.json") }.sorted { $0.path < $1.path }
        guard !requests.isEmpty else { return false }
        prepare()
        for request in requests.prefix(32) {
            guard let data = try? Data(contentsOf: request), data.count < 2_097_152,
                  let word = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                  let token = word["token"] as? String, token.count == 32, token.allSatisfy({ $0.isHexDigit }),
                  request.lastPathComponent == token + ".request.json",
                  let id = word["id"] as? String, !id.isEmpty, !id.contains("/"), id != ".", id != ".." else { continue }
            var outcome = "unknown"
            let proposed = word["verdict"] as? [String: Any]
            let ticketFile = queued.appendingPathComponent(id + ".json")
            let ticket = (try? Data(contentsOf: ticketFile)).flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any]
            let owner = proposed?["automaticAction"] as? [String: String]
            let automatic = proposed?["how"] as? String == "automatic-resolved"
                && ticket?["kind"] as? String == "action"
                && owner?["type"] == "iphone-blocked"
                && owner?["udid"]?.isEmpty == false && owner?["phoneCheck"]?.isEmpty == false
                && owner == ticket?["automaticAction"] as? [String: String]
            if (word["deadline"] as? Double ?? 0) < Date().timeIntervalSince1970 {
                outcome = "expired"
            } else if !automatic && protected(id) {
                outcome = "protected"
            } else if var verdict = proposed, verdict["id"] as? String == id,
                      verdict["how"] as? String == "dropped" || automatic {
                let ticket = queued.appendingPathComponent(id + ".json")
                let result = done.appendingPathComponent(id + ".json")
                if !fm.fileExists(atPath: ticket.path) {
                    outcome = "missing"
                } else {
                    do {
                        if automatic {
                            guard let results = automaticResults(id) else { throw CocoaError(.fileReadCorruptFile) }
                            verdict["results"] = results
                        }
                        let data = try JSONSerialization.data(withJSONObject: verdict, options: [.sortedKeys])
                        try fm.createDirectory(at: done, withIntermediateDirectories: true)
                        let aside = done.appendingPathComponent(".\(id).\(token).drop")
                        try data.write(to: aside, options: .atomic)
                        defer { try? fm.removeItem(at: aside) }
                        // Hard-link claim matches the terminal-result protocol:
                        // an existing user verdict can never be overwritten.
                        var receipt = data
                        do { try fm.linkItem(at: aside, to: result) }
                        catch {
                            // Recover only our exact technical receipt after a crash.
                            // A user verdict or newer edits keep their protection.
                            let previous = (try? Data(contentsOf: result)).flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any]
                            guard automatic, previous?["how"] as? String == "automatic-resolved",
                                  previous?["automaticAction"] as? [String: String] == owner,
                                  let before = previous?["results"] as? NSArray,
                                  let current = verdict["results"] as? NSArray, before == current else { throw error }
                            receipt = try Data(contentsOf: result)
                        }
                        didDrop(id, receipt)
                        try fm.removeItem(at: ticket)
                        outcome = "dropped"
                        removed = true
                    } catch {
                        outcome = "storage_error"
                    }
                }
            }
            let reply = folder.appendingPathComponent(token + ".reply.json")
            if let data = try? JSONSerialization.data(withJSONObject: ["id": id, "token": token, "outcome": outcome]) {
                try? data.write(to: reply, options: .atomic)
            }
            try? fm.removeItem(at: request)
        }
        return removed
    }
}
