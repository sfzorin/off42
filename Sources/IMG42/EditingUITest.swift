import AppKit
import Kit42

extension ImageWindowController {
    /// Exercises native adjustment controls and document pixel commands, including full Undo/Redo state.
    func auditEditing() -> Bool {
        guard let window, loaded != nil else { print("FAIL IMG editing: no loaded document window"); return false }
        let saved = state, savedOriginal = originalBase, savedDescription = originalDescription, savedDPI = originalDPI
        let savedGroups = undo.groupsByEvent
        undo.groupsByEvent = false
        var count = 0, failed = 0
        func check(_ value: Bool, _ name: String) { count += 1; if !value { failed += 1 }; print(value ? "PASS" : "FAIL", "IMG editing:", name) }
        func event(_ body: () -> Void) { undo.beginUndoGrouping(); body(); undo.endUndoGrouping() }
        func pump(_ seconds: TimeInterval = 0.08) {
            let end = Date(timeIntervalSinceNow: seconds)
            while Date() < end {
                if let event = NSApp.nextEvent(matching: .any, until: end, inMode: .default, dequeue: true) { NSApp.sendEvent(event) }
            }
        }
        func views(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(views) }
        let image = ImagePixelAudit.fixture(), size = CGSize(width: image.width, height: image.height)
        func reset(_ source: CGImage? = nil) {
            stopAnimation(); resetEditing(base: source ?? image)
            imageDescription = nil; originalDescription = nil; imageDPI = 72; originalDPI = 72
            showingOriginal = false; canvas.markup.hideMarks = false
            refreshDisplay(resize: true); undo.removeAllActions()
        }
        defer {
            renderWork?.cancel(); renderWork = nil
            canvas.markup.cancelInteraction(); canvas.markup.cancelTextEditing()
            originalBase = savedOriginal; originalDescription = savedDescription; originalDPI = savedDPI
            restore(saved); undo.removeAllActions(); undo.groupsByEvent = savedGroups
        }
        reset()
        if inspectorShows !== adjustPanel { toggleAdjustAction(nil) }
        window.makeKeyAndOrderFront(nil); window.makeFirstResponder(canvas.markup); pump()
        let controls = views(adjustPanel.view)
        check(inspectorShows === adjustPanel, "Adjust opens its native inspector")
        let sliders = controls.compactMap { $0 as? ResettingSlider }
        check(Set(sliders.compactMap { $0.identifier?.rawValue }) == Set(Adjustments.Key.allCases.map(\.rawValue)), "every adjustment has a native slider")
        if let slider = sliders.first(where: { $0.identifier?.rawValue == "brightness" }) {
            event { slider.doubleValue = 40; if let action = slider.action { _ = NSApp.sendAction(action, to: slider.target, from: slider) } }
            check(adjustments[.brightness] == 40 && isDirty, "non-drag native slider action updates the document")
            check(displayed.map(ImagePixelAudit.pixels) != ImagePixelAudit.pixels(image), "slider action changes rendered pixels")
            undo.undo(); check(adjustments.isIdentity && !isDirty && displayed === image, "slider Undo restores original pixels and clean state")
            undo.redo(); check(adjustments[.brightness] == 40 && slider.doubleValue == 40, "slider Redo restores value and inspector")
            event { slider.onReset?() }
            check(adjustments[.brightness] == 0, "slider reset callback resets the real control")
            undo.undo(); check(adjustments[.brightness] == 40, "slider reset is undoable")
        } else { check(false, "brightness slider exists") }
        if let popup = controls.compactMap({ $0 as? NSPopUpButton }).first {
            event { popup.selectItem(at: Adjustments.Look.sepia.rawValue); if let action = popup.action { _ = NSApp.sendAction(action, to: popup.target, from: popup) } }
            check(adjustments.look == .sepia && popup.indexOfSelectedItem == Adjustments.Look.sepia.rawValue, "native Look popup applies its chosen look")
            undo.undo(); check(adjustments.look == .none, "Look Undo restores the previous look")
            undo.redo(); check(adjustments.look == .sepia, "Look Redo restores its chosen look")
        } else { check(false, "Look popup exists") }
        if let resetButton = controls.compactMap({ $0 as? NSButton }).first(where: { $0.title == L("Reset All") }) {
            event { resetButton.performClick(nil) }
            check(adjustments.isIdentity && displayed === image, "Reset All restores every adjustment and source pixels")
            undo.undo(); check(adjustments.look == .sepia && adjustments[.brightness] == 40, "Reset All Undo restores the full adjustment state")
            undo.redo(); check(adjustments.isIdentity, "Reset All Redo restores identity")
        } else { check(false, "Reset All button exists") }
        reset(); applyAdjustments(Adjustments(), name: "Audit")
        check(!undo.canUndo && !isDirty, "applying unchanged adjustments creates no Undo entry")
        if let automatic = controls.compactMap({ $0 as? NSButton }).first(where: { $0.title == L("Auto Enhance") }) {
            event { automatic.performClick(nil) }
            check(adjustments.autoEnhance && displayed?.width == image.width, "native Auto Enhance applies without changing image dimensions")
            undo.undo(); check(!adjustments.autoEnhance && displayed === image, "Auto Enhance Undo restores source")
            undo.redo(); check(adjustments.autoEnhance, "Auto Enhance Redo")
        } else { check(false, "Auto Enhance button exists") }
        for op in [Orientation.Op.rotateLeft, .rotateRight, .flipHorizontal, .flipVertical] {
            reset()
            var mark = Mark(kind: .rectangle); mark.rect = CGRect(x: 20, y: 10, width: 15, height: 12)
            canvas.markup.marks = [mark]; canvas.markup.selectedID = mark.id
            let selection = ImagePixelAudit.rectSelection(CGRect(x: 5, y: 6, width: 30, height: 20), size: size)
            canvas.markup.pixelSelection = selection
            let expectedRect = mark.rect.applying(Orientation.transform(op, size: size))
            event { transformImage(op) }
            check(canvas.markup.marks.first?.rect == expectedRect && canvas.markup.pixelSelection == nil, "transform moves markup with the pixels: \(op)")
            let transformed = base
            undo.undo()
            check(base === image && canvas.markup.marks.first?.rect == mark.rect && canvas.markup.selectedID == mark.id && canvas.markup.pixelSelection?.bounds == selection.bounds,
                  "transform Undo restores pixels, markup and both selections: \(op)")
            undo.redo(); check(base === transformed && canvas.markup.marks.first?.rect == expectedRect, "transform Redo restores complete edit: \(op)")
        }
        reset()
        var mark = Mark(kind: .rectangle); mark.rect = CGRect(x: 18, y: 19, width: 10, height: 8)
        canvas.markup.marks = [mark]; canvas.markup.selectedID = mark.id
        let rectangle = ImagePixelAudit.rectSelection(CGRect(x: 10, y: 12, width: 30, height: 20), size: size)
        canvas.markup.pixelSelection = rectangle
        event { cropToSelectionAction(nil) }
        check(base?.width == 30 && base?.height == 20 && canvas.markup.marks.first?.rect == CGRect(x: 8, y: 7, width: 10, height: 8), "Crop uses the selection and translates existing markup")
        undo.undo()
        check(base === image && canvas.markup.pixelSelection?.bounds == rectangle.bounds && canvas.markup.selectedID == mark.id, "Crop Undo restores original selection and selected markup")
        undo.redo(); check(base?.width == 30 && canvas.markup.pixelSelection == nil, "Crop Redo restores cropped pixels")
        showOriginal(true)
        check(canvas.imageView.image === image && canvas.imageSize == size && canvas.markup.hideMarks, "Show Original after Crop uses the original image geometry")
        showOriginal(false)
        check(canvas.imageView.image === base && canvas.imageSize == CGSize(width: 30, height: 20) && !canvas.markup.hideMarks, "releasing Show Original restores the cropped editor geometry")
        reset(); canvas.markup.pixelSelection = rectangle
        event { deleteSelectionAction(nil) }
        check(base.map { ImagePixelAudit.sample($0, 20, 20)[3] } == 0 && canvas.markup.pixelSelection == nil, "Delete Selection makes selected pixels transparent")
        undo.undo(); check(base === image && canvas.markup.pixelSelection?.bounds == rectangle.bounds, "Delete Selection Undo restores pixels and selection")
        undo.redo(); check(base.map { ImagePixelAudit.sample($0, 20, 20)[3] } == 0, "Delete Selection Redo restores transparency")
        reset(); canvas.markup.pixelSelection = rectangle
        invertSelectionAction(nil)
        check(canvas.markup.pixelSelection?.bounds == CGRect(origin: .zero, size: size), "Invert Selection selects the complement")
        deselectAction(nil); check(canvas.markup.pixelSelection == nil, "Deselect removes the pixel selection")
        selectAll(nil); check(canvas.markup.pixelSelection?.bounds == CGRect(origin: .zero, size: size), "Select All covers the full image")
        reset(); canvas.markup.marks = [mark]
        event { _ = resizeImage(to: CGSize(width: 192, height: 128)) }
        check(base?.width == 192 && base?.height == 128 && canvas.markup.marks.first?.rect == mark.rect.applying(CGAffineTransform(scaleX: 2, y: 2)), "Resize changes pixel dimensions and scales markup")
        undo.undo(); check(base === image && canvas.markup.marks.first?.rect == mark.rect, "Resize Undo restores original dimensions and markup")
        undo.redo(); check(base?.width == 192 && base?.height == 128, "Resize Redo restores scaled dimensions")
        let current = base; undo.removeAllActions()
        for target in [CGSize(width: CGFloat.nan, height: 100), CGSize(width: 20000, height: 20000), .zero] {
            check(!resizeImage(to: target) && base === current && !undo.canUndo, "invalid Resize preserves image and history")
        }
        _ = resizeImage(to: CGSize(width: 192, height: 128))
        check(base === current && !undo.canUndo, "same-size Resize is a no-op")

        let dialog = ResizeDialog(size: CGSize(width: 1001, height: 601), dpi: 300)
        if let grid = dialog.alert.accessoryView as? NSGridView,
           let width = grid.cell(atColumnIndex: 1, rowIndex: 0).contentView as? NSTextField,
           let height = grid.cell(atColumnIndex: 1, rowIndex: 1).contentView as? NSTextField,
           let units = grid.cell(atColumnIndex: 2, rowIndex: 0).contentView as? NSPopUpButton,
           let resolution = grid.cell(atColumnIndex: 1, rowIndex: 2).contentView as? NSTextField,
           let resample = grid.cell(atColumnIndex: 1, rowIndex: 4).contentView as? NSButton {
            func change(_ field: NSTextField, _ value: String) { field.stringValue = value; dialog.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: field)) }
            for unit in ResizeDialog.Unit.allCases {
                units.selectItem(at: unit.rawValue); if let action = units.action { _ = NSApp.sendAction(action, to: units.target, from: units) }
                check(dialog.pixelSize == CGSize(width: 1001, height: 601) && dialog.isValid, "Resize unit conversion preserves exact pixels: \(unit)")
            }
            units.selectItem(at: ResizeDialog.Unit.pixels.rawValue); if let action = units.action { _ = NSApp.sendAction(action, to: units.target, from: units) }
            change(width, "2002")
            check(dialog.pixelSize == CGSize(width: 2002, height: 1202), "proportional Resize updates the other dimension")
            change(width, "nan"); units.selectItem(at: ResizeDialog.Unit.cm.rawValue); if let action = units.action { _ = NSApp.sendAction(action, to: units.target, from: units) }
            check(!dialog.isValid && dialog.alert.buttons.first?.isEnabled == false, "changing units cannot silently accept invalid dimensions")
            change(width, "16000"); change(height, "16000")
            check(!dialog.isValid, "Resize dialog rejects an excessive pixel allocation")
            change(width, "1001"); resample.performClick(nil)
            check(!width.isEnabled && !height.isEnabled && dialog.pixelSize == CGSize(width: 1001, height: 601), "No Resample disables pixel dimensions that cannot be changed")
            change(resolution, "150")
            check(dialog.resolutionDPI == 150 && dialog.pixelSize == CGSize(width: 1001, height: 601) && dialog.isValid, "No Resample changes print resolution without changing pixels")
        } else { check(false, "native Resize controls exist") }
        reset()
        if let request = captureBackgroundRemoval() {
            check(isCurrentBackgroundRemoval(request), "background-removal request starts with current image state")
            resetEditing(base: image)
            check(!isCurrentBackgroundRemoval(request) && !applyBackgroundRemoval(ImagePixelAudit.fixture(width: 50, height: 30), request: request) && base === image,
                  "resetting the same bitmap rejects an obsolete background-removal result")
        } else { check(false, "background-removal request fixture") }
        reset()
        if let request = captureBackgroundRemoval() {
            event { var changed = Adjustments(); changed[.brightness] = 10; applyAdjustments(changed, name: "Audit") }
            check(!isCurrentBackgroundRemoval(request), "intervening edits invalidate background removal")
        } else { check(false, "background-removal edit fixture") }
        reset()
        if let previous = captureBackgroundRemoval(), let latest = captureBackgroundRemoval() {
            check(!isCurrentBackgroundRemoval(previous) && isCurrentBackgroundRemoval(latest), "a newer background-removal request supersedes the previous request")
            let replacement = ImagePixelAudit.fixture(width: 80, height: 40)
            event { check(applyBackgroundRemoval(replacement, request: latest), "a current background-removal result applies") }
            undo.undo(); check(base === image, "background removal Undo restores the original pixels")
            undo.redo(); check(base === replacement, "background removal Redo restores the result")
        } else { check(false, "background-removal replacement fixture") }
        print("IMG42 editing regressions: \(count - failed)/\(count) passed")
        return failed == 0
    }
}
