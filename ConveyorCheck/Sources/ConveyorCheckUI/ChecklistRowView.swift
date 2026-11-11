#if canImport(UIKit)
import ConveyorCheck
import UIKit

/// One item of a checklist: its number and what to look at, and under them
/// its steps and its comment. With all the items in sight every line has its
/// three marks beside it; the item that is alone on screen has them beside the
/// island instead. A comment field opens when the item is marked as not
/// working, or when the panel asks for it; nothing else stands for it, so a
/// line without a comment stays one line.
final class ChecklistRowView: UIView {
    let id: String
    let note = UITextField()
    var onMark: (CheckMark) -> Void = { _ in }
    var onNote: (String) -> Void = { _ in }
    /// A tap on the item's words: what it opens is the panel's to say.
    var onTextTap: () -> Void = {}
    /// The comment or the detail opened or closed: the panel has a new height.
    var onResize: () -> Void = {}

    private let marks = ChecklistMarksView(buttonWidth: 36)
    private var state: CheckMark?
    private var text: String?
    private let number = UILabel()
    private let detail = UILabel()
    private var numberWidthConstraint: NSLayoutConstraint?
    /// The tester opened the comment by hand; left empty, it closes again.
    private var isCommentOpen = false
    /// The panel shows the comment with the buttons under the item, written or not.
    private var keepsCommentOpen = false

    /// What the number needs; the panel gives every row the widest.
    var numberWidth: CGFloat { ceil(number.intrinsicContentSize.width) }

    func setNumberWidth(_ width: CGFloat) {
        numberWidthConstraint?.isActive = false
        numberWidthConstraint = number.widthAnchor.constraint(equalToConstant: width)
        numberWidthConstraint?.isActive = true
    }

    init(id: String, number numberText: String, text itemText: String, style: ChecklistPanelStyle) {
        self.id = id
        super.init(frame: .zero)
        number.text = numberText
        number.font = .monospacedDigitSystemFont(ofSize: 14, weight: .semibold)
        number.textColor = ChecklistPanelStyle.secondaryText
        number.setContentHuggingPriority(.required, for: .horizontal)
        number.setContentCompressionResistancePriority(.required, for: .horizontal)
        let text = UILabel()
        text.text = itemText
        text.font = style.font(14)
        text.textColor = ChecklistPanelStyle.text
        text.numberOfLines = 0
        text.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        marks.onMark = { [weak self] in self?.onMark($0) }
        marks.setContentHuggingPriority(.required, for: .horizontal)
        marks.setContentCompressionResistancePriority(.required, for: .horizontal)

        // The first line of the text sits level with the marks beside it.
        let words = UIStackView(arrangedSubviews: [number, text])
        words.axis = .horizontal
        words.spacing = 6
        words.alignment = .firstBaseline
        words.isLayoutMarginsRelativeArrangement = true
        words.directionalLayoutMargins = .init(top: 6, leading: 0, bottom: 0, trailing: 0)
        // A tap on the words opens the comment: no fourth control per line.
        words.addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(wordsTapped)))
        words.isAccessibilityElement = true
        words.accessibilityLabel = "\(numberText) \(itemText)"
        words.accessibilityHint = "Комментарий к пункту"
        words.accessibilityTraits = .button
        words.accessibilityIdentifier = "checklist.item.\(id).text"
        let line = UIStackView(arrangedSubviews: [words, marks])
        line.axis = .horizontal
        line.spacing = 8
        line.alignment = .top

        detail.numberOfLines = 0
        detail.isHidden = true
        detail.accessibilityIdentifier = "checklist.item.\(id).detail"

        note.font = style.font(15)
        note.textColor = ChecklistPanelStyle.text
        note.attributedPlaceholder = NSAttributedString(string: "Комментарий",
                                                        attributes: [.foregroundColor: ChecklistPanelStyle.secondaryText])
        note.backgroundColor = ChecklistPanelStyle.field
        note.keyboardAppearance = .dark
        note.layer.cornerRadius = 12
        note.layer.cornerCurve = .continuous
        note.returnKeyType = .done
        note.leftView = UIView(frame: CGRect(x: 0, y: 0, width: 11, height: 1))
        note.leftViewMode = .always
        note.rightView = UIView(frame: CGRect(x: 0, y: 0, width: 11, height: 1))
        note.rightViewMode = .always
        note.heightAnchor.constraint(equalToConstant: 36).isActive = true
        note.accessibilityIdentifier = "checklist.item.\(id).note"
        note.addAction(UIAction { [weak self] _ in self.map { $0.onNote($0.note.text ?? "") } }, for: .editingChanged)

        let column = UIStackView(arrangedSubviews: [line, detail, note])
        column.axis = .vertical
        column.spacing = 6
        column.translatesAutoresizingMaskIntoConstraints = false
        addSubview(column)
        NSLayoutConstraint.activate([
            column.topAnchor.constraint(equalTo: topAnchor), column.bottomAnchor.constraint(equalTo: bottomAnchor),
            column.leadingAnchor.constraint(equalTo: leadingAnchor), column.trailingAnchor.constraint(equalTo: trailingAnchor),
        ])
        note.isHidden = true
        marks.setItem(id)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("Use init(id:number:text:style:)") }

    /// - Parameters:
    ///   - showsMarks: The marks stand in the line: all the items are in sight.
    ///   - keepsCommentOpen: The comment is in sight, written or not.
    func render(state next: CheckMark?, note nextText: String?, detail nextDetail: NSAttributedString?, showsMarks: Bool,
                keepsCommentOpen keeps: Bool) {
        if marks.isHidden == showsMarks {
            marks.isHidden = !showsMarks
            // An item's marks are found in one place: here, or beside the island.
            marks.setItem(showsMarks ? id : nil)
        }
        if nextDetail != detail.attributedText || detail.isHidden == (nextDetail != nil) {
            detail.attributedText = nextDetail
            detail.isHidden = nextDetail == nil
            onResize()
        }
        let becameProblem = next == .problem && state != .problem
        if next != state || nextText != text {
            state = next
            text = nextText
            marks.render(next)
            // Typing must not be overwritten by the echo of its own change.
            if !note.isFirstResponder { note.text = nextText }
        }
        keepsCommentOpen = keeps
        showComment(wantsComment)
        // A problem is worth nothing without what went wrong, so the comment opens with it.
        if becameProblem, window != nil, !isHidden { note.becomeFirstResponder() }
    }

    /// Opens the comment for typing; closes one that is open and empty.
    func toggleComment() {
        if note.isHidden {
            isCommentOpen = true
            showComment(true)
            if window != nil { note.becomeFirstResponder() }
        } else if (note.text ?? "").isEmpty, state != .problem {
            isCommentOpen = false
            note.resignFirstResponder()
            showComment(false)
        } else if window != nil {
            note.becomeFirstResponder()
        }
    }

    /// An empty comment the tester opened by hand does not stay on screen.
    func commentEditingEnded() {
        isCommentOpen = false
        showComment(wantsComment)
    }

    @objc private func wordsTapped() { onTextTap() }

    private var wantsComment: Bool {
        keepsCommentOpen || isCommentOpen || state == .problem || !(text ?? "").isEmpty
    }

    private func showComment(_ shown: Bool) {
        guard note.isHidden == shown else { return }
        note.isHidden = !shown
        onResize()
    }
}

/// The three marks of an item: works, does not work, skip.
final class ChecklistMarksView: UIStackView {
    var onMark: (CheckMark) -> Void = { _ in }

    private static let marks: [(state: CheckMark, symbol: String, label: String)] = [
        (.ok, "checkmark", "Работает"), (.problem, "xmark", "Не работает"), (.skipped, "minus", "Пропустить"),
    ]

    private var buttons: [CheckMark: UIButton] = [:]
    private var state: CheckMark?

    /// - Parameter buttonWidth: The width of a mark; nil shares out what the view is given.
    init(buttonWidth: CGFloat?) {
        super.init(frame: .zero)
        axis = .horizontal
        spacing = buttonWidth == nil ? 6 : 4
        distribution = buttonWidth == nil ? .fillEqually : .fill
        alignment = .center
        for mark in Self.marks {
            let button = UIButton(type: .system)
            button.accessibilityLabel = mark.label
            button.addAction(UIAction { [weak self] _ in self?.onMark(mark.state) }, for: .primaryActionTriggered)
            if let buttonWidth { button.widthAnchor.constraint(equalToConstant: buttonWidth).isActive = true }
            button.heightAnchor.constraint(equalToConstant: 30).isActive = true
            buttons[mark.state] = button
            addArrangedSubview(button)
        }
        styleButtons()
    }

    @available(*, unavailable)
    required init(coder: NSCoder) { fatalError("Use init(buttonWidth:)") }

    /// Whose marks they are: a mark is found by its item. Nil while they are out of sight.
    func setItem(_ id: String?) {
        for (state, button) in buttons {
            button.accessibilityIdentifier = id.map { "checklist.item.\($0).\(state.rawValue)" }
        }
    }

    func render(_ next: CheckMark?) {
        guard next != state else { return }
        state = next
        styleButtons()
    }

    private func styleButtons() {
        for mark in Self.marks {
            guard let button = buttons[mark.state] else { continue }
            let selected = state == mark.state
            let color: UIColor = switch mark.state {
            case .ok: .systemGreen
            case .problem: .systemRed
            case .skipped: .systemGray
            }
            var configuration: UIButton.Configuration = selected ? .filled() : .tinted()
            configuration.image = UIImage(systemName: mark.symbol)
            configuration.preferredSymbolConfigurationForImage = .init(pointSize: 12, weight: .bold)
            configuration.cornerStyle = .capsule
            configuration.contentInsets = .zero
            configuration.baseBackgroundColor = color
            configuration.baseForegroundColor = selected ? .white : color
            button.configuration = configuration
            button.accessibilityTraits = selected ? [.button, .selected] : .button
        }
    }
}

/// How far a checklist has come: a dash for every item in the colour of its
/// mark, the open one taller than the rest.
final class ChecklistProgressView: UIView {
    private var marks: [CheckMark?] = []
    private var current: Int?

    init() {
        super.init(frame: .zero)
        isOpaque = false
        backgroundColor = .clear
        isUserInteractionEnabled = false
        contentMode = .redraw
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("Use init()") }

    func show(marks next: [CheckMark?], current nextCurrent: Int?) {
        guard next != marks || nextCurrent != current else { return }
        marks = next
        current = nextCurrent
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        guard !marks.isEmpty else { return }
        // A long plan still fits under the island: its dashes get thin, not cut.
        let gap: CGFloat = marks.count > 12 ? 1 : 3
        let width = max((bounds.width - gap * CGFloat(marks.count - 1)) / CGFloat(marks.count), 1)
        for (index, mark) in marks.enumerated() {
            let isCurrent = index == current
            let height = isCurrent ? bounds.height : bounds.height / 2
            let dash = CGRect(x: CGFloat(index) * (width + gap), y: (bounds.height - height) / 2, width: width, height: height)
            let color: UIColor = switch mark {
            case .ok: .systemGreen
            case .problem: .systemRed
            case .skipped: .systemGray
            case nil: isCurrent ? ChecklistPanelStyle.text : UIColor(white: 0.23, alpha: 1)
            }
            color.setFill()
            UIBezierPath(roundedRect: dash, cornerRadius: min(width, height) / 2).fill()
        }
    }
}

/// The buttons of what the app can do for an item, in lines: as many to a
/// line as fit, a long name on a line of its own, and none cut off by the
/// screen's edge.
final class ChecklistFlowView: UIView {
    private static let spacing: CGFloat = 6
    private static let lineHeight: CGFloat = 32
    private var height: CGFloat = 0

    override var intrinsicContentSize: CGSize { CGSize(width: UIView.noIntrinsicMetric, height: height) }

    override func layoutSubviews() {
        super.layoutSubviews()
        var x: CGFloat = 0
        var y: CGFloat = 0
        for view in subviews {
            let width = min(ceil(view.systemLayoutSizeFitting(UIView.layoutFittingCompressedSize).width), bounds.width)
            if x > 0, x + width > bounds.width {
                x = 0
                y += Self.lineHeight + Self.spacing
            }
            view.frame = CGRect(x: x, y: y, width: width, height: Self.lineHeight)
            x += width + Self.spacing
        }
        let needed = subviews.isEmpty ? 0 : y + Self.lineHeight
        if abs(needed - height) > 0.5 {
            height = needed
            invalidateIntrinsicContentSize()
        }
    }
}

/// A switch among an item's buttons: a symbol or a few words for what it
/// switches, and how that stands now. The whole of it is the button, and the
/// app says how it stands once the action has run.
final class ChecklistSwitchButton: UIControl {
    /// What a tap runs: the action that flips the switch from where it stands.
    var action = ""
    private let knob = ChecklistSwitchKnob()

    init(symbol: String?, caption: String?, name: String, isOn: Bool, style: ChecklistPanelStyle) {
        super.init(frame: .zero)
        backgroundColor = ChecklistPanelStyle.control
        layer.cornerRadius = 16
        let row = UIStackView()
        row.spacing = 7
        row.alignment = .center
        row.isUserInteractionEnabled = false
        let image = symbol.flatMap { UIImage(systemName: $0, withConfiguration: UIImage.SymbolConfiguration(pointSize: 13, weight: .semibold)) }
        if let image {
            let icon = UIImageView(image: image)
            icon.tintColor = ChecklistPanelStyle.text
            icon.setContentCompressionResistancePriority(.required, for: .horizontal)
            row.addArrangedSubview(icon)
        }
        // A switch says nothing by itself: with no symbol to show, its name stands in the caption's place.
        if let words = caption ?? (image == nil ? name : nil) {
            let label = UILabel()
            label.text = words
            label.font = style.font(13, weight: .semibold)
            label.textColor = ChecklistPanelStyle.text
            label.lineBreakMode = .byTruncatingTail
            row.addArrangedSubview(label)
        }
        row.addArrangedSubview(knob)
        row.translatesAutoresizingMaskIntoConstraints = false
        addSubview(row)
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12), row.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
            row.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
        isAccessibilityElement = true
        accessibilityLabel = name
        accessibilityTraits = .button
        setOn(isOn, animated: false)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("Use init(symbol:caption:name:isOn:style:)") }

    var isOn: Bool { knob.isOn }

    func setOn(_ isOn: Bool, animated: Bool = true) {
        knob.setOn(isOn, animated: animated)
        accessibilityValue = isOn ? "включено" : "выключено"
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        super.touchesEnded(touches, with: event)
        if isEnabled, touches.contains(where: { bounds.contains($0.location(in: self)) }) { sendActions(for: .primaryActionTriggered) }
    }
}

/// The switch itself, drawn by the band: a track and a round knob, green when
/// on. The system's switch is made for a finger and a row of a list; scaled
/// down to a line of buttons it was wider than its place and looked cut.
final class ChecklistSwitchKnob: UIView {
    private static let size = CGSize(width: 34, height: 20)
    private let knob = UIView()
    private var leading: NSLayoutConstraint!
    private(set) var isOn = false

    init() {
        super.init(frame: .zero)
        isUserInteractionEnabled = false
        layer.cornerRadius = Self.size.height / 2
        knob.backgroundColor = .white
        knob.layer.cornerRadius = (Self.size.height - 4) / 2
        knob.translatesAutoresizingMaskIntoConstraints = false
        addSubview(knob)
        leading = knob.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2)
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: Self.size.width), heightAnchor.constraint(equalToConstant: Self.size.height),
            knob.widthAnchor.constraint(equalToConstant: Self.size.height - 4), knob.heightAnchor.constraint(equalTo: knob.widthAnchor),
            knob.centerYAnchor.constraint(equalTo: centerYAnchor), leading,
        ])
        setOn(false, animated: false)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("Use init()") }

    func setOn(_ isOn: Bool, animated: Bool) {
        self.isOn = isOn
        let place = {
            self.backgroundColor = isOn ? .systemGreen : UIColor(white: 0.34, alpha: 1)
            self.leading.constant = isOn ? Self.size.width - Self.size.height + 2 : 2
            self.layoutIfNeeded()
        }
        if animated, window != nil { UIView.animate(withDuration: 0.18, animations: place) } else { place() }
    }
}

/// One of several among an item's buttons: the options in a row, the one
/// that holds lit. A tap on an option chooses it.
final class ChecklistChoiceView: UIView {
    var onPick: (String) -> Void = { _ in }
    private let options: [ChecklistPanelView.Action.Option]
    private var buttons: [UIButton] = []
    private let style: ChecklistPanelStyle

    init(name: String, options: [ChecklistPanelView.Action.Option], current: String?, style: ChecklistPanelStyle) {
        self.options = options
        self.style = style
        super.init(frame: .zero)
        backgroundColor = ChecklistPanelStyle.control
        layer.cornerRadius = 16
        let row = UIStackView()
        row.translatesAutoresizingMaskIntoConstraints = false
        addSubview(row)
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 3), row.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -3),
            row.topAnchor.constraint(equalTo: topAnchor, constant: 3), row.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -3),
        ])
        for option in options {
            let button = UIButton(type: .system)
            button.accessibilityIdentifier = "checklist.action.\(option.id)"
            button.accessibilityLabel = "\(name): \(option.title)"
            button.addAction(UIAction { [weak self] _ in self?.onPick(option.id) }, for: .primaryActionTriggered)
            if option.symbol.flatMap({ UIImage(systemName: $0) }) == nil {
                // As wide as its word is when it holds, and heavier: the row does not shift when another option is chosen.
                let word = ceil((option.title as NSString).size(withAttributes: [.font: style.font(13, weight: .semibold)]).width)
                let wide = button.widthAnchor.constraint(equalToConstant: word + 20)
                wide.priority = .defaultHigh
                wide.isActive = true
            }
            buttons.append(button)
            row.addArrangedSubview(button)
        }
        show(current)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("Use init(name:options:current:style:)") }

    /// Lights the option that holds now.
    func show(_ current: String?) {
        for (button, option) in zip(buttons, options) {
            let holds = option.id == current
            var configuration = UIButton.Configuration.plain()
            if let symbol = option.symbol, let image = UIImage(systemName: symbol) {
                configuration.image = image
                configuration.preferredSymbolConfigurationForImage = .init(pointSize: 12, weight: holds ? .semibold : .medium)
            } else {
                configuration.attributedTitle = AttributedString(option.title, attributes: .init([
                    .font: style.font(13, weight: holds ? .semibold : .medium),
                ]))
                configuration.titleLineBreakMode = .byTruncatingTail
            }
            configuration.cornerStyle = .capsule
            configuration.contentInsets = .init(top: 2, leading: 10, bottom: 2, trailing: 10)
            configuration.background.backgroundColor = holds ? ChecklistPanelStyle.chosen : .clear
            configuration.baseForegroundColor = holds ? ChecklistPanelStyle.text : ChecklistPanelStyle.secondaryText
            button.configuration = configuration
            button.accessibilityTraits = holds ? [.button, .selected] : .button
        }
    }
}
#endif
