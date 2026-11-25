import AppKit
import Kit42

/// Dialogs for digital signatures: choose certificate, import/create, show verification results.
enum SignDialog {
    static func run(stampSelected: Bool) -> (options: PDFSigner.Options, visible: Bool)? {
        var identities = CertificateStore.identities()
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        var signButton: NSButton?
        func fill(select name: String? = nil) {
            popup.removeAllItems()
            popup.addItems(withTitles: identities.map(\.displayName))
            if identities.isEmpty { popup.addItem(withTitle: L("No certificates")) }
            if let name, let i = identities.firstIndex(where: { $0.displayName == name }) { popup.selectItem(at: i) }
            popup.isEnabled = !identities.isEmpty
            signButton?.isEnabled = !identities.isEmpty
        }
        fill()

        let reason = NSTextField(string: UserDefaults.standard.string(forKey: "sign.reason") ?? L("I agree with the content of this document"))
        let location = NSTextField(string: UserDefaults.standard.string(forKey: "sign.location") ?? "")
        location.placeholderString = L("City (optional)")

        let actions = ButtonRow()
        actions.add(L("Import .p12…")) {
            if let id = importCertificate() { identities = CertificateStore.identities(); fill(select: id.displayName) }
        }
        actions.add(L("Create…")) {
            if let id = createCertificate() { identities = CertificateStore.identities(); fill(select: id.displayName) }
        }

        let d = UserDefaults.standard
        let visible = NSButton(checkboxWithTitle: stampSelected ? L("Visible signature (in place of the selected image)") : L("Visible signature (bottom right of the current page)"),
                               target: nil, action: nil)
        visible.state = stampSelected || d.bool(forKey: "sign.visible") ? .on : .off
        let timestamp = NSButton(checkboxWithTitle: L("Add a trusted timestamp (needs internet)"), target: nil, action: nil)
        timestamp.state = d.object(forKey: "sign.tsa") == nil || d.bool(forKey: "sign.tsa") ? .on : .off
        let tsa = NSTextField(string: d.string(forKey: "sign.tsaURL") ?? "http://timestamp.digicert.com")
        let alert = NSAlert.form(L("Sign Document"),
                                 info: L("The signature is added to a copy of the document. Certificates come from the macOS Keychain."),
                                 ok: L("Sign…"),
                                 rows: [(L("Certificate:"), popup), ("", actions), (L("Reason:"), reason), (L("Location:"), location),
                                        ("", visible), ("", timestamp), (L("TSA server:"), tsa)])
        alert.icon = Theme42.symbol("checkmark.seal.fill", size: 40)
        signButton = alert.buttons.first
        signButton?.isEnabled = !identities.isEmpty
        while true {
            guard alert.runModal() == .alertFirstButtonReturn else { return nil }
            guard !identities.isEmpty, popup.indexOfSelectedItem >= 0, popup.indexOfSelectedItem < identities.count else {
                NSSound.beep()
                continue
            }
            let timestampURL: URL?
            if timestamp.state == .on {
                let value = tsa.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
                guard let url = URL(string: value), ["http", "https"].contains(url.scheme?.lowercased() ?? ""),
                      url.host?.isEmpty == false else {
                    let error = NSAlert(); error.messageText = L("Enter a valid HTTP or HTTPS timestamp URL.")
                    error.runModal(); continue
                }
                timestampURL = url
            } else { timestampURL = nil }
            d.set(reason.stringValue, forKey: "sign.reason")
            d.set(location.stringValue, forKey: "sign.location")
            d.set(visible.state == .on, forKey: "sign.visible")
            d.set(timestamp.state == .on, forKey: "sign.tsa")
            d.set(tsa.stringValue, forKey: "sign.tsaURL")
            var o = PDFSigner.Options(identity: identities[popup.indexOfSelectedItem], reason: reason.stringValue, location: location.stringValue)
            o.timestampURL = timestampURL
            return (o, visible.state == .on)
        }
    }

    static func importCertificate() -> SigningIdentity? {
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

    static func createCertificate() -> SigningIdentity? {
        let name = NSTextField(string: NSFullUserName())
        let email = NSTextField(string: "")
        email.placeholderString = "name@example.com"
        let org = NSTextField(string: "")
        org.placeholderString = L("Optional")
        let alert = NSAlert.form(L("New Signing Certificate"),
                                 info: L("A self-signed certificate valid for 5 years. Fine for internal documents; a legally binding signature needs a certificate from a certification authority."),
                                 ok: L("Create"), rows: [(L("Name:"), name), ("Email:", email), (L("Organisation:"), org)])
        guard alert.runModal() == .alertFirstButtonReturn, !name.stringValue.isEmpty else { return nil }
        do {
            return try CertificateStore.createSelfSigned(name: name.stringValue, email: email.stringValue, organization: org.stringValue)
        } catch {
            NSApp.presentError(error)
            return nil
        }
    }

    static func showResults(_ signatures: [SignatureInfo], window: NSWindow?) {
        let alert = NSAlert()
        alert.messageText = signatures.isEmpty ? L("This document has no digital signatures") : L("Digital signatures: %d", signatures.count)
        alert.icon = Theme42.symbol(signatures.isEmpty ? "seal" : "checkmark.seal.fill", size: 40)
        if !signatures.isEmpty {
            let stack = NSStackView(views: signatures.enumerated().map { card($0.element, index: $0.offset + 1) })
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 10
            stack.frame.size = stack.fittingSize
            alert.accessoryView = stack
        }
        if let window { alert.beginSheetModal(for: window) } else { alert.runModal() }
    }

    private static func card(_ s: SignatureInfo, index: Int) -> NSView {
        let style: BannerView.Style = s.status == .valid ? (s.coversWholeFile ? .success : .info) : s.status == .untrusted ? .warning : .error
        let color: NSColor = style == .success ? .systemGreen : style == .info ? .systemBlue : style == .warning ? .systemOrange : .systemRed
        let symbol = style == .success ? "checkmark.seal.fill" : style == .error ? "xmark.seal.fill" : "exclamationmark.triangle.fill"
        let icon = NSImageView(image: Theme42.symbol(symbol, size: 20, weight: .semibold) ?? NSImage())
        icon.contentTintColor = color

        let title = NSTextField(labelWithString: s.email.map { "\(s.signer) · \($0)" } ?? s.signer)
        title.font = .systemFont(ofSize: 13, weight: .semibold)
        var lines = [s.summary]
        if let d = s.date { lines.append(L("Signed: %@", d.formatted(date: .long, time: .shortened))) }
        if let t = s.timestamp { lines.append(L("Trusted timestamp: %@", t.formatted(date: .long, time: .standard))) }
        if let r = s.reason { lines.append(L("Reason: %@", r)) }
        if let l = s.location { lines.append(L("Location: %@", l)) }
        if s.status == .untrusted, let t = s.trustMessage { lines.append(t) }
        let body = NSTextField(wrappingLabelWithString: lines.joined(separator: "\n"))
        body.font = .systemFont(ofSize: 12)
        body.textColor = .secondaryLabelColor
        body.preferredMaxLayoutWidth = 300

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

/// Small horizontal row of push buttons with closure actions (used inside alerts).
final class ButtonRow: NSStackView {
    private var handlers: [() -> Void] = []

    func add(_ title: String, _ handler: @escaping () -> Void) {
        let b = NSButton(title: title, target: self, action: #selector(tap(_:)))
        b.bezelStyle = .push
        b.controlSize = .small
        b.tag = handlers.count
        handlers.append(handler)
        addArrangedSubview(b)
    }

    @objc private func tap(_ sender: NSButton) { handlers[sender.tag]() }
}
