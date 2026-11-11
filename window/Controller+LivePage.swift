// The item's live page: which page is on the window, what it has set, and where it stands.

import AppKit

extension Controller {
    private func key(_ ticket: Ticket, _ number: Int) -> String { ticket.id + "#" + String(number) }

    /// Puts the item's page on the window. A page lives for as long as its check is in the queue, so what he
    /// set on it is there when he comes back to the item.
    func showLivePage() {
        guard let ticket = active else { return }
        var wanted: LivePage?
        if let path = ticket.items[index].page, FileManager.default.fileExists(atPath: path) {
            let name = key(ticket, index)
            if let old = livePages[name] {
                wanted = old
            } else {
                let fresh = LivePage(path: path)
                let id = ticket.id, number = index
                fresh.changed = { [weak self] in self?.livePageChanged(of: id, item: number) }
                // The wheel over a page scrolls the page while the page has room that way, and the item when it has
                // none, as one list in another does in a browser. A turn of two fingers is the page's or the item's
                // from its start to its end, and so is the coasting after it: the hand-over does not come in the middle
                // of one stroke.
                fresh.web.turned = { [weak self, weak fresh] event in
                    guard let self, let fresh else { return false }
                    let by = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * 16
                    if self.wheelStaysOnPage(by: by, page: fresh, begins: event.momentumPhase.isEmpty && (event.phase.isEmpty || event.phase.contains(.began))) { return false }
                    self.box.scrollWheel(with: event)
                    return true
                }
                livePages[name] = fresh
                wanted = fresh
            }
        }
        if livePage !== wanted {
            livePage?.removeFromSuperview()
            livePage = wanted
        }
        shown[ObjectIdentifier(livePageHolder)] = wanted != nil
        if let wanted {
            livePageTall.constant = wanted.tall
            let set = pageValues(ticket, index)
            if marks.indices.contains(index), !set.isEmpty { marks[index].values = set }
        }
        refreshSent()
    }

    /// Whether this turn of the wheel over the page is the page's. It is decided where a turn begins and kept to
    /// its end; with the item having nothing to scroll it is the page's whenever the page can take it.
    func wheelStaysOnPage(by: CGFloat, page: LivePage, begins: Bool) -> Bool {
        if begins { wheelOnPage = scrollMost <= 0 || page.scrolls(by: by) }
        return wheelOnPage
    }

    /// What is set on an item's page: on the phone, when he turned it there last, or here.
    private func pageValues(_ ticket: Ticket, _ number: Int) -> [[String]] {
        phoneValues[key(ticket, number)] ?? livePages[key(ticket, number)]?.values ?? []
    }

    /// He set the item's page on the phone: that is what goes to the chat, until he turns the page here.
    func setPhoneValues(_ values: [[String]], item number: Int) {
        guard let ticket = active, marks.indices.contains(number), ticket.items[number].page != nil else { return }
        let read: [[String]] = values.prefix(LivePage.mostValues).compactMap { pair in
            guard pair.count == 2, !pair[0].isEmpty else { return nil }
            return [String(pair[0].prefix(LivePage.longestName)), String(pair[1].prefix(LivePage.longestValue))]
        }
        guard !read.isEmpty else { return }
        phoneValues[key(ticket, number)] = read
        marks[number].values = read
        guard number == index else { return }
        refreshSent()
        arrange()
        layout()
    }

    /// A page said what is set on it, or how tall it is.
    private func livePageChanged(of id: String, item number: Int) {
        guard let ticket = active, ticket.id == id, let said = livePages[key(ticket, number)] else { return }
        if !said.values.isEmpty {
            // The page's first word is how it came up; another one after it is his hand on it, here.
            let name = key(ticket, number)
            if let before = macValues[name], before != said.values {
                phoneValues[name] = nil
                if marks.indices.contains(number) { marks[number].mac = true }
            }
            macValues[name] = said.values
        }
        let set = pageValues(ticket, number)
        if marks.indices.contains(number), !set.isEmpty { marks[number].values = set }
        guard number == index else { return }
        livePageTall.constant = said.tall
        refreshSent()
        arrange()
        layout()
    }

    /// Under the page: what goes to the chat with his answer, so that he sees it before he answers.
    func refreshSent() {
        let values = livePage == nil ? [] : active.map { pageValues($0, index) } ?? []
        let line = NSMutableAttributedString(string: "Уйдёт чату с ответом: ", attributes: [.font: Face.sent, .foregroundColor: Ink.faint])
        line.append(NSAttributedString(string: values.map { $0.joined(separator: " ") }.joined(separator: " · "),
                                       attributes: [.font: Face.sent, .foregroundColor: Ink.soft]))
        sent.attributedStringValue = line
        sent.toolTip = values.map { $0.joined(separator: ": ") }.joined(separator: "\n")
        shown[ObjectIdentifier(sent)] = livePage != nil && !values.isEmpty
    }

    /// Before a check leaves the window, each of its items takes the last word of its page.
    func takeLivePageValues() {
        guard let ticket = active else { return }
        for number in marks.indices {
            let set = pageValues(ticket, number)
            if !set.isEmpty { marks[number].values = set }
        }
    }

    /// The pages of checks that are gone go with them.
    func dropLivePages() {
        for (name, old) in livePages where !tickets.contains(where: { name.hasPrefix($0.id + "#") }) {
            if livePage === old { livePage = nil }
            old.close()
            livePages[name] = nil
        }
        for name in Array(phoneValues.keys) + Array(macValues.keys) where !tickets.contains(where: { name.hasPrefix($0.id + "#") }) {
            phoneValues[name] = nil
            macValues[name] = nil
        }
    }

    /// Where the page stands: under the item, as wide as the column, at the place its holder keeps for it. It
    /// lies over the window's content, on the screen's own points: a web view is not drawn through a scaled
    /// view, and on the whole screen everything else is. There the page is larger by as much as the rest is.
    ///
    /// It stands on whole points. A web view counts its width in them, and its holder, scaled, is half a point
    /// or a hair off: the page was then a pixel of its own narrower or wider at one scale than at another, a
    /// word of it went to the next line, and its height went back into the scale. So the page is as wide in its
    /// own pixels as the item's column is in the window's points, whatever the scale, and no shorter than it
    /// asked to be.
    ///
    /// No more of it is seen than of the column: an item that is scrolled takes its page along, under the head
    /// and the bar.
    func placeLivePage() {
        guard let page = livePage else { return }
        guard active != nil, !folded, !livePageHolder.isHiddenOrHasHiddenAncestor else {
            if page.superview != nil { page.removeFromSuperview() }
            return
        }
        let seen = stage.convert(box.bounds, from: box)
        let sight = NSRect(x: seen.minX.rounded(.down), y: seen.minY.rounded(.up), width: seen.width.rounded(.up) + 2,
                           height: max(0, seen.maxY.rounded(.down) - seen.minY.rounded(.up)))
        if pageSight.frame != sight { pageSight.frame = sight }
        if page.superview !== pageSight { pageSight.addSubview(page) }
        let held = stage.convert(livePageHolder.bounds, from: livePageHolder)
        let wide = max(1, held.width.rounded())
        page.own = columnWide
        page.enlargement = wide / columnWide
        let tall = (page.tall * page.enlargement).rounded(.up)
        let place = NSRect(x: held.minX.rounded() - sight.minX, y: held.maxY.rounded() - tall - sight.minY, width: wide, height: tall)
        if page.frame != place { page.frame = place }
    }
}
