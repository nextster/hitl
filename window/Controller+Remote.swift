// The same queue in the app on the phone: what this window says of itself for it, and what he presses there.
//
// «чтобы на мобильное приложение hitl приходила всё та же очередь вопросов и можно было тыкать с телефона
// всё то же самое». The app is a second screen of this window, not a second window: it shows the check and the
// item that are here, and a button pressed there is pressed here, by the same code as a digit. So an answer
// given on the phone is the answer the window would have kept, and the verdict is written here as ever.
//
// The window talks to no network. It writes what it shows into a file, and takes what was pressed from a
// folder; `hitl phone` (src/hitl/phone.py) carries both to the phone and back.

import AppKit

let remoteDirectory = windowDirectory.appendingPathComponent("remote")
/// What the window shows now, for the phone.
let remoteStateFile = remoteDirectory.appendingPathComponent("state.json")
/// What he pressed on the phone, a file a press, taken in the order of their names.
let remoteInbox = remoteDirectory.appendingPathComponent("in")
/// The icons of the apps the checks came from, a file an app: the phone shows them before the project, as here.
let remoteIcons = remoteDirectory.appendingPathComponent("icons")

/// What the app on the phone has to know of this window to show the same.
struct RemoteState: Codable {
    /// A check he stepped away from, and how many of its items he had answered.
    struct Kept: Codable {
        var id: String
        var marked: Int
    }

    /// Grows by one with every change: the phone tells a new word from the one it has.
    var seq: Int
    var pid: Int32
    /// The check on the window, the item he is at and what he has answered in it.
    var active: String?
    var index: Int
    var marks: [Mark]
    /// The checks as this window lists them.
    var order: [String]
    var kept: [Kept]
    var canPostpone: Bool
    /// Every item has its answer: the check ends with «Готово», before that with «Доработать».
    var canFinish: Bool
    /// A comment is being written here, on the Mac.
    var editing: Bool
}

/// One press on the phone. It names its check and its item itself: the phone may be a moment behind.
struct RemoteWord: Codable {
    /// "take" a check, "go" to an item, "answer" it, its "note", the "values" of its page, a "shot" for its
    /// comment or "unshot", "postpone" the check, "finish" it.
    var `do`: String
    var ticket: String
    var label: String?
    /// The answer by its place among the item's rows, as a digit says it; -1 is his own answer to a question.
    var number: Int?
    var text: String?
    var values: [[String]]?
    var path: String?
    /// "done" or "rework".
    var how: String?
    /// Pressed while the phone had no way to the Mac and kept there until it had.
    var late: Bool?
}

extension Controller {
    /// Says what is on the window now, when that has changed since it was last said.
    func tellRemote() {
        for ticket in tickets { AppMark.keep(ticket.agent ?? "", in: remoteIcons) }
        var state = RemoteState(seq: 0, pid: getpid(), active: active?.id, index: index, marks: active == nil ? [] : marks,
                                order: tickets.map(\.id),
                                // The check on the window has its answers above; one he only looked into has none to count.
                                kept: kept.filter { $0.key != active?.id }
                                    .map { RemoteState.Kept(id: $0.key, marked: $0.value.marks.filter { $0.state != nil }.count) }
                                    .filter { $0.marked > 0 }.sorted { $0.id < $1.id },
                                canPostpone: tickets.count > 1, canFinish: active != nil && marks.allSatisfy { $0.state != nil },
                                editing: active != nil && isEditingNote)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        guard let plain = try? encoder.encode(state), plain != remoteSaid else { return }
        remoteSaid = plain
        remoteSeq += 1
        state.seq = remoteSeq
        guard let data = try? encoder.encode(state) else { return }
        try? FileManager.default.createDirectory(at: remoteDirectory, withIntermediateDirectories: true)
        try? data.write(to: remoteStateFile, options: .atomic)
    }

    /// Takes in what was pressed on the phone since the last look; `most` of the presses, in a scripted run
    /// that looks at the window between them.
    func hearRemote(most: Int? = nil) {
        let files = ((try? FileManager.default.contentsOfDirectory(at: remoteInbox, includingPropertiesForKeys: nil)) ?? [])
            .filter { $0.pathExtension == "json" }.sorted { $0.lastPathComponent < $1.lastPathComponent }
        for file in files.prefix(most ?? files.count) {
            let data = try? Data(contentsOf: file)
            try? FileManager.default.removeItem(at: file)
            if let data, let said = try? JSONDecoder().decode(RemoteWord.self, from: data) { obey(said) }
        }
    }

    /// Nothing is left of the last window's talk with the phone.
    func forgetRemote() {
        try? FileManager.default.removeItem(at: remoteInbox)
        try? FileManager.default.removeItem(at: remoteStateFile)
    }

    private func obey(_ said: RemoteWord) {
        guard let ticket = tickets.first(where: { $0.id == said.ticket }) else { return }
        if said.late == true {
            // A press kept on the phone goes only where it can still be his latest word: no check is taken up, gone
            // to or put off by it, an item he answered here meanwhile keeps that answer, and a check with such an
            // item is not ended by it either.
            if ["take", "go", "postpone"].contains(said.do) { return }
            if said.do == "finish", lateRefused.contains(ticket.id) { return }
            let answered = ticket.id == active?.id ? marks : kept[ticket.id]?.marks ?? []
            if ["answer", "note", "values"].contains(said.do), let label = said.label,
               answered.first(where: { $0.label == label })?.mac == true {
                lateRefused.insert(ticket.id)
                return
            }
        }
        // A press on the phone never brings the keyboard here: he is not at this screen.
        fromPhone = true
        defer { fromPhone = false }
        // Whatever it is, it is done in the check it names: that one comes up first, as by a click in the queue.
        if ticket.id != active?.id { jump(to: ticket.id) }
        guard active?.id == ticket.id else { return }
        switch said.do {
        case "take": return
        case "postpone": return postpone()
        case "finish":
            let done = said.how == "done"
            if done, !marks.allSatisfy({ $0.state != nil }) { return }
            return finish(how: done ? "done" : "rework")
        default: break
        }
        guard let label = said.label, let number = marks.firstIndex(where: { $0.label == label }) else { return }
        switch said.do {
        case "go": come(to: number)
        case "answer":
            guard let answer = said.number else { return }
            come(to: number)
            mark(answer)
        case "note":
            let text = said.text ?? ""
            marks[number].note = text
            // A comment he is writing here right now stays his latest word.
            if number == index, !isEditingNote {
                note.string = text
                layout()
            }
            touch(number)
        case "values":
            setPhoneValues(said.values ?? [], item: number)
        case "shot":
            guard let path = said.path, path.hasPrefix(shotsDirectory.path + "/"), FileManager.default.fileExists(atPath: path) else { return }
            var shots = marks[number].shots ?? []
            guard shots.count < mostShots, !shots.contains(path) else { return }
            shots.append(path)
            marks[number].shots = shots
            shotsChanged(at: number)
        case "unshot":
            guard let path = said.path, var shots = marks[number].shots, let place = shots.firstIndex(of: path) else { return }
            shots.remove(at: place)
            try? FileManager.default.removeItem(atPath: path)
            marks[number].shots = shots.isEmpty ? nil : shots
            shotsChanged(at: number)
        default: break
        }
    }

    private func come(to number: Int) {
        guard number != index else { return }
        leaveNote()
        index = number
        show()
    }

    private func shotsChanged(at number: Int) {
        tellPhone()
        guard number == index else { return }
        showShots()
        layout()
    }
}
