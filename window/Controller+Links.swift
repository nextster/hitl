// What an item offers to open and to copy: how a link is called and opened, how this app comes forward for it and steps
// back, and how a line gets into the clipboard.

import AppKit

extension Controller {
    // MARK: Actions

    /// An address, when the link is one: a site by its host, another app by its scheme.
    private func address(_ link: String) -> URL? {
        guard !link.hasPrefix("/"), !link.hasPrefix("~"), let url = URL(string: link), url.scheme != nil else { return nil }
        return url
    }

    /// What a link is called on its button: a pull request by its number, a site by its host, a file by its name.
    func name(of link: String) -> String {
        guard let url = address(link) else { return (link as NSString).lastPathComponent }
        guard ["http", "https"].contains(url.scheme ?? "") else { return url.scheme ?? link }
        let parts = url.pathComponents
        if url.host == "github.com", let pull = parts.firstIndex(of: "pull"), parts.indices.contains(pull + 1) { return "PR #\(parts[pull + 1])" }
        return url.host ?? link
    }

    /// Copies a line of the item, all of it, and the line says so for a moment. He pastes it where it goes himself:
    /// nothing is run and nothing comes forward.
    func copy(line number: Int) {
        let lines = copies
        guard let ticket = active, lines.indices.contains(number) else { return }
        if scriptFile != nil {
            copiedSaid.append(lines[number])  // a scripted run never touches his clipboard
        } else {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(lines[number], forType: .string)
        }
        copiedLine = (ticket.id + "/" + ticket.items[index].label, number)
        for (place, line) in copyTaps.enumerated() { line.copied = place == number }
        copiedTimer?.invalidate()
        copiedTimer = Timer.scheduledTimer(withTimeInterval: 1.5, repeats: false) { [weak self] _ in
            self?.copiedLine = nil
            self?.copyTaps.forEach { $0.copied = false }
        }
    }

    func open(link number: Int) {
        let links = opens
        guard active != nil, links.indices.contains(number) else { return }
        open(address: links[number])
    }

    /// Opens a link by what a chat gave for it: an address, or a file on this Mac. A link of the item's own, or
    /// one written into a text of the check.
    func open(address link: String) {
        if let command = ChatButton.command(for: link, environment: ProcessInfo.processInfo.environment) { return run(command: command, for: link) }
        var target = address(link)
        if target == nil {
            let file = URL(fileURLWithPath: (link as NSString).expandingTildeInPath)
            target = vaultAddress(of: file) ?? file
        }
        guard let target else { return }
        bring(target)
    }

    /// An address the setting of the «Чат» button gives a command for (a chat that lives in a terminal, say): the
    /// command runs and the window steps aside, as it does for an app that came forward. A scripted window runs nothing.
    private func run(command: [String], for link: String) {
        if scriptFile != nil {
            opened.append(link)
            ran.append(command.joined(separator: " "))
            closeLook()
            stepAside()
            return
        }
        let environment = ProcessInfo.processInfo.environment
        DispatchQueue.global(qos: .userInitiated).async {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
            process.arguments = command
            process.environment = environment
            process.standardInput = FileHandle.nullDevice; process.standardOutput = FileHandle.nullDevice; process.standardError = FileHandle.nullDevice
            try? process.run()
        }
    }

    /// Opens something in its own app and brings that app forward.
    func bring(_ target: URL) {
        if scriptFile != nil {
            opened.append(target.absoluteString)
            if Self.isSite(target) { browsed.append(target.absoluteString) }
            closeLook()  // as the opened app coming forward would
            stepAside()
        } else if NSApp.isActive {
            bringUp(target)
        } else {
            // The window floats over another app, and that app is the active one. macOS brings an opened app
            // forward only when the active app asks for it, so this one steps forward first.
            waitingToOpen = target
            NSApp.activate()
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { [weak self] in self?.openWaiting() }
        }
    }

    @objc private func openWaiting() {
        guard let target = waitingToOpen else { return }
        waitingToOpen = nil
        bringUp(target)
    }

    @objc func otherCameForward(_ notice: Notification) {
        guard let app = notice.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication, app.processIdentifier != getpid() else { return }
        appBefore = app
        let handedBack = handedBackAt.map { Date().timeIntervalSince($0) < 1 } ?? false
        handedBackAt = nil
        // Only an app with windows of its own: what draws over the screen without one, the screenshot
        // crosshair for one, must not take the page from under his pointer.
        if !handedBack, scriptFile == nil, app.activationPolicy == .regular { stepAside() }
    }

    /// This app became the active one. It asks for that only to open a link; macOS also does it by itself
    /// when the window is started by a chat of the app in front, and then he would be typing into nothing.
    @objc func cameForward() {
        timesForward += 1
        if waitingToOpen != nil {
            openWaiting()
        } else {
            stepBack()
        }
    }

    /// Hands the keyboard back to the app he was in.
    func stepBack() {
        guard NSApp.isActive else { return }
        handedBackAt = Date()
        if let app = appBefore, !app.isTerminated {
            NSApp.yieldActivation(to: app)
            app.activate(from: .current, options: [])
        } else {
            NSApp.deactivate()
        }
    }

    /// Open HTTP links in the default browser so an installed app cannot claim them.
    static func isSite(_ target: URL) -> Bool { ["http", "https"].contains(target.scheme?.lowercased() ?? "") }

    /// The web browser: the app this Mac opens the address of a site no app claims with.
    static var browser: URL? { URL(string: "https://example.com").flatMap { NSWorkspace.shared.urlForApplication(toOpen: $0) } }

    private func bringUp(_ target: URL) {
        let asked = NSApp.isActive
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        let opened: @Sendable (NSRunningApplication?, Error?) -> Void = { app, error in
            guard let app else {
                log("open \(target.absoluteString): \(error?.localizedDescription ?? "nothing took it")")
                return
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                let front = NSWorkspace.shared.frontmostApplication
                let came = front?.processIdentifier == app.processIdentifier
                log("open \(target.absoluteString): asked as the active app \(asked), \(app.localizedName ?? "?") in front \(came)"
                    + (came ? "" : ", in front is \(front?.localizedName ?? "?"); asking it forward"))
                if !came {
                    NSApp.yieldActivation(to: app)
                    app.activate(from: .current, options: [.activateAllWindows])
                }
            }
        }
        if Self.isSite(target), let browser = Self.browser {
            NSWorkspace.shared.open([target], withApplicationAt: browser, configuration: configuration, completionHandler: opened)
        } else {
            NSWorkspace.shared.open(target, configuration: configuration, completionHandler: opened)
        }
    }

    /// A note of an Obsidian vault opens in Obsidian, not in whatever edits its file type.
    private func vaultAddress(of file: URL) -> URL? {
        var folder = file.deletingLastPathComponent()
        while folder.path.count > 1 {
            if FileManager.default.fileExists(atPath: folder.appendingPathComponent(".obsidian").path),
               let path = file.path.addingPercentEncoding(withAllowedCharacters: .alphanumerics) {
                return URL(string: "obsidian://open?path=" + path)
            }
            folder = folder.deletingLastPathComponent()
        }
        return nil
    }
}
