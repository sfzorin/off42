import Foundation

enum PrinterJobEZPL {
    static func encode(_ p: PrinterJobPage, _ profile: PrinterJobProfile) throws -> Data {
        let count = String(p.data.count)
        var out = Data("^Q\(PrinterJobTSPL.mm(p.heightMM)),\(PrinterJobTSPL.mm(profile.continuous ? 0 : profile.gapMM)),0\r\n^W\(PrinterJobTSPL.mm(p.widthMM))\r\n^P1\r\n^R0\r\n~GG\(count.count)\(count)".utf8)
        out.append(p.data); out.append(Data("E\r\n".utf8)); return out
    }
    static func decode(_ data: Data, _ profile: PrinterJobProfile) throws -> [PrinterJobPage] {
        var c = PrinterJobCursor(data), pages = [PrinterJobPage](), total = 0
        while !c.end {
            guard pages.count < 500 else { throw PrinterJobError.limit }
            try c.take("^Q"); let hm = try c.decimal(); try c.take(","); _ = try c.decimal(); try c.take(",0\r\n^W"); let wm = try c.decimal(); try c.take("\r\n^P1\r\n^R0\r\n~GG")
            let digit = try c.raw(1)[0]; guard (49...57).contains(digit) else { throw PrinterJobError.invalid }
            let countText = try c.raw(Int(digit-48)); guard countText.allSatisfy({ (48...57).contains($0) }), let n = Int(String(decoding: countText, as: UTF8.self)) else { throw PrinterJobError.invalid }
            guard wm > 0, hm > 0, wm * profile.dotsPerMM <= Double(profile.maxWidthDots)+0.5, hm * profile.dotsPerMM <= 100000.5 else { throw PrinterJobError.limit }
            let w = Int((wm * profile.dotsPerMM).rounded()), h = Int((hm * profile.dotsPerMM).rounded())
            guard w > 0, w <= profile.maxWidthDots, h > 0, n == ((w+7)/8)*h, n <= 8_000_000, total+n <= 8_000_000 else { throw PrinterJobError.invalid }
            let pixels = try c.raw(n); try c.take("E\r\n")
            let page = PrinterJobPage(widthDots: w, heightDots: h, widthMM: wm, heightMM: hm, data: pixels)
            try PrinterJobCodec.validate(page, profile); pages.append(page); total += n
        }
        return pages
    }
}
