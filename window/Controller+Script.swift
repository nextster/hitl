// The scripted run, for the tool's own checks: the steps of a script, and `dump`, `snap` and `grips` to see the window by.

import AppKit

extension Controller {
    // MARK: Scripted runs
    //
    // For the tool's own checks: CONVEYOR_WINDOW_SCRIPT names a file of steps, one a
    // line. The window is never put on the screen and never takes the keyboard,
    // and what it would open is only written down.
    //   click TITLE a mouse click on the row or button with that title; «click TITLE › OPTION» on an option of a choice,
    //               «click Пункты › 3» on the third stroke over the check; where no button has that title, on the
    //               link with those words in a text of the check
    //   hover TITLE the pointer comes over it, or over a part of it, and stays
    //   focus       a click into the comment
    //   text TEXT   typed into the comment; \n is a line break
    //   paste PATH  ⌘V with that picture in the clipboard; paste-text TEXT the same with text.
    //               Both use a clipboard of their own, never the user's.
    //   keyboard    from here on the window counts as having the keyboard
    //   away        another app comes forward, as by ⌘Tab
    //   scroll N    the wheel turned over the item, N points of the screen further down it; less than nothing, back up
    //   wheel N     the wheel turned over the item's live page, N points further down: the page scrolls while it has
    //               room that way, and the item when it has none; each step is a turn of its own
    //   remote      the look a timer takes at what was pressed in the app on the phone, and the word left for it;
    //               «remote 1» takes only the next press
    //   arrive PATH a check written beforehand outside the queue goes into it, as one queued while he is at another;
    //               the window sees it at the next `reload`
    //   dump PATH   what is on the window, as JSON; with what its links would have opened and
    //               what a look at a picture would have shown
    //   snap-look PATH  a picture of a screen with the window and, over it, the card of the picture looked at
    //   snap PATH   a picture of the window's content

    private struct Dump: Codable {
        var id: String
        var index: Int
        var marks: [Mark]
        /// The items that hold something of his, in this check and in the ones he stepped away from.
        var holds: Int
        var origin: String
        /// The app the check came from, as the mark before the project says it, and the word that stands there when
        /// this Mac has no such app to take an icon of.
        var app: String
        var appWord: String
        /// The project the check is about, as the chip at the start of the head says it.
        var project: String
        var kind: String
        var count: String
        var header: String
        var about: String
        var item: String
        /// The plate over an item asked again: its line, and what he had written.
        var before: [String]
        var facts: [String]
        /// The words that stand as links in the texts of the check, from the top: what the task is, the item, what
        /// stands under it, its facts.
        var textLinks: [String]
        /// The item's live page: its file, whether it is up, how tall and how wide it stands on the screen, and
        /// what the window says will go to the chat.
        var livePage: String
        var livePageReady: Bool
        var livePageTall: Double
        var sent: String
        var livePageWide: Double
        /// The lines to copy, the ones that say they have just been copied, and what the clipboard would hold, in order.
        var copies: [String]
        var copied: [String]
        var clipboard: [String]
        var openAddresses: [String]
        var links: [String]
        /// The same buttons, each with how it stands: a switch on or off, a choice with the option that holds.
        var said: [String]
        var answers: [String]
        /// The answers that are ticked, and what stands over them, where several may hold at once.
        var ticked: [String]
        var several: String
        var keys: [String]
        var queue: [String]
        var editing: Bool
        var note: String
        var noteHeight: Double
        var height: Double
        var hint: String
        var doneEnabled: Bool
        var opened: [String]
        var ran: [String]
        var browsed: [String]
        var hasKeyboard: Bool
        var keysLive: Bool
        var held: String
        var pictures: [String]
        var folded: Bool
        var questionSize: Double
        var number: String
        var full: Bool
        var scale: Double
        /// How many times the whole column has been laid out to draw the page.
        var layouts: Int
        /// How far the item's live page is scrolled inside itself, and how far it can be.
        var pageScroll: [Double]
        /// How wide the item's column is, in the window's points, how far the item is scrolled and how far it can be.
        var column: Double
        var scrolled: Double
        var scrollMost: Double
        var window: [Double]
        var beside: [String]
        var list: [String]
        var looking: Int
        var looked: [String]
        var level: Int
        var timesForward: Int
        var frame: [Double]
        var screen: [Double]
        var carried: Bool
        var grips: [String]
        var onPhone: Bool
        var from: String
        var trails: [String]
        /// The folder and the branch stand in the head, under the chat's name; folded into a line, they do not.
        var originShown: Bool
        /// The answers drawn without a ground of their own: «Не понял».
        var quietAnswers: [String]
        /// «Готово» is the white button: it can be pressed.
        var doneReady: Bool
    }

    func run(script path: String) {
        steps = ((try? String(contentsOfFile: path, encoding: .utf8)) ?? "").split(separator: "\n").map(String.init)
        start(tries: 40)
    }

    /// The steps begin once the keyboard is back where it was: started from the app in front,
    /// this one is brought forward for a moment.
    private func start(tries: Int) {
        DispatchQueue.main.asyncAfter(deadline: .now() + (tries == 40 ? 0.2 : 0.05)) { [weak self] in
            guard let self else { return }
            if NSApp.isActive, tries > 0 {
                self.stepBack()
                self.start(tries: tries - 1)
            } else {
                self.nextStep()
            }
        }
    }

    private func nextStep() {
        guard !steps.isEmpty else { exit(0) }
        let parts = steps.removeFirst().split(separator: " ", maxSplits: 1).map(String.init)
        let argument = parts.count > 1 ? parts[1] : ""
        let proceed = { [weak self] in
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { self?.nextStep() }
            return
        }
        // What looks at a live page waits for it to come up: a web view loads in a process of its own.
        switch parts.first ?? "" {
        case "names": return refreshNames(force: true, then: proceed)
        case "page":  // a script run on the item's page: what his mouse would do there
            return settled { [weak self] in
                guard let web = self?.livePage?.web else { return proceed() }
                web.evaluateJavaScript(argument) { _, _ in DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { proceed() } }
            }
        case "wheel":  // the page says where it stands after it scrolled, so the next step waits for that
            return settled { [weak self] in
                guard let self else { return }
                let down = CGFloat(Double(argument) ?? 0)
                guard let page = self.livePage else { return proceed() }
                if self.wheelStaysOnPage(by: -down, page: page, begins: true) {
                    page.web.evaluateJavaScript("window.scrollBy(0, \(Double(down)))") { _, _ in
                        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { proceed() }
                    }
                } else {
                    _ = self.turn(-down)
                    proceed()
                }
            }
        case "dump":
            return settled { [weak self] in
                self?.dump(to: argument)
                proceed()
            }
        case "snap", "grips":
            let grips = parts.first == "grips"
            return settled { [weak self] in
                guard let self else { return }
                self.snapshot(to: argument, grips: grips, then: proceed)
            }
        default: break
        }
        switch parts.first ?? "" {
        case "key", "down", "up", "flag":
            fputs("hitl: synthetic keyboard input has been removed\n", stderr)
            exit(64)
        case "focus": focusNote()
        case "click": click(argument)
        case "hover": hover(argument)
        case "text": note.text.insertText(argument.replacingOccurrences(of: "\\n", with: "\n"), replacementRange: note.text.selectedRange())
        case "paste", "paste-text":
            let board = NSPasteboard(name: NSPasteboard.Name("conveyor-window-script-\(getpid())"))
            board.clearContents()
            if parts.first == "paste-text" {
                board.setString(argument, forType: .string)
            } else if let data = try? Data(contentsOf: URL(fileURLWithPath: argument)) {
                board.setData(data, forType: .png)
            }
            paste(from: board)
            board.releaseGlobally()
        case "keyboard":  // here, or «keyboard off»: gone elsewhere
            scriptedKeyboard = argument != "off"
            keyboardMoved()
        case "press": spot(argument).map { pressed(at: $0) }  // the mouse goes down on a part of the window
        case "lift": lifted()
        case "pointer":
            let at = argument.split(separator: " ").compactMap { Double($0) }
            if at.count == 2 {
                scriptedPointer = NSPoint(x: at[0], y: at[1])
                follow()
            }
        case "away":
            closeLook()
            stepAside()
        case "scroll": _ = turn(-CGFloat(Double(argument) ?? 0))
        case "snap-look": snapshotLook(to: argument)
        case "reload": reload()  // the look at the queue that a timer takes every second
        case "arrive":
            let file = URL(fileURLWithPath: argument)
            try? FileManager.default.moveItem(at: file, to: queueDirectory.appendingPathComponent(file.lastPathComponent))
        case "remote":  // what was pressed on the phone is taken in, «remote 1» only the next press, and what is on the window is said for it
            hearRemote(most: Int(argument))
            tellRemote()
        case "phone-ran":  // the phone's word of the button pressed here last: it ran it, or «phone-ran why it could not»
            if let run = phoneRun {
                phoneRan = PhoneSide.Ran(at: run.at, failure: argument.isEmpty ? nil : argument)
                showPhoneActions()
            }
        default: break
        }
        proceed()
    }

    /// Goes on once the item's page, when it has one, is up and has said its first word; at once without a page.
    /// A page that never comes up does not hold the run for good.
    private func settled(_ then: @escaping () -> Void) {
        guard let page = livePage, !page.isReady else { return then() }
        var gone = false
        let once = {
            guard !gone else { return }
            gone = true
            self.layout()
            then()
        }
        page.ready(once)
        DispatchQueue.main.asyncAfter(deadline: .now() + 10, execute: once)
    }

    /// The row or button with that title and where the pointer meets it: its middle, or the middle of a part of it.
    private func aim(_ named: String) -> (tap: Tap, at: (NSEvent.EventType) -> NSEvent?)? {
        // «click Лента › Месяц» is a click on an option of a choice, not on the middle of its button.
        let parts = named.components(separatedBy: " › ")
        let title = parts[0]
        func find(in view: NSView) -> Tap? {
            if let tap = view as? Tap, tap.title == title, !tap.isHiddenOrHasHiddenAncestor { return tap }
            for child in view.subviews { if let found = find(in: child) { return found } }
            return nil
        }
        backdrop.layoutSubtreeIfNeeded()
        guard let tap = find(in: backdrop) else { return nil }
        var point = tap.convert(NSPoint(x: tap.bounds.midX, y: tap.bounds.midY), to: nil)
        if parts.count == 2 {
            guard let spot = tap.spot(of: parts[1]) else { return nil }
            point = spot
        }
        return (tap, { [panel] type in
            NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                               windowNumber: panel.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)
        })
    }

    /// The texts of the check that are in sight, from the top.
    private var readTexts: [ReadText] {
        [context.about.text, question.text, details.text, facts.text].filter { !$0.isHiddenOrHasHiddenAncestor }
    }

    private func click(_ named: String) {
        // Handed to the button itself: a window that is not on the screen gets no clicks. What the window
        // does on any click, ending the comment, is done first, as it would be.
        guard let (tap, event) = aim(named), let down = event(.leftMouseDown), let up = event(.leftMouseUp) else {
            return clickLink(named)  // no button is called so: the words of a link in a text
        }
        panel.mouseWentDown?(down)
        tap.mouseDown(with: down)
        tap.mouseUp(with: up)
    }

    /// The mouse goes down and comes up on the words of a link, and the text itself makes of it what it makes of
    /// his click: the button coming up waits in the queue of events, where a text that follows the mouse looks for it.
    private func clickLink(_ words: String) {
        backdrop.layoutSubtreeIfNeeded()
        for text in readTexts {
            guard let point = text.spot(ofLink: words) else { continue }
            func event(_ type: NSEvent.EventType) -> NSEvent? {
                NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                                   windowNumber: panel.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)
            }
            guard let down = event(.leftMouseDown), let up = event(.leftMouseUp) else { return }
            NSApp.postEvent(up, atStart: true)
            text.mouseDown(with: down)
            return
        }
    }

    private func hover(_ named: String) {
        guard let (tap, event) = aim(named), let moved = event(.mouseMoved) else { return }
        tap.mouseEntered(with: moved)
        tap.mouseMoved(with: moved)
    }

    /// The middle of a part of the window, by name and in the window's own coordinates: where a scripted run presses.
    private func spot(_ name: String) -> NSPoint? {
        guard let content = panel.contentView else { return nil }
        // The window's own edge, beside everything it holds.
        if name == "edge" { return content.convert(NSPoint(x: 4, y: content.bounds.midY), to: nil) }
        let parts: [String: NSView?] = ["chat": who, "segments": segments, "task": context.about, "item": question, "comment": note,
                                        "answer": answerRows.first]
        guard let part = parts[name] ?? nil, !part.isHiddenOrHasHiddenAncestor else { return nil }
        return part.convert(NSPoint(x: part.bounds.midX, y: part.bounds.midY), to: nil)
    }

    /// What a press of the mouse takes the window along by, by name: a scripted run's way to see it.
    private var grips: [String] {
        ["chat", "segments", "task", "item", "comment", "answer", "edge"].filter { spot($0).map { takesAlong(at: $0) } ?? false }
    }

    private func dump(to path: String) {
        backdrop.layoutSubtreeIfNeeded()

        let state = Dump(id: active?.id ?? "", index: index, marks: marks, holds: holds, origin: who.stringValue, app: app.agent, appWord: app.word, project: project.isHidden ? "" : project.text, kind: kindLabel.stringValue,
                         count: count.stringValue, header: context.title.stringValue, about: context.about.stringValue,
                         item: question.stringValue, before: shown[ObjectIdentifier(before)] == true ? [before.said.caption, before.said.note] : [],
                         facts: shown[ObjectIdentifier(facts)] == true ? facts.facts : [],
                         textLinks: readTexts.flatMap(\.linkWords),
                         livePage: livePage.map { ($0.path as NSString).lastPathComponent } ?? "", livePageReady: livePage?.isReady ?? false,
                         livePageTall: Double(livePage?.superview == nil ? 0 : livePage?.frame.height ?? 0),
                         sent: sent.isHidden ? "" : sent.stringValue, livePageWide: Double(livePage?.superview == nil ? 0 : livePage?.frame.width ?? 0),
                         copies: copyTaps.map(\.title), copied: copyTaps.filter(\.copied).map(\.title), clipboard: copiedSaid,
                         openAddresses: opens, links: linkTaps.map(\.title), said: linkTaps.map(\.said), answers: answerRows.map(\.title),
                         ticked: answerRows.filter { $0.box == true }.map(\.title), several: several.isHidden ? "" : several.stringValue,
                         keys: (picturesBeside ? gallery : pictures).map(\.key) + copyTaps.map(\.key) + linkTaps.map(\.key) + textKeys.map(\.key) + answerRows.map(\.key), queue: queueTaps.map(\.title),
                         editing: isEditingNote,
                         note: note.string, noteHeight: Double(note.frame.height),
                         height: Double(root.fittingSize.height + (folded ? 0 : barHeight)),
                         hint: hint.stringValue, doneEnabled: done.isEnabled, opened: opened, ran: ran, browsed: browsed,
                         hasKeyboard: panel.isKeyWindow || NSApp.isActive, keysLive: hasKeyboard, held: held?.tap()?.title ?? "",
                         pictures: pictures.map(\.title), folded: folded,
                         questionSize: Double(question.font.pointSize), number: number.stringValue, full: full,
                         scale: Double(scale), layouts: columnLayouts, pageScroll: [livePage?.inside.at ?? 0, livePage?.inside.most ?? 0], column: Double(columnWide), scrolled: Double(scrolled), scrollMost: Double(scrollMost),
                         window: [Double(panel.frame.width), Double(panel.frame.height)],
                         beside: gallery.map(\.title), list: full ? entries.map(\.title) : [], looking: lookingAt ?? -1, looked: lookedAt,
                         level: panel.level.rawValue, timesForward: timesForward,
                         frame: [panel.frame.minX, panel.frame.minY, panel.frame.width, panel.frame.height].map { Double($0) },
                         screen: [stoodIn.minX, stoodIn.minY, stoodIn.width, stoodIn.height].map { Double($0) },
                         carried: carry != nil, grips: grips, onPhone: active?.isOnPhone == true,
                         from: origin.from, trails: linkTaps.map(\.trail), originShown: !origin.isHiddenOrHasHiddenAncestor,
                         quietAnswers: answerRows.filter(\.quiet).map(\.title), doneReady: done.primary && done.isEnabled)
        try? JSONEncoder().encode(state).write(to: URL(fileURLWithPath: path))
    }

    /// `grips`: the same picture, with what the mouse takes the window by tinted.
    private func snapshot(to path: String, grips: Bool = false, then: @escaping () -> Void) {
        guard let view = panel.contentView else { return then() }
        let scale: CGFloat = 2
        let size = view.bounds.size
        guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size.width * scale), pixelsHigh: Int(size.height * scale),
                                         bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                         colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
              let context = { rep.size = size; return NSGraphicsContext(bitmapImageRep: rep) }() else { return then() }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        view.effectiveAppearance.performAsCurrentDrawingAppearance {
            view.displayIgnoringOpacity(view.bounds, in: context)
        }
        if grips {
            NSColor.systemGreen.withAlphaComponent(0.3).setFill()
            let step: CGFloat = 2
            for x in stride(from: CGFloat(0), to: size.width, by: step) {
                for y in stride(from: CGFloat(0), to: size.height, by: step) {
                    let point = view.convert(NSPoint(x: x + step / 2, y: y + step / 2), to: nil)
                    if takesAlong(at: point) { NSRect(x: x, y: y, width: step, height: step).fill(using: .sourceOver) }
                }
            }
        }
        NSGraphicsContext.restoreGraphicsState()
        let write = {
            try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
            then()
        }
        // A web view draws in a process of its own: what the page shows is put into the picture apart.
        guard let page = livePage, page.isReady, page.superview != nil, !page.isHiddenOrHasHiddenAncestor else { return write() }
        let place = view.convert(page.bounds, from: page)
        let sight = view.convert(pageSight.bounds, from: pageSight)
        page.picture { image in
            if let image {
                NSGraphicsContext.saveGraphicsState()
                NSGraphicsContext.current = context
                NSBezierPath(rect: sight).addClip()  // of a page in an item that is scrolled, what is in sight
                NSBezierPath(roundedRect: place.insetBy(dx: 1, dy: 1), xRadius: 7, yRadius: 7).addClip()
                image.draw(in: place)
                NSGraphicsContext.restoreGraphicsState()
            }
            write()
        }
    }
}
