import AppKit
import Kit42

extension ImageWindowController {
    @MainActor func auditWindowModes() async -> Bool {
        guard let window else { return false }
        var checks = 0, failures = 0
        func check(_ value: Bool, _ name: String) {
            checks += 1; if !value { failures += 1 }
            print(value ? "PASS" : "FAIL", "IMG42 window modes:", name)
        }
        func transition(enter: Bool, action: () -> Void) async -> Bool {
            var completed = false
            let name = enter ? NSWindow.didEnterFullScreenNotification : NSWindow.didExitFullScreenNotification
            let observer = NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { _ in
                MainActor.assumeIsolated { completed = true }
            }
            defer { NotificationCenter.default.removeObserver(observer) }
            action()
            return await UITest.wait({ completed && window.styleMask.contains(.fullScreen) == enter }, seconds: 12)
        }
        setPanels([.toolbar, .markup, .filmstrip, .info], remember: false)
        let panels = visiblePanels
        let stored = Self.defaults.object(forKey: "panels") as? Int
        check(await transition(enter: true) { window.toggleFullScreen(nil) }, "native Full Screen enters a real macOS Space")
        check(visiblePanels.intersection([.markup, .filmstrip, .info, .adjust]).isEmpty && canvas.backgroundColor == .black,
              "Full Screen hides editing panels and uses a black background")
        check(Self.defaults.object(forKey: "panels") as? Int == stored, "entering Full Screen preserves saved panel preferences")
        startSlideshow()
        check(slideshowTimer != nil, "Slideshow starts inside an existing full-screen Space")
        stopSlideshow()
        try? await Task.sleep(nanoseconds: 150_000_000)
        check(slideshowTimer == nil && window.styleMask.contains(.fullScreen), "stopping Slideshow retains a full-screen Space entered by the user")
        check(await transition(enter: false) { window.toggleFullScreen(nil) }, "native Full Screen exits")
        check(visiblePanels == panels && canvas.backgroundColor == background.color, "exiting Full Screen restores panels and image background")
        check(await transition(enter: true) { startSlideshow() }, "Slideshow enters Full Screen from an ordinary window")
        check(await transition(enter: false) { stopSlideshow() }, "stopping Slideshow exits the Space it entered")
        check(slideshowTimer == nil && visiblePanels == panels, "Slideshow cleanup restores the original window controls")
        check(Self.defaults.object(forKey: "panels") as? Int == stored, "all transitions preserve saved panel preferences")
        if window.styleMask.contains(.fullScreen) { _ = await transition(enter: false) { window.toggleFullScreen(nil) } }
        print("IMG42 window mode regressions: \(checks - failures)/\(checks) passed")
        return failures == 0
    }
}
