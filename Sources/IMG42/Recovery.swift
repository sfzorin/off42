import AppKit
import Kit42

/// Crash recovery for one window's unsaved edits, on Kit42's `RecoveryJournal`: the working pixels (lossless TIFF),
/// marks (stamp images as parts), adjustments, description and resolution of the image/page being edited.
/// Pixel selection and Undo history are not kept; a restored image opens edited, with one Undo back to the file.
enum ImageRecovery {
    static let journal = RecoveryJournal.standard(app: "IMG42")

    struct Manifest: Codable {
        var version = 1
        var path: String
        var page: Int
        var hasBase: Bool
        var marks: [MarkRecord]
        var adjustments: [String: Double]
        var look: Int
        var autoEnhance: Bool
        var imageDescription: String?
        var dpi: Double
    }

    struct MarkRecord: Codable {
        var id: UUID, kind: String
        var color: Data, fill: Data?
        var width: Double, dashed: Bool, shadow: Bool, arrowStart: Bool, arrowEnd: Bool
        var points: [CGPoint], rect: CGRect
        var text: String, fontName: String, fontSize: Double
        var bold: Bool, italic: Bool, underline: Bool, alignment: Int
        var textColor: Data, hasImage: Bool, zoom: Double
    }

    struct Restored {
        var url: URL
        var page: Int
        var state: EditState
    }

    enum Failure: Error { case encode, decode }

    static func archive(_ color: NSColor) throws -> Data {
        try NSKeyedArchiver.archivedData(withRootObject: color, requiringSecureCoding: true)
    }

    static func unarchive(_ data: Data) throws -> NSColor {
        guard let color = try NSKeyedUnarchiver.unarchivedObject(ofClass: NSColor.self, from: data) else { throw Failure.decode }
        return color
    }

    /// nil when there is nothing unsaved; otherwise an encoder of the captured (immutable) state for a worker thread.
    static func capture(isDirty: Bool, url: URL?, page: Int, state: EditState) -> (() throws -> RecoveryPayload)? {
        guard isDirty, let url, state.base != nil else { return nil }
        return { try encode(url: url, page: page, state: state) }
    }

    static func encode(url: URL, page: Int, state: EditState) throws -> RecoveryPayload {
        var blobs: [String: Data] = [:]
        if let base = state.base {
            guard let data = RecoveryJournal.encodeImage(base) else { throw Failure.encode }
            blobs["base"] = data
        }
        var marks: [MarkRecord] = []
        for (i, m) in state.marks.enumerated() {
            if let image = m.image {
                guard let data = RecoveryJournal.encodeImage(image) else { throw Failure.encode }
                blobs["mark\(i)"] = data
            }
            marks.append(MarkRecord(id: m.id, kind: m.kind.rawValue, color: try archive(m.color), fill: try m.fill.map(archive),
                                    width: Double(m.width), dashed: m.dashed, shadow: m.shadow, arrowStart: m.arrowStart,
                                    arrowEnd: m.arrowEnd, points: m.points, rect: m.rect, text: m.text, fontName: m.fontName,
                                    fontSize: Double(m.fontSize), bold: m.bold, italic: m.italic, underline: m.underline,
                                    alignment: m.alignment.rawValue, textColor: try archive(m.textColor), hasImage: m.image != nil,
                                    zoom: Double(m.zoom)))
        }
        let a = state.adjustments
        let manifest = Manifest(path: url.path, page: page, hasBase: state.base != nil, marks: marks,
                                adjustments: Dictionary(uniqueKeysWithValues: a.values.map { ($0.key.rawValue, $0.value) }),
                                look: a.look.rawValue, autoEnhance: a.autoEnhance, imageDescription: state.imageDescription,
                                dpi: Double(state.dpi))
        let title = page > 0 ? "\(url.lastPathComponent) [\(page + 1)]" : url.lastPathComponent
        return RecoveryPayload(title: title, manifest: try JSONEncoder().encode(manifest), blobs: blobs)
    }

    static func decode(_ payload: RecoveryPayload) throws -> Restored {
        let m = try JSONDecoder().decode(Manifest.self, from: payload.manifest)
        guard m.version == 1, m.page >= 0, m.dpi.isFinite, m.dpi > 0 else { throw Failure.decode }
        var base: CGImage?
        if m.hasBase {
            guard let data = payload.blobs["base"], let image = RecoveryJournal.decodeImage(data) else { throw Failure.decode }
            base = image
        }
        var marks: [Mark] = []
        for (i, r) in m.marks.enumerated() {
            guard let kind = Mark.Kind(rawValue: r.kind), let alignment = NSTextAlignment(rawValue: r.alignment) else { throw Failure.decode }
            var mark = Mark(kind: kind)
            mark.id = r.id
            mark.color = try unarchive(r.color)
            mark.fill = try r.fill.map(unarchive)
            mark.width = CGFloat(r.width); mark.dashed = r.dashed; mark.shadow = r.shadow
            mark.arrowStart = r.arrowStart; mark.arrowEnd = r.arrowEnd
            mark.points = r.points; mark.rect = r.rect
            mark.text = r.text; mark.fontName = r.fontName; mark.fontSize = CGFloat(r.fontSize)
            mark.bold = r.bold; mark.italic = r.italic; mark.underline = r.underline; mark.alignment = alignment
            mark.textColor = try unarchive(r.textColor)
            mark.zoom = CGFloat(r.zoom)
            if r.hasImage {
                guard let data = payload.blobs["mark\(i)"], let image = RecoveryJournal.decodeImage(data) else { throw Failure.decode }
                mark.image = image
            }
            marks.append(mark)
        }
        var adjustments = Adjustments()
        for (key, value) in m.adjustments {
            guard let k = Adjustments.Key(rawValue: key) else { throw Failure.decode }
            adjustments[k] = value
        }
        adjustments.look = Adjustments.Look(rawValue: m.look) ?? .none
        adjustments.autoEnhance = m.autoEnhance
        return Restored(url: URL(fileURLWithPath: m.path), page: m.page,
                        state: EditState(base: base, marks: marks, adjustments: adjustments,
                                         imageDescription: m.imageDescription, dpi: CGFloat(m.dpi)))
    }

    /// Where a recovered image whose original file is gone is written, so that it can be opened and saved elsewhere.
    static func materialize(_ restored: Restored, in journal: RecoveryJournal) throws -> URL {
        guard let base = restored.state.base, let data = RecoveryJournal.encodeImage(base) else { throw Failure.encode }
        let folder = journal.directory.appendingPathComponent("Recovered Files", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let stem = restored.url.deletingPathExtension().lastPathComponent
        var url = folder.appendingPathComponent("\(stem) (\(L("Recovered"))).tiff")
        var n = 2
        while FileManager.default.fileExists(atPath: url.path) {
            url = folder.appendingPathComponent("\(stem) (\(L("Recovered")) \(n)).tiff"); n += 1
        }
        try data.write(to: url, options: .atomic)
        return url
    }
}

extension ImageWindowController {
    func recoveryCapture() -> (() throws -> RecoveryPayload)? {
        ImageRecovery.capture(isDirty: isDirty, url: url, page: page, state: state)
    }

    /// Applies a recovered editing state once its image has loaded; the record now belongs to this window.
    func applyRecovered(_ restored: ImageRecovery.Restored, recordID: UUID) {
        registerUndo(state, L("Recovered Changes"))
        restore(restored.state)
        canvas.document.showsCheckerboard = canvas.document.showsCheckerboard || restored.state.base.map { $0.alphaInfo != .none } ?? false
        recovery.adopt(recordID)
        recovery.noteChange()
        updateSubtitle()
    }
}

extension AppDelegate {
    /// After an unexpected quit: offers the unsaved images of the previous run, then restores or discards them.
    func offerRecovery() {
        guard let journal = ImageRecovery.journal else { return }
        let (records, damaged) = journal.scan()
        journal.discard(damaged)
        guard !records.isEmpty else { return }
        let restore = RecoveryJournal.askToRestore(appName: "IMG42", titles: records.map(\.payload.title))
        for record in records {
            guard restore, let restored = try? ImageRecovery.decode(record.payload) else { journal.clear(id: record.id); continue }
            var target = restored
            if !FileManager.default.fileExists(atPath: restored.url.path) {
                guard let copy = try? ImageRecovery.materialize(restored, in: journal) else { continue }
                target.url = copy; target.page = 0
            }
            openRecovered(target, recordID: record.id)
        }
    }
}
