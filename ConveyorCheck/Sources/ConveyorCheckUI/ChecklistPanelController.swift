#if canImport(UIKit)
import ConveyorCheck
import UIKit

/// What a checklist panel shows, and what its marks and buttons change. The
/// hand check from a host is one source (`DeviceCheckChecklist`); an app can
/// show a checklist of its own in the same panel with another.
@MainActor
public protocol ChecklistSource: AnyObject {
    /// Whose checklist it is: the tag the host gave, or the app's own.
    var tag: String { get }
    /// The project the checklist is about, shown before the tag; "" when there is none to say.
    var project: String { get }
    var title: String { get }
    var items: [ChecklistPanelView.Item] { get }
    /// The item the one-at-a-time view is on.
    var currentID: String? { get }
    var footer: ChecklistPanelView.Footer? { get }
    /// Whether the checklist can be put away without ending it.
    var canClose: Bool { get }

    func mark(_ id: String, _ state: CheckMark?)
    func note(_ id: String, _ text: String)
    func select(_ id: String)
    /// Prepares the app for an item the tester has come to. Nil when it did,
    /// or had nothing to do; otherwise why it could not.
    func prepare(for id: String) async -> String?
    /// Runs one of the item's actions; nil, or why it could not.
    func perform(_ action: String, for id: String) async -> String?
    func footerTapped()
}

public extension ChecklistSource {
    /// A checklist of the app's own is about the app itself, and says no project.
    var project: String { "" }
}

/// The window of a checklist. It sits above the app, so sheets and cards open
/// under it, and lets every touch outside the panel through to the app being
/// checked.
public final class ChecklistPanelWindow: UIWindow {
    public override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        guard let hit = super.hitTest(point, with: event), let root = rootViewController else { return nil }
        // A sheet the panel presents takes the whole window.
        if root.presentedViewController != nil { return hit }
        return hit === root.view ? nil : hit
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        fitScene()
    }

    /// The whole scene: the panel stands where the status bar was, and UIKit
    /// asks the full-screen window on top whether the status bar shows.
    public func fitScene() {
        guard let bounds = windowScene?.coordinateSpace.bounds else { return }
        if frame != bounds { frame = bounds }
        // The scene may have a new size: the app's window is placed in it again.
        (rootViewController as? ChecklistPanelController)?.makeRoom()
    }

    /// A panel out of sight takes no room from the app.
    public override var isHidden: Bool {
        didSet { (rootViewController as? ChecklistPanelController)?.makeRoom() }
    }
}

/// The room the app's window gives to the bands over it. An app may show two
/// checklists at once, its own under the host's: its window starts under the
/// one that reaches lower.
///
/// A window that starts under the band has nothing above it to keep clear
/// of, but a view controller whose view fills the window is still given the
/// status bar's inset, as if the window stood at the top of the screen. That
/// inset is taken back from each such controller for as long as the window is
/// moved. The other way round, a window that kept the inset under the band,
/// had the system hand a presented screen a smaller inset for a moment while
/// it came up, and a card laid out in that moment stood under the band. Taken
/// back late, the inset is only an empty strip under the band: nothing of
/// the app is ever behind it.
enum ChecklistRoom {
    private static var taken: [ObjectIdentifier: [ObjectIdentifier: CGFloat]] = [:]
    /// What was added to a controller's own top inset: minus the system's.
    private static var given: [ObjectIdentifier: (controller: () -> UIViewController?, top: CGFloat)] = [:]
    private static var windows: [ObjectIdentifier: () -> UIWindow?] = [:]
    private static var watch: Timer?

    static func take(_ down: CGFloat, from appWindow: UIWindow, for band: AnyObject) {
        let window = ObjectIdentifier(appWindow)
        var bands = taken[window] ?? [:]
        bands[ObjectIdentifier(band)] = down > 0 ? down : nil
        taken[window] = bands.isEmpty ? nil : bands
        windows[window] = bands.isEmpty ? nil : { [weak appWindow] in appWindow }
        guard let whole = appWindow.windowScene?.coordinateSpace.bounds else { return }
        let most = bands.values.max() ?? 0
        let room = CGRect(x: whole.minX, y: whole.minY + most, width: whole.width, height: whole.height - most)
        if appWindow.frame != room { appWindow.frame = room }
        settle(appWindow)
        // Screens the app presents later are given the same; looked for at every frame or two, while a band takes room.
        if taken.isEmpty {
            watch?.invalidate()
            watch = nil
        } else if watch == nil {
            watch = Timer.scheduledTimer(withTimeInterval: 1.0 / 30, repeats: true) { _ in
                MainActor.assumeIsolated { windows.values.compactMap { $0() }.forEach(settle) }
            }
        }
    }

    /// Takes the status bar's inset back from the controllers whose views fill a moved window, and gives it
    /// back to those of a window that is whole again, or that are gone from it.
    private static func settle(_ appWindow: UIWindow) {
        let moved = taken[ObjectIdentifier(appWindow)] != nil
        var filling: [UIViewController] = []
        var next = appWindow.rootViewController
        while let controller = next {
            if controller.isViewLoaded, controller.view.window === appWindow,
               abs(controller.view.convert(controller.view.bounds, to: appWindow).minY) < 0.5 {
                filling.append(controller)
            }
            next = controller.presentedViewController
        }
        for controller in filling {
            let id = ObjectIdentifier(controller)
            let ours = given[id]?.top ?? 0
            // What the system gives this controller's view by itself, without its own inset and ours.
            let system = controller.view.safeAreaInsets.top - controller.additionalSafeAreaInsets.top
            let wanted = moved ? min(-system, 0) : 0
            guard abs(wanted - ours) > 0.5 else { continue }
            controller.additionalSafeAreaInsets.top += wanted - ours
            given[id] = wanted == 0 ? nil : ({ [weak controller] in controller }, wanted)
        }
        // A controller that left the window, or whose window is whole again, has its own inset back.
        for (id, entry) in given where !filling.contains(where: { ObjectIdentifier($0) == id }) {
            guard let controller = entry.controller() else { given[id] = nil; continue }
            if !moved || controller.viewIfLoaded?.window == nil {
                if controller.viewIfLoaded?.window == nil || controller.view.window === appWindow {
                    controller.additionalSafeAreaInsets.top -= entry.top
                    given[id] = nil
                }
            }
        }
    }
}

/// Hosts `ChecklistPanelView` and turns its taps into the source's changes.
public final class ChecklistPanelController: UIViewController {
    /// Where the tester's choice between one item and all of them is kept
    /// unless the app names another key.
    public static let oneAtATimeKey = "hitl.checklist-panel.one-at-a-time"

    private let source: any ChecklistSource
    private let defaults: UserDefaults
    private let oneAtATimeKey: String
    private weak var appWindow: UIWindow?
    private let currentStyle: () -> UIUserInterfaceStyle
    private let onClose: () -> Void
    private let panel: ChecklistPanelView
    /// One item at a time, or all of them. The choice is the tester's and
    /// outlives the checklist.
    private var showsOneItem: Bool
    private var message: String?
    private var isBusy = false
    /// An item the tester came to while the app was still busy with the one
    /// before: its preparation runs next, unless they have moved on again.
    private var awaitingPreparation: String?
    /// The checklist is on its way out and takes no room from the app any more.
    private var hasLeft = false

    /// - Parameters:
    ///   - style: The app's colours and font.
    ///   - defaults: Where the tester's choice of view is kept, under `oneAtATimeKey`.
    ///   - appWindow: The app's window: it gets the keyboard back after a comment.
    ///   - currentStyle: The app's light or dark style now; an action of the
    ///     app may have changed it.
    ///   - onClose: The tester put the checklist away, where its source lets them.
    public init(source: any ChecklistSource, style: ChecklistPanelStyle, defaults: UserDefaults = .standard,
                oneAtATimeKey: String = ChecklistPanelController.oneAtATimeKey, appWindow: UIWindow?,
                currentStyle: @escaping () -> UIUserInterfaceStyle, onClose: @escaping () -> Void = {}) {
        self.source = source
        self.defaults = defaults
        self.oneAtATimeKey = oneAtATimeKey
        self.appWindow = appWindow
        self.currentStyle = currentStyle
        self.onClose = onClose
        showsOneItem = defaults.object(forKey: oneAtATimeKey) == nil ? true : defaults.bool(forKey: oneAtATimeKey)
        panel = ChecklistPanelView(style: style)
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    public required init?(coder: NSCoder) { fatalError("Use init(source:style:defaults:oneAtATimeKey:appWindow:currentStyle:onClose:)") }

    public override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .clear
        panel.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(panel)
        // From the very top of the screen, over the status bar's place: on a
        // phone from edge to edge, on a wider screen in the middle.
        NSLayoutConstraint.activate([
            panel.topAnchor.constraint(equalTo: view.topAnchor),
            panel.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            panel.widthAnchor.constraint(lessThanOrEqualToConstant: ChecklistPanelView.widestBand),
            panel.widthAnchor.constraint(lessThanOrEqualTo: view.widthAnchor),
            panel.heightAnchor.constraint(lessThanOrEqualTo: view.heightAnchor, multiplier: 0.7),
        ])
        let wide = panel.widthAnchor.constraint(equalTo: view.widthAnchor)
        wide.priority = .defaultHigh
        wide.isActive = true
        panel.onEvent = { [weak self] in self?.handle($0) }
    }

    /// The panel stands where the clock and the battery were.
    public override var prefersStatusBarHidden: Bool { true }

    public override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        makeRoom()
    }

    /// The app's window starts where the band ends, so the band covers nothing
    /// of the app at any height: the app is that much shorter while the band
    /// is unfolded, and whole again once it folds into the status bar's place
    /// or goes away. Only under a band that goes from edge to edge: one that
    /// hangs in the middle of a wide screen leaves the app be.
    func makeRoom() {
        guard let appWindow else { return }
        var down: CGFloat = 0
        if !hasLeft, let window = view.window, !window.isHidden, panel.frame.width >= view.bounds.width - 0.5,
           panel.frame.maxY > window.safeAreaInsets.top + 0.5 {
            down = panel.frame.maxY
        }
        ChecklistRoom.take(down, from: appWindow, for: self)
    }

    /// The checklist is going away: the app has its whole window back at once.
    public func leave() {
        hasLeft = true
        makeRoom()
    }

    /// A panel dropped without a word gives the room back too.
    isolated deinit { leave() }

    /// The sources read observable stores, so the panel follows every mark,
    /// including one made on the plan's own list.
    public override func updateProperties() {
        super.updateProperties()
        panel.render(.init(tag: source.tag, title: source.title, items: source.items, currentID: source.currentID,
                           showsOneItem: showsOneItem, footer: source.footer, canClose: source.canClose,
                           message: message, isBusy: isBusy, project: source.project))
    }

    public func collapse() { panel.setCollapsed(true) }

    /// Shows an item and prepares the app for it, as coming to it does.
    public func open(_ id: String) {
        panel.setCollapsed(false)
        arrive(at: id)
    }

    /// Runs one of an item's actions as a tap on its button would: the same
    /// button was pressed on a host's screen. Nil, or why it could not.
    public func perform(_ action: String, for id: String) async -> String? {
        guard source.items.contains(where: { $0.id == id }) else { return "В чек-листе нет пункта «\(id)»" }
        if source.currentID != id { source.select(id) }
        guard !isBusy else { return "Приложение ещё занято прошлым действием" }
        return await withCheckedContinuation { continuation in
            run({ [source] in await source.perform(action, for: id) }, then: { continuation.resume(returning: $0) })
        }
    }

    private func handle(_ event: ChecklistPanelView.Event) {
        let items = source.items
        switch event {
        case let .mark(id, state):
            // A second tap on a mark takes it back.
            let next = items.first { $0.id == id }?.state == state ? nil : state
            source.mark(id, next)
            // A problem wants its comment first; anything else moves on to
            // what is still without a mark.
            if showsOneItem, next == .ok || next == .skipped, let following = Self.unmarked(after: id, in: items) {
                view.endEditing(true)
                arrive(at: following)
            }
        case let .note(id, text): source.note(id, text)
        case let .step(offset):
            if let index = items.firstIndex(where: { $0.id == source.currentID }), items.indices.contains(index + offset) {
                arrive(at: items[index + offset].id)
            }
        case let .action(action):
            guard let current = source.currentID else { break }
            run { [source] in await source.perform(action, for: current) }
        case .toggleView:
            showsOneItem.toggle()
            defaults.set(showsOneItem, forKey: oneAtATimeKey)
        case .editingEnded:
            // A typed comment made this window key; the app gets its keyboard back.
            appWindow?.makeKey()
        case .footer:
            view.endEditing(true)
            source.footerTapped()
        case .close:
            view.endEditing(true)
            onClose()
        }
        setNeedsUpdateProperties()
    }

    /// The next item after `id` that has no mark yet, going round to the
    /// start; nil when every other item has one.
    static func unmarked(after id: String, in items: [ChecklistPanelView.Item]) -> String? {
        guard let index = items.firstIndex(where: { $0.id == id }) else { return nil }
        return (items[(index + 1)...] + items[..<index]).first { $0.state == nil }?.id
    }

    private func arrive(at id: String) {
        source.select(id)
        message = nil
        guard !isBusy else {
            awaitingPreparation = id
            setNeedsUpdateProperties()
            return
        }
        run { [source] in await source.prepare(for: id) }
    }

    /// Runs something the app does for an item. One at a time: the app is
    /// moving between screens while it runs.
    private func run(_ work: @escaping @MainActor () async -> String?, then: @escaping (String?) -> Void = { _ in }) {
        guard !isBusy else { setNeedsUpdateProperties(); return }
        isBusy = true
        message = nil
        view.endEditing(true)
        setNeedsUpdateProperties()
        Task { [weak self] in
            let failure = await work()
            then(failure)
            guard let self else { return }
            message = failure
            isBusy = false
            // A typed comment made this window key; the app gets its keyboard back.
            appWindow?.makeKey()
            // An appearance action changes the app's style; the panel follows it.
            view.window?.overrideUserInterfaceStyle = currentStyle()
            setNeedsUpdateProperties()
            if let next = awaitingPreparation {
                awaitingPreparation = nil
                if next == source.currentID { run { [source] in await source.prepare(for: next) } }
            }
        }
    }
}
#endif
