import AppKit
import Kit42
import UniformTypeIdentifiers

/// "Get a Certificate…": 1) create a signing key (Secure Enclave or Keychain), 2) make a certificate request (CSR)
/// for a certification authority, 3) import the issued certificate; plus guides for Russia, Türkiye, the EU and
/// free options. Suite42 cannot issue trusted certificates itself: certification authorities check identity first.
final class CertificateWizard: NSWindowController, NSTableViewDataSource, NSTableViewDelegate {
    static var shared: CertificateWizard?

    static func show(tab: Int? = nil) {
        if shared == nil { shared = CertificateWizard() }
        shared?.reload()
        if let tab, let tabs = shared?.window?.contentView as? NSTabView, tab < tabs.numberOfTabViewItems { tabs.selectTabViewItem(at: tab) }
        shared?.showWindow(nil)
        shared?.window?.makeKeyAndOrderFront(nil)
    }

    private var keys: [StoredKey] = []
    private let table = NSTableView()
    private let requestButton = NSButton(), saveButton = NSButton(), importButton = NSButton(), deleteButton = NSButton()

    init() {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 760, height: 560),
                              styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        window.title = L("Get a Certificate")
        window.minSize = NSSize(width: 640, height: 460)
        window.setFrameAutosaveName("CertificateWizard")
        super.init(window: window)
        window.isReleasedWhenClosed = false

        let tabs = NSTabView()
        tabs.addTabViewItem(item(L("Your Keys"), keysPage()))
        for guide in Guide.all { tabs.addTabViewItem(item(guide.title, guidePage(guide))) }
        window.contentView = tabs
        window.center()
    }

    required init?(coder: NSCoder) { fatalError() }

    private func item(_ label: String, _ view: NSView) -> NSTabViewItem {
        let item = NSTabViewItem()
        item.label = label
        item.view = view
        return item
    }

    // MARK: Keys page

    private func keysPage() -> NSView {
        let intro = NSTextField(wrappingLabelWithString: L("A trusted certificate is issued by a certification authority after it checks your identity. Create a key here, send the authority a request, and import the certificate it issues. The private key never leaves this Mac."))
        let steps = NSTextField(wrappingLabelWithString: [
            L("1. New Key: in the Secure Enclave (cannot be copied; Touch ID or your password for each signature) or in the Keychain (can be exported, accepted by more authorities)."),
            L("2. Create Request: your details go into a PKCS#10 request (.csr). Upload it to the authority or send it as they ask."),
            L("3. Import Certificate: the issued .cer, .crt, .pem or .p7b file is paired with its key. Then choose it in Sign Document."),
        ].joined(separator: "\n"))
        steps.textColor = .secondaryLabelColor
        for label in [intro, steps] { label.preferredMaxLayoutWidth = 700 }

        for (id, title) in [("key", L("Key")), ("storage", L("Stored in")), ("status", L("Status"))] {
            let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(id))
            column.title = title
            column.width = id == "status" ? 330 : id == "key" ? 200 : 130
            table.addTableColumn(column)
        }
        table.usesAlternatingRowBackgroundColors = true
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(importAction)
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder

        let newButton = NSButton(title: L("New Key…"), target: self, action: #selector(newKeyAction))
        func setup(_ b: NSButton, _ title: String, _ action: Selector) {
            b.title = title
            b.bezelStyle = .push
            b.target = self
            b.action = action
        }
        setup(requestButton, L("Create Request…"), #selector(requestAction))
        setup(saveButton, L("Save Request…"), #selector(saveRequestAction))
        setup(importButton, L("Import Certificate…"), #selector(importAction))
        setup(deleteButton, L("Delete Key…"), #selector(deleteAction))
        let buttons = NSStackView(views: [newButton, requestButton, saveButton, importButton, NSView(), deleteButton])
        buttons.distribution = .fill

        let stack = NSStackView(views: [intro, steps, scroll, buttons])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.edgeInsets = NSEdgeInsets(top: 16, left: 16, bottom: 16, right: 16)
        for v in [scroll, buttons] { v.widthAnchor.constraint(equalTo: stack.widthAnchor, constant: -32).isActive = true }
        scroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 160).isActive = true
        return stack
    }

    func reload() {
        let selected = selectedKey?.id
        keys = KeyVault.all()
        table.reloadData()
        if let selected, let row = keys.firstIndex(where: { $0.id == selected }) {
            table.selectRowIndexes([row], byExtendingSelection: false)
        } else if !keys.isEmpty, table.selectedRow < 0 {
            table.selectRowIndexes([keys.count - 1], byExtendingSelection: false)
        }
        updateButtons()
    }

    private var selectedKey: StoredKey? { table.selectedRow >= 0 && table.selectedRow < keys.count ? keys[table.selectedRow] : nil }

    private func updateButtons() {
        let key = selectedKey
        requestButton.isEnabled = key != nil
        saveButton.isEnabled = key?.request != nil
        deleteButton.isEnabled = key != nil
    }

    func numberOfRows(in tableView: NSTableView) -> Int { keys.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let key = keys[row]
        let text: String
        switch tableColumn?.identifier.rawValue {
        case "key": text = key.status == .ready ? key.displayName : key.label
        case "storage":
            text = key.storage == .secureEnclave ? (key.userPresence ? L("Secure Enclave, Touch ID") : L("Secure Enclave")) : "\(L("Keychain")), \(key.algorithm.title)"
        default:
            switch key.status {
            case .needsRequest: text = L("Next: create a request")
            case .waitingForCertificate: text = L("Request created · waiting for the certificate")
            case .ready:
                let expiry = key.certificateExpiry.map { $0.formatted(date: .abbreviated, time: .omitted) } ?? "?"
                text = L("Ready for signing · valid until %@", expiry)
            }
        }
        let field = NSTextField(labelWithString: text)
        field.lineBreakMode = .byTruncatingTail
        field.toolTip = text
        return field
    }

    func tableViewSelectionDidChange(_ notification: Notification) { updateButtons() }

    // MARK: Actions

    @objc private func newKeyAction() {
        let label = NSTextField(string: NSFullUserName())
        let storage = NSPopUpButton(frame: .zero, pullsDown: false)
        let enclave = KeyVault.secureEnclaveAvailable
        if enclave { storage.addItem(withTitle: L("Secure Enclave (recommended)")) }
        storage.addItems(withTitles: KeyAlgorithm.allCases.map { "\(L("Keychain")), \($0.title)" })
        let presence = NSButton(checkboxWithTitle: L("Ask for Touch ID or the password at each signature"), target: nil, action: nil)
        presence.state = .on
        let alert = NSAlert.form(L("New Signing Key"),
                                 info: L("A Secure Enclave key is P-256 (ECDSA) and cannot be copied to another Mac or backed up: if the Mac is lost, ask for a new certificate. Some authorities only accept RSA keys; choose a Keychain RSA key for them."),
                                 ok: L("Create"), rows: [(L("Name:"), label), (L("Store in:"), storage), ("", presence)])
        alert.icon = Theme42.symbol("key.fill", size: 40)
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let index = storage.indexOfSelectedItem
        do {
            let name = label.stringValue.trimmingCharacters(in: .whitespaces).isEmpty ? L("Signing key") : label.stringValue
            var key = enclave && index == 0
                ? try KeyVault.create(label: name, storage: .secureEnclave, userPresence: presence.state == .on)
                : try KeyVault.create(label: name, storage: .keychain, algorithm: KeyAlgorithm.allCases[index - (enclave ? 1 : 0)])
            reload()
            if let row = keys.firstIndex(where: { $0.id == key.id }) { table.selectRowIndexes([row], byExtendingSelection: false) }
            createRequest(for: &key)
        } catch {
            showError(error)
        }
    }

    @objc private func requestAction() {
        guard var key = selectedKey else { return }
        createRequest(for: &key)
    }

    private func createRequest(for key: inout StoredKey) {
        let s = key.subject ?? defaultSubject(key)
        func field(_ value: String, _ placeholder: String? = nil) -> NSTextField {
            let f = NSTextField(string: value)
            f.placeholderString = placeholder
            return f
        }
        let name = field(s.commonName), given = field(s.givenName, L("Optional")), surname = field(s.surname, L("Optional"))
        let email = field(s.email, "name@example.com"), org = field(s.organization, L("Optional"))
        let unit = field(s.organizationalUnit, L("Optional")), city = field(s.locality, L("Optional"))
        let region = field(s.state, L("Optional")), country = field(s.country, "TR, RU, DE…")
        let serial = field(s.serialNumber, L("Optional, as the authority asks (e.g. PNOTR-…)"))
        let alert = NSAlert.form(L("Certificate Request"),
                                 info: L("Enter your details exactly as in your identity document; the authority checks them. It may also ask for the request in its own form, then use its instructions."),
                                 ok: L("Create Request…"),
                                 rows: [(L("Full name:"), name), (L("Given name:"), given), (L("Surname:"), surname), ("Email:", email),
                                        (L("Organisation:"), org), (L("Department:"), unit), (L("City:"), city), (L("Region:"), region),
                                        (L("Country code:"), country), (L("ID number:"), serial)])
        alert.icon = Theme42.symbol("doc.badge.plus", size: 40)
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        var subject = CertificateSubject()
        subject.commonName = name.stringValue
        subject.givenName = given.stringValue
        subject.surname = surname.stringValue
        subject.email = email.stringValue
        subject.organization = org.stringValue
        subject.organizationalUnit = unit.stringValue
        subject.locality = city.stringValue
        subject.state = region.stringValue
        subject.country = String(country.stringValue.trimmingCharacters(in: .whitespaces).prefix(2))
        subject.serialNumber = serial.stringValue
        do {
            try KeyVault.certificationRequest(for: &key, subject: subject)
            reload()
            saveRequest(key)
        } catch {
            showError(error)
        }
    }

    private func defaultSubject(_ key: StoredKey) -> CertificateSubject {
        var s = CertificateSubject()
        s.commonName = key.label
        s.country = Locale.current.region?.identifier ?? ""
        return s
    }

    @objc private func saveRequestAction() {
        if let key = selectedKey { saveRequest(key) }
    }

    private func saveRequest(_ key: StoredKey) {
        guard let request = key.request, let window else { return }
        let pem = KeyVault.pem(request)
        let panel = NSSavePanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "csr") ?? .data]
        panel.nameFieldStringValue = "\(key.subject?.commonName ?? key.label).csr".replacingOccurrences(of: "/", with: "-")
        panel.message = L("Save the certificate request and send it to the certification authority")
        panel.beginSheetModal(for: window) { response in
            guard response == .OK, let url = panel.url else { return }
            do {
                try pem.write(to: url, atomically: true, encoding: .utf8)
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(pem, forType: .string)
                let done = NSAlert()
                done.messageText = L("Request saved")
                done.informativeText = L("“%@” is saved and also copied to the clipboard (some authorities ask you to paste it). When the certificate is issued, click Import Certificate.", url.lastPathComponent)
                done.beginSheetModal(for: window)
            } catch {
                self.showError(error)
            }
        }
    }

    @objc private func importAction() {
        guard let window else { return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = ["cer", "crt", "pem", "der", "p7b", "p7c"].compactMap { UTType(filenameExtension: $0) }
        panel.message = L("Choose the certificate issued for your request (.cer, .crt, .pem, .p7b)")
        let target = selectedKey
        panel.beginSheetModal(for: window) { response in
            guard response == .OK, let url = panel.url, let data = try? Data(contentsOf: url) else { return }
            do {
                // The certificate finds its key by the public key; the selection is only a hint.
                let key = try (try? KeyVault.importCertificate(data, into: target)) ?? KeyVault.importCertificate(data)
                self.reload()
                let done = NSAlert()
                done.messageText = L("Certificate added")
                done.informativeText = L("“%@” is paired with its key and can be chosen in Sign Document.", key.displayName)
                done.beginSheetModal(for: window)
            } catch {
                self.showError(error)
            }
        }
    }

    @objc private func deleteAction() {
        guard let key = selectedKey, let window else { return }
        let ask = NSAlert()
        ask.messageText = L("Delete “%@”?", key.label)
        ask.informativeText = key.status == .ready
            ? L("You will no longer be able to sign with this certificate. Existing signatures stay valid. This cannot be undone.")
            : L("A certificate issued later for this key's request cannot be used. This cannot be undone.")
        ask.addButton(withTitle: L("Delete"))
        ask.addButton(withTitle: L("Cancel"))
        ask.buttons[0].hasDestructiveAction = true
        ask.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            do { try KeyVault.delete(key) } catch { self.showError(error) }
            self.reload()
        }
    }

    private func showError(_ error: Error) {
        if let window { NSAlert(error: error).beginSheetModal(for: window) } else { NSApp.presentError(error) }
    }

    // MARK: Guides

    private func guidePage(_ guide: Guide) -> NSView {
        let text = NSTextView()
        text.isEditable = false
        text.drawsBackground = false
        text.textContainerInset = NSSize(width: 18, height: 16)
        text.isVerticallyResizable = true
        text.autoresizingMask = [.width]
        text.textContainer?.widthTracksTextView = true
        text.textStorage?.setAttributedString(guide.render())
        let scroll = NSScrollView()
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        return scroll
    }
}

/// Guidance on where to get a signing certificate. Facts checked in November 2026; the links are the official
/// registries, which stay current when providers change.
struct Guide {
    enum Block {
        case heading(String)
        case text(String)
        case step(String)
        case note(String)
        case link(String, String)
    }

    let title: String
    let blocks: [Block]

    func render() -> NSAttributedString {
        let out = NSMutableAttributedString()
        let body = NSFont.systemFont(ofSize: 13)
        func paragraph(spacing: CGFloat = 8, indent: CGFloat = 0) -> NSParagraphStyle {
            let p = NSMutableParagraphStyle()
            p.paragraphSpacing = spacing
            p.headIndent = indent
            p.firstLineHeadIndent = indent > 0 ? indent - 16 : 0
            return p
        }
        var number = 0
        for block in blocks {
            switch block {
            case .heading(let s):
                number = 0
                out.append(NSAttributedString(string: s + "\n", attributes: [
                    .font: NSFont.systemFont(ofSize: 15, weight: .semibold), .foregroundColor: NSColor.labelColor,
                    .paragraphStyle: paragraph(spacing: 6)]))
            case .text(let s):
                out.append(NSAttributedString(string: s + "\n", attributes: [.font: body, .foregroundColor: NSColor.labelColor,
                                                                             .paragraphStyle: paragraph()]))
            case .step(let s):
                number += 1
                out.append(NSAttributedString(string: "\(number).\t" + s + "\n", attributes: [
                    .font: body, .foregroundColor: NSColor.labelColor, .paragraphStyle: paragraph(spacing: 6, indent: 22)]))
            case .note(let s):
                out.append(NSAttributedString(string: s + "\n", attributes: [.font: NSFont.systemFont(ofSize: 12),
                                                                             .foregroundColor: NSColor.secondaryLabelColor,
                                                                             .paragraphStyle: paragraph(spacing: 12)]))
            case .link(let title, let url):
                out.append(NSAttributedString(string: "→ " + title + "\n", attributes: [
                    .font: body, .link: URL(string: url) as Any, .paragraphStyle: paragraph(spacing: 6)]))
            }
        }
        return out
    }

    static let all: [Guide] = [russia, turkey, europe, free]

    static let russia = Guide(title: L("Russia"), blocks: [
        .heading(L("Qualified electronic signature (КЭП)")),
        .text(L("Under Federal Law 63-FZ a qualified signature uses GOST keys on a certified token, and the certificate is issued only after identification in person. Requests made in this window (ECDSA/RSA) are not accepted for КЭП: the key is generated on the token by the authority or by CryptoPro CSP.")),
        .heading(L("Company directors and sole proprietors")),
        .step(L("Get a certified USB token (for example Rutoken ЭЦП 2.0/3.0 or JaCarta-2 SE).")),
        .step(L("Come to the Federal Tax Service (ФНС) certification centre or one of its authorised agents with your passport, SNILS and INN. The certificate is free.")),
        .step(L("Install CryptoPro CSP and the token driver on the Mac, then sign through them.")),
        .heading(L("Employees and private persons")),
        .step(L("Choose a commercial certification authority accredited by the Ministry of Digital Development (list below).")),
        .step(L("Apply on its website, then visit its office for identification. Employees sign documents for their company with a personal КЭП plus a machine-readable power of attorney (МЧД).")),
        .note(L("Without a qualified certificate, a key made here with a self-signed or corporate certificate gives a simple or unqualified signature, valid when the parties agree to it in writing.")),
        .link(L("Accredited certification authorities (Минцифры)"), "https://e-trust.gosuslugi.ru"),
        .link(L("Federal Tax Service (ФНС)"), "https://www.nalog.gov.ru"),
    ])

    static let turkey = Guide(title: L("Türkiye"), blocks: [
        .heading(L("Qualified electronic certificate (NES, Law 5070)")),
        .text(L("A secure electronic signature (güvenli elektronik imza) has the force of a handwritten signature. It needs a qualified certificate (nitelikli elektronik sertifika) from a provider authorised by BTK; the key is on a smart card or USB token.")),
        .step(L("Choose a provider: Kamu SM (TÜBİTAK, mainly for public employees through their institution), E-Tuğra, TÜRKTRUST or another provider on the BTK list.")),
        .step(L("Apply online with your T.C. identity number (foreign residents: their foreigner identity number).")),
        .step(L("Your identity is checked in person, by a courier at delivery, or at a provider office; the card and its PIN are delivered separately.")),
        .step(L("Install the card reader and card driver from the provider. If macOS then shows the certificate in the Keychain, choose it in Sign Document.")),
        .note(L("Mobile signature (mobil imza) from GSM operators works in operator and e-Devlet flows, not for signing files on the Mac. Providers generate the NES key on the card, so a request made here is used for non-qualified certificates only.")),
        .link(L("Authorised providers (BTK)"), "https://www.btk.gov.tr"),
        .link("Kamu SM", "https://kamusm.bilgem.tubitak.gov.tr"),
    ])

    static let europe = Guide(title: L("European Union"), blocks: [
        .heading(L("Qualified electronic signature (eIDAS)")),
        .text(L("A qualified electronic signature (QES) equals a handwritten signature in every EU country. It needs a qualified certificate from a qualified trust service provider (QTSP) and a qualified signature creation device (QSCD). The Secure Enclave is not a certified QSCD, so a key made here gives an advanced signature (AdES), not QES.")),
        .step(L("Find a QTSP for your country in the EU Trusted List (below): look for “qualified certificate for electronic signature”.")),
        .step(L("Verify your identity: in person, by video identification, or with your national eID card.")),
        .step(L("Sign either with a smart card or token from the provider, or remotely: many QTSPs offer the Cloud Signature Consortium API, which PDF42 supports in Sign Document › Remote (CSC).")),
        .step(L("For advanced signatures, ask the provider for a document-signing certificate for your own request (.csr) made in this window.")),
        .note(L("National eID cards of several countries (for example Estonia, Belgium, Portugal) already contain a signing certificate. Certificates from Adobe AATL members are trusted by Acrobat without extra set-up.")),
        .link(L("EU Trusted List browser"), "https://eidas.ec.europa.eu/efda/tl-browser/"),
        .link(L("Adobe Approved Trust List (AATL)"), "https://helpx.adobe.com/acrobat/kb/approved-trust-list1.html"),
    ])

    static let free = Guide(title: L("Free Options"), blocks: [
        .heading(L("Self-signed certificate")),
        .text(L("Sign Document › Create… makes a certificate instantly. The signature protects the document from changes, but other apps show the signer as not verified until the reader trusts your certificate. Good for internal documents and your own records.")),
        .heading(L("Free e-mail (S/MIME) certificates")),
        .text(L("Some public certification authorities issue free S/MIME certificates checked only by a code sent to your e-mail address. They can sign PDFs and usually chain to roots that macOS trusts (Acrobat only if the root is on its own list), but they confirm only the e-mail address, not your name.")),
        .step(L("If the provider accepts your own request, create a key and request here and import the certificate.")),
        .step(L("If the provider creates the key for you, download the .p12 file and import it with Sign Document › Import .p12….")),
        .heading(L("Your organisation's certification authority")),
        .text(L("Companies with their own authority (for example Active Directory Certificate Services) can issue a certificate for a request made here; their colleagues already trust it.")),
        .note(L("Check the provider's terms: free certificates are usually valid for one year and may be limited to personal use.")),
    ])
}
