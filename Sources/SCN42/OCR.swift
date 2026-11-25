import CoreGraphics
import CoreText
import Foundation
import Vision

/// One recognised line: text and its box in normalised image coordinates (origin bottom-left, as Vision).
struct TextLine: Codable, Equatable {
    var text: String
    var box: CGRect
}

/// Text recognition with Vision (adapted from PDF42's searchable-PDF OCR).
enum OCR {
    /// Russian, English and Turkish first, then the user's preferred languages Vision knows.
    static let languages: [String] = {
        var wanted = ["ru-RU", "en-US", "tr-TR"]
        let supported = (try? VNRecognizeTextRequest().supportedRecognitionLanguages()) ?? wanted
        for code in Locale.preferredLanguages {
            let base = code.split(separator: "-").first.map(String.init) ?? code
            if let match = supported.first(where: { $0 == code || $0.hasPrefix(base + "-") || $0 == base }), !wanted.contains(match) {
                wanted.append(match)
            }
        }
        return wanted.filter { supported.contains($0) }
    }()

    static func recognize(_ image: CGImage) -> [TextLine] {
        (try? recognizeChecked(image)) ?? []
    }

    /// Callers that save/cache recognition must distinguish an empty page from a failed Vision request.
    static func recognizeChecked(_ image: CGImage) throws -> [TextLine] {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = true
        request.recognitionLanguages = languages
        try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
        return readingOrder((request.results ?? []).compactMap { o in
            guard let s = o.topCandidates(1).first?.string, !s.isEmpty else { return nil }
            return TextLine(text: s, box: o.boundingBox)
        })
    }

    /// Lines top to bottom as plain text.
    static func text(_ lines: [TextLine]) -> String {
        readingOrder(lines).map(\.text).joined(separator: "\n")
    }

    /// Stable geometric reading order, independent of Vision's observation array order.
    static func readingOrder(_ lines: [TextLine]) -> [TextLine] {
        let sorted = lines.enumerated().sorted { a, b in
            let ay = a.element.box.midY, by = b.element.box.midY
            if ay.isFinite != by.isFinite { return ay.isFinite }
            if ay.isFinite, ay != by { return ay > by }
            return a.offset < b.offset
        }.map(\.element)
        var rows: [[TextLine]] = []
        for line in sorted {
            if let anchor = rows.last?.first, anchor.box.height > 0, line.box.height > 0,
               abs(anchor.box.midY - line.box.midY) < min(anchor.box.height, line.box.height) * 0.4 {
                rows[rows.count - 1].append(line)
            } else { rows.append([line]) }
        }
        return rows.flatMap { row in
            row.enumerated().sorted { a, b in
                let ax = a.element.box.minX, bx = b.element.box.minX
                if ax.isFinite != bx.isFinite { return ax.isFinite }
                if ax.isFinite, ax != bx { return ax < bx }
                return a.offset < b.offset
            }.map(\.element)
        }
    }

    /// First line that looks like a title (for file names): at least 3 letters, trimmed to 60 characters.
    static func title(_ lines: [TextLine]) -> String? {
        let sorted = readingOrder(lines)
        guard let line = sorted.first(where: { $0.text.filter(\.isLetter).count >= 3 }) else { return nil }
        let cleaned = line.text.components(separatedBy: CharacterSet(charactersIn: "/\\:*?\"<>|\n\t")).joined(separator: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines.union(.punctuationCharacters))
        return cleaned.isEmpty ? nil : String(cleaned.prefix(60)).trimmingCharacters(in: .whitespaces)
    }

    /// Invisible (searchable, selectable) text over the recognised lines; `box` is the page area the image covers.
    static func drawInvisibleText(_ lines: [TextLine], box: CGRect, in ctx: CGContext) {
        guard box.origin.x.isFinite, box.origin.y.isFinite, box.width.isFinite, box.height.isFinite,
              box.width > 0, box.height > 0 else { return }
        ctx.saveGState()
        ctx.setTextDrawingMode(.invisible)
        for line in readingOrder(lines) {
            guard line.box.origin.x.isFinite, line.box.origin.y.isFinite, line.box.width.isFinite,
                  line.box.height.isFinite, line.box.width > 0, line.box.height > 0 else { continue }
            let b = line.box.intersection(CGRect(x: 0, y: 0, width: 1, height: 1))
            guard !b.isNull, b.width > 0, b.height > 0 else { continue }
            let r = CGRect(x: box.minX + b.minX * box.width, y: box.minY + b.minY * box.height, width: b.width * box.width, height: b.height * box.height)
            let font = CTFontCreateWithName("Helvetica" as CFString, max(1, r.height * 0.8), nil)
            let ct = CTLineCreateWithAttributedString(NSAttributedString(string: line.text, attributes: [kCTFontAttributeName as NSAttributedString.Key: font]))
            let width = CGFloat(CTLineGetTypographicBounds(ct, nil, nil, nil))
            ctx.saveGState()
            ctx.translateBy(x: r.minX, y: r.minY + r.height * 0.18)
            if width > 0 { ctx.scaleBy(x: r.width / width, y: 1) }
            ctx.textPosition = .zero
            CTLineDraw(ct, ctx)
            ctx.restoreGState()
        }
        ctx.restoreGState()
    }
}
