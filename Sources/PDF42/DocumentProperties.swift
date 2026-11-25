import AppKit
import Kit42
import PDFKit

/// Fonts used by the pages of a file (page resources, forms inside them, annotation appearances).
enum FontScanner {
    struct Font: Hashable {
        var name: String
        var type: String
        var embedded: Bool
        var subset: Bool
    }

    /// Fonts per page index.
    static func scan(_ data: Data) -> [Int: Set<Font>] {
        guard let provider = CGDataProvider(data: data as CFData), let cg = CGPDFDocument(provider) else { return [:] }
        var out: [Int: Set<Font>] = [:]
        for i in 0..<cg.numberOfPages {
            guard let page = cg.page(at: i + 1)?.dictionary else { continue }
            var fonts = Set<Font>()
            var visited = Set<Int>()
            if let res = CGPDF.inherited(page, "Resources") { resources(res, into: &fonts, visited: &visited, depth: 0) }
            if let annots = CGPDF.array(page, "Annots") {
                for k in 0..<CGPDFArrayGetCount(annots) {
                    var a: CGPDFDictionaryRef?
                    guard CGPDFArrayGetDictionary(annots, k, &a), let ap = CGPDF.dict(a, "AP") else { continue }
                    appearance(ap, into: &fonts, visited: &visited)
                }
            }
            out[i] = fonts
        }
        return out
    }

    private static func appearance(_ ap: CGPDFDictionaryRef, into fonts: inout Set<Font>, visited: inout Set<Int>) {
        for key in ["N", "R", "D"] {
            if let s = CGPDF.stream(ap, key) { form(s, into: &fonts, visited: &visited, depth: 0) }
            else if let states = CGPDF.dict(ap, key) {
                CGPDFDictionaryApplyBlock(states, { _, obj, _ in
                    var s: CGPDFStreamRef?
                    if CGPDFObjectGetValue(obj, .stream, &s), let s { form(s, into: &fonts, visited: &visited, depth: 0) }
                    return true
                }, nil)
            }
        }
    }

    private static func form(_ s: CGPDFStreamRef, into fonts: inout Set<Font>, visited: inout Set<Int>, depth: Int) {
        guard depth < 16, visited.insert(unsafeBitCast(s, to: Int.self)).inserted, let dict = CGPDFStreamGetDictionary(s),
              let res = CGPDF.dict(dict, "Resources") else { return }
        resources(res, into: &fonts, visited: &visited, depth: depth + 1)
    }

    private static func resources(_ res: CGPDFDictionaryRef, into fonts: inout Set<Font>, visited: inout Set<Int>, depth: Int) {
        guard depth < 16 else { return }
        if let fd = CGPDF.dict(res, "Font") {
            CGPDFDictionaryApplyBlock(fd, { _, obj, _ in
                var f: CGPDFDictionaryRef?
                if CGPDFObjectGetValue(obj, .dictionary, &f), let f { fonts.insert(describe(f)) }
                return true
            }, nil)
        }
        if let xo = CGPDF.dict(res, "XObject") {
            CGPDFDictionaryApplyBlock(xo, { _, obj, _ in
                var s: CGPDFStreamRef?
                if CGPDFObjectGetValue(obj, .stream, &s), let s, CGPDF.name(CGPDFStreamGetDictionary(s), "Subtype") == "Form" {
                    form(s, into: &fonts, visited: &visited, depth: depth + 1)
                }
                return true
            }, nil)
        }
    }

    static func describe(_ f: CGPDFDictionaryRef) -> Font {
        let subtype = CGPDF.name(f, "Subtype") ?? "?"
        var name = CGPDF.name(f, "BaseFont") ?? (subtype == "Type3" ? "Type 3" : "?")
        var descriptor = CGPDF.dict(f, "FontDescriptor")
        var type = subtype
        if subtype == "Type0", let kids = CGPDF.array(f, "DescendantFonts") {
            var d: CGPDFDictionaryRef?
            if CGPDFArrayGetDictionary(kids, 0, &d), let d {
                descriptor = CGPDF.dict(d, "FontDescriptor")
                type = "Type0/" + (CGPDF.name(d, "Subtype") ?? "?")
            }
        }
        let embedded = subtype == "Type3" || ["FontFile", "FontFile2", "FontFile3"].contains { CGPDF.stream(descriptor, $0) != nil }
        let subset = name.count > 7 && name.dropFirst(6).first == "+" && name.prefix(6).allSatisfy { $0.isUppercase }
        name = AcroFormFixer.unescapeName(name)
        return Font(name: name, type: type, embedded: embedded, subset: subset)
    }
}

extension EditorWindowController {
    static func sameColor(_ a: NSColor, _ b: NSColor) -> Bool {
        guard let x = a.usingColorSpace(.sRGB), let y = b.usingColorSpace(.sRGB) else { return false }
        return abs(x.redComponent - y.redComponent) < 0.02 && abs(x.greenComponent - y.greenComponent) < 0.02 && abs(x.blueComponent - y.blueComponent) < 0.02
    }

    /// Page layout and page mode of the file, as far as the editor can show them.
    func applyInitialView() {
        guard let view = pdfDoc?.extras.view, let pdf else { return }
        switch view.pageLayout {
        case "SinglePage": pdfView.displayMode = .singlePage
        case "OneColumn": pdfView.displayMode = .singlePageContinuous
        case "TwoColumnLeft", "TwoColumnRight": pdfView.displayMode = .twoUpContinuous
        case "TwoPageLeft", "TwoPageRight": pdfView.displayMode = .twoUp
        default: break
        }
        pdfView.displaysAsBook = view.pageLayout.hasSuffix("Right")
        if view.pageLayout.hasPrefix("Two") { zoomMode = .width }
        switch view.pageMode {
        case "UseOutlines" where (pdf.outlineRoot?.numberOfChildren ?? 0) > 0: showSidebarMode(.outline)
        case "UseThumbs": showSidebarMode(.thumbnails)
        case "UseAttachments": showSidebarMode(.attachments)
        default: break
        }
        if view.openPage >= 0, let page = pdf.page(at: view.openPage) { pdfView.go(to: page) }
        switch view.openZoom {
        case "Fit": zoomMode = .page
        case "FitH": zoomMode = .width
        case "FitV":
            if let page = pdfView.currentPage {
                let height = Self.displayedSize(page).height
                if height.isFinite, height > 0 {
                    zoomMode = .custom; pdfView.autoScales = false
                    pdfView.scaleFactor = min(pdfView.maxScaleFactor, max(pdfView.minScaleFactor, (pdfView.bounds.height - 32) / height))
                }
            }
        default:
            if let percentage = Double(view.openZoom), percentage.isFinite, percentage > 0 {
                zoomMode = .custom; pdfView.autoScales = false
                pdfView.scaleFactor = min(pdfView.maxScaleFactor, max(pdfView.minScaleFactor, percentage / 100))
            }
        }
    }

    // MARK: - Properties

    @objc func propertiesAction(_ sender: Any?) {
        guard let pdf, let doc = pdfDoc else { return }
        let x = doc.extras
        func field(_ s: String, editable: Bool = true) -> NSTextField {
            let f = editable ? NSTextField(string: s) : NSTextField(labelWithString: s)
            f.translatesAutoresizingMaskIntoConstraints = false
            f.widthAnchor.constraint(equalToConstant: 330).isActive = true
            f.lineBreakMode = .byTruncatingMiddle
            if !editable { f.isSelectable = true; f.textColor = .secondaryLabelColor }
            return f
        }
        let date = { (d: Date?) in d.map { $0.formatted(date: .abbreviated, time: .standard) } ?? "—" }
        let title = field(x.info.title), author = field(x.info.author), subject = field(x.info.subject), keywords = field(x.info.keywords)
        let page = pdfView.currentPage ?? pdf.page(at: 0)
        var sizeText = "—"
        if let page {
            let s = EditorWindowController.displayedSize(page)
            sizeText = String(format: "%.0f × %.0f mm", s.width / PageGeometry.mm, s.height / PageGeometry.mm)
            if let name = PageGeometry.paperName(s) { sizeText += " (\(name))" }
        }
        let url = doc.fileURL
        let bytes = (try? url?.resourceValues(forKeys: [.fileSizeKey]).fileSize).flatMap { $0 }
        let fileSize = bytes.map { ByteCountFormatter.string(fromByteCount: Int64($0), countStyle: .file) + " (\($0.formatted()) " + L("bytes") + ")" } ?? "—"
        let description = Self.grid([
            (L("Title:"), title), (L("Author:"), author), (L("Subject:"), subject), (L("Keywords:"), keywords),
            (L("Application:"), field(x.info.creator.isEmpty ? "—" : x.info.creator, editable: false)),
            (L("PDF producer:"), field(x.info.producer.isEmpty ? "—" : x.info.producer, editable: false)),
            (L("Created:"), field(date(x.info.created), editable: false)), (L("Modified:"), field(date(x.info.modified), editable: false)),
            (L("File:"), field(url?.path ?? L("Not saved"), editable: false)), (L("File size:"), field(fileSize, editable: false)),
            (L("Pages:"), field("\(pdf.pageCount)", editable: false)), (L("Page size:"), field(sizeText, editable: false)),
            (L("PDF version:"), field(x.version.isEmpty ? "\(pdf.majorVersion).\(pdf.minorVersion)" : x.version, editable: false)),
            (L("Metadata:"), field(x.hadXMP ? L("XMP and Info dictionary") : L("Info dictionary"), editable: false)),
        ])

        // Initial view.
        let layout = NSPopUpButton(frame: .zero, pullsDown: false)
        layout.addItems(withTitles: [L("Default"), L("Single page"), L("Single page, continuous"), L("Two pages, continuous"),
                                     L("Two pages, continuous, cover"), L("Two pages"), L("Two pages, cover")])
        layout.selectItem(at: InitialView.layouts.firstIndex(of: x.view.pageLayout) ?? 0)
        let mode = NSPopUpButton(frame: .zero, pullsDown: false)
        mode.addItems(withTitles: [L("Default"), L("Page only"), L("Bookmarks panel and page"), L("Thumbnails panel and page"),
                                   L("Attachments panel and page"), L("Full screen")])
        mode.selectItem(at: InitialView.modes.firstIndex(of: x.view.pageMode) ?? 0)
        let openPage = NSTextField(string: x.view.openPage > 0 ? x.extrasLabel(x.view.openPage) : "")
        openPage.placeholderString = "1"
        let zoom = NSPopUpButton(frame: .zero, pullsDown: false)
        let zooms = [("", L("Default")), ("Fit", L("Whole page")), ("FitH", L("Page width")), ("FitV", L("Page height")),
                     ("50", "50 %"), ("75", "75 %"), ("100", "100 %"), ("125", "125 %"), ("150", "150 %"), ("200", "200 %")]
        zoom.addItems(withTitles: zooms.map(\.1))
        zoom.selectItem(at: zooms.firstIndex { $0.0 == x.view.openZoom } ?? 0)
        let fit = NSButton(checkboxWithTitle: L("Resize the window to the first page"), target: nil, action: nil)
        let center = NSButton(checkboxWithTitle: L("Centre the window on the screen"), target: nil, action: nil)
        let showTitle = NSButton(checkboxWithTitle: L("Show the document title instead of the file name"), target: nil, action: nil)
        let hideToolbar = NSButton(checkboxWithTitle: L("Hide the toolbar"), target: nil, action: nil)
        let hideMenubar = NSButton(checkboxWithTitle: L("Hide the menu bar"), target: nil, action: nil)
        for (b, on) in [(fit, x.view.fitWindow), (center, x.view.centerWindow), (showTitle, x.view.displayDocTitle),
                        (hideToolbar, x.view.hideToolbar), (hideMenubar, x.view.hideMenubar)] { b.state = on ? .on : .off }
        let initial = Self.grid([
            (L("Page layout:"), layout), (L("Show:"), mode), (L("Open at page:"), openPage), (L("Magnification:"), zoom),
            (L("Window:"), fit), ("", center), ("", showTitle), (L("Viewer:"), hideToolbar), ("", hideMenubar),
        ])

        // Fonts.
        let data = doc.originalData ?? pdf.dataRepresentation() ?? Data()
        var all: [FontScanner.Font: [Int]] = [:]
        for (i, set) in FontScanner.scan(data) { for f in set { all[f, default: []].append(i) } }
        let fontLines = all.keys.sorted { $0.name < $1.name }.map { f -> String in
            let state = f.embedded ? (f.subset ? L("embedded subset") : L("embedded")) : L("not embedded")
            let pages = all[f]!.sorted()
            return "\(f.name) — \(f.type), \(state), " + L("pages: %@", PDFTools.describeRanges(pages))
        }
        let fonts = Self.textList(fontLines.isEmpty ? [L("No fonts (the pages contain no text, or only images)")] : fontLines)

        // Security.
        var security: [String] = []
        security.append(pdf.isEncrypted ? L("Encrypted: yes") : L("Encrypted: no"))
        if pdf.isEncrypted {
            security.append(pdf.allowsPrinting ? L("Printing: allowed") : L("Printing: not allowed"))
            security.append(pdf.allowsCopying ? L("Copying text and images: allowed") : L("Copying text and images: not allowed"))
            security.append(pdf.allowsDocumentChanges ? L("Changing the document: allowed") : L("Changing the document: not allowed"))
            security.append(pdf.allowsCommenting ? L("Commenting: allowed") : L("Commenting: not allowed"))
            security.append(pdf.allowsFormFieldEntry ? L("Filling in forms: allowed") : L("Filling in forms: not allowed"))
        }
        let signatures = doc.signatures
        security.append(signatures.isEmpty ? L("Digital signatures: none") : L("Digital signatures: %d (%@)", signatures.count, signatures.map(\.signer).joined(separator: ", ")))
        security.append(L("Attachments: %d", x.files.count + x.annotationFiles.count))
        let securityView = Self.textList(security)

        let tabs = NSTabView(frame: NSRect(x: 0, y: 0, width: 520, height: 410))
        tabs.tabViewType = .noTabsBezelBorder
        let titles = [L("Description"), L("Initial View"), L("Fonts"), L("Security")]
        let switcher = NSSegmentedControl(labels: titles, trackingMode: .selectOne, target: nil, action: nil)
        let switchTarget = ClosureTarget { tabs.selectTabViewItem(at: max(0, switcher.selectedSegment)) }
        switcher.target = switchTarget
        switcher.action = #selector(ClosureTarget.fire)
        switcher.selectedSegment = 0
        for (label, content) in [(L("Description"), description), (L("Initial View"), initial), (L("Fonts"), fonts), (L("Security"), securityView)] {
            let item = NSTabViewItem(identifier: label)
            item.label = label
            let holder = NSView()
            content.translatesAutoresizingMaskIntoConstraints = false
            holder.addSubview(content)
            NSLayoutConstraint.activate([
                content.topAnchor.constraint(equalTo: holder.topAnchor, constant: 12),
                content.leadingAnchor.constraint(equalTo: holder.leadingAnchor, constant: 12),
                content.trailingAnchor.constraint(lessThanOrEqualTo: holder.trailingAnchor, constant: -12),
                content.bottomAnchor.constraint(lessThanOrEqualTo: holder.bottomAnchor, constant: -12),
            ])
            if content is NSScrollView {
                content.trailingAnchor.constraint(equalTo: holder.trailingAnchor, constant: -12).isActive = true
                content.bottomAnchor.constraint(equalTo: holder.bottomAnchor, constant: -12).isActive = true
            }
            item.view = holder
            tabs.addTabViewItem(item)
        }
        if let tab = ProcessInfo.processInfo.environment["K42_PROPERTIES_TAB"].flatMap(Int.init) { switcher.selectedSegment = tab; tabs.selectTabViewItem(at: tab) }
        let accessory = NSView(frame: NSRect(x: 0, y: 0, width: 520, height: 446))
        switcher.frame = NSRect(x: 0, y: 418, width: 520, height: 24)
        switcher.segmentDistribution = .fillEqually
        accessory.addSubview(switcher)
        accessory.addSubview(tabs)

        let alert = NSAlert()
        alert.messageText = L("Document Properties")
        alert.informativeText = doc.displayName ?? ""
        alert.addButton(withTitle: L("OK"))
        alert.addButton(withTitle: L("Cancel"))
        alert.accessoryView = accessory
        alert.layout()
        alert.window.initialFirstResponder = title
        let response = alert.runModal()
        _ = switchTarget
        guard response == .alertFirstButtonReturn else { return }

        var info = x.info
        info.title = title.stringValue
        info.author = author.stringValue
        info.subject = subject.stringValue
        info.keywords = keywords.stringValue
        var view = x.view
        view.pageLayout = InitialView.layouts[max(0, layout.indexOfSelectedItem)]
        view.pageMode = InitialView.modes[max(0, mode.indexOfSelectedItem)]
        let requestedPage = openPage.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard requestedPage.isEmpty || PageLabels.index(of: requestedPage, x.labels, pageCount: pdf.pageCount) != nil else { NSSound.beep(); return }
        view.openPage = PageLabels.index(of: requestedPage, x.labels, pageCount: pdf.pageCount) ?? 0
        view.openZoom = zooms[max(0, zoom.indexOfSelectedItem)].0
        view.fitWindow = fit.state == .on
        view.centerWindow = center.state == .on
        view.displayDocTitle = showTitle.state == .on
        view.hideToolbar = hideToolbar.state == .on
        view.hideMenubar = hideMenubar.state == .on
        if info != x.info || view != x.view {
            setExtras({ e in
                if e.info != info { e.info = info; e.dirty.insert(.info) }
                if e.view != view { e.view = view; e.dirty.insert(.view) }
            }, part: info != x.info ? .info : .view, name: L("Document Properties"))
        }
    }

    private static func grid(_ rows: [(String, NSView)]) -> NSGridView {
        let grid = NSGridView(views: rows.map { [NSTextField(labelWithString: $0.0), $0.1] })
        grid.rowSpacing = 6
        grid.column(at: 0).xPlacement = .trailing
        for (_, v) in rows where v is NSPopUpButton {
            v.translatesAutoresizingMaskIntoConstraints = false
            v.widthAnchor.constraint(equalToConstant: 260).isActive = true
        }
        return grid
    }

    private static func textList(_ lines: [String]) -> NSScrollView {
        let scroll = NSTextView.scrollableTextView()
        let text = scroll.documentView as! NSTextView
        text.isEditable = false
        text.drawsBackground = false
        text.font = .systemFont(ofSize: 12)
        text.textColor = .labelColor
        text.string = lines.joined(separator: "\n")
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        return scroll
    }
}

extension DocumentExtras {
    fileprivate func extrasLabel(_ i: Int) -> String { label(i) ?? "\(i + 1)" }
}

extension PDFTools {
    /// [0, 1, 2, 5] → "1–3, 6".
    static func describeRanges(_ indices: [Int]) -> String {
        var parts: [String] = []
        var i = 0
        let s = indices.sorted()
        while i < s.count {
            var j = i
            while j + 1 < s.count, s[j + 1] == s[j] + 1 { j += 1 }
            parts.append(j > i ? "\(s[i] + 1)–\(s[j] + 1)" : "\(s[i] + 1)")
            i = j + 1
        }
        return parts.joined(separator: ", ")
    }
}
