import AppKit
import Kit42

/// Dialogs for digital signatures: choose certificate, import/create, show verification results.
enum SignDialog {
    /// A certificate offered for signing: from the Keychain, from CryptoPro CSP (GOST) or a remote signing service (CSC).
    enum Choice {
        case keychain(SigningIdentity)
        case gost(CryptoPro.Certificate)
        case remote(CSCSigner)

        var title: String {
            switch self {
            case .keychain(let id): return id.displayName
            case .gost(let c): return L("GOST: %@", c.displayName)
            case .remote(let r): return "☁︎ " + r.credential.label
            }
        }
    }

    static func choices() -> [Choice] {
        let gost = CryptoPro.isInstalled ? ((try? CryptoPro.certificates()) ?? []) : []
        return CertificateStore.identities().map(Choice.keychain) + gost.map(Choice.gost)
    }

    static func run(stampSelected: Bool) -> (options: PDFSigner.Options, visible: Bool, attachments: [EmbeddedFile])? {
        var choices = Self.choices()
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        var signButton: NSButton?
        let pin = NSSecureTextField()
        pin.placeholderString = L("Container PIN (if it has one)")
        func selected() -> Choice? {
            let i = popup.indexOfSelectedItem
            return i >= 0 && i < choices.count ? choices[i] : nil
        }
        func updatePIN() {
            if case .gost? = selected() { pin.isEnabled = true } else { pin.isEnabled = false }
        }
        func fill(select title: String? = nil) {
            popup.removeAllItems()
            popup.addItems(withTitles: choices.map(\.title))
            if choices.isEmpty { popup.addItem(withTitle: L("No certificates")) }
            if let title, let i = choices.firstIndex(where: { $0.title == title }) { popup.selectItem(at: i) }
            else if let last = UserDefaults.standard.string(forKey: "sign.certificate"), let i = choices.firstIndex(where: { $0.title == last }) { popup.selectItem(at: i) }
            popup.isEnabled = !choices.isEmpty
            signButton?.isEnabled = !choices.isEmpty
            updatePIN()
        }
        fill()
        let popupTarget = ClosureTarget { updatePIN() }
        defer { withExtendedLifetime(popupTarget) {} }
        popup.target = popupTarget
        popup.action = #selector(ClosureTarget.fire)

        let reason = NSTextField(string: UserDefaults.standard.string(forKey: "sign.reason") ?? L("I agree with the content of this document"))
        let location = NSTextField(string: UserDefaults.standard.string(forKey: "sign.location") ?? "")
        location.placeholderString = L("City (optional)")

        let actions = ButtonRow()
        actions.add(L("Import .p12…")) {
            if let id = importCertificate() { choices = Self.choices(); fill(select: Choice.keychain(id).title) }
        }
        actions.add(L("Create…")) {
            if let id = createCertificate() { choices = Self.choices(); fill(select: Choice.keychain(id).title) }
        }
        actions.add(L("Remote (CSC)…")) {
            if let signer = connectRemote() { choices.append(.remote(signer)); fill(select: Choice.remote(signer).title) }
        }
        actions.add(L("CryptoPro (GOST)…")) {
            if showCryptoPro() { choices = Self.choices(); fill() }
        }

        // Power of attorney (МЧД) embedded before signing, with its own .sig when it lies next to it.
        var attachments: [EmbeddedFile] = []
        let poaLabel = NSTextField(labelWithString: L("None"))
        poaLabel.lineBreakMode = .byTruncatingMiddle
        let poaRow = ButtonRow()
        poaRow.add(L("Choose МЧД…")) {
            guard let files = choosePowerOfAttorney() else { return }
            attachments = files
            poaLabel.stringValue = files.map(\.name).joined(separator: ", ")
        }
        poaRow.add(L("Remove")) { attachments = []; poaLabel.stringValue = L("None") }
        poaRow.addArrangedSubview(poaLabel)

        let d = UserDefaults.standard
        let visible = NSButton(checkboxWithTitle: stampSelected ? L("Visible signature (in place of the selected image)") : L("Visible signature (bottom right of the current page)"),
                               target: nil, action: nil)
        visible.state = stampSelected || d.bool(forKey: "sign.visible") ? .on : .off
        let timestamp = NSButton(checkboxWithTitle: L("Add a trusted timestamp (needs internet)"), target: nil, action: nil)
        timestamp.state = d.object(forKey: "sign.tsa") == nil || d.bool(forKey: "sign.tsa") ? .on : .off
        let tsa = NSTextField(string: d.string(forKey: "sign.tsaURL") ?? "http://timestamp.digicert.com")
        let format = NSPopUpButton(frame: .zero, pullsDown: false)
        format.addItems(withTitles: PAdESLevel.allCases.map(\.title))
        format.selectItem(at: d.object(forKey: "sign.level") == nil ? PAdESLevel.baselineLTA.rawValue : d.integer(forKey: "sign.level"))
        let alert = NSAlert.form(L("Sign Document"),
                                 info: L("The signature is added to a copy of the document. Certificates come from the macOS Keychain, a remote signing service (CSC) or, for GOST signatures, CryptoPro CSP. Long-term levels (B-T and above) need the timestamp server."),
                                 ok: L("Sign…"),
                                 rows: [(L("Certificate:"), popup), ("", actions), (L("PIN:"), pin), (L("Reason:"), reason), (L("Location:"), location),
                                        (L("Power of attorney:"), poaRow), ("", visible), (L("Format:"), format), ("", timestamp), (L("TSA server:"), tsa)])
        alert.icon = Theme42.symbol("checkmark.seal.fill", size: 40)
        signButton = alert.buttons.first
        signButton?.isEnabled = !choices.isEmpty
        while true {
            guard alert.runModal() == .alertFirstButtonReturn else { return nil }
            guard let choice = selected() else {
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
            d.set(choice.title, forKey: "sign.certificate")
            d.set(format.indexOfSelectedItem, forKey: "sign.level")
            let signer: PDFSigner.Signer
            switch choice {
            case .keychain(let id): signer = .keychain(id)
            case .gost(let c):
                signer = .cryptoPro(c, CryptoPro.SignOptions(thumbprint: c.thumbprint, pin: pin.stringValue.isEmpty ? nil : pin.stringValue))
            case .remote(let r): signer = .remote(r)
            }
            var o = PDFSigner.Options(signer: signer, reason: reason.stringValue, location: location.stringValue)
            // GOST signatures keep CryptoPro's format; the PAdES level applies to Keychain and CSC signers.
            o.level = signer.isGOST ? .legacy : PAdESLevel(rawValue: format.indexOfSelectedItem) ?? .baselineB
            o.timestampURL = timestampURL
            if !signer.isGOST, o.level >= .baselineT, o.timestampURL == nil {
                let ask = NSAlert()
                ask.messageText = L("%@ needs a trusted timestamp", o.level.title)
                ask.informativeText = L("Turn on the timestamp, or the document is signed as PAdES B-B.")
                ask.addButton(withTitle: L("Sign as B-B"))
                ask.addButton(withTitle: L("Cancel"))
                guard ask.runModal() == .alertFirstButtonReturn else { continue }
                o.level = .baselineB
            }
            return (o, visible.state == .on, attachments)
        }
    }

    /// Picks an МЧД XML; its signature (`name.xml.sig`, `name.sig`, `.p7s`) is taken along when present.
    static func choosePowerOfAttorney() -> [EmbeddedFile]? {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.xml]
        panel.message = L("Choose the machine-readable power of attorney (МЧД, XML)")
        guard panel.runModal() == .OK, let url = panel.url, let data = try? Data(contentsOf: url) else { return nil }
        guard PowerOfAttorney(xml: data) != nil else {
            let a = NSAlert()
            a.messageText = L("This XML file is not a machine-readable power of attorney")
            a.runModal()
            return nil
        }
        var files = [EmbeddedFile(name: url.lastPathComponent, data: data, desc: L("Power of attorney (МЧД)"), modified: Date(), mime: "application/xml")]
        if let sig = PowerOfAttorney.signatureFile(next: url), let sd = try? Data(contentsOf: sig) {
            files.append(EmbeddedFile(name: sig.lastPathComponent, data: sd, desc: L("Signature of the power of attorney"), modified: Date(), mime: "application/pkcs7-signature"))
        }
        return files
    }

    /// CryptoPro status: install instructions, or key containers whose certificates can be installed.
    /// Returns true when certificates may have changed.
    @discardableResult
    static func showCryptoPro() -> Bool {
        let alert = NSAlert()
        alert.icon = Theme42.symbol("key.fill", size: 40)
        guard CryptoPro.isInstalled else {
            alert.messageText = L("CryptoPro CSP is not installed")
            alert.informativeText = CryptoPro.installHelp
            alert.addButton(withTitle: L("Open cryptopro.ru"))
            alert.addButton(withTitle: L("Test CA"))
            alert.addButton(withTitle: L("Close"))
            switch alert.runModal() {
            case .alertFirstButtonReturn: NSWorkspace.shared.open(CryptoPro.downloadURL)
            case .alertSecondButtonReturn: NSWorkspace.shared.open(CryptoPro.testCAURL)
            default: break
            }
            return false
        }
        let certs = (try? CryptoPro.certificates()) ?? []
        let containers = (try? CryptoPro.containers()) ?? []
        alert.messageText = L("CryptoPro CSP %@", CryptoPro.versionInfo() ?? "")
        var info = L("Certificates with a key: %d. Key containers: %d.", certs.count, containers.count)
        if let license = CryptoPro.licenseInfo() { info += "\n" + license }
        info += "\n" + L("To use a key container whose certificate is not listed, choose it and install its certificate.")
        alert.informativeText = info
        let popup = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 360, height: 26), pullsDown: false)
        popup.addItems(withTitles: containers.isEmpty ? [L("No key containers (insert the token)")] : containers)
        popup.isEnabled = !containers.isEmpty
        alert.accessoryView = popup
        alert.addButton(withTitle: L("Install Certificate"))
        alert.addButton(withTitle: L("Close"))
        guard alert.runModal() == .alertFirstButtonReturn, !containers.isEmpty else { return false }
        do {
            try CryptoPro.installCertificate(container: containers[popup.indexOfSelectedItem], pin: nil)
            return true
        } catch {
            NSApp.presentError(error)
            return false
        }
    }

    /// Connects to a Cloud Signature Consortium (API v2) service and picks a credential.
    static func connectRemote() -> CSCSigner? {
        let d = UserDefaults.standard
        let url = NSTextField(string: d.string(forKey: "csc.url") ?? "")
        url.placeholderString = "https://provider.example/csc/v2"
        let clientID = NSTextField(string: d.string(forKey: "csc.clientID") ?? "")
        clientID.placeholderString = L("OAuth client ID")
        let secret = NSSecureTextField()
        secret.placeholderString = L("Optional")
        let port = NSTextField(string: d.string(forKey: "csc.port") ?? "8765")
        let token = NSSecureTextField()
        token.placeholderString = L("Optional: an access token instead of OAuth")
        let alert = NSAlert.form(L("Remote Signing (CSC API)"),
                                 info: L("Signs with a certificate held by a trust service provider. Only the document hash is sent; you sign in at the provider in your browser (redirect http://127.0.0.1:PORT/callback)."),
                                 ok: L("Connect"),
                                 rows: [(L("Service URL:"), url), (L("Client ID:"), clientID), (L("Client secret:"), secret),
                                        (L("Redirect port:"), port), (L("Access token:"), token)])
        guard alert.runModal() == .alertFirstButtonReturn, let base = URL(string: url.stringValue.trimmingCharacters(in: .whitespaces)) else { return nil }
        d.set(url.stringValue, forKey: "csc.url")
        d.set(clientID.stringValue, forKey: "csc.clientID")
        d.set(port.stringValue, forKey: "csc.port")
        var config = CSCClient.Config(baseURL: base)
        config.accessToken = token.stringValue.isEmpty ? nil : token.stringValue
        config.clientID = clientID.stringValue.isEmpty ? nil : clientID.stringValue
        config.clientSecret = secret.stringValue.isEmpty ? nil : secret.stringValue
        config.redirectPort = UInt16(port.stringValue) ?? 8765
        let client = CSCClient(config)
        do {
            let credentials = try client.listCredentials().filter { !$0.chain.isEmpty }
            guard !credentials.isEmpty else { throw CSCError.service(L("no credentials with certificates")) }
            var chosen = credentials[0]
            if credentials.count > 1 {
                let list = NSPopUpButton(frame: .zero, pullsDown: false)
                list.addItems(withTitles: credentials.map(\.label))
                let pick = NSAlert.form(L("Choose a Certificate"), ok: L("Choose"), rows: [(L("Certificate:"), list)])
                guard pick.runModal() == .alertFirstButtonReturn else { return nil }
                chosen = credentials[list.indexOfSelectedItem]
            }
            return CSCSigner(client: client, credential: chosen, authData: askAuthData)
        } catch {
            NSApp.presentError(error)
            return nil
        }
    }

    /// PIN and/or one-time password for an explicitly authorized remote credential.
    static func askAuthData(_ credential: CSCClient.Credential) throws -> (pin: String?, otp: String?) {
        let pin = NSSecureTextField(), otp = NSTextField()
        var rows: [(String, NSView)] = []
        if credential.needsPIN || !credential.needsOTP { rows.append((L("PIN:"), pin)) }
        if credential.needsOTP { rows.append((L("One-time code:"), otp)) }
        let alert = NSAlert.form(L("Authorize Signature"), info: credential.label, ok: L("Sign"), rows: rows)
        alert.window.initialFirstResponder = rows[0].1
        guard alert.runModal() == .alertFirstButtonReturn else { throw CSCError.service(L("authorization was cancelled")) }
        return (rows.contains { $0.1 === pin } ? pin.stringValue : nil, credential.needsOTP ? otp.stringValue : nil)
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
        let poaError = s.powerOfAttorney.contains { $0.level == .error }
        let style: BannerView.Style = poaError ? .error : s.status == .valid ? (s.coversWholeFile ? .success : .info)
            : s.status == .untrusted || s.status == .notChecked ? .warning : .error
        let color: NSColor = style == .success ? .systemGreen : style == .info ? .systemBlue : style == .warning ? .systemOrange : .systemRed
        let symbol = style == .success ? "checkmark.seal.fill" : style == .error ? "xmark.seal.fill" : "exclamationmark.triangle.fill"
        let icon = NSImageView(image: Theme42.symbol(symbol, size: 20, weight: .semibold) ?? NSImage())
        icon.contentTintColor = color

        let title = NSTextField(labelWithString: s.email.map { "\(s.signer) · \($0)" } ?? s.signer)
        title.font = .systemFont(ofSize: 13, weight: .semibold)
        var lines = [s.summary]
        if let d = s.date { lines.append(L("Signed: %@", d.formatted(date: .long, time: .shortened))) }
        if let t = s.timestamp { lines.append(L("Trusted timestamp: %@", t.formatted(date: .long, time: .standard))) }
        if let level = s.level { lines.append(L("Format: %@", level)) }
        if let ltv = s.ltvSummary { lines.append(ltv) }
        if let r = s.reason { lines.append(L("Reason: %@", r)) }
        if let l = s.location { lines.append(L("Location: %@", l)) }
        lines += s.details
        if s.status == .untrusted || s.status == .notChecked, let t = s.trustMessage { lines.append(t) }
        if !s.powerOfAttorney.isEmpty {
            lines.append(L("Power of attorney (МЧД):"))
            for f in s.powerOfAttorney {
                let mark = f.level == .ok ? "✓" : f.level == .error ? "✗" : f.level == .warning ? "!" : "·"
                lines.append("\(mark) \(f.text)")
            }
        }
        let body = NSTextField(wrappingLabelWithString: lines.joined(separator: "\n"))
        body.font = .systemFont(ofSize: 12)
        body.textColor = .secondaryLabelColor
        body.preferredMaxLayoutWidth = 360

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
