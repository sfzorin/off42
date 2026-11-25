import CryptoKit
import Foundation
import Kit42

/// RFC 3161 trusted timestamp: asks a TSA to timestamp the signature value and embeds the token
/// as an unsigned attribute (id-aa-signatureTimeStampToken) of the CMS SignerInfo.
enum Timestamp {
    struct Node {
        var tag: [UInt8]
        var constructed: Bool
        var content: [UInt8] = []
        var children: [Node] = []
    }

    static func parse(_ b: [UInt8], _ pos: inout Int) throws -> Node {
        guard pos < b.count else { throw TSAError.malformed }
        var tag = [b[pos]]
        let constructed = b[pos] & 0x20 != 0
        if b[pos] & 0x1F == 0x1F {
            repeat { pos += 1; tag.append(b[pos]) } while b[pos] & 0x80 != 0
        }
        pos += 1
        guard pos < b.count else { throw TSAError.malformed }
        let first = b[pos]; pos += 1
        var node = Node(tag: tag, constructed: constructed)
        if first == 0x80 {
            // Indefinite length (BER from CMSEncoder): children until 00 00
            while pos + 1 < b.count, !(b[pos] == 0 && b[pos + 1] == 0) { node.children.append(try parse(b, &pos)) }
            pos += 2
            return node
        }
        var len = 0
        if first & 0x80 == 0 { len = Int(first) } else {
            for _ in 0..<Int(first & 0x7F) { len = len << 8 | Int(b[pos]); pos += 1 }
        }
        guard pos + len <= b.count else { throw TSAError.malformed }
        if constructed {
            let end = pos + len
            while pos < end { node.children.append(try parse(b, &pos)) }
        } else {
            node.content = Array(b[pos..<pos + len])
            pos += len
        }
        return node
    }

    static func encode(_ n: Node) -> [UInt8] {
        let body = n.constructed ? n.children.flatMap(encode) : n.content
        return n.tag + length(body.count) + body
    }

    static func length(_ n: Int) -> [UInt8] {
        if n < 0x80 { return [UInt8(n)] }
        var bytes: [UInt8] = []
        var v = n
        while v > 0 { bytes.insert(UInt8(v & 0xFF), at: 0); v >>= 8 }
        return [0x80 | UInt8(bytes.count)] + bytes
    }

    static func tlv(_ tag: UInt8, _ body: [UInt8]) -> [UInt8] { [tag] + length(body.count) + body }

    /// Returns the CMS with a timestamp token added to its first signer.
    static func add(to cms: Data, tsa: URL) throws -> Data {
        var pos = 0
        var root = try parse([UInt8](cms), &pos)
        // ContentInfo → [0] → SignedData → signerInfos (last SET) → SignerInfo
        guard root.children.count >= 2, !root.children[1].children.isEmpty else { throw TSAError.malformed }
        var signedData = root.children[1].children[0]
        guard let setIndex = signedData.children.lastIndex(where: { $0.tag == [0x31] }),
              var signer = signedData.children[setIndex].children.first,
              let signature = signer.children.first(where: { $0.tag == [0x04] }) else { throw TSAError.malformed }

        let token = try requestToken(for: signature.content, tsa: tsa)
        var tp = 0
        let tokenNode = try parse(token, &tp)
        // Attribute { id-aa-signatureTimeStampToken, SET { token } }
        let oid: [UInt8] = [0x06, 0x0B, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x10, 0x02, 0x0E]
        var p0 = 0
        let attribute = try parse(tlv(0x30, oid + tlv(0x31, encode(tokenNode))), &p0)
        if let i = signer.children.firstIndex(where: { $0.tag == [0xA1] }) {
            signer.children[i].children.append(attribute)
        } else {
            signer.children.append(Node(tag: [0xA1], constructed: true, children: [attribute]))
        }
        signedData.children[setIndex].children[0] = signer
        root.children[1].children[0] = signedData
        return Data(encode(root))
    }

    static func requestToken(for signature: [UInt8], tsa: URL) throws -> [UInt8] {
        try requestToken(digest: [UInt8](SHA256.hash(data: Data(signature))), tsa: tsa)
    }

    /// TimeStampToken for a SHA-256 digest (also used for PAdES document timestamps).
    static func requestToken(digest hash: [UInt8], tsa: URL) throws -> [UInt8] {
        let sha256: [UInt8] = [0x30, 0x0D, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00]
        let imprint = tlv(0x30, sha256 + tlv(0x04, hash))
        var nonce = (0..<8).map { _ in UInt8.random(in: 0...255) }
        nonce[0] &= 0x7F
        let request = tlv(0x30, [0x02, 0x01, 0x01] + imprint + tlv(0x02, nonce) + [0x01, 0x01, 0xFF])

        var req = URLRequest(url: tsa, timeoutInterval: 20)
        req.httpMethod = "POST"
        req.setValue("application/timestamp-query", forHTTPHeaderField: "Content-Type")
        req.httpBody = Data(request)
        var result: Result<Data, Error> = .failure(TSAError.noResponse)
        let done = DispatchSemaphore(value: 0)
        URLSession.shared.dataTask(with: req) { data, response, error in
            if let error { result = .failure(error) }
            else if let data, (response as? HTTPURLResponse)?.statusCode == 200 { result = .success(data) }
            done.signal()
        }.resume()
        done.wait()
        let bytes = [UInt8](try result.get())
        var pos = 0
        let resp = try parse(bytes, &pos)
        // TimeStampResp { status PKIStatusInfo { status INTEGER }, timeStampToken }
        guard let status = resp.children.first?.children.first?.content.last, status <= 1, resp.children.count > 1 else {
            throw TSAError.rejected
        }
        return encode(resp.children[1])
    }

    enum TSAError: LocalizedError {
        case malformed, noResponse, rejected
        var errorDescription: String? {
            switch self {
            case .malformed: return L("The timestamp response could not be read")
            case .noResponse: return L("The timestamp server did not respond")
            case .rejected: return L("The timestamp server rejected the request")
            }
        }
    }
}
