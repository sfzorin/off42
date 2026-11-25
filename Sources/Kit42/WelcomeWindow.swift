import AppKit

/// Start window shown when no document is open: app identity, primary actions, recent files.
public final class WelcomeWindowController: NSWindowController {
    public struct Action {
        public let title: String
        public let symbol: String
        public let action: Selector
        public init(_ title: String, symbol: String, action: Selector) {
            self.title = title
            self.symbol = symbol
            self.action = action
        }
    }

    private let recentStack = NSStackView()

    public init(app: Theme42.App, tagline: String, actions: [Action]) {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 680, height: 420),
                              styleMask: [.titled, .closable, .fullSizeContentView],
                              backing: .buffered, defer: false)
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.isMovableByWindowBackground = true
        window.standardWindowButton(.miniaturizeButton)?.isHidden = true
        window.standardWindowButton(.zoomButton)?.isHidden = true
        super.init(window: window)

        // Left: identity and actions
        let icon = NSImageView(image: NSApp.applicationIconImage ?? NSImage())
        icon.translatesAutoresizingMaskIntoConstraints = false
        icon.widthAnchor.constraint(equalToConstant: 112).isActive = true
        icon.heightAnchor.constraint(equalToConstant: 112).isActive = true

        let name = NSTextField(labelWithString: app.name)
        name.font = .systemFont(ofSize: 30, weight: .bold)
        let sub = NSTextField(labelWithString: tagline)
        sub.font = .systemFont(ofSize: 13)
        sub.textColor = .secondaryLabelColor
        let version = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? ""
        let ver = NSTextField(labelWithString: version.isEmpty ? "" : L("Version %@", version))
        ver.font = .systemFont(ofSize: 11)
        ver.textColor = .tertiaryLabelColor

        let actionButtons = actions.map(Self.actionButton)
        // Translated actions can be much longer than the English source. Give every
        // action the same natural width instead of clipping it to 220 points.
        let actionWidth = ceil(max(220, actionButtons.map { $0.intrinsicContentSize.width + 16 }.max() ?? 220))
        actionButtons.forEach { $0.widthAnchor.constraint(equalToConstant: actionWidth).isActive = true }
        let buttons = NSStackView(views: actionButtons)
        buttons.orientation = .vertical
        buttons.alignment = .centerX
        buttons.spacing = 4

        let language = NSStackView(views: [Theme42.symbol("globe", size: 12).map { NSImageView(image: $0) } ?? NSView(), LanguagePicker()])
        language.spacing = 4
        (language.arrangedSubviews.first as? NSImageView)?.contentTintColor = .secondaryLabelColor

        let left = NSStackView(views: [icon, name, sub, ver, buttons, NSView(), language])
        left.orientation = .vertical
        left.alignment = .centerX
        left.spacing = 6
        left.setCustomSpacing(14, after: icon)
        left.setCustomSpacing(28, after: ver)
        left.setCustomSpacing(20, after: buttons)
        left.edgeInsets = NSEdgeInsets(top: 44, left: 32, bottom: 32, right: 32)

        // Right: recent documents
        let header = NSTextField(labelWithString: L("Recent"))
        header.font = .systemFont(ofSize: 11, weight: .semibold)
        header.textColor = .secondaryLabelColor
        recentStack.orientation = .vertical
        recentStack.alignment = .leading
        recentStack.spacing = 2
        let right = NSStackView(views: [header, recentStack])
        right.orientation = .vertical
        right.alignment = .leading
        right.spacing = 8
        right.edgeInsets = NSEdgeInsets(top: 44, left: 16, bottom: 20, right: 16)

        let rightBackground = NSVisualEffectView()
        rightBackground.material = .sidebar
        rightBackground.blendingMode = .behindWindow
        right.translatesAutoresizingMaskIntoConstraints = false
        rightBackground.addSubview(right)
        NSLayoutConstraint.activate([
            right.leadingAnchor.constraint(equalTo: rightBackground.leadingAnchor),
            right.trailingAnchor.constraint(equalTo: rightBackground.trailingAnchor),
            right.topAnchor.constraint(equalTo: rightBackground.topAnchor),
        ])

        let root = NSStackView(views: [left, rightBackground])
        root.spacing = 0
        root.distribution = .fill
        left.translatesAutoresizingMaskIntoConstraints = false
        let leftWidth = ceil(max(330, actionWidth + 64, sub.intrinsicContentSize.width + 64))
        left.widthAnchor.constraint(equalToConstant: leftWidth).isActive = true
        rightBackground.translatesAutoresizingMaskIntoConstraints = false
        rightBackground.heightAnchor.constraint(equalTo: root.heightAnchor).isActive = true
        window.contentView = root
        window.setContentSize(NSSize(width: leftWidth + 350, height: 420))
        window.center()
        reloadRecent()
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    public override func showWindow(_ sender: Any?) {
        reloadRecent()
        super.showWindow(sender)
    }

    private func reloadRecent() {
        recentStack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        let urls = NSDocumentController.shared.recentDocumentURLs.prefix(8)
        if urls.isEmpty {
            let empty = NSTextField(labelWithString: L("No recent documents"))
            empty.textColor = .tertiaryLabelColor
            recentStack.addArrangedSubview(empty)
        }
        for url in urls {
            let row = RecentRow(url: url) { [weak self] in
                NSDocumentController.shared.openDocument(withContentsOf: url, display: true) { _, _, error in
                    if let error { NSApp.presentError(error) } else { self?.close() }
                }
            }
            recentStack.addArrangedSubview(row)
            row.widthAnchor.constraint(equalToConstant: 318).isActive = true
        }
    }

    private static func actionButton(_ action: Action) -> NSButton {
        let b = NSButton(title: "  " + action.title, image: Self.paddedSymbol(action.symbol), target: nil, action: action.action)
        b.imagePosition = .imageLeading
        b.bezelStyle = .accessoryBarAction
        b.showsBorderOnlyWhileMouseInside = true
        b.font = .systemFont(ofSize: 14)
        b.contentTintColor = .controlAccentColor
        b.translatesAutoresizingMaskIntoConstraints = false
        b.heightAnchor.constraint(equalToConstant: 30).isActive = true
        b.alignment = .left
        return b
    }

    /// Symbol centred in a fixed 24×20 canvas so button titles line up.
    private static func paddedSymbol(_ name: String) -> NSImage {
        let symbol = Theme42.symbol(name, size: 15) ?? NSImage()
        let image = NSImage(size: NSSize(width: 24, height: 20), flipped: false) { rect in
            let s = symbol.size
            symbol.draw(in: NSRect(x: (rect.width - s.width) / 2, y: (rect.height - s.height) / 2, width: s.width, height: s.height))
            return true
        }
        image.isTemplate = true
        return image
    }
}

private final class RecentRow: NSButton {
    private let onClick: () -> Void

    init(url: URL, onClick: @escaping () -> Void) {
        self.onClick = onClick
        super.init(frame: .zero)
        title = ""
        bezelStyle = .accessoryBarAction
        showsBorderOnlyWhileMouseInside = true
        target = self
        action = #selector(clicked)
        toolTip = url.path

        let icon = NSImageView(image: NSWorkspace.shared.icon(forFile: url.path))
        icon.translatesAutoresizingMaskIntoConstraints = false
        icon.widthAnchor.constraint(equalToConstant: 28).isActive = true
        icon.heightAnchor.constraint(equalToConstant: 28).isActive = true
        let name = NSTextField(labelWithString: url.lastPathComponent)
        name.font = .systemFont(ofSize: 13, weight: .medium)
        name.lineBreakMode = .byTruncatingMiddle
        let folder = NSTextField(labelWithString: (url.deletingLastPathComponent().path as NSString).abbreviatingWithTildeInPath)
        folder.font = .systemFont(ofSize: 11)
        folder.textColor = .secondaryLabelColor
        folder.lineBreakMode = .byTruncatingMiddle
        let text = NSStackView(views: [name, folder])
        text.orientation = .vertical
        text.alignment = .leading
        text.spacing = 1
        let row = NSStackView(views: [icon, text])
        row.spacing = 8
        row.edgeInsets = NSEdgeInsets(top: 4, left: 6, bottom: 4, right: 6)
        row.translatesAutoresizingMaskIntoConstraints = false
        addSubview(row)
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: leadingAnchor),
            row.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
            row.topAnchor.constraint(equalTo: topAnchor),
            row.bottomAnchor.constraint(equalTo: bottomAnchor),
            heightAnchor.constraint(equalToConstant: 42),
        ])
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    @objc private func clicked() { onClick() }
}
