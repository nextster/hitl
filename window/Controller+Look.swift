// A picture looked at closer: a card of the window's own in the middle of the screen, and the keys stay the window's.

import AppKit

extension Controller {
    // MARK: A picture, closer
    //
    // «нужно чтобы картинки quick look-ом прямо в hitl показывались и чтобы можно было квиклукнуть по
    // cmd+1/2/3... среди прочих экшнов». Until then a click on a picture opened it in another app, and that
    // app took him away from the check.
    //
    // It was the system's Quick Look for four builds, and in his hands it never left the keyboard alone: its
    // keys went nowhere until he clicked the window, then it came up empty, then small in a corner, and at
    // last: «он разваливает фокус. убирай нахуй полностью quicklook сделай что то своё что будет показывать
    // превью по центру экрана крупно и позволит переходить». So the card is the window's own. Nothing about
    // it touches the keyboard or which app is the active one: its panel cannot take the keys, and they are
    // read by the window, as every other key is.

    /// A picture is up on the screen. A scripted run shows none and only writes down what it would have shown.
    var isLookUp: Bool { scriptFile == nil && lookingAt != nil }

    /// Looks closer at one of the item's pictures, or of `files`: the screenshots of its comment.
    func look(at number: Int, among files: [String]? = nil) {
        let files = files ?? shownPictures
        guard active != nil, files.indices.contains(number) else { return }
        looked = files
        lookedIn = (active?.id ?? "") + "/" + (marks.indices.contains(index) ? marks[index].label : "")
        lookingAt = number
        if scriptFile != nil {
            lookedAt.append(files[number])
        } else {
            showLook()
        }
    }

    private func picture(_ path: String) -> NSImage? {
        if loaded[path] == nil { loaded[path] = NSImage(contentsOfFile: path) }
        return loaded[path]
    }

    /// The card with the picture he is at, as large as it fits in this much room.
    private func fill(_ card: Preview, room: NSSize) {
        guard let at = lookingAt, looked.indices.contains(at) else { return }
        let image = picture(looked[at])
        card.image = image
        card.number = at
        card.count = looked.count
        card.setFrameSize(Preview.size(for: image?.size ?? .zero, in: room))
    }

    /// Puts the card in the middle of the screen the window is on, over the window and over the page.
    private func showLook() {
        guard lookingAt != nil, let screen = panel.screen ?? NSScreen.main else { return }
        let holder = lookPanel ?? {
            let made = PreviewPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
            made.isOpaque = false
            made.backgroundColor = .clear
            made.hasShadow = true
            made.hidesOnDeactivate = false
            made.isFloatingPanel = true
            made.isReleasedWhenClosed = false
            made.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
            made.appearance = NSAppearance(named: .darkAqua)
            let card = Preview()
            card.close = { [weak self] in self?.closeLook() }
            card.turn = { [weak self] by in self?.turnLook(by: by) }
            made.contentView = card
            lookPanel = made
            return made
        }()
        guard let card = holder.contentView as? Preview else { return }
        let whole = screen.visibleFrame
        fill(card, room: NSSize(width: (whole.width * 0.86).rounded(), height: (whole.height * 0.9).rounded()))
        let size = card.frame.size
        holder.setFrame(NSRect(x: (whole.midX - size.width / 2).rounded(), y: (whole.midY - size.height / 2).rounded(),
                               width: size.width, height: size.height), display: true)
        holder.level = NSWindow.Level(rawValue: panel.level.rawValue + 1)
        holder.invalidateShadow()
        holder.orderFrontRegardless()
    }

    /// Puts the card away: by esc, space or a click on it, on another item, when the keys go to another app.
    func closeLook() {
        guard lookingAt != nil else { return }
        lookingAt = nil
        lookPanel?.orderOut(nil)
    }

    /// To the picture before or after; at the first and the last one it stays.
    func turnLook(by step: Int) {
        guard let at = lookingAt else { return }
        turnLook(to: min(max(at + step, 0), looked.count - 1))
    }

    private func turnLook(to number: Int) {
        guard let at = lookingAt, looked.indices.contains(number), number != at else { return }
        lookingAt = number
        if scriptFile != nil {
            lookedAt.append(looked[number])
        } else {
            showLook()
        }
    }

    /// The keys left the window with a picture up: he has gone to another app, and the picture goes away.
    func keysLeftWithLookUp() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            if let self, self.isLookUp, !self.panel.isKeyWindow { self.closeLook() }
        }
    }

    /// A key while a picture is looked at: the arrows walk the pictures, ⌘ with a digit goes to that one.
    func lookTakes(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, let at = lookingAt else { return false }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        switch event.keyCode {
        case 123, 126: turnLook(by: -1)
        case 124, 125: turnLook(by: 1)
        default:
            guard flags.contains(.command), let digit = Int(event.charactersIgnoringModifiers ?? ""), looked.indices.contains(digit - 1) else { return false }
            if digit - 1 != at { turnLook(to: digit - 1) }
        }
        return true
    }

    /// The card as it would stand on a screen, over the window: for a picture of it, as a scripted run shows none.
    func snapshotLook(to path: String) {
        guard lookingAt != nil, let content = panel.contentView else { return }
        let screen = NSSize(width: 1512, height: 944)
        let card = Preview()
        fill(card, room: NSSize(width: (screen.width * 0.86).rounded(), height: (screen.height * 0.9).rounded()))
        let scale: CGFloat = 2
        func drawn(_ view: NSView) -> NSBitmapImageRep? {
            let size = view.bounds.size
            guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size.width * scale), pixelsHigh: Int(size.height * scale),
                                             bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                             colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
                  let context = { rep.size = size; return NSGraphicsContext(bitmapImageRep: rep) }() else { return nil }
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = context
            content.effectiveAppearance.performAsCurrentDrawingAppearance { view.displayIgnoringOpacity(view.bounds, in: context) }
            NSGraphicsContext.restoreGraphicsState()
            return rep
        }
        guard let window = drawn(content), let over = drawn(card),
              let whole = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(screen.width * scale), pixelsHigh: Int(screen.height * scale),
                                           bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                           colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
              let context = { whole.size = screen; return NSGraphicsContext(bitmapImageRep: whole) }() else { return }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        // A screen with some app on it, the window in its middle, and the card over both.
        Ink.hex(0x3A3D46).setFill()
        NSRect(origin: .zero, size: screen).fill()
        func place(_ rep: NSBitmapImageRep, shadow: CGFloat) {
            let rect = NSRect(x: ((screen.width - rep.size.width) / 2).rounded(), y: ((screen.height - rep.size.height) / 2).rounded(),
                              width: rep.size.width, height: rep.size.height)
            NSGraphicsContext.saveGraphicsState()
            let shade = NSShadow()
            shade.shadowColor = NSColor.black.withAlphaComponent(0.55)
            shade.shadowBlurRadius = shadow
            shade.shadowOffset = NSSize(width: 0, height: -shadow / 3)
            shade.set()
            rep.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
            NSGraphicsContext.restoreGraphicsState()
        }
        place(window, shadow: 24)
        place(over, shadow: 40)
        NSGraphicsContext.restoreGraphicsState()
        try? whole.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
    }
}
