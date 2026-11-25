import Kit42
import AppKit
import PDFKit

enum Tool: Int, CaseIterable {
    case select, highlight, underline, strikeOut, ink, rectangle, oval, whiteout, text, note
    case editText, redact, textField, checkBox, choiceField, link, crop

    var title: String {
        switch self {
        case .select: return L("Select and Highlight Text")
        case .highlight: return L("Highlight")
        case .underline: return L("Underline")
        case .strikeOut: return L("Strikethrough")
        case .ink: return L("Pencil")
        case .rectangle: return L("Rectangle")
        case .oval: return L("Oval")
        case .whiteout: return L("Whiteout")
        case .text: return L("Text Box")
        case .note: return L("Note")
        case .editText: return L("Edit Text")
        case .redact: return L("Redact")
        case .textField: return L("Text Field")
        case .checkBox: return L("Checkbox")
        case .choiceField: return L("Dropdown List")
        case .link: return L("Link")
        case .crop: return L("Crop Tool")
        }
    }

    var symbol: String {
        switch self {
        case .select: return "cursorarrow"
        case .highlight: return "highlighter"
        case .underline: return "underline"
        case .strikeOut: return "strikethrough"
        case .ink: return "scribble"
        case .rectangle: return "rectangle"
        case .oval: return "circle"
        case .whiteout: return "eraser"
        case .text: return "character.textbox"
        case .note: return "note.text"
        case .editText: return "character.cursor.ibeam"
        case .redact: return "rectangle.inset.filled"
        case .textField: return "rectangle.and.pencil.and.ellipsis"
        case .checkBox: return "checkmark.square"
        case .choiceField: return "filemenu.and.selection"
        case .link: return "link"
        case .crop: return "crop"
        }
    }

    var defaultColor: NSColor {
        switch self {
        case .highlight, .note: return .systemYellow
        case .underline: return .systemBlue
        case .text: return .black
        case .whiteout: return .white
        case .redact: return .black
        case .link: return .systemBlue
        default: return .systemRed
        }
    }

    var isTextMarkup: Bool { self == .highlight || self == .underline || self == .strikeOut }

    /// Tools that place new AcroForm fields.
    var isFormField: Bool { self == .textField || self == .checkBox || self == .choiceField }

    /// Tools that only drag out a rectangle (no colour).
    var isAreaTool: Bool { self == .link || self == .crop }

    var fieldKind: PDFTools.FieldKind { self == .checkBox ? .checkBox : self == .choiceField ? .choice : .text }
}

extension PDFAnnotation {
    /// Annotation subtype without the leading slash ("Highlight", "Ink", ...).
    var kind: String {
        (type ?? "").trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    }

    func isKind(_ subtype: PDFAnnotationSubtype) -> Bool {
        kind == subtype.rawValue.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    }

    var isMarkup: Bool { isKind(.highlight) || isKind(.underline) || isKind(.strikeOut) }

    /// Annotations PDFView should keep handling itself (form fields, links, popups).
    var isSystemHandled: Bool { isKind(.widget) || isKind(.link) || isKind(.popup) }

    var isResizable: Bool { self is RedactOverlay || !(self is PageOverlay) && (isKind(.freeText) || isKind(.square) || isKind(.circle) || isKind(.stamp) || isKind(.widget)) }

    var keepsAspectRatio: Bool { isKind(.stamp) && !(self is PageOverlay) }

    var hasEditableBorder: Bool {
        !(self is PageOverlay) && !isWhiteout && (isKind(.square) || isKind(.circle) || isKind(.ink) || isKind(.widget))
    }
}

extension PDFPage {
    static func blank(size: CGSize = CGSize(width: 595, height: 842)) -> PDFPage {
        let page = PDFPage()
        page.setBounds(CGRect(origin: .zero, size: size), for: .mediaBox)
        return page
    }
}

/// Stamp annotation that draws a raster image (signatures, seals). PDFKit writes the drawing
/// into the annotation's appearance stream on save, so other viewers show it too.
final class ImageStampAnnotation: PDFAnnotation {
    let image: CGImage
    /// Multiply (ink); `.normal` only for the before/after demo.
    var blendMode: CGBlendMode = .multiply

    init(image: CGImage, bounds: CGRect) {
        self.image = image
        super.init(bounds: bounds, forType: .stamp, withProperties: nil)
    }

    required init?(coder: NSCoder) { nil }

    override func draw(with box: PDFDisplayBox, in context: CGContext) {
        context.saveGState()
        context.interpolationQuality = .high
        // Ink on paper multiplies with what is under it: text stays dark, overlapping inks darken each other.
        // Quartz writes this as /BM /Multiply in the saved appearance stream.
        context.setBlendMode(blendMode)
        context.draw(image, in: bounds)
        context.restoreGState()
    }
}
