import AppKit
import Kit42

Localization.register(rows: ppt42Strings)

#if APP_STORE
// The sandboxed App Store variant: shared library location, disabled features and its own strings.
if CommandLine.arguments.contains("--selftest"), !AppStoreEdition.selfCheck() { exit(1) }
#endif

if CommandLine.arguments.contains("--selftest") { exit(Int32(PPTFilesAudit.run() == 0 ? 0 : 1)) }

// Command-line checks (no UI): --info, --export, --convert, --demo. See CLI.swift.
if CLI.run(CommandLine.arguments) { exit(CLI.status) }

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
