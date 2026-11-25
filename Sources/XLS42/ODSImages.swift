import AppKit
import Kit42

extension ODS {
    static func embeddedImageData(_ data: Data) -> Data {
        if data.starts(with: [0x89, 0x50, 0x4E, 0x47]) { return data }
        guard let image = NSImage(data: data), let tiff = image.tiffRepresentation,
              let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) else { return data }
        return png
    }
    /// Cell-anchored embedded images retain the original PNG, including technical-object recipes.
    static func readImages(_ cell: XMLElement, anchor: CellRef, zip: ZipArchive) throws -> [SheetImage] {
        var frames = cell.children("frame")
        for paragraph in cell.children("p") { frames += paragraph.children("frame") }
        guard frames.count <= 1_000 else { throw ZipError.corrupt }
        func points(_ raw: String?) -> CGFloat? {
            guard let raw else { return nil }
            for (unit, factor) in [("cm", 72.0 / 2.54), ("mm", 72.0 / 25.4), ("in", 72.0), ("pt", 1.0), ("px", 0.75)] where raw.hasSuffix(unit) {
                guard let value = Double(raw.dropLast(unit.count)), value.isFinite else { return nil }
                return CGFloat(value * factor)
            }
            return nil
        }
        var result: [SheetImage] = []
        for frame in frames {
            guard let image = frame.child("image"), let href = image.attr("href"),
                  let data = zip[href.hasPrefix("./") ? String(href.dropFirst(2)) : href],
                  let width = points(frame.attr("width")), let height = points(frame.attr("height")),
                  width > 0, height > 0, width <= 144_000, height <= 144_000 else { continue }
            let x = points(frame.attr("x")) ?? 0, y = points(frame.attr("y")) ?? 0
            guard abs(x) <= 144_000, abs(y) <= 144_000 else { continue }
            result.append(SheetImage(anchor: anchor, offset: CGPoint(x: x, y: y), size: CGSize(width: width, height: height), data: data))
        }
        return result
    }

    static func imageXML(_ image: SheetImage, path: String, index: Int) -> String {
        func pt(_ value: CGFloat) -> String { Value.numberLiteral(Double(value)) + "pt" }
        let description = TechnicalElement.fromPNG(image.data)?.encodedDescription
        return "<draw:frame draw:name=\"Picture \(index)\" svg:x=\"\(pt(image.offset.x))\" svg:y=\"\(pt(image.offset.y))\" svg:width=\"\(pt(image.size.width))\" svg:height=\"\(pt(image.size.height))\">"
            + (description.map { "<svg:desc>\(xmlEscape($0))</svg:desc>" } ?? "")
            + "<draw:image xlink:href=\"\(xmlEscape(path))\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>"
    }
}
