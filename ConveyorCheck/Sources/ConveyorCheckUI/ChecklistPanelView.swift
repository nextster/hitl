#if canImport(UIKit)
import ConveyorCheck
import UIKit

/// A checklist that stands at the top of the screen while the app is checked
/// by hand: a black band where the status bar was, around the Dynamic Island
/// on a phone that has one. Folded, it takes nothing from the app: beside the
/// island are the way between the items and the three marks of the open one,
/// over it whose checklist it is and what the item says, under it how far the
/// check has come. Unfolded, the band grows down with the item's text, and
/// then with its comment and the buttons that end the checklist or show all
/// of its items.
///
/// It shows whatever a `ChecklistSource` gives it: the hand check a host
/// installs a build for, or a checklist of the app's own. What differs between
/// them is in `Content`; the view knows neither.
public final class ChecklistPanelView: UIView, UITextFieldDelegate {
    enum Event {
        case mark(String, CheckMark)
        case note(String, String)
        /// To the item before or after the current one.
        case step(Int)
        /// One of the current item's actions, by its id.
        case action(String)
        /// Between one item at a time and all of them.
        case toggleView
        /// The button under the items.
        case footer
        case editingEnded, close
    }

    /// Something the app can do for the item on screen: open the screen it is
    /// about, fold the dock, follow a link.
    public struct Action: Equatable {
        /// One of the things a choice chooses between.
        public struct Option: Equatable {
            /// What is run to choose it.
            public let id: String
            public let title: String
            /// An SF Symbol in the title's place.
            public var symbol: String?

            public init(id: String, title: String, symbol: String? = nil) {
                self.id = id
                self.title = title
                self.symbol = symbol
            }
        }

        /// What kind of button it is.
        public enum Look: Equatable {
            /// A few words.
            case words
            /// It opens a screen, and the title is the screen's address.
            case link
            /// It does something once: an SF Symbol, and the title is read out.
            case icon(String)
            /// A switch of what the symbol or the caption says, and how it
            /// stands; `id` is what flips it.
            case toggle(symbol: String?, caption: String?, isOn: Bool)
            /// One of several, and the one that holds. A tap on an option
            /// runs that option; `id` is the one after the current.
            case choice([Option], current: String?)
        }

        public let id: String
        public let title: String
        /// The one that prepares the screen stands out.
        public var isPrimary = false
        public var look = Look.words

        public init(id: String, title: String, isPrimary: Bool = false, look: Look = .words) {
            self.id = id
            self.title = title
            self.isPrimary = isPrimary
            self.look = look
        }

        /// The same button whichever way it stands: a switch flipped is not
        /// another button, and is not built anew.
        var standing: Action {
            switch look {
            case let .toggle(symbol, caption, _):
                Action(id: "", title: title, isPrimary: isPrimary, look: .toggle(symbol: symbol, caption: caption, isOn: false))
            case let .choice(options, _): Action(id: "", title: title, isPrimary: isPrimary, look: .choice(options, current: nil))
            case .words, .link, .icon: self
            }
        }
    }

    public struct Item: Equatable, Identifiable {
        public let id: String
        /// What stands before the text: "3." or "4a.".
        public let number: String
        public let text: String
        /// Steps and what to expect; shown with the item when it is alone on screen.
        public var detail: NSAttributedString?
        public var actions: [Action] = []
        public var state: CheckMark?
        public var note: String?

        public init(id: String, number: String, text: String, detail: NSAttributedString? = nil, actions: [Action] = [],
                    state: CheckMark? = nil, note: String? = nil) {
            self.id = id
            self.number = number
            self.text = text
            self.detail = detail
            self.actions = actions
            self.state = state
            self.note = note
        }
    }

    /// The button under the items.
    public enum Footer: Equatable {
        /// Ends the checklist before every item has a mark: it goes back with what there is.
        case sendBack
        /// Ends the checklist with every item marked.
        case done
        /// Opens the whole plan, with its report.
        case plan

        var title: String {
            switch self {
            case .sendBack: "Доработать"
            case .done: "Готово"
            case .plan: "План"
            }
        }
    }

    struct Content: Equatable {
        /// Whose checklist it is: the chat's tag, or the plan's.
        let tag: String
        let title: String
        let items: [Item]
        /// The item the one-at-a-time view is on.
        let currentID: String?
        let showsOneItem: Bool
        let footer: Footer?
        /// A checklist that can be put away without ending it.
        var canClose = false
        /// Why an action could not run.
        var message: String?
        /// An action is running: the others wait.
        var isBusy = false
        /// The project the checklist is about, first in the band's line; "" when there is none to say.
        var project = ""
    }

    /// The widest the band gets; a wider screen has it in the middle of its top edge.
    static let widestBand: CGFloat = 440
    /// The corners the band gives the app under it, and its own on a wide screen.
    private static let corner: CGFloat = 22
    /// The Dynamic Island, and what is left under it down to the safe area.
    private static let island = CGSize(width: 126, height: 37)
    private static let underIsland: CGFloat = 11
    /// From the edge of the screen to the first button: clear of its round corner.
    private static let side: CGFloat = 18

    var onEvent: (Event) -> Void = { _ in }
    private(set) var isCollapsed = false
    /// The comment of the open item and the buttons under it are in sight.
    private(set) var showsControls = false
    /// The device's top inset when the window is not there to say it: a test's.
    var deviceTopInset: CGFloat? { didSet { setNeedsLayout() } }

    private let style: ChecklistPanelStyle
    private let shape = CAShapeLayer()
    private let stack = UIStackView()
    private let top = UIView()
    private let context = UIStackView()
    private let tagLabel = UILabel()
    private let projectLabel = UILabel()
    private let titleLabel = UILabel()
    private let leftEar = UIStackView()
    private let previousButton = UIButton(type: .system)
    private let nextButton = UIButton(type: .system)
    private let countLabel = UILabel()
    private let collapseButton = UIButton(type: .system)
    private let marks = ChecklistMarksView(buttonWidth: nil)
    private let progress = ChecklistProgressView()
    private let details = UIStackView()
    private let scroll = UIScrollView()
    private let list = UIStackView()
    private let messageLabel = UILabel()
    private let actions = ChecklistFlowView()
    private let controls = UIStackView()
    private let viewButton = UIButton(type: .system)
    private let closeButton = UIButton(type: .system)
    private let footerButton = UIButton(type: .system)
    private var rows: [ChecklistRowView] = []
    private var content: Content?
    /// What is drawn now, so that typing a comment restyles nothing.
    private var shownFooter: Footer?
    private var shownActions: [Action] = []
    /// The app is acting for the item; the buttons dim once that has lasted.
    private var actionsAreBusy = false
    private var dimming: DispatchWorkItem?
    private var scrollHeight: NSLayoutConstraint!
    private var topHeight: NSLayoutConstraint!
    private var rowTop: NSLayoutConstraint!
    private var contextTop: NSLayoutConstraint!
    private var contextHeight: NSLayoutConstraint!

    init(style: ChecklistPanelStyle) {
        self.style = style
        super.init(frame: .zero)
        accessibilityIdentifier = "checklist.panel"
        // Black whatever the app looks like: one piece with the island.
        overrideUserInterfaceStyle = .dark
        insetsLayoutMarginsFromSafeArea = false
        shape.fillColor = ChecklistPanelStyle.surface.cgColor
        layer.addSublayer(shape)
        stack.axis = .vertical
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: topAnchor), stack.bottomAnchor.constraint(equalTo: bottomAnchor),
            stack.leadingAnchor.constraint(equalTo: leadingAnchor), stack.trailingAnchor.constraint(equalTo: trailingAnchor),
        ])
        buildTop()
        buildDetails()
        stack.addArrangedSubview(top)
        stack.addArrangedSubview(details)
    }

    @available(*, unavailable)
    public required init?(coder: NSCoder) { fatalError("Use init(style:)") }

    func setCollapsed(_ collapsed: Bool) {
        guard collapsed != isCollapsed else { return }
        isCollapsed = collapsed
        if collapsed { endEditing(true) }
        show()
    }

    func render(_ next: Content) {
        guard next != content else { return }
        let previous = content
        if next.items.map({ [$0.id, $0.number, $0.text] }) != previous?.items.map({ [$0.id, $0.number, $0.text] }) {
            rebuildRows(next.items)
        }
        content = next
        // Leaving an item leaves its comment and its buttons too.
        if next.showsOneItem, previous?.currentID != next.currentID {
            endEditing(true)
            showsControls = false
        }
        // With the last mark made, the button that ends the checklist comes into sight by itself.
        if next.footer == .done, let previous, previous.footer != .done {
            isCollapsed = false
            showsControls = true
        }
        if previous?.showsOneItem != next.showsOneItem || previous?.currentID != next.currentID {
            scroll.setContentOffset(.zero, animated: false)
        }
        show()
    }

    public override func layoutSubviews() {
        placeTop()
        super.layoutSubviews()
        // The items take what they need up to two fifths of the screen, then scroll.
        let wanted = list.systemLayoutSizeFitting(CGSize(width: scroll.bounds.width, height: UIView.layoutFittingCompressedSize.height),
                                                  withHorizontalFittingPriority: .required, verticalFittingPriority: .fittingSizeLevel).height
        let limit = (window?.bounds.height ?? 800) * 0.4
        if abs(scrollHeight.constant - min(wanted, limit)) > 0.5 { scrollHeight.constant = min(wanted, limit) }
        drawShape()
    }

    public override func safeAreaInsetsDidChange() {
        super.safeAreaInsetsDidChange()
        setNeedsLayout()
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        setNeedsLayout()
    }

    public func textFieldShouldReturn(_ textField: UITextField) -> Bool {
        textField.resignFirstResponder()
        return true
    }

    public func textFieldDidEndEditing(_ textField: UITextField) {
        rows.first { $0.note === textField }?.commentEditingEnded()
        onEvent(.editingEnded)
    }

    // MARK: What is in sight

    /// Draws the checklist as it is folded now.
    private func show() {
        guard let content else { return }
        let index = content.items.firstIndex { $0.id == content.currentID }
        let current = index.map { content.items[$0] }
        tagLabel.text = content.tag
        projectLabel.text = content.project
        projectLabel.isHidden = content.project.isEmpty
        renderTitle()
        // One item at a time, the number is that of the open item: «на телефоне показывает 2/6 а в реальности
        // везде открыто 3/6». With all of them in sight it is how many have a mark, and says so.
        if content.showsOneItem, let index {
            renderCount("\(index + 1)", of: content.items.count)
        } else {
            renderCount("✓\(content.items.count { $0.state != nil })", of: content.items.count)
        }
        collapseButton.accessibilityLabel = isCollapsed ? "Развернуть чек-лист" : "Свернуть чек-лист"

        details.isHidden = isCollapsed
        // With all the items in sight the buttons have nowhere else to be.
        controls.isHidden = content.showsOneItem && !showsControls
        closeButton.isHidden = !content.canClose
        for (row, item) in zip(rows, content.items) {
            let isOpen = content.showsOneItem && item.id == content.currentID
            row.isHidden = content.showsOneItem && !isOpen
            row.render(state: item.state, note: item.note, detail: content.showsOneItem ? item.detail : nil,
                       showsMarks: !content.showsOneItem, keepsCommentOpen: isOpen && showsControls)
        }

        // The open item's marks stand beside the island, in reach while the band is folded.
        marks.isHidden = !content.showsOneItem || current == nil
        marks.setItem(marks.isHidden ? nil : current?.id)
        marks.render(current?.state)
        for (button, offset) in [(previousButton, -1), (nextButton, 1)] {
            button.isHidden = !content.showsOneItem
            button.isEnabled = index.map { content.items.indices.contains($0 + offset) } ?? false
        }
        progress.show(marks: content.items.map(\.state), current: index)

        viewButton.configuration?.image = UIImage(systemName: content.showsOneItem ? "list.bullet" : "1.square")
        viewButton.accessibilityLabel = content.showsOneItem ? "Показать все пункты" : "Показывать по одному пункту"
        footerButton.isHidden = content.footer == nil
        renderFooter(content.footer)

        // What the app can do for an item goes with the item when it is alone on screen.
        let offered = content.showsOneItem ? current?.actions ?? [] : []
        renderActions(offered, enabled: !content.isBusy)
        messageLabel.text = content.message
        messageLabel.isHidden = content.message == nil
        setNeedsLayout()
    }

    /// Unfolded, the line over the island names the checklist; folded, the
    /// item it is on, so the band still says what to look at.
    private func renderTitle() {
        guard let content else { return }
        let current = content.items.first { $0.id == content.currentID }
        let folded = current.map { "\($0.number) \($0.text)" }
        titleLabel.text = isCollapsed ? folded ?? content.title : content.title
        titleLabel.textColor = isCollapsed ? ChecklistPanelStyle.text : ChecklistPanelStyle.secondaryText
        titleLabel.isHidden = (titleLabel.text ?? "").isEmpty
    }

    private func renderCount(_ number: String, of count: Int) {
        let text = NSMutableAttributedString(string: number, attributes: [.foregroundColor: ChecklistPanelStyle.text])
        text.append(NSAttributedString(string: "/\(count)", attributes: [.foregroundColor: ChecklistPanelStyle.secondaryText]))
        countLabel.attributedText = text
    }

    /// The tester marked the open item beside the island. A problem wants its
    /// comment, so the band opens down to it, folded or not.
    private func markCurrent(_ mark: CheckMark) {
        guard let content, let current = content.items.first(where: { $0.id == content.currentID }) else { return }
        if mark == .problem, current.state != .problem {
            isCollapsed = false
            showsControls = true
            show()
        }
        onEvent(.mark(current.id, mark))
    }

    /// A tap on an item's words. Alone on screen, the item shows or puts away
    /// its comment and the buttons under it; in the list, its comment.
    private func wordsTapped(_ row: ChecklistRowView) {
        guard content?.showsOneItem == true else { return row.toggleComment() }
        showsControls.toggle()
        if !showsControls { endEditing(true) }
        show()
    }

    // MARK: Where things stand

    /// The top of the band's row of buttons. On a phone with a Dynamic Island
    /// it is the island's own row; under a notch the row stands below it;
    /// anywhere else it leaves room for the line over it.
    private var rowY: CGFloat {
        let phone = traitCollection.userInterfaceIdiom == .phone ? window?.safeAreaInsets.top : nil
        guard let inset = deviceTopInset ?? phone, inset > 30 else { return 14 }
        // A notch leaves fifty points at most; an island leaves fifty-nine or more.
        return inset > 51 ? inset - Self.island.height - Self.underIsland : inset
    }

    private func placeTop() {
        let y = rowY
        guard abs(rowTop.constant - y) > 0.1 else { return }
        rowTop.constant = y
        contextTop.constant = max(y - 13, 0)
        // Where the island is eleven points from the top the line over it has less than its twelve.
        contextHeight.constant = min(y - contextTop.constant, 12)
        let size: CGFloat = y < 13 ? 9 : 10
        projectLabel.font = style.font(size, weight: .bold)
        tagLabel.font = style.font(size, weight: .semibold)
        titleLabel.font = style.font(size, weight: .medium)
        topHeight.constant = y + Self.island.height + Self.underIsland
    }

    /// The band covers the screen from edge to edge on a phone, and there the
    /// app under it gets round corners of its own; on a wider screen the band
    /// hangs from the middle of the top edge with its own corners rounded.
    private func drawShape() {
        let size = bounds.size
        let radius = Self.corner
        let path: UIBezierPath
        if let superview, size.width < superview.bounds.width - 0.5 {
            path = UIBezierPath(roundedRect: bounds, byRoundingCorners: [.bottomLeft, .bottomRight],
                                cornerRadii: CGSize(width: radius, height: radius))
        } else {
            path = UIBezierPath()
            path.move(to: .zero)
            path.addLine(to: CGPoint(x: size.width, y: 0))
            path.addLine(to: CGPoint(x: size.width, y: size.height + radius))
            path.addArc(withCenter: CGPoint(x: size.width - radius, y: size.height + radius), radius: radius,
                        startAngle: 0, endAngle: -.pi / 2, clockwise: false)
            path.addLine(to: CGPoint(x: radius, y: size.height))
            path.addArc(withCenter: CGPoint(x: radius, y: size.height + radius), radius: radius,
                        startAngle: -.pi / 2, endAngle: .pi, clockwise: false)
            path.close()
        }
        // The band changes its height at once, as its content does.
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        shape.path = path.cgPath
        CATransaction.commit()
    }

    // MARK: Parts

    private func buildTop() {
        // The colour an app shows the launch's tag in: the same name.
        tagLabel.font = style.font(10, weight: .semibold)
        tagLabel.textColor = .systemOrange
        tagLabel.setContentHuggingPriority(.required, for: .horizontal)
        tagLabel.setContentCompressionResistancePriority(.required, for: .horizontal)
        tagLabel.accessibilityIdentifier = "checklist.tag"
        // Which project the check is about, before whose it is: the tester holds several at once.
        projectLabel.font = style.font(10, weight: .bold)
        projectLabel.textColor = ChecklistPanelStyle.text
        projectLabel.setContentHuggingPriority(.required, for: .horizontal)
        projectLabel.setContentCompressionResistancePriority(.required, for: .horizontal)
        projectLabel.accessibilityIdentifier = "checklist.project"
        projectLabel.isHidden = true
        titleLabel.font = style.font(10, weight: .medium)
        titleLabel.lineBreakMode = .byTruncatingTail
        titleLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        titleLabel.accessibilityIdentifier = "checklist.title"
        context.axis = .horizontal
        context.spacing = 5
        context.alignment = .center
        context.addArrangedSubview(projectLabel)
        context.addArrangedSubview(tagLabel)
        context.addArrangedSubview(titleLabel)
        // The whole line unfolds the band, not only the number beside the island.
        context.addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(expand)))

        round(previousButton, symbol: "chevron.left", label: "Предыдущий пункт", id: "checklist.previous") { [weak self] in
            self?.onEvent(.step(-1))
        }
        round(nextButton, symbol: "chevron.right", label: "Следующий пункт", id: "checklist.next") { [weak self] in
            self?.onEvent(.step(1))
        }
        countLabel.font = .monospacedDigitSystemFont(ofSize: 15, weight: .semibold)
        countLabel.textAlignment = .center
        countLabel.adjustsFontSizeToFitWidth = true
        countLabel.minimumScaleFactor = 0.7
        countLabel.accessibilityIdentifier = "checklist.count"
        // The number folds the band and unfolds it: the largest thing beside the island.
        collapseButton.accessibilityLabel = "Свернуть чек-лист"
        collapseButton.accessibilityIdentifier = "checklist.collapse"
        collapseButton.addAction(UIAction { [weak self] _ in self.map { $0.setCollapsed(!$0.isCollapsed) } }, for: .primaryActionTriggered)
        let count = UIView()
        for view in [countLabel, collapseButton] {
            view.translatesAutoresizingMaskIntoConstraints = false
            count.addSubview(view)
            NSLayoutConstraint.activate([
                view.topAnchor.constraint(equalTo: count.topAnchor), view.bottomAnchor.constraint(equalTo: count.bottomAnchor),
                view.leadingAnchor.constraint(equalTo: count.leadingAnchor), view.trailingAnchor.constraint(equalTo: count.trailingAnchor),
            ])
        }
        leftEar.axis = .horizontal
        leftEar.spacing = 4
        leftEar.alignment = .center
        leftEar.addArrangedSubview(previousButton)
        leftEar.addArrangedSubview(count)
        leftEar.addArrangedSubview(nextButton)
        count.heightAnchor.constraint(equalTo: leftEar.heightAnchor).isActive = true

        marks.onMark = { [weak self] in self?.markCurrent($0) }

        for view in [context, leftEar, marks, progress] as [UIView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            top.addSubview(view)
        }
        let half = Self.island.width / 2 + 6
        rowTop = leftEar.topAnchor.constraint(equalTo: top.topAnchor, constant: 14)
        contextTop = context.topAnchor.constraint(equalTo: top.topAnchor, constant: 1)
        contextHeight = context.heightAnchor.constraint(equalToConstant: 12)
        topHeight = top.heightAnchor.constraint(equalToConstant: 14 + Self.island.height + Self.underIsland)
        NSLayoutConstraint.activate([
            topHeight, rowTop, contextTop, contextHeight,
            context.centerXAnchor.constraint(equalTo: top.centerXAnchor),
            // Clear of the screen's round corners at this height.
            context.widthAnchor.constraint(lessThanOrEqualTo: top.widthAnchor, constant: -112),
            leftEar.heightAnchor.constraint(equalToConstant: Self.island.height),
            leftEar.leadingAnchor.constraint(equalTo: top.leadingAnchor, constant: Self.side),
            leftEar.trailingAnchor.constraint(equalTo: top.centerXAnchor, constant: -half),
            marks.centerYAnchor.constraint(equalTo: leftEar.centerYAnchor),
            marks.leadingAnchor.constraint(equalTo: top.centerXAnchor, constant: half),
            marks.trailingAnchor.constraint(equalTo: top.trailingAnchor, constant: -Self.side),
            progress.topAnchor.constraint(equalTo: leftEar.bottomAnchor, constant: 2.5),
            progress.centerXAnchor.constraint(equalTo: top.centerXAnchor),
            progress.widthAnchor.constraint(equalToConstant: Self.island.width),
            progress.heightAnchor.constraint(equalToConstant: 6),
        ])
    }

    private func buildDetails() {
        details.axis = .vertical
        details.spacing = 8
        details.isLayoutMarginsRelativeArrangement = true
        details.insetsLayoutMarginsFromSafeArea = false
        details.directionalLayoutMargins = .init(top: 2, leading: Self.side, bottom: 12, trailing: Self.side)
        list.axis = .vertical
        list.spacing = 10
        list.translatesAutoresizingMaskIntoConstraints = false
        scroll.addSubview(list)
        scroll.showsVerticalScrollIndicator = true
        scroll.keyboardDismissMode = .interactive
        scroll.contentInsetAdjustmentBehavior = .never
        scrollHeight = scroll.heightAnchor.constraint(equalToConstant: 80)
        scrollHeight.priority = .init(999)
        NSLayoutConstraint.activate([
            list.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor),
            list.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor),
            list.leadingAnchor.constraint(equalTo: scroll.contentLayoutGuide.leadingAnchor),
            list.trailingAnchor.constraint(equalTo: scroll.contentLayoutGuide.trailingAnchor),
            list.widthAnchor.constraint(equalTo: scroll.frameLayoutGuide.widthAnchor),
            scrollHeight,
        ])
        details.addArrangedSubview(scroll)

        messageLabel.font = style.font(13, weight: .medium)
        messageLabel.textColor = .systemOrange
        messageLabel.numberOfLines = 0
        messageLabel.accessibilityIdentifier = "checklist.message"
        messageLabel.isHidden = true
        details.addArrangedSubview(messageLabel)

        actions.isHidden = true
        details.addArrangedSubview(actions)

        // One line under the item: the way to all the items, the way out of a
        // checklist that can be put away, and the button that ends it or opens the plan.
        controls.axis = .horizontal
        controls.spacing = 8
        controls.alignment = .center
        round(viewButton, symbol: "list.bullet", label: "Показать все пункты", id: "checklist.view", size: 34) { [weak self] in
            self?.onEvent(.toggleView)
        }
        round(closeButton, symbol: "xmark", label: "Закрыть чек-лист", id: "checklist.close", size: 34) { [weak self] in
            self?.onEvent(.close)
        }
        closeButton.isHidden = true
        footerButton.accessibilityIdentifier = "checklist.footer"
        footerButton.addAction(UIAction { [weak self] _ in self?.onEvent(.footer) }, for: .primaryActionTriggered)
        let gap = UIView()
        gap.setContentHuggingPriority(.init(1), for: .horizontal)
        controls.addArrangedSubview(viewButton)
        controls.addArrangedSubview(closeButton)
        controls.addArrangedSubview(gap)
        controls.addArrangedSubview(footerButton)
        details.addArrangedSubview(controls)
    }

    /// «Готово» stands out once every item has a mark. Until then the button
    /// sends the checklist back with what is marked so far, and looks the part.
    private func renderFooter(_ next: Footer?) {
        guard next != shownFooter, let next else { shownFooter = next; return }
        shownFooter = next
        var configuration: UIButton.Configuration = next == .done ? .filled() : .plain()
        configuration.cornerStyle = .capsule
        configuration.contentInsets = .init(top: 8, leading: 16, bottom: 8, trailing: 16)
        if next == .done {
            configuration.baseBackgroundColor = style.accent
        } else {
            configuration.background.backgroundColor = ChecklistPanelStyle.control
            configuration.baseForegroundColor = ChecklistPanelStyle.text
        }
        var title = AttributedString(next.title)
        title.font = style.font(15, weight: .semibold)
        configuration.attributedTitle = title
        footerButton.configuration = configuration
        footerButton.accessibilityHint = switch next {
        case .done: "Все пункты отмечены"
        case .sendBack: "Вернуть чату то, что отмечено"
        case .plan: "Весь план и отчёт"
        }
    }

    private func renderActions(_ next: [Action], enabled: Bool) {
        if next != shownActions {
            // A switch flipped or another option chosen moves in the button that is there.
            if next.map(\.standing) == shownActions.map(\.standing), actions.subviews.count == next.count {
                for (view, action) in zip(actions.subviews, next) { restate(view, as: action) }
            } else {
                actions.subviews.forEach { $0.removeFromSuperview() }
                next.forEach { actions.addSubview(makeControl($0)) }
                actions.isHidden = next.isEmpty
                actions.setNeedsLayout()
            }
            shownActions = next
        }
        actions.isUserInteractionEnabled = enabled
        dimActions(!enabled)
    }

    /// One button of what the app can do for the item, by its kind.
    private func makeControl(_ action: Action) -> UIView {
        switch action.look {
        case let .toggle(symbol, caption, isOn):
            let toggle = ChecklistSwitchButton(symbol: symbol, caption: caption, name: action.title, isOn: isOn, style: style)
            toggle.action = action.id
            toggle.accessibilityIdentifier = "checklist.action.\(action.id)"
            toggle.addAction(UIAction { [weak self, unowned toggle] _ in self?.onEvent(.action(toggle.action)) }, for: .primaryActionTriggered)
            return toggle
        case let .choice(options, current):
            let choice = ChecklistChoiceView(name: action.title, options: options, current: current, style: style)
            choice.onPick = { [weak self] in self?.onEvent(.action($0)) }
            return choice
        case .words, .link, .icon:
            break
        }
        let button = UIButton(type: .system)
        var configuration: UIButton.Configuration = action.isPrimary ? .filled() : .plain()
        if case let .icon(symbol) = action.look, let image = UIImage(systemName: symbol) {
            configuration.image = image
            configuration.preferredSymbolConfigurationForImage = .init(pointSize: 13, weight: .semibold)
            configuration.contentInsets = .init(top: 4, leading: 11, bottom: 4, trailing: 11)
            button.accessibilityLabel = action.title
        } else {
            // An address and an action's own id are in the font of code; a name in words is in the app's.
            let isCode = action.look == .link || !action.isPrimary
            configuration.attributedTitle = AttributedString(action.title, attributes: .init([
                .font: isCode ? UIFont.monospacedSystemFont(ofSize: 12, weight: .medium) : style.font(13, weight: .semibold),
            ]))
            configuration.titleLineBreakMode = action.look == .link ? .byTruncatingHead : .byTruncatingTail
            configuration.contentInsets = .init(top: 4, leading: 12, bottom: 4, trailing: 12)
        }
        configuration.cornerStyle = .capsule
        if action.isPrimary {
            configuration.baseBackgroundColor = style.accent
        } else {
            configuration.background.backgroundColor = ChecklistPanelStyle.control
            configuration.baseForegroundColor = ChecklistPanelStyle.text
        }
        button.configuration = configuration
        button.accessibilityIdentifier = "checklist.action.\(action.id)"
        button.addAction(UIAction { [weak self] _ in self?.onEvent(.action(action.id)) }, for: .primaryActionTriggered)
        return button
    }

    /// The button is the same and stands another way.
    private func restate(_ view: UIView, as action: Action) {
        switch action.look {
        case let .toggle(_, _, isOn):
            guard let toggle = view as? ChecklistSwitchButton else { return }
            toggle.action = action.id
            toggle.accessibilityIdentifier = "checklist.action.\(action.id)"
            toggle.setOn(isOn)
        case let .choice(_, current): (view as? ChecklistChoiceView)?.show(current)
        case .words, .link, .icon: break
        }
    }

    /// While the app acts for an item its buttons take no taps, and dim only
    /// if that lasts. Most actions are done at once: switched off and on
    /// again in the same moment, the buttons blinked at every press.
    private func dimActions(_ busy: Bool) {
        guard busy != actionsAreBusy else { return }
        actionsAreBusy = busy
        dimming?.cancel()
        dimming = nil
        if busy {
            let dim = DispatchWorkItem { [weak self] in
                UIView.animate(withDuration: 0.15) { self?.actions.alpha = 0.45 }
            }
            dimming = dim
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.4, execute: dim)
        } else if actions.alpha != 1 {
            UIView.animate(withDuration: 0.15) { self.actions.alpha = 1 }
        }
    }

    private func rebuildRows(_ items: [Item]) {
        rows.forEach { $0.removeFromSuperview() }
        rows = items.map { item in
            let row = ChecklistRowView(id: item.id, number: item.number, text: item.text, style: style)
            row.onMark = { [weak self] in self?.onEvent(.mark(item.id, $0)) }
            row.onNote = { [weak self] in self?.onEvent(.note(item.id, $0)) }
            row.onTextTap = { [weak self, unowned row] in self?.wordsTapped(row) }
            row.onResize = { [weak self] in self?.setNeedsLayout() }
            row.note.delegate = self
            list.addArrangedSubview(row)
            return row
        }
        // "4a." is wider than "1."; the texts still start on one line.
        let widest = rows.map(\.numberWidth).max() ?? 0
        rows.forEach { $0.setNumberWidth(widest) }
        scroll.setContentOffset(.zero, animated: false)
    }

    /// A round button of the band: an arrow beside the island, a control under the item.
    private func round(_ button: UIButton, symbol: String, label: String, id: String, size: CGFloat = 30,
                       _ action: @escaping () -> Void) {
        var configuration = UIButton.Configuration.plain()
        configuration.image = UIImage(systemName: symbol)
        configuration.preferredSymbolConfigurationForImage = .init(pointSize: 12, weight: .semibold)
        configuration.cornerStyle = .capsule
        configuration.contentInsets = .zero
        configuration.background.backgroundColor = ChecklistPanelStyle.control
        configuration.baseForegroundColor = ChecklistPanelStyle.text
        button.configuration = configuration
        button.accessibilityLabel = label
        button.accessibilityIdentifier = id
        button.addAction(UIAction { _ in action() }, for: .primaryActionTriggered)
        button.widthAnchor.constraint(equalToConstant: size).isActive = true
        button.heightAnchor.constraint(equalToConstant: size).isActive = true
    }

    @objc private func expand() { setCollapsed(false) }
}
#endif
