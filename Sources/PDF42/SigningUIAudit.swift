import AppKit
import Kit42
import PDFKit

/// UI regressions for the signing workflow (`K42_SIGNING_AUDIT=1`, run by scripts/audit-ui.py): the batch
/// progress window with real files, the summary, opening a signed result in an editor window, and the new
/// Signature menu commands reaching their handlers.
enum SigningUIAudit {
    static var passed = 0, total = 0

    static func expect(_ ok: Bool, _ name: String) {
        total += 1
        if ok { passed += 1 } else { print("FAIL \(name)") }
    }

    static func run() {
        let fm = FileManager.default
        let dir = fm.temporaryDirectory.appendingPathComponent("pdf42-signing-audit-\(UUID().uuidString)")
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        for name in ["a", "b", "c"] { try? PDFSelfTest.minimalPDF().write(to: dir.appendingPathComponent("\(name).pdf")) }
        try? Data("broken".utf8).write(to: dir.appendingPathComponent("d.pdf"))
        let shots = ProcessInfo.processInfo.environment["K42_UI_TEST_DIR"] ?? dir.path
        let progressShot = (shots as NSString).appendingPathComponent("pdf-batch-progress.png")
        try? fm.removeItem(atPath: progressShot)
        setenv("K42_BATCH_SNAPSHOT", progressShot, 1)

        guard let signer = try? PDFSelfTest.TestSigner(name: "UI Audit Signer") else { finish(dir); return }
        var options = PDFSigner.Options(identity: nil, reason: "UI audit", location: "")
        options.remote = signer
        options.level = .baselineB
        var settings = BatchSigner.Settings(options: options, outputFolder: dir.appendingPathComponent("out"), suffix: "signed", visiblePage: .first)
        settings.generatedMark = .signature("UI Audit", ink: InkColor.blue.pen, design: 3)
        settings.appendCompletion = true

        // 1. Batch through the progress window.
        let files = BatchSigner.collect([dir])
        expect(files.count == 4, "batch: four PDFs collected")
        let outcomes = BatchSignUI.runWithProgress(files, settings: settings)
        expect(outcomes.count == 4 && outcomes.filter(\.succeeded).count == 3, "batch: three signed, one failure reported")
        expect(fm.fileExists(atPath: progressShot) && ((try? fm.attributesOfItem(atPath: progressShot)[.size] as? Int) ?? 0) > 1000,
               "batch: progress window was drawn")
        expect(!NSApp.windows.contains { $0.title == L("Signing Files") && $0.isVisible }, "batch: progress window closed")
        let summary = BatchSignUI.summaryAlert(outcomes, total: files.count)
        expect(summary.messageText == L("Signed %d of %d files", 3, 4), "summary: title counts")
        expect(((summary.accessoryView as? NSScrollView)?.documentView as? NSTextView)?.string.contains("d.pdf") == true, "summary: failure listed")
        for o in outcomes where o.succeeded {
            guard let url = o.output, let data = try? Data(contentsOf: url) else { expect(false, "batch: output readable"); continue }
            let infos = PDFSignatureVerifier.verify(data)
            expect(infos.count == 1 && infos[0].status != .invalid, "batch: \(url.lastPathComponent) signature intact")
            expect(CompletionCertificate.check(data)?.matches == true, "batch: \(url.lastPathComponent) completion certificate matches")
        }

        // 2. A signed result in an editor window: signature banner and the new commands.
        guard let first = outcomes.first(where: \.succeeded)?.output else { finish(dir); return }
        NSDocumentController.shared.openDocument(withContentsOf: first, display: true) { document, _, _ in
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
                let editor = (document as? PDFDoc)?.windowControllers.first as? EditorWindowController
                expect(editor != nil, "editor: signed copy opens")
                expect(editor?.pdfDoc?.signatures.count == 1, "editor: one signature found")
                expect(editor?.pdf?.pageCount == 2, "editor: certificate page appended")
                editor?.window?.makeKeyAndOrderFront(nil)
                for selector in [#selector(EditorWindowController.exportVerificationReportAction(_:)),
                                 #selector(EditorWindowController.addCompletionCertificateAction(_:)),
                                 #selector(EditorWindowController.verifyCompletionCertificateAction(_:)),
                                 #selector(AppDelegate.batchSignAction(_:))] {
                    expect(menuItem(NSApp.mainMenu, action: selector) != nil, "menu: \(selector) present")
                    // The audit app may run in the background (no key window): check the handler directly.
                    let handler: NSObject? = selector == #selector(AppDelegate.batchSignAction(_:)) ? NSApp.delegate as? NSObject : editor
                    expect(handler?.responds(to: selector) == true, "menu: \(selector) handled")
                }
                // Verify Certificate of Completion runs and reports a match in a sheet.
                editor?.verifyCompletionCertificateAction(nil)
                let sheet = editor?.window?.attachedSheet
                let text = sheet?.contentView.map(allText) ?? ""
                expect(text.contains(L("The certificate of completion matches")), "verify completion: sheet reports a match")
                if let sheet { editor?.window?.endSheet(sheet) }
                if let data = editor?.pdfDoc?.originalData {
                    let model = VerificationReport.model(data, fileName: first.lastPathComponent)
                    expect(model.completion?.matches == true && model.signatures.first?.info.signer == "UI Audit Signer",
                           "report: signer and certificate of completion")
                }
                finish(dir)
            }
        }
    }

    static func allText(_ view: NSView) -> String {
        ((view as? NSTextField)?.stringValue ?? "") + view.subviews.map(allText).joined(separator: "\n")
    }

    static func menuItem(_ menu: NSMenu?, action: Selector) -> NSMenuItem? {
        for item in menu?.items ?? [] {
            if item.action == action { return item }
            if let found = menuItem(item.submenu, action: action) { return found }
        }
        return nil
    }

    static func finish(_ dir: URL) {
        try? FileManager.default.removeItem(at: dir)
        print("PDF42 signing UI regressions: \(passed)/\(total) passed")
        fflush(stdout)
        exit(passed == total && total > 0 ? 0 : 1)
    }
}
