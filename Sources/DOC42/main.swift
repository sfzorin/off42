import AppKit
import Kit42

Localization.register(rows: doc42Strings)

// --sign-document / --verify-document: Office and OpenDocument digital signatures (Kit42/DocumentSignatureCLI.swift).
if let code = DocumentSignatureCLI.run(CommandLine.arguments) { exit(code) }

// DOC42 --flatten in.docx out.docx: write the style-flattened document (for testing).
if let i = CommandLine.arguments.firstIndex(of: "--flatten"), CommandLine.arguments.count > i + 2 {
    let args = CommandLine.arguments
    try DocxStyles.flatten(Data(contentsOf: URL(fileURLWithPath: args[i + 1]))).write(to: URL(fileURLWithPath: args[i + 2]))
    exit(0)
}

// DOC42 --export in.docx out.(pdf|docx|rtf|odt|html|txt|md|epub): convert (for testing)
if let i = CommandLine.arguments.firstIndex(of: "--export"), CommandLine.arguments.count > i + 2 {
    let args = CommandLine.arguments
    let input = URL(fileURLWithPath: args[i + 1]), output = URL(fileURLWithPath: args[i + 2])
    let doc = try CLI.open(input.path)
    let ext = output.pathExtension.lowercased()
    if ProcessInfo.processInfo.environment["K42_DEBUG"] != nil { doc.debugPages() }
    let data = ext == "epub" ? try doc.epubData() : ext == "pdf" ? doc.pdfData() : ext == "md" ? doc.markdownData() : try doc.exportData(TextDocument.documentType(for: ext == "doc" ? "com.microsoft.word.doc" : ext))
    try data.write(to: output)
    print("\(ext): \(data.count) bytes")
    exit(0)
}

// Test hooks: --stats, --toc, --replace, --demo, --dump (see CLI.swift).
if try CLI.run(CommandLine.arguments) { exit(0) }

let app = NSApplication.shared
let documentController = DocDocumentController()
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
