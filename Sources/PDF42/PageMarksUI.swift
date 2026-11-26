import AppKit
import Kit42
import PDFKit

/// Pop-up menu of the signature/stamp library. Generated and imported stamps give a new impression on every call.
final class LibraryMarkPopup {
    enum Preference { case seal, signature }

    let popup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let urls: [URL]
    private let allowsNone: Bool
    private let key: String

    /// `key` remembers the choice between dialogs.
    init(prefer: Preference, allowsNone: Bool = false, key: String) {
        urls = StampLibrary.shared.items
        self.allowsNone = allowsNone
        self.key = "pageMarks." + key
        if allowsNone { popup.addItem(withTitle: L("None")) }
        var signatures = 0, stamps = 0, pictures = 0
        for url in urls {
            let mark = StampLibrary.mark(at: url)
            let title: String
            switch mark?.kind {
            case .signature?, .learned?: signatures += 1; title = L("Signature: %@", mark?.name ?? "\(signatures)")
            case .seal?: stamps += 1; title = L("Stamp: %@", mark?.sealSpec?.template.title ?? "\(stamps)")
            case .image?: stamps += 1; title = L("Imported stamp %d", stamps)
            case nil: pictures += 1; title = L("Picture %d", pictures)
            }
            popup.addItem(withTitle: title)
            if let image = NSImage(contentsOf: url) {
                let thumb = NSImage(size: NSSize(width: 44, height: 24), flipped: false) { rect in
                    NSColor.white.setFill()
                    rect.fill()
                    let k = min(rect.width / image.size.width, rect.height / image.size.height)
                    let s = NSSize(width: image.size.width * k, height: image.size.height * k)
                    image.draw(in: NSRect(x: (rect.width - s.width) / 2, y: (rect.height - s.height) / 2, width: s.width, height: s.height))
                    return true
                }
                popup.lastItem?.image = thumb
            }
        }
        let offset = allowsNone ? 1 : 0
        if let saved = UserDefaults.standard.string(forKey: self.key), let i = urls.firstIndex(where: { $0.lastPathComponent == saved }) {
            popup.selectItem(at: i + offset)
        } else if let i = urls.firstIndex(where: { url in
            let kind = StampLibrary.mark(at: url)?.kind
            return prefer == .signature ? kind == .signature || kind == .learned : kind == .seal || kind == .image
        }) {
            popup.selectItem(at: i + offset)
        } else if allowsNone {
            popup.selectItem(at: 0)
        }
    }

    var isEmpty: Bool { urls.isEmpty }

    var selectedURL: URL? {
        let i = popup.indexOfSelectedItem - (allowsNone ? 1 : 0)
        return i >= 0 && i < urls.count ? urls[i] : nil
    }

    func remember() {
        if let url = selectedURL { UserDefaults.standard.set(url.lastPathComponent, forKey: key) }
    }

    /// A new impression of the chosen item.
    func impression() -> CGImage? { selectedURL.flatMap(StampLibrary.impression(at:)) }
}

extension EditorWindowController {
    /// Adds marks as one undoable step and selects a single mark so it can be moved.
    func placeMarks(_ marks: [(PDFPage, PDFAnnotation)], name: String) {
        for (page, mark) in marks {
            mark.userName = NSFullUserName()
            mark.modificationDate = Date()
            add(mark, to: page, name: name)
        }
        if marks.count == 1, let (page, mark) = marks.first {
            setTool(.select)
            pdfView.go(to: page)
            pdfView.selectAnnotation(mark, on: page)
        }
    }

    /// Says the library is empty and offers to open it. Returns true when there is something to choose.
    private func requireLibrary(_ popup: LibraryMarkPopup) -> Bool {
        guard popup.isEmpty else { return true }
        let alert = NSAlert()
        alert.messageText = L("The library of signatures and stamps is empty")
        alert.informativeText = L("Create a signature or a stamp, or add a scan, then run this command again.")
        alert.addButton(withTitle: L("Open Library"))
        alert.addButton(withTitle: L("Cancel"))
        if alert.runModal() == .alertFirstButtonReturn { showStampsAction(nil) }
        return false
    }

    private static func cornerPopup(_ selected: PageMarks.Corner) -> NSPopUpButton {
        let p = NSPopUpButton(frame: .zero, pullsDown: false)
        p.addItems(withTitles: PageMarks.Corner.allCases.map(\.title))
        p.selectItem(at: selected.rawValue)
        return p
    }

    private static func languagePopup() -> NSPopUpButton {
        let p = NSPopUpButton(frame: .zero, pullsDown: false)
        p.addItems(withTitles: PageMarks.MarkLanguage.allCases.map(\.title))
        let saved = UserDefaults.standard.object(forKey: "pageMarks.language") as? Int
        p.selectItem(at: saved ?? PageMarks.MarkLanguage.current.rawValue)
        return p
    }

    private static func millimetres(_ value: Double) -> NSTextField {
        let f = NSTextField(string: String(format: "%.0f", value))
        f.translatesAutoresizingMaskIntoConstraints = false
        f.widthAnchor.constraint(equalToConstant: 60).isActive = true
        return f
    }

    private static func number(_ f: NSTextField, _ fallback: Double) -> Double {
        Double(f.stringValue.replacingOccurrences(of: ",", with: ".").trimmingCharacters(in: .whitespaces)) ?? fallback
    }

    // MARK: - Electronic signature mark

    @objc func eSignatureMarkAction(_ sender: Any?) {
        guard let pdf, pdf.pageCount > 0 else { return }
        let signatures = (pdfDoc?.signatures ?? []).filter { $0.signerCertificate != nil }
        let identities = CertificateStore.identities()
        let source = NSPopUpButton(frame: .zero, pullsDown: false)
        for s in signatures {
            source.addItem(withTitle: L("Signature in the document: %@", s.date.map { "\(s.signer), \($0.formatted(date: .abbreviated, time: .shortened))" } ?? s.signer))
        }
        for id in identities { source.addItem(withTitle: L("Certificate: %@", id.displayName)) }
        guard source.numberOfItems > 0 else {
            let alert = NSAlert()
            alert.messageText = L("No electronic signature to show")
            alert.informativeText = L("The document has no digital signatures and the Keychain has no signing certificates. Sign the document or import a certificate first.")
            alert.runModal()
            return
        }
        let language = Self.languagePopup()
        let corner = Self.cornerPopup(PageMarks.Corner(rawValue: UserDefaults.standard.integer(forKey: "pageMarks.eCorner")) ?? .bottomRight)
        let pages = PageSetChooser()
        let alert = NSAlert.form(L("Electronic Signature Mark"),
                                 info: L("The standard block for a printed copy of an electronically signed document: certificate serial number, owner and validity. It is added as a mark you can move; saving changes to a signed file invalidates its signature, so print it or use “Save As…”."),
                                 ok: L("Add"),
                                 rows: [(L("Signature:"), source), (L("Language:"), language), (L("Position:"), corner), (L("Pages:"), pages.view)])
        alert.icon = Theme42.symbol("checkmark.seal.text.page", size: 40) ?? Theme42.symbol("checkmark.seal", size: 40)
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        guard let targets = pages.indices(current: currentIndex, selected: targetIndices, pageCount: pdf.pageCount) else { NSSound.beep(); return }
        let details: PageMarks.CertificateDetails
        let i = source.indexOfSelectedItem
        if i < signatures.count, let cert = signatures[i].signerCertificate {
            details = PageMarks.CertificateDetails(certificate: cert, signedAt: signatures[i].timestamp ?? signatures[i].date)
        } else if let d = PageMarks.CertificateDetails(identity: identities[i - signatures.count]) {
            details = d
        } else { NSSound.beep(); return }
        let lang = PageMarks.MarkLanguage(rawValue: language.indexOfSelectedItem) ?? .ru
        let place = PageMarks.Corner(rawValue: corner.indexOfSelectedItem) ?? .bottomRight
        UserDefaults.standard.set(lang.rawValue, forKey: "pageMarks.language")
        UserDefaults.standard.set(place.rawValue, forKey: "pageMarks.eCorner")
        let marks: [(PDFPage, PDFAnnotation)] = targets.compactMap { i in
            pdf.page(at: i).map { ($0, PageMarks.eSignatureMark(details, language: lang, corner: place, on: $0)) }
        }
        placeMarks(marks, name: L("Electronic Signature Mark"))
    }

    // MARK: - Cross-page seal

    @objc func crossPageSealAction(_ sender: Any?) {
        guard let pdf, pdf.pageCount > 1 else { NSSound.beep(); return }
        let stamp = LibraryMarkPopup(prefer: .seal, key: "crossSeal")
        guard requireLibrary(stamp) else { return }
        let d = UserDefaults.standard
        let size = Self.millimetres(d.object(forKey: "pageMarks.crossSize") as? Double ?? 40)
        let level = NSPopUpButton(frame: .zero, pullsDown: false)
        level.addItems(withTitles: [L("Top"), L("Middle"), L("Bottom")])
        level.selectItem(at: d.object(forKey: "pageMarks.crossLevel") as? Int ?? 2)
        let pages = PageSetChooser()
        pages.popup.selectItem(at: 1)
        pages.range.isEnabled = false
        let alert = NSAlert.form(L("Cross-Page Seal"),
                                 info: L("One impression of the stamp is cut into strips along the right edges of the pages. Fan the printed pages out, each one a strip further right, and they show the whole seal: a page cannot be replaced unnoticed."),
                                 ok: L("Add"),
                                 rows: [(L("Stamp:"), stamp.popup), (L("Diameter (mm):"), size), (L("Height:"), level), (L("Pages:"), pages.view)])
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        guard let targets = pages.indices(current: currentIndex, selected: targetIndices, pageCount: pdf.pageCount), targets.count > 1 else {
            NSSound.beep()
            return
        }
        stamp.remember()
        let mm = Self.number(size, 40).clamped(10, 120)
        d.set(mm, forKey: "pageMarks.crossSize")
        d.set(level.indexOfSelectedItem, forKey: "pageMarks.crossLevel")
        guard let seal = stamp.impression() else { NSSound.beep(); return }
        let marks = PageMarks.crossPageSeal(seal, pages: targets.compactMap(pdf.page(at:)), size: CGFloat(mm) * PageGeometry.mm,
                                            level: [0.82, 0.5, 0.18][max(0, level.indexOfSelectedItem)])
        placeMarks(marks.map { ($0.0, $0.1 as PDFAnnotation) }, name: L("Cross-Page Seal"))
    }

    // MARK: - Certification note

    @objc func certificationNoteAction(_ sender: Any?) {
        guard let pdf, pdf.pageCount > 0, let last = pdf.page(at: pdf.pageCount - 1) else { return }
        let stamp = LibraryMarkPopup(prefer: .seal, key: "noteSeal")
        guard requireLibrary(stamp) else { return }
        let signature = LibraryMarkPopup(prefer: .signature, allowsNone: true, key: "noteSignature")
        let d = UserDefaults.standard
        let sheets = Self.millimetres(Double(pdf.pageCount))
        let signatory = NSTextField(string: d.string(forKey: "pageMarks.signatory") ?? "")
        signatory.placeholderString = L("Position, initials and surname")
        let language = Self.languagePopup()
        let alert = NSAlert.form(L("Certification Note"),
                                 info: L("“Numbered, bound and sealed: N sheets” at the bottom of the last page, with a stamp over the signature line. For two-sided printing, enter the number of sheets rather than pages."),
                                 ok: L("Add"),
                                 rows: [(L("Sheets:"), sheets), (L("Signed by:"), signatory), (L("Stamp:"), stamp.popup),
                                        (L("Signature:"), signature.popup), (L("Language:"), language)])
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let count = Int(Self.number(sheets, Double(pdf.pageCount)))
        guard count > 0 else { NSSound.beep(); return }
        stamp.remember()
        signature.remember()
        let lang = PageMarks.MarkLanguage(rawValue: language.indexOfSelectedItem) ?? .ru
        d.set(signatory.stringValue, forKey: "pageMarks.signatory")
        d.set(lang.rawValue, forKey: "pageMarks.language")
        let marks = PageMarks.certificationNote(on: last, sheets: count, signatory: signatory.stringValue, date: Date(), language: lang,
                                                seal: stamp.impression(), sealSize: 40 * PageGeometry.mm, signature: signature.impression())
        placeMarks(marks.map { (last, $0) }, name: L("Certification Note"))
        pdfView.go(to: last)
    }

    // MARK: - Initials on every page

    @objc func initialPagesAction(_ sender: Any?) {
        guard let pdf, pdf.pageCount > 0 else { return }
        let mark = LibraryMarkPopup(prefer: .signature, key: "initials")
        guard requireLibrary(mark) else { return }
        let d = UserDefaults.standard
        let corner = Self.cornerPopup(PageMarks.Corner(rawValue: d.integer(forKey: "pageMarks.initialsCorner")) ?? .bottomRight)
        let width = Self.millimetres(d.object(forKey: "pageMarks.initialsWidth") as? Double ?? 25)
        let margin = Self.millimetres(d.object(forKey: "pageMarks.initialsMargin") as? Double ?? 10)
        let pages = PageSetChooser()
        pages.popup.selectItem(at: 1)
        pages.range.isEnabled = false
        let alert = NSAlert.form(L("Initial Every Page"),
                                 info: L("Puts the signature or initials on each page; every copy is a new impression, as when signing by hand."),
                                 ok: L("Add"),
                                 rows: [(L("Signature:"), mark.popup), (L("Position:"), corner), (L("Width (mm):"), width),
                                        (L("Margin (mm):"), margin), (L("Pages:"), pages.view)])
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        guard let targets = pages.indices(current: currentIndex, selected: targetIndices, pageCount: pdf.pageCount) else { NSSound.beep(); return }
        mark.remember()
        let place = PageMarks.Corner(rawValue: corner.indexOfSelectedItem) ?? .bottomRight
        let w = Self.number(width, 25).clamped(5, 120), m = Self.number(margin, 10).clamped(0, 60)
        d.set(place.rawValue, forKey: "pageMarks.initialsCorner")
        d.set(w, forKey: "pageMarks.initialsWidth")
        d.set(m, forKey: "pageMarks.initialsMargin")
        let marks = PageMarks.initials(pages: targets.compactMap(pdf.page(at:)), corner: place, width: CGFloat(w) * PageGeometry.mm,
                                       margin: CGFloat(m) * PageGeometry.mm, impression: mark.impression)
        placeMarks(marks.map { ($0.0, $0.1 as PDFAnnotation) }, name: L("Initial Every Page"))
    }
}

private extension Double {
    func clamped(_ lo: Double, _ hi: Double) -> Double { Swift.min(hi, Swift.max(lo, self)) }
}
