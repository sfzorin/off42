import Foundation

enum PrinterJobBrotherQL {
    static func media(_ p: PrinterJobProfile) throws -> (UInt8, UInt8) {
        guard abs(p.dotsPerMM - 300.0/25.4) < 0.000001, p.maxWidthDots == 696, let media = p.brotherMedia else { throw PrinterJobError.invalid }
        switch media { case .continuous62: return (0x0a, 0); case .dieCut62x29: return (0x0b, 29); case .dieCut62x100: return (0x0b, 100) }
    }
    static func encode(_ pages: [PrinterJobPage], _ profile: PrinterJobProfile) throws -> Data {
        let (type, length) = try media(profile); var out = Data([0x1b,0x40])
        for (index,p) in pages.enumerated() {
            let maxHeight = length == 29 ? 22.9 : length == 100 ? 93.9 : 1000.0
            guard p.heightMM <= maxHeight, p.widthDots <= 696 else { throw PrinterJobError.limit }
            let count = p.heightDots * 90
            guard count <= 8_000_000, out.count + count + p.heightDots * 3 <= 10_000_000 else { throw PrinterJobError.limit }
            out.append(contentsOf: [0x1b,0x69,0x61,0x01,0x1b,0x69,0x21,0x00,0x1b,0x69,0x7a,0x8e,type,62,length,UInt8(count & 255),UInt8((count>>8)&255),UInt8((count>>16)&255),UInt8((count>>24)&255),index == 0 ? 0 : 1,0,0x1b,0x69,0x4d,0,0x1b,0x69,0x64,type == 0x0a ? 0x23 : 0,0,0x4d,0])
            let row = (p.widthDots+7)/8
            for y in 0..<p.heightDots {
                var transfer = [UInt8](repeating: 0, count: 90)
                for x in 0..<p.widthDots where p.data[y*row+x/8] & (0x80 >> (x%8)) != 0 { let target = x+12; transfer[target/8] |= 0x80 >> (target%8) }
                out.append(contentsOf: [0x67,0,0x5a]); out.append(contentsOf: transfer)
            }
            out.append(index == pages.count-1 ? 0x1a : 0x0c)
        }
        return out
    }
    static func decode(_ data: Data, _ profile: PrinterJobProfile) throws -> [PrinterJobPage] {
        let (type,length) = try media(profile); var c = PrinterJobCursor(data), pages = [PrinterJobPage](), total = 0
        try c.take("\u{1b}@")
        while true {
            guard pages.count < 500 else { throw PrinterJobError.limit }
            let header = try c.raw(21)
            guard Array(header.prefix(15)) == [0x1b,0x69,0x61,1,0x1b,0x69,0x21,0,0x1b,0x69,0x7a,0x8e,type,62,length], header[19] == (pages.isEmpty ? 0 : 1), header[20] == 0 else { throw PrinterJobError.unsupported }
            let n = Int(header[15]) | Int(header[16])<<8 | Int(header[17])<<16 | Int(header[18])<<24
            guard n > 0, n % 90 == 0, n <= 8_000_000, total+n <= 8_000_000 else { throw PrinterJobError.limit }
            let h = n/90, maxHeight = length == 29 ? 22.9 : length == 100 ? 93.9 : 1000.0
            guard Double(h)/profile.dotsPerMM <= maxHeight else { throw PrinterJobError.limit }
            let config = try c.raw(11); guard Array(config) == [0x1b,0x69,0x4d,0,0x1b,0x69,0x64,type == 0x0a ? 0x23 : 0,0,0x4d,0] else { throw PrinterJobError.unsupported }
            var raster = Data(repeating: 0, count: 87*h)
            for y in 0..<h {
                guard try c.raw(3) == Data([0x67,0,0x5a]) else { throw PrinterJobError.unsupported }
                let line = try c.raw(90)
                guard line[0] == 0, line[1] & 0xf0 == 0, line[88] & 0x0f == 0, line[89] == 0 else { throw PrinterJobError.unsupported }
                for x in 0..<696 where line[(x+12)/8] & (0x80 >> ((x+12)%8)) != 0 { raster[y*87+x/8] |= 0x80 >> (x%8) }
            }
            let end = try c.raw(1)[0]; guard end == 0x0c || end == 0x1a else { throw PrinterJobError.unsupported }
            let page = PrinterJobPage(widthDots: 696, heightDots: h, widthMM: 696/profile.dotsPerMM, heightMM: Double(h)/profile.dotsPerMM, data: raster)
            try PrinterJobCodec.validate(page, profile); pages.append(page); total += n
            if end == 0x1a { guard c.end else { throw PrinterJobError.unsupported }; break }
        }
        return pages
    }
}
