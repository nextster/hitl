import ConveyorCheck
import ConveyorCheckUI
import SwiftUI
import UIKit
import UserNotifications

@main
final class AppDelegate: UIResponder, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        return true
    }

    /// Where Apple delivers this phone's notifications: the Mac is told, and it is the Mac that asks Apple.
    func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        QueueModel.shared.registered(push: deviceToken.map { String(format: "%02x", $0) }.joined())
    }

    /// A notification of a check was tapped: the app opens on that check.
    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                            withCompletionHandler completionHandler: @escaping () -> Void) {
        let ticket = response.notification.request.content.userInfo["ticket"] as? String
        Task { @MainActor in
            if let ticket { QueueModel.shared.wants(ticket) }
        }
        completionHandler()
    }

    /// With the app in front the queue itself is on the screen: no banner over it.
    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                            withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([])
    }
}

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options connectionOptions: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: scene)
        let arguments = ProcessInfo.processInfo.arguments
        window.rootViewController = RootViewController(launch: DeviceCheckLaunch(arguments: arguments))
        window.makeKeyAndVisible()
        self.window = window
        // `--queue-far`: the Mac is looked for in the server's room only, as away from home, though it is in this network.
        QueueModel.shared.onlyFar = arguments.contains("--queue-far")
        // `--queue-word VALUE`: the queue is the one given here, and no Mac is looked for.
        if let flag = arguments.firstIndex(of: "--queue-word"), arguments.indices.contains(flag + 1) {
            QueueModel.shared.show(word: arguments[flag + 1])
        }
        QueueModel.shared.wake()
        if let flag = arguments.firstIndex(of: "--queue-play"), arguments.indices.contains(flag + 1) {
            QueueModel.shared.play(arguments[flag + 1])
        }
    }
}

/// The app is the Mac's queue of checks on the phone, and nothing else. Launched by the host with a checklist, the
/// checklist's band stands over the queue. There was a demo of the band here, a screen with things to touch and
final class RootViewController: UIViewController {
    /// The check a host installed this build for.
    private let hostCheck: DeviceCheckStore
    private var panelWindow: ChecklistPanelWindow?
    private let queue: UIViewController = {
        let screen = UIHostingController(rootView: QueueScreen(model: .shared))
        screen.overrideUserInterfaceStyle = .dark
        screen.view.backgroundColor = UIColor(red: 0x1A / 255, green: 0x1B / 255, blue: 0x1F / 255, alpha: 1)
        return screen
    }()

    init(launch: DeviceCheckLaunch) {
        hostCheck = DeviceCheckStore(defaults: .standard, launch: launch)
        super.init(nibName: nil, bundle: nil)
        QueueModel.shared.hasChecklist = { [weak self] in self?.hostCheck.isActive == true }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("Use init(launch:)") }

    override var preferredStatusBarStyle: UIStatusBarStyle { .lightContent }

    override func viewDidLoad() {
        super.viewDidLoad()
        addChild(queue)
        queue.view.frame = view.bounds
        queue.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(queue.view)
        queue.didMove(toParent: self)
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        guard panelWindow == nil, hostCheck.isActive, let appWindow = view.window else { return }
        // The band of the host's checklist. The app has no actions of its own for its items.
        panelWindow = DeviceCheckPanel.show(
            store: hostCheck, over: appWindow, style: ChecklistPanelStyle(), interfaceStyle: .dark, tint: .tintColor,
            currentStyle: { .dark }, controls: [],
            run: { ids in "У приложения «HITL» нет действий для пунктов: \(ids.joined(separator: ", "))" },
            openLink: { _ in false },
            onFinish: { [weak self] in self?.hidePanel() })
    }

    private func hidePanel() {
        guard let window = panelWindow else { return }
        panelWindow = nil
        DeviceCheckPanel.hide(window, returningKeyTo: view.window)
        QueueModel.shared.checklistEnded()
    }
}
