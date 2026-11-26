import Foundation

// P11Bridge42: loads a PKCS#11 module (smart-card middleware such as AKİS) in a separate process and answers one
// JSON request on stdin with one JSON response on stdout. Running out of process keeps a faulty vendor module from
// crashing the app and lets an Intel-only module run under Rosetta (`arch -x86_64`) next to an Apple-silicon app.
//
// Requests:
//   {"op":"list","module":"/path/lib.dylib"}
//   {"op":"sign","module":…,"slot":N,"id":"hex CKA_ID","label":"…","pin":"…","mechanism":"rsa-pkcs"|"ecdsa","data":"base64"}
// The module is never bundled: the path is the one the user's middleware installed.

typealias CK_RV = UInt
typealias CK_ULONG = UInt

struct P11Error: Error {
    let rv: CK_RV
    let call: String
}

final class Module {
    private let handle: UnsafeMutableRawPointer
    private let list: UnsafeRawPointer

    init(path: String) throws {
        guard let h = dlopen(path, RTLD_NOW | RTLD_LOCAL) else {
            throw NSError(domain: "P11", code: 1, userInfo: [NSLocalizedDescriptionKey: String(cString: dlerror())])
        }
        handle = h
        guard let sym = dlsym(h, "C_GetFunctionList") else {
            throw NSError(domain: "P11", code: 2, userInfo: [NSLocalizedDescriptionKey: "C_GetFunctionList not found: not a PKCS#11 module"])
        }
        typealias GetList = @convention(c) (UnsafeMutablePointer<UnsafeRawPointer?>) -> CK_RV
        var p: UnsafeRawPointer?
        let rv = unsafeBitCast(sym, to: GetList.self)(&p)
        guard rv == 0, let p else { throw P11Error(rv: rv, call: "C_GetFunctionList") }
        list = p
    }

    /// Function pointer `index` of CK_FUNCTION_LIST (after the 2-byte CK_VERSION, pointer-aligned).
    func fn<T>(_ index: Int, _ type: T.Type) -> T {
        let ptr = list.load(fromByteOffset: MemoryLayout<UnsafeRawPointer>.stride * (index + 1), as: UnsafeRawPointer.self)
        return unsafeBitCast(ptr, to: type)
    }

    func check(_ rv: CK_RV, _ call: String) throws { if rv != 0 { throw P11Error(rv: rv, call: call) } }

    func initialize() throws {
        typealias F = @convention(c) (UnsafeMutableRawPointer?) -> CK_RV
        // CK_C_INITIALIZE_ARGS with CKF_OS_LOCKING_OK: six pointer-sized fields.
        var args = [UInt](repeating: 0, count: 6)
        args[4] = 0x2
        let rv = args.withUnsafeMutableBytes { fn(0, F.self)($0.baseAddress) }
        if rv == 0x191 { return } // CKR_CRYPTOKI_ALREADY_INITIALIZED
        if rv != 0 { try check(fn(0, F.self)(nil), "C_Initialize") }
    }

    func finalize() {
        typealias F = @convention(c) (UnsafeMutableRawPointer?) -> CK_RV
        _ = fn(1, F.self)(nil)
    }

    func info() throws -> [String: Any] {
        typealias F = @convention(c) (UnsafeMutableRawPointer) -> CK_RV
        var buf = [UInt8](repeating: 0, count: 256)
        try check(buf.withUnsafeMutableBytes { fn(2, F.self)($0.baseAddress!) }, "C_GetInfo")
        return ["cryptoki": "\(buf[0]).\(buf[1])", "manufacturer": text(buf, 2, 32), "library": text(buf, 48, 32),
                "version": "\(buf[80]).\(buf[81])"]
    }

    func slots() throws -> [UInt] {
        typealias F = @convention(c) (UInt8, UnsafeMutablePointer<UInt>?, UnsafeMutablePointer<UInt>) -> CK_RV
        var count: UInt = 0
        try check(fn(4, F.self)(0, nil, &count), "C_GetSlotList")
        var ids = [UInt](repeating: 0, count: Int(count))
        if count > 0 { try check(ids.withUnsafeMutableBufferPointer { fn(4, F.self)(0, $0.baseAddress, &count) }, "C_GetSlotList") }
        return Array(ids.prefix(Int(count)))
    }

    func slotInfo(_ slot: UInt) throws -> (description: String, present: Bool) {
        typealias F = @convention(c) (UInt, UnsafeMutableRawPointer) -> CK_RV
        var buf = [UInt8](repeating: 0, count: 256)
        try check(buf.withUnsafeMutableBytes { fn(5, F.self)(slot, $0.baseAddress!) }, "C_GetSlotInfo")
        let flags = buf.withUnsafeBytes { $0.load(fromByteOffset: 96, as: UInt.self) }
        return (text(buf, 0, 64), flags & 1 != 0)
    }

    func tokenInfo(_ slot: UInt) throws -> [String: Any] {
        typealias F = @convention(c) (UInt, UnsafeMutableRawPointer) -> CK_RV
        var buf = [UInt8](repeating: 0, count: 512)
        try check(buf.withUnsafeMutableBytes { fn(6, F.self)(slot, $0.baseAddress!) }, "C_GetTokenInfo")
        let flags = buf.withUnsafeBytes { $0.load(fromByteOffset: 96, as: UInt.self) }
        return ["label": text(buf, 0, 32), "manufacturer": text(buf, 32, 32), "model": text(buf, 64, 16),
                "serial": text(buf, 80, 16), "flags": flags]
    }

    func openSession(_ slot: UInt) throws -> UInt {
        typealias F = @convention(c) (UInt, UInt, UnsafeMutableRawPointer?, UnsafeMutableRawPointer?, UnsafeMutablePointer<UInt>) -> CK_RV
        var session: UInt = 0
        try check(fn(12, F.self)(slot, 0x4, nil, nil, &session), "C_OpenSession")
        return session
    }

    func closeSession(_ s: UInt) {
        typealias F = @convention(c) (UInt) -> CK_RV
        _ = fn(13, F.self)(s)
    }

    func login(_ s: UInt, pin: String?) throws {
        typealias F = @convention(c) (UInt, UInt, UnsafePointer<UInt8>?, UInt) -> CK_RV
        var rv: CK_RV
        if let pin {
            let bytes = Array(pin.utf8)
            rv = bytes.withUnsafeBufferPointer { fn(18, F.self)(s, 1, $0.baseAddress, UInt(bytes.count)) }
        } else {
            rv = fn(18, F.self)(s, 1, nil, 0) // protected authentication path (PIN pad)
        }
        if rv == 0x100 { return } // CKR_USER_ALREADY_LOGGED_IN
        try check(rv, "C_Login")
    }

    func logout(_ s: UInt) {
        typealias F = @convention(c) (UInt) -> CK_RV
        _ = fn(19, F.self)(s)
    }

    /// Objects matching (type, value) attributes.
    func find(_ s: UInt, _ template: [(UInt, [UInt8])]) throws -> [UInt] {
        typealias Init = @convention(c) (UInt, UnsafeMutableRawPointer?, UInt) -> CK_RV
        typealias Find = @convention(c) (UInt, UnsafeMutablePointer<UInt>, UInt, UnsafeMutablePointer<UInt>) -> CK_RV
        typealias Final = @convention(c) (UInt) -> CK_RV
        let values = template.map { UnsafeMutableRawBufferPointer.allocate(byteCount: max(1, $0.1.count), alignment: 8) }
        defer { values.forEach { $0.deallocate() } }
        var attrs = [UInt](repeating: 0, count: template.count * 3)
        for (i, t) in template.enumerated() {
            values[i].copyBytes(from: t.1)
            attrs[i * 3] = t.0
            attrs[i * 3 + 1] = UInt(bitPattern: values[i].baseAddress)
            attrs[i * 3 + 2] = UInt(t.1.count)
        }
        try check(attrs.withUnsafeMutableBytes { fn(26, Init.self)(s, $0.baseAddress, UInt(template.count)) }, "C_FindObjectsInit")
        defer { _ = fn(28, Final.self)(s) }
        var found: [UInt] = []
        var batch = [UInt](repeating: 0, count: 32)
        while true {
            var n: UInt = 0
            try check(batch.withUnsafeMutableBufferPointer { fn(27, Find.self)(s, $0.baseAddress!, 32, &n) }, "C_FindObjects")
            if n == 0 { break }
            found += batch.prefix(Int(n))
        }
        return found
    }

    func attribute(_ s: UInt, _ object: UInt, _ type: UInt) -> [UInt8]? {
        typealias F = @convention(c) (UInt, UInt, UnsafeMutableRawPointer, UInt) -> CK_RV
        var attr: [UInt] = [type, 0, 0]
        guard attr.withUnsafeMutableBytes({ fn(24, F.self)(s, object, $0.baseAddress!, 1) }) == 0,
              attr[2] != UInt.max, attr[2] < 1 << 20 else { return nil }
        var value = [UInt8](repeating: 0, count: Int(attr[2]))
        let rv = value.withUnsafeMutableBytes { buf -> CK_RV in
            attr[1] = UInt(bitPattern: buf.baseAddress)
            return attr.withUnsafeMutableBytes { fn(24, F.self)(s, object, $0.baseAddress!, 1) }
        }
        return rv == 0 ? Array(value.prefix(Int(attr[2]))) : nil
    }

    func sign(_ s: UInt, key: UInt, mechanism: UInt, data: [UInt8]) throws -> [UInt8] {
        typealias Init = @convention(c) (UInt, UnsafeMutableRawPointer, UInt) -> CK_RV
        typealias Sign = @convention(c) (UInt, UnsafePointer<UInt8>, UInt, UnsafeMutablePointer<UInt8>?, UnsafeMutablePointer<UInt>) -> CK_RV
        var mech: [UInt] = [mechanism, 0, 0]
        try check(mech.withUnsafeMutableBytes { fn(42, Init.self)(s, $0.baseAddress!, key) }, "C_SignInit")
        var length: UInt = 0
        try check(data.withUnsafeBufferPointer { fn(43, Sign.self)(s, $0.baseAddress!, UInt(data.count), nil, &length) }, "C_Sign")
        var out = [UInt8](repeating: 0, count: Int(length))
        try check(data.withUnsafeBufferPointer { d in out.withUnsafeMutableBufferPointer { fn(43, Sign.self)(s, d.baseAddress!, UInt(data.count), $0.baseAddress, &length) } }, "C_Sign")
        return Array(out.prefix(Int(length)))
    }

    private func text(_ b: [UInt8], _ offset: Int, _ length: Int) -> String {
        String(decoding: b[offset..<(offset + length)], as: UTF8.self).trimmingCharacters(in: .whitespaces.union(.controlCharacters))
    }
}

func ulong(_ v: UInt) -> [UInt8] { withUnsafeBytes(of: v) { Array($0) } }
func hex(_ b: [UInt8]) -> String { b.map { String(format: "%02x", $0) }.joined() }
func unhex(_ s: String) -> [UInt8] {
    var out: [UInt8] = []
    var i = s.startIndex
    while let j = s.index(i, offsetBy: 2, limitedBy: s.endIndex), i < s.endIndex {
        if let v = UInt8(s[i..<j], radix: 16) { out.append(v) }
        i = j
    }
    return out
}

let CKA_CLASS: UInt = 0, CKA_LABEL: UInt = 3, CKA_VALUE: UInt = 0x11, CKA_ID: UInt = 0x102, CKA_KEY_TYPE: UInt = 0x100
let CKO_CERTIFICATE: UInt = 1, CKO_PRIVATE_KEY: UInt = 3

func list(_ m: Module) throws -> [String: Any] {
    var slotsOut: [[String: Any]] = [], certs: [[String: Any]] = []
    for slot in try m.slots() {
        let (description, present) = try m.slotInfo(slot)
        var entry: [String: Any] = ["slot": slot, "description": description, "present": present]
        guard present, let token = try? m.tokenInfo(slot) else { slotsOut.append(entry); continue }
        entry["token"] = token
        slotsOut.append(entry)
        guard let s = try? m.openSession(slot) else { continue }
        defer { m.closeSession(s) }
        let keyIDs = Set(((try? m.find(s, [(CKA_CLASS, ulong(CKO_PRIVATE_KEY))])) ?? []).compactMap { m.attribute(s, $0, CKA_ID) }.map(hex))
        for obj in (try? m.find(s, [(CKA_CLASS, ulong(CKO_CERTIFICATE))])) ?? [] {
            guard let der = m.attribute(s, obj, CKA_VALUE), !der.isEmpty else { continue }
            let id = hex(m.attribute(s, obj, CKA_ID) ?? [])
            certs.append(["slot": slot, "id": id, "label": String(decoding: m.attribute(s, obj, CKA_LABEL) ?? [], as: UTF8.self),
                          "der": Data(der).base64EncodedString(), "hasKey": keyIDs.contains(id)])
        }
    }
    return ["info": try m.info(), "slots": slotsOut, "certificates": certs]
}

func sign(_ m: Module, _ r: [String: Any]) throws -> [String: Any] {
    guard let slot = (r["slot"] as? NSNumber)?.uintValue, let data = (r["data"] as? String).flatMap({ Data(base64Encoded: $0) }) else {
        throw NSError(domain: "P11", code: 3, userInfo: [NSLocalizedDescriptionKey: "bad request"])
    }
    let s = try m.openSession(slot)
    defer { m.logout(s); m.closeSession(s) }
    try m.login(s, pin: r["pin"] as? String)
    var keys: [UInt] = []
    if let id = r["id"] as? String, !id.isEmpty { keys = try m.find(s, [(CKA_CLASS, ulong(CKO_PRIVATE_KEY)), (CKA_ID, unhex(id))]) }
    if keys.isEmpty, let label = r["label"] as? String, !label.isEmpty {
        keys = try m.find(s, [(CKA_CLASS, ulong(CKO_PRIVATE_KEY)), (CKA_LABEL, Array(label.utf8))])
    }
    if keys.isEmpty {
        let all = try m.find(s, [(CKA_CLASS, ulong(CKO_PRIVATE_KEY))])
        if all.count == 1 { keys = all }
    }
    guard let key = keys.first else { throw NSError(domain: "P11", code: 4, userInfo: [NSLocalizedDescriptionKey: "no private key for this certificate"]) }
    let mechanism: UInt = (r["mechanism"] as? String) == "ecdsa" ? 0x1041 : 0x1
    let signature = try m.sign(s, key: key, mechanism: mechanism, data: [UInt8](data))
    return ["signature": Data(signature).base64EncodedString()]
}

func respond(_ object: [String: Any]) {
    let data = (try? JSONSerialization.data(withJSONObject: object)) ?? Data("{\"error\":\"encoding\"}".utf8)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

let input = FileHandle.standardInput.readDataToEndOfFile()
guard let request = (try? JSONSerialization.jsonObject(with: input)) as? [String: Any], let path = request["module"] as? String else {
    respond(["error": "bad request"])
    exit(2)
}
#if arch(x86_64)
let architecture = "x86_64"
#else
let architecture = "arm64"
#endif
do {
    let module = try Module(path: path)
    try module.initialize()
    defer { module.finalize() }
    var result = request["op"] as? String == "sign" ? try sign(module, request) : try list(module)
    result["arch"] = architecture
    respond(result)
} catch let e as P11Error {
    respond(["error": "\(e.call) failed", "rv": e.rv, "arch": architecture])
    exit(1)
} catch {
    respond(["error": error.localizedDescription, "arch": architecture])
    exit(1)
}
