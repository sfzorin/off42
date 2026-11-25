import AppKit
import CoreImage
import Kit42

/// Photo adjustments. Every value is a slider value; 0 means "unchanged".
struct Adjustments: Equatable {
    enum Key: String, CaseIterable {
        case exposure, brightness, contrast, saturation, vibrance, temperature, tint, highlights, shadows, sharpness, vignette

        var title: String {
            switch self {
            case .exposure: return L("Exposure")
            case .brightness: return L("Brightness")
            case .contrast: return L("Contrast")
            case .saturation: return L("Saturation")
            case .vibrance: return L("Vibrance")
            case .temperature: return L("Temperature")
            case .tint: return L("Tint")
            case .highlights: return L("Highlights")
            case .shadows: return L("Shadows")
            case .sharpness: return L("Sharpness")
            case .vignette: return L("Vignette")
            }
        }

        /// Slider range; values are shown as integers.
        var range: ClosedRange<Double> {
            switch self {
            case .sharpness, .vignette: return 0...100
            default: return -100...100
            }
        }
    }

    enum Look: Int, CaseIterable {
        case none, mono, noir, sepia, invert
        var title: String {
            switch self {
            case .none: return L("Original")
            case .mono: return L("Black & White")
            case .noir: return L("Noir")
            case .sepia: return L("Sepia")
            case .invert: return L("Invert")
            }
        }
    }

    var values: [Key: Double] = [:]
    var look: Look = .none
    var autoEnhance = false

    subscript(key: Key) -> Double {
        get { values[key] ?? 0 }
        set {
            let value = newValue.isFinite ? min(key.range.upperBound, max(key.range.lowerBound, newValue)) : 0
            values[key] = value == 0 ? nil : value
        }
    }

    var isIdentity: Bool { values.isEmpty && look == .none && !autoEnhance }

    /// Applies the adjustments (GPU, Core Image).
    func apply(to input: CIImage, autoFilters: [CIFilter]? = nil) -> CIImage {
        guard !isIdentity else { return input }
        let extent = input.extent
        var image = input
        if autoEnhance {
            for template in autoFilters ?? input.autoAdjustmentFilters() {
                // Live proxies and the delayed full-size render may run concurrently. CIFilter is mutable.
                guard let f = template.copy() as? CIFilter else { continue }
                f.setValue(image, forKey: kCIInputImageKey)
                image = f.outputImage ?? image
            }
        }
        if self[.exposure] != 0 {
            image = image.applyingFilter("CIExposureAdjust", parameters: [kCIInputEVKey: self[.exposure] / 50])
        }
        if self[.temperature] != 0 || self[.tint] != 0 {
            // Warmer = target neutral below 6500 K.
            let target = CIVector(x: 6500 - self[.temperature] * 30, y: self[.tint] * 1.0)
            image = image.applyingFilter("CITemperatureAndTint", parameters: ["inputNeutral": CIVector(x: 6500, y: 0), "inputTargetNeutral": target])
        }
        if self[.highlights] != 0 || self[.shadows] != 0 {
            image = image.applyingFilter("CIHighlightShadowAdjust", parameters: [
                "inputHighlightAmount": 1 + min(0, self[.highlights]) / 100,
                "inputShadowAmount": self[.shadows] / 100,
            ])
            // CIHighlightShadowAdjust caps highlights at 1 (its identity). Values above 1 silently do
            // nothing, so the positive half of our Highlights control needs a highlight-lifting curve.
            if self[.highlights] > 0 {
                image = image.applyingFilter("CIToneCurve", parameters: [
                    "inputPoint0": CIVector(x: 0, y: 0), "inputPoint1": CIVector(x: 0.25, y: 0.25),
                    "inputPoint2": CIVector(x: 0.5, y: 0.5),
                    "inputPoint3": CIVector(x: 0.75, y: 0.75 + self[.highlights] / 500),
                    "inputPoint4": CIVector(x: 1, y: 1),
                ])
            }
        }
        if self[.brightness] != 0 || self[.contrast] != 0 || self[.saturation] != 0 {
            image = image.applyingFilter("CIColorControls", parameters: [
                kCIInputBrightnessKey: self[.brightness] / 250,
                kCIInputContrastKey: 1 + self[.contrast] / (self[.contrast] > 0 ? 200 : 250),
                kCIInputSaturationKey: 1 + self[.saturation] / 100,
            ])
        }
        if self[.vibrance] != 0 {
            image = image.applyingFilter("CIVibrance", parameters: ["inputAmount": self[.vibrance] / 100])
        }
        switch look {
        case .none: break
        case .mono: image = image.applyingFilter("CIPhotoEffectMono")
        case .noir: image = image.applyingFilter("CIPhotoEffectNoir")
        case .sepia: image = image.applyingFilter("CISepiaTone", parameters: [kCIInputIntensityKey: 0.9])
        case .invert: image = image.applyingFilter("CIColorInvert")
        }
        if self[.sharpness] != 0 {
            image = image.applyingFilter("CISharpenLuminance", parameters: [kCIInputSharpnessKey: self[.sharpness] / 50,
                                                                             kCIInputRadiusKey: max(1.5, extent.width / 1500)])
        }
        if self[.vignette] != 0 {
            image = image.applyingFilter("CIVignetteEffect", parameters: [
                kCIInputCenterKey: CIVector(x: extent.midX, y: extent.midY),
                kCIInputRadiusKey: hypot(extent.width, extent.height) * 0.42,
                kCIInputIntensityKey: self[.vignette] / 100,
                "inputFalloff": 0.6,
            ])
        }
        return image.cropped(to: extent)
    }

    static let context = CIContext(options: [.cacheIntermediates: false, .highQualityDownsample: true])

    /// Renders the adjusted image in the source colour space (8 bit).
    func render(_ base: CGImage, autoFilters: [CIFilter]? = nil) -> CGImage {
        renderChecked(base, autoFilters: autoFilters) ?? base
    }

    /// File output must fail when rendering fails instead of silently dropping visible adjustments.
    func renderChecked(_ base: CGImage, autoFilters: [CIFilter]? = nil) -> CGImage? {
        guard !isIdentity else { return base }
        let input = CIImage(cgImage: base)
        let output = apply(to: input, autoFilters: autoFilters)
        let cs = base.colorSpace.flatMap { $0.model == .rgb ? $0 : nil } ?? CGColorSpace(name: CGColorSpace.sRGB)!
        return Self.context.createCGImage(output, from: input.extent, format: .RGBA8, colorSpace: cs)
    }
}

/// Slider that resets to 0 on double-click and reports the start and end of a drag.
final class ResettingSlider: NSSlider {
    var onReset: (() -> Void)?
    var onBegin: (() -> Void)?
    var onEnd: (() -> Void)?
    private(set) var isTrackingDrag = false

    override func mouseDown(with event: NSEvent) {
        if event.clickCount == 2 {
            onReset?()
            return
        }
        isTrackingDrag = true
        onBegin?()
        super.mouseDown(with: event)   // tracks until mouse up
        isTrackingDrag = false
        onEnd?()
    }
}

/// Button that is "pressed" only while the mouse is held (Show Original).
final class HoldButton: NSButton {
    var onChange: ((Bool) -> Void)?
    override func mouseDown(with event: NSEvent) {
        onChange?(true)
        highlight(true)
        while let e = window?.nextEvent(matching: [.leftMouseUp, .leftMouseDragged]), e.type != .leftMouseUp {}
        highlight(false)
        onChange?(false)
    }
}

/// Side panel with adjustment sliders, looks, Auto Enhance, Reset and Show Original.
final class AdjustPanelController: NSViewController {
    /// Called on every change (live preview); `final` is true at the end of a slider drag or a click.
    var onChange: ((Adjustments, _ final: Bool) -> Void)?
    var onBeginChange: (() -> Void)?
    var onShowOriginal: ((Bool) -> Void)?

    private(set) var adjustments = Adjustments()
    private var sliders: [Adjustments.Key: ResettingSlider] = [:]
    private var valueLabels: [Adjustments.Key: NSTextField] = [:]
    private let lookPopup = NSPopUpButton()
    private let autoButton = NSButton(checkboxWithTitle: L("Auto Enhance"), target: nil, action: nil)

    override func loadView() {
        let title = NSTextField(labelWithString: L("Adjust"))
        title.font = .systemFont(ofSize: 13, weight: .semibold)

        autoButton.target = self
        autoButton.action = #selector(autoChanged)
        autoButton.toolTip = L("Let the computer balance exposure, colour and contrast")

        let lookLabel = NSTextField(labelWithString: L("Look"))
        lookLabel.font = .systemFont(ofSize: 12)
        lookLabel.textColor = .secondaryLabelColor
        for look in Adjustments.Look.allCases { lookPopup.addItem(withTitle: look.title) }
        lookPopup.controlSize = .small
        lookPopup.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        lookPopup.target = self
        lookPopup.action = #selector(lookChanged)
        let lookRow = NSStackView(views: [lookLabel, NSView(), lookPopup])

        var rows: [NSView] = [title, autoButton, lookRow, separator()]
        let groups: [[Adjustments.Key]] = [[.exposure, .brightness, .contrast, .highlights, .shadows],
                                           [.saturation, .vibrance, .temperature, .tint],
                                           [.sharpness, .vignette]]
        for (gi, group) in groups.enumerated() {
            if gi > 0 { rows.append(separator()) }
            for key in group { rows.append(row(for: key)) }
        }

        let reset = NSButton(title: L("Reset All"), target: self, action: #selector(resetAll))
        reset.bezelStyle = .push
        reset.controlSize = .regular
        let original = HoldButton(title: L("Show Original"), target: nil, action: nil)
        original.bezelStyle = .push
        original.toolTip = L("Hold to compare with the original")
        original.onChange = { [weak self] on in self?.onShowOriginal?(on) }
        let buttons = NSStackView(views: [original, reset])
        buttons.distribution = .fillEqually
        buttons.spacing = 8
        rows += [separator(), buttons]

        let stack = NSStackView(views: rows)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        stack.setCustomSpacing(14, after: title)
        stack.translatesAutoresizingMaskIntoConstraints = false
        for v in rows where !(v === title) && !(v === autoButton) {
            v.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        }

        let document = FlippedView()
        document.translatesAutoresizingMaskIntoConstraints = false
        document.addSubview(stack)
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.documentView = document
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: document.topAnchor, constant: 16),
            stack.leadingAnchor.constraint(equalTo: document.leadingAnchor, constant: 16),
            stack.trailingAnchor.constraint(equalTo: document.trailingAnchor, constant: -16),
            stack.bottomAnchor.constraint(equalTo: document.bottomAnchor, constant: -16),
            document.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor),
        ])
        view = scroll
    }

    private func separator() -> NSView {
        let box = NSBox()
        box.boxType = .separator
        return box
    }

    private func row(for key: Adjustments.Key) -> NSView {
        let name = NSTextField(labelWithString: key.title)
        name.font = .systemFont(ofSize: 12)
        let value = NSTextField(labelWithString: "0")
        value.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
        value.textColor = .secondaryLabelColor
        value.alignment = .right
        let header = NSStackView(views: [name, NSView(), value])
        let slider = ResettingSlider(value: 0, minValue: key.range.lowerBound, maxValue: key.range.upperBound, target: self, action: #selector(sliderMoved(_:)))
        slider.isContinuous = true
        slider.controlSize = .small
        slider.toolTip = L("Double-click to reset")
        slider.onBegin = { [weak self] in self?.onBeginChange?() }
        slider.onEnd = { [weak self] in
            guard let self else { return }
            self.onChange?(self.adjustments, true)
        }
        slider.onReset = { [weak self] in
            guard let self, self.adjustments[key] != 0 else { return }
            self.onBeginChange?()
            self.adjustments[key] = 0
            self.sync()
            self.onChange?(self.adjustments, true)
        }
        sliders[key] = slider
        valueLabels[key] = value
        slider.identifier = NSUserInterfaceItemIdentifier(key.rawValue)
        let stack = NSStackView(views: [header, slider])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 2
        header.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        slider.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        return stack
    }

    func set(_ adjustments: Adjustments) {
        self.adjustments = adjustments
        if isViewLoaded { sync() }
    }

    private func sync() {
        for (key, slider) in sliders {
            slider.doubleValue = adjustments[key]
            valueLabels[key]?.stringValue = Self.format(adjustments[key])
        }
        lookPopup.selectItem(at: adjustments.look.rawValue)
        autoButton.state = adjustments.autoEnhance ? .on : .off
    }

    static func format(_ v: Double) -> String {
        let i = Int(v.rounded())
        return i > 0 ? "+\(i)" : "\(i)"
    }

    override func viewWillAppear() {
        super.viewWillAppear()
        sync()
    }

    @objc private func sliderMoved(_ sender: ResettingSlider) {
        guard let raw = sender.identifier?.rawValue, let key = Adjustments.Key(rawValue: raw) else { return }
        // Keyboard/accessibility actions do not pass through mouseDown's begin/end callbacks.
        if !sender.isTrackingDrag { onBeginChange?() }
        adjustments[key] = sender.doubleValue.rounded()
        valueLabels[key]?.stringValue = Self.format(adjustments[key])
        onChange?(adjustments, !sender.isTrackingDrag)
    }

    @objc private func lookChanged() {
        onBeginChange?()
        adjustments.look = Adjustments.Look(rawValue: lookPopup.indexOfSelectedItem) ?? .none
        onChange?(adjustments, true)
    }

    @objc private func autoChanged() {
        onBeginChange?()
        adjustments.autoEnhance = autoButton.state == .on
        onChange?(adjustments, true)
    }

    @objc private func resetAll() {
        guard !adjustments.isIdentity else { return }
        onBeginChange?()
        adjustments = Adjustments()
        sync()
        onChange?(adjustments, true)
    }
}

final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}
