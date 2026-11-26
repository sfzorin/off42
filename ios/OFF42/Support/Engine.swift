import Foundation
import UniformTypeIdentifiers

/// The off42 engine (rust/crates/off42-mobile through its UniFFI Swift bindings). Every call is synchronous and some
/// take seconds on large files, so the UI awaits them through `Engine.run`, which runs them off the main thread.
enum Engine {
    /// Limits of the spreadsheet grid, as in the web and Android apps.
    static let maxRows = 1000
    static let maxCols = 100

    static func run<T>(_ work: @escaping () throws -> T) async throws -> T {
        try await Task.detached(priority: .userInitiated) { try work() }.value
    }

    /// A random non-zero seed for signature designs and impressions (the engine has no entropy of its own).
    /// SystemRandomNumberGenerator is the system's cryptographically secure generator.
    static func randomSeed() -> UInt64 {
        var generator = SystemRandomNumberGenerator()
        return UInt64.random(in: 1...UInt64.max, using: &generator)
    }
}

extension Error {
    /// The engine's message for its typed errors, the system's description otherwise.
    var engineMessage: String {
        guard let error = self as? Off42Error else { return localizedDescription }
        switch error {
        case let .UnsupportedFormat(message), let .PackagePartsWouldBeLost(message),
             let .InvalidInput(message), let .Failed(message):
            return message
        }
    }
}

/// What an opened file shows.
enum FileContent {
    case document(html: String, info: DocumentInfo)
    case spreadsheet(Workbook)
    case presentation([Slide])
    case pdf(PdfInfo)
}

/// A file opened in this session. Bytes stay in memory; nothing is written unless the user saves or shares.
struct OpenedFile: Identifiable {
    let id: Int
    let name: String
    let ext: String
    let kind: FileKind
    let bytes: Data
    let content: FileContent?
    let error: String?

    var stem: String {
        guard let dot = name.lastIndex(of: "."), dot != name.startIndex else { return name }
        return String(name[..<dot])
    }

    /// Reads a file with the engine. Call through `Engine.run`. Engine errors become the file's error text.
    static func load(id: Int, name: String, ext: String, kind: FileKind, bytes: Data) -> OpenedFile {
        do {
            let content: FileContent
            switch kind {
            case .document:
                content = .document(html: try documentHtml(bytes: bytes, from: ext), info: try documentInfo(bytes: bytes, from: ext))
            case .spreadsheet:
                content = .spreadsheet(try spreadsheetOpen(bytes: bytes, kind: ext, maxRows: UInt32(Engine.maxRows),
                                                           maxCols: UInt32(Engine.maxCols)))
            case .presentation:
                content = .presentation(try presentationOutline(bytes: bytes, from: ext))
            case .pdf:
                content = .pdf(try pdfInfo(bytes: bytes))
            }
            return OpenedFile(id: id, name: name, ext: ext, kind: kind, bytes: bytes, content: content, error: nil)
        } catch {
            return OpenedFile(id: id, name: name, ext: ext, kind: kind, bytes: bytes, content: nil, error: error.engineMessage)
        }
    }
}

enum Files {
    /// The engine's extension for a file name, falling back to the file's type for names without one.
    static func engineExtension(name: String, type: UTType?) -> String {
        let fromName = normalizedExtension(name: name)
        if name.contains("."), fileKind(name: fromName) != nil { return fromName }
        guard let type else { return fromName }
        // Exact type first, then the most specific supertype (CSV, Markdown and HTML are plain text too).
        let known = ["docx", "odt", "rtf", "md", "html", "csv", "xlsx", "ods", "xls", "pptx", "odp", "pdf", "txt"]
        let candidates = known.compactMap { ext in UTType(filenameExtension: ext).map { (ext, $0) } }
        if let exact = candidates.first(where: { $0.1 == type }) { return exact.0 }
        return candidates.first { type.conforms(to: $0.1) }?.0 ?? fromName
    }

    /// The content type of an engine extension, for the save dialog and sharing.
    static func contentType(ext: String) -> UTType {
        switch ext {
        case "md": return UTType(filenameExtension: "md") ?? .plainText
        case "svg": return .svg
        case "png": return .png
        case "pdf": return .pdf
        default: return UTType(filenameExtension: ext) ?? .data
        }
    }

    /// Types offered in the system document picker for a kind of file.
    static func pickerTypes(_ kind: FileKind) -> [UTType] {
        let extensions: [String]
        switch kind {
        case .document: extensions = ["docx", "odt", "rtf", "md", "markdown", "html", "htm", "txt"]
        case .spreadsheet: extensions = ["xlsx", "ods", "xls", "csv"]
        case .presentation: extensions = ["pptx", "odp"]
        case .pdf: extensions = ["pdf"]
        }
        var types: [UTType] = []
        for ext in extensions {
            if let type = UTType(filenameExtension: ext), !types.contains(type) { types.append(type) }
        }
        if kind == .document { types += [.plainText, .html, .rtf].filter { !types.contains($0) } }
        return types
    }

    /// A file name without characters that file systems refuse.
    static func safeName(_ name: String) -> String {
        let bad = CharacterSet(charactersIn: "/\\:*?\"<>|")
        let cleaned = name.unicodeScalars.map { bad.contains($0) ? "_" : String($0) }.joined()
        return cleaned.isEmpty ? "file" : cleaned
    }
}

enum Sheets {
    /// Spreadsheet column name: 0 → A, 25 → Z, 26 → AA.
    static func columnName(_ index: Int) -> String {
        var name = ""
        var n = index + 1
        while n > 0 {
            let scalar = UnicodeScalar(UInt8(65 + (n - 1) % 26))
            name.insert(Character(scalar), at: name.startIndex)
            n = (n - 1) / 26
        }
        return name
    }

    /// Text that reads as a number is right-aligned in the grid, as in the web app.
    static func looksNumeric(_ text: String) -> Bool {
        !text.isEmpty && text.range(of: #"^-?[\d.,\s%]+$"#, options: .regularExpression) != nil
    }

    /// A grid of a sheet's displayed text, sized to its used range within the limits.
    static func grid(_ sheet: Sheet) -> [[String]] {
        let maxRow = sheet.cells.map { Int($0.row) }.max() ?? -1
        let maxCol = sheet.cells.map { Int($0.col) }.max() ?? -1
        let rows = min(max(Int(sheet.rows), maxRow + 1, 1), Engine.maxRows)
        let cols = min(max(Int(sheet.cols), maxCol + 1, 1), Engine.maxCols)
        var grid = Array(repeating: Array(repeating: "", count: cols), count: rows)
        for cell in sheet.cells where Int(cell.row) < rows && Int(cell.col) < cols {
            grid[Int(cell.row)][Int(cell.col)] = cell.text
        }
        return grid
    }
}

enum DocumentPage {
    /// The engine's standalone HTML, made safe and shown as a white page: a Content-Security-Policy that allows no
    /// network, script or frame (only inline styles and data: images and fonts), a viewport and page styling.
    static func html(_ html: String) -> String {
        let head = """
        <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; font-src data:; \
        style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">\
        <meta name="viewport" content="width=device-width, initial-scale=1">\
        <style>html{background:transparent;padding:16px 12px 24px}body{background:#fff;color:#1d1d1f;\
        max-width:760px;min-height:calc(100vh - 40px);box-sizing:border-box;margin:0 auto;padding:36px 32px;\
        box-shadow:0 1px 4px rgba(0,0,0,.18);font:15px/1.5 -apple-system,system-ui,sans-serif;\
        overflow-wrap:break-word}img{max-width:100%;height:auto}\
        table{border-collapse:collapse;max-width:100%}td,th{border:1px solid #c7c7cc;padding:4px 6px}</style>
        """
        if let range = html.range(of: "<head>", options: .caseInsensitive) {
            var result = html
            result.insert(contentsOf: head, at: range.upperBound)
            return result
        }
        return "<!doctype html><html><head><meta charset=\"utf-8\">\(head)</head><body>\(html)</body></html>"
    }
}
