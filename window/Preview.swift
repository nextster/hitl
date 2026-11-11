// A picture looked at closer: the card it stands on in the middle of the screen, and the panel that holds the card.

import AppKit

/// The card: the picture as large as it fits, and under it which of the item's pictures this is and the keys.
/// A click on an arrow goes to the next picture, a click anywhere else puts the card away.
final class Preview: NSView {
    static let pad: CGFloat = 12
    static let bar: CGFloat = 44
    private static let narrowest: CGFloat = 340

    var image: NSImage? { didSet { needsDisplay = true } }
    /// Which of the item's pictures this is, and of how many; one picture has no arrows.
    var number = 0 { didSet { needsDisplay = true } }
    var count = 1 { didSet { needsDisplay = true } }
    var close: (() -> Void)?
    var turn: ((Int) -> Void)?

    /// How large the card is for a picture of this size, in this much room of the screen.
    static func size(for picture: NSSize, in room: NSSize) -> NSSize {
        guard picture.width > 0, picture.height > 0 else { return NSSize(width: narrowest, height: 240) }
        let ratio = min((room.width - 2 * pad) / picture.width, (room.height - pad - bar) / picture.height, 4)
        return NSSize(width: max((picture.width * ratio).rounded(), narrowest - 2 * pad) + 2 * pad,
                      height: max((picture.height * ratio).rounded(), 120) + pad + bar)
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    private var many: Bool { count > 1 }
    private var back: NSRect { NSRect(x: bounds.midX - 92, y: 6, width: 44, height: Preview.bar - 12) }
    private var forth: NSRect { NSRect(x: bounds.midX + 48, y: 6, width: 44, height: Preview.bar - 12) }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 14, yRadius: 14)
        Ink.panel.setFill()
        shape.fill()
        Ink.keyLitEdge.withAlphaComponent(0.7).setStroke()
        shape.stroke()

        let room = NSRect(x: Preview.pad, y: Preview.bar, width: bounds.width - 2 * Preview.pad, height: bounds.height - Preview.bar - Preview.pad)
        if let image, image.size.width > 0, image.size.height > 0 {
            let ratio = min(room.width / image.size.width, room.height / image.size.height)
            let fitted = NSSize(width: (image.size.width * ratio).rounded(.down), height: (image.size.height * ratio).rounded(.down))
            let rect = NSRect(x: (room.midX - fitted.width / 2).rounded(), y: (room.midY - fitted.height / 2).rounded(),
                              width: fitted.width, height: fitted.height)
            NSGraphicsContext.saveGraphicsState()
            NSBezierPath(roundedRect: rect, xRadius: 8, yRadius: 8).addClip()
            Ink.bar.setFill()
            rect.fill()
            image.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1)
            NSGraphicsContext.restoreGraphicsState()
        }

        let midY = Preview.bar / 2
        func write(_ text: String, _ font: NSFont, _ ink: NSColor, centredAt x: CGFloat) {
            let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: ink]
            let size = (text as NSString).size(withAttributes: attributes)
            (text as NSString).draw(at: NSPoint(x: (x - size.width / 2).rounded(), y: (midY - size.height / 2).rounded()), withAttributes: attributes)
        }
        if many {
            write("\(number + 1) из \(count)", NSFont.monospacedDigitSystemFont(ofSize: 13, weight: .medium), Ink.text, centredAt: bounds.midX)
            write("‹", NSFont.systemFont(ofSize: 20), number > 0 ? Ink.link : Ink.rule, centredAt: back.midX)
            write("›", NSFont.systemFont(ofSize: 20), number < count - 1 ? Ink.link : Ink.rule, centredAt: forth.midX)
            // The keys that do the same stand beside the arrows.
            KeyCap.draw("←", rightEdge: back.minX - 2, midY: midY, live: true)
            KeyCap.draw("→", rightEdge: forth.maxX + 2 + KeyCap.width("→"), midY: midY, live: true)
        }
        KeyCap.draw("esc", rightEdge: bounds.width - Preview.pad, midY: midY, live: true)
    }

    override func mouseDown(with event: NSEvent) {}

    override func mouseUp(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        if many, back.insetBy(dx: -30, dy: -6).contains(point) {
            turn?(-1)
        } else if many, forth.insetBy(dx: -30, dy: -6).contains(point) {
            turn?(1)
        } else {
            close?()
        }
    }
}

/// What holds the card on the screen. It never takes the keyboard: the keys stay with the check window, which is
/// what the keys of the card are read by.
final class PreviewPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}
