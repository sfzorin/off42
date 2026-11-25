import Foundation
import Kit42

/// Machine-readable power of attorney (МЧД) in the FNS XML formats: the unified format (root `Доверенность`,
/// `ВерсФорм="EMCHD_1"`) and the older format 002 (root `Файл`, `СвДов`). Only the fields needed to match a
/// signature are read; the XML schema is not validated.
struct PowerOfAttorney {
    var format = ""
    var number = ""
    var issued: Date?
    var expires: Date?
    var principalName: String?
    var principalINN: String?
    var principalOGRN: String?
    var representativeName: String?
    var representativeSNILS: String?
    var representativeINN: String?
    var powers: [String] = []

    /// Parses МЧД XML (any declared encoding, typically UTF-8 or windows-1251). Nil when it is not a power of attorney.
    init?(xml: Data) {
        let reader = Reader()
        let parser = XMLParser(data: xml)
        parser.delegate = reader
        guard parser.parse() || reader.sawRoot, reader.isPowerOfAttorney else { return nil }
        format = reader.format
        number = reader.number ?? ""
        issued = reader.issued.flatMap(Self.date)
        expires = reader.expires.flatMap(Self.date)
        principalName = reader.principalName
        principalINN = reader.principalINN
        principalOGRN = reader.principalOGRN
        representativeName = reader.representativeName
        representativeSNILS = reader.representativeSNILS.map(CertificateInfo.digitsOnly)
        representativeINN = reader.representativeINN
        powers = reader.powers
        guard !number.isEmpty || representativeSNILS != nil || representativeINN != nil else { return nil }
    }

    static func date(_ s: String) -> Date? {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "Europe/Moscow")
        for format in ["yyyy-MM-dd", "dd.MM.yyyy"] {
            f.dateFormat = format
            if let d = f.date(from: String(s.prefix(10))) { return d }
        }
        return nil
    }

    /// The МЧД's own detached signature next to it: `name.xml.sig`, `name.xml.p7s`, `name.sig`, `name.p7s`.
    static func signatureFile(next xml: URL) -> URL? {
        let base = xml.deletingPathExtension()
        let candidates = [xml.appendingPathExtension("sig"), xml.appendingPathExtension("p7s"),
                          base.appendingPathExtension("sig"), base.appendingPathExtension("p7s")]
        return candidates.first { FileManager.default.fileExists(atPath: $0.path) }
    }

    /// Looks like МЧД XML (cheap check before parsing embedded files).
    static func looksLike(_ data: Data) -> Bool {
        let head = data.prefix(4096)
        let text = String(data: head, encoding: .utf8) ?? String(data: head, encoding: .windowsCP1251) ?? ""
        return text.contains("Доверенность") || text.contains("СвДов") || text.contains("EMCHD") || text.contains("ON_DOVEL")
    }

    // MARK: - Checking against a signature

    struct Finding {
        enum Level { case ok, warning, error, info }
        var level: Level
        var text: String
    }

    /// The certificate belongs to the representative (same SNILS, or same personal INN).
    func isRepresentative(_ cert: CertificateInfo?) -> Bool {
        guard let cert else { return false }
        if let s = representativeSNILS, let c = cert.snils { return s == c }
        if let i = representativeINN, let c = cert.inn { return i == c }
        return false
    }

    /// Checks the power of attorney against the signer's certificate and the signing time.
    /// `poaSignature` is the detached signature of the МЧД file itself (by the principal), if available.
    func check(signer: CertificateInfo?, signedAt: Date?, xml: Data, poaSignature: Data?) -> [Finding] {
        var out: [Finding] = []
        let df = DateFormatter()
        df.dateStyle = .medium
        df.timeStyle = .none
        out.append(Finding(level: .info, text: L("Power of attorney № %@ (%@)", number.isEmpty ? "—" : number, format)))
        if let p = principalName ?? principalINN { out.append(Finding(level: .info, text: L("Principal: %@", p + (principalINN.map { principalName != nil ? ", " + L("INN %@", $0) : "" } ?? "")))) }
        if let r = representativeName ?? representativeSNILS ?? representativeINN { out.append(Finding(level: .info, text: L("Representative: %@", r))) }

        // Validity on the signing date (today when the time is unknown); the end date is inclusive.
        let when = signedAt ?? Date()
        if let issued, when < issued {
            out.append(Finding(level: .error, text: L("Signed before the power of attorney was issued (%@)", df.string(from: issued))))
        } else if let expires, when >= expires.addingTimeInterval(86_400) {
            out.append(Finding(level: .error, text: L("The power of attorney expired on %@", df.string(from: expires))))
        } else if issued != nil || expires != nil {
            let range = [issued, expires].map { $0.map(df.string) ?? "…" }.joined(separator: " – ")
            out.append(Finding(level: .ok, text: L("Valid on the signing date (%@)", range)))
        }

        // The signer must be the representative.
        if let signer {
            if let s = representativeSNILS, let c = signer.snils {
                out.append(s == c ? Finding(level: .ok, text: L("The signer is the representative (SNILS %@)", c))
                                  : Finding(level: .error, text: L("The signer's SNILS %@ differs from the representative's %@", c, s)))
            } else if let i = representativeINN, let c = signer.inn {
                out.append(i == c ? Finding(level: .ok, text: L("The signer is the representative (INN %@)", c))
                                  : Finding(level: .error, text: L("The signer's INN %@ differs from the representative's %@", c, i)))
            } else {
                out.append(Finding(level: .warning, text: L("The certificate has no SNILS or INN to compare with the representative")))
            }
            if let org = signer.innLE, let p = principalINN, org != p {
                out.append(Finding(level: .warning, text: L("The certificate names another organisation (INN %@)", org)))
            }
        } else {
            out.append(Finding(level: .warning, text: L("The signer's certificate is not available for comparison")))
        }

        // The power of attorney's own signature (by the principal).
        if let poaSignature {
            if CryptoPro.isInstalled, let v = try? CryptoPro.verifyDetached(xml, signature: poaSignature) {
                switch v.status {
                case .valid: out.append(Finding(level: .ok, text: L("The power of attorney's signature is valid")))
                case .untrusted: out.append(Finding(level: .warning, text: L("The power of attorney's signature is intact, but its certificate is not verified")))
                case .invalid: out.append(Finding(level: .error, text: L("The power of attorney's signature is invalid")))
                }
            } else {
                out.append(Finding(level: .warning, text: L("The power of attorney's signature was not checked: CryptoPro CSP is not installed")))
            }
            if let cms = CMSInfo(poaSignature), let c = cms.signers.first?.certificate, let p = principalINN,
               let org = c.innLE ?? c.inn, org != p {
                out.append(Finding(level: .warning, text: L("The power of attorney is signed by someone else than the principal (INN %@)", org)))
            }
        } else {
            out.append(Finding(level: .warning, text: L("No signature of the power of attorney (.sig) was found")))
        }
        out.append(Finding(level: .info, text: L("Revocation is not checked offline: check the number in the FNS registry (m4d.nalog.gov.ru)")))
        return out
    }

    // MARK: - XML reading

    private final class Reader: NSObject, XMLParserDelegate {
        var stack: [String] = []
        var sawRoot = false, isPowerOfAttorney = false
        var format = "", number: String?, issued: String?, expires: String?
        var principalName: String?, principalINN: String?, principalOGRN: String?
        var representativeName: String?, representativeSNILS: String?, representativeINN: String?
        var powers: [String] = []
        private var text = ""

        static let principalSections: Set<String> = ["Доверит", "СвДоверит"]
        static let representativeSections: Set<String> = ["Пред", "СвУпПред", "СвПред", "Представ", "СвПредст"]

        private var inPrincipal: Bool { stack.contains { Self.principalSections.contains($0) } }
        private var inRepresentative: Bool { stack.contains { Self.representativeSections.contains($0) } }
        /// The principal's own signer (director) is a person inside the principal section, not the representative.
        private var inPrincipalPerson: Bool { stack.contains { ["ЛицоБезДов", "СвЛицБезДов", "ЛицоБезДовер"].contains($0) } }

        func parser(_ parser: XMLParser, didStartElement name: String, namespaceURI: String?, qualifiedName: String?, attributes a: [String: String] = [:]) {
            let local = name.split(separator: ":").last.map(String.init) ?? name
            if stack.isEmpty {
                sawRoot = true
                isPowerOfAttorney = local == "Доверенность" || (local == "Файл" && (a["ИдФайл"] ?? "").contains("DOV"))
                    || (a["ВерсФорм"] ?? "").uppercased().contains("EMCHD")
                format = a["ВерсФорм"] ?? local
            }
            stack.append(local)
            if local == "СвДов" || local == "СвДовер" || local == "Довер" {
                isPowerOfAttorney = true
                number = number ?? a["НомДовер"] ?? a["ЕдНомМЧД"]
                issued = issued ?? a["ДатаВыдДовер"]
                expires = expires ?? a["СрокДейст"] ?? a["ДатаКонДовер"]
            }
            if let n = a["ЕдНомМЧД"], number == nil { number = n }
            if inRepresentative {
                if let v = a["СНИЛС"], representativeSNILS == nil { representativeSNILS = v }
                if let v = a["ИННФЛ"], representativeINN == nil { representativeINN = v }
                if local == "ФИО", representativeName == nil { representativeName = Self.fullName(a) }
            } else if inPrincipal, !inPrincipalPerson {
                if let v = a["НаимОрг"], principalName == nil { principalName = v }
                if let v = a["ИННЮЛ"] ?? a["ИННФЛ"], principalINN == nil { principalINN = v }
                if let v = a["ОГРН"] ?? a["ОГРНИП"], principalOGRN == nil { principalOGRN = v }
                if local == "ФИО", principalName == nil { principalName = Self.fullName(a) }
            }
            if let code = a["КодПолн"] { powers.append([code, a["НаимПолн"]].compactMap { $0 }.joined(separator: " — ")) }
            text = ""
        }

        func parser(_ parser: XMLParser, foundCharacters string: String) { text += string }

        func parser(_ parser: XMLParser, didEndElement name: String, namespaceURI: String?, qualifiedName: String?) {
            let local = stack.last ?? name
            if local == "ТекстПолн" || local == "СвПолн" || local == "НаимПолн" {
                let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
                if !t.isEmpty { powers.append(t) }
            }
            text = ""
            stack.removeLast()
        }

        static func fullName(_ a: [String: String]) -> String? {
            let s = [a["Фамилия"], a["Имя"], a["Отчество"]].compactMap { $0 }.joined(separator: " ")
            return s.isEmpty ? nil : s
        }
    }
}

extension CertificateInfo {
    static func digitsOnly(_ s: String) -> String { s.filter(\.isNumber) }
}
