// The one object behind the window: what it keeps, how it starts and builds its views; what it does is in the files beside.

import AppKit

final class Controller: NSObject {
    let panel: KeyPanel
    /// What the window's content lies on: the panel, drawn larger on the page.
    let stage = NSView()
    let zoom = NSView()
    let backdrop = Backdrop()
    /// What is read and answered: under the head and the progress, which on the page run across it.
    let column = NSStackView()
    /// The column stands in this: as much of an item is seen through it as the screen has room for, and the rest
    /// is scrolled to. How tall it is, where the column's top stands in it, how far the item is scrolled and can
    /// be, and which item that is: another one begins at its top.
    let box = Scroll()
    var boxTall: NSLayoutConstraint!
    var scrollTop: NSLayoutConstraint!
    var scrolled: CGFloat = 0
    var scrollMost: CGFloat = 0
    var scrolledItem = ""
    /// What in the column is as wide as the column by a rule of its own.
    var columnWides: [NSLayoutConstraint] = []
    /// Where the item's live page is seen: the column's room, as it lies on the window.
    let pageSight = PageSight()
    /// The page's list, at its left.
    let list = NSView()
    let listStack = NSStackView()
    let wide = Glyph("На весь экран")
    let root = NSStackView()
    /// The app the check came from, before the project.
    let app = AppMark()
    /// The project the check is about, before the chat's name.
    let project = Chip()
    let who = NSTextField(labelWithString: "")
    let chatNames = CheckNames()
    /// The folder and the branch the check comes from, under the chat's name.
    let origin = Origin()
    /// The kind of the item, at the right of the head.
    let kindLabel = Pill()
    /// The head: who asks and from where. The name stands after the icon, or at the start without one; under the
    /// name stands the folder, except in the folded line.
    let head = NSView()
    var nameAfterIcon: NSLayoutConstraint!
    var headWithOrigin: [NSLayoutConstraint] = []
    var headWithoutOrigin: [NSLayoutConstraint] = []
    let count = NSTextField(labelWithString: "")
    let segments = Segments()
    let context = Context()
    let number = NSTextField(labelWithString: "")
    let question = Words(Face.question, .white, room: contentWidth)
    let questionRow = NSStackView()
    let details = Words(Face.details, Ink.quiet, room: contentWidth)
    let facts = Facts()
    /// Over an item that is asked again: what he answered to it the last time.
    let before = BeforePlate()
    /// Where the item's live page stands in a small window, and under it what goes to the chat with the answer.
    let livePageHolder = NSView()
    let sent = NSTextField(wrappingLabelWithString: "")
    var livePageTall: NSLayoutConstraint!
    var livePage: LivePage?
    var livePages: [String: LivePage] = [:]
    /// On the whole screen: the item whose live page is on it, and the tallest that page has been since the
    /// item came there. The scale is counted by it. `said`: the page has said its height itself; until then it
    /// is the window's guess.
    var livePageRoom: (item: String, tall: CGFloat, said: Bool)?
    let fold = Glyph("Свернуть")
    let picturesGrid = NSStackView()
    /// The lines of the item he copies with a click, under its pictures.
    let copiesColumn = NSStackView()
    let linksHolder = NSView()
    let linksColumn = NSStackView()
    let answersColumn = NSStackView()
    /// Over the answers of an item where several options may hold at once: that they may, and how to go on.
    let several = NSTextField(labelWithString: "Можно несколько · дальше →")
    let note = NoteView(width: contentWidth)
    let shotsRow = NSStackView()
    let queueRow = NSStackView()
    let hint = NSTextField(labelWithString: "")
    let previous = Flat("‹")
    let following = Flat("›")
    let later = Flat("Отложить")
    let rework = Flat("Доработать")
    let done = Flat("Готово")
    var bar = NSStackView()
    var barTall: NSLayoutConstraint!
    var body: [NSView] = []
    var shown: [ObjectIdentifier: Bool] = [:]
    var linkTaps: [Flat] = []
    /// The keys of the links written into the texts of the check, from the top: after everything else the item
    /// has on ⌘ with a digit.
    var textKeys: [TextKey] = []
    var copyTaps: [CopyLine] = []
    /// The line copied last, by its check and item and by its place among the item's: it says so for a moment,
    /// in rows built anew meanwhile too.
    var copiedLine: (item: String, place: Int)?
    var copiedTimer: Timer?
    /// What a scripted run would have put into the clipboard: it never touches his.
    var copiedSaid: [String] = []
    var answerRows: [Row] = []
    var queueTaps: [Flat] = []
    var pictures: [Picture] = []
    /// Folded, the window is one line: who asks and how much waits.
    var folded = false
    /// On the whole screen the window is a page over everything. He turns it on; a new batch of checks comes small.
    var full = false
    /// How much larger than in a small window everything on the page is drawn.
    var scale: CGFloat = 1
    /// On the page, an item that does not fit: the step of the grid of scales it was drawn at last, where the next search begins.
    var scaleHint: Int?
    /// How many times the page has laid its column out, for the scripted runs to count.
    var columnLayouts = 0
    /// The turn of the wheel that is under way over a live page belongs to the page, not to the item.
    var wheelOnPage = false
    var entries: [Entry] = []
    var listKey: [String] = []
    /// The item's pictures, large at the right of the page.
    var gallery: [Picture] = []
    var galleryKey = ""
    /// The pictures looked at closer while the card with one is up, the item's or the screenshots of its comment,
    /// and which of them it shows.
    var looked: [String] = []
    var lookingAt: Int?
    /// The item whose pictures they are: he goes on to another one, and the card goes away.
    var lookedIn = ""
    /// What holds the card on the screen; made at the first look.
    var lookPanel: PreviewPanel?
    /// What the card would have shown, in a scripted run.
    var lookedAt: [String] = []
    var rootLeft: NSLayoutConstraint!
    var rootTop: NSLayoutConstraint!
    var rootWide: NSLayoutConstraint!
    var barLeft: NSLayoutConstraint!
    var headWide: NSLayoutConstraint!
    var segmentsWide: NSLayoutConstraint!
    /// What holds only in a small window: it is as tall as what it holds.
    var smallOnly: [NSLayoutConstraint] = []
    /// When this app gave the keyboard back to the one he was in: that app coming forward is not him leaving.
    var handedBackAt: Date?
    /// Where the small window's top right corner stands while the window is up: where he put it,
    /// or else the middle of the screen. The window grows down from it, so its top line stays still.
    var anchor: NSPoint?
    /// Where he moved the window to, by the mouse or with Shift held: «там, куда я его передвинул».
    /// Kept in a file, so the checks to come open there too.
    var put: NSPoint?
    /// The frame the window was last given here: found elsewhere, he has dragged it.
    var placed: NSRect?
    /// Whether the window is up. A scripted run never puts its window on the screen and still has to know.
    var isUp = false
    var journalOccluded: Bool?
    var journalLastCheck = ""
    /// The screen in whose middle the window came up.
    var stoodIn: NSRect = .zero
    var loaded: [String: NSImage] = [:]

    /// What he has answered in a check he stepped away from: it is there when he comes back.
    struct Kept: Codable {
        var marks: [Mark]
        var index: Int
        var shownAt: Double
    }

    /// His answers as they were last written down.
    var answersSaid: Data?

    var processingDrops = false
    var tickets: [Ticket] = []
    var active: Ticket?
    var index = 0
    var marks: [Mark] = []
    var shownAt: Double = 0
    var kept: [String: Kept] = [:]
    /// Checks of the phone that have been on this window: until then one is the next check, after it one waits in
    /// its place in the queue, and he may go to another check and is not pulled back.
    var broughtForward: Set<String> = []
    /// The check he was at in the window before this one: it comes up first, and a build on the phone waits for it.
    var cameBackTo: String?
    /// Checks of the phone he ended here: off this window while the queue's process takes them off the phone.
    var ending: Set<String> = []
    /// One item is the current one on both screens: he moves on either, and the later move is where both are.
    /// When he last moved, here or there, and the item this window has last said it is at.
    var currentAt: Double = 0
    var toldCurrent: String?
    /// The window is going to the item he came to on the phone: that is not a move of his own here.
    var followingPhone = false
    /// What the panel on the phone offers to do for each item, by the item's number: the same buttons are here.
    var phoneOffers: [String: [PhoneSide.Action]] = [:]
    /// The last of them pressed here, and what the phone has said of the last one it ran.
    var phoneRun: MacSide.Run?
    /// Which of the item's buttons that was, by its place among them.
    var phoneRunPlace: Int?
    var phoneRan: PhoneSide.Ran?
    /// Takes the phone's screen into the comment. One at a time: the phone answers in a second or two.
    let phoneShot = Flat("Снимок с телефона")
    var takingShot = false
    /// How long the phone is given to say what is on its screen.
    let phoneShotSeconds: Double = 20
    /// The app on the phone shows this window's queue: what the window last said of itself for it, and how
    /// many times it has said something new.
    var remoteSaid: Data?
    var remoteSeq = 0
    /// A press that came from the phone is being done: the keyboard stays where it is on the Mac.
    var fromPhone = false
    /// What he set on an item's page on the phone, and what the same page here last said: of the two the one
    /// he turned last goes to the chat.
    var phoneValues: [String: [[String]]] = [:]
    var macValues: [String: [[String]]] = [:]
    var emptySince: Date?
    var shotNumber = 0
    /// The verdict of the check on the window could not be written: said where the kind of the item stands.
    var savingTrouble: [String: String] = [:]
    /// Checks a press kept on the phone was not let into, for an item he answered here: such a check is not
    /// ended by a press that was kept with it.
    var lateRefused: Set<String> = []
    var quietUntil: Date?
    var waitingToOpen: URL?
    /// How many times this process was made the active app: none, in a scripted run.
    var timesForward = 0
    /// The app he was in before this one was brought forward: the keyboard goes back to it.
    var appBefore = NSWorkspace.shared.frontmostApplication
    var opened: [String] = []
    /// The commands the «Чат» button's setting ran for those, joined by spaces: a scripted window runs none.
    var ran: [String] = []
    /// Of those, the ones handed to the web browser by name: the addresses of sites.
    var browsed: [String] = []
    var steps: [String] = []
    var scriptedKeyboard = false

    /// The timer that writes his place for the window down once a move has come to rest.
    var savingPlace: Timer?
    /// The move of the window that is going on, by Shift or by the mouse, and the timer that follows the pointer.
    var carry: Carry?
    var carrying: Timer?
    /// Where a scripted run says the pointer is, and which modifiers it holds down.
    var scriptedPointer: NSPoint?
    /// The key that is down, if one is.
    var held: Held?

    override init() {
        panel = KeyPanel(contentRect: NSRect(x: 0, y: 0, width: panelWidth, height: 240),
                         styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        super.init()
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .floating
        panel.isFloatingPanel = true
        panel.hidesOnDeactivate = false
        panel.becomesKeyOnlyIfNeeded = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        // The window is moved by its own code. Left to the system as «movable by its background» it did not
        // move in his hands at all, whatever each view answered about itself.
        panel.isMovableByWindowBackground = false
        panel.isReleasedWhenClosed = false
        panel.appearance = NSAppearance(named: .darkAqua)
        panel.mouseWentDown = { [weak self] event in
            guard let self else { return }
            // A click beside the comment ends writing it.
            if self.isEditingNote, !self.note.bounds.contains(self.note.convert(event.locationInWindow, from: nil)) { self.leaveNote() }
            self.pressed(at: event.locationInWindow)
        }
        panel.mouseCameUp = { [weak self] in self?.lifted() }
        build()
        NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, self.panel.isKeyWindow else { return event }
            return self.handle(event) ? nil : event
        }
        NSEvent.addLocalMonitorForEvents(matching: [.keyUp, .flagsChanged]) { [weak self] event in
            self?.released(event)
            if event.type == .flagsChanged { self?.modifiersChanged(event) }
            return event
        }
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification] {
            NotificationCenter.default.addObserver(self, selector: #selector(keyboardMoved), name: name, object: panel)
        }
        NotificationCenter.default.addObserver(self, selector: #selector(windowMoved), name: NSWindow.didMoveNotification, object: panel)
        if let place = try? JSONDecoder().decode(Place.self, from: Data(contentsOf: placeFile)), place.corner.count == 2 {
            put = NSPoint(x: place.corner[0], y: place.corner[1])
        }
        NotificationCenter.default.addObserver(self, selector: #selector(cameForward), name: NSApplication.didBecomeActiveNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(journalVisibility), name: NSWindow.didChangeOcclusionStateNotification, object: panel)
        NotificationCenter.default.addObserver(self, selector: #selector(journalClose), name: NSWindow.willCloseNotification, object: panel)
        NotificationCenter.default.addObserver(self, selector: #selector(journalTermination), name: NSApplication.willTerminateNotification, object: nil)
        NSWorkspace.shared.notificationCenter.addObserver(self, selector: #selector(otherCameForward(_:)),
                                                          name: NSWorkspace.didActivateApplicationNotification, object: nil)
        Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in self?.reload() }
        // What he does on the phone is a small file here, written the moment the app says it: looked at thirty
        // times a second it is in the window before he looks up from the phone. A scripted run looks when its
        // script says so.
        if scriptFile == nil {
            Timer.scheduledTimer(withTimeInterval: 1.0 / 30, repeats: true) { [weak self] _ in self?.hearPhone() }
            // The same for the app on the phone that shows this queue: what he presses there is pressed here,
            // and what is here is said for it.
            Timer.scheduledTimer(withTimeInterval: 1.0 / 20, repeats: true) { [weak self] _ in
                self?.hearRemote()
                self?.tellRemote()
            }
        }
        // What was pressed before this window came up is not pressed in it. A scripted run has its presses
        // laid out beforehand, and takes them when its script says so.
        if scriptFile == nil { forgetRemote() }
        // What he had answered in the window before this one is here again, at the check and the item he was at.
        cameBackTo = recallAnswers()
        reload()
        if let scriptFile { run(script: scriptFile) }
    }

    @objc private func journalVisibility() {
        let visible = panel.occlusionState.contains(.visible)
        guard journalOccluded != !visible else { return }
        journalOccluded = !visible
        ActionJournal.shared.record("hitl.window.occlusion", request: active?.id ?? "",
                                    outcome: visible ? "visible" : "occluded", reason: "appkit", windowNumber: panel.windowNumber)
    }

    @objc private func journalTermination() {
        ActionJournal.shared.record("hitl.panel.exit", request: active?.id ?? journalLastCheck,
                                    outcome: "exiting", reason: "appkit_terminate", windowNumber: panel.windowNumber, marked: holds, waiting: tickets.count)
    }

    @objc private func journalClose() {
        ActionJournal.shared.record("hitl.window.closed", request: active?.id ?? journalLastCheck,
                                    outcome: "closed", reason: "appkit_close", windowNumber: panel.windowNumber, marked: holds, waiting: tickets.count)
    }

    private func build() {
        root.orientation = .vertical
        root.alignment = .leading
        root.spacing = 0
        root.edgeInsets = NSEdgeInsets(top: 12, left: side, bottom: 12, right: side)
        root.translatesAutoresizingMaskIntoConstraints = false

        // The head says who asks and from where («Шапка — кто спрашивает и откуда», 2026-10-20): the app's icon
        // beside two lines, the project and the chat, and under them the folder and the branch; at the right the
        // kind of the item, where he is in the check, and the window's two buttons.
        who.font = Face.chat
        who.textColor = Ink.heading
        who.lineBreakMode = .byTruncatingTail
        who.setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)
        who.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        count.font = Face.digits(12)
        count.textColor = Ink.soft
        count.setContentHuggingPriority(.required, for: .horizontal)
        count.setContentCompressionResistancePriority(.required, for: .horizontal)
        fold.press = { [weak self] in self?.toggleFold() }
        wide.symbol = "arrow.up.left.and.arrow.down.right"
        wide.press = { [weak self] in self?.toggleFull() }
        wide.toolTip = "На весь экран, ⌃⌘F"
        fold.toolTip = "Свернуть в строку, ⌘M"
        let named = NSStackView(views: [project, who])
        named.orientation = .horizontal
        named.alignment = .centerY
        named.spacing = 8
        let corner = NSStackView(views: [kindLabel, count, wide, fold])
        corner.orientation = .horizontal
        corner.alignment = .centerY
        corner.spacing = 6
        corner.setCustomSpacing(2, after: wide)
        for view in [app, named, origin, corner] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            head.addSubview(view)
        }
        head.translatesAutoresizingMaskIntoConstraints = false
        nameAfterIcon = named.leadingAnchor.constraint(equalTo: app.trailingAnchor, constant: 11)
        headWithOrigin = [origin.topAnchor.constraint(equalTo: named.bottomAnchor, constant: 1), head.bottomAnchor.constraint(equalTo: origin.bottomAnchor)]
        headWithoutOrigin = [head.bottomAnchor.constraint(equalTo: named.bottomAnchor)]
        NSLayoutConstraint.activate([
            app.leadingAnchor.constraint(equalTo: head.leadingAnchor), app.centerYAnchor.constraint(equalTo: head.centerYAnchor),
            head.topAnchor.constraint(lessThanOrEqualTo: app.topAnchor), head.bottomAnchor.constraint(greaterThanOrEqualTo: app.bottomAnchor),
            nameAfterIcon, named.topAnchor.constraint(equalTo: head.topAnchor),
            named.trailingAnchor.constraint(lessThanOrEqualTo: corner.leadingAnchor, constant: -8),
            origin.leadingAnchor.constraint(equalTo: named.leadingAnchor), origin.trailingAnchor.constraint(lessThanOrEqualTo: head.trailingAnchor, constant: -4),
            corner.trailingAnchor.constraint(equalTo: head.trailingAnchor, constant: 2), corner.centerYAnchor.constraint(equalTo: named.centerYAnchor),
        ] + headWithOrigin)
        headWide = head.widthAnchor.constraint(equalToConstant: contentWidth)
        segmentsWide = segments.widthAnchor.constraint(equalToConstant: contentWidth)
        segments.pick = { [weak self] number in self?.go(to: number) }
        NSLayoutConstraint.activate([headWide, segmentsWide])

        number.font = Face.of(Face.questionSize)
        number.textColor = Ink.faint
        number.setContentHuggingPriority(.required, for: .horizontal)
        number.setContentCompressionResistancePriority(.required, for: .horizontal)
        questionRow.orientation = .horizontal
        questionRow.alignment = .firstBaseline
        questionRow.spacing = 6
        questionRow.addArrangedSubview(number)
        questionRow.addArrangedSubview(question)
        // A link written into a text of the check opens at a click, as the item's own links do.
        for text in [question, details, context.about] { text.open = { [weak self] link in self?.open(address: link) } }
        facts.open = { [weak self] link in self?.open(address: link) }

        for column in [picturesGrid, copiesColumn, linksColumn, answersColumn] {
            column.orientation = .vertical
            column.alignment = .leading
            column.spacing = 2
        }
        picturesGrid.spacing = 8
        copiesColumn.spacing = 6
        linksColumn.spacing = 0
        // The links stand with their text at the edge of the text above; the room for the pointer's highlight hangs out.
        linksColumn.translatesAutoresizingMaskIntoConstraints = false
        linksHolder.translatesAutoresizingMaskIntoConstraints = false
        linksHolder.addSubview(linksColumn)
        NSLayoutConstraint.activate([
            linksColumn.leadingAnchor.constraint(equalTo: linksHolder.leadingAnchor, constant: -8),
            linksColumn.topAnchor.constraint(equalTo: linksHolder.topAnchor), linksColumn.bottomAnchor.constraint(equalTo: linksHolder.bottomAnchor),
        ])

        note.text.placeholder = "Комментарий"
        note.changed = { [weak self] in self?.noteChanged() }
        note.text.focusChanged = { [weak self] in DispatchQueue.main.async { self?.keyboardMoved() } }
        shotsRow.orientation = .horizontal
        shotsRow.spacing = 8

        queueRow.orientation = .horizontal
        queueRow.alignment = .centerY
        queueRow.spacing = 0
        hint.font = Face.hint
        hint.textColor = Ink.faint

        sent.maximumNumberOfLines = 2
        sent.cell?.truncatesLastVisibleLine = true
        sent.preferredMaxLayoutWidth = contentWidth
        sent.isSelectable = true
        sent.translatesAutoresizingMaskIntoConstraints = false
        livePageHolder.translatesAutoresizingMaskIntoConstraints = false
        livePageTall = livePageHolder.heightAnchor.constraint(equalToConstant: 160)
        columnWides = [linksHolder, livePageHolder, sent].map { $0.widthAnchor.constraint(equalToConstant: contentWidth) }
        NSLayoutConstraint.activate(columnWides + [livePageTall])

        body = [segments, context, before, questionRow, details, facts, livePageHolder, sent, picturesGrid, copiesColumn, linksHolder, several, answersColumn, note, shotsRow, queueRow, hint]
        column.orientation = .vertical
        column.alignment = .leading
        column.spacing = 0
        for view in body.dropFirst() { column.addArrangedSubview(view) }
        column.translatesAutoresizingMaskIntoConstraints = false
        box.translatesAutoresizingMaskIntoConstraints = false
        box.clipsToBounds = true
        box.addSubview(column)
        box.turned = { [weak self] by in self?.turn(by) ?? false }
        backdrop.turned = { [weak self] by in self?.turn(by) ?? false }
        scrollTop = column.topAnchor.constraint(equalTo: box.topAnchor)
        boxTall = box.heightAnchor.constraint(equalToConstant: 100)
        NSLayoutConstraint.activate([scrollTop, boxTall, column.leadingAnchor.constraint(equalTo: box.leadingAnchor),
                                     box.widthAnchor.constraint(equalTo: column.widthAnchor)])
        for view in [head, segments, box] { root.addArrangedSubview(view) }
        // Under the head, a hairline across the window: who asks above it, what is asked under it.
        root.setCustomSpacing(Controller.headRule * 2 + 1, after: head)
        root.setCustomSpacing(14, after: segments)
        column.setCustomSpacing(16, after: context)
        column.setCustomSpacing(12, after: before)
        column.setCustomSpacing(7, after: questionRow)
        column.setCustomSpacing(12, after: details)
        column.setCustomSpacing(12, after: facts)
        column.setCustomSpacing(5, after: livePageHolder)
        column.setCustomSpacing(8, after: sent)
        column.setCustomSpacing(8, after: picturesGrid)
        column.setCustomSpacing(6, after: copiesColumn)
        column.setCustomSpacing(12, after: linksHolder)
        column.setCustomSpacing(4, after: several)
        several.font = Face.hint
        several.textColor = Ink.faint
        column.setCustomSpacing(12, after: answersColumn)
        column.setCustomSpacing(12, after: note)
        column.setCustomSpacing(8, after: shotsRow)
        column.setCustomSpacing(6, after: queueRow)

        previous.size = 17
        following.size = 17
        rework.key = "⇧⌘↵"
        done.key = "⌘↵"
        done.primary = true
        previous.press = { [weak self] in self?.step(-1) }
        following.press = { [weak self] in self?.step(1) }
        later.press = { [weak self] in self?.postpone() }
        rework.press = { [weak self] in self?.finish(how: "rework") }
        done.press = { [weak self] in self?.finish(how: "done") }
        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        bar = NSStackView(views: [previous, following, spacer, later, rework, done])
        bar.orientation = .horizontal
        bar.alignment = .centerY
        bar.spacing = 2
        bar.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 0, right: 8)
        bar.translatesAutoresizingMaskIntoConstraints = false

        backdrop.addSubview(root)
        backdrop.addSubview(bar)
        rootLeft = root.leadingAnchor.constraint(equalTo: backdrop.leadingAnchor)
        rootTop = root.topAnchor.constraint(equalTo: backdrop.topAnchor)
        rootWide = root.widthAnchor.constraint(equalToConstant: panelWidth)
        barLeft = bar.leadingAnchor.constraint(equalTo: backdrop.leadingAnchor)
        smallOnly = [root.trailingAnchor.constraint(equalTo: backdrop.trailingAnchor), root.bottomAnchor.constraint(equalTo: bar.topAnchor)]
        NSLayoutConstraint.activate([rootLeft, rootTop, rootWide, barLeft, bar.trailingAnchor.constraint(equalTo: backdrop.trailingAnchor),
                                     bar.bottomAnchor.constraint(equalTo: backdrop.bottomAnchor)] + smallOnly)
        barTall = bar.heightAnchor.constraint(equalToConstant: barHeight)
        barTall.isActive = true

        listStack.orientation = .vertical
        listStack.alignment = .leading
        listStack.spacing = 1
        listStack.translatesAutoresizingMaskIntoConstraints = false
        list.addSubview(listStack)
        NSLayoutConstraint.activate([
            listStack.topAnchor.constraint(equalTo: list.topAnchor, constant: 10), listStack.leadingAnchor.constraint(equalTo: list.leadingAnchor, constant: 8),
            listStack.widthAnchor.constraint(equalToConstant: Entry.width),
        ])
        list.isHidden = true
        backdrop.addSubview(list)
        zoom.addSubview(backdrop)
        stage.addSubview(zoom)
        pageSight.clipsToBounds = true
        stage.addSubview(pageSight)
        panel.contentView = stage
    }
}
