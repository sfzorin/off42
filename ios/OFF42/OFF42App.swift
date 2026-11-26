import SwiftUI

@main
struct OFF42App: App {
    @State private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                // "Open in" / share to OFF42 from Files, Mail and other apps (document types in Info.plist).
                .onOpenURL { url in Task { await model.open(url: url) } }
                .task { await UITestHooks.apply(to: model) }
        }
    }
}

/// Launch hooks for the UI tests only, driven by environment variables that only XCUITest sets:
/// OFF42_UITEST_OPEN (file paths, one per line, opened through the same path as "Open in") and
/// OFF42_UITEST_START (a section, optionally "/tool"). OFF42_UITEST_EXPORT_DIR is read by AppModel.
@MainActor
enum UITestHooks {
    static func apply(to model: AppModel, environment: [String: String] = ProcessInfo.processInfo.environment) async {
        if let start = environment["OFF42_UITEST_START"] {
            let parts = start.split(separator: "/").map(String.init)
            if let section = AppSection(rawValue: parts[0]) {
                model.show(section, tool: parts.count > 1 ? Tool(rawValue: parts[1]) : nil)
            }
        }
        if let paths = environment["OFF42_UITEST_OPEN"] {
            for path in paths.split(separator: "\n") where !path.isEmpty {
                await model.open(url: URL(fileURLWithPath: String(path)))
            }
        }
    }
}
