// What the window reads and writes: its folder and files, its sizes, the tickets, answers and verdicts, the log.

import Foundation

let environment = ProcessInfo.processInfo.environment
let home = URL(fileURLWithPath: environment["CONVEYOR_HOME"] ?? (NSHomeDirectory() + "/.conveyor"))
/// What the window keeps for itself, apart from the checks: its lock and its process, what is marked in it, where it stands.
let windowDirectory = home.appendingPathComponent("window")
let queueDirectory = home.appendingPathComponent("queue")
let doneDirectory = home.appendingPathComponent("done")
let historyFile = home.appendingPathComponent("history.jsonl")
let progressFile = windowDirectory.appendingPathComponent("progress.json")
/// Where he moved the window to: the checks to come open there.
let placeFile = windowDirectory.appendingPathComponent("place.json")
/// His answers that are not sent yet, for the window that comes up after this one.
let answersFile = windowDirectory.appendingPathComponent("answers.json")
let shotsDirectory = home.appendingPathComponent("shots")
let scriptFile = environment["CONVEYOR_WINDOW_SCRIPT"]
/// A picture that stands in for the phone's screen in a scripted run: no phone is touched there.
let shotStandIn = environment["CONVEYOR_WINDOW_SHOT"]
/// A screen that stands in for the Mac's own in a scripted run, its size in points as «1352x878»: what the window
/// does on a screen too low for an item is then the same on whatever screen the checks happen to run.
let screenStandIn: NSSize? = {
    let parts = (environment["CONVEYOR_WINDOW_SCREEN"] ?? "").split(separator: "x").compactMap { Double($0) }
    return scriptFile != nil && parts.count == 2 && parts[0] > 0 && parts[1] > 0 ? NSSize(width: parts[0], height: parts[1]) : nil
}()
/// The window's width is his own pick, made on a live page of sliders (2026-10-10): 620. Its margins were 8 then;
/// they are 20 since the redesign of 2026-10-20, «Всё по одной линии слева»: the task, the question, the answers
/// and the comment start at one edge, where they started at two.
let panelWidth: CGFloat = 620
let side: CGFloat = 20
let contentWidth: CGFloat = panelWidth - 2 * side
/// How wide the item's column stands now, in the window's own points: as in the small window, or on the whole
/// screen all the room at the right of the list («сделай шириной по экрану»).
var columnWide: CGFloat = contentWidth
let barHeight: CGFloat = 44
/// The page, in the panel's own points: the list at its left, its margins, and how tall it counts
/// itself. The screen's height over that height is how much larger everything on it is drawn.
let listWidth: CGFloat = 236
let pageSide: CGFloat = 24
let pageHeight: CGFloat = 680

/// What he answered to an item the last time it was asked: a chat that asks an item again names the check it
/// answers, and the item keeps its number. The answer in words, "" when he only wrote, and what he wrote.
struct Before: Codable {
    var at: Double
    var answer: String
    var note: String?
}

struct Item: Codable {
    var label: String
    var text: String
    var details: String?
    /// What he has to know to answer, a line each: an explanation comes as these.
    var facts: [String]?
    var links: [String]?
    /// Lines he copies with one click, each whole: a command for the terminal, an address, a value to paste.
    var copies: [String]?
    var images: [String]?
    /// A live page for the item: an HTML file he turns things on.
    var page: String?
    var options: [String]?
    /// Several of the options may hold at once: he ticks them, and goes on himself.
    var many: Bool?
    /// The item's own kind, where it is not the check's: an explanation, a question and an action may stand in one
    /// check («можно миксовать вопросы и действия и объяснения в рамках одного набора вопросов»). With it come the
    /// word shown at the top and what its answers are called.
    var kind: String?
    var kindTitle: String?
    var answers: [String]?
    /// His last answer to this very item, shown over it.
    var before: Before?
}

/// The card a chat works on: one more thing to open, at every item.
struct Card: Codable {
    var title: String
    var path: String
}

struct Ticket: Codable {
    var id: String
    /// A check that is on the phone is shown here as well: «было бы круто если бы проверка на телефоне
    /// дублировалась плашкой на маке чтобы мне было проще писать комменты». Its queue is the phone's; here
    /// are the phone's name and number, and the folder where the two screens leave word for each other.
    var screen: String?
    var device: String?
    var udid: String?
    var folder: String?
    var isOnPhone: Bool { screen == "iphone" }
    var tag: String
    var chatIds: [String]?
    var chatRef: String?
    var chat: String?
    /// What opens that chat, when it has an address: one more thing to open, at every item («где диплинк в чат»).
    var chatLink: String?
    /// Where the check comes from, besides its chat: the agent, and the folder it works in.
    var agent: String?
    var cwd: String?
    /// Which of his projects the check is about, by the name he knows it under: the repository's own folder.
    var projectName: String?
    var title: String
    var about: String?
    var card: Card?
    var kind: String?
    var kindTitle: String?
    var answers: [String]?
    var project: String?
    var branch: String?
    var commit: String?
    var items: [Item]
    var createdAt: Double
}

struct Mark: Codable {
    var label: String
    var state: String?
    var choice: String?
    /// The options he ticked where several may hold at once, in the item's order; and whether his own answer,
    /// written as the comment, is ticked beside them.
    var choices: [String]?
    var other: Bool?
    /// «Не понял»: the item is to be said again. Kept beside a "no", so that whatever knows nothing of this
    /// word reads a no, as it did when he typed «не понял» under one.
    var unclear: Bool?
    var note: String
    var question: Bool?
    var shots: [String]?
    /// What was set on the item's page when he answered: names and values, in the page's order.
    var values: [[String]]?
    /// When the answer or the comment last changed, here or on the phone: of two the later one stays.
    var at: Double?
    /// The answer, the comment or the page was last changed here on the Mac, by his hand: a press the phone kept
    /// while it had no way here does not take its place («если я на маке ответил иначе раньше то принимать тот
    /// ответ», 2026-10-16). Kept with his answers on the disk; not a part of the verdict.
    var mac: Bool?
    /// What the item asked, written into the verdict: his answers are read later beside what they answered.
    var text: String?
    /// The item's own kind, where it is not the check's: the verdict names an answer by it.
    var kind: String?
    /// What the item offered besides its text, written into the verdict beside `text`: the history is read later to
    /// see what made an item good or bad («докинь в историю всё что сейчас мешает разбираться что было плохо и
    /// хорошо в hitl», 2026-11-03).
    var details: String?
    var options: [String]?
    var many: Bool?
    var facts: [String]?
    /// How many links, lines to copy and pictures the item had, and whether it had a live page.
    var links: Int?
    var copies: Int?
    var images: Int?
    var page: Bool?
}

extension Mark {
    /// The item holds something of his that lives in this window alone: an answer, a comment, a screenshot.
    /// Not what stands on its live page: a page says that by itself as it comes up.
    var isHis: Bool {
        state != nil || !note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !(shots ?? []).isEmpty
    }
}

/// What the phone says of the check that is on it, as the queue's process writes it down for this window.
struct PhoneSide: Codable {
    struct Answer: Codable {
        var label: String
        var state: String?
        var note: String?
        var at: Double?
    }

    /// A button the panel on the phone offers with an item: this window offers the same, under the same name.
    struct Action: Codable, Equatable {
        /// One of the things a choice chooses between: what to ask the phone for, and its name.
        struct Option: Codable, Equatable {
            var id: String
            var title: String
        }

        /// What to ask the phone for on a press: for a switch what flips it, for a choice the option after the current one.
        var id: String
        /// Its name in words; an address for a link.
        var title: String
        /// "link", "toggle", "choice" or "action"; nil for a button of plain words.
        var kind: String?
        /// The SF Symbol the phone shows on it.
        var symbol: String?
        /// How a switch stands.
        var on: Bool?
        var options: [Option]?
        /// The option that holds, by its id.
        var current: String?
    }

    struct Offer: Codable {
        var label: String
        var actions: [Action]
    }

    /// The last button pressed here that the phone has run: the moment of the press, and why it could not.
    struct Ran: Codable, Equatable {
        var at: Double
        var failure: String?
    }

    var id: String
    var results: [Answer]
    /// The item he is at on the phone, and when he came to it.
    var current: String?
    var currentAt: Double?
    var offers: [Offer]?
    var ran: Ran?
}

/// What was done here for a check that is on the phone, for the queue's process to pass on to it.
struct MacSide: Codable {
    /// A button of an item pressed here, for the phone to run: one press is one moment.
    struct Run: Codable {
        var label: String
        var action: String
        var at: Double
    }

    var id: String
    var results: [Mark]
    /// The item he is at here, and when he came to it: the panel on the phone goes there too.
    var current: String?
    var currentAt: Double?
    var run: Run?
    /// He ended the check here: "done" or "rework".
    var finish: String?
}

struct Verdict: Codable {
    var id: String
    /// Which screen the check was on: the history is one for the window and the phone.
    var screen: String? = "mac"
    var tag: String
    var chat: String?
    /// The agent the check came from, "Claude", "Codex" or "OpenCode": the history says whose items he did not understand.
    var agent: String?
    var projectName: String?
    var title: String
    var kind: String?
    var project: String?
    var branch: String?
    var commit: String?
    var results: [Mark]
    var how: String
    var createdAt: Double
    var shownAt: Double
    var finishedAt: Double
    /// "phone" when he ended it in the app on the phone; the window on the Mac otherwise.
    var endedOn: String?
    /// The sentence the check said about its task: the history is read without the card.
    var about: String?
}

/// What the window says of itself for the commands that wait: the check on it, and how much of his it holds.
struct Progress: Codable {
    var id: String
    /// The items that hold something of his, in the check on the window and in every check he stepped away
    /// from. A command puts a window of another version in this one's place only while that is 0. It counted
    /// the check on the window alone: a check of the phone came forward with nothing answered in it, and the
    /// window went with his answers to another one («сбросила нахуй всё что я наотвечал», 2026-10-11).
    /// The name is the one the commands that were waiting that day read.
    var marked: Int
    /// The items he has answered in the check on the window, of `total`.
    var answered: Int
    var total: Int
    var shownAt: Double
    var waiting: Int
}

/// The top right corner of the small window, where he put it.
struct Place: Codable {
    var corner: [Double]
}

/// What `hitl check` reads to know that a window is up and of which version.
struct Running: Codable {
    var pid: Int32
    var binary: String
}

/// What the plain answers mean, by position: yes, no, not now. What they are
/// called comes with the ticket: «работает», «да», «сделал».
let states = ["ok", "problem", "skipped"]
let usualAnswers = ["работает", "не работает", "пропустить"]
/// Every answer is on a digit, by its place, like the options of a question: «не пробел esc tab а цифры тоже
/// сделаем». Nothing else answers.
let answerKeys = ["1", "2", "3"]
/// The answer to a question when none of its options fits: his own words.
let ownAnswer = "Другое, напишу сам"
/// «Не понял», in every item and always on the same key, the last of the rows: «ноль но в списке оно будет не
/// первой а последней». In four days he typed it 32 times under a "no" or under nothing, and four times under a
/// "yes". An explanation has the answer already, «не понятно», and the key presses that one there.
let unclearAnswer = "Не понял"
let unclearKey = "0"
/// What `mark` is called with for it, as -1 is his own answer to a question.
let unclearNumber = -2
/// Screenshots one comment can carry: as many as fit in a row under it.
let mostShots = 4

/// A line for ~/.conveyor/window/panel.log: what cannot be seen from outside the window.
func log(_ text: String) {
    FileHandle.standardError.write((ISO8601DateFormatter().string(from: Date()) + " " + text + "\n").data(using: .utf8)!)
}
