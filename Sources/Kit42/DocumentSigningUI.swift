import AppKit

/// Digital-signature state and commands for an Office/OpenDocument document window (DOC42, XLS42).
/// Signing works on the saved file: the signature is added to the package on disk, and saving the document again
/// writes a new, unsigned package — so the banner warns once the document is edited.
public final class DocumentSignatureState {
    public private(set) var signatures: [DocumentSignature] = []
    public private(set) var banner: BannerView?
    private weak var document: NSDocument?
    private var warnedEdit = false

    public init(document: NSDocument) { self.document = document }

    public static let signableExtensions: Set<String> = ["docx", "docm", "xlsx", "xlsm", "pptx", "odt", "ods", "odp"]

    /// Adds a banner at the top of the window content (call after the content constraints exist).
    public func install(in root: NSView) {
        banner = BannerView.install(in: root)
        refreshBanner()
    }

    /// Reads the signatures of the file the document was opened from (or reverted to).
    public func load(_ url: URL?) {
        warnedEdit = false
        guard let url, Self.signableExtensions.contains(url.pathExtension.lowercased()),
              let data = try? Data(contentsOf: url, options: .mappedIfSafe) else {
            signatures = []
            refreshBanner()
            return
        }
        signatures = DocumentSignatures.verify(data)
        refreshBanner()
    }

    /// The document was written without the signatures.
    public func didSave() {
        if !signatures.isEmpty { load(document?.fileURL) }
    }

    /// First edit of a signed document.
    public func didEdit() {
        guard !warnedEdit, !signatures.isEmpty else { return }
        warnedEdit = true
        banner?.show(.warning, L("The document is signed. Saving your changes removes the signatures; sign it again afterwards."),
                     action: L("Details"), handler: { [weak self] in self?.showDetails() })
    }

    /// Asks before ⌘S overwrites a signed file. False cancels the save.
    public func confirmSave() -> Bool {
        guard !signatures.isEmpty, document?.isDocumentEdited == true else { return true }
        let alert = NSAlert()
        alert.messageText = L("Remove the digital signatures?")
        alert.informativeText = L("This file is signed by %@. Saving the changes writes a new file without the signatures. Use “Save As…” to keep the signed original.",
                                  signerNames)
        alert.addButton(withTitle: L("Save Without Signatures"))
        alert.addButton(withTitle: L("Cancel"))
        alert.alertStyle = .warning
        return alert.runModal() == .alertFirstButtonReturn
    }

    var signerNames: String { Array(Set(signatures.map(\.signer))).sorted().joined(separator: ", ") }

    public func refreshBanner() {
        guard let banner else { return }
        guard !signatures.isEmpty else { banner.hide(); return }
        let names = signerNames
        let details = L("Details")
        let show: () -> Void = { [weak self] in self?.showDetails() }
        if signatures.contains(where: { $0.status == .invalid }) {
            banner.show(.error, L("Signature invalid — the document was changed after signing (%@)", names), action: details, handler: show)
        } else if signatures.contains(where: { $0.status == .untrusted }) {
            banner.show(.warning, L("Signed by %@. The signature is intact, but the certificate is not confirmed by a certification authority", names), action: details, handler: show)
        } else {
            banner.show(.success, L("Signed by %@ · all signatures are valid", names), action: details, handler: show)
        }
    }

    public func showDetails() {
        DocumentSigningUI.showResults(signatures, window: document?.windowControllers.first?.window)
    }

    /// File › Sign Document…: signs the saved file in place (adding to any existing signatures).
    public func sign() {
        guard let document else { return }
        let window = document.windowControllers.first?.window
        guard let url = document.fileURL, Self.signableExtensions.contains(url.pathExtension.lowercased()) else {
            let alert = NSAlert()
            alert.messageText = L("Save the document first")
            alert.informativeText = L("Digital signatures are stored inside Word, Excel and OpenDocument files. Save the document as .docx, .xlsx, .odt or .ods, then sign it.")
            alert.runModal()
            return
        }
        if document.isDocumentEdited {
            let alert = NSAlert()
            alert.messageText = L("Save the changes before signing?")
            alert.informativeText = L("The signature covers the saved file.")
            alert.addButton(withTitle: L("Save and Sign"))
            alert.addButton(withTitle: L("Cancel"))
            guard alert.runModal() == .alertFirstButtonReturn else { return }
            document.save(to: url, ofType: document.fileType ?? "", for: .saveOperation) { [weak self] error in
                if let error { NSApp.presentError(error); return }
                self?.signSaved(url, window: window)
            }
            return
        }
        signSaved(url, window: window)
    }

    private func signSaved(_ url: URL, window: NSWindow?) {
        guard let (signer, comment) = DocumentSigningUI.chooseSigner() else { return }
        do {
            let data = try Data(contentsOf: url)
            let signed = try DocumentSignatures.sign(data, signer: signer, comment: comment)
            try signed.write(to: url, options: .atomic)
            document?.fileModificationDate = (try? FileManager.default.attributesOfItem(atPath: url.path))?[.modificationDate] as? Date
            load(url)
        } catch {
            if let window { NSApp.presentError(error, modalFor: window, delegate: nil, didPresent: nil, contextInfo: nil) } else { NSApp.presentError(error) }
        }
    }
}

extension DocumentSignatureState {
    /// UI regression for the signed-document lifecycle on the open window, from a signed file:
    /// banner shown and laid out above the content → an edit turns it into the removal warning → Save As writes an
    /// unsigned file and hides the banner → signing that file (test .p12 from K42_P12/K42_P12_PASSWORD) shows it again.
    /// The modal dialogs (certificate choice, PIN, save confirmation) are not driven.
    public func audit(edit: () -> Void) -> Bool {
        var passed = 0, total = 0
        func check(_ ok: Bool, _ name: String) {
            total += 1
            if ok { passed += 1 }
            print("\(ok ? "PASS" : "FAIL") signature UI: \(name)")
        }
        guard let document, let banner, let root = banner.superview else {
            print("FAIL signature UI: no document window")
            return false
        }
        func settle() {
            root.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        }
        settle()
        check(!signatures.isEmpty, "signatures read from the opened file (\(signatures.count))")
        check(!banner.isHidden && banner.frame.height >= Theme42.bannerHeight - 0.5, "banner visible with its full height")
        check(banner.currentStyle == .warning || banner.currentStyle == .success, "banner style matches an intact signature")
        let below = banner.anchoredBelow.filter { !$0.isHidden && $0.frame.height > 0 }
        check(!below.isEmpty && below.allSatisfy { $0.frame.maxY <= banner.frame.minY + 0.5 },
              "content re-anchored below the banner (\(below.count) views)")
        edit()
        settle()
        check(document.isDocumentEdited, "edit marks the document changed")
        check(banner.currentStyle == .warning && banner.text == L("The document is signed. Saving your changes removes the signatures; sign it again afterwards."),
              "edit turns the banner into the removal warning")

        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("k42-signature-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let copy = dir.appendingPathComponent("unsigned." + (document.fileURL?.pathExtension ?? "docx"))
        var finished = false
        var saveError: Error?
        document.save(to: copy, ofType: document.fileType ?? "", for: .saveAsOperation) { finished = true; saveError = $0 }
        let deadline = Date().addingTimeInterval(10)
        while !finished, Date() < deadline { RunLoop.main.run(until: Date().addingTimeInterval(0.05)) }
        settle()
        check(finished && saveError == nil, "Save As completes" + (saveError.map { ": \($0.localizedDescription)" } ?? ""))
        let written = (try? Data(contentsOf: copy)) ?? Data()
        check(!written.isEmpty && !DocumentSignatures.isSigned(written), "the saved file carries no signature")
        check(signatures.isEmpty && banner.isHidden && banner.frame.height < 0.5, "banner hidden and collapsed after saving")

        let env = ProcessInfo.processInfo.environment
        if let p12 = env["K42_P12"], let data = FileManager.default.contents(atPath: p12) {
            do {
                let id = try CertificateStore.importPKCS12(data, password: env["K42_P12_PASSWORD"] ?? "", toKeychain: false)
                let signed = try DocumentSignatures.sign(written, signer: try IdentitySigner(id.identity), comment: "UI audit")
                try signed.write(to: copy, options: .atomic)
                load(copy)
                settle()
                check(signatures.count == 1 && signatures[0].status != .invalid, "re-signed file verifies")
                check(!banner.isHidden && banner.frame.height >= Theme42.bannerHeight - 0.5, "banner shown again for the re-signed file")
            } catch {
                check(false, "re-sign with K42_P12: \(error.localizedDescription)")
            }
        }
        try? FileManager.default.removeItem(at: dir)
        print("Signature UI regressions: \(passed)/\(total) passed")
        return passed == total
    }
}

/// Dialogs: choose a certificate (Keychain, .p12 or smart card), PIN entry, verification results.
public enum DocumentSigningUI {
    enum Choice {
        case identity(SigningIdentity)
        case token(PKCS11Token, PKCS11Certificate)
    }

    /// Certificate, PIN and purpose; nil when cancelled.
    public static func chooseSigner() -> (XMLRawSigner, String)? {
        var choices: [Choice] = CertificateStore.identities().map { .identity($0) }
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        func title(_ c: Choice) -> String {
            switch c {
            case .identity(let id): return id.displayName
            case .token(_, let cert):
                let qualified = TurkishESign.isQualified(cert.der) ? " · " + L("qualified") : ""
                return "▣ " + cert.displayName + qualified
            }
        }
        func fill(select index: Int? = nil) {
            popup.removeAllItems()
            popup.addItems(withTitles: choices.map(title))
            if choices.isEmpty { popup.addItem(withTitle: L("No certificates")) }
            if let index, index < choices.count { popup.selectItem(at: index) }
            popup.isEnabled = !choices.isEmpty
        }
        fill()
        let purpose = NSTextField(string: UserDefaults.standard.string(forKey: "docsign.purpose") ?? "")
        purpose.placeholderString = L("Optional, e.g. Approved")
        let actions = NSStackView()
        func button(_ title: String, _ action: @escaping () -> Void) {
            let b = ClosureButton(title: title, action: action)
            b.bezelStyle = .push
            b.controlSize = .small
            actions.addArrangedSubview(b)
        }
        button(L("Import .p12…")) {
            if let id = importPKCS12() { choices.insert(.identity(id), at: 0); fill(select: 0) }
        }
        button(L("Smart Card…")) {
            let found = readSmartCards()
            guard !found.isEmpty else { return }
            choices.removeAll { if case .token = $0 { return true } else { return false } }
            // Qualified (non-repudiation) certificates first: the e-imza certificate rather than the authentication one.
            let sorted = found.sorted { a, b in a.1.keyUsage.nonRepudiation && !b.1.keyUsage.nonRepudiation }
            choices.append(contentsOf: sorted.map { .token($0.0, $0.1) })
            fill(select: choices.count - sorted.count)
        }
        let alert = NSAlert.form(L("Sign Document"),
                                 info: L("The signature is added to the saved file (XAdES). Word, Excel and LibreOffice show it; editing the document afterwards removes it. Certificates come from the Keychain, a .p12 file or a smart card."),
                                 ok: L("Sign…"), rows: [(L("Certificate:"), popup), ("", actions), (L("Purpose:"), purpose)])
        alert.icon = Theme42.symbol("checkmark.seal.fill", size: 40)
        while true {
            guard alert.runModal() == .alertFirstButtonReturn else { return nil }
            let i = popup.indexOfSelectedItem
            guard i >= 0, i < choices.count else { NSSound.beep(); continue }
            UserDefaults.standard.set(purpose.stringValue, forKey: "docsign.purpose")
            do {
                switch choices[i] {
                case .identity(let id):
                    return (try IdentitySigner(id.identity), purpose.stringValue)
                case .token(let token, let cert):
                    guard let pin = askPIN(token: token, certificate: cert) else { continue }
                    return (PKCS11Signer(token: token, certificate: cert, pin: pin), purpose.stringValue)
                }
            } catch {
                NSApp.presentError(error)
            }
        }
    }

    /// Certificates on cards in all installed PKCS#11 modules; errors are shown.
    public static func readSmartCards() -> [(PKCS11Token, PKCS11Certificate)] {
        #if APP_STORE
        // No P11Bridge42 helper in the sandboxed build: it would load the vendor's unsigned module.
        AppStoreEdition.showSmartCardModulesUnavailable()
        return []
        #else
        var modules = PKCS11Modules.installed()
        if modules.isEmpty {
            let alert = NSAlert()
            alert.messageText = L("No smart-card software found")
            alert.informativeText = L("Install the middleware of your card (for Turkish e-imza cards: AKİS from TÜBİTAK BİLGEM or the software of your certificate provider), or choose its PKCS#11 module (.dylib).")
            alert.addButton(withTitle: L("Choose Module…"))
            alert.addButton(withTitle: L("Cancel"))
            guard alert.runModal() == .alertFirstButtonReturn, let path = chooseModule() else { return [] }
            modules = [((path as NSString).lastPathComponent, path)]
        }
        var found: [(PKCS11Token, PKCS11Certificate)] = []
        var errors: [String] = []
        for (name, path) in modules {
            let token = PKCS11Token(module: path)
            do {
                let certs = try token.certificates()
                if certs.isEmpty {
                    let present = token.info.slots.contains { $0.token != nil }
                    errors.append("\(name): " + (present ? L("no certificates on the card") : L("no card in the reader")))
                }
                found += certs.map { (token, $0) }
            } catch {
                errors.append("\(name): \(error.localizedDescription)")
            }
        }
        if found.isEmpty {
            let alert = NSAlert()
            alert.messageText = L("No certificate found on a smart card")
            alert.informativeText = errors.joined(separator: "\n\n")
            alert.addButton(withTitle: L("OK"))
            alert.addButton(withTitle: L("Choose Module…"))
            if alert.runModal() == .alertSecondButtonReturn, let path = chooseModule() {
                UserDefaults.standard.set(path, forKey: "pkcs11.module")
                return readSmartCards()
            }
        }
        return found
        #endif
    }

    static func chooseModule() -> String? {
        let panel = NSOpenPanel()
        panel.message = L("Choose the PKCS#11 module of your card software (for AKİS: libakisp11.dylib)")
        panel.directoryURL = URL(fileURLWithPath: "/usr/local/lib")
        panel.allowedContentTypes = [.init(filenameExtension: "dylib")!, .init(filenameExtension: "so")!, .init(filenameExtension: "bundle")!]
        guard panel.runModal() == .OK, let path = panel.url?.path else { return nil }
        UserDefaults.standard.set(path, forKey: "pkcs11.module")
        return path
    }

    /// PIN for a card; nil when cancelled. A PIN-pad reader returns "" and the pad asks.
    public static func askPIN(token: PKCS11Token, certificate: PKCS11Certificate) -> String?? {
        if token.hasPinPad(slot: certificate.slot) {
            let alert = NSAlert()
            alert.messageText = L("Enter the PIN on the card reader")
            alert.informativeText = certificate.displayName
            alert.addButton(withTitle: L("Continue"))
            alert.addButton(withTitle: L("Cancel"))
            return alert.runModal() == .alertFirstButtonReturn ? .some(nil) : nil
        }
        let field = NSSecureTextField()
        var info = certificate.displayName
        if let warning = token.pinWarning(slot: certificate.slot) { info += "\n\n⚠︎ " + warning }
        let alert = NSAlert.form(L("Card PIN"), info: info, ok: L("Sign"), rows: [(L("PIN:"), field)])
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        return .some(field.stringValue)
    }

    static func importPKCS12() -> SigningIdentity? {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.init(filenameExtension: "p12")!, .init(filenameExtension: "pfx")!]
        panel.message = L("Choose a certificate file with a private key (.p12 / .pfx)")
        guard panel.runModal() == .OK, let url = panel.url, let data = try? Data(contentsOf: url) else { return nil }
        let field = NSSecureTextField()
        let alert = NSAlert.form(L("Certificate Password"), info: url.lastPathComponent, ok: L("Import"), rows: [(L("Password:"), field)])
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        do {
            return try CertificateStore.importPKCS12(data, password: field.stringValue, toKeychain: true)
        } catch {
            NSApp.presentError(error)
            return nil
        }
    }

    public static func showResults(_ signatures: [DocumentSignature], window: NSWindow?) {
        let alert = NSAlert()
        alert.messageText = signatures.isEmpty ? L("This document has no digital signatures") : L("Digital signatures: %d", signatures.count)
        alert.icon = Theme42.symbol(signatures.isEmpty ? "seal" : "checkmark.seal.fill", size: 40)
        if !signatures.isEmpty {
            let stack = NSStackView(views: signatures.map(card))
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 10
            stack.frame.size = stack.fittingSize
            alert.accessoryView = stack
        }
        if let window { alert.beginSheetModal(for: window) } else { alert.runModal() }
    }

    static func card(_ s: DocumentSignature) -> NSView {
        let style: BannerView.Style = s.status == .valid ? .success : s.status == .untrusted ? .warning : .error
        let icon = NSImageView(image: Theme42.symbol(style == .success ? "checkmark.seal.fill" : style == .error ? "xmark.seal.fill" : "exclamationmark.triangle.fill",
                                                     size: 20, weight: .semibold) ?? NSImage())
        icon.contentTintColor = style == .success ? .systemGreen : style == .warning ? .systemOrange : .systemRed
        let title = NSTextField(labelWithString: s.email.map { "\(s.signer) · \($0)" } ?? s.signer)
        title.font = .systemFont(ofSize: 13, weight: .semibold)
        var lines = [s.summary]
        if let d = s.date { lines.append(L("Signed: %@", d.formatted(date: .long, time: .shortened))) }
        lines.append(L("Format: %@", s.format))
        if let c = s.comment { lines.append(L("Purpose: %@", c)) }
        if let cert = s.certificate, let tr = TurkishESign.describe(cert) { lines.append(tr) }
        if s.status == .untrusted, let t = s.trustMessage { lines.append(t) }
        if !s.unsignedParts.isEmpty { lines.append(L("Not covered: %@", s.unsignedParts.prefix(5).joined(separator: ", "))) }
        let body = NSTextField(wrappingLabelWithString: lines.joined(separator: "\n"))
        body.font = .systemFont(ofSize: 12)
        body.textColor = .secondaryLabelColor
        body.preferredMaxLayoutWidth = 320
        let text = NSStackView(views: [title, body])
        text.orientation = .vertical
        text.alignment = .leading
        text.spacing = 3
        let row = NSStackView(views: [icon, text])
        row.alignment = .top
        row.spacing = 10
        return row
    }
}

/// Push button with a closure (alerts' accessory rows).
final class ClosureButton: NSButton {
    private var handler: (() -> Void)?
    convenience init(title: String, action: @escaping () -> Void) {
        self.init(frame: .zero)
        self.title = title
        handler = action
        target = self
        self.action = #selector(fire)
    }
    @objc private func fire() { handler?() }
}

public extension BannerView {
    /// Adds a banner at the top of `root`, moving everything that hung from the top of the safe area below it.
    /// The banner takes no space while hidden.
    static func install(in root: NSView) -> BannerView {
        let banner = BannerView()
        banner.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(banner)
        let guide = root.safeAreaLayoutGuide
        let anchored = root.constraints.filter {
            $0.isActive && $0.firstAttribute == .top && $0.secondAttribute == .top && ($0.secondItem as AnyObject?) === guide
        }
        for c in anchored {
            guard let item = c.firstItem else { continue }
            if let view = item as? NSView { banner.anchoredBelow.append(view) }
            c.isActive = false
            NSLayoutConstraint(item: item, attribute: .top, relatedBy: c.relation, toItem: banner, attribute: .bottom,
                               multiplier: 1, constant: c.constant).isActive = true
        }
        NSLayoutConstraint.activate([
            banner.topAnchor.constraint(equalTo: guide.topAnchor),
            banner.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            banner.trailingAnchor.constraint(equalTo: root.trailingAnchor),
        ])
        return banner
    }
}
