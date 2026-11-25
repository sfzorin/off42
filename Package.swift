// swift-tools-version:5.9
import PackageDescription

// App extensions start in NSExtensionMain instead of main.
let extensionEntry = ["-Xlinker", "-e", "-Xlinker", "_NSExtensionMain"]

// Suite42: PDF42, DOC42, XLS42, IMG42, PPT42, SCN42. Every app shares Kit42 (menus, toolbar, look, localization).
let package = Package(
    name: "Suite42",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "Kit42", targets: ["Kit42"]),
        .executable(name: "PDF42", targets: ["PDF42"]),
        .executable(name: "XLS42", targets: ["XLS42"]),
        .executable(name: "DOC42", targets: ["DOC42"]),
        .executable(name: "IMG42", targets: ["IMG42"]),
        .executable(name: "PPT42", targets: ["PPT42"]),
        .executable(name: "SCN42", targets: ["SCN42"]),
        // Quick Look extension binaries, packaged as .appex bundles by build.sh.
        .executable(name: "QLPreview42", targets: ["QLPreview42"]),
        .executable(name: "QLThumbnail42", targets: ["QLThumbnail42"]),
    ],
    targets: [
        .target(name: "Kit42", path: "Sources/Kit42", resources: [.copy("Resources/Translations"), .copy("Resources/Technical"), .copy("Resources/Labels")]),
        // Works with the Command Line Tools alone, without the XCTest framework in full Xcode.
        .executableTarget(name: "localization42audit", dependencies: ["Kit42"], path: "Tests/Kit42Tests"),
        .executableTarget(name: "production42audit", dependencies: ["Kit42"], path: "Tests/ProductionTests"),
        .executableTarget(name: "PDF42", dependencies: ["Kit42"], path: "Sources/PDF42"),
        .executableTarget(name: "XLS42", dependencies: ["Kit42"], path: "Sources/XLS42"),
        .executableTarget(name: "DOC42", dependencies: ["Kit42"], path: "Sources/DOC42"),
        .executableTarget(name: "IMG42", dependencies: ["Kit42"], path: "Sources/IMG42"),
        .executableTarget(name: "PPT42", dependencies: ["Kit42"], path: "Sources/PPT42"),
        .executableTarget(name: "SCN42", dependencies: ["Kit42"], path: "Sources/SCN42"),
        // Quick Look for formats macOS shows as plain text or not at all: Markdown, OpenDocument spreadsheets, TSV.
        .target(name: "Previews42", dependencies: ["Kit42"], path: "Sources/Previews42"),
        .executableTarget(name: "QLPreview42", dependencies: ["Previews42"], path: "Sources/QLPreview42",
                          linkerSettings: [.linkedFramework("QuickLookUI"), .unsafeFlags(extensionEntry)]),
        .executableTarget(name: "QLThumbnail42", dependencies: ["Previews42"], path: "Sources/QLThumbnail42",
                          linkerSettings: [.linkedFramework("QuickLookThumbnailing"), .unsafeFlags(extensionEntry)]),
        // Development tool: renders previews/thumbnails to files without Quick Look.
        .executableTarget(name: "ql42render", dependencies: ["Previews42"], path: "Sources/ql42render"),
    ]
)
