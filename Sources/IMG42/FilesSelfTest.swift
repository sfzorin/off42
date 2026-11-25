import AppKit
import ImageIO
import UniformTypeIdentifiers

enum ImageFileSelfTest {
    static func fixture(width: Int = 96, height: Int = 64, transparent: Bool = false) -> CGImage {
        let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        if !transparent { context.setFillColor(CGColor(gray: 1, alpha: 1)); context.fill(CGRect(x: 0, y: 0, width: width, height: height)) }
        context.setFillColor(CGColor(red: 1, green: 0, blue: 0, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width / 2, height: height))
        return context.makeImage()!
    }

    static func sample(_ image: CGImage, x: Int, y: Int) -> [UInt8] {
        let context = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8,
                                bytesPerRow: image.width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        let bytes = context.data!.bindMemory(to: UInt8.self, capacity: image.width * image.height * 4)
        return Array(UnsafeBufferPointer(start: bytes + (y * image.width + x) * 4, count: 4))
    }

    static func run() -> Int {
        var count = 0, failures = 0
        func check(_ condition: Bool, _ name: String) { count += 1; if !condition { failures += 1 }; print(condition ? "PASS" : "FAIL", "IMG files:", name) }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("IMG42-files-" + UUID().uuidString)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            defer { try? FileManager.default.removeItem(at: folder) }
            for size in [CGSize.zero, CGSize(width: -1, height: 10), CGSize(width: CGFloat.nan, height: 10),
                         CGSize(width: CGFloat.infinity, height: 10), CGSize(width: 20_000, height: 2), CGSize(width: 12_000, height: 12_000)] {
                check(ImageLoader.render(size: size, { _ in }) == nil, "invalid or excessive allocation rejected before conversion")
            }
            let original = fixture(), alpha = fixture(transparent: true)
            check(!ImageWriter.hasTransparency(original), "an opaque RGBA bitmap is not mistaken for transparent pixels")
            check(ImageWriter.hasTransparency(alpha), "actual transparent pixels are detected")
            check(ImageLoader.scaled(original, maxPixels: 0) == nil, "invalid thumbnail limit rejected")
            let png = folder.appendingPathComponent("source.png")
            try ImageWriter.write(alpha, to: png, type: UTType.png.identifier, description: .some("Audit caption"), dpi: 144)
            let loaded = ImageLoader.load(png)
            check(loaded.image?.width == 96 && loaded.image?.height == 64 && loaded.hasAlpha, "PNG preserves dimensions and alpha")
            check(loaded.image.map { sample($0, x: 80, y: 30)[3] == 0 } == true, "PNG retains transparent pixels")
            check(abs((ImageLoader.properties(png)[kCGImagePropertyDPIWidth] as? Double ?? 0) - 144) < 0.05, "PNG retains authored DPI")
            check(ImageWindowController.description(of: png) == "Audit caption", "description metadata round trips")
            let jpeg = folder.appendingPathComponent("white.jpg")
            try ImageWriter.write(alpha, to: jpeg, type: UTType.jpeg.identifier, quality: 1, metadataFrom: png)
            let jpg = ImageLoader.load(jpeg)
            check(jpg.image.map { sample($0, x: 80, y: 30).prefix(3).allSatisfy { $0 > 245 } } == true, "JPEG flattens transparency onto white")
            check(jpg.orientation == .up, "written pixels have upright orientation metadata")
            check(abs((ImageLoader.properties(jpeg)[kCGImagePropertyDPIWidth] as? Double ?? 0) - 144) < 0.05, "pixel export retains source print resolution")
            try ImageWriter.write(original, to: png, type: UTType.png.identifier, metadataFrom: png, description: .some(nil), dpi: 300)
            check(ImageWindowController.description(of: png) == nil, "an explicit empty caption clears existing metadata")
            check(abs((ImageLoader.properties(png)[kCGImagePropertyDPIWidth] as? Double ?? 0) - 300) < 0.05, "same-path replacement uses new DPI")
            check(ImageLoader.load(png, page: -1).image == nil, "negative image page rejected")
            let sentinel = Data("KEEP DESTINATION".utf8), target = folder.appendingPathComponent("existing.png")
            for (type, quality, dpi) in [("com.invalid.codec", 0.9, 72.0), (UTType.png.identifier, Double.nan, 72.0),
                                         (UTType.png.identifier, 2.0, 72.0), (UTType.png.identifier, 0.9, Double.infinity)] {
                try sentinel.write(to: target)
                do { try ImageWriter.write(original, to: target, type: type, quality: quality, dpi: CGFloat(dpi)); check(false, "invalid write must throw") }
                catch { check((try? Data(contentsOf: target)) == sentinel, "invalid format/quality/DPI preserves existing bytes") }
            }
            let directory = folder.appendingPathComponent("destination.png")
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            let child = directory.appendingPathComponent("keep"); try sentinel.write(to: child)
            do { try ImageWriter.write(original, to: directory, type: UTType.png.identifier); check(false, "directory destination must fail") }
            catch { check((try? Data(contentsOf: child)) == sentinel, "failed write never replaces a destination directory") }
            let missingParent = folder.appendingPathComponent("absent/out.png")
            do { try ImageWriter.write(original, to: missingParent, type: UTType.png.identifier); check(false, "missing destination directory must fail") }
            catch { check(!FileManager.default.fileExists(atPath: missingParent.path), "failed write leaves no partial output") }
            check((try FileManager.default.contentsOfDirectory(atPath: folder.path)).allSatisfy { !$0.hasSuffix(".tmp") }, "failed writes clean up their staging files")

            let large = folder.appendingPathComponent("wide.png")
            try ImageWriter.write(fixture(width: 17_000, height: 2), to: large, type: UTType.png.identifier)
            let proxy = ImageLoader.load(large)
            check(proxy.wasDownsampled && proxy.image != nil && (proxy.image?.width ?? 20_000) <= ImageLoader.maxDisplayDimension,
                  "oversized source is bounded before full decoding and labelled as a proxy")
            check(proxy.pixelSize.width == 17_000, "proxy retains original source dimensions for reporting")
            let wideAnimation = folder.appendingPathComponent("wide.gif")
            guard let animatedDestination = CGImageDestinationCreateWithURL(wideAnimation as CFURL, UTType.gif.identifier as CFString, 2, nil) else { throw CocoaError(.fileWriteUnknown) }
            CGImageDestinationAddImage(animatedDestination, fixture(width: 17_000, height: 2), nil)
            CGImageDestinationAddImage(animatedDestination, fixture(width: 17_000, height: 2, transparent: true), nil)
            guard CGImageDestinationFinalize(animatedDestination) else { throw CocoaError(.fileWriteUnknown) }
            let animationProxy = ImageLoader.load(wideAnimation)
            check(animationProxy.isAnimated && animationProxy.pages.count == 2 && animationProxy.wasDownsampled,
                  "oversized animated frames are also labelled as reduced previews")

            let tiff = folder.appendingPathComponent("pages.tiff")
            guard let destination = CGImageDestinationCreateWithURL(tiff as CFURL, UTType.tiff.identifier as CFString, 2, nil) else { throw CocoaError(.fileWriteUnknown) }
            CGImageDestinationAddImage(destination, original, nil)
            CGImageDestinationAddImage(destination, fixture(width: 40, height: 60), nil)
            guard CGImageDestinationFinalize(destination) else { throw CocoaError(.fileWriteUnknown) }
            let second = ImageLoader.load(tiff, page: 1)
            check(second.pageCount == 2 && second.image?.width == 40 && second.image?.height == 60, "multipage TIFF opens the requested page")
            check(Orientation.writeMetadata(description: "Whole file", to: tiff), "metadata-only TIFF write succeeds")
            check(ImageLoader.load(tiff).pageCount == 2 && ImageLoader.load(tiff, page: 1).image?.height == 60, "metadata-only save retains every TIFF page")
            check(!Orientation.rewriteLosslessly(tiff, orientation: .right), "single-image orientation rewrite rejects multipage files")
            check(!Orientation.writeMetadata(description: "Invalid DPI", resolution: .nan, to: tiff), "nonfinite metadata resolution is rejected")
            let bytes = try Data(contentsOf: png)
            CLI.edit([png.path, target.path, "--unknown"])
            check(CLI.status != 0 && (try? Data(contentsOf: target)) == sentinel, "unknown edit option does not overwrite a destination")
            check((try Data(contentsOf: png)) == bytes, "failed CLI input validation preserves source bytes")
        } catch { check(false, "fixture error: " + error.localizedDescription) }
        print("IMG42 file/output regressions: \(count - failures)/\(count) passed")
        return failures
    }
}
