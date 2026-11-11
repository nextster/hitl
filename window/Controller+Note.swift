// The comment of an item: its screenshots, getting into it and out, what ⌘V puts there.

import AppKit
import UniformTypeIdentifiers

extension Controller {
    /// The screenshots of this item's comment, in a row under it.
    func showShots() {
        shotsRow.arrangedSubviews.forEach { $0.removeFromSuperview() }
        let shots = marks[index].shots ?? []
        for (number, path) in shots.enumerated() {
            let shot = ShotView(path: path)
            shot.open = { [weak self] in self?.look(at: number, among: shots) }
            shot.remove = { [weak self] in self?.removeShot(number) }
            shotsRow.addArrangedSubview(shot)
        }
        let onPhone = active?.isOnPhone == true
        if onPhone {
            phoneShot.key = "⌘S"
            phoneShot.title = takingShot ? "Снимаю телефон…" : "Снимок с телефона"
            phoneShot.isEnabled = !takingShot && shots.count < mostShots
            phoneShot.press = { [weak self] in self?.takePhoneShot() }
            shotsRow.addArrangedSubview(phoneShot)
        }
        shown[ObjectIdentifier(shotsRow)] = !shots.isEmpty || onPhone
        arrange()
    }

    /// The phone's screen as it is now goes under the comment: «кнопка в окне: Мак сам снимает телефон».
    func takePhoneShot() {
        guard let ticket = active, ticket.isOnPhone, let udid = ticket.udid, !takingShot,
              (marks[index].shots ?? []).count < mostShots else { return }
        let id = ticket.id, item = index
        func took(_ data: Data?) {
            takingShot = false
            // He may have gone on to another item meanwhile: the picture is of what was on the phone for this one.
            if active?.id == id, index == item, let data, let picture = png(from: data) {
                attach(picture)
            } else {
                if data == nil { NSSound.beep() }
                showShots()
                layout()
            }
        }
        if let shotStandIn {
            took(try? Data(contentsOf: URL(fileURLWithPath: shotStandIn)))
            return
        }
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("hitl-phone-\(getpid())-\(Int(Date().timeIntervalSince1970 * 1000)).png")
        let capture = Process()
        capture.executableURL = URL(fileURLWithPath: "/usr/bin/xcrun")
        capture.arguments = ["devicectl", "device", "capture", "screenshot", "--device", udid, "--destination", file.path]
        capture.standardOutput = FileHandle.nullDevice
        capture.standardError = FileHandle.nullDevice
        capture.terminationHandler = { _ in
            DispatchQueue.main.async {
                let data = try? Data(contentsOf: file)
                try? FileManager.default.removeItem(at: file)
                took(data)
            }
        }
        takingShot = true
        showShots()
        do { try ActionJournal.shared.launch(capture, purpose: "phone_screenshot", request: id) } catch { return took(nil) }
        // A phone that does not answer does not keep «Снимаю телефон…» for good: the capture is ended, and the
        // button is his again (H10b).
        DispatchQueue.main.asyncAfter(deadline: .now() + phoneShotSeconds) {
            if capture.isRunning { capture.terminate() }
        }
    }

    func focusNote(ifEmpty: Bool = false) {
        // He pressed it on the phone: the comment is written there, and the keyboard here stays where it is.
        if fromPhone || (ifEmpty && !note.string.isEmpty) { return }
        if scriptFile == nil { panel.makeKey() }
        panel.makeFirstResponder(note.text)
        note.text.setSelectedRange(NSRange(location: (note.string as NSString).length, length: 0))
    }

    func leaveNote() {
        guard active != nil else { return }
        marks[index].note = note.string
        if isEditingNote { panel.makeFirstResponder(nil) }
    }

    func noteChanged() {
        guard active != nil else { return }
        marks[index].note = note.string
        touch(index)
        layout()
        saveAnswers()  // every letter: a comment half written is his too
    }

    // MARK: Pasting

    /// ⌘V: a screenshot from the clipboard goes under the comment, text goes into it.
    func paste(from board: NSPasteboard) {
        guard active != nil else { return }
        let files = (board.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]) ?? []
        if !files.isEmpty {
            // Files copied in Finder: the pictures among them, and nothing else of what Finder puts beside.
            let pictures = files.filter { UTType(filenameExtension: $0.pathExtension)?.conforms(to: .image) == true }
            if pictures.isEmpty { NSSound.beep() }
            for file in pictures { if let data = try? Data(contentsOf: file), let picture = png(from: data) { attach(picture) } }
        } else if let data = board.data(forType: .png) ?? board.data(forType: .tiff), let picture = png(from: data) {
            attach(picture)
        } else if let text = board.string(forType: .string) {
            if !isEditingNote { focusNote() }
            note.text.insertText(text, replacementRange: note.text.selectedRange())
        }
    }

    private func png(from data: Data) -> Data? {
        if data.starts(with: [0x89, 0x50, 0x4E, 0x47]) { return data }
        guard let image = NSImage(data: data), let tiff = image.tiffRepresentation else { return nil }
        return NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:])
    }

    private func attach(_ picture: Data) {
        guard let ticket = active else { return }
        var shots = marks[index].shots ?? []
        guard shots.count < mostShots else {
            NSSound.beep()
            return
        }
        guard let file = keepShot(picture, ticket: ticket.id, item: index + 1) else {
            NSSound.beep()
            return
        }
        shots.append(file.path)
        marks[index].shots = shots
        saveAnswers()  // a picture is his answer too: a window started again finds it
        tellPhone()
        showShots()
        layout()
    }

    /// The picture under a name no other file has: the number of this window's shots begins at nought again with
    /// every window, and a restarted one wrote its first picture over one of the last window's (the audit of
    /// 2026-10-16, A08). Written aside first, then linked in, which fails rather than replaces.
    private func keepShot(_ picture: Data, ticket: String, item: Int) -> URL? {
        try? FileManager.default.createDirectory(at: shotsDirectory, withIntermediateDirectories: true)
        let aside = shotsDirectory.appendingPathComponent(".\(UUID().uuidString).png")
        guard (try? picture.write(to: aside, options: .atomic)) != nil else { return nil }
        defer { try? FileManager.default.removeItem(at: aside) }
        for number in (shotNumber + 1)...(shotNumber + 1000) {
            let file = shotsDirectory.appendingPathComponent("\(ticket)-\(item)-\(number).png")
            if link(aside.path, file.path) == 0 {
                shotNumber = number
                return file
            }
            if errno != EEXIST { return nil }
        }
        return nil
    }

    private func removeShot(_ number: Int) {
        guard var shots = marks[index].shots, shots.indices.contains(number) else { return }
        try? FileManager.default.removeItem(atPath: shots.remove(at: number))
        marks[index].shots = shots.isEmpty ? nil : shots
        saveAnswers()
        tellPhone()
        showShots()
        layout()
    }
}
