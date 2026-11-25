import AppKit

/// Development aid: `K42_SNAPSHOT=/path/out.png` renders the front window (with toolbar) to a PNG
/// after `K42_SNAPSHOT_DELAY` seconds (default 2), then quits.
public enum DebugSnapshot {
    /// `K42_APPEARANCE=dark|light` forces the app appearance (dark mode checks). Installed by
    /// `Localization.register(rows:)`, which every app calls first in main.swift, so it applies
    /// before the first window without app changes.
    static let appearanceHook: Void = {
        guard ProcessInfo.processInfo.environment["K42_APPEARANCE"] != nil else { return }
        NotificationCenter.default.addObserver(forName: NSApplication.willFinishLaunchingNotification, object: nil, queue: .main) { _ in
            applyAppearanceIfRequested()
        }
        if NSApp != nil { applyAppearanceIfRequested() }
    }()

    public static func applyAppearanceIfRequested() {
        switch ProcessInfo.processInfo.environment["K42_APPEARANCE"]?.lowercased() {
        case "dark": NSApp?.appearance = NSAppearance(named: .darkAqua)
        case "light": NSApp?.appearance = NSAppearance(named: .aqua)
        default: break
        }
    }

    public static func scheduleIfRequested(prepare: (() -> Void)? = nil) {
        let env = ProcessInfo.processInfo.environment
        applyAppearanceIfRequested()
        guard let path = env["K42_SNAPSHOT"] else { return }
        let delay = Double(env["K42_SNAPSHOT_DELAY"] ?? "") ?? 2
        // Audits pump the run loop while awaiting native sheets and background preview work.
        // Starting inside a main-queue block would prevent their queued main-thread completions
        // from running until the audit had already timed out.
        Timer.scheduledTimer(withTimeInterval: max(0.01, delay), repeats: false) { _ in
            // Directly launched test bundles may still be inactive. Capture enabled toolbar and
            // focus states instead of the greyed-out appearance of a background application.
            NSApp.activate(ignoringOtherApps: true)
            NSApp.orderedWindows.first { $0.isVisible && !($0 is NSPanel) }?.makeKeyAndOrderFront(nil)
            // K42_AUDIT_RECENT=1 checks menu population, actions, and custom-controller routing.
            if env["K42_AUDIT_RECENT"] != nil, !auditRecentMenu() { exit(1) }
            // K42_SIZE=900x700 resizes the front window first (layout checks).
            if let size = env["K42_SIZE"]?.split(separator: "x").compactMap({ Double($0) }), size.count == 2,
               let window = NSApp.orderedWindows.first(where: { $0.isVisible }) {
                window.setContentSize(NSSize(width: size[0], height: size[1]))
            }
            prepare?()
            // K42_OPEN_GENERATOR=signature|seal opens the signature/stamp generator (use with K42_SNAPSHOT_SHEET=1).
            if let mode = env["K42_OPEN_GENERATOR"] {
                MarkGeneratorController.show(mode == "seal" ? .seal : mode == "learn" ? .learn : .signature,
                                             over: NSApp.orderedWindows.first { $0.isVisible }, onInsert: { _ in })
            }
            // K42_OPEN_MARK_INSERT=seal|signature opens the insert preview with its resolution choice (vector page;
            // K42_MARK_SCAN_DPI=300 makes it a scanned page). Use with K42_SNAPSHOT_SHEET=1.
            if let kind = env["K42_OPEN_MARK_INSERT"] {
                let mark: GeneratedMark = kind == "seal"
                    ? .seal(SealSpec(template: SealTemplate.all[0], values: [:], ink: SealTemplate.all[0].ink.stampPad, designSeed: 7))
                    : .signature(env["K42_GENERATOR_NAME"] ?? "Alex Morgan", ink: InkColor.blue.pen, design: 5)
                let scan = env["K42_MARK_SCAN_DPI"].flatMap(Double.init).map { CGFloat($0) }
                MarkInsertDialog.present(.mark(mark), over: NSApp.orderedWindows.first { $0.isVisible },
                                         placement: { MarkPlacement.standard(aspect: $0, scanDPI: scan) }, insert: { _ in })
            }
            // K42_HIDE_PANELS=1 runs View › Hide All Panels first.
            if env["K42_HIDE_PANELS"] != nil,
               let controller = NSApp.orderedWindows.first(where: { $0.isVisible })?.windowController as? PanelHiding {
                controller.toggleAllPanelsAction(nil)
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
                // K42_SNAPSHOT_POPOVER=1 captures an open popover instead of the window.
                let popover = env["K42_SNAPSHOT_POPOVER"] != nil
                    ? NSApp.windows.first { $0.isVisible && String(describing: type(of: $0)).contains("Popover") } : nil
                var window = popover ?? NSApp.orderedWindows.first { $0.isVisible && $0.contentView != nil }
                // K42_SNAPSHOT_SHEET=1 captures the sheet attached to the window (dialogs).
                if env["K42_SNAPSHOT_SHEET"] != nil, let sheet = NSApp.windows.compactMap(\.attachedSheet).first { window = sheet }
                guard LocalizationAudit.runIfRequested(in: window) else { exit(1) }
                if let frame = window?.contentView?.superview,
                   let rep = frame.bitmapImageRepForCachingDisplay(in: frame.bounds) {
                    frame.cacheDisplay(in: frame.bounds, to: rep)
                    try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
                }
                if env["K42_SNAPSHOT_KEEP"] == nil {
                    // An open sheet would block terminate: quit hard, it is a debug run.
                    if NSApp.windows.contains(where: { $0.attachedSheet != nil }) { exit(0) }
                    NSApp.terminate(nil)
                }
            }
        }
    }
}
