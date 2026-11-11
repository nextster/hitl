// Current labels are presentation only; Ticket and Verdict keep frozen provenance.
import AppKit

struct CheckName: Decodable {
    let chat: String
    let reference: String
    let status: String
}

final class CheckNames {
    private var entries: [String: (Data, CheckName)] = [:]
    private var loading = false
    private var callbacks: [() -> Void] = []
    private var lastRead = Date.distantPast

    private func basis(_ ticket: Ticket) -> Data? {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
        return try? encoder.encode(ticket)
    }

    func label(_ ticket: Ticket) -> CheckName? {
        guard let (original, label) = entries[ticket.id], original == basis(ticket) else { return nil }
        return label
    }

    func refresh(_ tickets: [Ticket], force: Bool = false, then: @escaping () -> Void) {
        guard !loading else { callbacks.append(then); return }
        guard force || Date().timeIntervalSince(lastRead) >= 3 else { return }
        loading = true
        let captured = tickets.compactMap { ticket -> (String, Data)? in
            basis(ticket).map { (ticket.id, $0) }
        }
        let environment = ProcessInfo.processInfo.environment
        DispatchQueue.global(qos: .utility).async {
            let process = Process(), input = Pipe(), output = Pipe()
            process.executableURL = URL(fileURLWithPath: environment["CONVEYOR_COMMAND"] ?? NSHomeDirectory() + "/Code/scripts/hitl")
            process.arguments = ["check", "--presentation"]
            process.environment = environment
            process.standardInput = input; process.standardOutput = output; process.standardError = FileHandle.nullDevice
            var labels: [CheckName] = []
            do {
                _ = fcntl(input.fileHandleForWriting.fileDescriptor, F_SETNOSIGPIPE, 1)
                try process.run()
                // Timeout bounds a broken backend without blocking the window's main thread.
                DispatchQueue.global().asyncAfter(deadline: .now() + 8) {
                    if process.isRunning { process.terminate() }
                }
                let data = Data("[".utf8) + captured.map { $0.1 }.enumerated().reduce(into: Data()) { result, entry in
                    if entry.offset > 0 { result.append(Data(",".utf8)) }; result.append(entry.element)
                } + Data("]".utf8)
                try input.fileHandleForWriting.write(contentsOf: data)
                try input.fileHandleForWriting.close()
                let answer = output.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit()
                if process.terminationStatus == 0 { labels = (try? JSONDecoder().decode([CheckName].self, from: answer)) ?? [] }
            } catch { if process.isRunning { process.terminate() } }
            let completed = labels
            DispatchQueue.main.async {
                self.entries.removeAll()
                if completed.count == captured.count {
                    for (entry, label) in zip(captured, completed) { self.entries[entry.0] = (entry.1, label) }
                }
                self.loading = false; self.lastRead = Date()
                then()
                let waiting = self.callbacks; self.callbacks.removeAll()
                waiting.forEach { $0() }
            }
        }
    }
}

extension Controller {
    func refreshNames(force: Bool = false, then: (() -> Void)? = nil) {
        let previousName = who.stringValue
        let previousLinks = opens
        chatNames.refresh(tickets, force: force) { [weak self] in
            guard let self else { return }
            if let ticket = self.active {
                self.refreshNameHeader(ticket)
                if previousName != self.who.stringValue || previousLinks != self.opens {
                    self.showLinks()
                    self.arrange()
                    self.layout()
                }
            }
            then?()
        }
    }

    /// What the "Чат" button opens: a reference the label gives that is an address as any other, else the address the
    /// check carries, which the setting of the button gave it when it was queued. The reference `conveyor://chat/APP/ID`
    /// named a chat to hitl's own app, which is gone (2026-11-03), so it opens nothing. The window names no
    /// program, as `names::open_link` in `rust/hitl` does not.
    func chatReference(_ ticket: Ticket) -> String? {
        let usable = { (link: String?) -> String? in
            guard let link = link?.trimmingCharacters(in: .whitespacesAndNewlines), !link.isEmpty, !link.lowercased().hasPrefix("conveyor:") else { return nil }
            return link
        }
        return usable(chatNames.label(ticket)?.reference) ?? usable(ticket.chatLink)
    }

    func refreshNameHeader(_ ticket: Ticket) {
        let label = chatNames.label(ticket)
        let folder = [ticket.cwd, ticket.project].compactMap { $0 }.first { !$0.isEmpty } ?? ""
        who.stringValue = [label?.chat ?? ticket.chat ?? "", ticket.tag, (folder as NSString).lastPathComponent,
                           "чат без имени"].first { !$0.isEmpty } ?? ""
        if let label, !label.reference.isEmpty { who.toolTip = label.reference }
        else { who.toolTip = label.map { "Сохранённая подпись проверки; текущий чат не подтверждён: " + $0.status } }
    }
}
