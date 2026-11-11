import Foundation
import Observation

/// The hand check the host installed this build for: its checklist and the
/// tester's marks. The marks go to a file the host reads off the device, and
/// ending the check there is what frees the device for the next build.
///
/// The checklist comes with the one launch the host makes and is kept in the
/// defaults, so it is still on screen when the app is reopened by hand. It
/// belongs to the installation it came with: another install, or a launch by
/// the host without a checklist, drops it.
@MainActor @Observable
public final class DeviceCheckStore {
    public private(set) var check: DeviceCheck?
    public private(set) var report: DeviceCheckReport?
    /// The item the one-at-a-time view is on.
    public private(set) var currentLabel: String?
    /// Whether this launch brought the check, rather than an earlier one.
    @ObservationIgnored public let arrivedWithThisLaunch: Bool
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let key: String
    @ObservationIgnored private let installation: String
    @ObservationIgnored private let directory: URL?
    @ObservationIgnored private let now: () -> Date
    /// The check was ended at the host: whoever shows the checklist takes it away.
    @ObservationIgnored public var finishedByHost: (() -> Void)?
    /// A button of an item was pressed at the host: whoever shows the
    /// checklist runs it as a tap on the button would. Nil when it did,
    /// otherwise why it could not.
    @ObservationIgnored public var runForHost: ((_ label: String, _ action: String) async -> String?)?
    /// The check is about to end, here or at the host: whoever shows the
    /// checklist puts back what its buttons changed in the app, while the
    /// app is still the host's to keep on the device.
    @ObservationIgnored public var beforeFinish: (() async -> Void)?
    /// How things stood before a button of the check first changed them, by
    /// a name of the panel's own: kept with the checklist between launches,
    /// for the panel to put back when the check ends.
    @ObservationIgnored public private(set) var remembered: [String: String] = [:]
    /// The moment of the host's latest press that was taken up, done or not yet.
    @ObservationIgnored private var asked: Double = 0
    @ObservationIgnored private var isEnding = false
    @ObservationIgnored private var listening: Timer?

    private struct Saved: Codable {
        let check: DeviceCheck
        let report: DeviceCheckReport
        let installation: String
        var current: String?
        var remembered: [String: String]?
    }

    /// Dates as seconds, like the rest of what an app keeps in its defaults.
    private static let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .secondsSince1970
        return encoder
    }()

    private static let decoder: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .secondsSince1970
        return decoder
    }()

    /// - Parameters:
    ///   - defaults: Where the checklist and its marks are kept between launches.
    ///   - key: The key they are kept under; the app's to name.
    ///   - launch: What the host passed to this launch, if it made it.
    ///   - installation: What tells this installed copy from the next one.
    ///     iOS gives every install its own bundle folder.
    ///   - directory: Where the report file goes; Application Support.
    public init(defaults: UserDefaults, key: String = "hitl.device-check", launch: DeviceCheckLaunch,
                installation: String = Bundle.main.bundlePath,
                directory: URL? = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first,
                now: @escaping () -> Date = { .now }) {
        self.defaults = defaults
        self.key = key
        self.installation = installation
        self.directory = directory
        self.now = now
        let saved = defaults.data(forKey: key)
            .flatMap { try? Self.decoder.decode(Saved.self, from: $0) }
            .flatMap { $0.installation == installation ? $0 : nil }
        arrivedWithThisLaunch = launch.check != nil
        if let arrived = launch.check {
            check = arrived
            // The host may launch one check twice; its marks stay.
            let kept = saved.flatMap { $0.check.id == arrived.id ? $0 : nil }
            report = kept?.report ?? DeviceCheckReport(check: arrived, now: now())
            currentLabel = kept?.current ?? arrived.items.first?.label
            remembered = kept?.remembered ?? [:]
            currentLabel.map { report?.select($0, now: now()) }
            save()
        } else if let saved, launch.tag == nil, !saved.report.isFinished {
            check = saved.check
            report = saved.report
            currentLabel = saved.current ?? saved.check.items.first?.label
            remembered = saved.remembered ?? [:]
            // The host still waits for the file, whatever happened to it.
            writeFile()
        } else if defaults.object(forKey: key) != nil {
            defaults.removeObject(forKey: key)
        }
    }

    /// Whether there is a checklist to show.
    public var isActive: Bool { check != nil && report?.isFinished == false }

    /// What the panel offers to do for the items: the host may offer the same.
    public func offer(_ offers: [DeviceCheckReport.Offer]) { change { $0.offer(offers) } }

    /// Keeps how something stood before the check changed it. The first
    /// word of it stays: what a later press finds is the check's own doing.
    public func remember(_ name: String, _ value: String) {
        guard isActive, remembered[name] == nil else { return }
        remembered[name] = value
        save()
    }

    public func mark(_ label: String, _ state: CheckMark?) { change { $0.mark(label, state, now: now()) } }
    public func note(_ label: String, _ text: String) { change { $0.note(label, text, now: now()) } }

    /// Moves the one-at-a-time view to an item of the checklist.
    public func select(_ label: String) {
        guard label != currentLabel, report?.result(of: label) != nil else { return }
        currentLabel = label
        report?.select(label, now: now())
        save()
    }

    /// Takes in what the host has said since the last look: the marks and
    /// comments the tester made on the host's screen, and the end of the
    /// check when it was ended there.
    public func readHost() {
        guard isActive, let directory,
              let data = try? Data(contentsOf: directory.appendingPathComponent(DeviceCheckHost.relativePath)),
              let host = DeviceCheckHost(fileData: data) else { return }
        take(host)
    }

    /// What the host said, from its file or from a line on the app's input.
    public func take(_ host: DeviceCheckHost) {
        guard isActive, let check, host.id == check.id else { return }
        change { $0.take(host, now: now()) }
        // The tester moved to another item at the host: the one-at-a-time view goes there too.
        if let current = report?.current, current != currentLabel {
            currentLabel = current
            save()
        }
        if host.finished {
            endForHost()
        } else if let run = host.run {
            press(run)
        }
    }

    /// The check was ended at the host. What its buttons changed is put back
    /// first: once the report says the check is over, the host may replace
    /// the app at any moment.
    private func endForHost() {
        guard let beforeFinish, !remembered.isEmpty else {
            finish()
            finishedByHost?()
            return
        }
        guard !isEnding else { return }
        isEnding = true
        Task { [weak self] in
            await beforeFinish()
            self?.finish()
            self?.finishedByHost?()
        }
    }

    /// A press at the host is taken up once, whatever the file says after it.
    private func press(_ run: DeviceCheckHost.Run) {
        guard run.at > max(asked, report?.ran?.at ?? 0), let runForHost else { return }
        asked = run.at
        Task { [weak self] in
            let failure = await runForHost(run.label, run.action)
            self?.change { $0.ran(at: run.at, failure: failure) }
        }
    }

    /// Looks at what the host says, again and again, for as long as the check
    /// is on. Often: the file is the app's own and small, and a mark made at
    /// the host should be here before the tester has looked up from it.
    public func listen(every interval: TimeInterval = 0.05) {
        listening?.invalidate()
        guard isActive else { return }
        // A press the file already holds was made before this launch: for another launch of the app, not this one.
        if let directory, let data = try? Data(contentsOf: directory.appendingPathComponent(DeviceCheckHost.relativePath)),
           let host = DeviceCheckHost(fileData: data), host.id == check?.id, let run = host.run {
            asked = max(asked, run.at)
        }
        listening = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                guard self.isActive else {
                    self.listening?.invalidate()
                    self.listening = nil
                    return
                }
                self.readHost()
            }
        }
    }

    /// Ends the check, done or sent back: the host sees it in the file, gives
    /// the marks to whoever asked for the check and installs the next build.
    public func finish() { change { $0.finish(now: now()) } }

    private func change(_ edit: (inout DeviceCheckReport) -> Void) {
        guard var next = report else { return }
        edit(&next)
        guard next != report else { return }
        report = next
        save()
    }

    private func save() {
        guard let check, let report else { return }
        let saved = Saved(check: check, report: report, installation: installation, current: currentLabel,
                          remembered: remembered.isEmpty ? nil : remembered)
        defaults.set(try? Self.encoder.encode(saved), forKey: key)
        writeFile()
    }

    /// The report on the app's output, as one line: a host attached to it has the mark the moment it is made.
    private func writeLine() {
        guard let report, let data = try? report.fileData() else { return }
        var line = Data(DeviceCheckLine.report.utf8)
        line.append(data)
        line.append(0x0A)
        ReportLines.shared.say(line)
    }

    private func writeFile() {
        writeLine()
        guard let report, let directory else { return }
        let url = directory.appendingPathComponent(DeviceCheckReport.relativePath)
        // The host reads the file right after the check ends, often with the
        // phone already locked. Without the file it waits to be told by hand.
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? report.fileData().write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }
}

/// Says lines on the app's output from a thread of its own. An output nobody
/// reads any more can hold a write up for good: then it holds up this thread
/// and nothing else. Only the latest line waits, and it says all the earlier
/// ones said.
private final class ReportLines: @unchecked Sendable {
    static let shared = ReportLines()

    private let condition = NSCondition()
    private var waiting: Data?
    private var started = false

    func say(_ line: Data) {
        condition.lock()
        defer { condition.unlock() }
        waiting = line
        if !started {
            started = true
            // An output whose reader is gone must not end the app.
            signal(SIGPIPE, SIG_IGN)
            let thread = Thread { [self] in run() }
            thread.name = "conveyor-check-lines"
            thread.start()
        }
        condition.signal()
    }

    private func run() {
        while true {
            condition.lock()
            while waiting == nil { condition.wait() }
            let line = waiting ?? Data()
            waiting = nil
            condition.unlock()
            line.withUnsafeBytes { bytes in
                var sent = 0
                while sent < bytes.count, let start = bytes.baseAddress {
                    let wrote = write(STDOUT_FILENO, start + sent, bytes.count - sent)
                    if wrote <= 0 { break }
                    sent += wrote
                }
            }
        }
    }
}
