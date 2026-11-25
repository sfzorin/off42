import AppKit

extension EditorWindowController {
    /// Await actual activation/key-window notifications, rather than spin a nested event loop inside a
    /// main-queue callback. The caller must keep native-event Undo groups entirely outside this suspension.
    @MainActor func activateForNativeAudit(_ context: String) async -> Bool {
        guard let window else { print("FAIL PDF audit focus: no window", context); return false }
        return await withCheckedContinuation { continuation in
            var observers: [NSObjectProtocol] = []
            var completed = false
            let center = NotificationCenter.default
            func ready() -> Bool { NSApp.isActive && window.isKeyWindow && NSApp.keyWindow === window }
            func complete(_ value: Bool) {
                guard !completed else { return }
                completed = true
                observers.forEach(center.removeObserver)
                observers.removeAll()
                continuation.resume(returning: value)
            }
            func changed() {
                guard !completed else { return }
                if NSApp.isActive, !window.isKeyWindow { window.makeKeyAndOrderFront(nil); window.makeMain() }
                if ready() { complete(true) }
            }
            observers.append(center.addObserver(forName: NSApplication.didBecomeActiveNotification, object: NSApp, queue: .main) { _ in changed() })
            observers.append(center.addObserver(forName: NSWindow.didBecomeKeyNotification, object: window, queue: .main) { _ in changed() })
            DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
                guard !completed else { return }
                if ready() { complete(true); return }
                let front = NSWorkspace.shared.frontmostApplication
                print("FAIL PDF audit focus:", context,
                      "active", NSApp.isActive, "running active", NSRunningApplication.current.isActive,
                      "key", window.isKeyWindow, "visible", window.isVisible,
                      "policy", NSApp.activationPolicy().rawValue,
                      "pid", ProcessInfo.processInfo.processIdentifier,
                      "front pid", front?.processIdentifier ?? -1, "front app", front?.bundleIdentifier ?? "none",
                      "key window", NSApp.keyWindow?.windowNumber ?? -1,
                      "expected window", window.windowNumber, "modal", NSApp.modalWindow?.windowNumber ?? -1)
                complete(false)
            }
            NSApp.activate(ignoringOtherApps: true)
            window.makeKeyAndOrderFront(nil); window.makeMain()
            changed()
        }
    }
}
