#if canImport(UIKit)
import ConveyorCheck
import UIKit

/// The hand check the host installed this build for, as the panel's
/// checklist: the items it asks about, and one button that ends the check,
/// «Готово» once every item has a mark and «Доработать» before that.
public final class DeviceCheckChecklist: ChecklistSource {
    /// The panel's own two actions; the rest are ids of the app's vocabulary.
    private static let setupAction = ">setup"
    private static let linkAction = ">link"

    private let store: DeviceCheckStore
    /// What the actions of the app's vocabulary are, as the app said it.
    private let controls: [DeviceCheckControl]
    /// Runs actions of the app's vocabulary by id; nil, or why it could not.
    private let run: ([String]) async -> String?
    /// Opens a deep link the item carries; false when the app has no such link.
    private let openLink: (URL) -> Bool
    private let onFinish: () -> Void
    private var isEnding = false
    private var watching: Timer?

    /// - Parameters:
    ///   - controls: What the actions of the app's vocabulary are: each is
    ///     offered on a button of its kind. An action none of them names is
    ///     offered under its id.
    ///   - run: Runs actions of the app's vocabulary by id; nil, or why it
    ///     could not, in words for the tester.
    ///   - openLink: Opens a deep link an item carries; false when the app
    ///     has no such link.
    ///   - onFinish: The tester ended the check.
    public init(store: DeviceCheckStore, controls: [DeviceCheckControl] = [], run: @escaping ([String]) async -> String?,
                openLink: @escaping (URL) -> Bool, onFinish: @escaping () -> Void) {
        self.store = store
        self.controls = controls
        self.run = run
        self.openLink = openLink
        self.onFinish = onFinish
        offer()
        // How a switch or a choice stands may change without a button of the check: by the tester's own hand
        // in the app, or a moment after an action has returned, as a new style on its way down the views. The
        // app is asked again and again, and the report says so only when something has changed.
        let stands = controls.contains { control in
            switch control.kind {
            case .toggle, .choice: true
            case .link, .action: false
            }
        }
        if stands {
            watching = Timer.scheduledTimer(withTimeInterval: 0.3, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.watch() }
            }
        }
    }

    isolated deinit { watching?.invalidate() }

    private func watch() {
        guard store.isActive else {
            watching?.invalidate()
            watching = nil
            return
        }
        offer()
    }

    /// The host may offer the same buttons on its own screen: it is told what
    /// they are, and again once an action has run, for a switch may stand
    /// the other way now.
    private func offer() {
        store.offer((store.check?.items ?? []).map { item in
            .init(label: item.label, actions: actions(of: item).map { action in
                switch action.look {
                case .words: .init(id: action.id, title: action.title)
                case .link: .init(id: action.id, title: action.title, kind: .link)
                case let .icon(symbol): .init(id: action.id, title: action.title, kind: .action, symbol: symbol)
                case let .toggle(symbol, _, isOn): .init(id: action.id, title: action.title, kind: .toggle, symbol: symbol, on: isOn)
                case let .choice(options, current):
                    .init(id: action.id, title: action.title, kind: .choice, options: options.map { .init(id: $0.id, title: $0.title) },
                          current: current)
                }
            })
        })
    }

    public var tag: String { store.check?.tag ?? "" }
    public var project: String { store.check?.project ?? "" }
    public var title: String { store.check?.title ?? "" }
    public var currentID: String? { store.currentLabel }
    public var footer: ChecklistPanelView.Footer? { store.report?.isComplete == true ? .done : .sendBack }
    public var canClose: Bool { false }

    public var items: [ChecklistPanelView.Item] {
        guard let check = store.check, let report = store.report else { return [] }
        return check.items.map { item in
            let result = report.result(of: item.label)
            return .init(id: item.label, number: "\(item.label).", text: item.text, actions: actions(of: item), state: result?.state,
                         note: result?.note)
        }
    }

    private func control(of action: String) -> DeviceCheckControl? {
        controls.first { $0.actions.contains(action) }
    }

    /// What the app can do for an item, one button each: the screen the item
    /// opened by itself, the switches and choices it set, the item's own
    /// buttons and its link. A switch or a choice is one button however many
    /// of its actions the item names.
    private func actions(of item: DeviceCheck.Item) -> [ChecklistPanelView.Action] {
        var actions: [ChecklistPanelView.Action] = []
        var shown: Set<String> = []
        func add(_ id: String) {
            guard let control = control(of: id) else { return actions.append(.init(id: id, title: id)) }
            guard shown.insert(control.actions[0]).inserted else { return }
            switch control.kind {
            case let .link(action, address): actions.append(.init(id: action, title: DeviceCheckControl.short(address), look: .link))
            case let .toggle(on, off, symbol, caption, isOn):
                let stands = isOn()
                actions.append(.init(id: stands ? off : on, title: control.name, look: .toggle(symbol: symbol, caption: caption, isOn: stands)))
            case let .choice(options, current):
                let holds = current()
                let place = options.firstIndex { $0.action == holds } ?? -1
                actions.append(.init(id: options[(place + 1) % options.count].action, title: control.name,
                                     look: .choice(options.map { .init(id: $0.action, title: $0.title, symbol: $0.symbol) }, current: holds)))
            case let .action(id, symbol): actions.append(.init(id: id, title: control.name, look: .icon(symbol)))
            }
        }
        if !item.setup.isEmpty {
            let opened = item.setup.compactMap { id -> String? in
                if case let .link(_, address) = control(of: id)?.kind { address } else { nil }
            }
            if item.setup.contains(where: { control(of: $0) == nil }) {
                // The app has not said what these actions are: the button says only that it prepares.
                actions.append(.init(id: Self.setupAction, title: "Подготовить", isPrimary: true))
            } else {
                // The screen the item opened by itself, by its address: a press brings the item's start back.
                if let address = opened.last {
                    actions.append(.init(id: Self.setupAction, title: DeviceCheckControl.short(address), isPrimary: true, look: .link))
                }
                for id in item.setup {
                    guard let control = control(of: id) else { continue }
                    if case .link = control.kind { shown.insert(control.actions[0]) } else { add(id) }
                }
            }
        }
        item.buttons.forEach(add)
        if let link = item.link { actions.append(.init(id: Self.linkAction, title: DeviceCheckControl.short(link), look: .link)) }
        return actions
    }

    public func mark(_ id: String, _ state: CheckMark?) { store.mark(id, state) }
    public func note(_ id: String, _ text: String) { store.note(id, text) }
    public func select(_ id: String) { store.select(id) }

    public func prepare(for id: String) async -> String? {
        guard let setup = item(id)?.setup, !setup.isEmpty else { return nil }
        return await act(setup)
    }

    public func perform(_ action: String, for id: String) async -> String? {
        guard let item = item(id) else { return nil }
        switch action {
        case Self.setupAction: return await act(item.setup)
        case Self.linkAction:
            guard let link = item.link, let url = URL(string: link), openLink(url) else {
                return "Приложение не открывает ссылку «\(item.link ?? "")»"
            }
            return nil
        default: return await act([action])
        }
    }

    /// Runs actions for the check. How a switch or a choice stood before the
    /// check first touched it is kept, to be put back when the check ends.
    private func act(_ actions: [String]) async -> String? {
        for action in actions {
            guard let control = control(of: action) else { continue }
            switch control.kind {
            case let .toggle(on, _, _, _, isOn): store.remember(on, isOn() ? "on" : "off")
            case let .choice(options, current): store.remember(options[0].action, current() ?? "")
            case .link, .action: break
            }
        }
        let failure = await run(actions)
        offer()
        return failure
    }

    /// Puts back what the check's buttons switched: every switch and choice
    /// as it stood before the check first touched it. What the check turns
    /// on is for the check, and does not stay with the app.
    public func restore() async {
        for (name, before) in store.remembered.sorted(by: { $0.key < $1.key }) {
            guard let control = control(of: name) else { continue }
            switch control.kind {
            case let .toggle(on, off, _, _, isOn):
                if isOn() != (before == "on") { _ = await run([before == "on" ? on : off]) }
            case let .choice(options, current):
                if current() != before, options.contains(where: { $0.action == before }) { _ = await run([before]) }
            case .link, .action: break
            }
        }
    }

    /// Ends the check, done or sent back: the host reads it off the device.
    public func footerTapped() {
        guard !isEnding else { return }
        guard !store.remembered.isEmpty else { return end() }
        isEnding = true
        Task {
            // Before the report says the check is over: after that the host may replace the app at any moment.
            await restore()
            end()
        }
    }

    private func end() {
        store.finish()
        UINotificationFeedbackGenerator().notificationOccurred(.success)
        onFinish()
    }

    private func item(_ id: String) -> DeviceCheck.Item? {
        store.check?.items.first { $0.label == id }
    }
}
#endif
