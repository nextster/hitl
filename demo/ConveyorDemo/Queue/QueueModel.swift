import Network
import Observation
import UIKit
import UserNotifications

/// The Mac's queue of checks, on the phone. The app is a second screen of the
/// check window: it shows what the window shows and sends what was pressed,
/// and the window does the rest, as for a digit pressed on the Mac.
@Observable
final class QueueModel {
    enum Link: Equatable {
        /// The Mac has not put its word on this phone yet.
        case unpaired
        case searching
        case connected
        /// The Mac is at home and answers, and nothing waits in its queue.
        case nothingWaits
        /// The Mac does not answer: it sleeps, or the phone is not at home and there is no server to meet at.
        case away
    }

    static let shared = QueueModel()

    private(set) var link: Link = .searching
    private(set) var queue: QueueWord?
    private(set) var macName = ""
    /// Pictures and files of the items, as they come from the Mac: by their path there.
    private(set) var pictures: [String: UIImage] = [:]
    private(set) var failed: [String: String] = [:]
    /// The list of every check that waits is up, over the item.
    var listUp = false
    /// The line of an item copied last: it says so for a moment.
    private(set) var copied: String?
    private var copies = 0
    /// A picture or a file of the Mac shown over the queue, and why one could not be shown.
    var viewing: Shown?
    var trouble: String?

    private var word: MacWord?
    /// The way to the Mac that is in use.
    private var mac: MacWire?
    /// An attempt to reach the Mac at home, or that way itself.
    private var home: MacLink?
    /// The server's room: kept open while the app is in front and the Mac is not reached at home.
    private var far: RelayLink?
    /// Every address at home was tried this round and none answered; and whether the Mac was there and only
    /// had nobody listening, which is how it stands with nothing in its queue.
    private var homeFailed = false
    private var homeRests = false
    private var farFailed = false
    /// Why the server's room was not reached the last time it was tried, in the system's words; nil once it answers.
    private(set) var serverTrouble: String?
    /// The rounds of looking that found no way, since the Mac was last heard or the app came in front.
    private var rounds = 0
    private var farRetry: Task<Void, Never>?
    /// Which network the phone is on: when it changes, the way in use is gone with it, whatever it still looks like.
    private let paths = NWPathMonitor()
    private var network: String?
    private var began = Date.distantPast
    /// At home the Mac answers in a fraction of a second: the server's room is taken once home has had this long.
    private let headStart = 1.2
    private var soon: Task<Void, Never>?
    private var isActive = false
    private var nextHost = 0
    private var retry: Task<Void, Never>?
    private var asked = 0
    private var answers: [Int: CheckedContinuation<(name: String, data: Data)?, Never>] = [:]
    private var fetching: Set<String> = []
    /// Why the Mac did not give the file asked last.
    private var refusal: String?
    /// A comment being written here that the Mac has not been told yet.
    private var note: (ticket: String, label: String, text: String)?
    /// What was pressed or written here with no way to the Mac, kept on the phone, through a launch too, and sent
    /// in its order once the Mac takes it, or the server's room holds it for the Mac. His pick of 2026-10-16:
    /// «если есть связь с сервером прихранить на сервере, если нет то прихранить на клиенте». Until then a
    /// press without a way went nowhere, and a comment typed meanwhile was dropped.
    private(set) var outbox: [Kept] = Kept.load()
    /// Kept words said into the server's room, in their order, with their frames: the server says of each frame
    /// whether it held it, and a word goes off the phone once all of its frames are held.
    private var intoRoom: [(id: String, frames: Int, heard: Int, refused: Bool)] = []
    /// The room the phone's hello was said into for what it holds: each way into the room greets the Mac anew.
    private var greeted: RelayLink?
    private var noteTimer: Task<Void, Never>?
    private var pushToken: String?
    /// The check a notification was tapped for: it comes up once the Mac is heard.
    private var wanted: String?
    private var askedForNotifications = false
    /// The queue on the screen is the Mac's word of this very connection. One left from before the app was put
    /// away may name a check that is gone by now: its pictures and page are not asked for («иногда такую
    /// хуйню показывает»: a notice that the file is not in the queue, over a check that had no file at all).
    private var isFresh = false
    private var isPlaying = false

    /// The Mac is heard through the server's room, not at home.
    var isFar: Bool { mac != nil && mac === far }
    /// No way to the Mac is open now: what is pressed is kept on the phone.
    var isOffline: Bool { mac == nil || !isFresh }
    /// A launch that is to see the way through the server from home: home is not tried at all.
    var onlyFar = false
    /// There is a server to meet at: a Mac that is not reached is then asleep or off, not merely elsewhere.
    var hasServer: Bool { word?.relay != nil }
    /// A queue given with the launch, in the Mac's place: while there is one, no Mac is looked for. It is gone
    /// once the app is put away; given for a checklist on the band, once that check has ended.
    private var given: QueueWord?
    /// Whether a host's checklist is on the band: a queue given with the launch is what it is about.
    var hasChecklist: () -> Bool = { false }

    private let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("conveyor-files", isDirectory: true)

    init() {
        try? FileManager.default.removeItem(at: scratch)
        // Where the Mac puts its word by cable: the folder is there for it from the first launch.
        try? FileManager.default.createDirectory(at: MacWord.file.deletingLastPathComponent(), withIntermediateDirectories: true)
        let center = NotificationCenter.default
        center.addObserver(forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.wake() }
        }
        center.addObserver(forName: UIApplication.didEnterBackgroundNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.sleep() }
        }
        paths.pathUpdateHandler = { [weak self] path in
            let now = path.status == .satisfied ? (path.availableInterfaces.first.map { "\($0.type) \($0.name)" } ?? "some") : "none"
            MainActor.assumeIsolated { self?.networkIs(now) }
        }
        paths.start(queue: .main)
    }

    // MARK: The connection

    /// The app is in front: the Mac is looked for, and listened to for as long as the app stays there.
    func wake() {
        isActive = true
        if let given {
            queue = given
            link = .connected
            return
        }
        guard mac == nil else { return }
        if link != .unpaired { link = .searching }
        rounds = 0
        connect()
    }

    func sleep() {
        isActive = false
        if !hasChecklist() { given = nil }
        flushNote()
        letGo()
        queue = nil
        listUp = false
        if link == .connected { link = .searching }
    }

    /// Every way and every attempt is dropped, and what waited for them.
    private func letGo() {
        retry?.cancel()
        soon?.cancel()
        farRetry?.cancel()
        home?.close()
        home = nil
        far?.close()
        far = nil
        lost()
    }

    /// The phone went from one network to another, from Wi‑Fi to the mobile one or back: the way that was in
    /// use belonged to the old one. It is not waited for to fail; the Mac is looked for anew, both ways.
    private func networkIs(_ now: String) {
        defer { network = now }
        guard let before = network, before != now, isActive else { return }
        letGo()
        rounds = 0
        if now != "none" { connect() } else { again(in: 1) }
    }

    /// One round of looking for the Mac: at home, by each of its addresses in turn, and in the server's room
    /// at the same time. Home is the faster way and is taken when it answers; the room when home does not.
    /// Neither waits for the other: each is tried again by itself, and the next round comes whatever this one does.
    private func connect() {
        retry?.cancel()
        guard isActive, mac == nil, given == nil else { return }
        guard let word = MacWord.read(), !word.hosts.isEmpty else {
            link = .unpaired
            return again(in: 2)
        }
        self.word = word
        macName = word.name
        if link == .unpaired { link = .searching }
        if let hung = home {
            // An attempt of the round before that neither answered nor was said no to.
            hung.close()
            home = nil
            nextHost += 1
        }
        if rounds > 0 {
            // The round before found no way home: the screen says how things stand.
            homeFailed = true
            judge()
        }
        rounds += 1
        began = Date()
        homeFailed = onlyFar
        homeRests = false
        if !onlyFar { tryHome(word) }
        if far == nil, let server = word.relay { tryFar(server) }
        if onlyFar { settle() }
        again(in: 8)
    }

    private func tryHome(_ word: MacWord) {
        let host = word.hosts[nextHost % word.hosts.count]
        let link = MacLink(host: host, word: word)
        home = link
        link.opened = { [weak self, weak link] in
            guard let self, let link, self.home === link else { return }
            self.take(link)
        }
        link.line = { [weak self, weak link] data in
            guard let self, let link, self.mac === link else { return }
            self.hear(data)
        }
        link.ended = { [weak self, weak link] how in
            guard let self, let link, self.home === link else { return }
            self.homeEnded(link, how, host: host, word: word)
        }
        link.start()
        // The Mac's own name means nothing outside its network, and nobody says so: after three seconds an
        // attempt that has not come through is taken for a no.
        Task { [weak self, weak link] in
            try? await Task.sleep(for: .seconds(3))
            guard let self, let link, self.home === link, self.mac !== link else { return }
            link.close()
            self.homeEnded(link, .unreachable, host: host, word: word)
        }
    }

    private func homeEnded(_ link: MacLink, _ how: MacLink.Ended, host: String, word: MacWord) {
        home = nil
        if mac === link { lost() }
        switch how {
        case .refused:
            // Something is there and nothing listens. By its own name that is the Mac with an empty queue;
            // by a number alone it may be anybody's machine in a network that is not his.
            homeFailed = true
            homeRests = host.hasSuffix(".local") || word.relay == nil
            settle()
        case .unreachable:
            nextHost += 1
            if nextHost % word.hosts.count == 0 {
                homeFailed = true
                homeRests = false
                settle()
            } else if mac == nil {
                tryHome(word)
            }
        case .lost:
            again(in: 0.3)
        }
    }

    private func tryFar(_ server: RelayWord) {
        guard let room = RelayLink(word: server) else { return }
        far = room
        farFailed = false
        room.held = { [weak self, weak room] kept in
            guard let self, let room, self.far === room else { return }
            self.held(kept)
        }
        room.macChanged = { [weak self, weak room] state in
            guard let self, let room, self.far === room else { return }
            self.serverTrouble = nil
            // The Mac that comes next knows this phone only by a hello: each time it is away, the room holds one first.
            if state != .here { self.greeted = nil }
            defer { self.handOut() }
            if self.mac === room {
                guard state != .here else { return }
                // The Mac left the room: its queue is not shown any more, and it is looked for again.
                self.lost()
                self.queue = nil
                self.link = state == .resting ? .nothingWaits : .away
                return self.again(in: 1)
            }
            self.settle()
            if self.mac == nil, state == .here {
                // Home has not said no yet: it gets its head start, and then the room is taken.
                let left = self.headStart - Date().timeIntervalSince(self.began)
                self.soon?.cancel()
                self.soon = Task { [weak self] in
                    try? await Task.sleep(for: .seconds(max(left, 0) + 0.05))
                    guard !Task.isCancelled else { return }
                    self?.settle()
                }
            }
        }
        room.line = { [weak self, weak room] data in
            guard let self, let room, self.mac === room else { return }
            self.hear(data)
        }
        room.ended = { [weak self, weak room] why in
            guard let self, let room, self.far === room else { return }
            self.far = nil
            self.farFailed = true
            self.serverTrouble = why
            if self.mac === room {
                self.lost()
                return self.again(in: 0.5)
            }
            self.settle()
            // The room is tried again by itself: home may take long to say no, and the first try may have come
            // in the moment between two networks.
            self.farRetry?.cancel()
            self.farRetry = Task { [weak self] in
                try? await Task.sleep(for: .seconds(1.5))
                guard !Task.isCancelled, let self, self.isActive, self.mac == nil, self.far == nil, let server = self.word?.relay else { return }
                self.tryFar(server)
            }
        }
        room.start()
    }

    /// Chooses with what is known by now: the room is taken when the Mac is in it and home has said no or
    /// has had its head start.
    private func settle() {
        guard isActive, mac == nil else { return }
        if let far, far.mac == .here, homeFailed || Date().timeIntervalSince(began) >= headStart {
            return take(far)
        }
        judge()
        if homeFailed { again(in: homeRests ? 2 : 3) }
    }

    /// With no way to the Mac, what the screen says of it: nothing waits, or the Mac is not there. Not before
    /// home has said no; and while the room has not said how things are, the looking goes on a moment more.
    private func judge() {
        guard mac == nil, homeFailed else { return }
        if let far, far.mac == .unknown, !homeRests { return }
        queue = nil
        link = homeRests || far?.mac == .resting ? .nothingWaits : .away
    }

    /// A way answers: it is the one in use from now, and the other is let go.
    private func take(_ wire: MacWire) {
        retry?.cancel()
        soon?.cancel()
        farRetry?.cancel()
        rounds = 0
        mac = wire
        if wire === far {
            home?.close()
            home = nil
        } else {
            far?.close()
            far = nil
        }
        var hello: [String: Any] = ["token": word?.token ?? "", "device": UIDevice.current.name, "front": true]
        if let pushToken { hello["push"] = pushToken }
        wire.say(["hello": hello])
    }

    /// The way that was in use is gone: nothing asked of it will be answered, and the queue on the screen is
    /// not the Mac's word of now.
    private func lost() {
        mac = nil
        isFresh = false
        failAnswers()
    }

    private func again(in seconds: Double) {
        retry?.cancel()
        retry = Task { [weak self] in
            try? await Task.sleep(for: .seconds(seconds))
            guard !Task.isCancelled else { return }
            self?.connect()
        }
    }

    private func hear(_ data: Data) {
        guard let line = try? JSONDecoder().decode(MacLine.self, from: data) else { return }
        if let welcome = line.welcome {
            link = .connected
            nextHost = 0
            if let name = welcome.mac, !name.isEmpty { macName = name }
            // Said at home: where the two meet when the phone is not there. Kept for the next time.
            if let server = welcome.relay, var known = word, known.relay != server {
                known.relay = server
                known.keep()
                word = known
            }
            askForNotifications()
        }
        if let queue = line.queue {
            link = .connected
            let wasFresh = isFresh
            isFresh = true
            if !wasFresh { handOut() }  // the Mac has said its word on this way: what was kept goes first
            if queue != self.queue { self.queue = queue }
            forget(gone: queue)
            if let wanted, queue.checks?.contains(where: { $0.id == wanted }) == true {
                self.wanted = nil
                if queue.active != wanted { press("take", ticket: wanted) }
            }
        }
        if let id = line.file, let waiting = answers.removeValue(forKey: id) {
            if let name = line.name, let text = line.data, let bytes = Data(base64Encoded: text) {
                waiting.resume(returning: (name, bytes))
            } else {
                refusal = line.error
                waiting.resume(returning: nil)
            }
        }
    }

    /// Nobody answers what was asked of a Mac that is gone.
    private func failAnswers() {
        let waiting = answers
        answers = [:]
        waiting.values.forEach { $0.resume(returning: nil) }
        fetching = []
    }

    // MARK: What is pressed

    /// One press, for the window on the Mac to do. A comment not sent yet goes first: the window may leave its item.
    func press(_ doing: String, ticket: String? = nil, label: String? = nil, number: Int? = nil, how: String? = nil,
               values: [[String]]? = nil, path: String? = nil) {
        guard let ticket = ticket ?? queue?.active else { return }
        flushNote()
        var word: [String: Any] = ["do": doing, "ticket": ticket]
        if let label { word["label"] = label }
        if let number { word["number"] = number }
        if let how { word["how"] = how }
        if let values { word["values"] = values }
        if let path { word["path"] = path }
        // Going from check to check means nothing later: only what he answered is kept for the Mac.
        if ["take", "go", "postpone"].contains(doing) { return mac?.say(word) ?? () }
        tell(word)
    }

    /// A word for the Mac: said at once while the way is open and nothing kept waits before it, else kept.
    private func tell(_ word: [String: Any]) {
        if let mac, isFresh, outbox.isEmpty { return mac.say(word) }
        keep(word)
    }

    private func keep(_ word: [String: Any]) {
        var late = word
        late["late"] = true
        guard let data = try? JSONSerialization.data(withJSONObject: late) else { return }
        // A comment typed on is one word: its latest text stands for the ones before it.
        if word["do"] as? String == "note", let last = outbox.last, !intoRoom.contains(where: { $0.id == last.id }),
           let before = last.said, before["do"] as? String == "note",
           before["ticket"] as? String == word["ticket"] as? String, before["label"] as? String == word["label"] as? String {
            outbox.removeLast()
        }
        outbox.append(Kept(id: UUID().uuidString, word: data))
        Kept.save(outbox)
        handOut()
    }

    /// Sends what was kept by the way there is now: to the Mac itself, or into the server's room, which holds it
    /// for the Mac. A picture stays on the phone until the Mac itself takes it: the room holds presses.
    func handOut() {
        guard !outbox.isEmpty else { return }
        if let mac, isFresh {
            for kept in outbox { if let word = kept.said { mac.say(word) } }
            outbox = []
            intoRoom = []
            Kept.save(outbox)
            return
        }
        guard mac == nil, let far, far.holds, far.mac == .gone || far.mac == .resting else { return }
        if greeted !== far {
            greeted = far
            intoRoom = []
            // The Mac takes nothing from a phone that has not said the key: the room holds the hello first. Not in
            // front: the Mac is not to think the app open when it comes.
            let hello = far.sayCounting(["hello": ["token": word?.token ?? "", "device": UIDevice.current.name, "front": false]])
            intoRoom.append((id: "", frames: hello, heard: 0, refused: false))
        }
        for kept in outbox where !intoRoom.contains(where: { $0.id == kept.id }) {
            guard let said = kept.said, said["shot"] == nil else { continue }
            intoRoom.append((id: kept.id, frames: far.sayCounting(said), heard: 0, refused: false))
        }
    }

    /// The server's word of one more frame said into the room: held, or not. A word all of whose frames it held
    /// is the Mac's now; one it did not hold stays on the phone.
    private func held(_ kept: Bool) {
        guard !intoRoom.isEmpty else { return }
        intoRoom[0].heard += 1
        if !kept { intoRoom[0].refused = true }
        guard intoRoom[0].heard >= intoRoom[0].frames else { return }
        let done = intoRoom.removeFirst()
        if !done.refused, !done.id.isEmpty {
            outbox.removeAll { $0.id == done.id }
            Kept.save(outbox)
        }
    }

    /// The comment of an item as it was last written here and is not with the Mac yet, if there is one.
    func pendingNote(ticket: String, label: String) -> String? {
        if let note, note.ticket == ticket, note.label == label { return note.text }
        return outbox.reversed().lazy.compactMap(\.said).first {
            $0["do"] as? String == "note" && $0["ticket"] as? String == ticket && $0["label"] as? String == label
        }?["text"] as? String
    }

    /// The comment of an item, as it is typed: told to the Mac a moment after the last letter.
    func write(_ text: String, ticket: String, label: String) {
        note = (ticket, label, text)
        noteTimer?.cancel()
        noteTimer = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            self?.flushNote()
        }
    }

    func flushNote() {
        noteTimer?.cancel()
        guard let note else { return }
        self.note = nil
        tell(["do": "note", "ticket": note.ticket, "label": note.label, "text": note.text])
    }

    /// A picture for the comment of an item: the Mac keeps it with the screenshots pasted there.
    func attach(_ image: UIImage, ticket: String, label: String) {
        let longest = max(image.size.width, image.size.height) * image.scale
        let scale = min(1, 2000 / max(longest, 1))
        let size = CGSize(width: image.size.width * image.scale * scale, height: image.size.height * image.scale * scale)
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let drawn = UIGraphicsImageRenderer(size: size, format: format).image { _ in image.draw(in: CGRect(origin: .zero, size: size)) }
        guard let png = drawn.pngData() else { return }
        flushNote()
        tell(["shot": ["ticket": ticket, "label": label, "data": png.base64EncodedString()]])
    }

    // MARK: Files of the Mac

    private func ask(_ path: String) async -> (name: String, data: Data)? {
        guard let mac, isFresh else { return nil }
        asked += 1
        let id = asked
        return await withCheckedContinuation { continuation in
            answers[id] = continuation
            mac.say(["get": path, "id": id])
        }
    }

    /// A picture of an item, once it has come; asked for at the first look.
    func picture(_ path: String) -> UIImage? {
        if let known = pictures[path] { return known }
        if !fetching.contains(path), failed[path] == nil, isFresh {
            fetching.insert(path)
            Task {
                let got = await ask(path)
                fetching.remove(path)
                if let got, let image = UIImage(data: got.data) {
                    pictures[path] = image
                } else if isFresh {
                    failed[path] = "не пришла"
                }
            }
        }
        return nil
    }

    /// The picture did not come, and will not on this connection: what stands for it is shown instead.
    func isMissing(_ path: String) -> Bool { failed[path] != nil }

    /// The text of an item's live page. Asked once the Mac has said its queue on this connection: a moment's wait
    /// when the app has just come back in front.
    func page(_ path: String) async -> String? {
        for _ in 0..<40 where !isFresh { try? await Task.sleep(for: .milliseconds(150)) }
        guard let got = await ask(path) else { return nil }
        return String(data: got.data, encoding: .utf8)
    }

    /// Shows a file of the Mac over the queue, in the app's own viewer: a picture to pinch, a note or a source
    /// as its text, anything else as the system's web view draws it.
    func show(file path: String) {
        let name = (path as NSString).lastPathComponent
        if let known = pictures[path] {
            viewing = Shown(name: name, what: .picture(known))
            return
        }
        Task {
            refusal = nil
            guard let got = await ask(path) else {
                // He tapped it and is told why it did not open; a picture or a page that did not come by
                // itself says so in its own place, with no notice over the screen.
                trouble = refusal ?? "Мак не отдал файл"
                return
            }
            if let image = UIImage(data: got.data) {
                viewing = Shown(name: got.name, what: .picture(image))
            } else if !["pdf", "html", "htm", "rtf", "webarchive"].contains((got.name as NSString).pathExtension.lowercased()),
                      got.data.count < 2_000_000, !got.data.contains(0), let text = String(data: got.data, encoding: .utf8) {
                viewing = Shown(name: got.name, what: .text(text))
            } else {
                let folder = scratch.appendingPathComponent(String(abs(path.hashValue)), isDirectory: true)
                try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                let file = folder.appendingPathComponent(got.name)
                guard (try? got.data.write(to: file)) != nil else { return }
                viewing = Shown(name: got.name, what: .file(file))
            }
        }
    }

    /// Copies a line of an item, all of it, into the phone's clipboard; the line says so for a second and a half.
    func copy(_ line: String) {
        UIPasteboard.general.string = line
        UINotificationFeedbackGenerator().notificationOccurred(.success)
        copied = line
        copies += 1
        let this = copies
        Task {
            try? await Task.sleep(for: .seconds(1.5))
            if copies == this { copied = nil }
        }
    }

    /// Opens what an item offers: a page in the browser, another app by its address, a file of the Mac here.
    func open(_ target: Open) {
        if target.kind == "file" { return show(file: target.link) }
        guard let url = URL(string: target.link) else { return }
        UIApplication.shared.open(url) { [weak self] opened in
            guard !opened else { return }
            MainActor.assumeIsolated {
                // A chat of the Claude app is also a page of claude.ai, for a phone whose app does not take the address.
                if url.scheme == "claude", let web = URL(string: "https://claude.ai/" + (url.host ?? "") + url.path) {
                    UIApplication.shared.open(web)
                } else {
                    self?.trouble = "На телефоне нечем открыть «\(target.title)»"
                }
            }
        }
    }

    /// Opens a link he tapped in a text of a check: a file of the Mac here, a page in the browser, another app by
    /// its address.
    func open(address: URL) {
        if address.isFileURL { return show(file: address.path) }
        let web = ["http", "https"].contains(address.scheme?.lowercased() ?? "")
        open(Open(title: address.host ?? address.scheme ?? address.absoluteString, link: address.absoluteString, kind: web ? "web" : "app"))
    }

    /// The links written into the texts he reads at an item, from the top: the line about the task, the item, what
    /// stands under it, its facts.
    func links(of item: Item, in check: Check) -> [String] {
        ([check.about ?? "", item.text, item.details ?? ""] + (item.facts ?? [])).flatMap { Linked.pieces($0).compactMap(\.link) }
    }

    /// What belonged to checks that are gone goes with them.
    private func forget(gone queue: QueueWord) {
        let named = Set((queue.checks ?? []).flatMap { $0.items.flatMap { $0.pictures ?? [] } } + (queue.marks ?? []).flatMap { $0.shots ?? [] }
            + (queue.checks ?? []).compactMap(\.appIcon))
        for path in pictures.keys where !named.contains(path) { pictures[path] = nil }
        for path in failed.keys where !named.contains(path) { failed[path] = nil }
        let waitingChecks = (queue.checks ?? []).filter { $0.onPhone != true }
        let ids = Set(waitingChecks.map(\.id))
        // A notification of a check that is answered by now has nothing to say.
        UNUserNotificationCenter.current().getDeliveredNotifications { delivered in
            let stale = delivered.filter { ($0.request.content.userInfo["ticket"] as? String).map { !ids.contains($0) } ?? false }
                .map(\.request.identifier)
            if !stale.isEmpty { UNUserNotificationCenter.current().removeDeliveredNotifications(withIdentifiers: stale) }
        }
        UNUserNotificationCenter.current().setBadgeCount(waitingChecks.count)
    }

    // MARK: Notifications

    /// Asked once the Mac is heard for the first time: by then he knows what the app is for.
    private func askForNotifications() {
        // A launch that presses by itself has nobody at the screen to answer the system's question.
        guard !askedForNotifications, !isPlaying else { return }
        askedForNotifications = true
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound]) { granted, _ in
            guard granted else { return }
            Task { @MainActor in UIApplication.shared.registerForRemoteNotifications() }
        }
    }

    /// Where Apple delivers this phone's notifications: the Mac is told, and tells Apple.
    func registered(push token: String) {
        guard token != pushToken else { return }
        pushToken = token
        mac?.say(["push": token, "device": UIDevice.current.name])
    }

    // MARK: A launch with a queue of its own

    /// `--queue-word VALUE` among the launch's arguments: the queue is the one written there, as the Mac says its
    /// own (the JSON of its word, in base64url without padding), and no Mac is looked for. For a look at the screen
    /// with a text of one's choosing and nobody at the Mac: a probe on the phone with its screen taken by
    /// `devicectl`. Nothing pressed in it goes anywhere, and its pictures and files do not come. It lasts until
    /// the app is put away: back in front, the app looks for the Mac as ever, so that such a launch is not what
    /// he finds in the app afterwards. Under a checklist on the band it stays for as long as that check does:
    /// a hand check of how a certain check looks, with that check under the band and no second one in his window.
    func show(word value: String) {
        var text = value.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        text += String(repeating: "=", count: (4 - text.count % 4) % 4)
        guard let data = Data(base64Encoded: text), let word = try? JSONDecoder().decode(QueueWord.self, from: data) else {
            trouble = "В --queue-word не очередь: нужен JSON слова Мака в base64url."
            return
        }
        given = word
        macName = word.mac ?? ""
    }

    /// The checklist on the band has ended: a queue given for it goes with it.
    func checklistEnded() {
        guard given != nil, !hasChecklist() else { return }
        given = nil
        queue = nil
        link = .searching
        rounds = 0
        connect()
    }

    // MARK: A launch that presses by itself

    /// `--queue-play STEPS` among the launch's arguments: the app does those steps by itself, one a second,
    /// once the Mac's queue has come. For a run with nobody at the screen: `answer=0` presses the item's first
    /// answer (`answer=-1` his own), `note=TEXT` writes its comment, `go=LABEL` comes to an item, `take=N` takes
    /// the N-th check of the queue, `file=N` shows what the item offers to open, `copy=N` copies the item's N-th
    /// line as a tap on it does, `link=N` opens the N-th link of its texts as a tap on it does, `list` puts the list of the
    /// checks up or away, `postpone`, `finish=done`, `away=N` puts the queue away for N seconds as leaving the app
    /// and coming back does, `far` goes on with the server's room only, as away from home, and `home` ends that.
    func play(_ steps: String) {
        isPlaying = true
        Task {
            while queue?.activeCheck == nil { try? await Task.sleep(for: .milliseconds(200)) }
            for step in steps.split(separator: ",").map(String.init) {
                try? await Task.sleep(for: .seconds(1))
                let parts = step.split(separator: "=", maxSplits: 1).map(String.init)
                let value = parts.count > 1 ? parts[1] : ""
                if parts[0] == "far" || parts[0] == "home" {
                    onlyFar = parts[0] == "far"
                    letGo()
                    rounds = 0
                    connect()
                    for _ in 0..<50 where queue?.activeCheck == nil { try? await Task.sleep(for: .milliseconds(200)) }
                    continue
                }
                if parts[0] == "away" {
                    sleep()
                    try? await Task.sleep(for: .seconds(Double(value) ?? 3))
                    wake()
                    for _ in 0..<50 where queue?.activeCheck == nil { try? await Task.sleep(for: .milliseconds(200)) }
                    continue
                }
                guard let queue, let check = queue.activeCheck else { continue }
                let item = check.items[min(max(queue.index ?? 0, 0), check.items.count - 1)]
                switch parts[0] {
                case "answer": press("answer", label: item.label, number: Int(value) ?? 0)
                case "note": write(value, ticket: check.id, label: item.label)
                case "go": press("go", label: value)
                case "take":
                    if let number = Int(value), let other = queue.checks, other.indices.contains(number) { press("take", ticket: other[number].id) }
                case "file":
                    if let number = Int(value), let opens = item.opens, opens.indices.contains(number) { open(opens[number]) }
                case "copy":
                    if let number = Int(value), let lines = item.copies, lines.indices.contains(number) { copy(lines[number]) }
                case "link":
                    let found = links(of: item, in: check)
                    if let number = Int(value), found.indices.contains(number), let address = Linked.address(found[number]) { open(address: address) }
                case "list": listUp.toggle()
                case "postpone": press("postpone")
                case "finish": press("finish", how: value == "done" ? "done" : "rework")
                default: break
                }
            }
        }
    }

    /// A notification of a check was tapped.
    func wants(_ ticket: String) {
        if let queue, queue.checks?.contains(where: { $0.id == ticket }) == true {
            if queue.active != ticket { press("take", ticket: ticket) }
        } else {
            wanted = ticket
        }
    }
}

/// One word kept on the phone for the Mac, as it will be said: JSON with "late" in it.
struct Kept: Codable, Equatable {
    var id: String
    var word: Data

    var said: [String: Any]? { try? JSONSerialization.jsonObject(with: word) as? [String: Any] }

    /// Where the words wait: the app's own folder, so that a launch after a crash or a closed app has them.
    static var file: URL {
        let folder = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return folder.appendingPathComponent("conveyor-outbox.json")
    }

    static func load() -> [Kept] {
        guard let data = try? Data(contentsOf: file) else { return [] }
        return (try? JSONDecoder().decode([Kept].self, from: data)) ?? []
    }

    static func save(_ kept: [Kept]) {
        if kept.isEmpty {
            try? FileManager.default.removeItem(at: file)
            return
        }
        try? FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        if let data = try? JSONEncoder().encode(kept) { try? data.write(to: file, options: .atomic) }
    }
}
