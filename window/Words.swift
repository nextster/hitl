// The texts of a check he reads, and the links a chat writes into them.

import AppKit

/// A link written into a text of a check, as in Markdown: «[team/320](https://…)» stands as «team/320», and a click
/// on those words opens the address. He sent a check whose text only named a task, «team/320», with «хотелось бы
/// чтобы в таких штуках ещё ссылки можно было делать прямо в тексте а ля как в маркдауне». An address is a page,
/// another app's own address or a file on this Mac; brackets around anything else are text as it is.
enum Linked {
    struct Piece: Equatable {
        var text: String
        var link: String?
    }

    /// The words in square brackets, then the address in round ones: no line break in either, and in the address
    /// no brackets but a pair of its own.
    private static let pattern = try? NSRegularExpression(pattern: #"\[([^\[\]\n]+)\]\(((?:[^()\n]|\([^()\n]*\))+)\)"#)

    static func isAddress(_ target: String) -> Bool {
        if target.hasPrefix("/") || target.hasPrefix("~/") { return true }
        return target.range(of: #"^[A-Za-z][A-Za-z0-9+.-]*:\S+$"#, options: .regularExpression) != nil
    }

    static func pieces(_ text: String) -> [Piece] {
        guard let pattern, text.contains("](") else { return text.isEmpty ? [] : [Piece(text: text, link: nil)] }
        let whole = text as NSString
        var pieces: [Piece] = []
        var from = 0
        for found in pattern.matches(in: text, range: NSRange(location: 0, length: whole.length)) {
            let words = whole.substring(with: found.range(at: 1)).trimmingCharacters(in: .whitespaces)
            let target = whole.substring(with: found.range(at: 2)).trimmingCharacters(in: .whitespaces)
            guard !words.isEmpty, isAddress(target) else { continue }
            if found.range.location > from {
                pieces.append(Piece(text: whole.substring(with: NSRange(location: from, length: found.range.location - from)), link: nil))
            }
            pieces.append(Piece(text: words, link: target))
            from = found.range.location + found.range.length
        }
        if from < whole.length { pieces.append(Piece(text: whole.substring(from: from), link: nil)) }
        return pieces
    }

    /// The text as he reads it: the links by their words.
    static func plain(_ text: String) -> String { pieces(text).map(\.text).joined() }

    /// How a link stands in a text, his pick of three: «Синяя с чертой». The pointer over it is a hand.
    static let look: [NSAttributedString.Key: Any] = [.foregroundColor: Ink.way, .underlineStyle: NSUnderlineStyle.single.rawValue,
                                                      .underlineColor: Ink.way.withAlphaComponent(0.55), .cursor: NSCursor.pointingHand]

    /// What stands for the cap of a link's key in a text, and before it a space that is never the end of a line:
    /// the cap stays on its link's line.
    private static let cap = "\u{FFFC}", glue = "\u{00A0}"

    /// A text of the window as he reads it: without what stands for the caps.
    static func read(_ shown: String) -> String {
        shown.replacingOccurrences(of: glue + cap, with: "").replacingOccurrences(of: cap, with: "")
    }

    /// A text with its links, each over its words: the address as the link itself, and in a tooltip. After a link
    /// stands the cap of its key, when `key` gives it one.
    static func text(_ said: String, _ attributes: [NSAttributedString.Key: Any], key: () -> String) -> NSAttributedString {
        let whole = NSMutableAttributedString()
        for piece in pieces(said) {
            guard let link = piece.link else {
                whole.append(NSAttributedString(string: piece.text, attributes: attributes))
                continue
            }
            var own = attributes
            own[.link] = link
            own[.toolTip] = link
            whole.append(NSAttributedString(string: piece.text, attributes: own))
            let name = key()
            guard !name.isEmpty else { continue }
            let cap = NSTextAttachment()
            cap.attachmentCell = CapCell(name)
            let stands = NSMutableAttributedString(string: glue)
            stands.append(NSAttributedString(attachment: cap))
            stands.addAttributes(attributes, range: NSRange(location: 0, length: stands.length))
            whole.append(stands)
        }
        return whole
    }
}

/// The cap of a key in a line of text, after the link the key opens: «И по ⌘ с цифрой». Smaller than a cap at the
/// right of a button, so that the line is as tall with it as without.
final class CapCell: NSTextAttachmentCell {
    private let key: String
    var live = false
    var pressed = false

    init(_ key: String) {
        self.key = key
        super.init()
    }

    required init(coder: NSCoder) { fatalError("the window is built in code") }

    override func cellSize() -> NSSize { NSSize(width: KeyCap.width(key, inText: true), height: KeyCap.tallInText) }
    /// It hangs under the line of the letters by no more than the smallest of the window's types does.
    override func cellBaselineOffset() -> NSPoint { NSPoint(x: 0, y: -3) }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView?) {
        KeyCap.draw(key, rightEdge: cellFrame.maxX, midY: cellFrame.midY, live: live, pressed: pressed, inText: true)
    }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView?, characterIndex charIndex: Int, layoutManager: NSLayoutManager) {
        draw(withFrame: cellFrame, in: controlView)
    }

    /// A press on the cap is not a pull at it: it is drawn, and that is all it is.
    override func wantsToTrackMouse() -> Bool { false }
}

/// A text of the check: selected by a pull of the mouse and copied, never typed in, and never given the keyboard
/// by the window itself. A click on a link in it opens the link.
final class ReadText: NSTextView {
    override var canBecomeKeyView: Bool { false }
    /// A link opens at the first click, wherever the keyboard is.
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    static func made(wide: CGFloat) -> ReadText {
        let text = ReadText(frame: NSRect(x: 0, y: 0, width: wide, height: 10))
        text.isEditable = false
        text.isSelectable = true
        text.drawsBackground = false
        text.textContainerInset = .zero
        text.textContainer?.lineFragmentPadding = 0
        text.textContainer?.widthTracksTextView = false
        text.textContainer?.containerSize = NSSize(width: wide, height: .greatestFiniteMagnitude)
        text.isVerticallyResizable = false
        text.isHorizontallyResizable = false
        text.selectedTextAttributes = [.backgroundColor: NSColor.selectedTextBackgroundColor]
        text.linkTextAttributes = Linked.look
        text.isAutomaticLinkDetectionEnabled = false
        return text
    }

    /// The window has the keyboard: the caps of the links' keys are lit.
    var capsLive = false {
        didSet {
            guard capsLive != oldValue else { return }
            caps.forEach { $0.live = capsLive }
            needsDisplay = true
        }
    }

    func show(_ whole: NSAttributedString) {
        textStorage?.setAttributedString(whole)
        setSelectedRange(NSRange(location: 0, length: 0))
        caps.forEach { $0.live = capsLive }
    }

    /// The caps that stand after its links, in their order.
    private var caps: [CapCell] {
        var found: [CapCell] = []
        guard let storage = textStorage else { return found }
        storage.enumerateAttribute(.attachment, in: NSRange(location: 0, length: storage.length)) { attachment, _, _ in
            if let cap = (attachment as? NSTextAttachment)?.attachmentCell as? CapCell { found.append(cap) }
        }
        return found
    }

    /// The key of its link, by the link's place, went down or came up: its cap is white while it is down.
    func press(cap place: Int, _ down: Bool) {
        let all = caps
        guard all.indices.contains(place) else { return }
        all[place].pressed = down
        needsDisplay = true
    }

    /// What he reads.
    var read: String { Linked.read(string) }

    /// The words of its links, in their order.
    var linkWords: [String] {
        var words: [String] = []
        guard let storage = textStorage else { return words }
        storage.enumerateAttribute(.link, in: NSRange(location: 0, length: storage.length)) { link, range, _ in
            if link != nil { words.append(storage.attributedSubstring(from: range).string) }
        }
        return words
    }

    /// Where the link with those words begins, in the window: the middle of its first letter.
    func spot(ofLink words: String) -> NSPoint? {
        guard let storage = textStorage, let manager = layoutManager, let container = textContainer else { return nil }
        var spot: NSPoint?
        storage.enumerateAttribute(.link, in: NSRange(location: 0, length: storage.length)) { link, range, stop in
            guard link != nil, storage.attributedSubstring(from: range).string == words else { return }
            let first = manager.glyphRange(forCharacterRange: NSRange(location: range.location, length: 1), actualCharacterRange: nil)
            let letter = manager.boundingRect(forGlyphRange: first, in: container)
            spot = convert(NSPoint(x: letter.midX + textContainerOrigin.x, y: letter.midY + textContainerOrigin.y), to: nil)
            stop.pointee = true
        }
        return spot
    }

    /// What is copied is what he reads: no trace of the caps.
    override func copy(_ sender: Any?) {
        let whole = string as NSString
        let picked = Linked.read(selectedRanges.map { whole.substring(with: $0.rangeValue) }.joined(separator: "\n"))
        guard !picked.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(picked, forType: .string)
    }

    var usedTall: CGFloat {
        guard let manager = layoutManager, let container = textContainer else { return 0 }
        manager.ensureLayout(for: container)
        return manager.usedRect(for: container).height
    }

    var usedWide: CGFloat {
        guard let manager = layoutManager, let container = textContainer else { return 0 }
        manager.ensureLayout(for: container)
        return manager.usedRect(for: container).maxX
    }
}

/// A text of the check that may hold links: its links from its start, and the keys the window gives them.
protocol LinkedText: NSView {
    var text: ReadText { get }
    var links: [(words: String, address: String)] { get }
    /// The keys of its links, in their order, "" for a link that has none: a cap stands after each link with one.
    var keys: [String] { get set }
}

/// The key of a link that stands in a text. It is not on the window: its cap in the text is, and is pressed with it.
final class TextKey: Tap {
    var key = ""
    var address = ""
    var lit: ((Bool) -> Void)?
    override var isHeld: Bool { didSet { lit?(isHeld) } }
}

/// A text of the check that stands by itself: the question, what stands under it, what the task is. It was a
/// label; a label does not open a link at a click.
final class Words: NSView, NSTextViewDelegate, LinkedText {
    let text: ReadText
    let font: NSFont
    private let ink: NSColor
    /// How much taller a label's line of this font is than a text view's, a point for some sizes: the lines stand
    /// where they stood in the label, at the heights he set the window's type by.
    private let taller: CGFloat
    /// A click on a link, with its address.
    var open: ((String) -> Void)?
    /// How wide the text may stand.
    var room: CGFloat { didSet { if room != oldValue { fit() } } }
    /// What the chat wrote, its links as it wrote them.
    var said = "" { didSet { if said != oldValue { fill() } } }
    var keys: [String] = [] { didSet { if keys != oldValue { fill() } } }
    /// What he reads.
    var stringValue: String { text.read }
    var links: [(words: String, address: String)] { Linked.pieces(said).compactMap { piece in piece.link.map { (piece.text, $0) } } }

    init(_ font: NSFont, _ ink: NSColor, room: CGFloat) {
        self.font = font
        self.ink = ink
        self.room = room
        text = ReadText.made(wide: room)
        let label = NSTextField(labelWithString: "Щу")
        label.font = font
        taller = max(0, label.intrinsicContentSize.height - (text.layoutManager?.defaultLineHeight(for: font) ?? label.intrinsicContentSize.height))
        super.init(frame: .zero)
        text.delegate = self
        addSubview(text)
        translatesAutoresizingMaskIntoConstraints = false
        setContentHuggingPriority(.defaultHigh, for: .vertical)
        setContentCompressionResistancePriority(.required, for: .vertical)
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    override var isFlipped: Bool { true }

    private func fill() {
        let style = NSMutableParagraphStyle()
        style.lineSpacing = Face.lineRoom + taller
        var place = 0
        text.show(Linked.text(said, [.font: font, .foregroundColor: ink, .paragraphStyle: style]) {
            place += 1
            return keys.indices.contains(place - 1) ? keys[place - 1] : ""
        })
        fit()
    }

    private func fit() {
        text.textContainer?.containerSize = NSSize(width: room, height: .greatestFiniteMagnitude)
        text.frame = NSRect(origin: .zero, size: intrinsicContentSize)
        invalidateIntrinsicContentSize()
        needsDisplay = true
    }

    /// As wide as its lines and as tall as they stand.
    override var intrinsicContentSize: NSSize {
        guard !said.isEmpty else { return .zero }
        return NSSize(width: min(room, (text.usedWide * 2).rounded(.up) / 2), height: text.usedTall + taller)
    }

    override var firstBaselineOffsetFromTop: CGFloat { text.layoutManager?.defaultBaselineOffset(for: font) ?? font.ascender }

    func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        open?((link as? URL)?.absoluteString ?? String(describing: link))
        return true
    }
}
