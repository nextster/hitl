import SwiftUI
import WebKit

/// An item's live page: switches and sliders he turns with a finger, and what
/// is set on it goes to the chat with his answer. The page is written for the
/// window on the Mac, 604 points wide. Drawn that wide and shrunk to the phone
/// it was too small to read or to touch («работает но оно пиздец мелкое, надо
/// как то адаптивно сделать»): here it is laid out as wide as the phone is,
/// a quarter larger than on the Mac, with a slider on a line of its own.
struct LivePage: View {
    /// How much larger than on the Mac the page is drawn: its 12.5 points of text are 15.6 here.
    static let larger: CGFloat = 1.25
    /// A page is as tall as it says: on a narrow screen its parts stand one under another, and the item
    /// scrolls with the page in it. On the Mac a page is 300 points at most; here that cut a scheme in half.
    static let tallest: CGFloat = 3000
    static let shortest: CGFloat = 60

    let model: QueueModel
    let path: String
    /// What the page says is set on it, as names and values.
    var set: ([[String]]) -> Void
    @State private var html: String?
    @State private var asked: CGFloat = 160
    @State private var missing = false

    var body: some View {
        GeometryReader { room in
            let scale = Self.larger
            Group {
                if let html, room.size.width > 0 {
                    PageWeb(html: Self.fitted(html, wide: room.size.width / scale), tall: { asked = $0 }, set: set)
                } else if missing {
                    Text("Страница не пришла с Мака").font(.system(size: 13)).foregroundStyle(Ink.faint)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ProgressView().tint(Ink.faint).frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .frame(width: room.size.width, height: tall)
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Ink.bar))
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).stroke(Ink.rule))
            .preference(key: PageHeight.self, value: tall)
        }
        .frame(height: height)
        .onPreferenceChange(PageHeight.self) { height = $0 }
        .task(id: path) {
            html = nil
            missing = false
            if let text = await model.page(path) { html = text } else { missing = true }
        }
        .accessibilityIdentifier("queue.page")
    }

    @State private var height: CGFloat = 100
    /// How tall the page stands on the phone: what it says of itself, drawn larger.
    private var tall: CGFloat { min(max(asked * Self.larger, Self.shortest), Self.tallest) }

    /// The page laid out that many points wide, whatever it says of its own width: the phone draws those
    /// points larger, across its whole width.
    private static func fitted(_ html: String, wide: CGFloat) -> String {
        let own = try? NSRegularExpression(pattern: "<meta[^>]*name=[\"']?viewport[\"']?[^>]*>", options: [.caseInsensitive])
        let bare = own?.stringByReplacingMatches(in: html, range: NSRange(html.startIndex..., in: html), withTemplate: "") ?? html
        return "<meta name=\"viewport\" content=\"width=\(Int(wide.rounded(.down))), user-scalable=no\">" + bare
    }
}

private struct PageHeight: PreferenceKey {
    static let defaultValue: CGFloat = 100
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = nextValue() }
}

private struct PageWeb: UIViewRepresentable {
    var html: String
    var tall: (CGFloat) -> Void
    var set: ([[String]]) -> Void

    /// What every page gets before its own script runs, the same as on the Mac: `hitl.set({…})` says what
    /// is set on the page now, and the page's height is told whenever it changes.
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
      function tall() { post({height: Math.ceil(document.documentElement.getBoundingClientRect().height)}) }
      window.addEventListener('load', function () {
        tall();
        if (window.ResizeObserver) new ResizeObserver(tall).observe(document.documentElement);
      });
    })();
    """

    /// What a narrow screen asks of a page built from the kit of the window's pages: a switch's text may wrap,
    /// a slider's name and number stand on one line and the slider on the next, as wide as the page, tall
    /// enough for a thumb; a drawing is no wider than the page.
    private static let narrow = """
    (function () {
      var style = document.createElement('style');
      style.textContent =
        'html{-webkit-text-size-adjust:none}' +
        '.row{height:auto!important;min-height:32px}.row span{white-space:normal!important}' +
        '.slide{flex-wrap:wrap;height:auto!important;row-gap:0;padding:5px 0}' +
        '.slide span{order:1;width:auto!important;flex:1 1 50%}.slide b{order:2}' +
        '.slide input{order:3;flex:1 1 100%;height:30px}' +
        'svg,img,canvas{max-width:100%;height:auto}';
      (document.head || document.documentElement).appendChild(style);
    })();
    """

    /// A page reaches nothing but itself: no network, whatever its script asks for.
    private static let fence = """
    [{"trigger": {"url-filter": ".*"}, "action": {"type": "block"}},
     {"trigger": {"url-filter": "^about:"}, "action": {"type": "ignore-previous-rules"}},
     {"trigger": {"url-filter": "^data:"}, "action": {"type": "ignore-previous-rules"}},
     {"trigger": {"url-filter": "^blob:"}, "action": {"type": "ignore-previous-rules"}}]
    """

    func makeCoordinator() -> Ear { Ear(tall: tall, set: set) }

    func makeUIView(context: Context) -> WKWebView {
        let setup = WKWebViewConfiguration()
        setup.websiteDataStore = .nonPersistent()
        setup.userContentController.add(context.coordinator, name: "conveyor")
        setup.userContentController.addUserScript(WKUserScript(source: Self.bridge, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        setup.userContentController.addUserScript(WKUserScript(source: Self.narrow, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        let web = WKWebView(frame: .zero, configuration: setup)
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.backgroundColor = .clear
        web.scrollView.isScrollEnabled = false
        web.scrollView.bounces = false
        web.navigationDelegate = context.coordinator
        web.overrideUserInterfaceStyle = .dark
        let html = html
        // Without its fence a page is not shown at all.
        WKContentRuleListStore.default().compileContentRuleList(forIdentifier: "conveyor-page", encodedContentRuleList: Self.fence) { rules, _ in
            MainActor.assumeIsolated {
                guard let rules else { return }
                web.configuration.userContentController.add(rules)
                web.loadHTMLString(html, baseURL: nil)
            }
        }
        return web
    }

    func updateUIView(_ web: WKWebView, context: Context) {
        context.coordinator.tall = tall
        context.coordinator.set = set
    }

    static func dismantleUIView(_ web: WKWebView, coordinator: Ear) {
        web.stopLoading()
        web.configuration.userContentController.removeScriptMessageHandler(forName: "conveyor")
    }

    final class Ear: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
        var tall: (CGFloat) -> Void
        var set: ([[String]]) -> Void
        private var said: [[String]] = []
        private var loaded = false

        init(tall: @escaping (CGFloat) -> Void, set: @escaping ([[String]]) -> Void) {
            self.tall = tall
            self.set = set
        }

        func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
            guard let body = message.body as? [String: Any] else { return }
            if let height = body["height"] as? Double, height > 0 { tall(CGFloat(height)) }
            if let pairs = body["values"] as? [[Any]] {
                let read: [[String]] = pairs.prefix(12).compactMap { pair in
                    guard pair.count == 2, let name = pair[0] as? String, let value = Self.word(pair[1]) else { return nil }
                    let short = String(name.split(whereSeparator: \.isWhitespace).joined(separator: " ").prefix(24))
                    return short.isEmpty ? nil : [short, value]
                }
                // The page's first word is how it came up: the same as on the Mac, and not his hand. What he
                // turns after that is.
                if read != said {
                    let first = said.isEmpty
                    said = read
                    if !first { set(read) }
                }
            }
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
                return String(text.split(whereSeparator: \.isWhitespace).joined(separator: " ").prefix(40))
            }
            return nil
        }

        /// The page goes nowhere: it is loaded once, and a link or a script that would leave it is refused.
        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
            if !loaded, action.targetFrame?.isMainFrame == true, action.request.url?.absoluteString == "about:blank" {
                loaded = true
                return .allow
            }
            return .cancel
        }
    }
}
