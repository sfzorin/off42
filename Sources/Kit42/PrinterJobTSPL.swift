import Foundation

enum PrinterJobTSPL {
    static func mm(_ n: Double) -> String { var s = String(format: "%.2f", locale: Locale(identifier: "en_US_POSIX"), n); while s.last == "0" { s.removeLast() }; if s.last == "." { s.removeLast() }; return s }
    static func encode(_ p: PrinterJobPage, _ profile: PrinterJobProfile) throws -> Data {
        let row = (p.widthDots+7)/8
        var out = Data("SIZE \(mm(p.widthMM)),\(mm(p.heightMM)) mm\r\nGAP \(mm(profile.continuous ? 0 : profile.gapMM)) mm,0 mm\r\nDENSITY 8\r\nCLS\r\nBITMAP 0,0,\(row),\(p.heightDots),0,".utf8)
        out.append(p.data); out.append(Data("\r\nPRINT 1,1\r\n".utf8)); return out
    }
    static func decode(_ data: Data, _ profile: PrinterJobProfile) throws -> [PrinterJobPage] {
        var c = PrinterJobCursor(data), pages = [PrinterJobPage](), total = 0
        while !c.end {
            guard pages.count < 500 else { throw PrinterJobError.limit }
            try c.take("SIZE "); let wm = try c.decimal(); try c.take(","); let hm = try c.decimal(); try c.take(" mm\r\nGAP "); _ = try c.decimal(); try c.take(" mm,0 mm\r\nDENSITY 8\r\n"); if c.starts("DIRECTION 1\r\n") { try c.take("DIRECTION 1\r\n") }; try c.take("CLS\r\nBITMAP 0,0,")
            let row = try c.number(); try c.take(","); let h = try c.number(); try c.take(",0,")
            guard wm > 0, hm > 0, wm * profile.dotsPerMM <= Double(profile.maxWidthDots)+0.5, hm * profile.dotsPerMM <= 100000.5 else { throw PrinterJobError.limit }
            let w = Int((wm * profile.dotsPerMM).rounded()), expectedH = Int((hm * profile.dotsPerMM).rounded())
            guard w > 0, w <= profile.maxWidthDots, h > 0, h == expectedH, row == (w+7)/8, row*h <= 8_000_000, total + row*h <= 8_000_000 else { throw PrinterJobError.invalid }
            let pixels = try c.raw(row*h); try c.take("\r\nPRINT 1,1\r\n")
            let p = PrinterJobPage(widthDots: w, heightDots: h, widthMM: wm, heightMM: hm, data: pixels)
            try PrinterJobCodec.validate(p, profile); pages.append(p); total += pixels.count
        }
        return pages
    }
}
