#if canImport(UIKit)
import ConveyorCheck
import UIKit

/// What an action of the app's vocabulary is, said once by the app: the
/// panel offers it on a button of that kind wherever a check names the
/// action, and a host that shows the checklist too offers the same.
///
/// There are four kinds. A link opens a screen and shows its address. A
/// switch is two actions that undo each other. A choice is several actions of
/// which one holds at a time. An action is done once and is a symbol. An
/// action the app says nothing of is offered under its own id.
public struct DeviceCheckControl {
    /// One of the actions a choice chooses between.
    public struct Option {
        /// The action that chooses it.
        public let action: String
        /// A word for it, short: the options stand in a row.
        public let title: String
        /// An SF Symbol in the word's place on the panel.
        public let symbol: String?

        public init(_ action: String, title: String, symbol: String? = nil) {
            self.action = action
            self.title = title
            self.symbol = symbol
        }
    }

    enum Kind {
        case link(action: String, address: String)
        case toggle(on: String, off: String, symbol: String?, caption: String?, isOn: @MainActor () -> Bool)
        case choice(options: [Option], current: @MainActor () -> String?)
        case action(id: String, symbol: String)
    }

    /// What it is called in words: read out, and shown by a host where the
    /// panel shows a symbol.
    let name: String
    let kind: Kind

    /// The action opens a screen, and this is the screen's address: the
    /// button shows it without the app's scheme, «/screen/settings».
    public static func link(_ action: String, opens address: String) -> Self {
        .init(name: address, kind: .link(action: action, address: address))
    }

    /// Two actions that undo each other are one switch.
    ///
    /// - Parameters:
    ///   - name: What is switched, as it is when on: «Дока раскрыта».
    ///   - symbol: An SF Symbol that says it without words: prefer one that
    ///     is plain to read over a caption.
    ///   - caption: A word or two beside the switch, with the symbol or in
    ///     its place. With neither, the name stands there.
    ///   - isOn: How it stands now.
    public static func toggle(_ name: String, symbol: String? = nil, caption: String? = nil, on: String, off: String,
                              isOn: @escaping @MainActor () -> Bool) -> Self {
        .init(name: name, kind: .toggle(on: on, off: off, symbol: symbol, caption: caption, isOn: isOn))
    }

    /// Several actions of which one holds at a time are one row of options.
    ///
    /// - Parameters:
    ///   - name: What is chosen: «Лента».
    ///   - current: The action of the option that holds now, when one does.
    public static func choice(_ name: String, options: [Option], current: @escaping @MainActor () -> String?) -> Self {
        .init(name: name, kind: .choice(options: options, current: current))
    }

    /// An action done once is a button with an SF Symbol.
    public static func action(_ id: String, name: String, symbol: String) -> Self {
        .init(name: name, kind: .action(id: id, symbol: symbol))
    }

    /// The actions of the vocabulary this says something of.
    var actions: [String] {
        switch kind {
        case let .link(action, _): [action]
        case let .toggle(on, off, _, _, _): [on, off]
        case let .choice(options, _): options.map(\.action)
        case let .action(id, _): [id]
        }
    }

    /// An address as a button says it: without the scheme, which is the
    /// app's own and the same on every button.
    static func short(_ address: String) -> String {
        guard let scheme = address.range(of: "://") else { return address }
        let rest = String(address[scheme.upperBound...])
        return address.lowercased().hasPrefix("http") ? rest : "/" + rest
    }
}
#endif
