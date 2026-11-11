// The window's shapes and their layout: folded into a line, small, or the page over the whole screen with its list and pictures.

import AppKit

extension Controller {
    /// The tiny icon in the corner: the window becomes one line, and back.
    func toggleFold() {
        guard active != nil else { return }
        leaveNote()
        closeLook()
        folded.toggle()
        if folded, full { leaveFull() }
        refreshChrome()
        layout()
    }

    /// The other icon: the window becomes a page over the whole screen, and back. There is no way to put
    /// the window away altogether: at most it is folded into a line.
    func toggleFull() {
        guard active != nil else { return }
        if full {
            leaveFull()
        } else {
            windowMoved()
            setDown()
            full = true
            folded = false
        }
        refreshChrome()
        layout()
        // He changed the shape himself, by its key or a click on the window: the keys are the window's, on the
        // page, where nothing else is on the screen, and back in the small window.
        if scriptFile == nil { panel.makeKey() }
    }

    /// The page becomes the small window again, laid out afresh where it stood. The window stays on the screen
    /// meanwhile: put away and brought up again it came up without the keyboard, and his next key went to the
    /// app under it («переключение между полноэкранным режимом и оверлеем убирает фокус с конвейера»).
    private func leaveFull() {
        full = false
        setDown()
        placed = nil  // the page's frame is not a place he dragged the window to
    }

    func hide() {
        if isUp {
            ActionJournal.shared.record("hitl.window.hide", request: active?.id ?? journalLastCheck, outcome: "hide_requested", reason: "queue_empty", windowNumber: panel.windowNumber, marked: holds, waiting: tickets.count)
        }
        setDown()
        panel.orderOut(nil)
        // A window put away has no keyboard. A scripted run only counts as having it, and here stops counting.
        if scriptedKeyboard {
            scriptedKeyboard = false
            keyboardMoved()
        }
        isUp = false
        placed = nil
    }

    /// Another app came forward while the page covered the screen: by ⌘Tab, or as what a link opened in.
    /// The page would hide it, so the window is small again. It is still there: it cannot be put away.
    func stepAside() {
        guard full, active != nil else { return }
        leaveFull()
        refreshChrome()
        layout()
    }

    /// A click on an item in the page's list, or on its stroke over the check. What he has answered stays as it is.
    func go(to number: Int) {
        guard let ticket = active, ticket.items.indices.contains(number) else { return }
        leaveNote()
        index = number
        show()
    }

    /// The screen the window counts by: all of it, the part of it free of the menu bar and the Dock, and the strip
    /// of the camera's notch. A scripted run may name a screen of its own.
    struct Sight {
        let whole: NSRect
        let free: NSRect
        let notch: CGFloat
    }

    func sight(_ screen: NSScreen?) -> Sight? {
        if let size = screenStandIn {
            let all = NSRect(origin: .zero, size: size)
            return Sight(whole: all, free: all, notch: 0)
        }
        return screen.map { Sight(whole: $0.frame, free: $0.visibleFrame, notch: $0.safeAreaInsets.top) }
    }

    /// On the whole screen an item is drawn no smaller than this part of its size in the small window; what does
    /// not fit then is scrolled to. It went down to a half, and the letters with it.
    static let smallest: CGFloat = 0.85
    /// However low the screen, this much of an item is seen at once.
    static let leastSeen: CGFloat = 120
    /// The air above the item's column on the page, and the least it is taken down to for an item that does not fit.
    static let air: CGFloat = 36
    static let leastAir: CGFloat = 8
    /// The scales an item that does not fit is drawn at stand on a grid of this step.
    static let grain: CGFloat = 0.005

    func layout() {
        let wasVisible = isUp
        if full, let screen = sight(panel.screen ?? NSScreen.main) {
            layoutPage(on: screen)
        } else {
            livePageRoom = nil
            widen(to: contentWidth)
            shape(page: nil, top: 0, beside: false)
            // Measured as it will be drawn, one to one: under the scale the page had left, the window on its way
            // back from the page came out a point or two taller than what it holds.
            zoom.bounds = NSRect(origin: .zero, size: zoom.frame.size)
            // The window is as tall as what it holds, and no taller than its screen has room for: an item taller
            // than that is seen through as much room as there is, and scrolled.
            boxTall.constant = folded ? 0 : column.fittingSize.height
            var tall = root.fittingSize.height + (folded ? 0 : barHeight)
            let stands = (anchor ?? putInSight).flatMap { corner in
                NSScreen.screens.first { $0.frame.contains(NSPoint(x: corner.x - panelWidth / 2, y: corner.y - 1)) }
            } ?? NSScreen.main
            if !folded, let room = sight(stands)?.free.height, tall > room - 8 {
                boxTall.constant = max(Controller.leastSeen, boxTall.constant - (tall - (room - 8)))
                tall = root.fittingSize.height + barHeight
            }
            settleScroll()
            let size = NSSize(width: panelWidth, height: tall)
            scale = 1
            windowMoved()
            let corner = anchor ?? putInSight ?? middle(for: size)
            anchor = corner
            let frame = fitted(NSRect(x: corner.x - size.width, y: corner.y - size.height, width: size.width, height: size.height))
            placed = frame
            panel.setFrame(frame, display: false)
            zoom.frame = NSRect(origin: .zero, size: frame.size)
            zoom.bounds = zoom.frame
            backdrop.frame = zoom.bounds
            backdrop.layoutSubtreeIfNeeded()
            markRule()
            markWay()
            place(pictures: [], page: .zero, top: 0)
        }
        // The comment he writes is in sight, though it stands under what the window has room for.
        if isEditingNote { reveal(hint.isHidden ? note : hint) }
        placeLivePage()
        backdrop.needsDisplay = true
        panel.display()
        panel.invalidateShadow()
        // A scripted run never puts its window on the screen. A process that shows a window, started from
        // the app in front, is made the active app by macOS: while the checks ran, his typing went astray.
        if !wasVisible {
            ActionJournal.shared.record("hitl.window.show", request: active?.id ?? journalLastCheck, outcome: scriptFile == nil ? "show_requested" : "offscreen", reason: "active_check", windowNumber: panel.windowNumber, marked: holds, waiting: tickets.count)
            isUp = true
            if scriptFile == nil { panel.orderFrontRegardless() }
        }
    }

    /// The window's two shapes: a small panel as tall as what it holds, or the page.
    private func shape(page: NSSize?, top: CGFloat, beside: Bool, air: CGFloat = 36) {
        let pane = page.map { $0.width - listWidth }
        if page != nil { NSLayoutConstraint.deactivate(smallOnly) }
        rootLeft.constant = page == nil ? 0 : listWidth
        barLeft.constant = rootLeft.constant
        rootTop.constant = top
        rootWide.constant = pane ?? panelWidth
        headWide.constant = pane.map { $0 - 2 * pageSide } ?? contentWidth
        segmentsWide.constant = headWide.constant
        if page == nil { NSLayoutConstraint.activate(smallOnly) }
        let edge = page == nil ? side : pageSide
        root.edgeInsets = NSEdgeInsets(top: page == nil ? 12 : 14, left: edge, bottom: 12, right: edge)
        // The column stands in the middle of the page, and at its left when pictures need the right.
        root.alignment = page == nil || beside ? .leading : .centerX
        column.edgeInsets = NSEdgeInsets(top: page == nil ? 0 : air, left: 0, bottom: 0, right: 0)
        bar.edgeInsets = NSEdgeInsets(top: 0, left: edge - 6, bottom: 0, right: edge - 6)
        backdrop.list = page == nil ? 0 : listWidth
        list.isHidden = page == nil
        panel.isMovable = page == nil
        panel.hasShadow = page == nil
        // The page lies over the Dock and the menu bar; a small window floats over the apps only.
        panel.level = page == nil ? .floating : NSWindow.Level(rawValue: NSWindow.Level.mainMenu.rawValue + 2)
    }

    /// Whether on the whole screen the item's pictures stand large at the right. They do when there is room for
    /// them there: beside a wide column on a narrow screen there is none, and they stay under the item, larger
    /// with everything else.
    var picturesBeside: Bool {
        guard full, let whole = sight(panel.screen ?? NSScreen.main)?.whole else { return false }
        let scale = min(2, max(1, whole.height / pageHeight))
        return whole.width / scale - (listWidth + pageSide + contentWidth + 40) - pageSide >= 240
    }

    /// The column stands this wide: everything across it follows, and its texts break their lines anew.
    func widen(to wide: CGFloat) {
        guard wide != columnWide else { return }
        columnWide = wide
        columnWides.forEach { $0.constant = wide }
        sent.preferredMaxLayoutWidth = wide
        context.widen()
        before.widen()
        facts.widen()
        note.widen(to: wide)
        details.room = wide
        fitQuestion()
        for view in answerRows as [NSView] + copyTaps + [segments] {
            view.invalidateIntrinsicContentSize()
            view.needsDisplay = true
        }
    }

    /// The scroll kept within what there is to scroll.
    private func settleScroll() {
        scrollMost = box.isHidden ? 0 : max(0, (column.fittingSize.height - boxTall.constant).rounded(.up))
        scrolled = min(max(0, scrolled), scrollMost)
        scrollTop.constant = -scrolled
    }

    /// How far under the head its hairline runs, and from it the item: the space after the head is twice this and the line.
    static let headRule: CGFloat = 11

    /// The hairline under the head, across the window, between who asks and what is asked.
    private func markRule() {
        backdrop.rule = folded ? nil : (backdrop.convert(head.bounds, from: head).minY - Controller.headRule - 1).rounded()
    }

    /// Beside a column that is scrolled, at its right: all the way it goes, and the part of it in sight.
    private func markWay() {
        box.more = (scrolled > 0, scrolled < scrollMost)
        guard scrollMost > 0, !box.isHidden else {
            backdrop.way = nil
            return
        }
        let seen = backdrop.convert(box.bounds, from: box)
        let all = NSRect(x: seen.maxX + ((full ? pageSide : side) - 4) / 2, y: seen.minY + 2, width: 4, height: seen.height - 4)
        let tall = all.height * max(24 / all.height, boxTall.constant / (boxTall.constant + scrollMost))
        backdrop.way = (all, NSRect(x: all.minX, y: all.maxY - tall - (all.height - tall) * scrolled / scrollMost, width: 4, height: tall))
    }

    /// The wheel turned over the column, or two fingers moved on it, by so many points of the screen.
    func turn(_ by: CGFloat) -> Bool {
        guard scrollMost > 0 else { return false }
        scroll(to: scrolled - by / scale)
        return true
    }

    func scroll(to place: CGFloat) {
        let place = (min(max(0, place), scrollMost) * 2).rounded() / 2
        guard place != scrolled else { return }
        scrolled = place
        scrollTop.constant = -scrolled
        backdrop.layoutSubtreeIfNeeded()
        markWay()
        placeLivePage()
        backdrop.needsDisplay = true
    }

    /// A part of the item is brought into sight, when it stands above or under what the window shows of the item.
    func reveal(_ part: NSView) {
        guard scrollMost > 0, !part.isHiddenOrHasHiddenAncestor else { return }
        backdrop.layoutSubtreeIfNeeded()
        let place = box.convert(part.bounds, from: part)
        if place.maxY + 8 > box.bounds.height {
            scroll(to: scrolled + place.maxY + 8 - box.bounds.height)
        } else if place.minY < 0 {
            scroll(to: scrolled + place.minY - 8)
        }
    }

    /// The page: the whole screen, the strip of the camera's notch left empty. Everything is drawn larger by
    /// as much as the screen is taller than the page counts itself. The item's column is as wide as the page is at
    /// the right of its list («а чё такая странная вёрстка в фулскрине, сделай шириной по экрану»): an item that
    /// would not fit was drawn smaller at the width of the small window, a narrow column in the middle of the
    /// screen. Drawn smaller, to fit, it still is, as wide as the page is at each scale, and no smaller than
    /// `smallest`: what does not fit then is scrolled to.
    private func layoutPage(on screen: Sight) {
        let whole = screen.whole
        let notch = screen.notch
        let files = picturesBeside ? shownPictures : []
        for path in files where loaded[path] == nil { loaded[path] = NSImage(contentsOfFile: path) }
        let ready = files.filter { (loaded[$0]?.size.width ?? 0) > 0 && (loaded[$0]?.size.height ?? 0) > 0 }
        panel.setFrame(whole, display: false)
        zoom.frame = NSRect(origin: .zero, size: whole.size)
        // The scale is counted with the item's live page as tall as it has been at its tallest since the item
        // came on the page; the page itself stands as tall as it is now. A page is drawn at the scale, and at
        // another scale its text may break its lines elsewhere: counted by the page's height of the moment, the
        // scale changed the height and the height the scale, round and round, and everything on the screen
        // shook («интерфейс дрожит в фуллскрине. в компактном виде не дрожит»). So while an item stands, what
        // its page does can only make everything smaller, and each time it does the page is taller than ever:
        // that has an end. A click on the page that makes it shorter moves what is under it and nothing else.
        let asks = livePageHolder.isHidden ? nil : livePage?.tall
        let item = (active?.id ?? "") + "#" + String(index)
        if let asks {
            // Before the page has said how tall it is, its height is a guess of the window's, not the page's word.
            let known = livePageRoom?.item == item && livePageRoom?.said == true ? livePageRoom?.tall ?? 0 : 0
            livePageRoom = (item, max(known, asks), livePage?.hasSaid == true)
            livePageTall.constant = max(known, asks)
        } else {
            livePageRoom = nil
        }
        var air: CGFloat = Controller.air
        var last = (scale: scale, air: air, tall: CGFloat(0))
        /// How tall the page's content is with its bar, at the scale tried. The page is given that scale first:
        /// a label counts its height in whole pixels of the screen, so measured under the scale the layout before
        /// had left, the same content came out a little taller or shorter each time, and a page that fills the
        /// screen was drawn anew, a hair smaller or larger, at every letter of a comment.
        /// Every call lays the whole column out anew; `columnLayouts` counts them, and the search below keeps them few.
        func needed() -> CGFloat {
            columnLayouts += 1
            let page = NSSize(width: whole.width / scale, height: whole.height / scale)
            zoom.bounds = NSRect(origin: .zero, size: page)
            // Beside its pictures the column is as wide as in the small window; with nothing beside it, as wide as
            // the page is at the right of the list.
            widen(to: ready.isEmpty ? max(contentWidth, (page.width - listWidth - 2 * pageSide).rounded(.down)) : contentWidth)
            shape(page: page, top: notch / scale, beside: !ready.isEmpty, air: air)
            boxTall.constant = column.fittingSize.height
            last = (scale, air, root.fittingSize.height + barHeight + 12)
            return last.tall
        }
        let room = whole.height - notch
        let own = min(2, max(1, whole.height / pageHeight))
        // An item too tall for the page first takes the air above the column, and only then is everything drawn
        // smaller: at the largest scale at which it fits, looked for between the smallest and the page's own, for
        // a smaller scale is a wider column too, and a wider column a shorter item.
        //
        // The air is added straight onto the item's height, so what an item measures with all of it says what it
        // would with the least (`spare` less) and nothing is laid out to find that out. Every scale tried costs a
        // layout of the whole column, 25 thousandths of a second or so, at every letter of a comment under an
        // item that does not fit. The scales stand on a grid of `grain`, and the search starts from the step the
        // item had last (`scaleHint`), where a letter moves it by one at most: that step and the next one, and the
        // layout that stays, where bisecting the whole range took ten and more.
        let spare = Controller.air - Controller.leastAir
        let grain = Controller.grain
        let bottom = Int((Controller.smallest / grain).rounded())
        let top = Int((own / grain).rounded(.up))
        func scaleAt(_ step: Int) -> CGFloat { step >= top ? own : max(Controller.smallest, CGFloat(step) * grain) }
        // `low` is the largest step known to fit, one under the grid when none does, `high` the least known not to.
        var low = bottom - 1, high = top + 1
        var heights: [Int: CGFloat] = [:]
        func probe(_ step: Int) {
            scale = scaleAt(step)
            heights[step] = needed()
            if (heights[step]! - spare) * scale <= room { low = step } else { high = step }
        }
        if let hint = scaleHint {
            let start = min(max(hint, bottom), top)
            probe(start)
            if low == start { if start < top { probe(start + 1) } } else if start > bottom { probe(start - 1) }
        } else {
            probe(top)
        }
        while high - low > 1 { probe((low + high) / 2) }
        let step = max(low, bottom)
        scale = low < bottom ? Controller.smallest : scaleAt(low)
        scaleHint = low == top ? nil : step
        // What does not fit at the scale found takes the air down to the least, and the layout that stays is
        // made with the air it ends up with.
        let seen = heights[step] ?? 0
        air = min(Controller.air, max(Controller.leastAir, Controller.air - (seen - room / scale)))
        var tall = last.scale == scale && last.air == air ? last.tall : needed()
        let page = NSSize(width: whole.width / scale, height: whole.height / scale)
        if let asks { livePageTall.constant = asks }
        if tall * scale > room { boxTall.constant = max(Controller.leastSeen, boxTall.constant - (tall - room / scale)) }
        settleScroll()
        backdrop.frame = zoom.bounds
        list.frame = NSRect(x: 0, y: 0, width: listWidth - 1, height: page.height - notch / scale)
        fillList(room: list.frame.height - 22)
        backdrop.layoutSubtreeIfNeeded()
        markRule()
        markWay()
        place(pictures: ready, page: page, top: backdrop.convert(context.bounds, from: context).maxY)
    }

    /// The page's list: this check's items, each with the colour of its answer, and under them the checks
    /// that wait. A click goes there. What does not fit is a number.
    private func fillList(room: CGFloat) {
        guard let ticket = active else { return }
        let others = tickets.filter { $0.id != ticket.id }
        let key = ["\(index)", "\(Int(room))"] + ticket.items.map(\.text) + marks.map { ($0.state ?? "-") + ($0.question == true ? "?" : "") + ($0.unclear == true ? "!" : "") }
            + others.map { $0.id + $0.title }
        guard key != listKey else { return }
        listKey = key

        func caption(_ text: String) -> NSView {
            let label = NSTextField(labelWithString: text)
            label.font = Face.small(11.5, .medium)
            label.textColor = Ink.faint
            let holder = NSStackView(views: [label])
            holder.edgeInsets = NSEdgeInsets(top: 4, left: 10, bottom: 5, right: 0)
            return holder
        }
        func tall(_ entries: [Entry]) -> CGFloat { entries.reduce(0) { $0 + $1.intrinsicContentSize.height + 1 } }

        var items: [Entry] = []
        for (number, item) in ticket.items.enumerated() {
            let entry = Entry(Linked.plain(item.text))
            entry.label = item.label.isEmpty ? "" : item.label + "."
            entry.mark = ink(of: marks[number], in: tint(of: kind(at: number)))
            entry.current = number == index
            entry.press = { [weak self] in self?.go(to: number) }
            items.append(entry)
        }
        let waiting: [Entry] = others.map { other in
            let name = [other.tag, other.kindTitle ?? ""].filter { !$0.isEmpty }.joined(separator: " · ")
            let entry = Entry(name.isEmpty ? other.title : name)
            entry.note = name.isEmpty ? "" : other.title
            entry.mark = tint(of: other.kind)
            entry.lines = 1
            let id = other.id
            entry.press = { [weak self] in self?.jump(to: id) }
            return entry
        }
        let heading: CGFloat = 28
        // A long check keeps to a line an item, so that the waiting checks stay in sight.
        if heading + tall(items) > room * (waiting.isEmpty ? 1 : 0.7) { items.forEach { $0.lines = 1 } }
        var left = room - heading - tall(items) - 12 - heading
        var fitting: [Entry] = []
        for (number, entry) in waiting.enumerated() {
            let needs = entry.intrinsicContentSize.height + 1
            if needs + (number < waiting.count - 1 ? 26 : 0) > left { break }
            fitting.append(entry)
            left -= needs
        }

        listStack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        entries = items + fitting
        listStack.addArrangedSubview(caption(items.count > 1 ? "Пункты" : "Пункт"))
        items.forEach { listStack.addArrangedSubview($0) }
        if !waiting.isEmpty {
            if let last = items.last { listStack.setCustomSpacing(12, after: last) }
            listStack.addArrangedSubview(caption("Дальше"))
            fitting.forEach { listStack.addArrangedSubview($0) }
            if fitting.count < waiting.count { listStack.addArrangedSubview(caption("ещё \(waiting.count - fitting.count)")) }
        }
    }

    /// The item's pictures at the right of the page, from the top of the column to the action bar:
    /// in one column or in two, whichever shows them larger. A click, or its key, looks at one closer.
    private func place(pictures files: [String], page: NSSize, top: CGFloat) {
        let left = listWidth + pageSide + contentWidth + 40
        let area = NSRect(x: left, y: barHeight + 28, width: page.width - left - pageSide, height: top - barHeight - 28)
        let key = files.isEmpty ? "" : files.joined(separator: "|") + NSStringFromRect(area)
        guard key != galleryKey else { return }
        galleryKey = key
        gallery.forEach { $0.removeFromSuperview() }
        gallery = []
        guard !files.isEmpty, area.width > 80, area.height > 80 else { return }
        let images = files.compactMap { loaded[$0] }
        let sizes = images.map(\.size)
        let gap: CGFloat = 16
        var best: [NSRect] = []
        var bestArea: CGFloat = 0
        for columns in 1...min(2, sizes.count) {
            let rows = Int((Double(sizes.count) / Double(columns)).rounded(.up))
            let cell = NSSize(width: (area.width - gap * CGFloat(columns - 1)) / CGFloat(columns),
                              height: (area.height - gap * CGFloat(rows - 1)) / CGFloat(rows))
            var rects: [NSRect] = []
            var covered: CGFloat = 0
            for (number, size) in sizes.enumerated() {
                let ratio = min(cell.width / size.width, cell.height / size.height)
                let fitted = NSSize(width: (size.width * ratio).rounded(.down), height: (size.height * ratio).rounded(.down))
                let column = CGFloat(number % columns), row = CGFloat(number / columns)
                let slot = NSRect(x: area.minX + column * (cell.width + gap), y: area.maxY - (row + 1) * cell.height - row * gap,
                                  width: cell.width, height: cell.height)
                // Against the top of its place, level with the text beside it.
                rects.append(NSRect(x: (slot.midX - fitted.width / 2).rounded(), y: (slot.maxY - fitted.height).rounded(),
                                    width: fitted.width, height: fitted.height))
                covered += fitted.width * fitted.height
            }
            if covered > bestArea {
                best = rects
                bestArea = covered
            }
        }
        for (number, rect) in best.enumerated() {
            let picture = Picture(path: files[number], image: images[number], frame: rect)
            // Its key is its place among the item's pictures, whichever of them could be drawn here.
            let place = shownPictures.firstIndex(of: files[number]) ?? number
            picture.key = "⌘\(place + 1)"
            picture.keysLive = hasKeyboard
            picture.press = { [weak self] in self?.look(at: place) }
            backdrop.addSubview(picture)
            gallery.append(picture)
        }
    }
}
