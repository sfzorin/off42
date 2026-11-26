import AppKit
import Kit42
import PDFKit

/// Signs many PDFs with one identity and one PAdES level: each file gets its own signed copy (and, for a
/// visible signature, its own impression of the handwritten signature).
enum BatchSigner {
    enum Page: Int { case first, last }

    struct Settings {
        var options: PDFSigner.Options
        /// nil: next to each original.
        var outputFolder: URL?
        var suffix = "signed"
        /// Visible signature on this page (bottom right), or invisible.
        var visiblePage: Page?
        var handwriting: URL?
        /// A generated signature (CLI) instead of a library item; a new impression per file.
        var generatedMark: GeneratedMark?
        /// Append the offline certificate of completion after signing.
        var appendCompletion = false
        /// Files embedded before signing (e.g. a power of attorney); only into files that are not signed yet.
        var attachments: [EmbeddedFile] = []
        var overwrite = false
    }

    struct Outcome {
        var input: URL
        var output: URL?
        var error: String?
        var warnings: [String] = []
        var succeeded: Bool { output != nil && error == nil }
    }

    /// PDFs in the given files and folders (folders recursively), sorted, without duplicates.
    static func collect(_ urls: [URL]) -> [URL] {
        var found: [URL] = []
        let fm = FileManager.default
        for url in urls {
            var isDir: ObjCBool = false
            guard fm.fileExists(atPath: url.path, isDirectory: &isDir) else { continue }
            if isDir.boolValue {
                let walker = fm.enumerator(at: url, includingPropertiesForKeys: [.isRegularFileKey], options: [.skipsHiddenFiles, .skipsPackageDescendants])
                while let item = walker?.nextObject() as? URL {
                    if item.pathExtension.lowercased() == "pdf" { found.append(item) }
                }
            } else if url.pathExtension.lowercased() == "pdf" {
                found.append(url)
            }
        }
        var seen = Set<String>()
        return found.map(\.standardizedFileURL)
            .filter { seen.insert($0.path).inserted }
            .sorted { $0.path.localizedStandardCompare($1.path) == .orderedAscending }
    }

    /// "<name> (<suffix>).pdf" in the output folder (or next to the original); never the original itself.
    static func outputURL(for input: URL, settings: Settings) -> URL {
        let folder = settings.outputFolder ?? input.deletingLastPathComponent()
        let base = input.deletingPathExtension().lastPathComponent
        let suffix = settings.suffix.trimmingCharacters(in: .whitespaces)
        var url = folder.appendingPathComponent(suffix.isEmpty ? base + ".pdf" : "\(base) (\(suffix)).pdf")
        if url.standardizedFileURL == input.standardizedFileURL { url = folder.appendingPathComponent("\(base) (\(L("signed"))).pdf") }
        if !settings.overwrite {
            var n = 2
            let stem = url.deletingPathExtension().lastPathComponent
            while FileManager.default.fileExists(atPath: url.path) {
                url = folder.appendingPathComponent("\(stem) \(n).pdf")
                n += 1
            }
        }
        return url
    }

    /// Signs one file and writes the copy. Already signed files get another signature as an incremental
    /// update (existing signatures stay valid); others are normalised first, as when signing in the editor.
    static func signFile(_ input: URL, settings: Settings) throws -> Outcome {
        let data = try Data(contentsOf: input)
        guard let pdf = PDFDocument(data: data) else { throw PDFStructureError.malformed(L("not a PDF document")) }
        if pdf.isEncrypted || pdf.isLocked { throw PDFStructureError.unsupported(L("the document is encrypted")) }
        let alreadySigned = !PDFSignatureVerifier.verify(data).isEmpty
        let base: Data
        if alreadySigned { base = data } else {
            guard let normalized = pdf.dataRepresentation() else { throw PDFStructureError.malformed(L("not a PDF document")) }
            base = try PDFSigner.embed(settings.attachments, into: AcroFormFixer.ensureFields(normalized))
        }
        var options = settings.options
        if let which = settings.visiblePage, pdf.pageCount > 0, let page = pdf.page(at: which == .first ? 0 : pdf.pageCount - 1) {
            let rect = SignatureAppearance.defaultRect(in: page.bounds(for: .cropBox))
            let pen = settings.generatedMark?.impression() ?? HandwrittenSignature.impression(settings.handwriting)
            guard let image = SignatureAppearance.render(size: rect.size, handwriting: pen,
                                                         details: SignatureAppearance.details(for: options)) else {
                throw PDFStructureError.unsupported(L("The image size is invalid"))
            }
            options.visible = (pdf.index(for: page), rect, image)
        }
        var signed = try PAdES.sign(base, options: options)
        let output = outputURL(for: input, settings: settings)
        if settings.appendCompletion {
            signed.data = try CompletionCertificate.append(to: signed.data, fileName: output.lastPathComponent)
        }
        try FileManager.default.createDirectory(at: output.deletingLastPathComponent(), withIntermediateDirectories: true)
        try signed.data.write(to: output, options: .atomic)
        var warnings = signed.warnings
        if alreadySigned, !settings.attachments.isEmpty { warnings.append(L("Attachments were not added: the file was already signed")) }
        return Outcome(input: input, output: output, warnings: warnings)
    }

    /// Signs every file; `progress(index, url)` is called before each file and returns false to stop.
    static func run(_ files: [URL], settings: Settings, progress: (Int, URL) -> Bool = { _, _ in true }) -> [Outcome] {
        var outcomes: [Outcome] = []
        for (i, url) in files.enumerated() {
            guard progress(i, url) else { break }
            do { outcomes.append(try signFile(url, settings: settings)) } catch {
                outcomes.append(Outcome(input: url, error: error.localizedDescription))
            }
        }
        return outcomes
    }

    /// Plain-text summary (also the CLI output and the exportable log).
    static func summary(_ outcomes: [Outcome], total: Int) -> String {
        var lines = [L("Signed %d of %d files", outcomes.filter(\.succeeded).count, total)]
        for o in outcomes {
            if let error = o.error { lines.append("✗ \(o.input.lastPathComponent): \(error)") }
            else if let out = o.output { lines.append("✓ \(o.input.lastPathComponent) → \(out.lastPathComponent)") }
            for w in Set(o.warnings).sorted() { lines.append("   ⚠︎ \(w)") }
        }
        if outcomes.count < total { lines.append(L("Stopped: %d files were not processed", total - outcomes.count)) }
        return lines.joined(separator: "\n")
    }
}

// MARK: - UI

/// File ▸ Sign Multiple Files…: choose files/folders, batch options, identity and level, then a progress
/// window and a summary.
enum BatchSignUI {
    static func start() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = true
        panel.allowedContentTypes = [.pdf, .folder]
        panel.message = L("Choose PDF files or folders to sign")
        panel.prompt = L("Choose")
        guard panel.runModal() == .OK else { return }
        let files = BatchSigner.collect(panel.urls)
        guard !files.isEmpty else {
            let alert = NSAlert()
            alert.messageText = L("No PDF files found")
            alert.runModal()
            return
        }
        guard var settings = askSettings(count: files.count) else { return }
        guard let (options, visible, attachments) = SignDialog.run(stampSelected: false) else { return }
        settings.options = options
        settings.attachments = attachments
        if !visible { settings.visiblePage = nil }
        settings.handwriting = HandwrittenSignature.selected
        let outcomes = runWithProgress(files, settings: settings)
        showSummary(outcomes, total: files.count)
    }

    /// Output folder, suffix and the page for a visible signature.
    static func askSettings(count: Int) -> BatchSigner.Settings? {
        let d = UserDefaults.standard
        #if APP_STORE
        var folder: URL? = d.string(forKey: "batch.folder").map { SecurityScopedFolders.url(path: $0, key: "batch.folder") }
        #else
        var folder: URL? = d.string(forKey: "batch.folder").map { URL(fileURLWithPath: $0) }
        #endif
        let where_ = NSPopUpButton(frame: .zero, pullsDown: false)
        func fillFolder() {
            where_.removeAllItems()
            where_.addItem(withTitle: L("Next to each original"))
            if let folder { where_.addItem(withTitle: L("Folder: %@", folder.lastPathComponent)) }
            if folder != nil, d.bool(forKey: "batch.useFolder") { where_.selectItem(at: 1) }
        }
        fillFolder()
        let chooser = ButtonRow()
        chooser.add(L("Choose Folder…")) {
            let p = NSOpenPanel()
            p.canChooseDirectories = true
            p.canChooseFiles = false
            p.canCreateDirectories = true
            #if APP_STORE
            if p.runModal() == .OK, let url = p.url {
                SecurityScopedFolders.remember(url, key: "batch.folder")
                folder = url; d.set(true, forKey: "batch.useFolder"); fillFolder(); where_.selectItem(at: 1)
            }
            #else
            if p.runModal() == .OK, let url = p.url { folder = url; d.set(true, forKey: "batch.useFolder"); fillFolder(); where_.selectItem(at: 1) }
            #endif
        }
        let suffix = NSTextField(string: d.string(forKey: "batch.suffix") ?? L("signed"))
        let page = NSPopUpButton(frame: .zero, pullsDown: false)
        page.addItems(withTitles: [L("First page"), L("Last page")])
        page.selectItem(at: d.integer(forKey: "batch.page"))
        let alert = NSAlert.form(L("Sign Multiple Files"),
                                 info: L("%d PDF files will be signed with one certificate and format. Each signed copy is saved as a new file; the originals are not changed.", count),
                                 ok: L("Continue…"),
                                 rows: [(L("Save to:"), where_), ("", chooser), (L("File name suffix:"), suffix), (L("Visible signature on:"), page)])
        alert.icon = Theme42.symbol("signature", size: 40)
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let useFolder = where_.indexOfSelectedItem >= 1 && folder != nil
        d.set(useFolder, forKey: "batch.useFolder")
        if let folder { d.set(folder.path, forKey: "batch.folder") }
        d.set(suffix.stringValue, forKey: "batch.suffix")
        d.set(page.indexOfSelectedItem, forKey: "batch.page")
        return BatchSigner.Settings(options: PDFSigner.Options(identity: nil, reason: "", location: ""),
                                    outputFolder: useFolder ? folder : nil, suffix: suffix.stringValue,
                                    visiblePage: BatchSigner.Page(rawValue: page.indexOfSelectedItem) ?? .first)
    }

    /// Runs the batch in a modal progress window (main thread, so remote-signing prompts can appear).
    static func runWithProgress(_ files: [URL], settings: BatchSigner.Settings) -> [BatchSigner.Outcome] {
        let progress = BatchProgressWindow(total: files.count)
        let session = NSApp.beginModalSession(for: progress.window)
        defer { NSApp.endModalSession(session); progress.window.orderOut(nil) }
        return BatchSigner.run(files, settings: settings) { i, url in
            progress.update(index: i, file: url.lastPathComponent)
            _ = NSApp.runModalSession(session)
            if i == 1, let path = ProcessInfo.processInfo.environment["K42_BATCH_SNAPSHOT"] { _ = progress.snapshot(to: path) }
            return !progress.cancelled
        }
    }

    static func showSummary(_ outcomes: [BatchSigner.Outcome], total: Int) {
        let alert = summaryAlert(outcomes, total: total)
        let outputs = outcomes.compactMap(\.output)
        if alert.runModal() == .alertFirstButtonReturn, !outputs.isEmpty { NSWorkspace.shared.activateFileViewerSelecting(outputs) }
    }

    static func summaryAlert(_ outcomes: [BatchSigner.Outcome], total: Int) -> NSAlert {
        let ok = outcomes.filter(\.succeeded).count
        let alert = NSAlert()
        alert.messageText = ok == total ? L("All %d files are signed", total) : L("Signed %d of %d files", ok, total)
        alert.icon = Theme42.symbol(ok == total ? "checkmark.seal.fill" : "exclamationmark.triangle.fill", size: 40)
        let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 460, height: 180))
        text.string = BatchSigner.summary(outcomes, total: total)
        text.isEditable = false
        text.font = .systemFont(ofSize: 11)
        let scroll = NSScrollView(frame: text.frame)
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        alert.accessoryView = scroll
        alert.addButton(withTitle: L("Show in Finder"))
        alert.addButton(withTitle: L("Done"))
        return alert
    }
}

/// Small window with a progress bar, the current file and Cancel.
final class BatchProgressWindow: NSObject {
    let window: NSPanel
    private let bar = NSProgressIndicator()
    private let label = NSTextField(labelWithString: "")
    private let total: Int
    private(set) var cancelled = false

    init(total: Int) {
        self.total = total
        window = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 420, height: 120), styleMask: [.titled], backing: .buffered, defer: false)
        super.init()
        window.title = L("Signing Files")
        bar.isIndeterminate = false
        bar.minValue = 0
        bar.maxValue = Double(total)
        bar.style = .bar
        label.lineBreakMode = .byTruncatingMiddle
        let cancel = NSButton(title: L("Cancel"), target: self, action: #selector(cancel(_:)))
        cancel.keyEquivalent = "\u{1b}"
        let title = NSTextField(labelWithString: L("Signing %d files…", total))
        title.font = .systemFont(ofSize: 13, weight: .semibold)
        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        let buttons = NSStackView(views: [spacer, cancel])
        let stack = NSStackView(views: [title, bar, label, buttons])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 16, left: 20, bottom: 16, right: 20)
        stack.translatesAutoresizingMaskIntoConstraints = false
        window.contentView = stack
        bar.widthAnchor.constraint(equalToConstant: 380).isActive = true
        label.widthAnchor.constraint(equalToConstant: 380).isActive = true
        buttons.widthAnchor.constraint(equalToConstant: 380).isActive = true
        window.center()
        window.makeKeyAndOrderFront(nil)
    }

    func update(index: Int, file: String) {
        bar.doubleValue = Double(index)
        label.stringValue = L("%d of %d: %@", index + 1, total, file)
        window.displayIfNeeded()
    }

    @objc private func cancel(_ sender: Any?) { cancelled = true }

    /// PNG of the window (UI audit).
    func snapshot(to path: String) -> Bool {
        guard let view = window.contentView?.superview, let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return false }
        view.cacheDisplay(in: view.bounds, to: rep)
        return (try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))) != nil
    }
}
