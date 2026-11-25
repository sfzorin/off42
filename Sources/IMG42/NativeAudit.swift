import AppKit
import Kit42

/// Isolated native scenarios run by `IMG42 --audit NAME`.
@MainActor enum ImageNativeAudit {
    static func run(_ scenario: String) async {
        setlinebuf(stdout)
        // Segmentation checks operate on generated pixels and do not need a foreground window.
        if scenario == "vision" { exit(await ImageVisionAudit.run() == 0 ? 0 : 1) }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("IMG42-audit-" + UUID().uuidString)
        var passed = false
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            guard let image = ImageLoader.render(size: CGSize(width: 320, height: 240), { context in
                context.setFillColor(NSColor.white.cgColor); context.fill(CGRect(x: 0, y: 0, width: 320, height: 240))
            }) else { throw CocoaError(.fileWriteUnknown) }
            let url = folder.appendingPathComponent("source.png")
            try ImageWriter.write(image, to: url, type: "public.png")
            let owner = ImageWindowController(urls: [url], start: url)
            owner.showWindow(nil)
            NSApp.activate(ignoringOtherApps: true); owner.window?.makeKeyAndOrderFront(nil)
            guard await UITest.wait({ owner.base != nil && owner.window?.isKeyWindow == true }) else { throw CocoaError(.coderInvalidValue) }
            switch scenario {
            case "markup": passed = await owner.auditMarkup()
            case "editing": passed = owner.auditEditing()
            case "files": passed = await owner.auditFiles()
            case "core": passed = await owner.auditCore()
            case "window-modes": passed = await owner.auditWindowModes()
            case "live-text": passed = await owner.auditLiveText()
            default: print("FAIL IMG42 audit: unknown scenario")
            }
            if let window = owner.window { UITest.capture(window, "img-" + scenario) }
            owner.canvas.markup.cancelTextEditing(); owner.stopAnimation(); owner.slideshowTimer?.invalidate()
            owner.window?.close()
        } catch { print("FAIL IMG42 audit fixture:", error) }
        try? FileManager.default.removeItem(at: folder)
        exit(passed ? 0 : 1)
    }
}
