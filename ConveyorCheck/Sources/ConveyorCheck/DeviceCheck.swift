import Foundation

/// A hand check that a host puts on a shared device: what the tester is asked
/// to look at, as the host passes it with `--ios-run-checklist`.
///
/// The host is the tool on the Mac that installs the build and waits for the
/// tester. It owns both formats in this file, the argument and the report,
/// and serves any app that reads them. It installs one check at a time and
/// holds the device for it until the report says the tester is done.
public struct DeviceCheck: Codable, Equatable, Sendable {
    public struct Item: Codable, Equatable, Identifiable, Sendable {
        /// The item's number in the chat that asked for the check ("3", "4a").
        /// The tester and the chat refer to items by it.
        public let label: String
        public let text: String
        /// What the app does when the tester comes to the item, by the ids of
        /// the app's own action vocabulary, in order.
        public let setup: [String]
        /// Actions of the same vocabulary offered as buttons with the item.
        public let buttons: [String]
        /// A deep link offered with the item.
        public let link: String?

        public var id: String { label }

        public init(label: String, text: String, setup: [String] = [], buttons: [String] = [], link: String? = nil) {
            self.label = label
            self.text = text
            self.setup = setup
            self.buttons = buttons
            self.link = link
        }

        /// The host leaves out what an item does not have.
        public init(from decoder: any Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            label = try container.decode(String.self, forKey: .label)
            text = try container.decode(String.self, forKey: .text)
            setup = try container.decodeIfPresent([String].self, forKey: .setup) ?? []
            buttons = try container.decodeIfPresent([String].self, forKey: .buttons) ?? []
            link = try container.decodeIfPresent(String.self, forKey: .link)
        }
    }

    /// The host's ticket for this check; the report names it back.
    public let id: String
    /// The chat the build came from ("r/38").
    public let tag: String
    /// The project the check is about, by the name the tester knows it under; nil from a host that does not say.
    public let project: String?
    public let title: String
    public let items: [Item]

    public init(id: String, tag: String, project: String? = nil, title: String, items: [Item]) {
        self.id = id
        self.tag = tag
        self.project = project
        self.title = title
        self.items = items
    }

    /// Reads the argument's value: the JSON as unpadded base64url, so that no
    /// shell on the way from the host to the app can touch the text.
    public init?(launchArgument: String) {
        var base64 = launchArgument.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        base64 += String(repeating: "=", count: (4 - base64.count % 4) % 4)
        guard let data = Data(base64Encoded: base64), let check = try? JSONDecoder().decode(Self.self, from: data),
              !check.id.isEmpty, !check.items.isEmpty else { return nil }
        self = check
    }
}

/// What the tester marked, as the host reads it off the device while the
/// check is on it. The tester ends the check with «Готово», once every item
/// has a mark, or with «Доработать» before that; either sets `finishedAt`,
/// which frees the device for the next check and sends the marks to the chat
/// that asked.
///
/// The host may show the same checklist on a screen of its own, where the
/// tester marks and comments too. So every mark says when it was made, and
/// the report says which item the tester is at: of two marks of one item the
/// later one stays, wherever it was made.
public struct DeviceCheckReport: Codable, Equatable, Sendable {
    public struct Result: Codable, Equatable, Sendable {
        public let label: String
        public var state: CheckMark?
        public var note: String?
        /// When the mark or the comment last changed, in seconds since 1970;
        /// nil while the item is untouched.
        public var at: Double?

        public init(label: String, state: CheckMark? = nil, note: String? = nil, at: Double? = nil) {
            self.label = label
            self.state = state
            self.note = note
            self.at = at
        }
    }

    /// What the app can do for an item, as the panel offers it on a button:
    /// a host that shows the checklist too may offer the same button there.
    public struct Offer: Codable, Equatable, Sendable {
        public struct Action: Codable, Equatable, Sendable {
            /// What kind of button it is, when not one of plain words.
            public enum Kind: String, Codable, Sendable {
                /// Opens a screen: the title is its address.
                case link
                /// A switch: `on` says how it stands, and `id` is what flips it.
                case toggle
                /// One of several: `options` are they and `current` the one
                /// that holds; `id` is the one after it.
                case choice
                /// Something done once.
                case action
            }

            /// One of the things a choice chooses between.
            public struct Option: Codable, Equatable, Sendable {
                /// What the host names to choose it.
                public let id: String
                public let title: String

                public init(id: String, title: String) {
                    self.id = id
                    self.title = title
                }
            }

            /// What the host names when it asks for the action.
            public let id: String
            /// What the button is called in words: an address for a link. A
            /// host says it where the panel shows only a symbol.
            public let title: String
            public let kind: Kind?
            /// The SF Symbol the panel shows on the button.
            public let symbol: String?
            public let on: Bool?
            public let options: [Option]?
            public let current: String?

            public init(id: String, title: String, kind: Kind? = nil, symbol: String? = nil, on: Bool? = nil,
                        options: [Option]? = nil, current: String? = nil) {
                self.id = id
                self.title = title
                self.kind = kind
                self.symbol = symbol
                self.on = on
                self.options = options
                self.current = current
            }
        }

        public let label: String
        public let actions: [Action]

        public init(label: String, actions: [Action]) {
            self.label = label
            self.actions = actions
        }
    }

    /// The last action the host asked for, done: the moment of the asking,
    /// as the host stamped it, and why the app could not, when it could not.
    public struct Ran: Codable, Equatable, Sendable {
        public let at: Double
        public let failure: String?
    }

    /// The ticket of the check these marks belong to.
    public let id: String
    public private(set) var results: [Result]
    /// The buttons of the items that have any; nil while no panel has said.
    public private(set) var offers: [Offer]?
    public private(set) var ran: Ran?
    /// The item the tester is at, and when they came to it, in seconds since
    /// 1970: a host that shows the checklist too goes to the same item, and
    /// may lead the way there itself.
    public private(set) var current: String?
    public private(set) var currentAt: Double?
    public private(set) var updatedAt: Date
    public private(set) var finishedAt: Date?

    /// Where the host looks for the file, under the app's Application Support.
    public static let relativePath = "ios-run/check.json"

    public init(check: DeviceCheck, now: Date) {
        id = check.id
        results = check.items.map { Result(label: $0.label) }
        current = check.items.first?.label
        updatedAt = now
    }

    public var isFinished: Bool { finishedAt != nil }
    public var marked: Int { results.count { $0.state != nil } }
    /// Every item has a mark: the check can be called done, not sent back.
    public var isComplete: Bool { marked == results.count }

    public func result(of label: String) -> Result? {
        results.first { $0.label == label }
    }

    public mutating func mark(_ label: String, _ state: CheckMark?, now: Date) {
        change(label, now: now) { $0.state = state }
    }

    public mutating func note(_ label: String, _ text: String, now: Date) {
        change(label, now: now) { $0.note = text.isEmpty ? nil : text }
    }

    /// The tester came to an item.
    public mutating func select(_ label: String, now: Date) {
        guard !isFinished, label != current, result(of: label) != nil else { return }
        current = label
        currentAt = stamp(now)
    }

    /// When something the tester did here happened: now, and in any case
    /// later than everything this report has seen, from here or from the
    /// host. The device's clock and the host's are two clocks. Compared as
    /// they are, a second between them made the later of two moves the
    /// earlier one, and a move made here was taken back by the host's older
    /// one. What is done after another thing was seen is later than it,
    /// whatever either clock says.
    private func stamp(_ now: Date) -> Double {
        let seen = max(results.compactMap(\.at).max() ?? 0, currentAt ?? 0)
        return max(now.timeIntervalSince1970, seen + 0.001)
    }

    /// Takes in the marks and comments made at the host. One made there
    /// later than the device's own replaces it; an earlier one is dropped.
    public mutating func take(_ host: DeviceCheckHost, now: Date) {
        guard !isFinished, host.id == id else { return }
        for theirs in host.results {
            guard let index = results.firstIndex(where: { $0.label == theirs.label }),
                  theirs.at > results[index].at ?? 0 else { continue }
            let next = Result(label: theirs.label, state: theirs.state, note: theirs.note.flatMap { $0.isEmpty ? nil : $0 }, at: theirs.at)
            guard next != results[index] else { continue }
            results[index] = next
            updatedAt = now
        }
        // The item the tester has come to at the host, when that was later than their last move here.
        if let theirs = host.current, let at = host.currentAt, at > currentAt ?? 0, result(of: theirs) != nil {
            current = theirs
            currentAt = at
        }
    }

    public mutating func finish(now: Date) {
        guard !isFinished else { return }
        finishedAt = now
        updatedAt = now
    }

    /// The panel says what it offers to do for the items.
    public mutating func offer(_ offers: [Offer]) {
        guard !isFinished else { return }
        let offered = offers.filter { !$0.actions.isEmpty }
        self.offers = offered.isEmpty ? nil : offered
    }

    /// The action the host asked for at `at` is done, or could not be.
    public mutating func ran(at: Double, failure: String?) {
        guard !isFinished else { return }
        ran = Ran(at: at, failure: failure)
    }

    /// The file's bytes: dates in ISO 8601 and keys in a fixed order, so the
    /// same marks always make the same file.
    public func fileData() throws -> Data {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return try encoder.encode(self)
    }

    /// A finished check keeps the marks it was finished with.
    private mutating func change(_ label: String, now: Date, _ edit: (inout Result) -> Void) {
        guard !isFinished, let index = results.firstIndex(where: { $0.label == label }) else { return }
        var next = results[index]
        edit(&next)
        guard next != results[index] else { return }
        next.at = stamp(now)
        results[index] = next
        updatedAt = now
    }
}

/// What the app says on its output when the report changes: this mark, a
/// space and the JSON the report file holds, as one line. A host that launched
/// the app attached to its output has a mark the moment it is made; the file
/// says the same a little later, and is all there is when the app was opened
/// by hand. An app's input is not connected by such a launch, so the host's
/// own word comes as its file.
public enum DeviceCheckLine {
    public static let report = "\u{1E}ios-run-check "
}

/// What the host says while the check is on the device, in a file it puts
/// beside the report: the marks and comments the tester made on the host's
/// own screen, a button of an item pressed there, and that the check was
/// ended there.
public struct DeviceCheckHost: Codable, Equatable, Sendable {
    /// One of the buttons the report offers for an item, pressed on the
    /// host's screen.
    public struct Run: Codable, Equatable, Sendable {
        public let label: String
        public let action: String
        /// When it was pressed, in seconds since 1970: one press is one moment.
        public let at: Double

        public init(label: String, action: String, at: Double) {
            self.label = label
            self.action = action
            self.at = at
        }
    }

    public struct Result: Codable, Equatable, Sendable {
        public let label: String
        public let state: CheckMark?
        public let note: String?
        /// When it was made, in seconds since 1970.
        public let at: Double

        public init(label: String, state: CheckMark?, note: String?, at: Double) {
            self.label = label
            self.state = state
            self.note = note
            self.at = at
        }
    }

    /// The ticket of the check; a file left by another check is not this one's.
    public let id: String
    public let results: [Result]
    /// The item the tester is at on the host's screen, and when they came to it.
    public let current: String?
    public let currentAt: Double?
    /// The latest button pressed at the host.
    public let run: Run?
    /// The tester ended the check at the host.
    public let finished: Bool

    /// Where the host puts the file, under the app's Application Support.
    public static let relativePath = "ios-run/host.json"

    public init(id: String, results: [Result], current: String? = nil, currentAt: Double? = nil, run: Run? = nil,
                finished: Bool = false) {
        self.id = id
        self.results = results
        self.current = current
        self.currentAt = currentAt
        self.run = run
        self.finished = finished
    }

    /// The host leaves out what it has nothing to say about.
    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        results = try container.decodeIfPresent([Result].self, forKey: .results) ?? []
        current = try container.decodeIfPresent(String.self, forKey: .current)
        currentAt = try container.decodeIfPresent(Double.self, forKey: .currentAt)
        run = try container.decodeIfPresent(Run.self, forKey: .run)
        finished = try container.decodeIfPresent(Bool.self, forKey: .finished) ?? false
    }

    public init?(fileData: Data) {
        guard let host = try? JSONDecoder().decode(Self.self, from: fileData) else { return nil }
        self = host
    }
}
