import AppKit
import ImageIO

/// Maps the modern Keynote IWA object graph. Schema: KNArchives, TSDArchives,
/// TSWPArchives and TSPMessages 14.4 from psobot/keynote-parser; object IDs
/// and placement semantics cross-checked against libetonyek's IWAParser.
struct KeynoteImport {
    struct Result { var deck: Deck; var warnings: [String] }
    enum Failure: LocalizedError {
        case invalid(String), unsupported(String)
        var errorDescription: String? {
            switch self { case .invalid(let s): return "Invalid Keynote document: \(s)"; case .unsupported(let s): return "Unsupported Keynote content: \(s)" }
        }
    }
    private let archive: KeynoteArchive
    private var warnings: Set<String> = []
    private var count = 0
    private var active: Set<UInt64> = []
    init(_ archive: KeynoteArchive) { self.archive = archive }

    mutating func read() throws -> Result {
        guard let doc = archive.objects[1], doc.type == 1 else { throw Failure.invalid("missing document root") }
        let root = try doc.message()
        let show = try object(ref(root, 2), type: 2).message()
        guard let size = try one(show, 4), let width = number(size, 1), let height = number(size, 2), width > 0, height > 0, width <= 100_000, height <= 100_000 else { throw Failure.invalid("invalid slide size") }
        guard let tree = try one(show, 3) else { throw Failure.invalid("missing slide tree") }
        var deck = Deck.makeDefault(); deck.size = CGSize(width: width, height: height); deck.slides = []
        let nodes = try refs(tree, 2)
        guard !nodes.isEmpty, nodes.count <= 10_000 else { throw Failure.invalid("missing or excessive slides") }
        for node in nodes { try readNode(node, deck: &deck, depth: 0) }
        warnings.insert("Keynote theme, master objects, inherited styles, notes, transitions and animations are not preserved.")
        return Result(deck: deck, warnings: warnings.sorted())
    }
    private mutating func readNode(_ id: UInt64, deck: inout Deck, depth: Int) throws {
        guard depth <= 64, active.insert(id).inserted, deck.slides.count < 10_000 else { throw Failure.invalid("slide tree cycle or limit") }
        defer { active.remove(id) }
        let node = try object(id, type: 4).message()
        if let slideID = try optionalRef(node, 2) {
            let message = try object(slideID, type: 5).message()
            var slide = Slide(layout: 0); slide.hidden = node.integer(4) == 1
            let order = try refs(message, 42), owned = try refs(message, 7)
            let ids = order.isEmpty ? owned : order
            guard ids.count <= 10_000, Set(ids).count == ids.count, Set(ids) == Set(owned) else { throw Failure.invalid("inconsistent drawable order") }
            for id in ids { slide.objects.append(try drawable(id, depth: 0, deck: &deck)) }
            if !message.values(2).isEmpty || !message.values(43).isEmpty { warnings.insert("Keynote object animations are not preserved.") }
            deck.slides.append(slide)
        }
        for child in try refs(node, 1) { try readNode(child, deck: &deck, depth: depth + 1) }
    }
    private mutating func drawable(_ id: UInt64, depth: Int, deck: inout Deck) throws -> SlideObject {
        count += 1
        guard count <= 100_000, depth <= 64, active.insert(id).inserted else { throw Failure.invalid("drawable cycle or limit") }
        defer { active.remove(id) }
        guard let obj = archive.objects[id] else { throw Failure.invalid("missing drawable \(id)") }
        let message = try obj.message()
        if obj.type == 3008 {
            guard let placement = try one(message, 1) else { throw Failure.invalid("missing group placement") }
            var result = SlideObject(id: count, kind: .group, frame: try frame(placement))
            for child in try refs(message, 2) { result.children.append(try drawable(child, depth: depth + 1, deck: &deck)) }
            return result
        }
        if obj.type == 3005 {
            guard let placement = try one(message, 1), let dataID = try optionalRef(message, 15) ?? optionalRef(message, 11), let data = archive.dataFile(id: dataID), let source = CGImageSourceCreateWithData(data as CFData, nil), let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any], let width = (properties[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue, let height = (properties[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue, width > 0, height > 0, width <= 20_000, height <= 20_000, width <= 100_000_000 / height else { throw Failure.invalid("missing or invalid image data") }
            guard message.values(5).isEmpty, message.values(10).isEmpty else { throw Failure.unsupported("image mask on object \(id)") }
            let ext: String
            if data.starts(with: [0x89, 0x50, 0x4e, 0x47]) { ext = "png" }
            else if data.starts(with: [0xff, 0xd8]) { ext = "jpg" }
            else if data.starts(with: [0x49, 0x49, 0x2a, 0]) || data.starts(with: [0x4d, 0x4d, 0, 0x2a]) { ext = "tiff" }
            else { throw Failure.unsupported("image encoding on object \(id)") }
            var result = SlideObject(id: count, kind: .picture, frame: try frame(placement))
            result.media = deck.addMedia(data, ext: ext)
            if let geometry = try one(placement, 1) { result.rotation = CGFloat(number(geometry, 4) ?? 0); result.flipH = (geometry.integer(3) ?? 0) & 4 != 0 }
            warnings.insert("Keynote image effects and media styles are not preserved.")
            return result
        }
        guard obj.type == 2011 || obj.type == 7 else { throw Failure.unsupported("drawable type \(obj.type) (object \(id))") }
        let shapeInfo: KeynoteMessage
        if obj.type == 7 { guard let m = try one(message, 1) else { throw Failure.invalid("missing placeholder") }; shapeInfo = m } else { shapeInfo = message }
        guard let shape = try one(shapeInfo, 1), let placement = try one(shape, 1) else { throw Failure.invalid("missing shape geometry") }
        let textBox = obj.type == 7 || shapeInfo.integer(6) == 1
        var result = SlideObject(id: count, kind: textBox ? .textBox : .shape, frame: try frame(placement))
        if !textBox { result.fill = .solid(PColor(rgb: 0x808080)) }
        if let styleID = try optionalRef(shape, 2), let styleObject = archive.objects[styleID], styleObject.type == 2025 {
            let style = try styleObject.message()
            if let wrapper = try one(style, 1), let properties = try one(wrapper, 11), let fill = try one(properties, 1) {
                if let c = try one(fill, 1), let rgb = color(c) { result.fill = .solid(rgb) }
                else if !fill.fields.isEmpty { warnings.insert("Some Keynote fills are not preserved.") }
            }
        }
        if let geometry = try one(placement, 1) {
            result.rotation = CGFloat(number(geometry, 4) ?? 0)
            result.flipH = (geometry.integer(3) ?? 0) & 4 != 0
        }
        if let path = try one(shape, 3), !textBox {
            if let scalar = try one(path, 4), scalar.integer(1) == 0 {
                result.geometry = "roundRect"
            } else if let point = try one(path, 3), let kind = point.integer(1), kind == 0 || kind == 1 || kind == 10 {
                result.geometry = kind == 0 ? "leftArrow" : (kind == 1 ? "rightArrow" : "leftRightArrow")
            } else if try ellipse(path) {
                result.geometry = "ellipse"
            } else if !path.values(3).isEmpty || !path.values(4).isEmpty || !path.values(5).isEmpty || !path.values(6).isEmpty || !path.values(7).isEmpty || !path.values(8).isEmpty {
                throw Failure.unsupported("shape path on object \(id)")
            }
            result.flipH = result.flipH != (path.integer(1) == 1)
            result.flipV = path.integer(2) == 1
        }
        if let storageID = try optionalRef(shapeInfo, 4) ?? optionalRef(shapeInfo, 2) {
            let storage = try object(storageID, type: 2001).message()
            guard storage.values(9).isEmpty, storage.values(11).isEmpty else { throw Failure.unsupported("text attachments or fields on object \(id)") }
            let chunks = try storage.values(3).map { value -> String in
                guard case let .bytes(data) = value, let string = String(data: data, encoding: .utf8) else { throw Failure.invalid("invalid text") }
                return string
            }
            let style = try textStyle(storage)
            result.text = TextBody(paragraphs: chunks.joined().components(separatedBy: "\n").map { Paragraph($0, props: style, run: style.run) })
        }
        if obj.type == 7 { let kind = message.integer(2); result.placeholder = Placeholder(type: kind == 2 ? "title" : "body", idx: nil) }
        warnings.insert("Keynote mixed text styles, effects and some shape formatting may not be preserved.")
        return result
    }
    private func color(_ value: KeynoteMessage) -> PColor? {
        let model = value.integer(1)
        let components: [Double]
        if model == 1, let r = number(value, 3), let g = number(value, 4), let b = number(value, 5) { components = [r,g,b] }
        else if model == 3, let w = number(value, 11) { components = [w,w,w] }
        else { return nil }
        let alpha = number(value, 6) ?? 1
        guard (components + [alpha]).allSatisfy({ $0.isFinite && $0 >= 0 && $0 <= 1 }) else { return nil }
        let rgb = components.reduce(UInt32(0)) { ($0 << 8) | UInt32(($1 * 255).rounded()) }
        return PColor(rgb: rgb, alpha: CGFloat(alpha))
    }
    private func textStyle(_ storage: KeynoteMessage) throws -> LevelStyle {
        guard let table = try one(storage, 5), let first = try table.messages(1).first, first.integer(1) == 0, let id = try optionalRef(first, 2) else { return LevelStyle() }
        var chain: [KeynoteMessage] = [], visited: Set<UInt64> = [], current: UInt64? = id
        while let id = current {
            guard chain.count < 64, visited.insert(id).inserted, let object = archive.objects[id], object.type == 2022 else { throw Failure.invalid("invalid paragraph style chain") }
            let message = try object.message(); chain.append(message)
            current = try one(message, 1).flatMap { try optionalRef($0, 3) }
        }
        var result = LevelStyle()
        for message in chain.reversed() {
            if let chars = try one(message, 11) {
                if let size = number(chars, 3), size.isFinite, size > 0, size <= 1000 { result.run.size = CGFloat(size) }
                if let font = chars.bytes(5).flatMap({ String(data: $0, encoding: .utf8) }) { result.run.font = font }
                if let bold = chars.integer(1) { result.run.bold = bold != 0 }
                if let italic = chars.integer(2) { result.run.italic = italic != 0 }
                if let c = try one(chars, 7), let value = color(c) { result.run.color = value }
            }
            if let para = try one(message, 12), let alignment = para.integer(1) {
                switch alignment { case 0: result.align = .left; case 1: result.align = .right; case 2: result.align = .center; case 3: result.align = .justify; default: break }
            }
        }
        return result
    }
    // Keynote serializes ellipses as four cubic segments. Require the canonical
    // cardinal endpoints and control points, rather than treating arbitrary curves as ellipses.
    private func ellipse(_ path: KeynoteMessage) throws -> Bool {
        guard let bezier = try one(path, 5), let size = try one(bezier, 2), let width = number(size, 1), let height = number(size, 2), width > 0, height > 0, let body = try one(bezier, 3) else { return false }
        let elements = try body.messages(1)
        guard elements.count == 6, elements[0].integer(1) == 1, elements[5].integer(1) == 5 else { return false }
        let targets: [(Double, Double)] = [(width/2,0),(width,height/2),(width/2,height),(0,height/2),(width/2,0)]
        for i in 0..<5 {
            if i > 0 && elements[i].integer(1) != 4 { return false }
            let points = try elements[i].messages(2)
            guard points.count == (i == 0 ? 1 : 3), let p = points.last, let x = number(p, 1), let y = number(p, 2), abs(x-targets[i].0) < width*0.01, abs(y-targets[i].1) < height*0.01 else { return false }
            if i > 0 {
                // Canonical kappa control points rule out other curves with the same endpoints.
                let k = 0.55228475
                let allControls: [[(Double, Double)]] = [[(width/2+k*width/2,0),(width,height/2-k*height/2)],[(width,height/2+k*height/2),(width/2+k*width/2,height)],[(width/2-k*width/2,height),(0,height/2+k*height/2)],[(0,height/2-k*height/2),(width/2-k*width/2,0)]]
                let controls = allControls[i-1]
                for j in 0..<2 { guard let x = number(points[j],1), let y = number(points[j],2), abs(x-controls[j].0) < width*0.01, abs(y-controls[j].1) < height*0.01 else { return false } }
            }
        }
        return true
    }
    private func frame(_ drawable: KeynoteMessage) throws -> CGRect {
        guard let g = try one(drawable, 1), let p = try one(g, 1), let s = try one(g, 2), let x = number(p, 1), let y = number(p, 2), let w = number(s, 1), let h = number(s, 2), w >= 0, h >= 0, [x,y,w,h].allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 }) else { throw Failure.invalid("invalid object geometry") }
        return CGRect(x: x, y: y, width: w, height: h)
    }
    private func number(_ m: KeynoteMessage, _ n: Int) -> Double? {
        guard case let .fixed32(bits)? = m.values(n).first else { return nil }
        return Double(Float(bitPattern: bits))
    }
    private func one(_ m: KeynoteMessage, _ n: Int) throws -> KeynoteMessage? { try m.messages(n).first }
    private func ref(_ m: KeynoteMessage, _ n: Int) throws -> UInt64 { guard let id = try optionalRef(m, n) else { throw Failure.invalid("missing reference") }; return id }
    private func optionalRef(_ m: KeynoteMessage, _ n: Int) throws -> UInt64? { try one(m, n)?.integer(1) }
    private func refs(_ m: KeynoteMessage, _ n: Int) throws -> [UInt64] { try m.messages(n).map { guard let id = $0.integer(1) else { throw Failure.invalid("invalid reference") }; return id } }
    private func object(_ id: UInt64, type: UInt64) throws -> KeynoteObject { guard let obj = archive.objects[id], obj.type == type else { throw Failure.invalid("missing object \(id), expected type \(type)") }; return obj }
}
