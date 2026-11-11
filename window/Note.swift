// The comment: its text with a placeholder, the line it is written on, a screenshot under it.

import AppKit

/// The comment's text: a plain text view with a placeholder.
final class NoteText: NSTextView {
    var placeholder = ""
    var focusChanged: (() -> Void)?

    /// The comment gets the cursor by a click into it or from the window, for a typed letter or a "no".
    /// It is not what a window picks for itself when it gets the keyboard: then the keys answer.
    override var canBecomeKeyView: Bool { false }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard string.isEmpty else { return }
        let padding = textContainer?.lineFragmentPadding ?? 0
        placeholder.draw(at: NSPoint(x: textContainerInset.width + padding, y: textContainerInset.height),
                         withAttributes: [.font: font ?? Face.details, .foregroundColor: Ink.faint])
    }

    override func becomeFirstResponder() -> Bool {
        let became = super.becomeFirstResponder()
        focusChanged?()
        return became
    }

    override func resignFirstResponder() -> Bool {
        let resigned = super.resignFirstResponder()
        focusChanged?()
        return resigned
    }
}

/// The comment field: a framed field to write in («Комментарий — поле в рамке», 2026-10-20). It grows with its text
/// and never scrolls.
final class NoteView: NSView, NSTextViewDelegate {
    let text = NoteText(usingTextLayoutManager: false)
    var changed: (() -> Void)?
    var tint = Ink.rule { didSet { needsDisplay = true } }
    private var height: NSLayoutConstraint!
    private var wide: NSLayoutConstraint!

    init(width: CGFloat) {
        super.init(frame: NSRect(x: 0, y: 0, width: width, height: 30))
        text.font = Face.small(13.5)
        text.textColor = Ink.text
        text.insertionPointColor = Ink.text
        text.drawsBackground = false
        text.isRichText = false
        text.allowsUndo = true
        text.isAutomaticQuoteSubstitutionEnabled = false
        text.isAutomaticDashSubstitutionEnabled = false
        text.isAutomaticTextReplacementEnabled = false
        text.textContainerInset = NSSize(width: NoteView.inset, height: 8)
        text.textContainer?.lineFragmentPadding = 2
        text.isVerticallyResizable = false
        text.isHorizontallyResizable = false
        text.textContainer?.widthTracksTextView = false
        text.textContainer?.heightTracksTextView = false
        text.textContainer?.containerSize = NSSize(width: width - 2 * NoteView.inset, height: .greatestFiniteMagnitude)
        text.delegate = self
        text.translatesAutoresizingMaskIntoConstraints = false
        addSubview(text)
        translatesAutoresizingMaskIntoConstraints = false
        height = heightAnchor.constraint(equalToConstant: 30)
        wide = widthAnchor.constraint(equalToConstant: width)
        NSLayoutConstraint.activate([
            text.leadingAnchor.constraint(equalTo: leadingAnchor), text.trailingAnchor.constraint(equalTo: trailingAnchor),
            text.topAnchor.constraint(equalTo: topAnchor), text.bottomAnchor.constraint(equalTo: bottomAnchor),
            wide, height,
        ])
        fit()
    }

    /// The column is another width: the comment breaks its lines by it.
    func widen(to width: CGFloat) {
        guard wide.constant != width else { return }
        wide.constant = width
        text.textContainer?.containerSize = NSSize(width: width - 2 * NoteView.inset, height: .greatestFiniteMagnitude)
        fit()
        text.needsDisplay = true
    }

    /// How far the text stands in from the field's frame.
    static let inset: CGFloat = 10

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    var string: String {
        get { text.string }
        set {
            guard text.string != newValue else { return }
            text.string = newValue
            fit()
            text.needsDisplay = true
        }
    }

    /// As tall as its lines, one line when empty.
    private func fit() {
        guard let manager = text.layoutManager, let container = text.textContainer, let font = text.font else { return }
        manager.ensureLayout(for: container)
        let lines = max(manager.usedRect(for: container).height, manager.defaultLineHeight(for: font))
        height.constant = lines.rounded(.up) + 2 * text.textContainerInset.height
    }

    func textDidChange(_ notification: Notification) {
        fit()
        text.needsDisplay = true
        changed?()
    }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 9, yRadius: 9)
        Ink.field.setFill()
        shape.fill()
        (window?.firstResponder === text ? Ink.edge.blended(withFraction: 0.55, of: tint) ?? tint : Ink.edge).setStroke()
        shape.stroke()
    }
}

/// A screenshot under the comment: a click looks at it closer, the cross takes it away.
final class ShotView: NSView {
    private let image: NSImage?
    private let cross = NSButton(title: "", target: nil, action: nil)
    var open: (() -> Void)?
    var remove: (() -> Void)?

    init(path: String) {
        image = NSImage(contentsOfFile: path)
        super.init(frame: NSRect(x: 0, y: 0, width: 96, height: 60))
        translatesAutoresizingMaskIntoConstraints = false
        cross.isBordered = false
        cross.attributedTitle = NSAttributedString(string: "✕", attributes: [.font: NSFont.systemFont(ofSize: 9, weight: .bold),
                                                                             .foregroundColor: NSColor.white])
        cross.refusesFirstResponder = true
        cross.target = self
        cross.action = #selector(crossed)
        cross.toolTip = "Убрать снимок"
        cross.translatesAutoresizingMaskIntoConstraints = false
        addSubview(cross)
        toolTip = "Показать крупно"
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: 96), heightAnchor.constraint(equalToConstant: 60),
            cross.topAnchor.constraint(equalTo: topAnchor, constant: 3), cross.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -3),
            cross.widthAnchor.constraint(equalToConstant: 16), cross.heightAnchor.constraint(equalToConstant: 16),
        ])
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    override func mouseDown(with event: NSEvent) {}

    override func mouseUp(with event: NSEvent) {
        if bounds.contains(convert(event.locationInWindow, from: nil)) { open?() }
    }

    @objc private func crossed() { remove?() }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 6, yRadius: 6)
        Ink.key.setFill()
        shape.fill()
        if let image, image.size.width > 0, image.size.height > 0 {
            NSGraphicsContext.saveGraphicsState()
            shape.addClip()
            let scale = min(bounds.width / image.size.width, bounds.height / image.size.height)
            let size = NSSize(width: image.size.width * scale, height: image.size.height * scale)
            image.draw(in: NSRect(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2, width: size.width, height: size.height),
                       from: .zero, operation: .sourceOver, fraction: 1)
            NSGraphicsContext.restoreGraphicsState()
        }
        Ink.keyEdge.setStroke()
        shape.stroke()
        NSColor.black.withAlphaComponent(0.6).setFill()
        NSBezierPath(ovalIn: cross.frame).fill()
    }
}
