// Where the window stands and how he moves it: the middle of the screen or the place he put it at, Shift and the mouse.

import AppKit

extension Controller {
    /// Where a window of this size has its top right corner when it stands in the middle of the screen he is at:
    /// «чтобы окно вылезало по центру по умолчанию».
    func middle(for size: NSSize) -> NSPoint {
        guard let screen = sight(NSScreen.main)?.free else { return NSPoint(x: size.width, y: size.height) }
        stoodIn = screen
        return NSPoint(x: (screen.midX + size.width / 2).rounded(), y: (screen.midY + size.height / 2).rounded())
    }

    /// The place he put the window at, while the screen it was on is there.
    var putInSight: NSPoint? {
        guard let put else { return nil }
        let top = NSPoint(x: put.x - panelWidth / 2, y: put.y - 1)
        return NSScreen.screens.contains { $0.frame.contains(top) } ? put : nil
    }

    /// A window grown past the bottom of its screen is lifted for as long as it is that tall: the answers stay in sight.
    func fitted(_ frame: NSRect) -> NSRect {
        // Not while he is moving it: a window is not pulled from under his hand.
        guard carry == nil, scriptFile != nil || NSEvent.pressedMouseButtons & 1 == 0 else { return frame }
        let top = NSPoint(x: frame.midX, y: frame.maxY - 1)
        guard let screen = sight(NSScreen.screens.first { $0.frame.contains(top) } ?? NSScreen.main)?.free else { return frame }
        var frame = frame
        frame.origin.y = max(frame.minY, screen.minY + 8)
        if frame.maxY > screen.maxY { frame.origin.y = screen.maxY - frame.height }
        return frame
    }

    /// The window is not where it was put: he has dragged it, and there it stays.
    @objc func windowMoved() {
        guard isUp, !full, let placed else { return }
        let frame = panel.frame
        guard abs(frame.minX - placed.minX) > 0.5 || abs(frame.minY - placed.minY) > 0.5 else { return }
        self.placed = frame
        stand(at: NSPoint(x: frame.maxX, y: frame.maxY))
    }

    /// His own choice of the place: it holds for this window and for the checks to come.
    private func stand(at corner: NSPoint) {
        anchor = corner
        put = corner
        savingPlace?.invalidate()
        savingPlace = nil
        // Written once the move has come to rest, not at every step of it.
        if scriptFile != nil {
            writePlace()
        } else {
            savingPlace = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: false) { [weak self] _ in self?.writePlace() }
        }
    }

    private func writePlace() {
        guard let put else { return }
        try? JSONEncoder().encode(Place(corner: [Double(put.x), Double(put.y)])).write(to: placeFile, options: .atomic)
    }

    /// The window goes where the pointer goes. With Shift held down while the keyboard is here, without a click,
    /// until Shift comes up: «как в obsidian overlay плагине». Or by the mouse, pressed on anything that is not
    /// pressed or written in, until the button comes up: «да и просто мышкой двигать тоже не помешает».
    struct Carry {
        let pointer: NSPoint
        let corner: NSPoint
        let byMouse: Bool
    }

    private var pointer: NSPoint { scriptedPointer ?? NSEvent.mouseLocation }
    private var canMove: Bool { isUp && !full && active != nil }

    private func carries(_ flags: NSEvent.ModifierFlags) -> Bool {
        flags.contains(.shift) && flags.isDisjoint(with: [.command, .control, .option])
    }

    func modifiersChanged(_ event: NSEvent) {
        if carry?.byMouse == true { return }  // the mouse has the window: the keys change nothing
        guard carries(event.modifierFlags) else {
            setDown()
            return
        }
        // Only Shift going down picks the window up: not ⌘ let go while Shift is still held.
        guard carry == nil, canMove, hasKeyboard, event.keyCode == 56 || event.keyCode == 60 else { return }
        pickUp(byMouse: false)
    }

    /// Whether a press at this point of the window takes the window along. Everything does but what is pressed
    /// or written in, and but the text of the check: a pull of the mouse selects it, «текст выделяется, как сейчас».
    func takesAlong(at point: NSPoint) -> Bool {
        guard canMove, let content = panel.contentView else { return false }
        var view = content.hitTest(content.superview?.convert(point, from: nil) ?? point)
        if view == nil { return false }
        while let here = view {
            if here is Tap || here is NoteView || here is ShotView || here is NSButton || here is LivePage { return false }
            if let label = here as? NSTextField, label.isSelectable { return false }
            if let text = here as? NSTextView, text.isSelectable { return false }
            view = here.superview
        }
        return true
    }

    func pressed(at point: NSPoint) {
        if carry == nil, takesAlong(at: point) { pickUp(byMouse: true) }
    }

    func lifted() {
        if carry?.byMouse == true { setDown() }
    }

    private func pickUp(byMouse: Bool) {
        carry = Carry(pointer: pointer, corner: NSPoint(x: panel.frame.maxX, y: panel.frame.maxY), byMouse: byMouse)
        guard scriptFile == nil else { return }
        let timer = Timer(timeInterval: 1.0 / 120, repeats: true) { [weak self] _ in self?.follow() }
        RunLoop.main.add(timer, forMode: .common)
        carrying = timer
    }

    func follow() {
        guard let carry else { return }
        // Shift or the button let go where this app did not hear of it ends the move as well.
        let held = carry.byMouse ? scriptFile != nil || NSEvent.pressedMouseButtons & 1 != 0
                                 : hasKeyboard && (scriptFile != nil || carries(NSEvent.modifierFlags))
        guard canMove, held else {
            setDown()
            return
        }
        let now = pointer
        let corner = NSPoint(x: (carry.corner.x + now.x - carry.pointer.x).rounded(), y: (carry.corner.y + now.y - carry.pointer.y).rounded())
        var frame = panel.frame
        frame.origin = NSPoint(x: corner.x - frame.width, y: corner.y - frame.height)
        guard frame.origin != panel.frame.origin else { return }
        // Never so far off every screen that nothing of it is left to see.
        guard NSScreen.screens.contains(where: { $0.frame.intersection(frame).width >= 80 && $0.frame.intersection(frame).height >= 40 }) else { return }
        placed = frame
        panel.setFrameOrigin(frame.origin)
        stand(at: corner)
    }

    func setDown() {
        carrying?.invalidate()
        carrying = nil
        carry = nil
    }
}
