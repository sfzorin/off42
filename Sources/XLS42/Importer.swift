import Foundation
import Kit42

/// Picks the reader by file contents: OLE2 → .xls, ZIP with OpenDocument mimetype → .ods,
/// other ZIP → .xlsx, anything else → CSV/TSV text.
enum SpreadsheetFile {
    enum Kind: Equatable { case xlsx, xls, ods, spreadsheetML, csv }

    static func kind(of data: Data, typeName: String = "") -> Kind {
        let b = [UInt8](data.prefix(8))
        if b.starts(with: [0xD0, 0xCF, 0x11, 0xE0]) { return .xls }
        if b.starts(with: [0x50, 0x4B, 0x03, 0x04]) {
            if let zip = try? ZipArchive(data: data), let m = zip.string("mimetype"), m.contains("opendocument.spreadsheet") { return .ods }
            return .xlsx
        }
        if SpreadsheetML.isWorkbook(data) { return .spreadsheetML }
        return .csv
    }

    static func read(_ data: Data, typeName: String = "") throws -> Workbook {
        let detected = kind(of: data)
        let type = typeName.lowercased()
        let expected: Kind?
        if type == "xml" || type == "public.xml" || type == "excel 2003 xml spreadsheet" { expected = .spreadsheetML }
        else if type == "xlsx" || type == "excel workbook" || type.contains("openxmlformats.spreadsheetml") || type.hasSuffix(".xlsx") { expected = .xlsx }
        else if type == "ods" || type.hasSuffix(".ods") || (type.contains("opendocument") && type.contains("spreadsheet")) { expected = .ods }
        else if type == "xls" || type.hasSuffix(".xls") || type == "excel 97-2003 workbook" { expected = .xls }
        else { expected = nil }
        // An explicit workbook type (notably Revert) must never reinterpret
        // corrupt binary data as CSV and replace the user's document with it.
        if let expected, detected != expected { throw ZipError.corrupt }
        switch detected {
        case .xls: return try XLSReader.read(data)
        case .ods: return try ODS.read(data)
        case .xlsx: return try XLSX.read(data)
        case .spreadsheetML: return try SpreadsheetML.read(data)
        case .csv: return XLSX.readCSV(data)
        }
    }
}
