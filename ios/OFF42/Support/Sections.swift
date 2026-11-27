import SwiftUI

/// The six sections of the app, as on Android and the web.
enum AppSection: String, CaseIterable, Identifiable, Hashable {
    case documents, spreadsheets, presentations, pdf, labels, signatures

    var id: String { rawValue }

    var title: String {
        switch self {
        case .documents: return tr("section_documents")
        case .spreadsheets: return tr("section_spreadsheets")
        case .presentations: return tr("section_presentations")
        case .pdf: return "PDF"
        case .labels: return tr("section_labels")
        case .signatures: return tr("section_marks")
        }
    }

    var symbol: String {
        switch self {
        case .documents: return "doc.text"
        case .spreadsheets: return "tablecells"
        case .presentations: return "rectangle.on.rectangle"
        case .pdf: return "doc.richtext"
        case .labels: return "qrcode"
        case .signatures: return "signature"
        }
    }

    /// Accent colours of the approved redesign, one per app.
    var accent: Color {
        switch self {
        case .documents: return Color(rgb: 0x2F6FDE)
        case .spreadsheets: return Color(rgb: 0x1E8E4F)
        case .presentations: return Color(rgb: 0xE0891B)
        case .pdf: return Color(rgb: 0xD23F31)
        case .labels: return Color(rgb: 0x0B8A8A)
        case .signatures: return Color(rgb: 0x7B4FC4)
        }
    }

    var fileKind: FileKind? {
        switch self {
        case .documents: return .document
        case .spreadsheets: return .spreadsheet
        case .presentations: return .presentation
        case .pdf: return .pdf
        case .labels, .signatures: return nil
        }
    }

    var tools: [Tool] {
        switch self {
        case .labels: return [.codes, .labelSheet, .marks]
        case .signatures: return [.signature, .stamp]
        default: return []
        }
    }

    /// The formats a section opens, for its hint line.
    var formats: String {
        switch self {
        case .documents: return "DOCX, ODT, RTF, Markdown, HTML, TXT"
        case .spreadsheets: return "XLSX, ODS, XLS, CSV"
        case .presentations: return "PPTX, ODP"
        default: return "PDF"
        }
    }

    static func of(_ kind: FileKind) -> AppSection {
        switch kind {
        case .document: return .documents
        case .spreadsheet: return .spreadsheets
        case .presentation: return .presentations
        case .pdf: return .pdf
        }
    }
}

/// The generators of the two tool sections.
enum Tool: String, CaseIterable, Identifiable, Hashable {
    case codes, labelSheet, marks, signature, stamp

    var id: String { rawValue }

    var title: String {
        switch self {
        case .codes: return tr("tab_codes")
        case .labelSheet: return tr("labels_title")
        case .marks: return tr("marks_title")
        case .signature: return tr("sig_title")
        case .stamp: return tr("stamp_title")
        }
    }

    var symbol: String {
        switch self {
        case .codes: return "qrcode"
        case .labelSheet: return "square.grid.3x3"
        case .marks: return "checkmark.seal"
        case .signature: return "signature"
        case .stamp: return "seal"
        }
    }
}

extension Color {
    init(rgb: UInt32) {
        self.init(red: Double((rgb >> 16) & 0xFF) / 255, green: Double((rgb >> 8) & 0xFF) / 255, blue: Double(rgb & 0xFF) / 255)
    }
}

/// The section's icon as in Settings: a white symbol on a rounded square in the section's colour.
struct SectionIcon: View {
    let symbol: String
    let color: Color
    @ScaledMetric(relativeTo: .body) private var size: CGFloat = 29

    var body: some View {
        Image(systemName: symbol)
            .font(.system(size: size * 0.55, weight: .medium))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(color, in: RoundedRectangle(cornerRadius: size * 0.24, style: .continuous))
            .accessibilityHidden(true)
    }
}
