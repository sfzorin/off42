import AppKit
import Kit42

/// Crash recovery for a window's unsaved scan, on Kit42's `RecoveryJournal`: every page's geometry-corrected base
/// (held JPEG-compressed in memory, so it is re-encoded as maximum-quality JPEG: near-lossless, not bit-exact), output pixels (lossless TIFF, 1-bit kept), DPI, processing options,
/// report and recognised text. Undo history and selection are not kept; restored pages open as one unsaved document.
enum ScanRecovery {
    static let journal = RecoveryJournal.standard(app: "SCN42")

    struct PageRecord: Codable {
        var dpi: Double
        var options: ProcessOptions
        var report: ProcessReport
        var text: [TextLine]?
        /// ImageIO reads a 1-bit TIFF back as 8-bit gray; such pages are packed to 1 bit again (same pixels).
        var oneBitBase: Bool
        var oneBitImage: Bool
    }

    static func packed(_ image: CGImage, oneBit: Bool) -> CGImage? {
        oneBit && image.bitsPerPixel != 1 ? Bitmap(image, gray: true).oneBitImage(threshold: 128) : image
    }

    struct Manifest: Codable {
        var version = 1
        var pages: [PageRecord]
    }

    enum Failure: Error { case encode, decode }

    /// nil when there is nothing unsaved; otherwise an encoder of the captured page snapshots for a worker thread.
    static func capture(isDirty: Bool, pages: [ScanPage]) -> (() throws -> RecoveryPayload)? {
        guard isDirty, !pages.isEmpty else { return nil }
        let snapshots = pages.map(\.snapshot)
        return { try encode(snapshots) }
    }

    static func encode(_ pages: [ScanPage.Snapshot]) throws -> RecoveryPayload {
        var blobs: [String: Data] = [:], records: [PageRecord] = []
        for (i, page) in pages.enumerated() {
            // The base is held JPEG-compressed (ScanPage.compact); 1-bit bases are not, and go to lossless TIFF.
            let base = page.base.bitsPerPixel > 1 ? Exporter.jpegData(page.base, quality: 1, dpi: 300) : RecoveryJournal.encodeImage(page.base)
            guard let base, let image = RecoveryJournal.encodeImage(page.image) else { throw Failure.encode }
            blobs["base\(i)"] = base
            blobs["image\(i)"] = image
            records.append(PageRecord(dpi: page.dpi, options: page.options, report: page.report, text: page.text,
                                      oneBitBase: page.base.bitsPerPixel == 1, oneBitImage: page.image.bitsPerPixel == 1))
        }
        let title = pagesText(pages.count)
        return RecoveryPayload(title: title, manifest: try JSONEncoder().encode(Manifest(pages: records)), blobs: blobs)
    }

    static func decode(_ payload: RecoveryPayload) throws -> [ScanPage] {
        let manifest = try JSONDecoder().decode(Manifest.self, from: payload.manifest)
        guard manifest.version == 1, !manifest.pages.isEmpty else { throw Failure.decode }
        return try manifest.pages.enumerated().map { i, r in
            guard r.dpi.isFinite, r.dpi > 0, r.options.isValid,
                  let baseData = payload.blobs["base\(i)"], let imageData = payload.blobs["image\(i)"],
                  let decoded = RecoveryJournal.decodeImage(imageData), let image = packed(decoded, oneBit: r.oneBitImage)
            else { throw Failure.decode }
            let base: CGImage?
            if baseData.starts(with: [0xFF, 0xD8]), let provider = CGDataProvider(data: baseData as CFData) {
                base = CGImage(jpegDataProviderSource: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
            } else {
                base = RecoveryJournal.decodeImage(baseData).flatMap { packed($0, oneBit: r.oneBitBase) }
            }
            guard let base else { throw Failure.decode }
            return ScanPage(restoring: ScanPage.Snapshot(base: base, image: image, dpi: r.dpi, options: r.options, report: r.report, text: r.text))
        }
    }
}

extension ScanWindowController {
    func recoveryCapture() -> (() throws -> RecoveryPayload)? {
        ScanRecovery.capture(isDirty: doc.isDirty, pages: doc.pages)
    }

    /// Recovered pages become this (empty) window's unsaved document; the record now belongs to this window.
    func adoptRecovered(_ pages: [ScanPage], recordID: UUID) {
        doc.append(pages)
        doc.undo.removeAllActions()
        recovery.adopt(recordID)
        recovery.noteChange()
        pageList.select([0])
        banner.show(.info, L("Recovered Changes"))
    }
}

extension AppDelegate {
    /// After an unexpected quit: offers the unsaved scans of the previous run, then restores or discards them.
    func offerRecovery(into first: ScanWindowController?) {
        guard let journal = ScanRecovery.journal else { return }
        let (records, damaged) = journal.scan()
        journal.discard(damaged)
        guard !records.isEmpty else { return }
        let restore = RecoveryJournal.askToRestore(appName: "SCN42", titles: records.map(\.payload.title))
        var target = first?.doc.isEmpty == true ? first : nil
        for record in records {
            guard restore, let pages = try? ScanRecovery.decode(record.payload) else { journal.clear(id: record.id); continue }
            let w = target ?? newWindow()
            target = nil
            w.adoptRecovered(pages, recordID: record.id)
        }
    }
}
