import Kit42
import PDFKit

/// Resolution of a scanned page, for drawing signatures and stamps as sharp as the scan around them.
enum ScanResolution {
    /// The scan's resolution when the page is a scan: an image (on the page or in a form on it) shaped like the page
    /// and covering most of it, as scanners and camera apps write pages. Nil for pages drawn in lines and text.
    static func dpi(of page: PDFPage) -> CGFloat? {
        guard let dict = page.pageRef?.dictionary, let res = CGPDF.inherited(dict, "Resources") else { return nil }
        let box = page.bounds(for: .mediaBox)
        guard box.width > 1, box.height > 1 else { return nil }
        var sizes: [(Double, Double)] = []
        collect(res, into: &sizes, depth: 0)
        var best: (area: Double, dpi: CGFloat)?
        for (w, h) in sizes {
            // Either orientation (a page may be rotated against its image).
            for (pw, ph) in [(box.width, box.height), (box.height, box.width)] {
                let dx = w / Double(pw / 72), dy = h / Double(ph / 72)
                guard dx > 0, abs(dx - dy) / dx < 0.05 else { continue }
                let dpi = CGFloat((dx + dy) / 2)
                guard dpi >= 50, dpi <= 2400, best == nil || w * h > best!.area else { continue }
                best = (w * h, dpi)
            }
        }
        return best.map { min(1200, $0.dpi) }
    }

    private static func collect(_ res: CGPDFDictionaryRef, into sizes: inout [(Double, Double)], depth: Int) {
        guard depth < 4, let xo = CGPDF.dict(res, "XObject") else { return }
        CGPDFDictionaryApplyBlock(xo, { _, obj, _ in
            var s: CGPDFStreamRef?
            guard CGPDFObjectGetValue(obj, .stream, &s), let s, let d = CGPDFStreamGetDictionary(s) else { return true }
            switch CGPDF.name(d, "Subtype") {
            case "Image":
                if let w = CGPDF.number(d, "Width"), let h = CGPDF.number(d, "Height"), w >= 200, h >= 200 { sizes.append((w, h)) }
            case "Form":
                if let inner = CGPDF.dict(d, "Resources") { collect(inner, into: &sizes, depth: depth + 1) }
            default: break
            }
            return true
        }, nil)
    }
}
