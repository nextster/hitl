// A live page that came with an item (not the page the window becomes on the whole screen): switches and sliders he turns with the mouse, and what is set on it goes
// to the chat with his answer.

import AppKit
import WebKit

/// The page's web view. It takes the mouse at the first click and never the keyboard, which stays the window's:
/// every answer is still a digit while he turns things on the page.
final class LivePageWeb: WKWebView {
    /// The wheel turned over the page: true when the window took the turn, to scroll the item the page stands in.
    var turned: ((NSEvent) -> Bool)?

    override var acceptsFirstResponder: Bool { false }
    override func scrollWheel(with event: NSEvent) {
        if turned?(event) != true { super.scrollWheel(with: event) }
    }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func willOpenMenu(_ menu: NSMenu, with event: NSEvent) { menu.removeAllItems() }
}

/// Hands the page's word on without holding the page: a web view keeps its handlers alive.
private final class LivePageEar: NSObject, WKScriptMessageHandler {
    weak var page: LivePage?

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        page?.heard(message.body)
    }
}

final class LivePage: NSView, WKNavigationDelegate {
    static let tallest: CGFloat = 300
    static let shortest: CGFloat = 60
    static let mostValues = 12
    static let longestName = 24
    static let longestValue = 40

    /// What every page gets before its own script runs: `hitl.set({…})` says what is set on the page now,
    /// and the page's height is told whenever it changes, with the width it was counted at.
    private static let bridge = """
    (function () {
      function post(body) { try { window.webkit.messageHandlers.conveyor.postMessage(body) } catch (e) {} }
      window.hitl = window.conveyor = {
        set: function (values) {
          var pairs = [];
          for (var name in values) if (Object.prototype.hasOwnProperty.call(values, name)) pairs.push([String(name), values[name]]);
          post({values: pairs});
        }
      };
      function place() { return [window.scrollY, Math.max(0, document.documentElement.scrollHeight - window.innerHeight)] }
      function tall() { post({height: Math.ceil(document.documentElement.getBoundingClientRect().height), width: window.innerWidth, scroll: place()}) }
      window.addEventListener('scroll', function () { post({scroll: place()}) }, {passive: true});
      window.addEventListener('load', function () {
        tall();
        if (window.ResizeObserver) new ResizeObserver(tall).observe(document.documentElement);
        window.addEventListener('resize', tall);
      });
    })();
    """

    /// A page reaches nothing but its own file: no network, whatever its script asks for.
    private static let fence = """
    [{"trigger": {"url-filter": ".*"}, "action": {"type": "block"}},
     {"trigger": {"url-filter": "^file:"}, "action": {"type": "ignore-previous-rules"}},
     {"trigger": {"url-filter": "^about:"}, "action": {"type": "ignore-previous-rules"}},
     {"trigger": {"url-filter": "^data:"}, "action": {"type": "ignore-previous-rules"}},
     {"trigger": {"url-filter": "^blob:"}, "action": {"type": "ignore-previous-rules"}}]
    """
    private static var rules: WKContentRuleList?
    private static var waitingForRules: [(WKContentRuleList?) -> Void] = []
    private static var askedForRules = false

    private static func withRules(_ then: @escaping (WKContentRuleList?) -> Void) {
        if let rules { return then(rules) }
        waitingForRules.append(then)
        guard !askedForRules else { return }
        askedForRules = true
        let folder = windowDirectory.appendingPathComponent("rules")
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        guard let store = WKContentRuleListStore(url: folder) else { return answerRules(nil) }
        store.compileContentRuleList(forIdentifier: "conveyor-page", encodedContentRuleList: fence) { list, error in
            if let error { log("the page's fence was not built: \(error.localizedDescription)") }
            answerRules(list)
        }
    }

    private static func answerRules(_ list: WKContentRuleList?) {
        rules = list
        askedForRules = list != nil
        let waiting = waitingForRules
        waitingForRules = []
        waiting.forEach { $0(list) }
    }

    let path: String
    let web: LivePageWeb
    /// What is set on the page, in the page's own order and words: it goes to the chat with his answer.
    private(set) var values: [[String]] = []
    /// How tall the page says it is at the window's width.
    private(set) var asked: CGFloat = 160
    private(set) var isReady = false
    /// It has said how tall it is; until then its height is the window's guess.
    private(set) var hasSaid = false
    /// How wide the page is in its own pixels: as wide as the item's column in the window's points.
    var own: CGFloat = contentWidth
    /// The page is taller than it is let to stand: the wheel over it scrolls the page itself.
    var scrollsInside: Bool { asked > LivePage.tallest }
    /// How far the page itself is scrolled and how far it can be, as it said last, in its own pixels.
    private(set) var inside: (at: Double, most: Double) = (0, 0)
    /// Whether a turn of the wheel by so many points, positive when the content is pulled down, finds room in the
    /// page itself: it can still go that way. At its edge, or with nothing to scroll, the turn is the item's.
    func scrolls(by turned: CGFloat) -> Bool {
        guard scrollsInside else { return false }
        return turned > 0 ? inside.at > 1 : inside.at < inside.most - 1
    }
    /// How much larger than in the small window the page is drawn: on the whole screen it stands large.
    var enlargement: CGFloat = 1 { didSet { if enlargement != oldValue { web.pageZoom = enlargement } } }
    /// Its values or its height changed, or it has come up.
    var changed: (() -> Void)?
    private let ear = LivePageEar()
    private var whenReady: [() -> Void] = []

    var tall: CGFloat { min(max(asked, LivePage.shortest), LivePage.tallest) }

    init(path: String) {
        self.path = path
        let setup = WKWebViewConfiguration()
        setup.websiteDataStore = .nonPersistent()
        setup.userContentController.add(ear, name: "conveyor")
        setup.userContentController.addUserScript(WKUserScript(source: LivePage.bridge, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        web = LivePageWeb(frame: NSRect(x: 0, y: 0, width: contentWidth, height: 160), configuration: setup)
        super.init(frame: web.frame)
        ear.page = self
        web.navigationDelegate = self
        web.setValue(false, forKey: "drawsBackground")
        web.autoresizingMask = [.width, .height]
        addSubview(web)
        toolTip = nil
        LivePage.withRules { [weak self] rules in
            guard let self else { return }
            // Without its fence a page is not shown at all.
            guard let rules else { return }
            self.web.configuration.userContentController.add(rules)
            let file = URL(fileURLWithPath: self.path)
            self.web.loadFileURL(file, allowingReadAccessTo: file)
        }
    }

    required init?(coder: NSCoder) { fatalError("the window is built in code") }

    override var intrinsicContentSize: NSSize { NSSize(width: contentWidth, height: tall) }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 8, yRadius: 8)
        Ink.bar.setFill()
        shape.fill()
        Ink.edge.setStroke()
        shape.stroke()
    }

    /// The page goes nowhere: it is loaded once, and a link or a script that would leave it is refused.
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let own = URL(fileURLWithPath: path).standardizedFileURL
        let wanted = action.request.url?.standardizedFileURL
        decisionHandler(action.targetFrame?.isMainFrame == true && wanted?.path == own.path && wanted?.isFileURL == true ? .allow : .cancel)
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        // The page's own load handlers have run by now, and its first word is on its way.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in
            guard let self else { return }
            self.isReady = true
            let waiting = self.whenReady
            self.whenReady = []
            waiting.forEach { $0() }
            self.changed?()
        }
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        log("the page \(path) did not load: \(error.localizedDescription)")
    }

    /// WebKit's own process of the page ended, crashed or taken by the system: the page is loaded again and says its
    /// values anew. Left alone it stood empty, and his answer went with the last values it had said (H10a).
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        log("the page \(path) lost its process: loading it again")
        isReady = false
        let file = URL(fileURLWithPath: path)
        web.loadFileURL(file, allowingReadAccessTo: file)
    }

    fileprivate func heard(_ body: Any) {
        guard let said = body as? [String: Any] else { return }
        if let place = said["scroll"] as? [Double], place.count == 2 { inside = (place[0], place[1]) }
        var moved = false
        // On the whole screen the window gives the page a zoom and a size, one after the other, and between the
        // two the page stands narrower or wider for a moment: the height it counts then is not its height. Taken
        // for one, it made the window draw everything at another scale, which changed the page's zoom again.
        // The page says its height anew once it is as wide as it stays.
        let steady = (said["width"] as? Double).map { abs(CGFloat($0) - own) <= 2 } ?? true
        if steady, let height = said["height"] as? Double, height > 0, abs(CGFloat(height) - asked) > 0.5 {
            asked = CGFloat(height)
            invalidateIntrinsicContentSize()
            moved = true
        }
        if steady, said["height"] != nil { hasSaid = true }
        if let pairs = said["values"] as? [[Any]] {
            let read: [[String]] = pairs.prefix(LivePage.mostValues).compactMap { pair in
                guard pair.count == 2, let name = pair[0] as? String, let value = LivePage.word(pair[1]) else { return nil }
                let short = String(name.split(whereSeparator: \.isWhitespace).joined(separator: " ").prefix(LivePage.longestName))
                return short.isEmpty ? nil : [short, value]
            }
            if read != values {
                values = read
                moved = true
            }
        }
        if moved { changed?() }
    }

    /// A value as the chat will read it: a number as it is, a switch as «да» or «нет», a text cut short.
    private static func word(_ value: Any) -> String? {
        if let number = value as? NSNumber {
            if CFGetTypeID(number) == CFBooleanGetTypeID() { return number.boolValue ? "да" : "нет" }
            let double = number.doubleValue
            guard double.isFinite else { return nil }
            return double == double.rounded() && abs(double) < 1e15 ? String(Int64(double)) : String(format: "%.3g", double)
        }
        if let text = value as? String {
            return String(text.split(whereSeparator: \.isWhitespace).joined(separator: " ").prefix(longestValue))
        }
        return nil
    }

    /// Once the page is up: at once, when it is already.
    func ready(_ then: @escaping () -> Void) {
        if isReady { then() } else { whenReady.append(then) }
    }

    /// What the page shows now, as a picture: a web view draws in a process of its own, and the window's
    /// own picture of itself has an empty card where the page is.
    func picture(_ then: @escaping (NSImage?) -> Void) {
        let setup = WKPDFConfiguration()
        setup.rect = web.bounds
        web.createPDF(configuration: setup) { result in
            if case .success(let data) = result { then(NSImage(data: data)) } else { then(nil) }
        }
    }

    func close() {
        changed = nil
        web.stopLoading()
        web.configuration.userContentController.removeScriptMessageHandler(forName: "conveyor")
        removeFromSuperview()
    }
}
