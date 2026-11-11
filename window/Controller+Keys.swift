// The keys: a key that is down and what letting it go presses, and what every key does.

import AppKit

extension Controller {
    // MARK: Keys

    /// A key that is down, and what letting it go will press. Nothing is pressed until then, and what will be is
    /// lit meanwhile: «когда я зажимаю клавишу подсвечивалось то что произойдёт при отпускании, а выполнялось
    /// только при отпускании».
    struct Held {
        let code: UInt16
        let command: Bool
        /// What the key presses; looked up anew, because the window rebuilds its rows.
        let tap: () -> Tap?
        /// The item the key went down over: its release answers that one or nothing.
        let ticket: String
        let index: Int
        let act: () -> Void
    }

    private func hold(_ event: NSEvent, _ tap: @escaping () -> Tap?, _ act: @escaping () -> Void) {
        // Another key went down before this one came up, as in fast typing: the first one counts as let go.
        letGo(press: true)
        held = Held(code: event.keyCode, command: event.modifierFlags.contains(.command), tap: tap,
                    ticket: active?.id ?? "", index: index, act: act)
        tap()?.isHeld = true
    }

    /// The held key came up, or was called off: by esc, or by the keyboard going elsewhere.
    func letGo(press: Bool) {
        guard let was = held else { return }
        held = nil
        was.tap()?.isHeld = false
        if press, was.ticket == (active?.id ?? ""), was.index == index { was.act() }
    }

    /// A key came up or a modifier changed. macOS does not always tell an app of a key let go while ⌘ is down,
    /// so letting ⌘ go counts for the keys pressed with it.
    func released(_ event: NSEvent) {
        guard let was = held else { return }
        if event.type == .keyUp, event.keyCode == was.code {
            letGo(press: true)
        } else if event.type == .flagsChanged, was.command, !event.modifierFlags.contains(.command) {
            letGo(press: true)
        }
    }

    private func row(_ number: Int) -> () -> Tap? {
        { [weak self] in self.flatMap { $0.answerRows.indices.contains(number) ? $0.answerRows[number] : nil } }
    }

    /// Whether the key was taken. Every key is written at the right of what it presses, and presses it when let go.
    func handle(_ event: NSEvent) -> Bool {
        guard active != nil else { return false }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let code = event.keyCode
        if held != nil, code == 53 {  // esc calls a held key off
            if !event.isARepeat { letGo(press: false) }
            return true
        }
        if lookingAt != nil {
            // A picture is looked at, and the keys are the window's all the while: space and esc put the card
            // away, the arrows and ⌘ with a digit go to another picture, and every other key does what it does. In a
            // comment only esc is the picture's: the rest is typed.
            if code == 53 || (code == 49 && !isEditingNote) {
                if !event.isARepeat { closeLook() }
                return true
            }
            if !isEditingNote, lookTakes(event) { return true }
        }
        let editing = isEditingNote
        let back = flags.contains(.shift)
        let typed = event.charactersIgnoringModifiers ?? ""

        if flags.contains(.command) {
            switch code {
            case 36, 76:  // ⌘↵ ends the check, ⇧⌘↵ sends it back; from the comment too
                if event.isARepeat || isQuiet { return true }
                if back {
                    hold(event, { [weak self] in self?.rework }) { [weak self] in self?.finish(how: "rework") }
                } else if done.isEnabled {
                    hold(event, { [weak self] in self?.done }) { [weak self] in self?.finish(how: "done") }
                }
                return true
            case 3 where flags.contains(.control):  // ⌃⌘F: the whole screen, and back
                if !event.isARepeat { hold(event, { [weak self] in self?.wide }) { [weak self] in self?.toggleFull() } }
                return true
            case 46:  // ⌘M: one line, and back
                if !event.isARepeat { hold(event, { [weak self] in self?.fold }) { [weak self] in self?.toggleFold() } }
                return true
            case 9 where !folded:  // ⌘V, wherever in the window the keyboard is
                paste(from: .general)
                return true
            case 1 where !folded && active?.isOnPhone == true:  // ⌘S: the phone's screen into the comment
                if !event.isARepeat { hold(event, { [weak self] in self?.phoneShot }) { [weak self] in self?.takePhoneShot() } }
                return true
            default:
                break
            }
            if folded { return false }
            if let digit = Int(typed), digit >= 1 {  // ⌘1, ⌘2: the item's pictures, looked at closer, then its lines to copy, what the phone does for it, what it offers to open and the links in its texts
                if !event.isARepeat {
                    let shown = shownPictures.count
                    let lines = copies.count
                    if digit <= shown {
                        hold(event, { [weak self] in self?.pictureTap(digit - 1) }) { [weak self] in self?.look(at: digit - 1) }
                    } else if digit <= shown + lines {
                        let line = digit - 1 - shown
                        hold(event, { [weak self] in self.flatMap { $0.copyTaps.indices.contains(line) ? $0.copyTaps[line] : nil } }) {
                            [weak self] in self?.copy(line: line)
                        }
                    } else if digit > shown + lines + linkTaps.count {
                        let link = digit - 1 - shown - lines - linkTaps.count
                        hold(event, { [weak self] in self.flatMap { $0.textKeys.indices.contains(link) ? $0.textKeys[link] : nil } }) {
                            [weak self] in
                            guard let self, self.textKeys.indices.contains(link) else { return }
                            self.open(address: self.textKeys[link].address)
                        }
                    } else {
                        let link = digit - 1 - shown - lines
                        hold(event, { [weak self] in self.flatMap { $0.linkTaps.indices.contains(link) ? $0.linkTaps[link] : nil } }) {
                            [weak self] in
                            guard let self else { return }
                            let onPhone = self.phoneActions.count
                            if link < onPhone { self.pressPhone(link) } else { self.open(link: link - onPhone) }
                        }
                    }
                }
                return true
            }
            guard let text = panel.firstResponder as? NSTextView else { return false }
            // A window of an app without a menu has no Edit menu to do these.
            switch code {
            case 8: text.copy(nil)
            case 0: text.selectAll(nil)
            case 7 where text.isEditable: text.cut(nil)
            case 6 where text.isEditable: back ? text.undoManager?.redo() : text.undoManager?.undo()
            default: return false
            }
            return true
        }
        if flags.contains(.control) || folded { return false }

        if editing {
            // The keys belong to the text, except the ones that leave it.
            if note.text.hasMarkedText() { return false }
            switch code {
            case 36 where !back && !flags.contains(.option), 76 where !back && !flags.contains(.option):
                leaveNote()
                if marks[index].state != nil { advance() }
            case 53:
                leaveNote()
            case 48:  // out of the comment and on, without touching the answer
                step(back ? -1 : 1)
            case 123 where note.string.isEmpty, 126 where note.string.isEmpty:
                step(-1)
            case 124 where note.string.isEmpty, 125 where note.string.isEmpty:
                step(1)
            default:
                return false
            }
            return true
        }

        if event.isARepeat || isQuiet { return true }  // a held key answers one item, not a row of them
        switch code {
        case 36, 76, 49, 53:  // return, enter, space, escape: they answered once; now they do nothing, and quietly
            break
        case 123, 126:
            hold(event, { [weak self] in self?.previous }) { [weak self] in self?.step(-1) }
        case 124, 125:
            hold(event, { [weak self] in self?.following }) { [weak self] in self?.step(1) }
        case 48:  // tab: on, or back, without answering
            hold(event, { [weak self] in back ? self?.previous : self?.following }) { [weak self] in self?.step(back ? -1 : 1) }
        default:
            if typed == unclearKey, !flags.contains(.command), offersUnclear || (itemKind == "explain" && active?.isOnPhone != true && options.isEmpty && answers.count > 1) {
                if offersUnclear {  // «не понял», the last row of every item
                    hold(event, { [weak self] in self?.answerRows.last }) { [weak self] in self?.mark(unclearNumber) }
                } else {  // an explanation says it with its own second answer, «не понятно»
                    hold(event, row(1)) { [weak self] in self?.mark(1) }
                }
                return true
            }
            if let digit = Int(typed), digit >= 1 {  // a digit is an answer by its place: yes, no, not now, or an option
                let place = digit - 1
                if options.isEmpty {
                    if digit <= answers.count { hold(event, row(place)) { [weak self] in self?.mark(place) } }
                } else if digit <= options.count {
                    hold(event, row(place)) { [weak self] in self?.mark(place) }
                } else if digit == options.count + 1 {
                    hold(event, row(place)) { [weak self] in self?.mark(-1) }  // his own answer, after the options
                }
                return true
            }
            // Anything else that prints is the start of a comment.
            guard let first = typed.unicodeScalars.first, typed.unicodeScalars.count == 1, first.value > 0x20, first.value != 0x7F,
                  !(0xF700...0xF8FF).contains(first.value) else { return false }
            focusNote()
            return false
        }
        return true
    }

    /// Right after a check is ended the keys rest: a key meant for it must not answer the next one.
    private var isQuiet: Bool {
        guard scriptFile == nil, let quietUntil else { return false }
        return Date() < quietUntil
    }
}
