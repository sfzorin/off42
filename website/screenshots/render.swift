import AppKit
let a = CommandLine.arguments
let doc = CGPDFDocument(URL(fileURLWithPath: a[1]) as CFURL)!, page = doc.page(at: 1)!
let s: CGFloat = 4, box = page.getBoxRect(.mediaBox)
let ctx = CGContext(data: nil, width: Int(box.width * s), height: Int(box.height * s), bitsPerComponent: 8, bytesPerRow: 0,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
ctx.setFillColor(.white); ctx.fill(CGRect(x: 0, y: 0, width: box.width * s, height: box.height * s))
ctx.scaleBy(x: s, y: s); ctx.drawPDFPage(page)
try! NSBitmapImageRep(cgImage: ctx.makeImage()!).representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: a[2]))
