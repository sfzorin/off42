import AppKit
// textpng OUT.png SIZE WEIGHT(regular|medium|bold) R G B TEXT — one line of text, system font with automatic
// fallback (CJK, Thai, Arabic shaping and right-to-left order are handled by Core Text), transparent background.
let a = CommandLine.arguments
let out = a[1], size = CGFloat(Double(a[2])!), weight = a[3]
let color = NSColor(srgbRed: CGFloat(Double(a[4])!) / 255, green: CGFloat(Double(a[5])!) / 255, blue: CGFloat(Double(a[6])!) / 255, alpha: 1)
let text = a[7]
let w: NSFont.Weight = weight == "bold" ? .bold : weight == "medium" ? .medium : .regular
let font = NSFont.systemFont(ofSize: size, weight: w)
let para = NSMutableParagraphStyle(); para.baseWritingDirection = .natural
let str = NSAttributedString(string: text, attributes: [.font: font, .foregroundColor: color, .paragraphStyle: para, .kern: weight == "medium" && size > 40 ? -size * 0.012 : 0])
let bounds = str.boundingRect(with: NSSize(width: 10000, height: 1000), options: [.usesLineFragmentOrigin, .usesFontLeading])
let scale: CGFloat = 1, pad: CGFloat = size * 0.15
let W = Int(ceil(bounds.width + 2 * pad)), H = Int(ceil(bounds.height + 2 * pad))
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: W, pixelsHigh: H, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState(); NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
str.draw(with: NSRect(x: pad, y: pad, width: bounds.width + 1, height: bounds.height), options: [.usesLineFragmentOrigin, .usesFontLeading])
NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
print(W, H, Int(pad))
