// The look of the window: its colours, the colour of each kind of check, the fonts, the cap with a key's name.

import AppKit

// MARK: The look

enum Ink {
    static func hex(_ value: UInt32, _ alpha: CGFloat = 1) -> NSColor {
        NSColor(srgbRed: CGFloat((value >> 16) & 0xFF) / 255, green: CGFloat((value >> 8) & 0xFF) / 255,
                blue: CGFloat(value & 0xFF) / 255, alpha: alpha)
    }
    static let panel = hex(0x1A1B1F)
    static let edge = hex(0x2D2E35)
    static let bar = hex(0x15161A)
    static let barEdge = hex(0x2A2B31)
    static let text = hex(0xF3F3F5)
    static let heading = lifted(0xC9CAD2)
    static let soft = hex(0xA4A6B0)
    static let quiet = lifted(0x9A9CA7)
    static let dim = lifted(0x8F919C)
    static let faint = hex(0x6F717C)
    static let link = hex(0xB9BBC5)
    static let rule = hex(0x33343C)
    static let hover = hex(0x24252B)
    static let key = hex(0x2A2B32)
    static let keyEdge = hex(0x383942)
    static let keyLit = hex(0x41434F)
    static let keyLitEdge = hex(0x5E6172)
    static let no = hex(0xFF5A52)
    /// The task's card, an answer's button and the comment's field: what stands apart from the panel by its own ground.
    static let card = hex(0x1F2025)
    static let cardEdge = hex(0x25262C)
    static let surface = hex(0x212228)
    static let field = hex(0x141519)
    /// The line under the head, between who asks and what is asked.
    static let hair = hex(0x26272D)
    /// The folder and the branch in the head: quieter than the chat's name, but there to be read.
    static let origin = hex(0x9A9CA7)
    /// A link in a text he reads: what a click on takes him somewhere.
    static let way = hex(0x6CA6FF)
    /// His own earlier answer, over an item asked again.
    static let before = hex(0xFF9F0A)
    /// «Не понял»: the colour of an explanation, which is what it asks for.
    static let unclear = hex(0xFFD60A)

    /// How far the grey of a text he reads stands on the way to white: what stands under a question, the facts,
    /// what the task is. Drawn in the grey itself it was hard to read («читается плоховато»); 0.6 is what he set.
    static let lift: CGFloat = 0.6
    static func lifted(_ value: UInt32) -> NSColor { hex(value).blended(withFraction: lift, of: text) ?? hex(value) }
}

/// Each kind of check has its colour: the kind in the head, the progress, the chosen answer.
func tint(of kind: String?) -> NSColor {
    switch kind {
    case "question": return Ink.hex(0x4C8DFF)
    case "plan": return Ink.hex(0xA78BFA)
    case "action": return Ink.hex(0xFF9F0A)
    case "result": return Ink.hex(0x34C759)
    case "explain": return Ink.hex(0xFFD60A)
    default: return Ink.hex(0x22B8A0)
    }
}

enum Face {
    /// What he set on a live page on 2026-10-11, after «читается плоховато»: the size of an answer, how much larger
    /// than it was first drawn everything else is (what stands under a question was 13 points and is 14.5), and the
    /// room between the lines of a text of several. The question was 16 then; it is 19 since the redesign he took
    /// on 2026-10-20 («Вопрос — главное в окне: 19 пунктов вместо 16, белым»), so that the eye finds it first.
    static let questionSize: CGFloat = 19
    static let rowSize: CGFloat = 15
    static let larger: CGFloat = 1.5
    static let lineRoom: CGFloat = 2.5
    /// An answer's button: its text and the room around it, a button of its own since the redesign.
    static let rowTall: CGFloat = 36

    static func of(_ size: CGFloat, _ weight: NSFont.Weight = .regular) -> NSFont { NSFont.systemFont(ofSize: size, weight: weight) }
    /// A text that is not the question or an answer, by the size it was first drawn at.
    static func small(_ size: CGFloat, _ weight: NSFont.Weight = .regular) -> NSFont { of(size + larger, weight) }
    static func digits(_ size: CGFloat) -> NSFont { NSFont.monospacedDigitSystemFont(ofSize: size + larger, weight: .regular) }

    static let meta = small(12.5)
    static let metaBold = small(12.5, .semibold)
    /// The chat's name in the head, and the folder and branch under it.
    static let chat = of(15, .medium)
    static let origin = small(11)
    static let question = of(questionSize, .semibold)
    static let details = small(13)
    static let strong = small(13, .medium)
    static let row = of(rowSize)
    static let hint = small(12)
    static let sent = small(11.5)
    static let key = NSFont.monospacedSystemFont(ofSize: 11, weight: .medium)

    static let oneLine: NSParagraphStyle = {
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byTruncatingTail
        return style
    }()
}

/// The name of a key: a small cap at the right of what the key presses. It is lit while the window has the
/// keyboard and the key works, and a dim outline while it does not: that is all that says where the keyboard is.
enum KeyCap {
    /// In a line of text, after a link, the cap is smaller: the line is as tall with it as without.
    static let tallInText: CGFloat = 15
    private static let faceInText = NSFont.monospacedSystemFont(ofSize: 9.5, weight: .medium)

    static func width(_ text: String, inText: Bool = false) -> CGFloat {
        text.isEmpty ? 0 : ((text as NSString).size(withAttributes: [.font: inText ? faceInText : Face.key]).width).rounded(.up) + (inText ? 9 : 12)
    }

    /// The cap at the start of an answer's button: as tall as its text, and at least square.
    static let inRow: CGFloat = 22
    static func widthInRow(_ text: String) -> CGFloat { text.isEmpty ? 0 : max(inRow, width(text)) }

    /// `pressed`: the key is down right now, and the cap is white like a key under a finger. `chosen`: the answer it
    /// presses is the one chosen, and the cap is filled with that colour. `tall`: a cap of an answer's button.
    static func draw(_ text: String, rightEdge: CGFloat, midY: CGFloat, live: Bool, pressed: Bool = false, inText: Bool = false,
                     chosen: NSColor? = nil, tall wanted: CGFloat? = nil) {
        guard !text.isEmpty else { return }
        let wide = wanted == nil ? width(text, inText: inText) : widthInRow(text), tall: CGFloat = wanted ?? (inText ? tallInText : 18)
        let rect = NSRect(x: rightEdge - wide, y: midY - tall / 2, width: wide, height: tall)
        let round: CGFloat = inText ? 4 : wanted == nil ? 5 : 6
        let shape = NSBezierPath(roundedRect: rect.insetBy(dx: 0.5, dy: 0.5), xRadius: round, yRadius: round)
        if let chosen, !pressed {
            chosen.setFill()
            shape.fill()
        } else if pressed || live {
            (pressed ? Ink.text : Ink.keyLit).setFill()
            shape.fill()
        }
        (chosen != nil && !pressed ? chosen! : pressed ? Ink.text : live ? Ink.keyLitEdge : Ink.keyEdge).setStroke()
        shape.stroke()
        let attributes: [NSAttributedString.Key: Any] = [.font: inText ? faceInText : Face.key,
                                                         .foregroundColor: pressed || chosen != nil ? Ink.panel : live ? Ink.text : Ink.faint]
        let size = (text as NSString).size(withAttributes: attributes)
        (text as NSString).draw(at: NSPoint(x: rect.midX - size.width / 2, y: rect.midY - size.height / 2), withAttributes: attributes)
    }
}
