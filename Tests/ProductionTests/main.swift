import AppKit
import Kit42

_ = NSApplication.shared
NSApp.setActivationPolicy(.prohibited)
let args = CommandLine.arguments
let output: URL
if let index = args.firstIndex(of: "--output"), args.indices.contains(index + 1) {
    output = URL(fileURLWithPath: args[index + 1], isDirectory: true)
} else {
    output = FileManager.default.temporaryDirectory.appendingPathComponent("suite42-production-audit-\(UUID().uuidString)", isDirectory: true)
}
var failures: [String] = []
func run(_ name: String, _ checks: () -> [String]) {
    let errors = checks()
    print("\(name): \(errors.isEmpty ? "PASS" : "FAIL")")
    errors.forEach { print("  \($0)") }
    failures += errors.map { "\(name): \($0)" }
}
run("Technical symbols and independent barcode decoding", TechnicalElementChecks.run)
run("Complete label outputs and invalid inputs", LabelSheetChecks.run)
run("Save-to-file printing and calibration") { LabelSheetChecks.runPrint(outputDirectory: output) }
if args.contains("--ui") {
    run("Native technical-element controls", TechnicalElementChecks.runUI)
    run("Native label controls and stale preview") { LabelSheetChecks.runUI(outputDirectory: output) }
}
print("Artifacts: \(output.path)")
print(failures.isEmpty ? "ALL PRODUCTION CHECKS PASSED" : "\(failures.count) PRODUCTION CHECKS FAILED")
exit(failures.isEmpty ? 0 : 1)
