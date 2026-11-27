import Foundation
import Darwin
import Kit42

/// Bounded wire-level reader for modern iWork IWA records. No files are extracted.
enum KeynoteArchiveError: Error { case malformed, limit, unsupported }

struct KeynoteMessage {
    enum Value { case integer(UInt64), fixed64(UInt64), bytes(Data), fixed32(UInt32) }
    let fields: [Int: [Value]]
    init(_ data: Data) throws {
        var cursor = KeynoteCursor(data), fields: [Int: [Value]] = [:]
        while !cursor.done {
            let tag = try cursor.varint(), number = tag >> 3
            guard number > 0, number <= 536_870_911 else { throw KeynoteArchiveError.malformed }
            let value: Value
            switch tag & 7 {
            case 0: value = .integer(try cursor.varint())
            case 1: value = .fixed64(try cursor.fixed(8))
            case 2: value = .bytes(try cursor.take(try cursor.length()))
            case 5: value = .fixed32(UInt32(try cursor.fixed(4)))
            default: throw KeynoteArchiveError.unsupported
            }
            fields[Int(number), default: []].append(value)
        }
        self.fields = fields
    }
    func values(_ field: Int) -> [Value] { fields[field] ?? [] }
    func integer(_ field: Int) -> UInt64? { if case let .integer(v)? = fields[field]?.first { return v }; return nil }
    func bytes(_ field: Int) -> Data? { if case let .bytes(v)? = fields[field]?.first { return v }; return nil }
    func messages(_ field: Int) throws -> [KeynoteMessage] {
        try values(field).map { value in guard case let .bytes(data) = value else { throw KeynoteArchiveError.malformed }; return try KeynoteMessage(data) }
    }
}

private struct KeynoteCursor {
    let bytes: [UInt8]
    var offset = 0
    init(_ data: Data) { bytes = Array(data) }
    var done: Bool { offset == bytes.count }
    mutating func take(_ n: Int) throws -> Data {
        guard n >= 0, n <= bytes.count - offset else { throw KeynoteArchiveError.malformed }
        defer { offset += n }; return Data(bytes[offset ..< offset + n])
    }
    mutating func byte() throws -> UInt8 { guard offset < bytes.count else { throw KeynoteArchiveError.malformed }; defer { offset += 1 }; return bytes[offset] }
    mutating func varint() throws -> UInt64 {
        var v: UInt64 = 0
        for i in 0..<10 {
            let b = try byte()
            if i == 9 && b > 1 { throw KeynoteArchiveError.malformed }
            v |= UInt64(b & 127) << (i * 7)
            if b & 128 == 0 { return v }
        }
        throw KeynoteArchiveError.malformed
    }
    mutating func length() throws -> Int {
        let n = try varint(); guard n <= UInt64(Int.max) else { throw KeynoteArchiveError.limit }; return Int(n)
    }
    mutating func fixed(_ n: Int) throws -> UInt64 {
        let d = try take(n); return d.enumerated().reduce(0) { $0 | (UInt64($1.element) << ($1.offset * 8)) }
    }
}

struct KeynoteObject {
    let id: UInt64
    let type: UInt64
    let payload: Data
    let additionalPayloads: [Data]
    func message() throws -> KeynoteMessage { try KeynoteMessage(payload) }
}

struct KeynoteArchive {
    static let maxBytes = 256 * 1024 * 1024
    private let files: [String: Data]
    let objects: [UInt64: KeynoteObject]

    init(url: URL) throws {
        let values = try url.resourceValues(forKeys: [.isDirectoryKey, .fileSizeKey])
        if values.isDirectory == true { try self.init(directory: url); return }
        guard let size = values.fileSize, size >= 0, size <= Self.maxBytes else { throw KeynoteArchiveError.limit }
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var data = Data()
        while data.count <= Self.maxBytes {
            let chunk = try handle.read(upToCount: min(1024 * 1024, Self.maxBytes + 1 - data.count)) ?? Data()
            if chunk.isEmpty { break }
            data.append(chunk)
        }
        guard data.count <= Self.maxBytes, data.count == size else { throw KeynoteArchiveError.limit }
        try self.init(data: data)
    }
    init(data: Data) throws {
        guard data.count >= 22, data.count <= Self.maxBytes else { throw KeynoteArchiveError.limit }
        try Self.checkZipSizes(data)
        let zip = try ZipArchive(data: data)
        try self.init(files: zip.entries)
    }
    init(directory: URL) throws {
        let fm = FileManager.default
        guard let resolved = realpath(directory.path, nil) else { throw KeynoteArchiveError.malformed }
        let canonicalPath = String(cString: resolved)
        free(resolved)
        let base = URL(fileURLWithPath: canonicalPath, isDirectory: true)
        guard let walker = fm.enumerator(at: base, includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey], options: []) else { throw KeynoteArchiveError.malformed }
        var files: [String: Data] = [:], total = 0
        for case let url as URL in walker {
            let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
            guard values.isSymbolicLink != true else { throw KeynoteArchiveError.malformed }
            guard values.isRegularFile == true else { continue }
            let prefix = canonicalPath + "/"
            guard url.path.hasPrefix(prefix) else { throw KeynoteArchiveError.malformed }
            let path = String(url.path.dropFirst(prefix.count))
            guard Self.safe(path), let size = values.fileSize, size <= Self.maxBytes - total else { throw KeynoteArchiveError.limit }
            let data = try Data(contentsOf: url); total += data.count
            guard total <= Self.maxBytes else { throw KeynoteArchiveError.limit }
            files[path] = data
        }
        try self.init(files: files)
    }
    init(files: [String: Data], maxDecodedBytes: Int = KeynoteArchive.maxBytes) throws {
        guard maxDecodedBytes > 0, maxDecodedBytes <= Self.maxBytes else { throw KeynoteArchiveError.limit }
        var total = 0, decoded = 0, records: [UInt64: KeynoteObject] = [:]
        guard files.keys.allSatisfy({ Self.safe($0.hasSuffix("/") ? String($0.dropLast()) : $0) }), files["Index/Document.iwa"] != nil else { throw KeynoteArchiveError.malformed }
        for (path, data) in files where path.hasPrefix("Index/") && path.hasSuffix(".iwa") {
            total += data.count
            guard total <= Self.maxBytes else { throw KeynoteArchiveError.limit }
            let stream = try Self.decompress(data, limit: maxDecodedBytes - decoded)
            decoded += stream.count
            var cursor = KeynoteCursor(stream)
            while !cursor.done {
                let header = try KeynoteMessage(cursor.take(cursor.length()))
                guard let id = header.integer(1), id > 0 else { throw KeynoteArchiveError.malformed }
                let infos = try header.messages(2)
                guard !infos.isEmpty else { throw KeynoteArchiveError.malformed }
                var payloads: [Data] = []
                var objectType: UInt64?
                for info in infos {
                    guard let type = info.integer(1), let length = info.integer(3), length <= UInt64(Self.maxBytes) else { throw KeynoteArchiveError.malformed }
                    if let prior = objectType, prior != type { throw KeynoteArchiveError.unsupported }
                    objectType = type
                    payloads.append(try cursor.take(Int(length)))
                }
                guard records.count < 1_000_000, records[id] == nil, let type = objectType, let first = payloads.first else { throw KeynoteArchiveError.malformed }
                records[id] = KeynoteObject(id: id, type: type, payload: first, additionalPayloads: Array(payloads.dropFirst()))
            }
        }
        self.files = files; self.objects = records
    }
    func file(_ path: String) -> Data? { Self.safe(path) ? files[path] : nil }
    func dataFile(id: UInt64) -> Data? {
        guard let index = objects[2], let metadata = try? index.message(), let entries = try? metadata.messages(4) else { return nil }
        for entry in entries where entry.integer(1) == id {
            for field in [4, 3] {
                guard let raw = entry.bytes(field), let name = String(data: raw, encoding: .utf8), let data = file("Data/" + name) else { continue }
                return data
            }
        }
        return nil
    }
    // Check advertised sizes before the shared ZIP reader allocates inflated entries.
    private static func checkZipSizes(_ data: Data) throws {
        let b = [UInt8](data)
        func u16(_ i: Int) -> Int { Int(b[i]) | Int(b[i + 1]) << 8 }
        func u32(_ i: Int) -> Int { u16(i) | u16(i + 2) << 16 }
        var end = b.count - 22
        let first = max(0, end - 65535)
        while end >= first {
            if u32(end) == 0x06054b50 && end + 22 + u16(end + 20) == b.count { break }
            end -= 1
        }
        guard end >= first else { throw KeynoteArchiveError.malformed }
        let count = u16(end + 10), start = u32(end + 16), size = u32(end + 12)
        guard start <= end, size <= end - start else { throw KeynoteArchiveError.malformed }
        var p = start, total = 0
        for _ in 0..<count {
            guard p <= end - 46, u32(p) == 0x02014b50 else { throw KeynoteArchiveError.malformed }
            let n = u32(p + 24), recordSize = 46 + u16(p + 28) + u16(p + 30) + u16(p + 32)
            guard n <= maxBytes - total, recordSize <= end - p else { throw KeynoteArchiveError.limit }
            total += n; p += recordSize
        }
        guard p == start + size else { throw KeynoteArchiveError.malformed }
    }
    private static func safe(_ path: String) -> Bool {
        !path.isEmpty && !path.hasPrefix("/") && !path.contains("\\") && !path.split(separator: "/", omittingEmptySubsequences: false).contains { $0.isEmpty || $0 == "." || $0 == ".." }
    }
    static func decompress(_ data: Data, limit: Int = maxBytes) throws -> Data {
        var input = KeynoteCursor(data), result = Data()
        while !input.done {
            let kind = try input.byte(), length = Int(try input.fixed(3))
            guard kind == 0 else { throw KeynoteArchiveError.unsupported }
            let block = try snappy(input.take(length), limit: limit - result.count)
            guard block.count <= limit - result.count else { throw KeynoteArchiveError.limit }
            result.append(block)
        }
        return result
    }
    private static func snappy(_ data: Data, limit: Int) throws -> Data {
        var cursor = KeynoteCursor(data), out: [UInt8] = []
        let expected = try cursor.length()
        guard expected <= limit else { throw KeynoteArchiveError.limit }
        out.reserveCapacity(expected)
        while !cursor.done {
            let tag = try cursor.byte(), mode = tag & 3
            if mode == 0 {
                var length = Int(tag >> 2)
                if length >= 60 { length = Int(try cursor.fixed(length - 59)) }
                guard length < expected - out.count else { throw KeynoteArchiveError.malformed }
                out.append(contentsOf: try cursor.take(length + 1))
            } else {
                let length: Int, distance: Int
                if mode == 1 { length = 4 + Int((tag >> 2) & 7); distance = Int(tag & 0xe0) << 3 | Int(try cursor.byte()) }
                else { length = 1 + Int(tag >> 2); distance = Int(try cursor.fixed(mode == 2 ? 2 : 4)) }
                guard distance > 0, distance <= out.count, length <= expected - out.count else { throw KeynoteArchiveError.malformed }
                for _ in 0..<length { out.append(out[out.count - distance]) }
            }
        }
        guard out.count == expected else { throw KeynoteArchiveError.malformed }
        return Data(out)
    }
}
