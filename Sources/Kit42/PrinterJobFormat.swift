import Foundation

/// A flattened, black-is-one, MSB-first printer raster. Physical size is explicit.
public struct PrinterJobPage {
    public let widthDots: Int
    public let heightDots: Int
    public let widthMM: Double
    public let heightMM: Double
    public let data: Data
    public init(widthDots: Int, heightDots: Int, widthMM: Double, heightMM: Double, data: Data) {
        self.widthDots = widthDots; self.heightDots = heightDots
        self.widthMM = widthMM; self.heightMM = heightMM; self.data = data
    }
}
public enum PrinterJobFormat { case zpl, tspl, ezpl, sbpl, brotherQL }
public enum PrinterJobError: LocalizedError {
    case invalid, unsupported, limit
    public var errorDescription: String? {
        switch self {
        case .invalid: return "Invalid printer job, page geometry, or printer profile."
        case .unsupported: return "The file contains a printer command or setting that is not supported."
        case .limit: return "The page or job exceeds the selected printer's size or processing limits. Choose a matching profile or resize the page."
        }
    }
}
public enum PrinterJobBrotherMedia { case continuous62, dieCut62x29, dieCut62x100 }
public struct PrinterJobProfile {
    /// Printer dots per millimetre, e.g. 8 for the 203 dpi Lably profiles.
    public let dotsPerMM: Double
    public let maxWidthDots: Int
    public let continuous: Bool
    public let gapMM: Double
    public let brotherMedia: PrinterJobBrotherMedia?
    public init(dotsPerMM: Double, maxWidthDots: Int, continuous: Bool = false, gapMM: Double = 3, brotherMedia: PrinterJobBrotherMedia? = nil) {
        self.dotsPerMM = dotsPerMM; self.maxWidthDots = maxWidthDots
        self.continuous = continuous; self.gapMM = gapMM; self.brotherMedia = brotherMedia
    }
    func validate() throws {
        guard dotsPerMM.isFinite, dotsPerMM > 0, dotsPerMM <= 100, (1...10000).contains(maxWidthDots), gapMM.isFinite, (0...100).contains(gapMM) else { throw PrinterJobError.invalid }
    }
}
public enum PrinterJobCodec {
    public static func decode(_ data: Data, format: PrinterJobFormat, profile: PrinterJobProfile) throws -> [PrinterJobPage] {
        try profile.validate()
        guard !data.isEmpty, data.count <= 10_000_000 else { throw PrinterJobError.limit }
        switch format { case .zpl: return try PrinterJobZPL.decode(data, profile); case .tspl: return try PrinterJobTSPL.decode(data, profile); case .ezpl: return try PrinterJobEZPL.decode(data, profile); case .sbpl: return try PrinterJobSBPL.decode(data, profile); case .brotherQL: return try PrinterJobBrotherQL.decode(data, profile) }
    }
    public static func encode(_ pages: [PrinterJobPage], format: PrinterJobFormat, profile: PrinterJobProfile) throws -> Data {
        try profile.validate(); guard !pages.isEmpty, pages.count <= 500 else { throw PrinterJobError.limit }
        for page in pages { try validate(page, profile) }
        if case .brotherQL = format { return try PrinterJobBrotherQL.encode(pages, profile) }
        var out = Data()
        for page in pages {
            try validate(page, profile)
            switch format { case .zpl: out.append(try PrinterJobZPL.encode(page, profile)); case .tspl: out.append(try PrinterJobTSPL.encode(page, profile)); case .ezpl: out.append(try PrinterJobEZPL.encode(page, profile)); case .sbpl: out.append(try PrinterJobSBPL.encode(page, profile)); case .brotherQL: throw PrinterJobError.invalid }
            guard out.count <= 10_000_000 else { throw PrinterJobError.limit }
        }
        return out
    }
    static func validate(_ page: PrinterJobPage, _ profile: PrinterJobProfile) throws {
        guard page.widthDots > 0, page.widthDots <= profile.maxWidthDots, (1...100000).contains(page.heightDots), page.widthMM.isFinite, page.heightMM.isFinite, page.widthMM > 0, page.heightMM > 0 else { throw PrinterJobError.invalid }
        let row = (page.widthDots + 7) / 8
        guard row * page.heightDots <= 8_000_000, page.data.count == row * page.heightDots else { throw PrinterJobError.limit }
        guard abs(page.widthMM * profile.dotsPerMM - Double(page.widthDots)) <= 0.501, abs(page.heightMM * profile.dotsPerMM - Double(page.heightDots)) <= 0.501 else { throw PrinterJobError.invalid }
        if page.widthDots % 8 != 0 {
            let mask = UInt8((1 << (8 - page.widthDots % 8)) - 1)
            for y in 0..<page.heightDots where page.data[y * row + row - 1] & mask != 0 { throw PrinterJobError.invalid }
        }
    }
}
struct PrinterJobCursor {
    let bytes: [UInt8]; var index = 0
    init(_ data: Data) { bytes = Array(data) }
    var end: Bool { index == bytes.count }
    mutating func take(_ text: String) throws {
        let b = Array(text.utf8); guard index + b.count <= bytes.count, bytes[index..<index+b.count].elementsEqual(b) else { throw PrinterJobError.unsupported }; index += b.count
    }
    mutating func number() throws -> Int {
        let start = index; while index < bytes.count, bytes[index] >= 48, bytes[index] <= 57 { index += 1; if index-start > 9 { throw PrinterJobError.limit } }
        guard index > start, let n = Int(String(decoding: bytes[start..<index], as: UTF8.self)) else { throw PrinterJobError.invalid }; return n
    }
    mutating func decimal() throws -> Double {
        let start = index; while index < bytes.count, (48...57).contains(bytes[index]) || bytes[index] == 46 { index += 1; if index-start > 16 { throw PrinterJobError.limit } }
        guard index > start, let n = Double(String(decoding: bytes[start..<index], as: UTF8.self)), n.isFinite, n >= 0 else { throw PrinterJobError.invalid }; return n
    }
    mutating func raw(_ count: Int) throws -> Data { guard count >= 0, count <= bytes.count-index else { throw PrinterJobError.invalid }; defer { index += count }; return Data(bytes[index..<index+count]) }
    func starts(_ s: String) -> Bool { let b = Array(s.utf8); return index+b.count <= bytes.count && bytes[index..<index+b.count].elementsEqual(b) }
}
