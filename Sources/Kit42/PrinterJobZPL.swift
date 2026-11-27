import Foundation

enum PrinterJobZPL {
    static func encode(_ p: PrinterJobPage, _ profile: PrinterJobProfile) throws -> Data {
        let row = (p.widthDots + 7) / 8, rows = max(1, 99999 / row)
        var s = "^XA^MN\(profile.continuous ? "N" : "Y")^PW\(p.widthDots)^LL\(p.heightDots)^LH0,0"
        for y in stride(from: 0, to: p.heightDots, by: rows) {
            let n = min(rows, p.heightDots-y)*row
            s += "^FO0,\(y)^GFA,\(n),\(n),\(row),"
            for b in p.data[y*row..<y*row+n] { s += String(format: "%02X", b) }
            s += "^FS"
        }
        return Data((s + "^XZ").utf8)
    }
    static func decode(_ data: Data, _ profile: PrinterJobProfile) throws -> [PrinterJobPage] {
        var c = PrinterJobCursor(data), pages = [PrinterJobPage](), total = 0
        while !c.end {
            guard pages.count < 500 else { throw PrinterJobError.limit }
            try c.take("^XA^MN"); if c.starts("N") { try c.take("N") } else { try c.take("Y") }
            try c.take("^PW"); let w = try c.number(); try c.take("^LL"); let h = try c.number(); try c.take("^LH0,0")
            guard w > 0, w <= profile.maxWidthDots, (1...100000).contains(h) else { throw PrinterJobError.limit }
            let row = (w+7)/8; guard row*h <= 8_000_000, total + row*h <= 8_000_000 else { throw PrinterJobError.limit }
            var raster = Data(); var y = 0
            while c.starts("^FO") {
                try c.take("^FO0,"); guard try c.number() == y else { throw PrinterJobError.unsupported }
                try c.take("^GFA,"); let n = try c.number(); try c.take(","); guard try c.number() == n else { throw PrinterJobError.invalid }; try c.take(","); guard try c.number() == row else { throw PrinterJobError.invalid }; try c.take(",")
                guard n > 0, n <= 99999, n % row == 0, n <= row*(h-y) else { throw PrinterJobError.invalid }
                let hex = try c.raw(n*2); var decoded = Data(); decoded.reserveCapacity(n)
                func nibble(_ b: UInt8) throws -> UInt8 { if (48...57).contains(b) { return b-48 }; if (65...70).contains(b) { return b-55 }; throw PrinterJobError.invalid }
                for i in stride(from: 0, to: hex.count, by: 2) { decoded.append(try nibble(hex[i]) * 16 + nibble(hex[i+1])) }
                raster.append(decoded); y += n/row; try c.take("^FS")
            }
            guard y == h else { throw PrinterJobError.invalid }; if c.starts("^PQ") { try c.take("^PQ1") }; try c.take("^XZ")
            let p = PrinterJobPage(widthDots: w, heightDots: h, widthMM: Double(w)/profile.dotsPerMM, heightMM: Double(h)/profile.dotsPerMM, data: raster)
            try PrinterJobCodec.validate(p, profile); pages.append(p); total += raster.count
        }
        return pages
    }
}
