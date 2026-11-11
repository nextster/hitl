// The window's own views: the panel, the backdrop, what is pressed, the marks of a check and its task.

import AppKit

final class KeyPanel: NSPanel {
    var mouseWentDown: ((NSEvent) -> Void)?
    var mouseCameUp: (() -> Void)?

    override var canBecomeKey: Bool { true }

    /// A click anywhere on the window gives it the keyboard, and nothing else does. A panel
    /// left to take the keyboard on any occasion took it at launch, in the middle of his typing.
    override func sendEvent(_ event: NSEvent) {
        if event.type == .leftMouseDown {
            if scriptFile == nil, !isKeyWindow { makeKey() }
            mouseWentDown?(event)
        } else if event.type == .leftMouseUp {
            mouseCameUp?()
        }
        super.sendEvent(event)
    }
}

/// The panel itself: a dark rounded sheet with the action bar along its bottom. On the whole
/// screen it is the page: square, edge to edge, with the strip of the list at its left.
final class Backdrop: NSView {
    /// The action bar along the bottom; folded, the window has none.
    var bar: CGFloat = barHeight { didSet { needsDisplay = true } }
    /// Where the hairline under the head runs, from the bottom; folded, there is none.
    var rule: CGFloat? { didSet { if rule != oldValue { needsDisplay = true } } }
    /// How wide the list's strip is; none in a small window.
    var list: CGFloat = 0 { didSet { needsDisplay = true } }
    /// Beside a column that is scrolled: all the way it goes, and the part of it that is in sight. That there is
    /// more below is seen by it.
    var way: (all: NSRect, seen: NSRect)? { didSet { if way?.all != oldValue?.all || way?.seen != oldValue?.seen { needsDisplay = true } } }

    /// The wheel turned anywhere over the window, by this many points of the screen: over the head and the bar it
    /// scrolls the item too.
    var turned: ((CGFloat) -> Bool)?

    override func scrollWheel(with event: NSEvent) {
        let by = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * 16
        if turned?(by) != true { super.scrollWheel(with: event) }
    }

    private func drawWay() {
        guard let way else { return }
        Ink.rule.withAlphaComponent(0.6).setFill()
        NSBezierPath(roundedRect: way.all, xRadius: way.all.width / 2, yRadius: way.all.width / 2).fill()
        Ink.soft.setFill()
        NSBezierPath(roundedRect: way.seen, xRadius: way.seen.width / 2, yRadius: way.seen.width / 2).fill()
    }

    override func draw(_ dirtyRect: NSRect) {
        if list > 0 {
            Ink.panel.setFill()
            bounds.fill()
            Ink.bar.setFill()
            NSRect(x: 0, y: 0, width: list, height: bounds.height).fill()
            NSRect(x: list, y: 0, width: bounds.width - list, height: bar).fill()
            Ink.barEdge.setFill()
            NSRect(x: list - 1, y: 0, width: 1, height: bounds.height).fill()
            NSRect(x: list, y: bar, width: bounds.width - list, height: 1).fill()
            if let rule {
                Ink.hair.setFill()
                NSRect(x: list, y: rule, width: bounds.width - list, height: 1).fill()
            }
            drawWay()
            return
        }
        let radius = min(16, bounds.height / 2)
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: radius, yRadius: radius)
        NSGraphicsContext.saveGraphicsState()
        shape.addClip()
        Ink.panel.setFill()
        bounds.fill()
        if bar > 0 {
            Ink.bar.setFill()
            NSRect(x: 0, y: 0, width: bounds.width, height: bar).fill()
            Ink.barEdge.setFill()
            NSRect(x: 0, y: bar, width: bounds.width, height: 1).fill()
        }
        if let rule {
            Ink.hair.setFill()
            NSRect(x: 0, y: rule, width: bounds.width, height: 1).fill()
        }
        drawWay()
        NSGraphicsContext.restoreGraphicsState()
        Ink.edge.setStroke()
        shape.stroke()
    }
}

/// The item's column, seen through as much room as the screen leaves it. An item taller than that is scrolled, by
/// two fingers or the wheel, and its head, its strokes and the bar under it stay where they are («в нефуллскрине
/// всё ещё не влазит по высоте, сделай скроллабл»).
final class Scroll: NSView {
    /// The wheel turned over the column, by this many points of the screen; false when there is nothing to scroll.
    var turned: ((CGFloat) -> Bool)?
    /// Whether more of the item stands above what is seen, and under it: the edge it goes behind is shaded.
    var more: (above: Bool, under: Bool) = (false, false) {
        didSet { if more != oldValue { shade.more = more } }
    }
    private let shade = ScrollShade()

    override var isFlipped: Bool { true }

    override func didAddSubview(_ subview: NSView) {
        super.didAddSubview(subview)
        if subview !== shade { addSubview(shade) }  // over whatever the column is
    }

    override func layout() {
        super.layout()
        if shade.frame != bounds { shade.frame = bounds }
    }

    override func scrollWheel(with event: NSEvent) {
        let by = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * 16
        if turned?(by) != true { super.scrollWheel(with: event) }
    }
}

/// The edges of a scrolled column: what goes behind one fades into it.
final class ScrollShade: NSView {
    var more: (above: Bool, under: Bool) = (false, false) { didSet { needsDisplay = true } }

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        let tall: CGFloat = 14
        if more.above {
            NSGradient(starting: Ink.panel, ending: Ink.panel.withAlphaComponent(0))?.draw(in: NSRect(x: 0, y: 0, width: bounds.width, height: tall), angle: 90)
        }
        if more.under {
            NSGradient(starting: Ink.panel.withAlphaComponent(0), ending: Ink.panel)?
                .draw(in: NSRect(x: 0, y: bounds.height - tall, width: bounds.width, height: tall), angle: 90)
        }
    }
}

/// Where the item's live page is seen on the window: the column's own room, and no more of the page than is in it.
/// It lies over everything, and is nothing to the mouse but where the page is.
final class PageSight: NSView {
    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        return hit === self ? nil : hit
    }
}

/// Something to press: it knows the pointer is over it and calls back on a click.
class Tap: NSView {
    var title: String { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    var press: (() -> Void)?
    var isEnabled = true { didSet { needsDisplay = true } }
    var keysLive = false { didSet { needsDisplay = true } }
    private(set) var isOver = false { didSet { needsDisplay = true } }
    private(set) var isDown = false { didSet { needsDisplay = true } }
    /// Its key is held down: it looks pressed, and letting the key go presses it.
    var isHeld = false { didSet { needsDisplay = true } }
    var looksPressed: Bool { isDown || isHeld }

    init(_ title: String) {
        self.title = title
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        setContentHuggingPriority(.required, for: .horizontal)
        setAccessibilityRole(.button)
        setAccessibilityLabel(title)
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    override func mouseEntered(with event: NSEvent) { isOver = true }

    override func mouseExited(with event: NSEvent) {
        isOver = false
        isDown = false
    }

    private func holds(_ event: NSEvent) -> Bool { bounds.contains(convert(event.locationInWindow, from: nil)) }

    override func mouseDown(with event: NSEvent) { isDown = isEnabled }

    override func mouseDragged(with event: NSEvent) {
        if isEnabled { isDown = holds(event) }
    }

    override func mouseUp(with event: NSEvent) {
        let pressed = isDown && holds(event)
        isDown = false
        if pressed { press?() }
    }

    /// The middle of a part of it, by the part's name and in the window: where a scripted click on that part
    /// lands. Most are pressed whole and have no parts.
    func spot(of part: String) -> NSPoint? { nil }
}

/// An answer: a button across the column, its key at its start, beside the words it presses («Ответы — кнопками,
/// цифра слева рядом со словами», 2026-10-20). The chosen one is tinted, with its cap filled and a tick at the end,
/// whether or not the window has the keyboard. «Не понял» is quieter: a button without its ground.
final class Row: Tap {
    var key = ""
    var chosen: NSColor? { didSet { needsDisplay = true } }
    /// A box before the title, ticked or not: the row is one of several that may hold at once.
    var box: Bool? { didSet { needsDisplay = true } }
    /// Not an answer to the item but about it: drawn without a ground of its own.
    var quiet = false { didSet { needsDisplay = true } }

    override var intrinsicContentSize: NSSize { NSSize(width: columnWide, height: Face.rowTall) }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 9, yRadius: 9)
        if let chosen {
            (Ink.panel.blended(withFraction: looksPressed ? 0.26 : 0.17, of: chosen) ?? chosen).setFill()
            shape.fill()
            (Ink.panel.blended(withFraction: 0.6, of: chosen) ?? chosen).setStroke()
            shape.stroke()
        } else if looksPressed {
            Ink.rule.setFill()
            shape.fill()
        } else if isOver {
            Ink.hover.blended(withFraction: 0.35, of: Ink.rule)?.setFill()
            shape.fill()
        } else if !quiet {
            Ink.surface.setFill()
            shape.fill()
        }
        var left: CGFloat = 8
        if !key.isEmpty {
            KeyCap.draw(key, rightEdge: left + KeyCap.widthInRow(key), midY: bounds.midY, live: keysLive, pressed: isHeld, chosen: chosen, tall: KeyCap.inRow)
            left += KeyCap.widthInRow(key) + 12
        }
        if let box {
            let square = NSRect(x: left, y: (bounds.midY - 7.5).rounded(), width: 15, height: 15)
            let outline = NSBezierPath(roundedRect: square.insetBy(dx: 0.75, dy: 0.75), xRadius: 4, yRadius: 4)
            if box {
                (chosen ?? Ink.soft).setFill()
                outline.fill()
                let tick = NSBezierPath()
                tick.move(to: NSPoint(x: square.minX + 3.6, y: square.midY - 0.2))
                tick.line(to: NSPoint(x: square.minX + 6.4, y: square.minY + 4.2))
                tick.line(to: NSPoint(x: square.maxX - 3.4, y: square.maxY - 4.2))
                tick.lineWidth = 1.8
                tick.lineCapStyle = .round
                tick.lineJoinStyle = .round
                NSColor.white.setStroke()
                tick.stroke()
            } else {
                outline.lineWidth = 1.5
                Ink.faint.setStroke()
                outline.stroke()
            }
            left += 15 + 10
        }
        let ink: NSColor = chosen != nil ? .white : quiet && !isOver ? Ink.soft : Ink.text
        let attributes: [NSAttributedString.Key: Any] = [.font: Face.row, .foregroundColor: ink, .paragraphStyle: Face.oneLine]
        let height = (title as NSString).size(withAttributes: attributes).height
        let tickRoom: CGFloat = chosen != nil && box == nil ? 30 : 12
        (title as NSString).draw(in: NSRect(x: left, y: (bounds.height - height) / 2, width: bounds.width - left - tickRoom, height: height),
                                 withAttributes: attributes)
        // The chosen one says so at its end too: a tick in its colour.
        guard let chosen, box == nil else { return }
        let setup = NSImage.SymbolConfiguration(pointSize: 13, weight: .bold).applying(.init(paletteColors: [chosen]))
        if let tick = NSImage(systemSymbolName: "checkmark", accessibilityDescription: "выбрано")?.withSymbolConfiguration(setup) {
            tick.draw(in: NSRect(x: (bounds.width - 14 - tick.size.width).rounded(), y: (bounds.midY - tick.size.height / 2).rounded(),
                                 width: tick.size.width, height: tick.size.height))
        }
    }
}

final class Flat: Tap {
    var key = "" { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    var size: CGFloat = 12.5 + Face.larger
    var color = Ink.link
    /// The one to press: white and heavier.
    var strong = false { didSet { needsDisplay = true } }
    /// The button that ends the check: a white one while it can be pressed, an outline while it cannot.
    var primary = false { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    /// What it opens is another place: in the colour of a link, after a sign that it opens elsewhere.
    var opens = false { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    /// What stands after the title, quieter: where a press of it has got to.
    var trail = "" { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    var trailColor = Ink.faint { didSet { needsDisplay = true } }
    /// The title is an address: in the font of code.
    var isAddress = false { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    /// The symbol the phone shows on the button, before its name here.
    var symbol: String? { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    /// A switch, on or off: drawn after the title. Nil for anything else.
    var isOn: Bool? { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    /// What a choice chooses between, after the title, and the one that holds. A click on one picks it; a press
    /// of the button anywhere else, or by its key, is the option after the current.
    var options: [String] = [] { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    var current: Int? { didSet { needsDisplay = true } }
    var pick: ((Int) -> Void)?

    /// What the button says, in words: its name and how it stands.
    var said: String {
        if let isOn { return title + (isOn ? ": включено" : ": выключено") }
        guard !options.isEmpty else { return title }
        return title + ": " + options.enumerated().map { $0.offset == current ? "[\($0.element)]" : $0.element }.joined(separator: " ")
    }

    private var face: NSFont {
        isAddress ? NSFont.monospacedSystemFont(ofSize: size - 0.5, weight: .regular) : NSFont.systemFont(ofSize: size, weight: strong ? .semibold : .regular)
    }
    private var plain: NSFont { NSFont.systemFont(ofSize: size) }
    private var heavy: NSFont { NSFont.systemFont(ofSize: size, weight: .semibold) }
    private func width(_ text: String, _ font: NSFont) -> CGFloat { (text as NSString).size(withAttributes: [.font: font]).width.rounded(.up) }

    private var shownTitle: String { options.isEmpty ? title : title + ":" }
    private var image: NSImage? {
        (symbol ?? (opens ? "arrow.up.forward.square" : nil)).flatMap { NSImage(systemSymbolName: $0, accessibilityDescription: nil) }?
            .withSymbolConfiguration(.init(pointSize: size - 1.5, weight: .medium))
    }
    private var symbolWidth: CGFloat { image.map { $0.size.width.rounded(.up) + 6 } ?? 0 }
    private var titleWidth: CGFloat { width(shownTitle, isAddress ? face : heavy) }
    private static let switchSize = NSSize(width: 24, height: 14)
    private var switchWidth: CGFloat { isOn == nil ? 0 : 7 + Self.switchSize.width }
    /// Where each option stands, from the left edge of the button.
    private var optionFrames: [NSRect] {
        var x = 8 + symbolWidth + titleWidth + 8
        return options.map { option in
            let frame = NSRect(x: x, y: 0, width: width(option, heavy), height: bounds.height)
            x = frame.maxX + 9
            return frame
        }
    }
    private var optionsWidth: CGFloat { options.isEmpty ? 0 : 8 + options.map { width($0, heavy) }.reduce(0, +) + CGFloat(options.count - 1) * 9 }
    private var trailWidth: CGFloat { trail.isEmpty ? 0 : 7 + width(trail, plain) }

    override var intrinsicContentSize: NSSize {
        let cap = KeyCap.width(key)
        let side: CGFloat = primary ? 12 : 8
        return NSSize(width: side + symbolWidth + titleWidth + switchWidth + optionsWidth + trailWidth + (cap > 0 ? 7 + cap : 0) + side - (primary ? 4 : 0),
                      height: primary ? 30 : 28)
    }

    /// The middle of an option.
    override func spot(of option: String) -> NSPoint? {
        guard let place = options.firstIndex(of: option) else { return nil }
        return convert(NSPoint(x: optionFrames[place].midX, y: bounds.midY), to: nil)
    }

    override func mouseUp(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        guard let pick, let option = optionFrames.firstIndex(where: { $0.insetBy(dx: -4.5, dy: 0).contains(point) }) else {
            return super.mouseUp(with: event)
        }
        // A click on an option is that option, not the one after the current.
        let usual = press
        press = { pick(option) }
        super.mouseUp(with: event)
        press = usual
    }

    override func draw(_ dirtyRect: NSRect) {
        if primary {
            let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 8, yRadius: 8)
            if isEnabled {
                (looksPressed ? Ink.soft : isOver ? NSColor.white : Ink.text).setFill()
                shape.fill()
            } else {
                Ink.edge.setStroke()
                shape.stroke()
            }
        } else if isEnabled, isOver || looksPressed {
            (looksPressed ? Ink.rule : Ink.hover).setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 7, yRadius: 7).fill()
        }
        let ink: NSColor = primary ? (isEnabled ? Ink.panel : Ink.faint) : !isEnabled ? Ink.faint.withAlphaComponent(0.55)
            : strong ? .white : opens ? (isOver || isHeld ? Ink.way.blended(withFraction: 0.35, of: .white) ?? Ink.way : Ink.way) : isOver || isHeld ? Ink.text : color
        let attributes: [NSAttributedString.Key: Any] = [.font: primary && isEnabled ? heavy : face, .foregroundColor: ink]
        let height = (shownTitle as NSString).size(withAttributes: attributes).height
        let baseline = (bounds.height - height) / 2
        var x: CGFloat = primary ? 12 : 8
        if let image = image?.withSymbolConfiguration(.init(pointSize: size - 1.5, weight: .medium).applying(.init(paletteColors: [ink]))) {
            image.draw(in: NSRect(x: x, y: (bounds.midY - image.size.height / 2).rounded(), width: image.size.width, height: image.size.height))
            x += symbolWidth
        }
        (shownTitle as NSString).draw(at: NSPoint(x: x, y: baseline), withAttributes: attributes)
        x += titleWidth
        if let isOn {
            // A switch as the phone draws it, small: lit and with its knob at the right when on.
            let frame = NSRect(x: x + 7, y: (bounds.height - Self.switchSize.height) / 2, width: Self.switchSize.width, height: Self.switchSize.height)
            (isOn ? Ink.hex(0x34C759).withAlphaComponent(isEnabled ? 1 : 0.5) : Ink.keyEdge).setFill()
            NSBezierPath(roundedRect: frame, xRadius: frame.height / 2, yRadius: frame.height / 2).fill()
            let knob = frame.height - 4
            (isOn ? NSColor.white : Ink.soft).setFill()
            NSBezierPath(ovalIn: NSRect(x: isOn ? frame.maxX - knob - 2 : frame.minX + 2, y: frame.minY + 2, width: knob, height: knob)).fill()
            x += switchWidth
        }
        for (place, frame) in optionFrames.enumerated() {
            let holds = place == current
            (options[place] as NSString).draw(at: NSPoint(x: frame.minX, y: baseline), withAttributes: [
                .font: holds ? heavy : plain, .foregroundColor: !isEnabled ? Ink.faint.withAlphaComponent(0.55) : holds ? Ink.text : Ink.faint,
            ])
        }
        x += optionsWidth
        if !trail.isEmpty {
            (trail as NSString).draw(at: NSPoint(x: x + 7, y: baseline), withAttributes: [.font: plain, .foregroundColor: trailColor])
        }
        if primary {
            KeyCap.draw(key, rightEdge: bounds.width - 8, midY: bounds.midY, live: keysLive && isEnabled, pressed: isHeld && isEnabled,
                        chosen: isEnabled ? Ink.hex(0xD3D4DA) : nil)
            return
        }
        KeyCap.draw(key, rightEdge: bounds.width - 8, midY: bounds.midY, live: keysLive && isEnabled, pressed: isHeld && isEnabled)
    }
}

/// A line to copy with one click: a command for the terminal, an address, a value to paste somewhere («нужна фича
/// copyable строчки, я бы щас раз и скопировал по клику нужную команду»). It stands in the font of code on a card
/// of its own, cut at its end when it is long; a click on it, or its key, copies all of it and says so for a moment.
final class CopyLine: Tap {
    private static let face = NSFont.monospacedSystemFont(ofSize: 11.5 + Face.larger, weight: .regular)
    private static let word = Face.small(11.5, .medium)
    private static let done = "Скопировано"
    private static let ok = Ink.hex(0x34C759)

    var key = "" { didSet { needsDisplay = true } }
    /// It has just been copied: a tick and the word in the place of the sign, for a moment.
    var copied = false { didSet { needsDisplay = true } }

    override var intrinsicContentSize: NSSize { NSSize(width: columnWide, height: 34) }

    /// What stands at the right: the key's cap, and before it the sign of copying or the word that it is done.
    private var rightRoom: CGFloat {
        let cap = KeyCap.width(key)
        let said = copied ? (CopyLine.done as NSString).size(withAttributes: [.font: CopyLine.word]).width.rounded(.up) + 6 : 0
        return 10 + (cap > 0 ? cap + 9 : 0) + 15 + said
    }
    private var textRoom: CGFloat { columnWide - 12 - 10 - rightRoom }
    /// The line does not fit and is cut: all of it is in its tooltip.
    var isCut: Bool { (title as NSString).size(withAttributes: [.font: CopyLine.face]).width > textRoom }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 8, yRadius: 8)
        (looksPressed ? Ink.rule : isOver ? Ink.hover : Ink.bar).setFill()
        shape.fill()
        (copied ? CopyLine.ok.withAlphaComponent(0.55) : isOver || isHeld ? Ink.keyLitEdge : Ink.edge).setStroke()
        shape.stroke()

        var right = bounds.width - 10
        if !key.isEmpty {
            KeyCap.draw(key, rightEdge: right, midY: bounds.midY, live: keysLive, pressed: isHeld)
            right -= KeyCap.width(key) + 9
        }
        let ink = copied ? CopyLine.ok : isOver || isHeld ? Ink.text : Ink.faint
        let setup = NSImage.SymbolConfiguration(pointSize: 11.5, weight: .medium).applying(.init(paletteColors: [ink]))
        if let sign = NSImage(systemSymbolName: copied ? "checkmark" : "doc.on.doc", accessibilityDescription: nil)?.withSymbolConfiguration(setup) {
            sign.draw(in: NSRect(x: (right - 7.5 - sign.size.width / 2).rounded(), y: (bounds.midY - sign.size.height / 2).rounded(),
                                 width: sign.size.width, height: sign.size.height))
        }
        right -= 15
        if copied {
            let attributes: [NSAttributedString.Key: Any] = [.font: CopyLine.word, .foregroundColor: CopyLine.ok]
            let size = (CopyLine.done as NSString).size(withAttributes: attributes)
            (CopyLine.done as NSString).draw(at: NSPoint(x: right - 6 - size.width.rounded(.up), y: ((bounds.height - size.height) / 2).rounded()),
                                             withAttributes: attributes)
            right -= size.width.rounded(.up) + 6
        }
        let attributes: [NSAttributedString.Key: Any] = [.font: CopyLine.face, .foregroundColor: Ink.text, .paragraphStyle: Face.oneLine]
        let height = (title as NSString).size(withAttributes: attributes).height
        (title as NSString).draw(in: NSRect(x: 12, y: ((bounds.height - height) / 2).rounded(), width: right - 10 - 12, height: height),
                                 withAttributes: attributes)
    }
}

/// A tiny icon to press: the fold of the window.
final class Glyph: Tap {
    var symbol = "chevron.up" { didSet { needsDisplay = true } }

    override var intrinsicContentSize: NSSize { NSSize(width: 22, height: 20) }

    override func draw(_ dirtyRect: NSRect) {
        if isOver || looksPressed {
            (looksPressed ? Ink.rule : Ink.hover).setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 6, yRadius: 6).fill()
        }
        let setup = NSImage.SymbolConfiguration(pointSize: 10.5, weight: .semibold)
            .applying(.init(paletteColors: [isOver || isHeld ? Ink.text : Ink.soft]))
        guard let image = NSImage(systemSymbolName: symbol, accessibilityDescription: title)?.withSymbolConfiguration(setup) else { return }
        image.draw(in: NSRect(x: bounds.midX - image.size.width / 2, y: bounds.midY - image.size.height / 2,
                              width: image.size.width, height: image.size.height))
    }
}

/// A line of the page's list: an item of the check with the colour of its answer, or a check
/// that waits with what it is about. A click goes there.
final class Entry: Tap {
    static let width = listWidth - 16
    private static let face = Face.small(12.5)
    private static let line = NSLayoutManager().defaultLineHeight(for: face).rounded(.up)
    private static let textLeft: CGFloat = 26

    /// In front of the title, quieter: an item's number.
    var label = ""
    /// Under the title, quieter: what a waiting check is about.
    var note = ""
    /// The dot: filled with the colour of the answer, a ring while there is none.
    var mark: NSColor?
    var current = false { didSet { needsDisplay = true } }
    /// How many lines the title may take; one, when the list is long.
    var lines = 2 { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }

    private var room: CGFloat { Entry.width - Entry.textLeft - 8 }

    override var isFlipped: Bool { true }

    private func text(_ ink: NSColor) -> NSAttributedString {
        let whole = NSMutableAttributedString()
        if !label.isEmpty { whole.append(NSAttributedString(string: label + " ", attributes: [.font: Entry.face, .foregroundColor: Ink.dim])) }
        whole.append(NSAttributedString(string: title, attributes: [.font: Entry.face, .foregroundColor: ink]))
        return whole
    }

    private var second: NSAttributedString { NSAttributedString(string: note, attributes: [.font: Entry.face, .foregroundColor: Ink.dim]) }

    private func tall(_ string: NSAttributedString, most: Int) -> CGFloat {
        let wanted = string.boundingRect(with: NSSize(width: room, height: .greatestFiniteMagnitude), options: [.usesLineFragmentOrigin]).height
        return min(wanted.rounded(.up), Entry.line * CGFloat(most))
    }

    override var intrinsicContentSize: NSSize {
        NSSize(width: Entry.width, height: 5 + tall(text(Ink.text), most: lines) + (note.isEmpty ? 0 : 1 + tall(second, most: 2)) + 5)
    }

    override func draw(_ dirtyRect: NSRect) {
        if current || isOver || isDown {
            (isDown ? Ink.rule : current ? Ink.hover : Ink.hover.withAlphaComponent(0.55)).setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 7, yRadius: 7).fill()
        }
        let dot = NSRect(x: 9, y: 5 + (Entry.line - 8) / 2, width: 8, height: 8)
        if let mark {
            mark.setFill()
            NSBezierPath(ovalIn: dot).fill()
        } else {
            let ring = NSBezierPath(ovalIn: dot.insetBy(dx: 0.75, dy: 0.75))
            ring.lineWidth = 1.5
            (current ? Ink.soft : Ink.faint).setStroke()
            ring.stroke()
        }
        let first = text(current || isOver ? Ink.text : Ink.link)
        let firstTall = tall(first, most: lines)
        first.draw(with: NSRect(x: Entry.textLeft, y: 5, width: room, height: firstTall), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
        if !note.isEmpty {
            second.draw(with: NSRect(x: Entry.textLeft, y: 5 + firstTall + 1, width: room, height: tall(second, most: 2)),
                        options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
        }
    }
}

/// The facts of an item, a line each on a quiet card: what he has to know to answer, instead of a text to read
/// («я не хочу читать простыню»). A few words between two stars stand out. All of them are one text, so that a
/// pull of the mouse selects across them: as labels of their own each could be selected only by itself
/// («сейчас можно только текст одного пункта выделить»).
final class Facts: NSView, NSTextViewDelegate, LinkedText {
    private static let face = Face.details
    private static let strong = Face.strong
    private static let line = NSLayoutManager().defaultLineHeight(for: face).rounded(.up)
    /// The facts stand as a list at the column's left edge, without a box of their own: one edge for everything he reads.
    private static let textLeft: CGFloat = 16
    private static let top: CGFloat = 1
    private static var room: CGFloat { columnWide - textLeft - 4 }
    private var wide: NSLayoutConstraint!

    let text: ReadText
    /// Where each fact begins in the text: its dot stands by that line.
    private var starts: [Int] = []
    /// A click on a link in a fact, with its address.
    var open: ((String) -> Void)?

    var facts: [String] = [] { didSet { if facts != oldValue { fill() } } }
    var tint = Ink.soft { didSet { needsDisplay = true } }
    var keys: [String] = [] { didSet { if keys != oldValue { fill() } } }
    var links: [(words: String, address: String)] {
        facts.flatMap { $0.components(separatedBy: "**") }.flatMap { Linked.pieces($0) }.compactMap { piece in piece.link.map { (piece.text, $0) } }
    }

    init() {
        text = ReadText.made(wide: Facts.room)
        super.init(frame: .zero)
        text.frame = NSRect(x: Facts.textLeft, y: Facts.top, width: Facts.room, height: Facts.line)
        text.delegate = self
        addSubview(text)
        translatesAutoresizingMaskIntoConstraints = false
        wide = widthAnchor.constraint(equalToConstant: columnWide)
        wide.isActive = true
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    override var isFlipped: Bool { true }

    /// The column is another width: the facts break their lines by it.
    func widen() {
        wide.constant = columnWide
        text.textContainer?.containerSize = NSSize(width: Facts.room, height: .greatestFiniteMagnitude)
        text.frame = NSRect(x: Facts.textLeft, y: Facts.top, width: Facts.room, height: textTall)
        invalidateIntrinsicContentSize()
        needsDisplay = true
    }

    /// A fact with what stands out between two stars and with its links; `key` gives each link its key.
    static func text(_ fact: String, key: () -> String) -> NSAttributedString {
        let whole = NSMutableAttributedString()
        for (number, part) in fact.components(separatedBy: "**").enumerated() where !part.isEmpty {
            let stands = number % 2 == 1
            whole.append(Linked.text(part, [.font: stands ? strong : face, .foregroundColor: stands ? Ink.text : Ink.heading], key: key))
        }
        return whole
    }

    func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        open?((link as? URL)?.absoluteString ?? String(describing: link))
        return true
    }

    private var textTall: CGFloat { max(text.usedTall.rounded(.up), Facts.line) }

    private func fill() {
        let whole = NSMutableAttributedString()
        let between = NSMutableParagraphStyle()
        between.paragraphSpacing = 6
        between.lineSpacing = Face.lineRoom
        starts = []
        var place = 0
        for (number, fact) in facts.enumerated() {
            starts.append(whole.length)
            whole.append(Facts.text(fact) {
                place += 1
                return keys.indices.contains(place - 1) ? keys[place - 1] : ""
            })
            if number < facts.count - 1 { whole.append(NSAttributedString(string: "\n", attributes: [.font: Facts.face])) }
        }
        whole.addAttribute(.paragraphStyle, value: between, range: NSRange(location: 0, length: whole.length))
        text.show(whole)
        text.frame = NSRect(x: Facts.textLeft, y: Facts.top, width: Facts.room, height: textTall)
        invalidateIntrinsicContentSize()
        needsDisplay = true
    }

    override var intrinsicContentSize: NSSize { NSSize(width: columnWide, height: Facts.top + textTall + 2) }

    override func draw(_ dirtyRect: NSRect) {
        guard let manager = text.layoutManager, let container = text.textContainer else { return }
        manager.ensureLayout(for: container)
        tint.setFill()
        for start in starts where start < (text.textStorage?.length ?? 0) {
            let lineTop = manager.lineFragmentRect(forGlyphAt: manager.glyphIndexForCharacter(at: start), effectiveRange: nil).minY
            NSBezierPath(ovalIn: NSRect(x: 2, y: Facts.top + lineTop + (Facts.line - 6) / 2, width: 6, height: 6)).fill()
        }
    }
}

/// The app a check came from, before the project's name, as the icon he knows the app by: «слева от имени проекта
/// только символом если можно. если сложно то текстом коротенечко». A check of a chat of OpenCode had come with no
/// app said anywhere. The icon is the installed app's own, asked of the system by the app's identifier, so nothing of
/// another's is kept here; an app this Mac does not have is said by its name, small.
final class AppMark: NSView {
    /// The apps chats run in, by the name a check gives its agent.
    private static let apps = ["Claude": "com.anthropic.claudefordesktop", "Codex": "com.openai.codex", "OpenCode": "ai.opencode.desktop"]
    private static var icons: [String: NSImage] = [:]
    private static let face = Face.small(11, .medium)
    /// As tall as the project's tag beside it, so that the head is of one height with an icon and without. An app's
    /// icon keeps a margin of its own inside this square.
    private static let side = (19 + Face.larger).rounded()
    /// How tall it stands: as the tag in the folded line, larger beside the two lines of the head.
    static let inLine = side
    static let inHead: CGFloat = 30
    var tall = AppMark.side { didSet { if tall != oldValue { invalidateIntrinsicContentSize(); needsDisplay = true } } }
    private var icon: NSImage?
    var agent = "" {
        didSet {
            icon = AppMark.icon(of: agent)
            isHidden = agent.isEmpty
            toolTip = agent.isEmpty ? nil : agent
            invalidateIntrinsicContentSize()
            needsDisplay = true
        }
    }
    /// What stands in the icon's place when this Mac has no such app: the agent's name.
    var word: String { icon == nil ? agent : "" }

    init() {
        super.init(frame: .zero)
        isHidden = true
        setContentHuggingPriority(.required, for: .horizontal)
        setContentCompressionResistancePriority(.required, for: .horizontal)
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    private static func icon(of agent: String) -> NSImage? {
        if let known = icons[agent] { return known }
        guard let identifier = apps[agent], let app = NSWorkspace.shared.urlForApplication(withBundleIdentifier: identifier) else { return nil }
        let found = NSWorkspace.shared.icon(forFile: app.path)
        icons[agent] = found
        return found
    }

    private static var kept: Set<String> = []

    /// The same icon as a file, once a run, for the app on the phone: it has no way to this Mac's apps, and asks
    /// for the file as it asks for an item's picture.
    static func keep(_ agent: String, in folder: URL) {
        guard !agent.isEmpty, !kept.contains(agent), let icon = icon(of: agent) else { return }
        kept.insert(agent)
        let side = 96
        guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: side, pixelsHigh: side, bitsPerSample: 8, samplesPerPixel: 4,
                                            hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else { return }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
        icon.draw(in: NSRect(x: 0, y: 0, width: side, height: side), from: .zero, operation: .copy, fraction: 1)
        NSGraphicsContext.restoreGraphicsState()
        guard let picture = bitmap.representation(using: .png, properties: [:]) else { return }
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try? picture.write(to: folder.appendingPathComponent(agent + ".png"), options: .atomic)
    }

    override var intrinsicContentSize: NSSize {
        guard icon == nil else { return NSSize(width: tall, height: tall) }
        return NSSize(width: (agent as NSString).size(withAttributes: [.font: AppMark.face]).width.rounded(.up), height: tall)
    }

    override func draw(_ dirtyRect: NSRect) {
        if let icon {
            icon.draw(in: bounds, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: [.interpolation: NSImageInterpolation.high.rawValue])
            return
        }
        let attributes: [NSAttributedString.Key: Any] = [.font: AppMark.face, .foregroundColor: Ink.soft]
        let size = (agent as NSString).size(withAttributes: attributes)
        (agent as NSString).draw(at: NSPoint(x: 0, y: bounds.midY - size.height / 2), withAttributes: attributes)
    }
}

/// The project a check is about, by the name he knows it under, first in the window: «мне очень надо понять к чему
/// конкретно относится текущий запрос. в данном случае это hitl. но этого нигде нет». It stood only inside the
/// grey line of the folder, in the middle of a worktree's path.
final class Chip: NSView {
    private static let face = Face.small(11.5, .semibold)
    var text = "" {
        didSet {
            isHidden = text.isEmpty
            invalidateIntrinsicContentSize()
            needsDisplay = true
        }
    }

    init() {
        super.init(frame: .zero)
        isHidden = true
        setContentHuggingPriority(.required, for: .horizontal)
        setContentCompressionResistancePriority(.required, for: .horizontal)
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    override var intrinsicContentSize: NSSize {
        NSSize(width: ((text as NSString).size(withAttributes: [.font: Chip.face]).width).rounded(.up) + 14, height: (19 + Face.larger).rounded())
    }

    override func draw(_ dirtyRect: NSRect) {
        guard !text.isEmpty else { return }
        Ink.text.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: 6, yRadius: 6).fill()
        let attributes: [NSAttributedString.Key: Any] = [.font: Chip.face, .foregroundColor: Ink.panel]
        let size = (text as NSString).size(withAttributes: attributes)
        (text as NSString).draw(at: NSPoint(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2), withAttributes: attributes)
    }
}

/// His last answer to an item that is asked again, over the item: when he answered, with which answer, and what
/// he wrote, two lines of it at most. In half an hour one question came to him four times, each time as a new one,
/// with other chats' checks between; to the plate drawn over such a question he said «Да, показывать».
final class BeforePlate: NSView {
    private let caption = NSTextField(labelWithString: "")
    private let words = NSTextField(wrappingLabelWithString: "")
    private let stack = NSStackView()
    private var wide: NSLayoutConstraint!

    var said: (caption: String, note: String) = ("", "") {
        didSet {
            caption.stringValue = said.caption
            words.stringValue = said.note
            words.isHidden = said.note.isEmpty
            invalidateIntrinsicContentSize()
            needsDisplay = true
        }
    }

    init() {
        super.init(frame: .zero)
        caption.font = Face.small(11.5, .semibold)
        caption.textColor = Ink.before
        caption.lineBreakMode = .byTruncatingTail
        words.font = Face.details
        words.textColor = Ink.heading
        words.maximumNumberOfLines = 2
        words.cell?.truncatesLastVisibleLine = true
        words.isSelectable = true
        words.preferredMaxLayoutWidth = columnWide - 28
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 2
        stack.edgeInsets = NSEdgeInsets(top: 8, left: 15, bottom: 9, right: 13)
        stack.translatesAutoresizingMaskIntoConstraints = false
        stack.addArrangedSubview(caption)
        stack.addArrangedSubview(words)
        addSubview(stack)
        translatesAutoresizingMaskIntoConstraints = false
        wide = widthAnchor.constraint(equalToConstant: columnWide)
        NSLayoutConstraint.activate([wide,
                                     stack.leadingAnchor.constraint(equalTo: leadingAnchor), stack.trailingAnchor.constraint(equalTo: trailingAnchor),
                                     stack.topAnchor.constraint(equalTo: topAnchor), stack.bottomAnchor.constraint(equalTo: bottomAnchor)])
    }

    /// The column is another width: what he wrote breaks its lines by it.
    func widen() {
        wide.constant = columnWide
        words.preferredMaxLayoutWidth = columnWide - 28
        invalidateIntrinsicContentSize()
        needsDisplay = true
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 8, yRadius: 8)
        Ink.bar.setFill()
        shape.fill()
        Ink.edge.setStroke()
        shape.stroke()
        NSGraphicsContext.saveGraphicsState()
        shape.addClip()
        Ink.before.setFill()
        NSRect(x: 0, y: 0, width: 3, height: bounds.height).fill()
        NSGraphicsContext.restoreGraphicsState()
    }
}

/// A picture that came with an item: shown whole. A click, or its key, looks at it closer, on a card over the screen.
final class Picture: Tap {
    let path: String
    private let image: NSImage?
    private let size: NSSize
    /// The key that looks at it closer, on a cap in its corner.
    var key = "" { didSet { needsDisplay = true } }

    init(path: String, width: CGFloat, tallest: CGFloat) {
        self.path = path
        image = NSImage(contentsOfFile: path)
        let natural = image?.size ?? NSSize(width: 4, height: 3)
        let ratio = natural.width > 0 ? natural.height / natural.width : 0.75
        size = NSSize(width: width, height: min(max((width * ratio).rounded(), 60), tallest))
        super.init((path as NSString).lastPathComponent)
        toolTip = "Показать крупно"
    }

    /// A picture at a place the page gave it.
    init(path: String, image: NSImage, frame: NSRect) {
        self.path = path
        self.image = image
        size = frame.size
        super.init((path as NSString).lastPathComponent)
        translatesAutoresizingMaskIntoConstraints = true
        self.frame = frame
        toolTip = "Показать крупно"
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    override var intrinsicContentSize: NSSize { size }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 8, yRadius: 8)
        Ink.bar.setFill()
        shape.fill()
        if let image, image.size.width > 0, image.size.height > 0 {
            NSGraphicsContext.saveGraphicsState()
            shape.addClip()
            let scale = min(bounds.width / image.size.width, bounds.height / image.size.height)
            let fitted = NSSize(width: image.size.width * scale, height: image.size.height * scale)
            image.draw(in: NSRect(x: bounds.midX - fitted.width / 2, y: bounds.midY - fitted.height / 2, width: fitted.width, height: fitted.height),
                       from: .zero, operation: .sourceOver, fraction: looksPressed ? 0.8 : 1)
            NSGraphicsContext.restoreGraphicsState()
        }
        (isOver || isHeld ? Ink.soft : Ink.edge).setStroke()
        shape.stroke()
        guard !key.isEmpty else { return }
        // The cap stands in the picture's corner, on a patch of the panel's own colour: a picture may be of any.
        let cap = NSRect(x: bounds.maxX - 6 - KeyCap.width(key), y: bounds.maxY - 6 - 18, width: KeyCap.width(key), height: 18)
        Ink.panel.withAlphaComponent(0.92).setFill()
        NSBezierPath(roundedRect: cap.insetBy(dx: -3, dy: -3), xRadius: 7, yRadius: 7).fill()
        KeyCap.draw(key, rightEdge: cap.maxX, midY: cap.midY, live: keysLive, pressed: isHeld)
    }
}

/// Where he is in a check: a short stroke an item, coloured by its answer. A click on a stroke goes to its item:
/// «сделай эти сегменты кликабельными».
final class Segments: Tap {
    /// The colour of each item's answer; nil where there is none yet.
    var colors: [NSColor?] = [] { didSet { needsDisplay = true } }
    var current = 0 { didSet { needsDisplay = true } }
    /// A click on a stroke, with the place of its item.
    var pick: ((Int) -> Void)?
    /// The stroke the pointer is over: it stands as tall as the current one.
    private var pointed: Int? { didSet { if pointed != oldValue { needsDisplay = true } } }
    /// A stroke is too thin to aim at: a click this far over or under it is a click on it.
    private static let reach: CGFloat = 8

    init() { super.init("Пункты") }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    override var intrinsicContentSize: NSSize { NSSize(width: columnWide, height: 4) }

    /// The room for the pointer hangs out of the strokes' own place, so nothing in the window stands elsewhere for it.
    override var alignmentRectInsets: NSEdgeInsets { NSEdgeInsets(top: Segments.reach, left: 0, bottom: Segments.reach, right: 0) }

    /// The stroke at a point of the window. The gaps between the strokes belong to them: there is nowhere to miss.
    private func place(at point: NSPoint) -> Int? {
        let here = convert(point, from: nil)
        guard !colors.isEmpty, bounds.contains(here) else { return nil }
        return min(colors.count - 1, max(0, Int(here.x / bounds.width * CGFloat(colors.count))))
    }

    override func spot(of part: String) -> NSPoint? {
        guard let number = Int(part), colors.indices.contains(number - 1) else { return nil }
        return convert(NSPoint(x: bounds.width * (CGFloat(number) - 0.5) / CGFloat(colors.count), y: bounds.midY), to: nil)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .activeAlways, .inVisibleRect], owner: self))
    }

    override func mouseEntered(with event: NSEvent) {
        super.mouseEntered(with: event)
        pointed = place(at: event.locationInWindow)
    }

    override func mouseMoved(with event: NSEvent) { pointed = place(at: event.locationInWindow) }

    override func mouseExited(with event: NSEvent) {
        super.mouseExited(with: event)
        pointed = nil
    }

    override func mouseDragged(with event: NSEvent) {
        super.mouseDragged(with: event)
        pointed = place(at: event.locationInWindow)
    }

    override func mouseUp(with event: NSEvent) {
        // The stroke the button came up over is the one pressed.
        press = place(at: event.locationInWindow).map { number in { [weak self] in self?.pick?(number) } }
        super.mouseUp(with: event)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard !colors.isEmpty else { return }
        let gap: CGFloat = 2
        let width = (bounds.width - gap * CGFloat(colors.count - 1)) / CGFloat(colors.count)
        for (number, answered) in colors.enumerated() {
            let over = number == pointed && number != current
            (answered ?? (number == current ? Ink.text : over ? Ink.faint : Ink.rule)).setFill()
            let tall: CGFloat = number == current || over ? 4 : 2
            NSRect(x: CGFloat(number) * (width + gap), y: (bounds.height - tall) / 2, width: width, height: tall).fill()
        }
    }
}

/// What the task is, on a quiet card of its own: read once, then passed by the eye. His pick of 2026-10-20, «Задача —
/// отдельной спокойной карточкой, как в выбранном тобой варианте «Б»». The folder and the branch are not here: they
/// say who asks, and stand in the head.
final class Context: NSView {
    let title = NSTextField(wrappingLabelWithString: "")
    let about = Words(Face.meta, Ink.dim, room: contentWidth - 2 * Context.inset)
    private var wide: NSLayoutConstraint!
    private static let inset: CGFloat = 14

    init() {
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        title.font = Face.metaBold
        title.textColor = Ink.heading
        let stack = NSStackView(views: [title, about])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 2
        stack.edgeInsets = NSEdgeInsets(top: 9, left: Context.inset, bottom: 10, right: Context.inset)
        stack.translatesAutoresizingMaskIntoConstraints = false
        title.preferredMaxLayoutWidth = contentWidth - 2 * Context.inset
        title.isSelectable = true
        addSubview(stack)
        wide = widthAnchor.constraint(equalToConstant: contentWidth)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor), stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor), stack.bottomAnchor.constraint(equalTo: bottomAnchor),
            wide,
        ])
    }

    /// The column is another width: what the task is breaks its lines by it.
    func widen() {
        wide.constant = columnWide
        title.preferredMaxLayoutWidth = columnWide - 2 * Context.inset
        about.room = columnWide - 2 * Context.inset
        invalidateIntrinsicContentSize()
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 10, yRadius: 10)
        Ink.card.setFill()
        shape.fill()
        Ink.cardEdge.setStroke()
        shape.stroke()
    }
}

/// Where the check comes from, under the chat's name in the head: the folder and the branch, the branch after the
/// symbol of a branch («давай вместо "ветка" сделаем значок ветки гита»). One line, the folder cut in its middle when
/// it is long. It stood in the middle of what he reads, under the task; he wanted it «куда то убрать в более
/// подходящее место», in the window and not behind a button («хорошо бы держать в окошке в котором дофига пустого
/// места»), and not in a band over the bar («нет, это очень плохо»). The head says who asks: it says from where too.
final class Origin: NSTextField {
    /// The same line in plain words, the symbol as «⎇»: what a scripted run reads.
    private(set) var from = ""

    init() {
        super.init(frame: .zero)
        isEditable = false
        isSelectable = false
        isBordered = false
        drawsBackground = false
        font = Face.origin
        textColor = Ink.origin
        lineBreakMode = .byTruncatingMiddle
        maximumNumberOfLines = 1
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        setContentHuggingPriority(NSLayoutConstraint.Priority(1), for: .horizontal)
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    func come(from folder: String, branch: String) {
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byTruncatingMiddle
        let attributes: [NSAttributedString.Key: Any] = [.font: Face.origin, .foregroundColor: Ink.origin, .paragraphStyle: style]
        let line = NSMutableAttributedString(string: folder, attributes: attributes)
        var words = folder
        if !branch.isEmpty {
            let between = folder.isEmpty ? "" : "   "
            line.append(NSAttributedString(string: between, attributes: attributes))
            let look = NSImage.SymbolConfiguration(pointSize: Face.origin.pointSize - 1, weight: .medium).applying(.init(paletteColors: [Ink.origin]))
            if let symbol = NSImage(systemSymbolName: "arrow.triangle.branch", accessibilityDescription: "ветка")?.withSymbolConfiguration(look) {
                let attachment = NSTextAttachment()
                attachment.image = symbol
                // The symbol stands on the line of the letters, a little under it, as a letter with a tail does.
                attachment.bounds = NSRect(x: 0, y: -2, width: symbol.size.width, height: symbol.size.height)
                line.append(NSAttributedString(attachment: attachment))
                line.append(NSAttributedString(string: "\u{2009}", attributes: attributes))
            }
            line.append(NSAttributedString(string: branch, attributes: attributes))
            words += (folder.isEmpty ? "" : " · ") + "⎇ " + branch
        }
        line.addAttributes(attributes, range: NSRange(location: 0, length: line.length))
        attributedStringValue = line
        isHidden = words.isEmpty
        toolTip = words.isEmpty ? nil : words
        from = words
    }
}

/// The kind of the item in the head, in its colour on a tint of it: «Вопрос», «План». It takes the place of the dot
/// and of the coloured word that stood apart from it.
final class Pill: NSView {
    private static let face = Face.small(11, .semibold)
    var text = "" { didSet { isHidden = text.isEmpty; invalidateIntrinsicContentSize(); needsDisplay = true } }
    var tint = Ink.soft { didSet { needsDisplay = true } }
    /// The word is a trouble, not a kind: red, on no tint.
    var trouble = false { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    /// What it says, as a scripted run reads it.
    var stringValue: String { text }

    init() {
        super.init(frame: .zero)
        isHidden = true
        setContentHuggingPriority(.required, for: .horizontal)
        setContentCompressionResistancePriority(.required, for: .horizontal)
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    private var shown: String { trouble ? text : text.prefix(1).uppercased() + text.dropFirst() }

    override var intrinsicContentSize: NSSize {
        NSSize(width: ((shown as NSString).size(withAttributes: [.font: Pill.face]).width).rounded(.up) + (trouble ? 0 : 16),
               height: (19 + Face.larger).rounded())
    }

    override func draw(_ dirtyRect: NSRect) {
        guard !text.isEmpty else { return }
        if !trouble {
            (Ink.panel.blended(withFraction: 0.2, of: tint) ?? tint).setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 6, yRadius: 6).fill()
        }
        let ink = trouble ? NSColor.systemRed : tint.blended(withFraction: 0.45, of: .white) ?? tint
        let attributes: [NSAttributedString.Key: Any] = [.font: Pill.face, .foregroundColor: ink]
        let size = (shown as NSString).size(withAttributes: attributes)
        (shown as NSString).draw(at: NSPoint(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2), withAttributes: attributes)
    }
}
