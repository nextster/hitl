import Foundation

/// The window's queue as the Mac says it: the checks in the window's order,
/// which of them is on the window, the item he is at and what is answered.

struct QueueWord: Decodable, Equatable {
    var mac: String?
    /// The window on the Mac is up: an answer pressed here has where to go.
    var window: Bool?
    var active: String?
    var index: Int?
    var marks: [Mark]?
    var canPostpone: Bool?
    var canFinish: Bool?
    var checks: [Check]?
    var recent: [Recent]?

    var activeCheck: Check? { checks?.first { $0.id == active } }
}

struct Check: Decodable, Equatable, Identifiable {
    var id: String
    /// A build's checklist that is on the phone itself, in the app under check.
    var onPhone: Bool?
    var tag: String?
    var chat: String?
    /// Which of his projects the check is about, by the name he knows it under: shown first, as on the Mac.
    var projectName: String?
    var agent: String?
    /// The icon of the app the check came from, a file of the Mac: asked for as an item's picture is.
    var appIcon: String?
    var folder: String?
    var branch: String?
    var title: String?
    var about: String?
    var kind: String?
    var kindTitle: String?
    var answers: [String]?
    var createdAt: Double?
    var marked: Int?
    var items: [Item]

    /// A chat is called by its name; one without a name, by the folder it works in. The app is said by its mark,
    /// before the project.
    var who: String {
        [chat ?? "", tag ?? "", folder ?? "", "чат без имени"].first { !$0.isEmpty } ?? ""
    }

    var from: String {
        var parts = [agent ?? "", folder ?? ""].filter { !$0.isEmpty }
        if let branch, !branch.isEmpty { parts.append(branch) }
        return parts.joined(separator: " · ")
    }
}

struct Item: Decodable, Equatable {
    var label: String
    var text: String
    var details: String?
    var facts: [String]?
    /// Lines he copies with a tap, each whole: a command for the terminal, an address, a value to paste.
    var copies: [String]?
    var pictures: [String]?
    var page: String?
    var options: [String]?
    var many: Bool?
    var opens: [Open]?
    /// The item's own kind, where it is not the check's, with the word for it and what its answers are called.
    var kind: String?
    var kindTitle: String?
    var answers: [String]?
}

/// Something an item offers to open: a page, another app by its own address, a file on the Mac.
struct Open: Decodable, Equatable, Hashable {
    var title: String
    var link: String
    var kind: String
}

struct Mark: Decodable, Equatable {
    var label: String
    var state: String?
    var choice: String?
    var choices: [String]?
    var other: Bool?
    /// «Не понял»: the item is to be said again. The window keeps it beside a "no".
    var unclear: Bool?
    var note: String?
    var question: Bool?
    var shots: [String]?
    var values: [[String]]?
}

struct Recent: Decodable, Equatable, Hashable {
    var tag: String?
    var title: String?
    var at: Double?
    var summary: String?
}

/// One line from the Mac: its greeting, the queue, or a file that was asked for.
struct MacLine: Decodable {
    struct Welcome: Decodable {
        var mac: String?
        /// The server's room, said at home to a phone that may not know of it yet.
        var relay: RelayWord?
    }

    var welcome: Welcome?
    var queue: QueueWord?
    var file: Int?
    var name: String?
    var data: String?
    var error: String?
}

/// What the plain answers mean, by position: yes, no, not now.
let answerStates = ["ok", "problem", "skipped"]
/// A link a chat wrote into a text of a check, as in Markdown: «[team/320](https://…)» is read as «team/320», and a
/// tap on those words opens the address («хотелось бы чтобы в таких штуках ещё ссылки можно было делать прямо в
/// тексте а ля как в маркдауне»). The window on the Mac reads a text by the same rule, and so does `hitl check`.
enum Linked {
    struct Piece: Equatable {
        var text: String
        var link: String?
    }

    /// The words in square brackets, then the address in round ones: no line break in either, and in the address
    /// no brackets but a pair of its own.
    private static let pattern = try? NSRegularExpression(pattern: #"\[([^\[\]\n]+)\]\(((?:[^()\n]|\([^()\n]*\))+)\)"#)

    /// A file of the Mac, a page, another app's own address: anything else in the brackets' place is text as it is.
    static func isAddress(_ target: String) -> Bool {
        if target.hasPrefix("/") || target.hasPrefix("~/") { return true }
        return target.range(of: #"^[A-Za-z][A-Za-z0-9+.-]*:\S+$"#, options: .regularExpression) != nil
    }

    static func pieces(_ text: String) -> [Piece] {
        guard let pattern, text.contains("](") else { return text.isEmpty ? [] : [Piece(text: text, link: nil)] }
        let whole = text as NSString
        var pieces: [Piece] = []
        var from = 0
        for found in pattern.matches(in: text, range: NSRange(location: 0, length: whole.length)) {
            let words = whole.substring(with: found.range(at: 1)).trimmingCharacters(in: .whitespaces)
            let target = whole.substring(with: found.range(at: 2)).trimmingCharacters(in: .whitespaces)
            guard !words.isEmpty, isAddress(target) else { continue }
            if found.range.location > from {
                pieces.append(Piece(text: whole.substring(with: NSRange(location: from, length: found.range.location - from)), link: nil))
            }
            pieces.append(Piece(text: words, link: target))
            from = found.range.location + found.range.length
        }
        if from < whole.length { pieces.append(Piece(text: whole.substring(from: from), link: nil)) }
        return pieces
    }

    /// The text as he reads it: the links by their words.
    static func plain(_ text: String) -> String { pieces(text).map(\.text).joined() }

    /// What a tap on a link goes by: a file of the Mac by its path, which the Mac gives in full, anything else by
    /// its address.
    static func address(_ link: String) -> URL? {
        link.hasPrefix("/") ? URL(fileURLWithPath: link) : URL(string: link)
    }
}

/// The answer to a question when none of its options fits: his own words.
let ownAnswer = "Другое, напишу сам"
/// «Не понял», the last of every item's rows, and what the window is asked for it, as -1 is his own answer.
let unclearAnswer = "Не понял"
let unclearNumber = -2
