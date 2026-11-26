import AppKit
import Kit42

/// Crash-recovery records: complete page state round trip (pixels, 1-bit kind, DPI, options, report, OCR text),
/// damaged records, and the autosave/clear lifecycle driven by the document's own dirty state.
enum ScanRecoveryAudit {
    static func spin(_ seconds: TimeInterval) { RunLoop.main.run(until: Date().addingTimeInterval(seconds)) }

    static func run() -> Int {
        var count = 0, failures = 0
        func check(_ value: Bool, _ name: String) {
            count += 1; if !value { failures += 1 }
            print(value ? "PASS" : "FAIL", "SCN recovery:", name)
        }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-recovery-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: folder) }
        let journal = RecoveryJournal(directory: folder)
        func recordFiles() -> [String] {
            ((try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []).filter { $0.hasSuffix(".k42recovery") }
        }

        // A colour page (JPEG-held base) and a 1-bit page with recognised text.
        var colour = Bitmap(width: 90, height: 60, channels: 4, fill: 240)
        for y in 10..<40 { for x in 10..<50 { let i = (y * 90 + x) * 4; colour.data[i] = 200; colour.data[i + 1] = 30; colour.data[i + 2] = 60 } }
        var gray = Bitmap(width: 70, height: 50, channels: 1, fill: 255)
        for y in 5..<20 { for x in 5..<60 { gray.data[y * 70 + x] = 0 } }
        guard let colourImage = colour.cgImage(), let grayImage = gray.cgImage(), let bits = gray.oneBitImage(threshold: 128) else {
            print("FAIL SCN recovery: fixtures"); return 1
        }
        var options = ProcessOptions(); options.enhancement = .none; options.colorMode = .bw; options.brightness = 0.25
        options.priorQuad = Quad(tl: CGPoint(x: 0.1, y: 0.1), tr: CGPoint(x: 0.9, y: 0.1), br: CGPoint(x: 0.9, y: 0.9), bl: CGPoint(x: 0.1, y: 0.9))
        var report = ProcessReport(); report.cropMethod = "edges"; report.deskewAngle = 1.5; report.rotation = 90
        report.ink = 0.2; report.outputSize = CGSize(width: 70, height: 50)
        let first = ScanPage(base: colourImage, image: colourImage, dpi: 200, options: ProcessOptions(), report: ProcessReport())
        let second = ScanPage(base: grayImage, image: bits, dpi: 300, options: options, report: report)
        let lines = [TextLine(text: "Счёт № 42", box: CGRect(x: 0.1, y: 0.2, width: 0.5, height: 0.1))]

        let doc = ScanDocument()
        check(ScanRecovery.capture(isDirty: doc.isDirty, pages: doc.pages) == nil, "an empty document produces no record")
        doc.append([first, second])
        doc.setText(second, lines)
        check(ScanRecovery.capture(isDirty: false, pages: doc.pages) == nil, "a saved document produces no record")
        do {
            guard let encode = ScanRecovery.capture(isDirty: doc.isDirty, pages: doc.pages) else { throw ScanRecovery.Failure.encode }
            let id = UUID()
            try journal.write(id: id, try encode())
            let scan = journal.scan()
            check(scan.records.map(\.id) == [id] && scan.damaged.isEmpty, "written record is read back")
            guard let record = scan.records.first else { throw ScanRecovery.Failure.decode }
            check(record.payload.title == pagesText(2), "record title gives the page count")
            let pages = try ScanRecovery.decode(record.payload)
            check(pages.count == 2, "every page is restored in order")
            if pages.count == 2 {
                let a = pages[0], b = pages[1]
                // RGB only: the padding byte of RGBX pixels is not image content.
                func rgb(_ image: CGImage) -> [UInt8] { Bitmap(image, gray: false).data.enumerated().filter { $0.offset % 4 != 3 }.map(\.element) }
                check(rgb(a.image) == rgb(colourImage), "output pixels are restored exactly")
                check(b.image.bitsPerPixel == 1 && Bitmap(b.image, gray: true).data == Bitmap(bits, gray: true).data,
                      "a 1-bit page stays 1-bit with identical pixels")
                let restoredBase = rgb(a.base), heldBase = rgb(first.base)
                let maxDiff = zip(restoredBase, heldBase).map { abs(Int($0) - Int($1)) }.max() ?? 255
                check(a.base.width == 90 && a.base.height == 60 && restoredBase.count == heldBase.count && maxDiff <= 6,
                      "the compressed geometry base is restored at its size (max channel difference \(maxDiff))")
                check(a.dpi == 200 && b.dpi == 300 && b.options == options && b.report == report && a.report == first.report,
                      "DPI, processing options and report are restored")
                check(b.text == lines && a.text == nil, "recognised text is restored; unrecognised pages stay unrecognised")
                check(Set(pages.map(\.id)).isDisjoint(with: [first.id, second.id]), "restored pages get fresh identities")
                let restored = ScanDocument()
                restored.append(pages); restored.undo.removeAllActions()
                check(restored.isDirty && !restored.undo.canUndo, "a restored scan opens unsaved, without stale Undo")
                try? b.refinish(b.options)
                check(b.image.bitsPerPixel == 1, "a restored page can be processed again from its base")
            }
            var bytes = try Data(contentsOf: record.file)
            bytes[bytes.count / 2] ^= 0x01
            try bytes.write(to: folder.appendingPathComponent(UUID().uuidString + ".k42recovery"))
            let rescan = journal.scan()
            check(rescan.records.map(\.id) == [id] && rescan.damaged.count == 1, "a damaged record is reported, not restored")
            journal.discard(rescan.damaged)
            journal.clear(id: id)
            check(recordFiles().isEmpty, "discard and clear remove damaged and valid records")
        } catch {
            check(false, "record round trip: \(error)")
        }

        // Lifecycle on the document's dirty state: edits/OCR are written after the delay; Save clears at once.
        let saver = RecoveryAutosaver(journal: journal, delay: 0.05) { ScanRecovery.capture(isDirty: doc.isDirty, pages: doc.pages) }
        doc.onChange = { saver.noteChange() }
        doc.rotate90(first)
        spin(0.01); journal.waitUntilIdle()
        check(recordFiles().isEmpty, "an edit is not written before the autosave delay")
        spin(0.25); journal.waitUntilIdle()
        check(journal.scan().records.first.flatMap { try? ScanRecovery.decode($0.payload) }?.first?.image.width == 60,
              "an unsaved page edit is written after the autosave delay")
        doc.markSaved(revision: doc.revision)
        spin(0.02); journal.waitUntilIdle()
        check(recordFiles().isEmpty, "saving clears the record at once")
        doc.setText(first, [TextLine(text: "late OCR", box: .zero)])
        spin(0.25); journal.waitUntilIdle()
        check(journal.scan().records.first.flatMap { try? ScanRecovery.decode($0.payload) }?.first?.text?.first?.text == "late OCR",
              "OCR after saving is unsaved work and is recorded")
        doc.undo.removeAllActions(); doc.markSaved(revision: doc.revision)
        spin(0.02); journal.waitUntilIdle()
        check(recordFiles().isEmpty, "saving again clears the OCR record")
        doc.remove(ids: [second.id])
        saver.flush(); journal.waitUntilIdle()
        check(recordFiles().count == 1, "flush writes unsaved pages immediately")
        saver.discard(); journal.waitUntilIdle()
        check(recordFiles().isEmpty, "closing with Don't Save (or a confirmed Quit) clears the record")

        print("SCN42 recovery checks: \(count - failures)/\(count) passed")
        return failures
    }
}

private extension ScanDocument {
    func rotate90(_ page: ScanPage) { modify([page], name: "Rotate") { $0.rotate(clockwise: 90) } }
}
