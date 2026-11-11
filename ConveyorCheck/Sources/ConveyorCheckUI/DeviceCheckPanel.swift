#if canImport(UIKit)
import ConveyorCheck
import UIKit

/// The checklist of a hand check over an app, at the top of its screen: shown
/// open on the launch that brings it, folded when the app is reopened with
/// the check still on, and taken away when the tester ends it.
public enum DeviceCheckPanel {
    /// Puts the panel in a window of its own above the app's. Nil when the
    /// store has no check to show, or the app's window is in no scene yet.
    ///
    /// - Parameters:
    ///   - interfaceStyle: The app's light or dark style, and `tint` its tint:
    ///     the panel's window does not inherit them from the app's. The panel
    ///     itself is black in either; a sheet it presents follows the app.
    ///   - currentStyle: The app's style now; an action may have changed it.
    ///   - controls: What the actions of the app's vocabulary are: a link, a
    ///     switch, a choice or an action done once. Each is offered on a
    ///     button of its kind, and what a switch or a choice was set to by the
    ///     check goes back when the check ends.
    ///   - run: Runs actions of the app's vocabulary by id; nil, or why it
    ///     could not, in words for the tester.
    ///   - openLink: Opens a deep link an item carries; false when the app
    ///     has no such link.
    ///   - onFinish: The tester ended the check: time to `hide` the window.
    public static func show(store: DeviceCheckStore, over appWindow: UIWindow, style: ChecklistPanelStyle,
                            interfaceStyle: UIUserInterfaceStyle, tint: UIColor, defaults: UserDefaults = .standard,
                            oneAtATimeKey: String = ChecklistPanelController.oneAtATimeKey,
                            currentStyle: @escaping () -> UIUserInterfaceStyle, controls: [DeviceCheckControl] = [],
                            run: @escaping ([String]) async -> String?, openLink: @escaping (URL) -> Bool,
                            onFinish: @escaping () -> Void) -> ChecklistPanelWindow? {
        guard store.isActive, let scene = appWindow.windowScene else { return nil }
        let checklist = DeviceCheckChecklist(store: store, controls: controls, run: run, openLink: openLink, onFinish: onFinish)
        let controller = ChecklistPanelController(source: checklist, style: style, defaults: defaults, oneAtATimeKey: oneAtATimeKey,
                                                  appWindow: appWindow, currentStyle: currentStyle)
        let window = ChecklistPanelWindow(windowScene: scene)
        window.fitScene()
        // Above a checklist of the app's own: this one says what the build is here for.
        window.windowLevel = .normal + 2
        window.backgroundColor = .clear
        window.overrideUserInterfaceStyle = interfaceStyle
        window.tintColor = tint
        window.rootViewController = controller
        window.isHidden = false
        controller.loadViewIfNeeded()
        if !store.arrivedWithThisLaunch { controller.collapse() }
        // The host may show the same checklist on its own screen: marks made there come here, and so do a press
        // of an item's button and the end of the check.
        store.finishedByHost = onFinish
        store.beforeFinish = { [weak checklist] in await checklist?.restore() }
        store.runForHost = { [weak controller] label, action in
            guard let controller else { return "Чек-лист уже убран с экрана" }
            return await controller.perform(action, for: label)
        }
        store.listen()
        return window
    }

    /// Takes the panel away once the check is over: the marks are in the file
    /// the host reads. The app's window gets the keyboard and its whole
    /// height back, and the status bar is the app's to show again.
    public static func hide(_ window: ChecklistPanelWindow, returningKeyTo appWindow: UIWindow?, animated: Bool = true) {
        (window.rootViewController as? ChecklistPanelController)?.leave()
        appWindow?.makeKey()
        UIView.animate(withDuration: animated ? 0.25 : 0, animations: {
            window.alpha = 0
        }, completion: { _ in
            window.isHidden = true
            appWindow?.rootViewController?.setNeedsStatusBarAppearanceUpdate()
        })
    }
}
#endif
