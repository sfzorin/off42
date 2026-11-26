import AppKit
import Kit42
import PDFKit

// Command-line helpers for testing signatures without the UI:
//   PDF42 --verify file.pdf
//   PDF42 --sign in.pdf identity.p12 password out.pdf [visible] [tsa | tsa=URL] [level=pkcs7|B-B|B-T|B-LT|B-LTA]
//   PDF42 --ltv in.pdf out.pdf                          (DSS with certificates, OCSP, CRLs for every signature)
//   PDF42 --doc-timestamp in.pdf out.pdf [TSA-URL]      (RFC 3161 document timestamp; B-LTA for signed files)
//   PDF42 --csc-credentials SERVICE-URL [auth…]          (CSC API v2: list credentials)
//   PDF42 --csc-sign in.pdf out.pdf SERVICE-URL CREDENTIAL-ID [auth…] [pin=…] [otp=…] [tsa=URL] [level=…]
//   auth: token=… | user=… password=… | client-id=… [client-secret=…] [port=8765] [no-browser]
// Keys and certificate requests ("Get a Certificate…"; SUITE42_KEY_DIR overrides the key folder):
//   PDF42 --key-new enclave|keychain LABEL [p256|rsa2048|rsa3072] [no-presence]   (prints the key id)
//   PDF42 --key-list
//   PDF42 --key-csr ID out.csr CN=… [O=… OU=… L=… ST=… C=… email=… serial=… given=… surname=…]
//   PDF42 --key-import ID|any certificate-file          (.cer/.crt/.pem/.p7b; pairs the certificate with its key)
//   PDF42 --key-sign in.pdf ID out.pdf [tsa | tsa=URL] [level=…]
//   PDF42 --key-delete ID
Localization.register(rows: pdf42Strings)

let args = CommandLine.arguments
if args.count >= 2, args[1] == "--selftest" {
    let basic = PDFSelfTest.run(), deep = PDFFilesAudit.run(), signatures = SignatureSelfTest.run()
    exit(basic == 0 && deep == 0 && signatures == 0 ? 0 : 1)
}
// PDF42 --mark-overlay DIR: a page of text with a stamp and a signature over it, saved and reopened,
// with multiply (ink) and, for comparison, normal blending.
// Local only: PDF42 --site-samples OUT_DIR writes signed sample documents for website screenshots.
if args.count >= 4, args[1] == "--site-candidates" {
    try SiteSamples.candidates(args[2], to: URL(fileURLWithPath: args[3]))
    exit(0)
}
if args.count >= 4, args[1] == "--site-samples" {
    try SiteSamples.write(copies: URL(fileURLWithPath: args[2]), to: URL(fileURLWithPath: args[3]))
    exit(0)
}
if args.count >= 3, args[1] == "--mark-overlay" {
    try MarkOverlayDemo.write(to: URL(fileURLWithPath: args[2]))
    exit(0)
}
// PDF42 --signature-blindtest REAL_DIR OUT_DIR [copies]: real and generated signatures shuffled on a sheet, with a key.
// PDF42 --signature-blindtest-score key.json answers.txt: how well testers told them apart.
if args.count >= 4, args[1] == "--signature-blindtest" {
    print(try SignatureBlindTest.make(real: URL(fileURLWithPath: args[2]), to: URL(fileURLWithPath: args[3]),
                                      copies: args.count > 4 ? Int(args[4]) ?? 2 : 2, pooled: args.contains("--pooled")))
    exit(0)
}
// PDF42 --signature-fidelity-synthetic OUT: the same on a generated sample sheet with traps (regression, no real data).
if args.count >= 3, args[1] == "--signature-fidelity-synthetic" {
    let result = try SignatureFidelity.runSynthetic(to: URL(fileURLWithPath: args[2]))
    print(result.report)
    exit(result.failures.isEmpty ? 0 : 1)
}
// PDF42 --signature-fidelity INPUT… OUT [--check]: learned copies measured against held-out real signatures.
if args.count >= 4, args[1] == "--signature-fidelity" {
    let paths = args.dropFirst(2).filter { $0 != "--check" }
    let result = try SignatureFidelity.run(inputs: paths.dropLast().map { URL(fileURLWithPath: $0) }, to: URL(fileURLWithPath: paths.last!))
    print(result.report)
    exit(args.contains("--check") && !result.failures.isEmpty ? 1 : 0)
}
if args.count >= 4, args[1] == "--signature-blindtest-score" {
    print(try SignatureBlindTest.score(key: URL(fileURLWithPath: args[2]), answers: URL(fileURLWithPath: args[3])))
    exit(0)
}
// PDF42 --signature-ranking "Name" OUT.png [pool]: random designs sorted by quality, the three offered outlined.
if args.count >= 4, args[1] == "--signature-ranking" {
    try SignatureRankingSheet.write(name: args[2], to: URL(fileURLWithPath: args[3]), pool: args.count > 4 ? Int(args[4]) ?? 30 : 30)
    exit(0)
}
// PDF42 --signature-paraphs "Name" OUT.png: offered signatures, each with three copies of its paraph.
if args.count >= 4, args[1] == "--signature-paraphs" {
    try SignatureRankingSheet.writeParaphs(name: args[2], to: URL(fileURLWithPath: args[3]))
    exit(0)
}
// PDF42 --signature-bench DIR [designs copies]: fluency, tremor, variability and legibility of generated signatures.
if args.count >= 3, args[1] == "--signature-bench" {
    let numbers = args.dropFirst(3).compactMap { Int($0) }
    let designs = numbers.first ?? 6, copies = numbers.dropFirst().first ?? 5
    let folder = URL(fileURLWithPath: args[2])
    print(try SignatureBench.run(to: folder, designs: designs, copies: copies))
    // --check: exit 1 when fluency, wobble or variability leave the thresholds.
    if args.contains("--check") {
        let failed = SignatureBench.check(folder: folder)
        for f in failed { print("FAIL signature bench: \(f)") }
        if failed.isEmpty { print("Signature bench checks: passed") }
        exit(failed.isEmpty ? 0 : 1)
    }
    exit(0)
}
// PDF42 --mark-showcase out.pdf: description, signature styles, stamps of many countries, document signature sections.
if args.count >= 3, args[1] == "--mark-showcase" {
    try MarkShowcase.write(to: URL(fileURLWithPath: args[2]))
    exit(0)
}
// PDF42 --mark-samples DIR: generated signatures and stamps for visual review.
if args.count >= 3, args[1] == "--mark-samples" {
    try MarkSamples.write(to: URL(fileURLWithPath: args[2]))
    let count = (try? FileManager.default.contentsOfDirectory(atPath: args[2]).filter { $0.hasSuffix(".png") }.count) ?? 0
    print("Mark samples: \(count) files")
    exit(0)
}
// GOST signatures and detached-file operations use the dedicated CSP command handler.
if let code = GOSTCLI.run(args) { exit(code) }
if let code = PDFMainCLI.run(args) { exit(code) }
if let code = KeyCLI.run(args) { exit(code) }

if let code = SigningWorkflowCLI.run(args) { exit(code) }
if let code = ToolsCLI.run(args) { exit(code) }

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
