import Foundation

enum PrinterJobSBPL {
    static func encode(_ p: PrinterJobPage, _ profile: PrinterJobProfile) throws -> Data {
        let bw = (p.widthDots+7)/8, bh = (p.heightDots+7)/8, row = bw
        guard bw <= 999, bh <= 999 else { throw PrinterJobError.limit }
        var s = "\u{1b}A\u{1b}IG\(profile.continuous ? "2" : "1")\u{1b}H0000\u{1b}V0000\u{1b}GH" + String(format: "%03d%03d", bw, bh)
        for by in 0..<bh { for bx in 0..<bw { for y in 0..<8 {
            let py = by*8+y; let b: UInt8 = py < p.heightDots ? p.data[py*row+bx] : 0
            s += String(format: "%02X", b)
        } } }
        return Data((s + "\u{1b}Q1\u{1b}Z").utf8)
    }
    static func decode(_ data: Data, _ profile: PrinterJobProfile) throws -> [PrinterJobPage] {
        var c = PrinterJobCursor(data), pages = [PrinterJobPage](), total = 0
        func fixedNumber(_ c: inout PrinterJobCursor) throws -> Int {
            let bytes = try c.raw(3); guard bytes.allSatisfy({ (48...57).contains($0) }), let n = Int(String(decoding: bytes, as: UTF8.self)), n > 0 else { throw PrinterJobError.invalid }; return n
        }
        func nibble(_ b: UInt8) throws -> UInt8 { if (48...57).contains(b) { return b-48 }; if (65...70).contains(b) { return b-55 }; throw PrinterJobError.invalid }
        while !c.end {
            guard pages.count < 500 else { throw PrinterJobError.limit }
            try c.take("\u{1b}A\u{1b}IG"); if c.starts("1") { try c.take("1") } else { try c.take("2") }
            try c.take("\u{1b}H0000\u{1b}V0000\u{1b}GH")
            let bw = try fixedNumber(&c), bh = try fixedNumber(&c), w = bw*8, h = bh*8, n = bw*h
            // The command records only whole 8x8 blocks; the original size is unknowable.
            guard w <= profile.maxWidthDots, n <= 8_000_000, total+n <= 8_000_000 else { throw PrinterJobError.limit }
            let hex = try c.raw(n*2); var raster = Data(repeating: 0, count: n); var i = 0
            for by in 0..<bh { for bx in 0..<bw { for y in 0..<8 {
                raster[(by*8+y)*bw+bx] = try nibble(hex[i])*16 + nibble(hex[i+1]); i += 2
            } } }
            try c.take("\u{1b}Q1\u{1b}Z")
            let page = PrinterJobPage(widthDots: w, heightDots: h, widthMM: Double(w)/profile.dotsPerMM, heightMM: Double(h)/profile.dotsPerMM, data: raster)
            try PrinterJobCodec.validate(page, profile); pages.append(page); total += n
        }
        return pages
    }
}
