import AppKit
import CryptoKit
import Foundation
import Kit42

/// Remote signing through the Cloud Signature Consortium API v2 (qualified signatures at a QTSP, eIDAS).
/// Only the SHA-256 hash of the signed attributes leaves the computer; the document stays local.
final class CSCClient {
    struct Config {
        /// Service base URL including the version path, e.g. https://qtsp.example/csc/v2
        var baseURL: URL
        var accessToken: String?
        var clientID: String?
        var clientSecret: String?
        var username: String?
        var password: String?
        /// Loopback port for the OAuth 2.0 authorization code redirect (must match the client registration).
        var redirectPort: UInt16 = 8765
        /// Opens the OAuth authorization page (browser).
        var openURL: (URL) -> Void = { NSWorkspace.shared.open($0) }
    }

    struct Credential {
        var id: String
        var description: String?
        var chain: [X509]
        var algorithms: [String]
        var authMode: String          // implicit, explicit, oauth2code
        var needsPIN = false
        var needsOTP = false
        var otpOnline = false
        var scal: String?

        var label: String { (chain.first?.name ?? id) + (description.map { " — \($0)" } ?? "") }
    }

    let config: Config
    private(set) var info: [String: Any] = [:]
    private var serviceToken: String?

    init(_ config: Config) { self.config = config }

    // MARK: HTTP

    private func endpoint(_ path: String) -> URL { config.baseURL.appendingPathComponent(path) }

    @discardableResult
    func post(_ path: String, _ body: [String: Any], token: String? = nil, basic: (String, String)? = nil) throws -> [String: Any] {
        var req = URLRequest(url: endpoint(path), timeoutInterval: 60)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let token { req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
        if let (u, p) = basic { req.setValue("Basic " + Data("\(u):\(p)".utf8).base64EncodedString(), forHTTPHeaderField: "Authorization") }
        req.httpBody = try JSONSerialization.data(withJSONObject: body)
        return try send(req)
    }

    private func send(_ req: URLRequest) throws -> [String: Any] {
        guard ["https", "http"].contains(req.url?.scheme ?? "") else { throw CSCError.service(L("invalid provider URL")) }
        var result: Result<(Data, Int), Error> = .failure(CSCError.service(L("no response")))
        let done = DispatchSemaphore(value: 0)
        URLSession.shared.dataTask(with: req) { data, response, error in
            if let error { result = .failure(error) } else { result = .success((data ?? Data(), (response as? HTTPURLResponse)?.statusCode ?? 0)) }
            done.signal()
        }.resume()
        while done.wait(timeout: .now() + 0.05) == .timedOut {
            if Thread.isMainThread { RunLoop.current.run(until: Date().addingTimeInterval(0.05)) }
        }
        let (data, code) = try result.get()
        let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
        guard code == 200 else {
            let message = [json["error"] as? String, json["error_description"] as? String].compactMap { $0 }.joined(separator: ": ")
            throw CSCError.service("\(req.url?.lastPathComponent ?? ""): HTTP \(code) \(message)")
        }
        return json
    }

    // MARK: Service authorization

    func loadInfo() throws -> [String: Any] {
        if info.isEmpty { info = try post("info", ["lang": "en"]) }
        return info
    }

    var authTypes: [String] { info["authType"] as? [String] ?? [] }

    /// OAuth 2.0 base: the "oauth2" URL from /info, or the service URL.
    private var oauthBase: URL {
        (info["oauth2"] as? String).flatMap(URL.init(string:)) ?? config.baseURL
    }

    /// Obtains a service access token: given token, HTTP Basic (auth/login), client credentials or the
    /// authorization code flow in the browser.
    func authorizeService() throws {
        if serviceToken != nil { return }
        if let t = config.accessToken, !t.isEmpty { serviceToken = t; return }
        _ = try? loadInfo()
        if let u = config.username, let p = config.password {
            serviceToken = try post("auth/login", [:], basic: (u, p))["access_token"] as? String
        } else if let id = config.clientID, let secret = config.clientSecret, !secret.isEmpty,
                  authTypes.isEmpty || authTypes.contains("oauth2client") {
            serviceToken = try token(["grant_type": "client_credentials", "client_id": id, "client_secret": secret])
        } else if config.clientID != nil {
            serviceToken = try authorizationCode(scope: "service", extra: [:])
        }
        guard serviceToken != nil else { throw CSCError.service(L("no access token: configure a token, a login or an OAuth client")) }
    }

    private func token(_ form: [String: String]) throws -> String {
        var req = URLRequest(url: oauthBase.appendingPathComponent("oauth2/token"), timeoutInterval: 60)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        req.httpBody = Data(Self.formEncode(form).utf8)
        guard let t = try send(req)["access_token"] as? String else { throw CSCError.service(L("no access token")) }
        return t
    }

    static func formEncode(_ form: [String: String]) -> String {
        var allowed = CharacterSet.alphanumerics
        allowed.insert(charactersIn: "-._~")
        return form.sorted { $0.key < $1.key }.map {
            "\($0.key)=\($0.value.addingPercentEncoding(withAllowedCharacters: allowed) ?? "")"
        }.joined(separator: "&")
    }

    /// OAuth 2.0 authorization code flow with PKCE and a loopback redirect (RFC 8252).
    func authorizationCode(scope: String, extra: [String: String]) throws -> String {
        guard let clientID = config.clientID else { throw CSCError.service(L("no OAuth client ID")) }
        let redirect = "http://127.0.0.1:\(config.redirectPort)/callback"
        let verifier = Data((0..<32).map { _ in UInt8.random(in: 0...255) }).base64URL
        let challenge = Data(SHA256.hash(data: Data(verifier.utf8))).base64URL
        let state = UUID().uuidString
        var params = ["response_type": "code", "client_id": clientID, "redirect_uri": redirect, "scope": scope,
                      "code_challenge": challenge, "code_challenge_method": "S256", "state": state]
        params.merge(extra) { $1 }
        let listener = try LoopbackListener(port: config.redirectPort)
        guard var comps = URLComponents(url: oauthBase.appendingPathComponent("oauth2/authorize"), resolvingAgainstBaseURL: false) else {
            throw CSCError.service(L("invalid provider URL"))
        }
        comps.percentEncodedQuery = Self.formEncode(params)
        config.openURL(comps.url!)
        let query = try listener.waitForRedirect(timeout: 300)
        guard query["state"] == state else { throw CSCError.service(L("OAuth state mismatch")) }
        guard let code = query["code"] else { throw CSCError.service(query["error_description"] ?? query["error"] ?? L("authorization was cancelled")) }
        var form = ["grant_type": "authorization_code", "code": code, "client_id": clientID, "redirect_uri": redirect, "code_verifier": verifier]
        if let s = config.clientSecret, !s.isEmpty { form["client_secret"] = s }
        return try token(form)
    }

    // MARK: Credentials

    func listCredentials() throws -> [Credential] {
        try authorizeService()
        let list = try post("credentials/list", ["credentialInfo": true, "certificates": "chain", "certInfo": true, "authInfo": true],
                            token: serviceToken)
        let ids = list["credentialIDs"] as? [String] ?? []
        let infos = list["credentialInfos"] as? [[String: Any]] ?? []
        return try ids.map { id in
            if let i = infos.first(where: { $0["credentialID"] as? String == id }), Self.certificates(i).count > 0 {
                return Self.credential(id, i)
            }
            return try credential(id)
        }
    }

    func credential(_ id: String) throws -> Credential {
        try authorizeService()
        let i = try post("credentials/info", ["credentialID": id, "certificates": "chain", "certInfo": true, "authInfo": true], token: serviceToken)
        return Self.credential(id, i)
    }

    private static func certificates(_ i: [String: Any]) -> [X509] {
        ((i["cert"] as? [String: Any])?["certificates"] as? [String] ?? []).compactMap {
            Data(base64Encoded: $0).flatMap { X509(der: [UInt8]($0)) }
        }
    }

    static func credential(_ id: String, _ i: [String: Any]) -> Credential {
        let key = i["key"] as? [String: Any] ?? [:]
        let auth = i["auth"] as? [String: Any] ?? [:]
        var c = Credential(id: id, description: i["description"] as? String, chain: certificates(i),
                           algorithms: key["algo"] as? [String] ?? [],
                           authMode: auth["mode"] as? String ?? i["authMode"] as? String ?? "explicit", scal: i["SCAL"] as? String)
        // v2: auth.objects [{type: Password|OTP, id: PIN|OTP, generator?}], v1: PIN.presence / OTP.presence.
        for o in auth["objects"] as? [[String: Any]] ?? [] {
            let type = (o["type"] as? String ?? "").lowercased()
            if type == "password" { c.needsPIN = true }
            if type == "otp" { c.needsOTP = true; c.otpOnline = (o["generator"] as? String) == "online" || (o["type_online"] as? Bool) == true }
        }
        if (i["PIN"] as? [String: Any])?["presence"] as? String == "true" { c.needsPIN = true }
        if let otp = i["OTP"] as? [String: Any], otp["presence"] as? String == "true" {
            c.needsOTP = true
            c.otpOnline = otp["type"] as? String == "online"
        }
        return c
    }

    func sendOTP(_ credential: Credential) throws {
        try post("credentials/sendOTP", ["credentialID": credential.id], token: serviceToken)
    }

    /// Signature activation data (SAD) for signing `hashes` with the credential.
    func authorize(_ credential: Credential, hashes: [[UInt8]], pin: String?, otp: String?) throws -> String {
        let b64 = hashes.map { Data($0).base64EncodedString() }
        if credential.authMode == "oauth2code" {
            // Credential-scoped token: the access token itself authorizes these hashes.
            let token = try authorizationCode(scope: "credential", extra: [
                "credentialID": credential.id, "numSignatures": "\(hashes.count)",
                "hashes": hashes.map { Data($0).base64URL }.joined(separator: ","), "hashAlgorithmOID": OID.sha256,
            ])
            return token
        }
        var body: [String: Any] = ["credentialID": credential.id, "numSignatures": hashes.count, "hashes": b64, "hashAlgorithmOID": OID.sha256]
        var authData: [[String: String]] = []
        if let pin { authData.append(["id": "PIN", "value": pin]) }
        if let otp { authData.append(["id": "OTP", "value": otp]) }
        if !authData.isEmpty { body["authData"] = authData }
        guard let sad = try post("credentials/authorize", body, token: serviceToken)["SAD"] as? String else {
            throw CSCError.service(L("no signature activation data (SAD)"))
        }
        return sad
    }

    func signHash(_ credential: Credential, sad: String, hashes: [[UInt8]], signAlgo: String) throws -> [[UInt8]] {
        var token = serviceToken
        if credential.authMode == "oauth2code" { token = sad }
        var body: [String: Any] = ["credentialID": credential.id, "hashes": hashes.map { Data($0).base64EncodedString() },
                                   "hashAlgorithmOID": OID.sha256, "signAlgo": signAlgo, "operationMode": "S"]
        if credential.authMode != "oauth2code" { body["SAD"] = sad }
        let r = try post("signatures/signHash", body, token: token)
        let sigs = (r["signatures"] as? [String] ?? []).compactMap { Data(base64Encoded: $0).map { [UInt8]($0) } }
        guard sigs.count == hashes.count else { throw CSCError.service(L("the service returned no signature")) }
        return sigs
    }
}

/// CMS signer backed by a CSC credential.
struct CSCSigner: CMSRawSigner {
    let client: CSCClient
    let credential: CSCClient.Credential
    /// Asked once per signature for PIN / OTP when the credential needs them (explicit authorization).
    var authData: (CSCClient.Credential) throws -> (pin: String?, otp: String?) = { _ in (nil, nil) }

    var chain: [X509] { credential.chain }

    func sign(attributes: [UInt8]) throws -> (signature: [UInt8], algorithm: [UInt8]) {
        guard let leaf = chain.first else { throw CSCError.service(L("the credential has no certificate")) }
        let isEC = leaf.keyAlgorithm == OID.ecPublicKey
        let algos = credential.algorithms
        let signAlgo: String
        if isEC { signAlgo = algos.contains(OID.ecdsaWithSHA256) || algos.isEmpty ? OID.ecdsaWithSHA256 : OID.ecPublicKey }
        else if algos.isEmpty || algos.contains(OID.rsaEncryption) { signAlgo = OID.rsaEncryption }
        else if algos.contains(OID.sha256WithRSA) { signAlgo = OID.sha256WithRSA }
        else { throw CSCError.service(L("the credential offers no supported algorithm (RSA or ECDSA with SHA-256)")) }

        let hash = [UInt8](SHA256.hash(data: Data(attributes)))
        var pin: String?, otp: String?
        if credential.authMode == "explicit" {
            if credential.needsOTP, credential.otpOnline { try client.sendOTP(credential) }
            (pin, otp) = try authData(credential)
        }
        let sad = try client.authorize(credential, hashes: [hash], pin: pin, otp: otp)
        let signature = try client.signHash(credential, sad: sad, hashes: [hash], signAlgo: signAlgo)[0]
        return (signature, isEC ? DER.seq(DER.oid(OID.ecdsaWithSHA256)) : DER.seq(DER.oid(OID.sha256WithRSA), DER.null))
    }
}

enum CSCError: LocalizedError {
    case service(String)
    var errorDescription: String? {
        if case .service(let s) = self { return L("Remote signing service: %@", s) }
        return nil
    }
}

extension Data {
    var base64URL: String {
        base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}

/// One-shot HTTP listener on 127.0.0.1 for the OAuth redirect.
final class LoopbackListener {
    private let fd: Int32

    init(port: UInt16) throws {
        fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { throw CSCError.service("socket") }
        var yes: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &yes, socklen_t(MemoryLayout<Int32>.size))
        var addr = sockaddr_in()
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = port.bigEndian
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        guard bound == 0, listen(fd, 4) == 0 else {
            close(fd)
            throw CSCError.service(L("port %d is busy", Int(port)))
        }
    }

    deinit { close(fd) }

    /// Waits for GET /callback?… and returns its query parameters.
    func waitForRedirect(timeout: TimeInterval) throws -> [String: String] {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            var p = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
            if poll(&p, 1, 100) > 0 {
                let client = accept(fd, nil, nil)
                guard client >= 0 else { continue }
                defer { close(client) }
                var buffer = [UInt8](repeating: 0, count: 16384)
                let n = read(client, &buffer, buffer.count)
                let request = String(decoding: buffer[0..<max(0, n)], as: UTF8.self)
                guard let line = request.split(separator: "\r\n").first, line.hasPrefix("GET "),
                      let target = line.split(separator: " ").dropFirst().first,
                      let comps = URLComponents(string: "http://127.0.0.1" + target), comps.path == "/callback" else {
                    _ = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".withCString { write(client, $0, strlen($0)) }
                    continue
                }
                let body = "<html><body style=\"font:15px -apple-system\">" + L("You can close this page and return to PDF42.") + "</body></html>"
                let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: \(body.utf8.count)\r\nConnection: close\r\n\r\n\(body)"
                _ = response.withCString { write(client, $0, strlen($0)) }
                var out: [String: String] = [:]
                for item in comps.queryItems ?? [] { out[item.name] = item.value ?? "" }
                return out
            }
            if Thread.isMainThread { RunLoop.current.run(until: Date().addingTimeInterval(0.05)) }
        }
        throw CSCError.service(L("authorization timed out"))
    }
}
