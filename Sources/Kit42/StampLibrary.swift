import AppKit
import UniformTypeIdentifiers

/// Shared library of signature and stamp images (PNG with transparency), used by every Suite42 app.
/// Stored in ~/Library/Application Support/Suite42/Stamps.
public final class StampLibrary {
    public static let shared = StampLibrary()

    public let folder: URL = {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let url = base.appendingPathComponent("Suite42/Stamps", isDirectory: true)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }()

    public var items: [URL] {
        let urls = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.creationDateKey])) ?? []
        return urls.filter { $0.pathExtension.lowercased() == "png" }.sorted {
            let a = (try? $0.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
            let b = (try? $1.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
            return a > b
        }
    }

    @discardableResult
    public func add(_ image: CGImage) throws -> URL {
        let url = folder.appendingPathComponent(UUID().uuidString + ".png")
        guard let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError)
        }
        try data.write(to: url)
        return url
    }

    public func remove(_ url: URL) {
        if let source = Self.mark(at: url)?.source { try? FileManager.default.removeItem(at: sourcesFolder.appendingPathComponent(source)) }
        try? FileManager.default.removeItem(at: url)
        try? FileManager.default.removeItem(at: Self.recipeURL(for: url))
    }

    public static func image(at url: URL) -> CGImage? {
        guard let image = NSImage(contentsOf: url) else { return nil }
        var rect = CGRect(origin: .zero, size: image.size)
        return image.cgImage(forProposedRect: &rect, context: nil, hints: nil)
    }

    /// Asks for an image file and optionally cleans the white background. Returns nil when cancelled.
    public static func pickImage(window: NSWindow?, askCleanup: Bool = true) -> CGImage? {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.png, .jpeg, .heic, .tiff, .image]
        panel.message = L("Choose an image of a signature or stamp")
        guard panel.runModal() == .OK, let url = panel.url, var image = image(at: url) else { return nil }
        if askCleanup {
            let alert = NSAlert()
            alert.messageText = L("Remove white background?")
            alert.informativeText = L("For scanned signatures and stamps: white becomes transparent and empty margins are cropped.")
            alert.addButton(withTitle: L("Remove Background"))
            alert.addButton(withTitle: L("Keep As Is"))
            if alert.runModal() == .alertFirstButtonReturn {
                image = ImageTools.removeWhiteBackground(image) ?? image
            }
        }
        return image
    }
}

public enum ImageTools {
    /// Makes near-white pixels transparent (with a soft edge) and trims empty borders.
    public static func removeWhiteBackground(_ image: CGImage, threshold: CGFloat = 0.86) -> CGImage? {
        let w = image.width, h = image.height
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                  space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
              let data = ctx.data else { return nil }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        let px = data.bindMemory(to: UInt8.self, capacity: w * h * 4)
        let soft: CGFloat = 0.10
        var minX = w, minY = h, maxX = -1, maxY = -1
        for y in 0..<h {
            for x in 0..<w {
                let i = (y * w + x) * 4
                let a = CGFloat(px[i + 3]) / 255
                guard a > 0 else { continue }
                let r = CGFloat(px[i]) / 255 / a, g = CGFloat(px[i + 1]) / 255 / a, b = CGFloat(px[i + 2]) / 255 / a
                let lum = 0.299 * r + 0.587 * g + 0.114 * b
                var keep: CGFloat = 1
                if lum >= threshold + soft { keep = 0 } else if lum > threshold { keep = 1 - (lum - threshold) / soft }
                let na = a * keep
                // Re-premultiply with the original colour.
                px[i] = UInt8(min(255, r * na * 255)); px[i + 1] = UInt8(min(255, g * na * 255))
                px[i + 2] = UInt8(min(255, b * na * 255)); px[i + 3] = UInt8(na * 255)
                if na > 0.05 {
                    minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y)
                }
            }
        }
        guard let cleaned = ctx.makeImage() else { return nil }
        guard maxX >= minX, maxY >= minY else { return cleaned }
        let pad = 4
        // CGContext rows start at the bottom; CGImage cropping starts at the top.
        let crop = CGRect(x: max(0, minX - pad), y: max(0, (h - 1 - maxY) - pad),
                          width: min(w, maxX - minX + 1 + pad * 2), height: min(h, maxY - minY + 1 + pad * 2))
        return cleaned.cropping(to: crop) ?? cleaned
    }
}

/// Popover with the signature/stamp library. `onPick` receives the chosen image.
public final class StampLibraryViewController: NSViewController {
    private let onPick: (CGImage) -> Void
    private let grid = NSStackView()
    private let columns = 3
    public weak var popover: NSPopover?
    /// False when the library is opened only for management (no document to insert into).
    public var canInsert = true
    /// Where the mark will go, for the mark's aspect ratio (width / height): its width and, on a scanned page, the
    /// scan's resolution. Asked when a mark is inserted; the default is the usual size on a vector page.
    public var placement: (CGFloat) -> MarkPlacement = { MarkPlacement.standard(aspect: $0) }

    public init(onPick: @escaping (CGImage) -> Void) {
        self.onPick = onPick
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    public override func loadView() {
        let title = NSTextField(labelWithString: L("Signatures and Stamps"))
        title.font = .systemFont(ofSize: 13, weight: .semibold)

        grid.orientation = .vertical
        grid.alignment = .leading
        grid.spacing = 8

        let add = NSButton(title: L("Add to Library…"), target: self, action: #selector(addToLibrary))
        add.bezelStyle = .push
        add.keyEquivalent = "\r"
        let once = NSButton(title: L("Insert File…"), target: self, action: #selector(insertOnce))
        once.bezelStyle = .push
        let buttons = NSStackView(views: [once, add])
        buttons.spacing = 10
        buttons.distribution = .fillEqually
        let makeSignature = NSButton(title: L("Create Signature…"), target: self, action: #selector(createSignature))
        let makeStamp = NSButton(title: L("Create Stamp…"), target: self, action: #selector(createStamp))
        for b in [makeSignature, makeStamp] { b.bezelStyle = .push }
        let generate = NSStackView(views: [makeSignature, makeStamp])
        generate.spacing = 10
        generate.distribution = .fillEqually
        let learn = NSButton(title: L("My Signature…"), target: self, action: #selector(learnSignature))
        learn.bezelStyle = .push
        learn.toolTip = L("Teach the generator your own signature: sign a few times or bring a scan")

        let stack = NSStackView(views: [title, grid, generate, learn, buttons])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        // Fixed width with real insets: wrapped text measures correctly and the buttons keep clear of the edge.
        let root = NSView()
        root.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: root.topAnchor, constant: 16),
            stack.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -16),
            stack.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: Self.inset),
            stack.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -Self.inset),
            root.widthAnchor.constraint(equalToConstant: Self.width),
            buttons.widthAnchor.constraint(equalTo: stack.widthAnchor),
            generate.widthAnchor.constraint(equalTo: stack.widthAnchor),
            learn.widthAnchor.constraint(equalTo: stack.widthAnchor),
        ])
        view = root
        reload()
    }

    private static let width: CGFloat = 372, inset: CGFloat = 20

    public override func viewWillAppear() {
        super.viewWillAppear()
        view.layoutSubtreeIfNeeded()
        preferredContentSize = view.fittingSize
    }

    private func reload() {
        grid.arrangedSubviews.forEach { $0.removeFromSuperview() }
        let items = StampLibrary.shared.items
        if items.isEmpty {
            let hint = NSTextField(wrappingLabelWithString: L("Add a PNG or a scan of your signature or stamp — the white background can be made transparent. Images are stored locally and shared by all 42 apps."))
            hint.textColor = .secondaryLabelColor
            hint.font = .systemFont(ofSize: 12)
            hint.preferredMaxLayoutWidth = Self.width - Self.inset * 2
            hint.maximumNumberOfLines = 0
            hint.translatesAutoresizingMaskIntoConstraints = false
            grid.addArrangedSubview(hint)
            hint.widthAnchor.constraint(equalToConstant: Self.width - Self.inset * 2).isActive = true
            if isViewLoaded {
                view.layoutSubtreeIfNeeded()
                preferredContentSize = view.fittingSize
            }
            return
        }
        var row: NSStackView?
        for (i, url) in items.enumerated() {
            if i % columns == 0 {
                row = NSStackView()
                row?.spacing = 8
                grid.addArrangedSubview(row!)
            }
            row?.addArrangedSubview(tile(url))
        }
        if isViewLoaded {
            view.layoutSubtreeIfNeeded()
            preferredContentSize = view.fittingSize
        }
    }

    private func tile(_ url: URL) -> NSView {
        let b = NSButton(image: NSImage(contentsOf: url) ?? NSImage(), target: self, action: #selector(picked(_:)))
        b.identifier = NSUserInterfaceItemIdentifier(url.path)
        b.bezelStyle = .smallSquare
        b.imageScaling = .scaleProportionallyDown
        b.toolTip = L("Insert. Right-click to delete.")
        b.wantsLayer = true
        b.layer?.cornerRadius = Theme42.cornerRadius
        // Stamps are ink on paper: the tile stays white in dark mode too, with a hairline edge.
        b.layer?.backgroundColor = NSColor.white.cgColor
        b.layer?.borderWidth = 0.5
        b.layer?.borderColor = NSColor(white: 0.5, alpha: 0.5).cgColor
        b.translatesAutoresizingMaskIntoConstraints = false
        b.widthAnchor.constraint(equalToConstant: 104).isActive = true
        b.heightAnchor.constraint(equalToConstant: 64).isActive = true
        let menu = NSMenu()
        let del = NSMenuItem(title: L("Remove from Library"), action: #selector(deleteItem(_:)), keyEquivalent: "")
        del.target = self
        del.representedObject = url
        menu.addItem(del)
        b.menu = menu
        return b
    }

    @objc private func picked(_ sender: NSButton) {
        // Generated signatures and stamps give a new impression every time.
        guard let path = sender.identifier?.rawValue else { return }
        let url = URL(fileURLWithPath: path)
        let source: MarkInsertDialog.Source
        if let mark = StampLibrary.mark(at: url) { source = .mark(mark) } else if let image = StampLibrary.image(at: url) { source = .picture(image) } else { return }
        let window = view.window?.parent ?? NSApp.mainWindow
        popover?.performClose(nil)
        // A preview of this impression and the resolution to draw it at, then the document places it.
        MarkInsertDialog.present(source, over: window, placement: placement, insert: onPick)
    }

    @objc private func deleteItem(_ sender: NSMenuItem) {
        guard let url = sender.representedObject as? URL else { return }
        StampLibrary.shared.remove(url)
        reload()
    }

    /// Adds a file: as a stamp (a new impression every time, from SVG, PDF or an image) or as a fixed picture.
    @objc private func addToLibrary() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image, .svg, .pdf]
        panel.message = L("Choose an image of a signature or stamp (SVG, PDF, PNG, JPEG…)")
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let alert = NSAlert()
        alert.messageText = L("How should this image be inserted?")
        alert.informativeText = L("As a stamp, every insertion is a new impression: pad pressure, grain, gaps and angle vary, as on paper. As a picture, it is inserted exactly as it is.")
        alert.addButton(withTitle: L("As a Stamp"))
        alert.addButton(withTitle: L("As a Picture"))
        alert.addButton(withTitle: L("Cancel"))
        let choice = alert.runModal()
        do {
            switch choice {
            case .alertFirstButtonReturn:
                try StampLibrary.shared.importStamp(url)
            case .alertSecondButtonReturn:
                guard var image = StampLibrary.rasterize(url) ?? StampLibrary.image(at: url) else { NSSound.beep(); return }
                if !["svg", "pdf"].contains(url.pathExtension.lowercased()) {
                    let clean = NSAlert()
                    clean.messageText = L("Remove white background?")
                    clean.informativeText = L("For scanned signatures and stamps: white becomes transparent and empty margins are cropped.")
                    clean.addButton(withTitle: L("Remove Background"))
                    clean.addButton(withTitle: L("Keep As Is"))
                    if clean.runModal() != .alertFirstButtonReturn, let original = StampLibrary.image(at: url) { image = original }
                }
                try StampLibrary.shared.add(image)
            default:
                return
            }
        } catch {
            NSApp.presentError(error)
        }
        reload()
    }

    private func openGenerator(_ mode: MarkGeneratorController.Mode) {
        let parent = NSApp.mainWindow
        let onPick = self.onPick, placement = self.placement
        popover?.performClose(nil)
        MarkGeneratorController.show(mode, over: parent, onInsert: canInsert ? { onPick($0) } : nil,
                                     onInsertMark: canInsert ? { mark in
                                         // After the generator sheet has closed: the preview and resolution dialog.
                                         DispatchQueue.main.async {
                                             MarkInsertDialog.present(.mark(mark), over: parent, placement: placement, insert: onPick)
                                         }
                                     } : nil,
                                     onLibraryChange: { [weak self] in self?.reload() })
    }

    @objc private func createSignature() { openGenerator(.signature) }

    @objc private func createStamp() { openGenerator(.seal) }

    @objc private func learnSignature() { openGenerator(.learn) }

    @objc private func insertOnce() {
        let window = view.window?.parent ?? NSApp.mainWindow
        guard let image = StampLibrary.pickImage(window: view.window) else { return }
        popover?.performClose(nil)
        MarkInsertDialog.present(.picture(image), over: window, placement: placement, insert: onPick)
    }
}
