import AppKit
import CoreGraphics
import PDFKit
import Kit42

enum PDFPrinterFiles {
    static func settings(_ name: String, _ stock: String) throws -> (PrinterJobFormat, PrinterJobProfile) {
        let dots = 8.0
        switch name.lowercased() {
        case "zpl": return (.zpl, .init(dotsPerMM: dots, maxWidthDots: 832))
        case "tspl": return (.tspl, .init(dotsPerMM: dots, maxWidthDots: 864))
        case "ezpl": return (.ezpl, .init(dotsPerMM: dots, maxWidthDots: 864))
        case "sbpl": return (.sbpl, .init(dotsPerMM: dots, maxWidthDots: 832))
        case "brother":
            let media: PrinterJobBrotherMedia
            switch stock { case "continuous": media = .continuous62; case "62x29": media = .dieCut62x29; case "62x100": media = .dieCut62x100; default: throw PrinterJobError.invalid }
            return (.brotherQL, .init(dotsPerMM: 300.0/25.4, maxWidthDots: 696, brotherMedia: media))
        default: throw PrinterJobError.unsupported
        }
    }
    static func toPDF(_ data: Data, format: PrinterJobFormat, profile: PrinterJobProfile) throws -> Data {
        let pages = try PrinterJobCodec.decode(data, format: format, profile: profile)
        let output = NSMutableData()
        guard let consumer = CGDataConsumer(data: output), let context = CGContext(consumer: consumer, mediaBox: nil, nil) else { throw PrinterJobError.invalid }
        for page in pages {
            let row = (page.widthDots+7)/8
            var gray = [UInt8](repeating: 255, count: page.widthDots*page.heightDots)
            for y in 0..<page.heightDots { for x in 0..<page.widthDots where page.data[y*row+x/8] & (0x80 >> (x%8)) != 0 { gray[y*page.widthDots+x] = 0 } }
            guard let provider = CGDataProvider(data: Data(gray) as CFData), let image = CGImage(width: page.widthDots, height: page.heightDots, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: page.widthDots, space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue), provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else { throw PrinterJobError.invalid }
            var box = CGRect(x: 0, y: 0, width: page.widthMM*72/25.4, height: page.heightMM*72/25.4)
            context.beginPage(mediaBox: &box); context.interpolationQuality = .none; context.draw(image, in: box); context.endPage()
        }
        context.closePDF(); return output as Data
    }
    static func fromPDF(_ pdf: PDFDocument, format: PrinterJobFormat, profile: PrinterJobProfile) throws -> Data {
        guard !pdf.isLocked, (1...500).contains(pdf.pageCount) else { throw PrinterJobError.limit }
        var pages = [PrinterJobPage](), total = 0
        for i in 0..<pdf.pageCount {
            guard let page = pdf.page(at: i), let ref = page.pageRef else { throw PrinterJobError.invalid }
            let bounds = page.bounds(for: .cropBox), rotation = ((page.rotation % 360)+360)%360
            guard [0,90,180,270].contains(rotation), bounds.width > 0, bounds.height > 0 else { throw PrinterJobError.invalid }
            let swap = rotation == 90 || rotation == 270
            let widthMM = (swap ? bounds.height : bounds.width)*25.4/72, heightMM = (swap ? bounds.width : bounds.height)*25.4/72
            guard widthMM.isFinite, heightMM.isFinite, widthMM*profile.dotsPerMM <= Double(profile.maxWidthDots)+0.0001, heightMM*profile.dotsPerMM <= 100000 else { throw PrinterJobError.limit }
            let w = Int((widthMM*profile.dotsPerMM).rounded()), h = Int((heightMM*profile.dotsPerMM).rounded())
            guard w > 0, h > 0, w*h <= 16_000_000, total+w*h <= 16_000_000 else { throw PrinterJobError.limit }
            // Let Core Graphics own the backing store for the entire context lifetime.
            guard let context = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w, space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue) else { throw PrinterJobError.invalid }
            context.setFillColor(gray: 1, alpha: 1); context.fill(CGRect(x: 0, y: 0, width: w, height: h))
            context.interpolationQuality = .none
            // Map the crop box to the exact output grid; CGPDFPage's convenience
            // transform does not enlarge small pages to fill the destination.
            context.scaleBy(x: CGFloat(w) / (swap ? bounds.height : bounds.width), y: CGFloat(h) / (swap ? bounds.width : bounds.height))
            switch rotation {
            case 90: context.translateBy(x: 0, y: bounds.width); context.rotate(by: -.pi / 2)
            case 180: context.translateBy(x: bounds.width, y: bounds.height); context.rotate(by: .pi)
            case 270: context.translateBy(x: bounds.height, y: 0); context.rotate(by: .pi / 2)
            default: break
            }
            context.translateBy(x: -bounds.minX, y: -bounds.minY)
            let originalRotation = page.rotation
            page.rotation = 0
            page.draw(with: .cropBox, to: context)
            page.rotation = originalRotation
            guard let data = context.data else { throw PrinterJobError.invalid }
            let pixels = data.bindMemory(to: UInt8.self, capacity: w*h)
            let row = (w+7)/8; var packed = Data(repeating: 0, count: row*h)
            // Bitmap memory and printer rasters both run from top to bottom.
            for y in 0..<h { for x in 0..<w where pixels[y*w+x] < 128 { packed[y*row+x/8] |= 0x80 >> (x%8) } }
            pages.append(.init(widthDots: w, heightDots: h, widthMM: widthMM, heightMM: heightMM, data: packed)); total += w*h
        }
        return try PrinterJobCodec.encode(pages, format: format, profile: profile)
    }
}
