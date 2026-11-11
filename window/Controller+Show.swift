// Showing an item, and what changes around it without the item changing: who else waits, where the keyboard is.

import AppKit
import UniformTypeIdentifiers

extension Controller {
    // MARK: Showing

    var options: [String] { active?.items[index].options ?? [] }
    /// Several of the item's options may hold at once: a digit ticks one or takes its tick off, and he goes on himself.
    var isMany: Bool { active?.items[index].many == true && !options.isEmpty }
    /// A link that is a picture on this Mac: it is shown in the window, not offered to open.
    private func isPicture(_ link: String) -> Bool {
        guard link.hasPrefix("/") || link.hasPrefix("~") else { return false }
        let file = (link as NSString).expandingTildeInPath
        return UTType(filenameExtension: (file as NSString).pathExtension)?.conforms(to: .image) == true && FileManager.default.fileExists(atPath: file)
    }

    private var ownLinks: [String] { (active?.items[index].links ?? []).filter { !isPicture($0) } }
    /// The item's pictures: given as such, or as links to picture files.
    var shownPictures: [String] {
        guard let item = active?.items[index] else { return [] }
        return Array(((item.images ?? []) + (item.links ?? []).filter(isPicture).map { ($0 as NSString).expandingTildeInPath }).prefix(4))
    }
    /// The chat's card and the chat itself, when the check has them: offered at every item, after its own links.
    private var chatOpens: [(title: String, link: String)] {
        guard let ticket = active else { return [] }
        var found: [(title: String, link: String)] = []
        if let card = ticket.card { found.append((capital(card.title), card.path)) }
        if let link = chatReference(ticket), !link.isEmpty { found.append(("Чат", link)) }
        return found
    }
    /// The lines of the item he copies with a click.
    var copies: [String] { active?.items[index].copies ?? [] }
    /// What the item offers to open: its own links, then the chat's card, then the chat.
    var opens: [String] { ownLinks + chatOpens.map(\.link) }
    /// The picture as it stands in the window now: in its grid, or large at the right of the page.
    func pictureTap(_ number: Int) -> Picture? {
        let files = shownPictures
        guard files.indices.contains(number) else { return nil }
        return (picturesBeside ? gallery : pictures).first { $0.path == files[number] }
    }
    /// The kind of an item: its own, or the check's.
    func kind(at number: Int) -> String? {
        guard let ticket = active else { return nil }
        return (ticket.items.indices.contains(number) ? ticket.items[number].kind : nil) ?? ticket.kind
    }
    var itemKind: String? { kind(at: index) }
    var answers: [String] { Array((active?.items[index].answers ?? active?.answers ?? usualAnswers).prefix(states.count)) }
    /// «Не понял» stands under the answers of every item, but not where it is there already, in an explanation,
    /// and not in a check that is on the phone: its panel has three marks and knows no fourth.
    var offersUnclear: Bool {
        guard let ticket = active else { return false }
        return !ticket.isOnPhone && itemKind != "explain"
    }
    var isEditingNote: Bool { panel.firstResponder === note.text }
    var hasKeyboard: Bool { scriptedKeyboard || panel.isKeyWindow }

    @objc func keyboardMoved() {
        // A key held while the keyboard goes elsewhere presses nothing here, and Shift moves nothing.
        if !hasKeyboard {
            if isLookUp { keysLeftWithLookUp() }
            letGo(press: false)
            if carry?.byMouse != true { setDown() }
        }
        note.needsDisplay = true
        refreshChrome()
    }

    private func capital(_ text: String) -> String { text.prefix(1).uppercased() + text.dropFirst() }

    /// The lines to copy, under the item's pictures: each on ⌘ with the digit after the pictures', and on a click.
    func showCopies() {
        copiesColumn.arrangedSubviews.forEach { $0.removeFromSuperview() }
        copyTaps = []
        guard let ticket = active else { return }
        let before = shownPictures.count
        let here = ticket.id + "/" + ticket.items[index].label
        for (number, text) in copies.enumerated() {
            let line = CopyLine(text)
            line.setAccessibilityLabel("Скопировать: " + text)
            line.key = before + number < 9 ? "⌘\(before + number + 1)" : ""
            line.toolTip = line.isCut ? text : "Скопировать"
            line.copied = copiedLine.map { $0.item == here && $0.place == number } ?? false
            line.press = { [weak self] in self?.copy(line: number) }
            copyTaps.append(line)
            copiesColumn.addArrangedSubview(line)
        }
        shown[ObjectIdentifier(copiesColumn)] = !copyTaps.isEmpty
    }

    /// What the item offers besides its answers, under its pictures.
    func showLinks() {
        // What the item offers besides its answers: on ⌘ with the digits after the pictures' and the copied
        // lines', so that the plain digits stay with the answers. First what the phone does for the item, the
        // buttons of its panel under the same names («оно должно дублироваться как экшн на маке»), then what
        // there is to open.
        let files = shownPictures
        let taken = files.count + copies.count
        linksColumn.arrangedSubviews.forEach { $0.removeFromSuperview() }
        linkTaps = []
        let onPhone = phoneActions
        for (number, action) in onPhone.enumerated() {
            let tap = Flat(action.title)
            // Each as what it is on the phone: an address, a switch and how it stands, one of several, a name
            // for what is a symbol there.
            switch action.kind {
            case "link": tap.isAddress = true
            case "toggle":
                tap.symbol = action.symbol
                tap.isOn = action.on ?? false
            case "choice":
                tap.options = (action.options ?? []).map(\.title)
                tap.current = action.options?.firstIndex { $0.id == action.current }
                tap.pick = { [weak self] option in self?.pressPhone(number, option: option) }
            case "action": tap.symbol = action.symbol
            default: break
            }
            tap.setAccessibilityLabel(tap.said)
            tap.key = taken + number < 9 ? "⌘\(taken + number + 1)" : ""
            let went = trail(at: number)
            tap.trail = went.text
            tap.trailColor = went.failed ? Ink.no : Ink.faint
            tap.press = { [weak self] in self?.pressPhone(number) }
            linkTaps.append(tap)
            linksColumn.addArrangedSubview(tap)
        }
        let own = ownLinks.count
        let ofChat = chatOpens
        for (number, link) in opens.enumerated() {
            let tap = Flat(number < own ? name(of: link) : ofChat[number - own].title)
            tap.opens = true
            let place = taken + onPhone.count + number
            tap.key = place < 9 ? "⌘\(place + 1)" : ""
            tap.press = { [weak self] in self?.open(link: number) }
            linkTaps.append(tap)
            linksColumn.addArrangedSubview(tap)
        }
        shown[ObjectIdentifier(linksHolder)] = !linkTaps.isEmpty
        keyTextLinks()
    }

    /// The texts of the check that may hold links, from the top.
    var linkedTexts: [LinkedText] { [context.about, question, details, facts] }

    /// The links written into the texts are opened from the keyboard too, his pick: «И по ⌘ с цифрой». Their digits
    /// go on after everything else the item has on such keys, so that those keep theirs, and each stands on a
    /// cap after its link.
    func keyTextLinks() {
        var place = shownPictures.count + copies.count + linkTaps.count
        textKeys = []
        for view in linkedTexts {
            var keys: [String] = []
            for link in view.links {
                let tap = TextKey(link.words)
                tap.key = place < 9 ? "⌘\(place + 1)" : ""
                tap.address = link.address
                let among = keys.count
                tap.lit = { [weak view] down in view?.text.press(cap: among, down) }
                keys.append(tap.key)
                textKeys.append(tap)
                place += 1
            }
            view.keys = keys
        }
    }

    /// What changes without the item changing: who else waits, and where the keyboard is.
    func refreshChrome() {
        guard let ticket = active else { return }
        refreshNameHeader(ticket)
        let others = tickets.filter { $0.id != ticket.id }
        // A waiting check is named by its chat and its kind. Two that would read the same, a plan and a result
        // of one chat once looked like one check shown twice, are named by their own titles.
        func short(_ text: String, _ most: Int) -> String { text.count > most ? String(text.prefix(most - 1)) + "…" : text }
        let plain = others.map { other in
            other.tag.isEmpty ? short(other.title, 24) : [short(other.tag, 20), other.kindTitle ?? ""].filter { !$0.isEmpty }.joined(separator: " · ")
        }
        var names = plain
        for number in names.indices where plain.filter({ $0 == plain[number] }).count > 1 {
            names[number] = [short(others[number].tag, 20), short(others[number].title, 24)].filter { !$0.isEmpty }.joined(separator: " · ")
        }
        // As many as fit in the line; the rest is a number.
        let label = NSTextField(labelWithString: "Дальше")
        label.font = Face.hint
        label.textColor = Ink.faint
        var taps: [Flat] = []
        var room = contentWidth - label.intrinsicContentSize.width
        for (number, name) in names.enumerated() {
            let tap = Flat(name)
            tap.size = 12 + Face.larger
            let id = others[number].id
            tap.press = { [weak self] in self?.jump(to: id) }
            tap.toolTip = others[number].title
            let rest = Flat("ещё \(names.count - number - 1)")
            rest.size = 12 + Face.larger
            let needed = tap.intrinsicContentSize.width + (number < names.count - 1 ? rest.intrinsicContentSize.width : 0)
            if needed > room, !taps.isEmpty || number < names.count - 1 {
                let left = Flat("ещё \(names.count - number)")
                left.size = 12 + Face.larger
                left.isEnabled = false
                taps.append(left)
                break
            }
            room -= tap.intrinsicContentSize.width
            taps.append(tap)
        }
        let before = queueTaps.map(\.title) + [hint.stringValue, later.isHidden ? "-" : "+"]
        if taps.map(\.title) != queueTaps.map(\.title) {
            queueRow.arrangedSubviews.forEach { $0.removeFromSuperview() }
            queueTaps = taps
            if !taps.isEmpty {
                queueRow.addArrangedSubview(label)
                taps.forEach { queueRow.addArrangedSubview($0) }
            }
        }
        shown[ObjectIdentifier(queueRow)] = !queueTaps.isEmpty
        later.isHidden = others.isEmpty
        // Only while a comment is written. Where the keyboard is, the keys say themselves: a line that came and
        // went with it resized the window under the very click that brought the keyboard, and the click missed
        // its button («первое нажатие на эту кнопку … не сворачивает его а убирает эту надпись»).
        hint.stringValue = isEditingNote ? "↵ дальше · ⇧↵ новая строка · esc выйти" : ""
        // On the page the line keeps its room while it says nothing: coming and going, it made the page count
        // itself anew, and an item that fills the screen was drawn smaller, all of it, at the click into the
        // comment («снизу выезжают эти подсказки и перелейаучивают всё окно»).
        shown[ObjectIdentifier(hint)] = full || !hint.stringValue.isEmpty
        for tap in pictures as [Tap] + gallery + copyTaps + linkTaps + answerRows + [rework, done] { tap.keysLive = hasKeyboard }
        for view in linkedTexts { view.text.capsLive = hasKeyboard }
        let place = ticket.items.count > 1 ? "\(index + 1) из \(ticket.items.count)" : ""
        count.stringValue = folded && !others.isEmpty ? [place, "ещё \(others.count)"].filter { !$0.isEmpty }.joined(separator: " · ") : place
        count.isHidden = count.stringValue.isEmpty
        let changed = before != queueTaps.map(\.title) + [hint.stringValue, later.isHidden ? "-" : "+"]
        arrange()
        if changed { layout() }
    }

    /// Shows what the item has and, folded, only the first line.
    func arrange() {
        for view in body {
            // On the page the pictures stand large at its right and the waiting checks are in its list.
            let wanted = !folded && (shown[ObjectIdentifier(view)] ?? true) && !(picturesBeside && view === picturesGrid) && !(full && view === queueRow)
            if view.isHidden == wanted { view.isHidden = !wanted }
        }
        if box.isHidden != folded { box.isHidden = folded }
        // Folded into a line, the head is that line: the folder under the name goes, and the icon is as tall as the tag.
        let withOrigin = !folded && !origin.from.isEmpty
        if origin.isHidden == withOrigin { origin.isHidden = !withOrigin }
        if headWithOrigin.first?.isActive != withOrigin {
            NSLayoutConstraint.deactivate(withOrigin ? headWithoutOrigin : headWithOrigin)
            NSLayoutConstraint.activate(withOrigin ? headWithOrigin : headWithoutOrigin)
        }
        app.tall = folded ? AppMark.inLine : AppMark.inHead
        nameAfterIcon.constant = app.isHidden ? -app.intrinsicContentSize.width : folded ? 8 : 11
        if bar.isHidden != folded { bar.isHidden = folded }
        barTall.constant = folded ? 0 : barHeight
        backdrop.bar = folded ? 0 : barHeight
        wide.isHidden = folded
        wide.symbol = full ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right"
        wide.title = full ? "В окно" : "На весь экран"
        wide.toolTip = full ? "Обратно в окно, ⌃⌘F" : "На весь экран, ⌃⌘F"
        fold.symbol = folded ? "chevron.down" : "chevron.up"
        fold.title = folded ? "Развернуть" : "Свернуть"
        fold.toolTip = folded ? "Развернуть, ⌘M" : "Свернуть в строку, ⌘M"
    }

    /// The colour of an item's answer. An answer in his own words is an answer: it takes the check's colour, not the colour of a "no".
    func ink(of mark: Mark, in color: NSColor) -> NSColor? {
        switch mark.state {
        case "ok": return color
        case "problem": return mark.unclear == true ? Ink.unclear : mark.question == true ? color : Ink.no
        case "skipped": return Ink.faint
        default: return nil
        }
    }

    /// The question stands beside its number, in what the column has left.
    func fitQuestion() {
        question.room = columnWide - (number.isHidden ? 0 : number.intrinsicContentSize.width.rounded(.up) + 6)
    }

    func show() {
        guard let ticket = active else { return }
        let item = ticket.items[index]
        let mark = marks[index]
        // Another item is read from its top; the same one, answered or written under, stays where he scrolled it.
        if scrolledItem != ticket.id + "#" + String(index) {
            scrolledItem = ticket.id + "#" + String(index)
            scrolled = 0
        }
        // The colour and the word at the top are the item's: in a check of several kinds they change as he goes.
        let color = tint(of: itemKind)
        // The card shows the pictures of the item he looked from: on another item it has nothing to show.
        if lookingAt != nil, lookedIn != ticket.id + "/" + mark.label { closeLook() }
        if ticket.isOnPhone, mark.label != toldCurrent {
            // He came to another item here, by a key, an answer or a click: the panel on the phone goes there too.
            toldCurrent = mark.label
            if !followingPhone {
                currentAt = stamp()
                tellPhone()
            }
        }

        // The app is said by its mark, before the project. A chat is called by its name; one without a name, by
        // the folder it works in.
        let folder = [ticket.cwd, ticket.project].compactMap { $0 }.first { !$0.isEmpty } ?? ""
        refreshNameHeader(ticket)
        app.agent = ticket.agent ?? ""
        project.text = ticket.projectName ?? ""
        origin.come(from: (folder as NSString).abbreviatingWithTildeInPath, branch: ticket.branch ?? "")
        kindLabel.text = ticket.isOnPhone ? "на телефоне" + (ticket.device.map { " " + $0 } ?? "") : item.kindTitle ?? ticket.kindTitle ?? ""
        kindLabel.tint = color
        kindLabel.trouble = false
        if let trouble = savingTrouble[ticket.id] {
            kindLabel.text = trouble
            kindLabel.trouble = true
        }
        segments.colors = marks.enumerated().map { ink(of: $1, in: tint(of: kind(at: $0))) }
        segments.current = index
        shown[ObjectIdentifier(segments)] = ticket.items.count > 1
        context.title.stringValue = ticket.title
        context.about.said = ticket.about ?? ""
        context.about.isHidden = context.about.stringValue.isEmpty

        // The question is what the eye lands on; its number stands beside it, quieter, and long text hangs under the text.
        number.stringValue = !item.label.isEmpty && ticket.items.count > 1 ? "\(item.label)." : ""
        number.isHidden = number.stringValue.isEmpty
        fitQuestion()
        question.said = item.text
        if let was = item.before {
            let day = Date(timeIntervalSince1970: was.at)
            let clock = DateFormatter()
            clock.dateFormat = Calendar.current.isDateInToday(day) ? "'в' HH:mm" : "dd.MM 'в' HH:mm"
            before.said = ((was.answer.isEmpty ? "Ты написал " : "Ты ответил ") + clock.string(from: day) + (was.answer.isEmpty ? "" : ": " + was.answer),
                           was.note ?? "")
        }
        shown[ObjectIdentifier(before)] = item.before != nil
        details.said = item.details ?? ""
        shown[ObjectIdentifier(details)] = !(item.details ?? "").isEmpty
        facts.facts = item.facts ?? []
        facts.tint = color
        shown[ObjectIdentifier(facts)] = !facts.facts.isEmpty
        showLivePage()

        // Pictures stand right in the window: one across it, more of them two in a row. Each is looked at closer
        // by its key, ⌘1 and on: «чтобы можно было квиклукнуть по cmd+1/2/3... среди прочих экшнов».
        picturesGrid.arrangedSubviews.forEach { $0.removeFromSuperview() }
        pictures = []
        let files = shownPictures
        for start in stride(from: 0, to: files.count, by: 2) {
            let pair = Array(files[start..<min(start + 2, files.count)])
            let width = files.count == 1 ? contentWidth : ((contentWidth - 8) / 2).rounded(.down)
            let line = NSStackView()
            line.orientation = .horizontal
            line.alignment = .top
            line.spacing = 8
            for path in pair {
                let picture = Picture(path: path, width: width, tallest: files.count == 1 ? 260 : 150)
                let place = pictures.count
                picture.key = "⌘\(place + 1)"
                picture.press = { [weak self] in self?.look(at: place) }
                pictures.append(picture)
                line.addArrangedSubview(picture)
            }
            picturesGrid.addArrangedSubview(line)
        }
        shown[ObjectIdentifier(picturesGrid)] = !files.isEmpty

        showCopies()
        showLinks()

        answersColumn.arrangedSubviews.forEach { $0.removeFromSuperview() }
        answerRows = []
        if options.isEmpty {
            // Yes, no, not now.
            for (number, title) in answers.enumerated() {
                let row = Row(capital(title))
                row.key = answerKeys[number]
                row.chosen = mark.state == states[number] && mark.unclear != true ? [color, Ink.no, Ink.faint][number] : nil
                row.press = { [weak self] in self?.mark(number) }
                answerRows.append(row)
            }
        } else {
            // A question: its options by number, and his own words when none fits. Where several may hold at
            // once, each has a box, and so has his own answer.
            let many = isMany
            let ticked = Set(mark.choices ?? [])
            for (number, title) in options.enumerated() {
                let row = Row(title)
                row.key = number < 9 ? "\(number + 1)" : ""
                if many { row.box = ticked.contains(title) }
                row.chosen = (many ? ticked.contains(title) : mark.choice == title) ? color : nil
                row.press = { [weak self] in self?.mark(number) }
                answerRows.append(row)
            }
            let other = Row(ownAnswer)
            other.key = options.count < 9 ? "\(options.count + 1)" : ""
            if many { other.box = mark.other == true }
            other.chosen = (many ? mark.other == true : mark.state == "problem" && mark.unclear != true) ? color : nil
            other.press = { [weak self] in self?.mark(-1) }
            answerRows.append(other)
        }
        if offersUnclear {
            let unclear = Row(unclearAnswer)
            unclear.key = unclearKey
            unclear.chosen = mark.unclear == true ? Ink.unclear : nil
            unclear.quiet = true
            unclear.press = { [weak self] in self?.mark(unclearNumber) }
            answerRows.append(unclear)
        }
        answerRows.forEach { answersColumn.addArrangedSubview($0) }
        shown[ObjectIdentifier(several)] = isMany
        // The rows are new ones: a key held over the item is still held.
        held?.tap()?.isHeld = true

        note.tint = color
        note.string = mark.note
        showShots()
        previous.isHidden = ticket.items.count < 2
        following.isHidden = ticket.items.count < 2
        previous.isEnabled = index > 0
        following.isEnabled = index < ticket.items.count - 1
        done.isEnabled = marks.allSatisfy { $0.state != nil }
        done.strong = done.isEnabled
        refreshChrome()
        layout()
        writeProgress()
        saveAnswers()
    }
}
