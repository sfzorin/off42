import AppKit
import Kit42

Localization.register(rows: img42Strings)

#if APP_STORE
// The sandboxed App Store variant: shared library location, disabled features and its own strings.
if CommandLine.arguments.contains("--selftest"), !AppStoreEdition.selfCheck() { exit(1) }
#endif

if CommandLine.arguments.contains("--selftest") {
    let pixels = ImagePixelAudit.run(), files = ImageFileSelfTest.run(), recovery = ImageRecoverySelfTest.run()
    exit(pixels == 0 && files == 0 && recovery == 0 ? 0 : 1)
}

// Command-line checks (no UI): --info, --edit, --stats, --ocr, --types, --rotate. See CLI.swift.
if CLI.run(CommandLine.arguments) { exit(CLI.status) }

let documentController = ImageDocumentController()   // the first instance becomes NSDocumentController.shared
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
