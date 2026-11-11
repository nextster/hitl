// Key decisions only. One child for this window, bounded pending writes, no file I/O on the UI thread.
import Foundation

final class ActionJournal: @unchecked Sendable {
    static let shared = ActionJournal()
    private let writing = DispatchQueue(label: "hitl.journal", qos: .utility)
    private let slots = DispatchSemaphore(value: 64)
    private let lock = NSLock()
    private var lost = 0
    private var process: Process?
    private var input: FileHandle?
    private let session = UUID().uuidString

    func record(_ kind: String, request: String, item: Int = -1, outcome: String = "", phone: Bool = false,
                reason: String = "", related: String = "", windowNumber: Int = -1, marked: Int = -1, waiting: Int = -1) {
        guard ProcessInfo.processInfo.environment["CONVEYOR_JOURNAL"] != "0" else { return }
        guard slots.wait(timeout: .now()) == .success else {
            lock.lock(); lost += 1; lock.unlock()
            return
        }
        let at = Date().timeIntervalSince1970
        writing.async { [self] in
            defer { slots.signal() }
            do {
                if process?.isRunning != true { try start() }
                lock.lock(); let dropped = lost; lock.unlock()
                if dropped > 0 {
                    try send(["kind": "journal.loss", "source": "hitl.mac", "outcome": "lost", "dropped": dropped])
                    lock.lock(); lost -= dropped; lock.unlock()
                }
                try send(["kind": kind, "request": request, "item": item, "outcome": outcome,
                          "source": phone ? "hitl.phone" : "hitl.mac", "reason": reason, "subject": related,
                          "producer_pid": Int(getpid()), "window": session, "native_window": windowNumber,
                          "marked": marked, "waiting": waiting,
                          "at": at,
                          "binary": URL(fileURLWithPath: CommandLine.arguments[0]).lastPathComponent])
            } catch {
                lock.lock(); lost += 1; lock.unlock()
                input?.closeFile(); input = nil
                process = nil
            }
        }
    }

    /// A command launched by the window, not a native app's unseen descendants.
    func launch(_ child: Process, purpose: String, request: String = "") throws {
        if ProcessInfo.processInfo.environment["CONVEYOR_JOURNAL"] == "0" { try child.run(); return }
        let action = UUID().uuidString
        let executable = child.executableURL?.path ?? ""
        let previous = child.terminationHandler
        child.terminationHandler = { [self] child in
            processEvent(action: action, executable: executable, purpose: purpose, request: request,
                         outcome: "exited", pid: child.processIdentifier, code: child.terminationStatus)
            previous?(child)
        }
        processEvent(action: action, executable: executable, purpose: purpose, request: request, outcome: "requested")
        var environment = child.environment ?? ProcessInfo.processInfo.environment
        environment["CONVEYOR_TRACE_ID"] = action
        environment["CONVEYOR_PARENT_ACTION"] = action
        child.environment = environment
        do {
            try child.run()
            processEvent(action: action, executable: executable, purpose: purpose, request: request,
                         outcome: "spawned", pid: child.processIdentifier)
        } catch {
            processEvent(action: action, executable: executable, purpose: purpose, request: request,
                         outcome: "error", code: Int32((error as NSError).code))
            throw error
        }
    }

    private func processEvent(action: String, executable: String, purpose: String, request: String,
                              outcome: String, pid: pid_t = 0, code: Int32? = nil) {
        guard ProcessInfo.processInfo.environment["CONVEYOR_JOURNAL"] != "0" else { return }
        guard slots.wait(timeout: .now()) == .success else {
            lock.lock(); lost += 1; lock.unlock(); return
        }
        var value: [String: Any] = ["kind": outcome == "exited" ? "native.process.result" : "native.process.launch",
            "source": "hitl.process", "outcome": outcome, "action": action, "trace": action,
            "executable": executable, "purpose": purpose, "request": request,
            "launcher_pid": Int(getpid()), "launcher_ppid": Int(getppid()), "at": Date().timeIntervalSince1970]
        if pid > 0 { value["child_pid"] = Int(pid) }
        if let code { value[outcome == "exited" ? "exit_code" : "native_code"] = Int(code) }
        let event = value
        writing.async { [self] in
            defer { slots.signal() }
            do {
                if process?.isRunning != true { try start() }
                try send(event)
            } catch {
                lock.lock(); lost += 1; lock.unlock()
                input?.closeFile(); input = nil; process = nil
            }
        }
    }

    private func send(_ value: [String: Any]) throws {
        let data = try JSONSerialization.data(withJSONObject: value)
        try input?.write(contentsOf: data + Data([10]))
    }

    private func start() throws {
        let environment = ProcessInfo.processInfo.environment
        let path = environment["CONVEYOR_COMMAND"] ?? NSHomeDirectory() + "/Code/scripts/hitl"
        let child = Process(), feed = Pipe()
        child.executableURL = URL(fileURLWithPath: path)
        child.arguments = ["journal", "_stream"]
        child.environment = environment
        child.standardInput = feed
        child.standardOutput = FileHandle.nullDevice
        child.standardError = FileHandle.nullDevice
        // A dead journal reader is an EPIPE, never a signal that ends the window.
        _ = fcntl(feed.fileHandleForWriting.fileDescriptor, F_SETNOSIGPIPE, 1)
        try child.run()
        process = child
        input = feed.fileHandleForWriting
        try send(["kind": "native.process.launch", "source": "hitl.process", "outcome": "spawned",
                  "purpose": "journal_writer", "executable": path, "child_pid": Int(child.processIdentifier),
                  "launcher_pid": Int(getpid()), "launcher_ppid": Int(getppid()), "at": Date().timeIntervalSince1970])
    }

    // Used by the private fixture. The actual window never waits for diagnostic writes.
    func drain() { writing.sync {} }

    /// Only on process exit: give pending facts up to 100 ms, then close stdin.
    /// Interactive paths never wait. SIGKILL/crash cannot promise an exit record.
    func close() {
        let closed = DispatchSemaphore(value: 0)
        writing.async { [self] in input?.closeFile(); input = nil; closed.signal() }
        _ = closed.wait(timeout: .now() + 0.1)
    }
}
