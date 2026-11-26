import AppKit
import CryptoKit
import Kit42
import Security

// MARK: - Certificate details for appearances and reports

extension X509 {
    /// (attribute OID, value) pairs of a DER Name, in order.
    static func nameAttributes(_ der: [UInt8]) -> [(String, String)] {
        guard let name = DER.parse(der) else { return [] }
        var out: [(String, String)] = []
        for rdn in name.children {
            for atv in rdn.children where atv.children.count == 2 {
                let value = atv.children[1]
                let text: String
                if value.tag == [0x1E] {  // BMPString
                    var units: [UInt16] = []
                    var i = 0
                    while i + 1 < value.content.count { units.append(UInt16(value.content[i]) << 8 | UInt16(value.content[i + 1])); i += 2 }
                    text = String(decoding: units, as: UTF16.self)
                } else {
                    text = String(decoding: value.content, as: UTF8.self)
                }
                out.append((DER.oidString(atv.children[0].content), text))
            }
        }
        return out
    }

    /// "CN, O, C"-style one-line name.
    static func displayName(_ der: [UInt8]) -> String {
        let attrs = nameAttributes(der)
        let order = ["2.5.4.3", "2.5.4.10", "2.5.4.11", "2.5.4.7", "2.5.4.6"]
        let parts = order.flatMap { oid in attrs.filter { $0.0 == oid }.map(\.1) }
        return parts.isEmpty ? attrs.map(\.1).joined(separator: ", ") : parts.joined(separator: ", ")
    }

    static func commonName(_ der: [UInt8]) -> String? { nameAttributes(der).first { $0.0 == "2.5.4.3" }?.1 }

    var subjectName: String { Self.displayName(subject) }
    var issuerName: String { Self.displayName(issuer) }
    var issuerCommonName: String { Self.commonName(issuer) ?? issuerName }

    var notBefore: Date? {
        guard let cert = DER.parse(der), let tbs = cert.children.first else { return nil }
        let i = tbs.children.first?.tag == [0xA0] ? 1 : 0
        guard tbs.children.count > i + 3 else { return nil }
        let validity = tbs.children[i + 3]
        return validity.children.first.flatMap(DER.parseTime)
    }

    var serialHex: String { DER.hex(Array(serial.drop { $0 == 0 })).isEmpty ? "00" : DER.hex(Array(serial.drop { $0 == 0 })) }

    /// SHA-256 fingerprint as "AB:CD:…".
    var fingerprint: String { sha256.map { String(format: "%02X", $0) }.joined(separator: ":") }

    var keyDescription: String {
        switch keyAlgorithm {
        case OID.rsaEncryption:
            let modulus = DER.parse(publicKey)?.children.first?.content.drop { $0 == 0 }.count ?? 0
            return modulus > 0 ? "RSA \(modulus * 8)" : "RSA"
        case "1.2.840.10045.2.1": return "ECDSA"
        case "1.2.643.7.1.1.1.1", "1.2.643.7.1.1.1.2": return "GOST R 34.10-2012"
        default: return keyAlgorithm
        }
    }
}

extension PDFSigner.Options {
    /// The certificate the signature will carry (local identity or remote chain).
    var signerCertificate: X509? {
        if let remote { return remote.chain.first }
        if case .cryptoPro(let cert, _) = signer { return cert.info.flatMap { X509(der: [UInt8]($0.der)) } }
        if case .secureEnclave(let key) = signer { return key.certificate.flatMap { X509(der: [UInt8]($0)) } }
        guard let identity = identity?.identity else { return nil }
        var cert: SecCertificate?
        guard SecIdentityCopyCertificate(identity, &cert) == errSecSuccess, let cert else { return nil }
        return X509(cert)
    }
}

// MARK: - Handwritten signature from the shared library

/// Picks a handwritten signature (generated, learned or drawn) from the Suite42 signature library for the
/// visible part of a digital signature. Generated marks give a new impression for every document.
enum HandwrittenSignature {
    static let defaultsKey = "sign.handwriting"

    struct Choice {
        var url: URL
        var title: String
        var isSignature: Bool
    }

    /// Library items, signatures first. Seals and pictures are offered too (a scanned signature is a picture).
    static func choices(library: StampLibrary = .shared) -> [Choice] {
        let items = library.items.map { url -> Choice in
            let mark = StampLibrary.mark(at: url)
            switch mark?.kind {
            case .signature?: return Choice(url: url, title: mark?.name.map { L("Signature: %@", $0) } ?? L("Signature"), isSignature: true)
            case .learned?: return Choice(url: url, title: mark?.name.map { L("Learned signature: %@", $0) } ?? L("Learned signature"), isSignature: true)
            case .seal?: return Choice(url: url, title: L("Seal: %@", mark?.template ?? ""), isSignature: false)
            default:
                let date = (try? url.resourceValues(forKeys: [.creationDateKey]).creationDate)?.formatted(date: .abbreviated, time: .omitted) ?? ""
                return Choice(url: url, title: L("Picture from %@", date), isSignature: true)
            }
        }
        return items.filter(\.isSignature) + items.filter { !$0.isSignature }
    }

    /// The remembered choice (a library file name), if it still exists.
    static var selected: URL? {
        get {
            guard let name = UserDefaults.standard.string(forKey: defaultsKey), !name.isEmpty else { return nil }
            let url = StampLibrary.shared.folder.appendingPathComponent(name)
            return FileManager.default.fileExists(atPath: url.path) ? url : nil
        }
        set { UserDefaults.standard.set(newValue?.lastPathComponent ?? "", forKey: defaultsKey) }
    }

    /// A fresh impression of a library item (a new copy each call for generated marks).
    static func impression(_ url: URL?) -> CGImage? { url.flatMap(StampLibrary.impression(at:)) }

    /// Popup for the signing dialog: "None" plus library items with small previews.
    static func popup(selecting url: URL?) -> (NSPopUpButton, [Choice]) {
        let choices = choices()
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        popup.addItem(withTitle: L("None (text only)"))
        for c in choices {
            popup.addItem(withTitle: c.title)
            if let image = StampLibrary.image(at: c.url) {
                let thumb = NSImage(cgImage: image, size: NSSize(width: 48 * CGFloat(image.width) / CGFloat(max(1, image.height)) / 2, height: 24))
                popup.lastItem?.image = thumb
            }
        }
        if let url, let i = choices.firstIndex(where: { $0.url == url }) { popup.selectItem(at: i + 1) }
        return (popup, choices)
    }
}

// MARK: - Appearance

/// Visible appearance of a digital signature: a handwritten signature impression on the left and the
/// certificate details (signer, issuer, serial, date, format) on the right, like Acrobat's "name + details".
enum SignatureAppearance {
    struct Details {
        var name: String
        var reason = ""
        var location = ""
        var date = Date()
        var certificate: X509?
        var level: PAdESLevel?
    }

    static func details(for options: PDFSigner.Options, date: Date = Date()) -> Details {
        Details(name: options.signerName, reason: options.reason, location: options.location, date: date,
                certificate: options.signerCertificate, level: options.level)
    }

    /// Text lines under the signer name.
    static func lines(_ d: Details) -> [String] {
        var lines = [L("Date: %@", Self.shortDateFormatter.string(from: d.date))]
        if let c = d.certificate {
            lines.append(L("Issuer: %@", c.issuerCommonName))
            lines.append(L("Serial: %@", c.serialHex.count > 20 ? String(c.serialHex.prefix(20)) + "…" : c.serialHex))
        }
        if let level = d.level, level != .legacy { lines.append(level.cliName == "B-B" ? "PAdES B-B" : "PAdES \(level.cliName)") }
        if !d.reason.isEmpty { lines.append(d.reason) }
        if !d.location.isEmpty { lines.append(d.location) }
        return lines
    }

    static let shortDateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm xxx"
        f.locale = Locale(identifier: "en_US_POSIX")
        return f
    }()

    static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss ZZZZZ"
        f.locale = Locale(identifier: "en_US_POSIX")
        return f
    }()

    /// Renders at 3× for print. `handwriting` is drawn with ink (multiply) over the left part, slightly over the
    /// text column as a pen signature would be; without it only the details are shown.
    /// Nil for a size outside the image-export limits (16,384 px per side, 64,000,000 px in total at 3×).
    static func render(size: CGSize, handwriting: CGImage?, details d: Details) -> CGImage? {
        let scale: CGFloat = 3
        guard size.width.isFinite, size.height.isFinite, size.width >= 1, size.height >= 1,
              size.width * scale <= 16384, size.height * scale <= 16384,
              size.width * size.height * scale * scale <= 64_000_000 else { return nil }
        let w = Int(size.width * scale), h = Int(size.height * scale)
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        ctx.scaleBy(x: scale, y: scale)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: false)
        defer { NSGraphicsContext.restoreGraphicsState() }
        let rect = CGRect(origin: .zero, size: size)
        let blue = NSColor(srgbRed: 0.1, green: 0.25, blue: 0.55, alpha: 1)
        NSColor(srgbRed: 0.1, green: 0.3, blue: 0.7, alpha: 0.8).setStroke()
        let frame = NSBezierPath(roundedRect: rect.insetBy(dx: 0.5, dy: 0.5), xRadius: 4, yRadius: 4)
        frame.lineWidth = 1
        frame.stroke()

        var textRect = rect.insetBy(dx: 6, dy: 4)
        if let image = handwriting {
            // Left half for the pen signature, fitted by aspect ratio.
            let box = CGRect(x: 4, y: 3, width: rect.width * 0.48, height: rect.height - 6)
            let aspect = CGFloat(image.width) / max(1, CGFloat(image.height))
            var fit = CGSize(width: box.width, height: box.width / aspect)
            if fit.height > box.height { fit = CGSize(width: box.height * aspect, height: box.height) }
            let imgRect = CGRect(x: box.minX + (box.width - fit.width) / 2, y: box.minY + (box.height - fit.height) / 2,
                                 width: fit.width, height: fit.height)
            ctx.saveGState()
            ctx.setBlendMode(.multiply)
            ctx.interpolationQuality = .high
            ctx.draw(image, in: imgRect)
            ctx.restoreGState()
            textRect.origin.x = box.maxX + 4
            textRect.size.width = rect.maxX - 6 - textRect.minX
        }
        let lines = Self.lines(d)
        // Font size so that name + lines fit the height.
        let count = CGFloat(lines.count + 2)
        let pointSize = max(4, min(9, (textRect.height) / (count * 1.32)))
        let font = NSFont(name: "Helvetica Neue", size: pointSize) ?? .systemFont(ofSize: pointSize)
        let bold = NSFont(name: "Helvetica Neue Bold", size: pointSize + 1) ?? .boldSystemFont(ofSize: pointSize + 1)
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingTail
        let text = NSMutableAttributedString(string: L("Digitally signed by") + "\n",
                                             attributes: [.font: font, .foregroundColor: blue, .paragraphStyle: paragraph])
        text.append(NSAttributedString(string: d.name + "\n", attributes: [.font: bold, .foregroundColor: blue, .paragraphStyle: paragraph]))
        text.append(NSAttributedString(string: lines.joined(separator: "\n"), attributes: [.font: font, .foregroundColor: blue, .paragraphStyle: paragraph]))
        let bounds = text.boundingRect(with: textRect.size, options: [.usesLineFragmentOrigin])
        let height = min(textRect.height, bounds.height)
        text.draw(with: CGRect(x: textRect.minX, y: textRect.midY - height / 2, width: textRect.width, height: height),
                  options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
        return ctx.makeImage()
    }

    /// Default rectangle for a visible signature: bottom right of the page's crop box.
    static func defaultRect(in box: CGRect, size: CGSize = CGSize(width: 240, height: 80)) -> CGRect {
        CGRect(x: box.maxX - 36 - size.width, y: box.minY + 36, width: size.width, height: size.height)
    }
}
