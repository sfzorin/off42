import AppKit
import CryptoKit
import ImageIO
import UniformTypeIdentifiers

/// What one editing session needs to come back after a crash: an app-defined manifest plus named binary parts.
public struct RecoveryPayload {
    public var title: String
    public var manifest: Data
    public var blobs: [String: Data]

    public init(title: String, manifest: Data, blobs: [String: Data] = [:]) {
        self.title = title
        self.manifest = manifest
        self.blobs = blobs
    }
}

/// A recovery record read back from disk; every part has been length- and SHA-256-checked.
public struct RecoveryRecord {
    public let id: UUID
    public let created: Date
    public let payload: RecoveryPayload
    public let file: URL
}

/// Crash-recovery store for apps whose windows are not NSDocuments (IMG42, SCN42). The NSDocument apps get the same
/// behaviour from AppKit autosave (`autosavingDelay = 30`, `autosavesInPlace = false`) in "Autosave Information".
///
/// One record per editing session: `<id>.k42recovery` = magic, header length, JSON header (manifest, part lengths and
/// hashes, owning process), then the parts. Records are written atomically on a serial queue, so a failed write keeps
/// the previous record and a later clear always runs after earlier writes. Damaged records are reported, never decoded.
public final class RecoveryJournal {
    public static let defaultDelay: TimeInterval = 30
    static let magic = Data("K42RECOVERY1\n".utf8)
    static let fileExtension = "k42recovery"

    public let directory: URL
    let queue = DispatchQueue(label: "kit42.recoveryjournal", qos: .utility)
    /// Called on the main thread when a write or clear fails (the previous record stays intact).
    public var onError: ((Error) -> Void)?

    public init(directory: URL) {
        self.directory = directory
    }

    /// The app's journal in ~/Library/Autosave Information/<app>/, or `K42_RECOVERY_DIR`. UI tests, native audits and
    /// debug snapshots (`--uitest`, `--audit`, `K42_SNAPSHOT`) get no journal unless `K42_RECOVERY_DIR` is set, so
    /// they can neither leave records for the user's next launch nor pick up the user's own unsaved work.
    public static func standard(app: String) -> RecoveryJournal? {
        let env = ProcessInfo.processInfo.environment
        if let dir = env["K42_RECOVERY_DIR"], !dir.isEmpty { return RecoveryJournal(directory: URL(fileURLWithPath: dir, isDirectory: true)) }
        let args = CommandLine.arguments
        guard !args.contains("--uitest"), !args.contains("--audit"), env["K42_SNAPSHOT"] == nil, env["K42_NO_RECOVERY"] == nil,
              let base = try? FileManager.default.url(for: .autosavedInformationDirectory, in: .userDomainMask,
                                                       appropriateFor: nil, create: true) else { return nil }
        return RecoveryJournal(directory: base.appendingPathComponent(app, isDirectory: true))
    }

    func file(_ id: UUID) -> URL { directory.appendingPathComponent("\(id.uuidString).\(Self.fileExtension)") }

    // MARK: Writing

    private struct Header: Codable {
        struct Part: Codable { var name: String; var length: Int; var sha256: String }
        var version = 1
        var id: UUID
        var created: Date
        var pid: Int32
        var title: String
        var manifest: Data
        var parts: [Part]
    }

    static func hex(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }

    /// Encodes and writes synchronously on the caller's thread (use `enqueueWrite` from the UI).
    public func write(id: UUID, _ payload: RecoveryPayload, created: Date = Date()) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let names = payload.blobs.keys.sorted()
        let header = Header(id: id, created: created, pid: getpid(), title: payload.title, manifest: payload.manifest,
                            parts: names.map { Header.Part(name: $0, length: payload.blobs[$0]!.count, sha256: Self.hex(payload.blobs[$0]!)) })
        let json = try JSONEncoder().encode(header)
        var data = Self.magic
        var length = UInt64(json.count).bigEndian
        data.append(Data(bytes: &length, count: 8))
        data.append(json)
        for name in names { data.append(payload.blobs[name]!) }
        try data.write(to: file(id), options: .atomic)
    }

    /// Removes the record (a missing record is not an error). Unrelated files in the directory are untouched.
    public func clear(id: UUID) {
        let url = file(id)
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        do { try FileManager.default.removeItem(at: url) } catch { report(error) }
    }

    /// Serial background write: `encode` runs off the main thread on immutable captured state.
    func enqueueWrite(id: UUID, _ encode: @escaping () throws -> RecoveryPayload) {
        queue.async { [self] in
            do { try write(id: id, try encode()) } catch { report(error) }
        }
    }

    func enqueueClear(id: UUID) { queue.async { [self] in clear(id: id) } }

    /// Waits for queued writes and clears (self-tests, termination).
    public func waitUntilIdle() { queue.sync {} }

    private func report(_ error: Error) {
        DispatchQueue.main.async { [weak self] in
            if let onError = self?.onError { onError(error) } else { NSLog("Recovery journal: %@", "\(error)") }
        }
    }

    // MARK: Reading

    /// Valid records of sessions that are no longer running (oldest first) and unreadable record files.
    public func scan() -> (records: [RecoveryRecord], damaged: [URL]) {
        let files = (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? []
        var records: [RecoveryRecord] = [], damaged: [URL] = []
        for url in files where url.pathExtension == Self.fileExtension {
            guard let (record, pid) = Self.read(url) else { damaged.append(url); continue }
            // A record of another live instance belongs to that instance, not to crash recovery.
            if pid != getpid(), kill(pid, 0) == 0 { continue }
            records.append(record)
        }
        return (records.sorted { $0.created < $1.created }, damaged)
    }

    static func read(_ url: URL) -> (RecoveryRecord, Int32)? {
        guard let data = try? Data(contentsOf: url, options: .mappedIfSafe), data.count >= magic.count + 8,
              data.prefix(magic.count) == magic else { return nil }
        let lengthBytes = data.subdata(in: magic.count..<magic.count + 8)
        let length = lengthBytes.reduce(UInt64(0)) { $0 << 8 | UInt64($1) }
        var offset = magic.count + 8
        guard length <= UInt64(data.count - offset),
              let header = try? JSONDecoder().decode(Header.self, from: data.subdata(in: offset..<offset + Int(length))),
              header.version == 1, url.deletingPathExtension().lastPathComponent == header.id.uuidString else { return nil }
        offset += Int(length)
        var blobs: [String: Data] = [:]
        for part in header.parts {
            guard part.length >= 0, part.length <= data.count - offset, blobs[part.name] == nil else { return nil }
            let blob = data.subdata(in: offset..<offset + part.length)
            guard hex(blob) == part.sha256 else { return nil }
            blobs[part.name] = blob
            offset += part.length
        }
        guard offset == data.count else { return nil }
        return (RecoveryRecord(id: header.id, created: header.created,
                               payload: RecoveryPayload(title: header.title, manifest: header.manifest, blobs: blobs), file: url), header.pid)
    }

    /// Deletes damaged record files found by `scan` (only files with the journal's own extension).
    public func discard(_ damaged: [URL]) {
        for url in damaged where url.pathExtension == Self.fileExtension && url.deletingLastPathComponent().standardizedFileURL == directory.standardizedFileURL {
            try? FileManager.default.removeItem(at: url)
        }
    }

    // MARK: Pixels

    /// Lossless TIFF (LZW): keeps bit depth (1-bit, 16-bit, float), alpha and colour space; bypasses export codecs.
    public static func encodeImage(_ image: CGImage) -> Data? {
        let data = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(data, UTType.tiff.identifier as CFString, 1, nil) else { return nil }
        let props: [CFString: Any] = [kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFCompression: 5]]
        CGImageDestinationAddImage(dest, image, props as CFDictionary)
        return CGImageDestinationFinalize(dest) ? data as Data : nil
    }

    /// Decodes a recovery image fully (not lazily backed by the record file).
    public static func decodeImage(_ data: Data) -> CGImage? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        return CGImageSourceCreateImageAtIndex(src, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary)
    }

    // MARK: Launch prompt

    /// Asks whether to restore `count` unsaved sessions after an unexpected quit. true = Restore, false = Discard.
    public static func askToRestore(appName: String, titles: [String]) -> Bool {
        let alert = NSAlert()
        alert.messageText = L("%@ quit while you had unsaved changes. Restore them?", appName)
        let list = titles.prefix(8).map { "• " + $0 }.joined(separator: "\n") + (titles.count > 8 ? "\n…" : "")
        alert.informativeText = L("Recovered work opens as unsaved. If you discard it, it cannot be recovered later.") + "\n\n" + list
        alert.addButton(withTitle: L("Restore"))
        alert.addButton(withTitle: L("Discard"))
        return alert.runModal() == .alertFirstButtonReturn
    }
}

/// Keeps one editing session's recovery record in step with its unsaved state, like AppKit autosave: changes are written
/// after `delay` (coalesced), a clean state removes the record at once. `capture` runs on the main thread and returns
/// nil when there is nothing unsaved; otherwise an encoder that runs off the main thread on the captured state.
public final class RecoveryAutosaver {
    public typealias Capture = () -> (() throws -> RecoveryPayload)?

    public let journal: RecoveryJournal?
    public private(set) var id = UUID()
    private let delay: TimeInterval
    private let capture: Capture
    private var evaluationQueued = false
    private var writeTimer: Timer?
    private var hasRecord = false

    public init(journal: RecoveryJournal?, delay: TimeInterval = RecoveryJournal.defaultDelay, capture: @escaping Capture) {
        self.journal = journal
        self.delay = ProcessInfo.processInfo.environment["K42_RECOVERY_DELAY"].flatMap(TimeInterval.init) ?? delay
        self.capture = capture
    }

    deinit { writeTimer?.invalidate() }

    /// Takes over a record restored at launch: its next write replaces it and saving/discarding removes it.
    public func adopt(_ recordID: UUID) {
        if hasRecord, recordID != id { journal?.enqueueClear(id: id) }
        id = recordID
        hasRecord = true
    }

    /// The document changed (edit, Undo/Redo, save, revert, close). Evaluated once per run-loop turn.
    public func noteChange() {
        guard journal != nil, !evaluationQueued else { return }
        evaluationQueued = true
        DispatchQueue.main.async { [weak self] in
            self?.evaluationQueued = false
            self?.evaluate()
        }
    }

    private func evaluate() {
        guard capture() != nil else { discard(); return }
        guard writeTimer == nil else { return }
        writeTimer = Timer.scheduledTimer(withTimeInterval: delay, repeats: false) { [weak self] _ in
            self?.writeTimer = nil
            self?.flush()
        }
    }

    /// Writes the current unsaved state now (or removes the record when clean).
    public func flush() {
        writeTimer?.invalidate(); writeTimer = nil
        guard let journal else { return }
        guard let encode = capture() else { discard(); return }
        hasRecord = true
        journal.enqueueWrite(id: id, encode)
    }

    /// Saved, discarded or closed: the record goes away (after any write already queued).
    public func discard() {
        writeTimer?.invalidate(); writeTimer = nil
        guard hasRecord, let journal else { return }
        hasRecord = false
        journal.enqueueClear(id: id)
    }
}
