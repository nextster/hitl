#if canImport(UIKit)
import UIKit

/// How the panel looks. It is black whatever the app looks like, so that it
/// reads as one piece with the Dynamic Island it stands around; the app gives
/// its accent and its font.
public struct ChecklistPanelStyle {
    /// The colour of the button that ends a checklist with every item marked.
    public var accent: UIColor
    /// The app's font at a size and a weight.
    public var makeFont: (CGFloat, UIFont.Weight) -> UIFont

    /// The panel's own colours, for a source that lays out an item's detail.
    public static let text = UIColor.white
    public static let secondaryText = UIColor(white: 0.62, alpha: 1)
    static let surface = UIColor.black
    /// Under a comment.
    static let field = UIColor(white: 0.11, alpha: 1)
    /// Under a round button.
    static let control = UIColor(white: 0.17, alpha: 1)
    /// Under the option of a choice that holds.
    static let chosen = UIColor(white: 0.36, alpha: 1)

    public init(accent: UIColor = .tintColor,
                font makeFont: @escaping (CGFloat, UIFont.Weight) -> UIFont = { .systemFont(ofSize: $0, weight: $1) }) {
        self.accent = accent
        self.makeFont = makeFont
    }

    func font(_ size: CGFloat, weight: UIFont.Weight = .regular) -> UIFont { makeFont(size, weight) }
}
#endif
