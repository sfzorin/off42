import AppKit

/// Thin status bar at the top of a document (signature state, read-only, compatibility, ...).
public final class BannerView: NSView {
    public enum Style {
        case info, success, warning, error

        var color: NSColor {
            switch self {
            case .info: return .systemBlue
            case .success: return .systemGreen
            case .warning: return .systemOrange
            case .error: return .systemRed
            }
        }

        var symbol: String {
            switch self {
            case .info: return "info.circle.fill"
            case .success: return "checkmark.seal.fill"
            case .warning: return "exclamationmark.triangle.fill"
            case .error: return "xmark.seal.fill"
            }
        }
    }

    private let icon = NSImageView()
    private let label = NSTextField(labelWithString: "")
    private let button = NSButton(title: "", target: nil, action: nil)
    private var handler: (() -> Void)?
    private var style: Style = .info
    private var height: NSLayoutConstraint!

    /// A hidden banner takes no height, so it can sit in a constraint layout as well as in a stack view.
    public override var isHidden: Bool {
        didSet { height?.constant = isHidden ? 0 : Theme42.bannerHeight }
    }

    public override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        isHidden = true

        label.font = .systemFont(ofSize: 12, weight: .medium)
        label.lineBreakMode = .byTruncatingTail
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        button.bezelStyle = .accessoryBarAction
        button.controlSize = .small
        button.target = self
        button.action = #selector(tapped)

        let stack = NSStackView(views: [icon, label, NSView(), button])
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 14, bottom: 0, right: 10)
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)

        let line = NSBox()
        line.boxType = .separator
        line.translatesAutoresizingMaskIntoConstraints = false
        addSubview(line)

        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
            line.leadingAnchor.constraint(equalTo: leadingAnchor),
            line.trailingAnchor.constraint(equalTo: trailingAnchor),
            line.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        height = heightAnchor.constraint(equalToConstant: 0)
        height.isActive = true
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    public func show(_ style: Style, _ text: String, action: String? = nil, handler: (() -> Void)? = nil) {
        self.style = style
        updateTint()
        icon.image = Theme42.symbol(style.symbol, size: 14, weight: .semibold)
        icon.contentTintColor = style.color
        label.stringValue = text
        button.title = action ?? ""
        button.isHidden = action == nil
        self.handler = handler
        isHidden = false
    }

    public func hide() { isHidden = true }

    /// Current text and style (UI regression checks).
    public var text: String { label.stringValue }
    public var currentStyle: Style { style }
    /// Views that hung from the top of the safe area before `install(in:)` put the banner there.
    public internal(set) var anchoredBelow: [NSView] = []

    /// Layer colours are not dynamic: resolve the system colour for the current appearance.
    private func updateTint() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            layer?.backgroundColor = style.color.withAlphaComponent(0.14).cgColor
        }
    }

    public override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        updateTint()
    }

    @objc private func tapped() { handler?() }
}
