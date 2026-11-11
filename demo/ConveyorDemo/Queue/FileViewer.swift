import SwiftUI
import WebKit

/// A picture or a file of the Mac, shown over the queue: the app's own viewer, with a button that closes it
/// where a thumb is, on a sheet that a swipe down puts away. The system's viewer opened them and gave him
/// nothing to close them with («Открываются а закрыть то как»).
struct Shown: Identifiable {
    enum What {
        case picture(UIImage)
        case text(String)
        /// Anything else, as a file kept for the look: a PDF, a page.
        case file(URL)
    }

    let id = UUID()
    var name: String
    var what: What
}

struct FileViewer: View {
    let shown: Shown
    var close: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Text(shown.name).font(.system(size: 15, weight: .semibold)).foregroundStyle(Ink.who).lineLimit(1).truncationMode(.middle)
                Spacer(minLength: 8)
                Button(action: close) {
                    Image(systemName: "xmark").font(.system(size: 15, weight: .bold)).foregroundStyle(Ink.text)
                        .frame(width: 44, height: 44).background(Circle().fill(Ink.key)).contentShape(Rectangle())
                }
                .accessibilityLabel("Закрыть")
            }
            .padding(.horizontal, 18).padding(.top, 14).padding(.bottom, 8)
            Group {
                switch shown.what {
                case .picture(let image): Zoomed(image: image)
                case .text(let text):
                    ScrollView {
                        Text(text).font(.system(size: 15)).foregroundStyle(Ink.text).textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading).padding(18)
                    }
                case .file(let url): WebFile(url: url)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Ink.bar)
            // The whole of it answers a touch: at first only its letters did («кнопка закрыть только по тапу
            // на надпись работает»).
            Button(action: close) {
                Text("Закрыть").font(.system(size: 15, weight: .semibold)).foregroundStyle(Ink.who)
                    .frame(maxWidth: .infinity, minHeight: 40)
                    .background(RoundedRectangle(cornerRadius: 11, style: .continuous).fill(Ink.key))
                    .contentShape(Rectangle())
            }
            .padding(.horizontal, 18).padding(.top, 8).padding(.bottom, 6)
            .accessibilityIdentifier("queue.viewer.close")
        }
        .background(Ink.panel.ignoresSafeArea())
        .preferredColorScheme(.dark)
    }
}

/// A picture that fits the screen, pinched larger and dragged about; two taps bring it closer and back.
private struct Zoomed: UIViewRepresentable {
    var image: UIImage

    func makeCoordinator() -> Holder { Holder() }

    func makeUIView(context: Context) -> UIScrollView {
        let scroll = UIScrollView()
        scroll.delegate = context.coordinator
        scroll.minimumZoomScale = 1
        scroll.maximumZoomScale = 8
        scroll.showsVerticalScrollIndicator = false
        scroll.showsHorizontalScrollIndicator = false
        scroll.backgroundColor = .clear
        let picture = context.coordinator.picture
        picture.image = image
        picture.contentMode = .scaleAspectFit
        picture.frame = scroll.bounds
        picture.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        scroll.addSubview(picture)
        let twice = UITapGestureRecognizer(target: context.coordinator, action: #selector(Holder.twice(_:)))
        twice.numberOfTapsRequired = 2
        scroll.addGestureRecognizer(twice)
        return scroll
    }

    func updateUIView(_ scroll: UIScrollView, context: Context) {
        context.coordinator.picture.image = image
    }

    final class Holder: NSObject, UIScrollViewDelegate {
        let picture = UIImageView()

        func viewForZooming(in scrollView: UIScrollView) -> UIView? { picture }

        @objc func twice(_ tap: UITapGestureRecognizer) {
            guard let scroll = tap.view as? UIScrollView else { return }
            if scroll.zoomScale > 1.01 {
                scroll.setZoomScale(1, animated: true)
            } else {
                let at = tap.location(in: picture)
                let size = CGSize(width: scroll.bounds.width / 3, height: scroll.bounds.height / 3)
                scroll.zoom(to: CGRect(x: at.x - size.width / 2, y: at.y - size.height / 2, width: size.width, height: size.height), animated: true)
            }
        }
    }
}

/// A file that is neither a picture nor a text, drawn by the system's web view: a PDF, a page. It reaches
/// nothing but the file itself.
private struct WebFile: UIViewRepresentable {
    var url: URL

    func makeUIView(context: Context) -> WKWebView {
        let setup = WKWebViewConfiguration()
        setup.websiteDataStore = .nonPersistent()
        let web = WKWebView(frame: .zero, configuration: setup)
        web.isOpaque = false
        web.backgroundColor = .clear
        web.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())
        return web
    }

    func updateUIView(_ web: WKWebView, context: Context) {}
}
