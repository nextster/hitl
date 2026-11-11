// The queue of checks: reading it, which check is on the window, its answers and the verdict it ends with.

import AppKit

extension Controller {
    // MARK: Queue

    private func readQueue() -> [Ticket] {
        let files = (try? FileManager.default.contentsOfDirectory(at: queueDirectory, includingPropertiesForKeys: nil)) ?? []
        let decoder = JSONDecoder()
        let found = files.filter { $0.pathExtension == "json" }
            .compactMap { try? decoder.decode(Ticket.self, from: Data(contentsOf: $0)) }
            .sorted { ($0.createdAt, $0.id) < ($1.createdAt, $1.id) }
        ending.formIntersection(found.map(\.id))
        // A build that has gone on the phone and has not been on this window yet is the next check: it waits for
        // the one he is at, not for the queue before it.
        let line = found.filter { !ending.contains($0.id) }
        let next = { (ticket: Ticket) in ticket.isOnPhone && !self.broughtForward.contains(ticket.id) }
        return line.filter(next) + line.filter { !next($0) }
    }

    func reload() {
        let previousIDs = Set(tickets.map(\.id))
        tickets = readQueue()
        refreshNames()
        let currentIDs = Set(tickets.map(\.id))
        for id in currentIDs.subtracting(previousIDs) {
            ActionJournal.shared.record("hitl.card.observed", request: id, outcome: "queued", reason: "queue_entry_added")
        }
        for id in previousIDs.subtracting(currentIDs) {
            ActionJournal.shared.record("hitl.card.removed", request: id, outcome: "removed", reason: "queue_entry_removed")
        }
        kept = kept.filter { entry in tickets.contains { $0.id == entry.key } }
        dropLivePages()
        if let current = active, !tickets.contains(where: { $0.id == current.id }) {
            ActionJournal.shared.record("hitl.card.left", request: current.id, outcome: "removed", reason: "active_missing_from_queue")
            active = nil  // dropped from outside
        }
        // A build that has just gone on the phone does not take the place of the check he is at: «если я нахожусь в
        // процессе ответа на какую то проверку не надо мне сбивать его проверкой с телефона. пусть подождёт текущего
        // этапа». It stands first among those that wait, and comes up when that one is ended or put off.
        if active == nil {
            let wasAt = cameBackTo.flatMap { id in tickets.first { $0.id == id } }
            if let first = wasAt ?? tickets.first { take(first) }
        }
        cameBackTo = nil
        if !processingDrops {
            processingDrops = true
            let removed = DropRequests.process(directory: windowDirectory, queued: queueDirectory, done: doneDirectory,
                prepare: { [self] in
                    // Delivered phone edits precede withdrawal. These callbacks
                    // can reload recursively; nested reloads skip drop decisions.
                    hearPhone(); hearRemote()
                },
                protected: { [self] id in
                    if FileManager.default.fileExists(atPath: answersFile.path) {
                        guard let data = try? Data(contentsOf: answersFile),
                              (try? JSONDecoder().decode(Answers.self, from: data)) != nil else { return true }
                    }
                    let held = active?.id == id ? marks : kept[id]?.marks ?? []
                    return held.contains { $0.isHis || !$0.note.isEmpty || $0.values != nil || $0.choice != nil
                        || !($0.choices ?? []).isEmpty || $0.other == true || $0.unclear == true }
                }, automaticResults: { [self] id in
                    if FileManager.default.fileExists(atPath: answersFile.path) {
                        guard let data = try? Data(contentsOf: answersFile),
                              (try? JSONDecoder().decode(Answers.self, from: data)) != nil else { return nil }
                    }
                    let held = active?.id == id ? marks : kept[id]?.marks ?? []
                    guard let data = try? JSONEncoder().encode(held) else { return nil }
                    return (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]]
                }, didDrop: { [self] id, data in
                    let word = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
                    if word?["how"] as? String == "automatic-resolved",
                       let history = try? String(contentsOf: historyFile, encoding: .utf8),
                       history.split(separator: "\n").contains(where: { line in
                           let row = (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any]
                           return row?["id"] as? String == id
                       }) { return }
                    appendHistory(data)
                    ActionJournal.shared.record("hitl.finish", request: id, outcome: word?["how"] as? String ?? "dropped", reason: "serialized_drop")
                })
            processingDrops = false
            if removed { reload(); return }
        }
        guard active != nil else {
            saveAnswers()  // nothing waits: nothing of his is left to keep
            try? FileManager.default.removeItem(at: progressFile)
            if !full { windowMoved() }
            hide()
            anchor = nil  // the next batch of checks comes up where he put the window, or in the middle of the screen
            folded = false  // and unfolded, in the small window
            full = false
            closeLook()
            // A short wait, so that the next check of a batch finds the window.
            if emptySince == nil { emptySince = Date() }
            if let since = emptySince, Date().timeIntervalSince(since) > 20 {
                ActionJournal.shared.record("hitl.panel.exit_requested", request: journalLastCheck, outcome: "exiting", reason: "queue_empty_20s", marked: holds, waiting: tickets.count)
                forgetRemote()
                NSApp.terminate(nil)
            }
            return
        }
        emptySince = nil
        hearPhone()
        refreshChrome()
        writeProgress()
        saveAnswers()
    }

    // MARK: The same check on the phone

    private func side(_ name: String, of ticket: Ticket) -> URL? {
        ticket.folder.map { URL(fileURLWithPath: $0).appendingPathComponent(name) }
    }

    /// Takes in what he did on the phone since the last look: a later answer or comment replaces the one here,
    /// and the window goes to the item he has come to there.
    func hearPhone() {
        guard let ticket = active, ticket.isOnPhone, let file = side("phone.json", of: ticket),
              let data = try? Data(contentsOf: file), let phone = try? JSONDecoder().decode(PhoneSide.self, from: data),
              phone.id == ticket.id else { return }
        var changed = false
        for answer in phone.results {
            guard let number = marks.firstIndex(where: { $0.label == answer.label }), let at = answer.at, at > marks[number].at ?? 0 else { continue }
            // A comment he is writing here right now is his latest word, whatever the phone says of the item.
            let typing = isEditingNote && number == index
            if marks[number].state != answer.state {
                ActionJournal.shared.record("hitl.mark", request: ticket.id, item: number, outcome: answer.state ?? "unset", phone: true)
            }
            marks[number].state = answer.state
            if !typing { marks[number].note = answer.note ?? "" }
            marks[number].at = at
            changed = true
        }
        // He came to another item on the phone later than he last moved here: this window goes there. Not from
        // under a comment he is writing: it will go once he is out of it.
        if let at = phone.currentAt, at > currentAt, !isEditingNote, let number = marks.firstIndex(where: { $0.label == phone.current }) {
            currentAt = at
            if number != index {
                index = number
                changed = true
            }
        }
        // The buttons the panel offers with the items, and how the last one pressed here went.
        let offers = Dictionary((phone.offers ?? []).map { ($0.label, $0.actions) }, uniquingKeysWith: { first, _ in first })
        let here = marks.indices.contains(index) ? marks[index].label : ""
        let offered = offers[here] != phoneOffers[here] || phone.ran != phoneRan
        phoneOffers = offers
        phoneRan = phone.ran
        if changed {
            followingPhone = true
            show()
            followingPhone = false
        } else if offered {
            showPhoneActions()
        }
    }

    /// The buttons of the item on the window that the phone runs.
    var phoneActions: [PhoneSide.Action] {
        guard active?.isOnPhone == true, marks.indices.contains(index) else { return [] }
        return phoneOffers[marks[index].label] ?? []
    }

    /// One of them pressed: the phone runs it as a tap on its own button, and says how it went. A switch is
    /// flipped and a choice goes to its next option; `option` is the option of a choice that was clicked itself.
    func pressPhone(_ number: Int, option: Int? = nil) {
        let actions = phoneActions
        guard actions.indices.contains(number) else { return }
        var asked = actions[number].id
        if let option, let options = actions[number].options, options.indices.contains(option) { asked = options[option].id }
        phoneRun = MacSide.Run(label: marks[index].label, action: asked, at: max(stamp(), (phoneRun?.at ?? 0) + 0.001))
        phoneRunPlace = number
        tellPhone()
        showPhoneActions()
    }

    /// Only the buttons change: the comment he may be writing is left as it is.
    func showPhoneActions() {
        showLinks()
        refreshChrome()
        layout()
    }

    /// What stands after a button's name: where its press has got to. Nothing for a button not pressed last.
    /// The button is known by its place among the item's: a switch flipped asks for another action than before.
    func trail(at number: Int) -> (text: String, failed: Bool) {
        guard let run = phoneRun, phoneRunPlace == number, marks.indices.contains(index), run.label == marks[index].label else { return ("", false) }
        guard let ran = phoneRan, ran.at == run.at else { return ("отправлено на телефон…", false) }
        guard let failure = ran.failure, !failure.isEmpty else { return ("сделано", false) }
        return ("не вышло: " + (failure.count > 70 ? String(failure.prefix(69)) + "…" : failure), true)
    }

    /// Leaves word of what was done here, for the phone: every answer with its time, and the end of the check.
    func tellPhone(finish: String? = nil) {
        guard let ticket = active, ticket.isOnPhone, let file = side("mac.json", of: ticket) else { return }
        try? FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        let said = MacSide(id: ticket.id, results: marks, current: marks.indices.contains(index) ? marks[index].label : nil,
                           currentAt: currentAt > 0 ? currentAt : nil, run: phoneRun, finish: finish)
        if let data = try? JSONEncoder().encode(said) {
            try? data.write(to: file, options: .atomic)
        }
    }

    /// An answer or a comment changed here, now.
    func touch(_ number: Int) {
        if marks.indices.contains(number) { marks[number].mac = fromPhone ? nil : true }
        guard active?.isOnPhone == true, marks.indices.contains(number) else { return }
        marks[number].at = stamp()
        tellPhone()
    }

    /// When something he did here happened: now, and in any case later than everything seen so far, here or
    /// from the phone. The Mac's clock and the phone's are two clocks: compared as they are, a second between
    /// them made the later of two moves the earlier one, and a move made on one screen was taken back by the
    /// other's older one («иногда если попереходить на маке то на телефоне отваливается»). What is done after
    /// another thing was seen is later than it, whatever either clock says.
    func stamp() -> Double {
        max(Date().timeIntervalSince1970, max(currentAt, marks.compactMap(\.at).max() ?? 0) + 0.001)
    }

    /// Puts a check on the window: fresh, or as he left it.
    private func take(_ ticket: Ticket) {
        ActionJournal.shared.record("hitl.card.shown", request: ticket.id, outcome: "selected", related: active?.id ?? "",
                                    windowNumber: panel.windowNumber)
        journalLastCheck = ticket.id
        active = ticket
        if ticket.isOnPhone { broughtForward.insert(ticket.id) }
        if let was = kept[ticket.id], was.marks.count == ticket.items.count {
            marks = was.marks
            index = min(was.index, ticket.items.count - 1)
            shownAt = was.shownAt
        } else {
            marks = ticket.items.map {
                Mark(label: $0.label, state: nil, choice: nil, note: "", question: ($0.options ?? []).isEmpty ? nil : true, shots: nil)
            }
            index = 0
            shownAt = Date().timeIntervalSince1970
        }
        // Coming up is not a move of his: the phone is not pulled to where this window happens to open.
        toldCurrent = marks.indices.contains(index) ? marks[index].label : nil
        currentAt = 0
        phoneOffers = [:]
        phoneRun = nil
        phoneRan = nil
        show()
    }

    /// Keeps what is answered in the check on the window, before another takes its place.
    private func keep() {
        guard let ticket = active else { return }
        leaveNote()
        takeLivePageValues()
        kept[ticket.id] = Kept(marks: marks, index: index, shownAt: shownAt)
    }

    /// «Отложить»: this check goes to the end of the queue, the next one comes up.
    func postpone() {
        guard let ticket = active, tickets.count > 1 else { return }
        ActionJournal.shared.record("hitl.postpone", request: ticket.id, outcome: "postponed", phone: fromPhone)
        keep()
        let file = queueDirectory.appendingPathComponent("\(ticket.id).json")
        if let data = try? Data(contentsOf: file), var raw = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] {
            raw["createdAt"] = Date().timeIntervalSince1970
            if let moved = try? JSONSerialization.data(withJSONObject: raw) { try? moved.write(to: file, options: .atomic) }
        }
        quietUntil = Date().addingTimeInterval(0.5)
        active = nil
        reload()
    }

    /// A click on a check in the queue: it comes up now, the order of the queue stays.
    func jump(to id: String) {
        guard let ticket = tickets.first(where: { $0.id == id }), ticket.id != active?.id else { return }
        keep()
        take(ticket)
        refreshChrome()
        writeProgress()
    }

    /// The items that hold something of his, in the check on the window and in the ones he stepped away from.
    var holds: Int {
        // The check on the window may be among the kept ones, as he left it once: what is here is the later word.
        let aside = kept.filter { $0.key != active?.id }.values.flatMap(\.marks)
        return ((active == nil ? [] : marks) + aside).filter(\.isHis).count
    }

    func writeProgress() {
        guard let ticket = active else { return }
        let progress = Progress(id: ticket.id, marked: holds, answered: marks.filter { $0.state != nil }.count, total: marks.count,
                                shownAt: shownAt, waiting: max(tickets.count - 1, 0))
        if let data = try? JSONEncoder().encode(progress) { try? data.write(to: progressFile, options: .atomic) }
    }

    /// The answer by its place among the rows; -1 is his own answer to a question.
    func mark(_ number: Int) {
        guard active != nil else { return }
        marks[index].note = note.string
        if number == unclearNumber {
            // Not a no: the chat says this item again, and the other answers of the check stand.
            guard offersUnclear else { return }
            marks[index].state = "problem"
            marks[index].unclear = true
            marks[index].choice = nil
            marks[index].choices = nil
            marks[index].other = nil
            touch(index)
            if let ticket = active { ActionJournal.shared.record("hitl.mark", request: ticket.id, item: index, outcome: marks[index].state ?? "unset", phone: fromPhone) }
            // What he did not understand is worth a word: the comment is ready for it (unlike a plain «нет», which opens none).
            show()
            focusNote(ifEmpty: true)
            return
        }
        marks[index].unclear = nil
        if isMany {
            // A tick goes on or comes off, and the item stays: he goes on himself, by the arrow or Tab.
            if number < 0 {
                marks[index].other = marks[index].other == true ? nil : true
            } else if options.indices.contains(number) {
                var ticked = Set(marks[index].choices ?? [])
                if ticked.remove(options[number]) == nil { ticked.insert(options[number]) }
                marks[index].choices = ticked.isEmpty ? nil : options.filter(ticked.contains)
            } else {
                return
            }
            marks[index].choice = nil
            marks[index].state = marks[index].choices != nil || marks[index].other == true ? "ok" : nil
            touch(index)
            if let ticket = active { ActionJournal.shared.record("hitl.mark", request: ticket.id, item: index, outcome: marks[index].state ?? "unset", phone: fromPhone) }
            show()
            // His own answer is the comment: it is ready for it.
            if number < 0, marks[index].other == true { focusNote(ifEmpty: true) }
            return
        }
        if options.isEmpty {
            guard answers.indices.contains(number) else { return }
            marks[index].state = states[number]
            marks[index].choice = nil
        } else if number < 0 {
            marks[index].state = "problem"
            marks[index].choice = nil
        } else if options.indices.contains(number) {
            marks[index].state = "ok"
            marks[index].choice = options[number]
        } else {
            return
        }
        touch(index)
        if let ticket = active { ActionJournal.shared.record("hitl.mark", request: ticket.id, item: index, outcome: marks[index].state ?? "unset", phone: fromPhone) }
        if marks[index].state == "problem", fromPhone || number < 0 || (options.isEmpty && (itemKind ?? "checklist") == "checklist") {
            // «Не работает» and a free-text answer stay on the item for a comment.
            // A remote answer never takes the Mac's keyboard.
            show()
            focusNote(ifEmpty: true)
        } else {
            leaveNote()
            advance()
        }
    }

    /// On to the next item without an answer; with none left, this one stays.
    func advance() {
        if let next = marks.indices.first(where: { $0 > index && marks[$0].state == nil })
            ?? marks.indices.first(where: { marks[$0].state == nil }) {
            index = next
        }
        show()
    }

    func step(_ delta: Int) {
        guard let ticket = active else { return }
        leaveNote()
        index = min(max(index + delta, 0), ticket.items.count - 1)
        show()
    }

    func finish(how: String) {
        guard let ticket = active else { return }
        ActionJournal.shared.record("hitl.finish", request: ticket.id, outcome: "intent", phone: fromPhone)
        leaveNote()
        takeLivePageValues()
        for number in marks.indices { marks[number].note = marks[number].note.trimmingCharacters(in: .whitespacesAndNewlines) }
        quietUntil = Date().addingTimeInterval(0.5)
        if ticket.isOnPhone {
            // The check is the phone's: its queue ends it there too and writes the one verdict, with what was marked on both screens.
            tellPhone(finish: how == "done" ? "done" : "rework")
            ending.insert(ticket.id)
            kept[ticket.id] = nil
            active = nil
            reload()
            return
        }
        for number in marks.indices where ticket.items.indices.contains(number) {
            marks[number].text = ticket.items[number].text
            marks[number].kind = ticket.items[number].kind
            let item = ticket.items[number]
            marks[number].details = item.details
            marks[number].options = item.options
            marks[number].many = item.many
            marks[number].facts = item.facts
            marks[number].links = item.links?.count
            marks[number].copies = item.copies?.count
            marks[number].images = item.images?.count
            marks[number].page = item.page == nil ? nil : true
            marks[number].mac = nil  // the window's own note of where an answer was given, not his answer
        }
        let verdict = Verdict(id: ticket.id, tag: ticket.tag, chat: ticket.chat, agent: ticket.agent, projectName: ticket.projectName, title: ticket.title, kind: ticket.kind,
                              project: ticket.project, branch: ticket.branch, commit: ticket.commit, results: marks,
                              how: how, createdAt: ticket.createdAt, shownAt: shownAt, finishedAt: Date().timeIntervalSince1970,
                              endedOn: fromPhone ? "phone" : nil, about: ticket.about)
        let encoder = JSONEncoder()
        let queued = queueDirectory.appendingPathComponent("\(ticket.id).json")
        let done = doneDirectory.appendingPathComponent("\(ticket.id).json")
        // The window's look at the queue is up to a second old: a check its chat has taken back meanwhile is not
        // ended by «Готово» (the audit of 2026-10-16, H03).
        guard FileManager.default.fileExists(atPath: queued.path) else {
            ActionJournal.shared.record("hitl.finish", request: ticket.id, outcome: "refused", phone: fromPhone, reason: "queue_entry_missing")
            kept[ticket.id] = nil
            active = nil
            reload()
            return
        }
        // The ticket goes only once the verdict is on the disk: it is what the chat reads. A write that failed
        // took the ticket away all the same and left the chat nothing (A09); now the check stays with his
        // answers, the window says so, and «Готово» tries again. The verdict is linked in, never written over
        // one that is there: a check has one verdict, the first, whether a drop or this window wrote it (H03).
        let data: Data
        do {
            data = try encoder.encode(verdict)
            try FileManager.default.createDirectory(at: doneDirectory, withIntermediateDirectories: true)
            let aside = doneDirectory.appendingPathComponent(".\(ticket.id).\(getpid()).claim")
            try data.write(to: aside, options: .atomic)
            defer { try? FileManager.default.removeItem(at: aside) }
            if link(aside.path, done.path) != 0 {
                let failure = errno
                guard failure == EEXIST else { throw POSIXError(POSIXErrorCode(rawValue: failure) ?? .EIO) }
                ActionJournal.shared.record("hitl.finish", request: ticket.id, outcome: "refused", phone: fromPhone, reason: "verdict_already_exists")
                try? FileManager.default.removeItem(at: queued)
                kept[ticket.id] = nil
                active = nil
                reload()
                return
            }
        } catch {
            ActionJournal.shared.record("hitl.finish", request: ticket.id, outcome: "storage_error", phone: fromPhone)
            savingTrouble[ticket.id] = "Ответ не сохранился на диск. «Готово» — ещё раз"
            NSSound.beep()
            show()
            layout()
            return
        }
        savingTrouble[ticket.id] = nil
        ActionJournal.shared.record("hitl.finish", request: ticket.id, outcome: how, phone: fromPhone)
        appendHistory(data)
        try? FileManager.default.removeItem(at: queued)
        kept[ticket.id] = nil
        active = nil
        reload()
    }

    /// One line of the history, written whole at its end: the commands of hitl write theirs there at the same
    /// time, and a line written at a place found a moment before took the place of theirs (the audit of
    /// 2026-10-16, H02: 214 of 2000 lines lost).
    func appendHistory(_ data: Data) {
        let line = data + Data("\n".utf8)
        let file = Darwin.open(historyFile.path, O_WRONLY | O_APPEND | O_CREAT, 0o644)
        guard file >= 0 else { return }
        defer { Darwin.close(file) }
        _ = line.withUnsafeBytes { Darwin.write(file, $0.baseAddress, $0.count) }
    }
}
