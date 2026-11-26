import Foundation

/// Turkish electronic signatures (e-imza, Law 5070): recognising qualified electronic certificates (NES) and their
/// certification service providers (ESHS) such as Kamu SM (TÜBİTAK BİLGEM), E-Tuğra and TÜRKTRUST.
/// Trust itself comes from the system trust settings: the providers' root certificates must be installed
/// (Kamu SM and the others publish them); nothing is bundled.
public enum TurkishESign {
    /// BTK certificate policy arc for Law 5070 (qualified electronic certificate policies live under it).
    static let law5070Arc = "2.16.792.1.61.0.1.5070"
    /// Kamu SM's own policy arc.
    static let kamuSMArc = "2.16.792.1.2.1.1.5.7"
    /// ETSI EN 319 412-5 QcCompliance.
    static let qcCompliance = "0.4.0.1862.1.1"

    public struct Provider {
        public let name: String
        let patterns: [String]
    }

    public static let providers: [Provider] = [
        Provider(name: "Kamu SM (TÜBİTAK BİLGEM)", patterns: ["kamu sm", "kamu sertifikasyon", "tübitak", "tubitak"]),
        Provider(name: "E-Tuğra", patterns: ["e-tuğra", "e-tugra", "etugra"]),
        Provider(name: "TÜRKTRUST", patterns: ["türktrust", "turktrust"]),
        Provider(name: "E-İmzaTR", patterns: ["e-imzatr", "e-i̇mzatr", "eimzatr"]),
        Provider(name: "E-Güven", patterns: ["e-güven", "e-guven", "elektronik bilgi güvenliği"]),
    ]

    /// The Turkish provider that issued the certificate (from the issuer name), if any.
    public static func provider(_ der: Data) -> Provider? {
        guard let cert = CertificateDetails(der: der) else { return nil }
        let issuer = cert.issuerAttributes.map(\.1).joined(separator: " ").lowercased()
        return providers.first { p in p.patterns.contains { issuer.contains($0) } }
    }

    /// A qualified electronic certificate under Law 5070 (NES): a 5070/Kamu SM policy, or QcCompliance from a
    /// Turkish provider.
    public static func isQualified(_ der: Data) -> Bool {
        guard let cert = CertificateDetails(der: der) else { return false }
        if cert.policies.contains(where: { $0.hasPrefix(law5070Arc) || $0.hasPrefix(kamuSMArc) }) { return true }
        let turkish = cert.issuerAttributes.contains { $0.0 == "C" && $0.1 == "TR" } || provider(der) != nil
        return turkish && cert.qcStatements.contains(qcCompliance)
    }

    /// Turkish national ID (TC Kimlik No) from the subject serialNumber, masked for display.
    public static func maskedNationalID(_ der: Data) -> String? {
        guard let cert = CertificateDetails(der: der),
              let serial = cert.subjectAttributes.first(where: { $0.0 == "SERIALNUMBER" })?.1 else { return nil }
        let digits = serial.filter(\.isNumber)
        guard digits.count == 11 else { return nil }
        return String(digits.prefix(3)) + "******" + String(digits.suffix(2))
    }

    /// One line for signature reports, nil for certificates without a Turkish connection.
    public static func describe(_ der: Data) -> String? {
        let qualified = isQualified(der)
        let p = provider(der)
        guard qualified || p != nil else { return nil }
        var parts: [String] = []
        parts.append(qualified ? L("Qualified electronic certificate (Law 5070)") : L("Turkish certificate (not qualified)"))
        if let p { parts.append(p.name) }
        if let id = maskedNationalID(der) { parts.append(L("TR ID %@", id)) }
        return parts.joined(separator: " · ")
    }
}
