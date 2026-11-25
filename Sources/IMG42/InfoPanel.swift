import AppKit
import ImageIO
import Kit42

/// Side panel with file, camera and location information plus every metadata field.
final class InfoPanelController: NSViewController {
    private let stack = NSStackView()
    private var mapsURL: URL?

    static func exposureDescription(_ seconds: Double) -> String? {
        guard seconds.isFinite, seconds > 0 else { return nil }
        if seconds >= 1 { return String(format: "%g s", seconds) }
        let denominator = (1 / seconds).rounded()
        guard denominator.isFinite, denominator < Double(Int.max) else { return String(format: "%g s", seconds) }
        return "1/\(Int(denominator)) s"
    }

    override func loadView() {
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
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

    func show(url: URL?, loaded: LoadedImage?) {
        _ = view
        stack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        mapsURL = nil
        guard let url else { return }
        let p = ImageLoader.properties(url)
        let exif = p[kCGImagePropertyExifDictionary] as? [CFString: Any] ?? [:]
        let tiff = p[kCGImagePropertyTIFFDictionary] as? [CFString: Any] ?? [:]
        let gps = p[kCGImagePropertyGPSDictionary] as? [CFString: Any] ?? [:]
        let aux = p[kCGImagePropertyExifAuxDictionary] as? [CFString: Any] ?? [:]
        let values = try? url.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey, .creationDateKey])

        let title = NSTextField(wrappingLabelWithString: url.lastPathComponent)
        title.font = .systemFont(ofSize: 14, weight: .semibold)
        title.isSelectable = true
        add(title)
        let date = DateFormatter()
        date.dateStyle = .medium
        date.timeStyle = .short

        var file: [(String, String)] = []
        if let size = values?.fileSize { file.append((L("Size"), ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file))) }
        if let l = loaded {
            let w = Int(l.pixelSize.width), h = Int(l.pixelSize.height)
            let shown = Orientation.swapsAxes(l.orientation) ? "\(h) × \(w)" : "\(w) × \(h)"
            if w > 0 { file.append((L("Dimensions"), shown + " px")) }
            file.append((L("Format"), ImageFormats.formatName(url, typeIdentifier: l.typeIdentifier)))
            if l.isAnimated { file.append((L("Frames"), "\(l.pages.count)")) } else if l.pageCount > 1 { file.append((L("Pages"), "\(l.pageCount)")) }
            if l.orientation != .up { file.append((L("Orientation"), "\(l.orientation.rawValue)")) }
        }
        if let profile = p[kCGImagePropertyProfileName] as? String { file.append((L("Colour Profile"), profile)) }
        else if let model = p[kCGImagePropertyColorModel] as? String { file.append((L("Colour Model"), model)) }
        if let depth = p[kCGImagePropertyDepth] as? Int { file.append((L("Bit Depth"), "\(depth)")) }
        if let dpi = p[kCGImagePropertyDPIWidth] as? Double, dpi.isFinite, dpi > 0 { file.append((L("Resolution"), String(format: "%.0f dpi", dpi))) }
        if let alpha = p[kCGImagePropertyHasAlpha] as? Bool, alpha { file.append((L("Transparency"), L("Yes"))) }
        if let d = values?.contentModificationDate { file.append((L("Modified"), date.string(from: d))) }
        if let d = values?.creationDate { file.append((L("Created"), date.string(from: d))) }
        section(L("File"), file)

        var camera: [(String, String)] = []
        if let taken = ImageLoader.dateTaken(url) { camera.append((L("Date Taken"), date.string(from: taken))) }
        let make = (tiff[kCGImagePropertyTIFFMake] as? String ?? "").trimmingCharacters(in: .whitespaces)
        let model = (tiff[kCGImagePropertyTIFFModel] as? String ?? "").trimmingCharacters(in: .whitespaces)
        if !make.isEmpty || !model.isEmpty { camera.append((L("Camera"), model.hasPrefix(make) ? model : "\(make) \(model)".trimmingCharacters(in: .whitespaces))) }
        if let lens = exif[kCGImagePropertyExifLensModel] as? String ?? aux[kCGImagePropertyExifAuxLensModel] as? String { camera.append((L("Lens"), lens)) }
        if let f = exif[kCGImagePropertyExifFocalLength] as? Double {
            var s = String(format: "%g mm", f)
            if let f35 = exif[kCGImagePropertyExifFocalLenIn35mmFilm] as? Int { s += " (\(f35) mm)" }
            camera.append((L("Focal Length"), s))
        }
        var exposure: [String] = []
        if let t = exif[kCGImagePropertyExifExposureTime] as? Double, let value = Self.exposureDescription(t) { exposure.append(value) }
        if let fn = exif[kCGImagePropertyExifFNumber] as? Double { exposure.append(String(format: "ƒ/%g", fn)) }
        if let iso = (exif[kCGImagePropertyExifISOSpeedRatings] as? [Int])?.first { exposure.append("ISO \(iso)") }
        if let bias = exif[kCGImagePropertyExifExposureBiasValue] as? Double, bias != 0 { exposure.append(String(format: "%+.1f EV", bias)) }
        if !exposure.isEmpty { camera.append((L("Exposure"), exposure.joined(separator: "  "))) }
        if let flash = exif[kCGImagePropertyExifFlash] as? Int { camera.append((L("Flash"), flash & 1 == 1 ? L("Fired") : L("Did not fire"))) }
        section(L("Camera"), camera)

        if let lat = gps[kCGImagePropertyGPSLatitude] as? Double, let lon = gps[kCGImagePropertyGPSLongitude] as? Double,
           lat.isFinite, lon.isFinite, (0...90).contains(lat), (0...180).contains(lon) {
            let la = (gps[kCGImagePropertyGPSLatitudeRef] as? String) == "S" ? -lat : lat
            let lo = (gps[kCGImagePropertyGPSLongitudeRef] as? String) == "W" ? -lon : lon
            var rows = [(L("Coordinates"), String(format: "%.5f, %.5f", la, lo))]
            if let alt = gps[kCGImagePropertyGPSAltitude] as? Double, alt.isFinite {
                let sign: Double = (gps[kCGImagePropertyGPSAltitudeRef] as? Int) == 1 ? -1 : 1
                rows.append((L("Altitude"), String(format: "%.0f m", alt * sign)))
            }
            section(L("Location"), rows)
            mapsURL = URL(string: String(format: "maps://?ll=%.6f,%.6f&q=%.6f,%.6f", la, lo, la, lo))
            let maps = NSButton(title: L("Show in Maps"), target: self, action: #selector(openMaps))
            maps.bezelStyle = .push
            maps.image = NSImage(systemSymbolName: "map", accessibilityDescription: nil)
            maps.imagePosition = .imageLeading
            add(maps)
        }

        // Everything else, as ImageIO reports it.
        var all: [(String, String)] = []
        func flatten(_ dict: [CFString: Any], prefix: String) {
            for (k, v) in dict.sorted(by: { ($0.key as String) < ($1.key as String) }) {
                let key = (k as String).replacingOccurrences(of: "{", with: "").replacingOccurrences(of: "}", with: "")
                if let sub = v as? [CFString: Any] { flatten(sub, prefix: key + " › ") } else { all.append((prefix + key, Self.describe(v))) }
            }
        }
        flatten(p, prefix: "")
        // The raw ImageIO dictionary repeats the File section, so it starts collapsed; the choice is remembered.
        if let grid = section(L("All Metadata"), all, small: true), let header = stack.arrangedSubviews.dropLast().last as? NSTextField {
            let expanded = UserDefaults.standard.bool(forKey: Self.allMetadataKey)
            let toggle = NSButton(title: header.stringValue, target: self, action: #selector(toggleAllMetadata(_:)))
            toggle.setButtonType(.pushOnPushOff)
            toggle.bezelStyle = .disclosure
            toggle.title = ""
            toggle.state = expanded ? .on : .off
            if let i = stack.arrangedSubviews.firstIndex(of: header) {
                let spacing = stack.customSpacing(after: header)
                header.removeFromSuperview()
                // Constructing a stack with `header` reparents it immediately: capture its old index first.
                let row = NSStackView(views: [toggle, header])
                row.spacing = 2
                stack.insertArrangedSubview(row, at: i)
                stack.setCustomSpacing(spacing, after: row)
                if i > 0 { stack.setCustomSpacing(16, after: stack.arrangedSubviews[i - 1]) }
            }
            grid.isHidden = !expanded
            allMetadataGrid = grid
        }
    }

    private static let allMetadataKey = "img.allMetadataExpanded"
    private weak var allMetadataGrid: NSView?
    var metadataIsExpanded: Bool { allMetadataGrid?.isHidden == false }

    @objc private func toggleAllMetadata(_ sender: NSButton) {
        allMetadataGrid?.isHidden = sender.state != .on
        UserDefaults.standard.set(sender.state == .on, forKey: Self.allMetadataKey)
    }

    static func describe(_ v: Any) -> String {
        if let a = v as? [Any] { return a.map { describe($0) }.joined(separator: ", ") }
        if let d = v as? Double { return d == d.rounded() && abs(d) < 1e12 ? String(Int(d)) : String(format: "%g", d) }
        if let data = v as? Data { return "\(data.count) bytes" }
        return "\(v)"
    }

    private func add(_ v: NSView) {
        stack.addArrangedSubview(v)
    }

    @discardableResult
    private func section(_ title: String, _ rows: [(String, String)], small: Bool = false) -> NSView? {
        guard !rows.isEmpty else { return nil }
        let header = NSTextField(labelWithString: title.uppercased())
        header.font = .systemFont(ofSize: 10, weight: .semibold)
        header.textColor = .secondaryLabelColor
        stack.addArrangedSubview(header)
        stack.setCustomSpacing(4, after: header)
        if let prev = stack.arrangedSubviews.dropLast().last { stack.setCustomSpacing(16, after: prev) }
        let size: CGFloat = small ? 10.5 : 12
        let grid = NSGridView(views: rows.map { row in
            let k = NSTextField(labelWithString: row.0)
            k.font = .systemFont(ofSize: size)
            k.textColor = .secondaryLabelColor
            k.lineBreakMode = .byTruncatingMiddle
            k.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            let v = NSTextField(wrappingLabelWithString: row.1)
            v.font = .systemFont(ofSize: size)
            v.isSelectable = true
            v.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            return [k, v]
        })
        grid.rowSpacing = small ? 3 : 5
        grid.columnSpacing = 8
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = small ? 120 : 92
        grid.rowAlignment = .firstBaseline
        stack.addArrangedSubview(grid)
        grid.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        return grid
    }

    @objc private func openMaps() {
        if let mapsURL { NSWorkspace.shared.open(mapsURL) }
    }
}
