import CryptoKit
import Foundation
import Kit42
import Security

/// Validation data for long-term validation (PAdES B-LT): certificate chains, OCSP responses and CRLs,
/// fetched from the URLs in the certificates (AIA caIssuers / OCSP, CRL distribution points).
enum Revocation {
    struct ValidationData {
        var certificates: [X509] = []
        var ocsps: [[UInt8]] = []
        var crls: [[UInt8]] = []
        var warnings: [String] = []

        mutating func add(_ cert: X509) { if !certificates.contains(cert) { certificates.append(cert) } }
        mutating func merge(_ other: ValidationData) {
            other.certificates.forEach { add($0) }
            for o in other.ocsps where !ocsps.contains(o) { ocsps.append(o) }
            for c in other.crls where !crls.contains(c) { crls.append(c) }
            warnings += other.warnings
        }
    }

    /// Synchronous HTTP (GET, or POST with a body). Only http/https.
    static func fetch(_ url: URL, body: Data? = nil, contentType: String? = nil, timeout: TimeInterval = 20) throws -> Data {
        guard ["http", "https"].contains(url.scheme?.lowercased() ?? "") else { throw RevocationError.http(url.absoluteString, 0) }
        var req = URLRequest(url: url, timeoutInterval: timeout)
        if let body {
            req.httpMethod = "POST"
            req.httpBody = body
        }
        if let contentType { req.setValue(contentType, forHTTPHeaderField: "Content-Type") }
        var result: Result<Data, Error> = .failure(RevocationError.http(url.absoluteString, 0))
        let done = DispatchSemaphore(value: 0)
        URLSession.shared.dataTask(with: req) { data, response, error in
            let code = (response as? HTTPURLResponse)?.statusCode ?? 0
            if let error { result = .failure(error) }
            else if let data, code == 200 { result = .success(data) }
            else { result = .failure(RevocationError.http(url.absoluteString, code)) }
            done.signal()
        }.resume()
        done.wait()
        return try result.get()
    }

    // MARK: OCSP (RFC 6960)

    static func ocspRequest(for cert: X509, issuer: X509) -> [UInt8] {
        let certID = DER.seq(DER.seq(DER.oid(OID.sha1), DER.null),
                             DER.octet([UInt8](Insecure.SHA1.hash(data: Data(cert.issuer)))),
                             DER.octet([UInt8](Insecure.SHA1.hash(data: Data(issuer.publicKey)))),
                             DER.tlv(0x02, cert.serial))
        return DER.seq(DER.seq(DER.seq(DER.seq(certID))))
    }

    struct OCSPInfo {
        enum Status { case good, revoked, unknown }
        var status: Status
        var serials: [[UInt8]]
        var producedAt: Date?
        var certificates: [X509]
    }

    /// Parses an OCSPResponse; nil if not a successful basic response.
    static func parseOCSP(_ bytes: [UInt8]) -> OCSPInfo? {
        guard let resp = DER.parse(bytes), resp.children.count == 2, resp.children[0].content == [0],
              let rb = resp.children[1].children.first, rb.children.count == 2,
              DER.oidString(rb.children[0].content) == OID.ocspBasic,
              let basic = DER.parse(rb.children[1].content), let tbs = basic.children.first else { return nil }
        var status = OCSPInfo.Status.unknown
        var serials: [[UInt8]] = []
        var producedAt: Date?
        for c in tbs.children {
            if c.tag == [0x18] { producedAt = DER.parseTime(c) }
            if c.tag == [0x30] {
                for single in c.children where single.children.count >= 2 {
                    if let s = single.children[0].children.last?.content { serials.append(s) }
                    switch single.children[1].tag.first {
                    case 0x80: if status == .unknown { status = .good }
                    case 0xA1: status = .revoked
                    default: break
                    }
                }
            }
        }
        let certs = basic.children.first { $0.tag == [0xA0] }?.children.first?.children.compactMap { X509(der: DER.encode($0)) } ?? []
        return OCSPInfo(status: status, serials: serials, producedAt: producedAt, certificates: certs)
    }

    static func ocsp(for cert: X509, issuer: X509) throws -> [UInt8] {
        var lastError: Error = RevocationError.noSource(cert.name)
        for s in cert.ocspURLs {
            guard let url = URL(string: s) else { continue }
            do {
                let bytes = [UInt8](try fetch(url, body: Data(ocspRequest(for: cert, issuer: issuer)), contentType: "application/ocsp-request"))
                guard let info = parseOCSP(bytes) else { throw RevocationError.badResponse(s) }
                if info.status == .revoked { throw RevocationError.revoked(cert.name) }
                return bytes
            } catch RevocationError.revoked(let name) {
                throw RevocationError.revoked(name)
            } catch { lastError = error }
        }
        throw lastError
    }

    // MARK: CRL (RFC 5280)

    /// Issuer Name (DER) of a CRL and its revoked serials.
    static func parseCRL(_ bytes: [UInt8]) -> (issuer: [UInt8], revoked: [[UInt8]], nextUpdate: Date?)? {
        guard let crl = DER.parse(bytes), let tbs = crl.children.first, tbs.tag == [0x30] else { return nil }
        var i = tbs.children.first?.tag == [0x02] ? 1 : 0
        guard tbs.children.count > i + 2 else { return nil }
        i += 1
        let issuer = DER.encode(tbs.children[i])
        var next: Date?
        var revoked: [[UInt8]] = []
        for c in tbs.children.dropFirst(i + 2) {
            if c.tag == [0x17] || c.tag == [0x18] { next = next ?? DER.parseTime(c) }
            if c.tag == [0x30] { revoked = c.children.compactMap { $0.children.first?.content } }
        }
        return (issuer, revoked, next)
    }

    static func crl(for cert: X509) throws -> [UInt8] {
        var lastError: Error = RevocationError.noSource(cert.name)
        for s in cert.crlURLs where s.lowercased().hasPrefix("http") {
            guard let url = URL(string: s) else { continue }
            do {
                let bytes = [UInt8](try fetch(url, timeout: 30))
                guard let info = parseCRL(bytes) else { throw RevocationError.badResponse(s) }
                if info.revoked.contains(cert.serial) { throw RevocationError.revoked(cert.name) }
                return bytes
            } catch RevocationError.revoked(let name) {
                throw RevocationError.revoked(name)
            } catch { lastError = error }
        }
        throw lastError
    }

    // MARK: Chains

    static func issuer(of cert: X509, in pool: [X509]) -> X509? {
        // A self-signed root wins over a cross-certificate with the same subject and key.
        let candidates = pool.filter { $0.subject == cert.issuer && $0 != cert }
        return candidates.first { $0.isSelfIssued } ?? candidates.first
    }

    /// The path macOS builds for `leaf` (prefers self-signed roots in the system store over cross-certificates).
    static func systemChain(_ leaf: X509, pool: [X509]) -> [X509] {
        let certs = ([leaf] + pool).compactMap { SecCertificateCreateWithData(nil, Data($0.der) as CFData) }
        var trust: SecTrust?
        guard SecTrustCreateWithCertificates(certs as CFArray, SecPolicyCreateBasicX509(), &trust) == errSecSuccess, let trust else { return [] }
        SecTrustSetNetworkFetchAllowed(trust, false)
        _ = SecTrustEvaluateWithError(trust, nil)
        return (SecTrustCopyCertificateChain(trust) as? [SecCertificate] ?? []).compactMap { X509($0) }
    }

    /// Fetches validation data for every certificate on the path of `leaves` (signer and timestamp certificates).
    /// Responses already in `known` (an earlier signature, an existing DSS) are reused instead of fetched again.
    static func collect(leaves: [X509], pool initial: [X509], known: ValidationData = ValidationData()) -> ValidationData {
        var data = ValidationData()
        var pool = initial
        // Issuers on the system-built paths come first.
        var preferred: [X509: X509] = [:]
        for leaf in leaves {
            let chain = systemChain(leaf, pool: initial)
            for (a, b) in zip(chain, chain.dropFirst()) where b.subject == a.issuer { preferred[a] = b }
        }
        var queue = leaves
        var done = Set<X509>()
        while let cert = queue.first {
            queue.removeFirst()
            guard !done.contains(cert), done.count < 32 else { continue }
            done.insert(cert)
            data.add(cert)
            if cert.isSelfIssued { continue }
            var issuer = preferred[cert] ?? issuer(of: cert, in: pool)
            if issuer == nil {
                for s in cert.caIssuerURLs {
                    guard let url = URL(string: s), let bytes = try? fetch(url) else { continue }
                    let found = X509.certificates(in: [UInt8](bytes))
                    pool += found
                    if let i = Self.issuer(of: cert, in: found) { issuer = i; break }
                }
            }
            guard let issuer else {
                data.warnings.append(L("The issuer of “%@” was not found", cert.name))
                continue
            }
            queue.append(issuer)
            if cert.ocspNoCheck { continue }
            if let o = known.ocsps.first(where: { covered(cert, ocsps: [$0], crls: []) }) {
                data.ocsps.append(o)
                continue
            }
            if let c = known.crls.first(where: { covered(cert, ocsps: [], crls: [$0]) }) {
                data.crls.append(c)
                continue
            }
            do {
                let response = try ocsp(for: cert, issuer: issuer)
                data.ocsps.append(response)
                for responder in parseOCSP(response)?.certificates ?? [] {
                    pool.append(responder)
                    queue.append(responder)
                }
            } catch RevocationError.revoked(let name) {
                data.warnings.append(L("The certificate “%@” is revoked", name))
            } catch {
                do { data.crls.append(try crl(for: cert)) } catch {
                    data.warnings.append(L("No revocation data for “%@”: %@", cert.name, error.localizedDescription))
                }
            }
        }
        return data
    }

    // MARK: Checks (verification)

    /// Whether `ocsps`/`crls` cover `cert` (by serial and issuer name).
    static func covered(_ cert: X509, ocsps: [[UInt8]], crls: [[UInt8]]) -> Bool {
        if cert.ocspNoCheck { return true }
        let nameHash = [UInt8](Insecure.SHA1.hash(data: Data(cert.issuer)))
        let nameHash256 = [UInt8](SHA256.hash(data: Data(cert.issuer)))
        for o in ocsps {
            guard let resp = DER.parse(o), resp.children.count == 2, let rb = resp.children[1].children.first,
                  rb.children.count == 2, let basic = DER.parse(rb.children[1].content) else { continue }
            for certID in DER.all([0x30], in: basic) where certID.children.count == 4 && certID.children[3].tag == [0x02] {
                if certID.children[3].content == cert.serial,
                   [nameHash, nameHash256].contains(certID.children[1].content) { return true }
            }
        }
        for c in crls {
            if let info = parseCRL(c), info.issuer == cert.issuer, !info.revoked.contains(cert.serial) { return true }
        }
        return false
    }

    /// Chain of `leaf` in `pool`, leaf first.
    static func chain(of leaf: X509, pool: [X509]) -> [X509] {
        var chain = [leaf]
        var cur = leaf
        while !cur.isSelfIssued, let next = issuer(of: cur, in: pool), !chain.contains(next), chain.count < 16 {
            chain.append(next)
            cur = next
        }
        return chain
    }

    enum RevocationError: LocalizedError {
        case http(String, Int), badResponse(String), revoked(String), noSource(String)
        var errorDescription: String? {
            switch self {
            case .http(let url, let code): return code == 0 ? L("No response from %@", url) : L("%@ answered with HTTP %d", url, code)
            case .badResponse(let url): return L("The response from %@ could not be read", url)
            case .revoked(let name): return L("The certificate “%@” is revoked", name)
            case .noSource(let name): return L("“%@” has no OCSP or CRL address", name)
            }
        }
    }
}
