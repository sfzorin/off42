import AppKit
import Kit42

/// Crash-recovery records: complete editing state round trip, damaged records, and the autosave/clear lifecycle.
enum ImageRecoverySelfTest {
    static func rgba16(_ width: Int, _ height: Int, _ draw: (CGContext) -> Void) -> CGImage? {
        guard let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 16, bytesPerRow: width * 8,
                                  space: CGColorSpace(name: CGColorSpace.displayP3)!,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder16Little.rawValue) else { return nil }
        draw(ctx)
        return ctx.makeImage()
    }

    /// Full 16-bit samples in one fixed layout, so differently stored but equal pixels compare equal.
    static func samples(_ image: CGImage) -> [UInt16] {
        guard let ctx = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 16, bytesPerRow: image.width * 8,
                                  space: CGColorSpace(name: CGColorSpace.displayP3)!,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder16Little.rawValue) else { return [] }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        let p = ctx.data!.bindMemory(to: UInt16.self, capacity: image.width * image.height * 4)
        return Array(UnsafeBufferPointer(start: p, count: image.width * image.height * 4))
    }

    static func spin(_ seconds: TimeInterval) { RunLoop.main.run(until: Date().addingTimeInterval(seconds)) }

    static func run() -> Int {
        var count = 0, failures = 0
        func check(_ condition: Bool, _ name: String) { count += 1; if !condition { failures += 1 }; print(condition ? "PASS" : "FAIL", "IMG recovery:", name) }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("IMG42-recovery-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: folder) }
        let journal = RecoveryJournal(directory: folder)
        func recordFiles() -> [String] {
            ((try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []).filter { $0.hasSuffix(".k42recovery") }
        }

        // A 16-bit wide-gamut base with a fine gradient that 8-bit storage would lose.
        guard let base = rgba16(37, 23, { ctx in
            for x in 0..<37 {
                ctx.setFillColor(CGColor(red: CGFloat(x) / 36 * 0.01 + 0.5, green: 0.25, blue: 0.75, alpha: 0.8))
                ctx.fill(CGRect(x: x, y: 0, width: 1, height: 23))
            }
        }) else { print("FAIL IMG recovery: fixture"); return 1 }
        let stamp = ImageFileSelfTest.fixture(width: 12, height: 8, transparent: true)
        var pen = Mark(kind: .pen)
        pen.color = .systemBlue; pen.width = 7.5; pen.dashed = true; pen.points = [CGPoint(x: 1.25, y: 2), CGPoint(x: 30, y: 19.5)]
        var text = Mark(kind: .text)
        text.text = "Подпись ✓"; text.fill = NSColor(srgbRed: 1, green: 1, blue: 0, alpha: 0.4); text.rect = CGRect(x: 3, y: 4, width: 20, height: 9)
        text.fontName = "Menlo"; text.fontSize = 13; text.bold = true; text.underline = true; text.alignment = .center
        text.textColor = NSColor(calibratedRed: 0.1, green: 0.2, blue: 0.3, alpha: 1)
        var image = Mark(kind: .image)
        image.image = stamp; image.rect = CGRect(x: 10, y: 10, width: 12, height: 8); image.shadow = true
        var adjustments = Adjustments()
        adjustments[.exposure] = 12; adjustments[.vignette] = 40; adjustments.look = .sepia; adjustments.autoEnhance = true
        let state = EditState(base: base, marks: [pen, text, image], adjustments: adjustments, imageDescription: "Caption «x»", dpi: 240)
        let source = URL(fileURLWithPath: "/tmp/IMG42 recovery/photo.tiff")

        check(ImageRecovery.capture(isDirty: false, url: source, page: 0, state: state) == nil, "a clean image produces no record")
        check(ImageRecovery.capture(isDirty: true, url: nil, page: 0, state: state) == nil, "an image without a file produces no record")
        do {
            guard let encode = ImageRecovery.capture(isDirty: true, url: source, page: 2, state: state) else { throw ImageRecovery.Failure.encode }
            let id = UUID()
            try journal.write(id: id, try encode())
            let scan = journal.scan()
            check(scan.records.count == 1 && scan.damaged.isEmpty && scan.records.first?.id == id, "written record is read back")
            guard let record = scan.records.first else { throw ImageRecovery.Failure.decode }
            check(record.payload.title == "photo.tiff [3]", "record title names the file and page")
            let r = try ImageRecovery.decode(record.payload)
            check(r.url == source && r.page == 2, "original path and page are restored")
            if let restored = r.state.base {
                check(restored.bitsPerComponent == 16 && restored.width == 37 && restored.height == 23, "16-bit working pixels keep depth and size")
                check(samples(restored) == samples(base), "working pixels are restored exactly")
            } else { check(false, "working pixels are restored") }
            check(r.state.marks.count == 3, "all marks are restored")
            for (a, b) in zip(r.state.marks, state.marks) {
                var a = a
                if let restoredImage = a.image, let original = b.image {
                    check(samples(restoredImage) == samples(original), "stamp image of a mark is restored exactly")
                    a.image = original
                }
                check(a.hasSameContent(as: b), "mark \(b.kind.rawValue) keeps identity, geometry, colours and text attributes")
            }
            check(r.state.adjustments == adjustments, "adjustments, look and Auto Enhance are restored")
            check(r.state.imageDescription == "Caption «x»" && r.state.dpi == 240, "description and resolution are restored")

            // A changed byte or an unrelated file is never decoded or deleted by mistake.
            var bytes = try Data(contentsOf: record.file)
            bytes[bytes.count - 5] ^= 0x40
            let corrupt = folder.appendingPathComponent(UUID().uuidString + ".k42recovery")
            try bytes.write(to: corrupt)
            try Data("garbage".utf8).write(to: folder.appendingPathComponent(UUID().uuidString + ".k42recovery"))
            let unrelated = folder.appendingPathComponent("notes.txt")
            try Data("keep".utf8).write(to: unrelated)
            let second = journal.scan()
            check(second.records.map(\.id) == [id] && second.damaged.count == 2, "damaged records are reported, not restored")
            journal.discard(second.damaged)
            check(recordFiles() == ["\(id.uuidString).k42recovery"] && FileManager.default.fileExists(atPath: unrelated.path),
                  "discarding damaged records keeps valid records and unrelated files")
            journal.clear(id: id)
            check(recordFiles().isEmpty, "clearing removes the record")

            // Missing original: the recovered pixels are written to a file that can be opened and saved elsewhere.
            let copy = try ImageRecovery.materialize(r, in: journal)
            let reloaded = ImageLoader.load(copy)
            check(reloaded.error == nil && reloaded.image.map { samples($0) == samples(base) } == true,
                  "a recovered image whose original is gone is materialized losslessly")
        } catch {
            check(false, "record round trip: \(error)")
        }

        // Lifecycle: an edit is written after the autosave delay; Save (clean) and Close remove the record.
        var dirty = false
        let saver = RecoveryAutosaver(journal: journal, delay: 0.05) {
            ImageRecovery.capture(isDirty: dirty, url: source, page: 0, state: state)
        }
        saver.noteChange(); spin(0.2); journal.waitUntilIdle()
        check(recordFiles().isEmpty, "an unedited image writes nothing")
        dirty = true
        saver.noteChange(); spin(0.01); journal.waitUntilIdle()
        check(recordFiles().isEmpty, "an edit is not written before the autosave delay")
        spin(0.25); journal.waitUntilIdle()
        check(recordFiles() == ["\(saver.id.uuidString).k42recovery"], "an unsaved edit is written after the autosave delay")
        dirty = false
        saver.noteChange(); spin(0.02); journal.waitUntilIdle()
        check(recordFiles().isEmpty, "saving (clean state) clears the record at once")
        dirty = true
        saver.flush(); journal.waitUntilIdle()
        check(recordFiles().count == 1, "flush writes an unsaved edit immediately")
        saver.discard(); journal.waitUntilIdle()
        check(recordFiles().isEmpty, "closing (Don't Save / confirmed Quit) clears the record")

        // A restored record is adopted: the next write replaces it, saving removes it.
        let previous = UUID()
        try? journal.write(id: previous, RecoveryPayload(title: "previous", manifest: Data("{}".utf8)))
        saver.adopt(previous)
        saver.flush(); journal.waitUntilIdle()
        check(recordFiles() == ["\(previous.uuidString).k42recovery"] && journal.scan().records.first?.payload.title == "photo.tiff",
              "an adopted record is replaced, not duplicated")
        dirty = false
        saver.noteChange(); spin(0.02); journal.waitUntilIdle()
        check(recordFiles().isEmpty, "saving a restored image clears its record")

        print("IMG recovery self-test: \(count - failures)/\(count) passed")
        return failures
    }
}
