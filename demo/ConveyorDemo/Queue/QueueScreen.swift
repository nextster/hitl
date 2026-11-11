import PhotosUI
import SwiftUI

/// The type of an item, as he set it on a live page on 2026-10-11, the day he set the window's on the Mac
/// («давай шрифты в конвейере покрутим»): the sizes of the question, of what stands under it with the facts
/// and of an answer, and the room between the lines of a text of several.
enum Face {
    static let question: CGFloat = 20
    static let details: CGFloat = 16
    static let answer: CGFloat = 17
    static let lineRoom: CGFloat = 3
}

/// The look of the check window on the Mac, "graphite", on the phone.
enum Ink {
    static func hex(_ value: UInt32) -> Color {
        Color(red: Double((value >> 16) & 0xFF) / 255, green: Double((value >> 8) & 0xFF) / 255, blue: Double(value & 0xFF) / 255)
    }
    static let panel = hex(0x1A1B1F)
    static let bar = hex(0x15161A)
    static let barEdge = hex(0x2A2B31)
    static let text = hex(0xF3F3F5)
    static let who = hex(0xD7D8DE)
    static let heading = hex(0xC9CAD2)
    /// What stands under a question and the facts: the heading's grey a third of the way to white, which is
    /// how bright he set what he reads on a live page on 2026-10-11.
    static let read = hex(0xD6D6DD)
    static let soft = hex(0xA4A6B0)
    static let quiet = hex(0x9A9CA7)
    static let dim = hex(0x8F919C)
    static let faint = hex(0x6F717C)
    static let link = hex(0xB9BBC5)
    static let rule = hex(0x33343C)
    static let row = hex(0x24252B)
    static let rowEdge = hex(0x383942)
    static let key = hex(0x2A2B32)
    static let no = hex(0xFF5A52)
    /// What takes him somewhere else in the app: the way back to the queue.
    static let way = hex(0x4C8DFF)
    /// A link in a text he reads: the blue of one in the window on the Mac, with a line under it, his pick there.
    static let linked = hex(0x6CA6FF)
    /// «Не понял»: the colour of an explanation, which is what it asks for.
    static let unclear = hex(0xFFD60A)
    /// A line that has just been copied says so in this.
    static let done = hex(0x34C759)

    /// Each kind of check has its colour: the dot, the progress, the chosen answer.
    static func tint(of kind: String?) -> Color {
        switch kind {
        case "question": hex(0x4C8DFF)
        case "plan": hex(0xA78BFA)
        case "action": hex(0xFF9F0A)
        case "result": hex(0x34C759)
        case "explain": hex(0xFFD60A)
        default: hex(0x22B8A0)
        }
    }
}

struct QueueScreen: View {
    let model: QueueModel

    var body: some View {
        @Bindable var model = model
        ZStack {
            Ink.panel.ignoresSafeArea()
            if let queue = model.queue, let check = queue.activeCheck, let marks = queue.marks, marks.count == check.items.count {
                // The queue and a check of it, as a list and its row are in any app of the phone: «‹ Очередь» goes
                // to the list, a row of the list comes back to its check.
                if model.listUp {
                    QueueList(model: model, queue: queue, note: nil, close: { model.listUp = false })
                        .transition(.move(edge: .leading))
                } else {
                    CheckView(model: model, queue: queue, check: check, marks: marks, showList: { model.listUp = true })
                        .transition(.move(edge: .trailing))
                }
            } else if let queue = model.queue, !(queue.checks ?? []).isEmpty {
                // The checks are there and the window on the Mac is not up yet: it comes up by itself.
                QueueList(model: model, queue: queue, note: "Окно на Маке поднимается…", close: nil)
            } else {
                Resting(model: model)
            }
        }
        .animation(.snappy(duration: 0.26), value: model.listUp)
        // A sheet, so that a swipe down puts it away too: «круто но свайпом вниз было бы вообще супер».
        .sheet(item: $model.viewing) { shown in
            FileViewer(shown: shown) { model.viewing = nil }
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
                .presentationBackground(Ink.panel)
        }
        .alert("Не вышло", isPresented: Binding(get: { model.trouble != nil }, set: { if !$0 { model.trouble = nil } })) {
            Button("Понятно", role: .cancel) {}
        } message: {
            Text(model.trouble ?? "")
        }
        .tint(Ink.text)
    }
}

/// Nothing to answer: why, in a line.
private struct Resting: View {
    let model: QueueModel

    private var words: (symbol: String, title: String, body: String) {
        switch model.link {
        case .unpaired:
            ("cable.connector", "Телефон ещё не знает Мак", "Подключи телефон к Маку кабелем: Мак сам запишет сюда свой адрес и ключ.")
        case .searching: ("dot.radiowaves.left.and.right", "Ищу Мак…", "")
        case .connected, .nothingWaits: ("checkmark.circle", "Проверок нет", "Когда чат что-то спросит, вопрос появится здесь.")
        case .away:
            if !model.hasServer {
                ("moon.zzz", "Мак не отвечает", "Он спит, или телефон не в домашней сети.")
            } else if let trouble = model.serverTrouble {
                // The phone did not reach the server at all: that is the phone's network, not the Mac.
                ("wifi.exclamationmark", "Нет связи с сервером", "Телефон не достаёт до сервера, через который говорит с Маком вне дома.\n\n" + trouble)
            } else {
                ("moon.zzz", "Мак не на связи", "Он спит, выключен или без сети. Проснётся, и очередь появится сама.")
            }
        }
    }

    var body: some View {
        VStack(spacing: 12) {
            Spacer()
            Image(systemName: words.symbol).font(.system(size: 44, weight: .light)).foregroundStyle(Ink.faint)
            Text(words.title).font(.system(size: 22, weight: .semibold)).foregroundStyle(Ink.text)
            if !words.body.isEmpty {
                Text(words.body).font(.system(size: 15)).foregroundStyle(Ink.quiet).multilineTextAlignment(.center)
            }
            Spacer()
        }
        .padding(28)
        .accessibilityIdentifier("queue.resting")
    }
}

/// The app a check came from, before the project, as the window on the Mac has it: «слева от имени проекта только
/// символом если можно. если сложно то текстом коротенечко». The icon is the Mac's own of that app, asked for as an
/// item's picture is; where the Mac has no such app, the app is said by its name. While the icon is on its way
/// its place stands empty, so that a word does not turn into a picture under his eye.
private struct AppMark: View {
    let model: QueueModel
    let check: Check
    static let side: CGFloat = 22

    var body: some View {
        if let agent = check.agent, !agent.isEmpty {
            if let path = check.appIcon, !path.isEmpty, !model.isMissing(path) {
                Group {
                    if let icon = model.picture(path) {
                        Image(uiImage: icon).resizable().interpolation(.high)
                    } else {
                        Color.clear
                    }
                }
                .frame(width: Self.side, height: Self.side)
                .accessibilityLabel(agent)
                .accessibilityIdentifier("queue.app")
            } else {
                Text(agent)
                    .font(.system(size: 12.5, weight: .medium))
                    .foregroundStyle(Ink.soft)
                    .lineLimit(1)
                    .fixedSize()
                    .accessibilityIdentifier("queue.app")
            }
        }
    }
}

/// One check, one item at a time: what the window on the Mac shows, with buttons for a finger where it has digits.
/// The project a check is about, before the chat's name: the white tag of the window on the Mac.
private struct ProjectTag: View {
    let name: String?

    var body: some View {
        if let name, !name.isEmpty {
            Text(name)
                .font(.system(size: 12.5, weight: .bold))
                .foregroundStyle(Ink.panel)
                .lineLimit(1)
                .fixedSize()
                .padding(.horizontal, 7)
                .padding(.vertical, 2)
                .background(Ink.text, in: RoundedRectangle(cornerRadius: 6))
                .accessibilityIdentifier("queue.project")
        }
    }
}

private struct CheckView: View {
    let model: QueueModel
    let queue: QueueWord
    let check: Check
    let marks: [Mark]
    var showList: () -> Void

    private var index: Int { min(max(queue.index ?? 0, 0), check.items.count - 1) }
    private var item: Item { check.items[index] }
    private var mark: Mark { marks[index] }
    /// The colour and the word at the top are the item's: in a check of several kinds they change as he goes.
    private var tint: Color { Ink.tint(of: item.kind ?? check.kind) }
    private var others: [Check] { (queue.checks ?? []).filter { $0.id != check.id } }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    head
                    if model.isOffline || !model.outbox.isEmpty { KeptLine(count: model.outbox.count) }
                    ItemView(model: model, check: check, item: item, mark: mark, tint: tint)
                        .id(check.id + "/" + item.label)
                }
                .padding(.horizontal, 18)
                .padding(.top, 10)
                .padding(.bottom, 16)
                .pageWide()
                // A tap on a link in a text of the check: a file of the Mac is shown here, anything else opens as
                // what an item offers to open does.
                .environment(\.openURL, OpenURLAction { address in
                    model.open(address: address)
                    return .handled
                })
            }
            .scrollDismissesKeyboard(.interactively)
            foot
        }
    }

    /// «Как обычное приложение iPhone», his pick of three pictures: the way back to the queue with how many
    /// wait there, the check's title large, its kind as a coloured tag before what the check is about.
    private var head: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Button(action: showList) {
                    HStack(spacing: 5) {
                        Image(systemName: "chevron.left").font(.system(size: 16, weight: .semibold))
                        Text("Очередь").font(.system(size: 17))
                        if !others.isEmpty {
                            Text("\(others.count)").font(.system(size: 13, weight: .bold).monospacedDigit()).foregroundStyle(Ink.text)
                                .padding(.horizontal, 7).frame(minWidth: 23, minHeight: 23)
                                .background(Ink.way, in: Capsule())
                                .padding(.leading, 2)
                        }
                    }
                    .foregroundStyle(Ink.way)
                    .frame(minHeight: 40).contentShape(Rectangle())
                }
                .buttonStyle(Pressing())
                .accessibilityIdentifier("queue.waiting")
                Spacer(minLength: 8)
                if check.items.count > 1 {
                    Text("\(index + 1) из \(check.items.count)").font(.system(size: 15).monospacedDigit()).foregroundStyle(Ink.quiet)
                        .lineLimit(1).fixedSize()
                }
            }
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                // The icon stands by the middle of the line's letters, not on their baseline.
                AppMark(model: model, check: check)
                    .alignmentGuide(.firstTextBaseline) { $0[VerticalAlignment.center] + 5 }
                ProjectTag(name: check.projectName)
                Text(check.who).font(.system(size: 14)).foregroundStyle(Ink.soft).lineLimit(2)
            }
            .padding(.top, 4)
            if let title = check.title, !title.isEmpty {
                Text(title).font(.system(size: 24, weight: .bold)).foregroundStyle(Ink.text)
                    .fixedSize(horizontal: false, vertical: true).padding(.top, 8)
            }
            about.padding(.top, 8)
            Rectangle().fill(Ink.rule).frame(height: 1).padding(.top, 14)
        }
    }

    /// The kind of what is asked, a tag in the line, and what the check is about after it.
    private var about: some View {
        let kind = check.onPhone == true ? "на телефоне" : item.kindTitle ?? check.kindTitle ?? ""
        let tag = kind.isEmpty ? Text("") : Text("\(Image(uiImage: KindTag.drawn(kind, tint: UIColor(tint))))  ").baselineOffset(-4)
        return Text("\(tag)\(Text(Linked.text(check.about ?? "")))")
            .font(.system(size: 15)).foregroundStyle(Ink.who).tint(Ink.linked)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityIdentifier("queue.about")
    }

    /// Under the item, in two lines: the dashes with the arrows, then how the check is put off or ended, the
    /// two buttons sharing the screen's width. It was three lines of buttons 50 points tall: «хотелось бы ui
    /// кнопок внизу не настолько огромный».
    private var foot: some View {
        VStack(spacing: 6) {
            if check.items.count > 1 {
                HStack(spacing: 6) {
                    arrow("chevron.left", to: index - 1)
                    HStack(spacing: 5) {
                        ForEach(Array(marks.enumerated()), id: \.offset) { number, one in
                            Capsule().fill(color(of: one, in: Ink.tint(of: check.items[number].kind ?? check.kind)) ?? Ink.rule)
                                .frame(height: 5)
                                .overlay(Capsule().stroke(Ink.text, lineWidth: number == index ? 1.5 : 0).padding(-3))
                                .frame(maxWidth: .infinity, minHeight: 28)
                                .contentShape(Rectangle())
                                .onTapGesture { go(to: number) }
                        }
                    }
                    arrow("chevron.right", to: index + 1)
                }
            }
            HStack(spacing: 10) {
                if queue.canPostpone == true {
                    Button("Отложить") { model.press("postpone") }
                        .buttonStyle(EndStyle(fill: Ink.key, ink: Ink.who))
                        .accessibilityIdentifier("queue.postpone")
                }
                if queue.canFinish == true {
                    Button("Готово") { model.press("finish", how: "done") }
                        .buttonStyle(EndStyle(fill: tint, ink: Ink.bar))
                        .accessibilityIdentifier("queue.done")
                } else {
                    Button("Доработать") { model.press("finish", how: "rework") }
                        .buttonStyle(EndStyle(fill: Ink.key, ink: Ink.who))
                        .accessibilityIdentifier("queue.rework")
                }
            }
        }
        .padding(.horizontal, 18)
        .padding(.top, 6)
        .padding(.bottom, 8)
        .background(Ink.panel)
    }

    private func arrow(_ symbol: String, to number: Int) -> some View {
        Button { go(to: number) } label: {
            Image(systemName: symbol).font(.system(size: 17, weight: .semibold)).frame(width: 44, height: 36).contentShape(Rectangle())
        }
        .foregroundStyle(check.items.indices.contains(number) ? Ink.link : Ink.rule)
        .disabled(!check.items.indices.contains(number))
    }

    private func go(to number: Int) {
        guard check.items.indices.contains(number), number != index else { return }
        model.press("go", label: check.items[number].label)
    }

    /// The colour of an item's answer; an answer in his own words is an answer, not a "no".
    private func color(of mark: Mark, in tint: Color) -> Color? {
        switch mark.state {
        case "ok": tint
        case "problem": mark.unclear == true ? Ink.unclear : mark.question == true ? tint : Ink.no
        case "skipped": Ink.faint
        default: nil
        }
    }
}

/// The word for a kind of check on its colour, as a picture: a picture can stand in a line of text, and the
/// words after it go on under it.
private enum KindTag {
    static func drawn(_ title: String, tint: UIColor) -> UIImage {
        let words = NSAttributedString(string: title, attributes: [.font: UIFont.systemFont(ofSize: 12.5, weight: .bold), .foregroundColor: tint])
        let size = words.size()
        let whole = CGSize(width: ceil(size.width) + 14, height: ceil(size.height) + 5)
        return UIGraphicsImageRenderer(size: whole).image { _ in
            tint.withAlphaComponent(0.24).setFill()
            UIBezierPath(roundedRect: CGRect(origin: .zero, size: whole), cornerRadius: 6).fill()
            words.draw(at: CGPoint(x: 7, y: 2.5))
        }
    }
}

private struct EndStyle: ButtonStyle {
    var fill: Color
    var ink: Color

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 15, weight: .semibold)).foregroundStyle(ink)
            .lineLimit(1).minimumScaleFactor(0.85)
            .padding(.horizontal, 13).frame(maxWidth: .infinity, minHeight: 44)
            .background(RoundedRectangle(cornerRadius: 13, style: .continuous).fill(fill))
            .contentShape(Rectangle())
            .opacity(configuration.isPressed ? 0.6 : 1)
    }
}

/// An item: its text and what stands under it, its answers, its comment, what it offers to open.
/// No way to the Mac: what he answers here is kept on the phone and goes by itself once there is one.
private struct KeptLine: View {
    let count: Int

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: "icloud.slash").font(.system(size: 14, weight: .semibold))
            Text(count > 0 ? "Нет связи с Маком. Ответы сохранены на телефоне и уйдут сами: \(count)"
                           : "Нет связи с Маком. Что ответишь, сохранится на телефоне и уйдёт само")
                .font(.system(size: 15))
                .fixedSize(horizontal: false, vertical: true)
        }
        .foregroundStyle(Ink.quiet)
        .padding(.top, 10)
        .accessibilityIdentifier("queue.kept")
    }
}

private struct ItemView: View {
    let model: QueueModel
    let check: Check
    let item: Item
    let mark: Mark
    let tint: Color
    @State private var draft = ""
    @State private var picked: PhotosPickerItem?
    @FocusState private var writing: Bool

    private var options: [String] { item.options ?? [] }
    private var isMany: Bool { item.many == true && !options.isEmpty }
    private var answers: [String] {
        Array((item.answers ?? check.answers ?? ["работает", "не работает", "пропустить"]).prefix(answerStates.count))
    }
    /// The lines he copies with a tap.
    private var copies: [String] { item.copies ?? [] }
    /// The facts as the item has them. The Mac says each line to copy among them too, for a build of this app that
    /// knows no such lines: here a line stands once, as what it is.
    private var facts: [String] { (item.facts ?? []).filter { !copies.contains($0) } }
    /// «Не понял» stands under the answers of every item, but not where it is there already, in an explanation,
    /// and not in a build's checklist that is on the phone: its band has three marks.
    private var offersUnclear: Bool { check.onPhone != true && (item.kind ?? check.kind) != "explain" }

    /// One row of the list of answers: what it says, whether it is taken, and what the window is told for it.
    private struct Answer: Identifiable {
        var id: String
        var title: String
        var box: Bool?
        var chosen: Color?
        var dim: Bool
        var number: Int
    }

    private var rows: [Answer] {
        var rows: [Answer] = []
        if options.isEmpty {
            for (number, title) in answers.enumerated() {
                rows.append(Answer(id: "queue.answer.\(number)", title: title.prefix(1).uppercased() + title.dropFirst(), box: nil,
                                   chosen: mark.state == answerStates[number] && mark.unclear != true ? [tint, Ink.no, Ink.faint][number] : nil,
                                   dim: number == 2, number: number))
            }
        } else {
            let ticked = Set(mark.choices ?? [])
            for (number, title) in options.enumerated() {
                rows.append(Answer(id: "queue.answer.\(number)", title: title, box: isMany ? ticked.contains(title) : nil,
                                   chosen: (isMany ? ticked.contains(title) : mark.choice == title) ? tint : nil, dim: false, number: number))
            }
            rows.append(Answer(id: "queue.answer.own", title: ownAnswer, box: isMany ? mark.other == true : nil,
                               chosen: (isMany ? mark.other == true : mark.state == "problem" && mark.unclear != true) ? tint : nil,
                               dim: true, number: -1))
        }
        if offersUnclear {
            rows.append(Answer(id: "queue.answer.unclear", title: unclearAnswer, box: nil, chosen: mark.unclear == true ? Ink.unclear : nil,
                               dim: true, number: unclearNumber))
        }
        return rows
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                if check.items.count > 1, !item.label.isEmpty {
                    Text(item.label + ".").font(.system(size: Face.question)).foregroundStyle(Ink.faint)
                }
                Text(Linked.text(item.text)).font(.system(size: Face.question, weight: .semibold)).foregroundStyle(Ink.text).tint(Ink.linked)
                    .lineSpacing(Face.lineRoom).fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
            }
            .padding(.top, 8)
            if let details = item.details, !details.isEmpty {
                Text(Linked.text(details)).font(.system(size: Face.details)).foregroundStyle(Ink.read).tint(Ink.linked).lineSpacing(Face.lineRoom)
                    .padding(.top, 8).textSelection(.enabled)
            }
            if !facts.isEmpty {
                VStack(alignment: .leading, spacing: 5) {
                    ForEach(facts, id: \.self) { fact in
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            Circle().fill(tint.opacity(0.8)).frame(width: 5, height: 5).alignmentGuide(.firstTextBaseline) { $0[.bottom] + 4 }
                            Text(Self.marked(fact)).font(.system(size: Face.details)).foregroundStyle(Ink.read).tint(Ink.linked)
                                .lineSpacing(Face.lineRoom)
                        }
                    }
                }
                .padding(.top, 10)
            }
            if let page = item.page {
                LivePage(model: model, path: page) { values in
                    model.press("values", ticket: check.id, label: item.label, values: values)
                }
                .padding(.top, 12)
                if let values = mark.values, !values.isEmpty {
                    Text("\(Text("Уйдёт чату с ответом: ").foregroundStyle(Ink.faint))\(Text(values.map { $0.joined(separator: " ") }.joined(separator: " · ")).foregroundStyle(Ink.soft))")
                        .font(.system(size: 12)).padding(.top, 5)
                }
            }
            if let pictures = item.pictures, !pictures.isEmpty {
                VStack(spacing: 8) {
                    ForEach(pictures, id: \.self) { path in
                        PictureView(model: model, path: path, tallest: pictures.count == 1 ? 220 : 160)
                    }
                }
                .padding(.top, 12)
            }
            if !copies.isEmpty {
                VStack(spacing: 8) {
                    ForEach(copies, id: \.self) { line in
                        CopyLine(line: line, copied: model.copied == line) { model.copy(line) }
                    }
                }
                .padding(.top, 12)
            }
            if isMany {
                Text("Можно несколько").font(.system(size: 12)).foregroundStyle(Ink.faint).padding(.top, 14)
            }
            // The answers as one list with lines between its rows, the way a phone's settings offer a choice.
            VStack(spacing: 0) {
                ForEach(Array(rows.enumerated()), id: \.element.id) { place, row in
                    if place > 0 { Rectangle().fill(Ink.rowEdge).frame(height: 1).padding(.leading, 16) }
                    AnswerRow(title: row.title, box: row.box, chosen: row.chosen, dim: row.dim) { answer(row.number) }
                        .accessibilityIdentifier(row.id)
                }
            }
            .background(Ink.row)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .padding(.top, isMany ? 6 : 16)
            comment
            if let opens = item.opens, !opens.isEmpty {
                Chips(opens: opens) { model.open($0) }.padding(.top, 12)
            }
        }
        .onAppear { draft = model.pendingNote(ticket: check.id, label: item.label) ?? mark.note ?? "" }
        .onChange(of: mark.note ?? "") { _, said in
            // What was written on the Mac comes here, unless he is writing here or what he wrote here has not
            // reached the Mac yet: the Mac's older word does not take its place.
            if !writing, model.pendingNote(ticket: check.id, label: item.label) == nil { draft = said }
        }
        .onChange(of: draft) { _, text in
            if writing, text != (mark.note ?? "") { model.write(text, ticket: check.id, label: item.label) }
        }
        .onChange(of: picked) { _, item in
            guard let item else { return }
            picked = nil
            Task {
                if let data = try? await item.loadTransferable(type: Data.self), let image = UIImage(data: data) {
                    model.attach(image, ticket: check.id, label: self.item.label)
                }
            }
        }
        .onDisappear { model.flushNote() }
    }

    private var comment: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .bottom, spacing: 8) {
                TextField("Комментарий", text: $draft, axis: .vertical)
                    .focused($writing)
                    .font(.system(size: 15.5)).foregroundStyle(Ink.text)
                    .lineLimit(1...8)
                    .padding(.horizontal, 14).padding(.vertical, 12)
                    .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.bar))
                    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).stroke(writing ? tint.opacity(0.7) : Ink.barEdge))
                    .accessibilityIdentifier("queue.note")
                if writing {
                    Button { writing = false } label: {
                        Image(systemName: "keyboard.chevron.compact.down").font(.system(size: 17)).frame(width: 44, height: 44).contentShape(Rectangle())
                    }
                    .foregroundStyle(Ink.link)
                } else if (mark.shots ?? []).count < 4 {
                    PhotosPicker(selection: $picked, matching: .images) {
                        Image(systemName: "photo.badge.plus").font(.system(size: 17)).frame(width: 44, height: 44).contentShape(Rectangle())
                    }
                    .foregroundStyle(Ink.link)
                    .accessibilityLabel("Добавить снимок к комментарию")
                }
            }
            if let shots = mark.shots, !shots.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(shots, id: \.self) { path in
                            ZStack(alignment: .topTrailing) {
                                PictureView(model: model, path: path, tallest: 84).frame(width: 84)
                                Button { model.press("unshot", ticket: check.id, label: item.label, path: path) } label: {
                                    Image(systemName: "xmark.circle.fill").font(.system(size: 18)).foregroundStyle(Ink.text, Ink.bar)
                                }
                                .padding(2)
                            }
                        }
                    }
                }
            }
        }
        .padding(.top, 12)
    }

    private func answer(_ number: Int) {
        model.press("answer", ticket: check.id, label: item.label, number: number)
        // A "no" and his own answer are worth a word: the comment is ready for it, as on the Mac.
        let asksWhy = number == unclearNumber || (options.isEmpty ? number == 1 : number < 0 && !(isMany && mark.other == true))
        if asksWhy { writing = true } else if !isMany { writing = false }
    }

    /// **Two stars** make a few words stand out; a link stands by its words.
    private static func marked(_ fact: String) -> AttributedString {
        var whole = AttributedString()
        for (number, part) in fact.components(separatedBy: "**").enumerated() {
            var piece = Linked.text(part)
            if number % 2 == 1 {
                piece.font = .system(size: Face.details, weight: .semibold)
                for run in piece.runs where run.link == nil { piece[run.range].foregroundColor = Ink.text }
            }
            whole += piece
        }
        return whole
    }
}

extension Linked {
    /// A text with its links, each over its words: in the blue of a link, with a line under it.
    static func text(_ said: String) -> AttributedString {
        var whole = AttributedString()
        for piece in pieces(said) {
            var part = AttributedString(piece.text)
            if let link = piece.link, let address = address(link) {
                part.link = address
                part.foregroundColor = Ink.linked
                part.underlineStyle = .single
            }
            whole += part
        }
        return whole
    }
}

private struct AnswerRow: View {
    var title: String
    /// A box, where several of the options may hold at once: ticked or not.
    var box: Bool?
    var chosen: Color?
    var dim: Bool
    var press: () -> Void

    var body: some View {
        Button(action: press) {
            HStack(spacing: 12) {
                if let box {
                    RoundedRectangle(cornerRadius: 7, style: .continuous).fill(box ? (chosen ?? Ink.text) : .clear)
                        .overlay(RoundedRectangle(cornerRadius: 7, style: .continuous).stroke(box ? .clear : Ink.dim, lineWidth: 1.5))
                        .overlay(Image(systemName: "checkmark").font(.system(size: 12, weight: .heavy)).foregroundStyle(Ink.bar).opacity(box ? 1 : 0))
                        .frame(width: 22, height: 22)
                }
                Text(title).font(.system(size: Face.answer, weight: .medium))
                    .foregroundStyle(chosen != nil ? Ink.text : dim ? Ink.soft : Ink.text)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: 8)
                if box == nil, let chosen {
                    Image(systemName: "checkmark").font(.system(size: 15, weight: .bold)).foregroundStyle(chosen)
                }
            }
            .padding(.horizontal, 16).padding(.vertical, 6).frame(minHeight: 50)
            .background(chosen.map { $0.opacity(0.2) } ?? .clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(Pressing())
    }
}

private extension View {
    /// What scrolls on a page is as wide as the page, whatever stands in it: a thing wider than its line runs out
    /// at the right by itself and does not take the page along. Without this a scroll view is as wide as the
    /// widest thing in it, and the screen's stack as wide as the scroll view.
    func pageWide() -> some View {
        frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
    }
}

private struct Pressing: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.opacity(configuration.isPressed ? 0.55 : 1)
    }
}

/// A line he copies with a tap: a command for the terminal, an address, a value to paste («нужна фича copyable
/// строчки, я бы щас раз и скопировал по клику нужную команду», and to leaving the phone for later: «нет»). It
/// stands in the font of code on a card of its own, three lines of it at most, and is copied whole; for a moment
/// the card says that it is.
private struct CopyLine: View {
    var line: String
    var copied: Bool
    var press: () -> Void

    var body: some View {
        Button(action: press) {
            HStack(alignment: .center, spacing: 10) {
                Text(line).font(.system(size: 14.5, design: .monospaced)).foregroundStyle(Ink.text)
                    .lineLimit(3).truncationMode(.tail).multilineTextAlignment(.leading)
                Spacer(minLength: 4)
                Image(systemName: "doc.on.doc").font(.system(size: 15, weight: .medium)).foregroundStyle(Ink.soft).opacity(copied ? 0 : 1)
            }
            .padding(.horizontal, 14).padding(.vertical, 12).frame(minHeight: 48)
            // The word stands over the line's end, so that nothing moves when it comes and goes.
            .overlay(alignment: .trailing) {
                if copied {
                    HStack(spacing: 6) {
                        Text("Скопировано").font(.system(size: 14, weight: .medium))
                        Image(systemName: "checkmark").font(.system(size: 14, weight: .bold))
                    }
                    .foregroundStyle(Ink.done)
                    .padding(.leading, 12).padding(.trailing, 14).frame(maxHeight: .infinity)
                    .background(Ink.bar)
                }
            }
            .background(Ink.bar)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).stroke(copied ? Ink.done.opacity(0.55) : Ink.barEdge))
            .contentShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
        .buttonStyle(Pressing())
        .accessibilityLabel("Скопировать: " + line)
        .accessibilityIdentifier("queue.copy")
    }
}

/// A picture of an item, as it comes from the Mac; a tap shows it large.
private struct PictureView: View {
    let model: QueueModel
    let path: String
    var tallest: CGFloat

    var body: some View {
        Group {
            if let image = model.picture(path) {
                Image(uiImage: image).resizable().scaledToFit().frame(maxWidth: .infinity, maxHeight: tallest)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(Ink.rule))
                    .onTapGesture { model.show(file: path) }
            } else {
                RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Ink.row).frame(height: min(tallest, 120))
                    .overlay {
                        if model.failed[path] != nil {
                            Text("Картинка не пришла с Мака").font(.system(size: 13)).foregroundStyle(Ink.faint)
                        } else {
                            ProgressView().tint(Ink.faint)
                        }
                    }
            }
        }
        .accessibilityIdentifier("queue.picture")
    }
}

/// What an item offers to open, as a row of small buttons that wraps.
private struct Chips: View {
    var opens: [Open]
    var press: (Open) -> Void

    var body: some View {
        Wrap(spacing: 8) {
            ForEach(opens, id: \.self) { target in
                Button { press(target) } label: {
                    HStack(spacing: 5) {
                        Text(target.title).lineLimit(1)
                        Image(systemName: target.kind == "file" ? "doc.text" : "arrow.up.right").font(.system(size: 11, weight: .semibold))
                    }
                    .font(.system(size: 14)).foregroundStyle(Ink.link)
                    .padding(.horizontal, 14).frame(height: 36)
                    .overlay(Capsule().stroke(Ink.rowEdge))
                    .contentShape(Capsule())
                }
            }
        }
    }
}

/// Lays its views out in lines, a new line when one is full. No view gets more than a line's width: one that is
/// wider is cut short where it stands. A file with a long name, on a button as wide as that name, made the item
/// wider than the screen, and the whole check with it: «разъебало вёрстку на телефоне».
private struct Wrap: Layout {
    var spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        place(subviews, in: proposal.width ?? .infinity).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (subview, frame) in zip(subviews, place(subviews, in: bounds.width).frames) {
            subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY), proposal: ProposedViewSize(frame.size))
        }
    }

    private func place(_ subviews: Subviews, in width: CGFloat) -> (size: CGSize, frames: [CGRect]) {
        var frames: [CGRect] = []
        var x: CGFloat = 0, y: CGFloat = 0, tall: CGFloat = 0, wide: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(ProposedViewSize(width: width.isFinite ? width : nil, height: nil))
            if x > 0, x + size.width > width {
                x = 0
                y += tall + spacing
                tall = 0
            }
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
            x += size.width + spacing
            tall = max(tall, size.height)
            wide = max(wide, x - spacing)
        }
        return (CGSize(width: wide, height: y + tall), frames)
    }
}

/// Every check that waits, in the window's order, and the last ones ended. A tap takes a check first. It is the
/// page «‹ Очередь» of a check goes to; a row of it, or «К проверке», comes back.
struct QueueList: View {
    let model: QueueModel
    let queue: QueueWord
    var note: String?
    var close: (() -> Void)?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                HStack(alignment: .firstTextBaseline) {
                    Text("Очередь").font(.system(size: 30, weight: .bold)).foregroundStyle(Ink.text)
                    Spacer()
                    if let close {
                        Button(action: close) {
                            HStack(spacing: 4) {
                                Text("К проверке").font(.system(size: 17))
                                Image(systemName: "chevron.right").font(.system(size: 15, weight: .semibold))
                            }
                            .foregroundStyle(Ink.way).frame(minHeight: 40).contentShape(Rectangle())
                        }
                        .buttonStyle(Pressing())
                        .accessibilityIdentifier("queue.back")
                    }
                }
                HStack(spacing: 7) {
                    Circle().fill(queue.window == true ? Ink.hex(0x34C759) : Ink.hex(0xFF9F0A)).frame(width: 8, height: 8)
                    Text(note ?? "\(model.macName.isEmpty ? "Мак" : model.macName) на связи\(model.isFar ? " через сервер" : "") · ждут: \((queue.checks ?? []).count)")
                        .font(.system(size: 13.5)).foregroundStyle(Ink.quiet)
                }
                .padding(.top, 6).padding(.bottom, 14)
                Rectangle().fill(Ink.rule).frame(height: 1)
                ForEach(queue.checks ?? []) { check in
                    Button {
                        model.press("take", ticket: check.id)
                        close?()
                    } label: {
                        row(check)
                    }
                    .buttonStyle(Pressing())
                    .accessibilityIdentifier("queue.check")
                    Rectangle().fill(Ink.barEdge).frame(height: 1)
                }
                if let recent = queue.recent, !recent.isEmpty {
                    Text("НЕДАВНИЕ").font(.system(size: 12, weight: .semibold)).kerning(0.5).foregroundStyle(Ink.faint).padding(.top, 20).padding(.bottom, 4)
                    ForEach(recent, id: \.self) { one in
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            Text([one.tag ?? "", "«\(one.title ?? "")»"].filter { !$0.isEmpty }.joined(separator: " ")).lineLimit(1)
                                .foregroundStyle(Ink.dim)
                            Spacer(minLength: 8)
                            Text(one.summary ?? "").foregroundStyle(Ink.faint).lineLimit(1)
                        }
                        .font(.system(size: 14)).padding(.vertical, 8)
                    }
                }
            }
            .padding(.horizontal, 18).padding(.top, 10).padding(.bottom, 24)
            .pageWide()
        }
        .background(Ink.panel)
        .accessibilityIdentifier("queue.list")
    }

    private func row(_ check: Check) -> some View {
        let isActive = check.id == queue.active
        let count = check.items.count
        let marked = check.marked ?? 0
        return VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                Circle().fill(Ink.tint(of: check.kind)).frame(width: 10, height: 10)
                AppMark(model: model, check: check)
                ProjectTag(name: check.projectName)
                Text(check.who).font(.system(size: 15, weight: .semibold)).foregroundStyle(Ink.text).lineLimit(1)
                Spacer(minLength: 8)
                Text(isActive ? "сейчас · \(marked) из \(count)" : marked > 0 ? "отмечено \(marked) из \(count)" : Self.since(check.createdAt))
                    .font(.system(size: 13)).foregroundStyle(Ink.faint)
            }
            Text(check.title ?? "").font(.system(size: 16.5)).foregroundStyle(Ink.text).multilineTextAlignment(.leading)
            Text([check.onPhone == true ? "сборка на телефоне" : check.kindTitle ?? "", Self.items(count, plan: check.kind == "plan")]
                .filter { !$0.isEmpty }.joined(separator: " · "))
                .font(.system(size: 13)).foregroundStyle(Ink.dim)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 14)
        .padding(.horizontal, 18)
        .background(isActive ? Ink.hex(0x202127) : .clear)
        .padding(.horizontal, -18)
        .contentShape(Rectangle())
    }

    private static func since(_ moment: Double?) -> String {
        guard let moment, moment > 0 else { return "" }
        return "с " + Date(timeIntervalSince1970: moment).formatted(date: .omitted, time: .shortened)
    }

    private static func items(_ count: Int, plan: Bool) -> String {
        let tail = count % 100, last = count % 10
        let form = (11...14).contains(tail) ? 2 : last == 1 ? 0 : (2...4).contains(last) ? 1 : 2
        return "\(count) " + (plan ? ["шаг", "шага", "шагов"] : ["пункт", "пункта", "пунктов"])[form]
    }
}
