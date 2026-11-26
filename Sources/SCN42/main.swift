import AppKit
import Kit42

Localization.register(rows: scn42Strings)

#if APP_STORE
// The sandboxed App Store variant: shared library location, disabled features and its own strings.
if CommandLine.arguments.contains("--selftest"), !AppStoreEdition.selfCheck() { exit(1) }
#endif

// Command-line checks (no UI): --process, --pipeline, --detect-photos, --fake-scan, --list-devices. See CLI.swift.
if CLI.run(CommandLine.arguments) { exit(CLI.status) }

let documentController = ScanDocumentController()   // the first instance becomes NSDocumentController.shared
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
