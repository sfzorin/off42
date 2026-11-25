import Foundation

/// Post-processing of saved files with form fields, appended as an incremental update. PDFKit writes
/// widget annotations but (1) not the catalog's /AcroForm, without which Acrobat ignores the fields,
/// (2) check boxes with one appearance instead of /Yes and /Off states, (3) the embedded widget font
/// stream without /Length. Files without widgets are returned unchanged.
enum AcroFormFixer {
    static func ensureFields(_ data: Data) -> Data {
        guard let file = try? PDFFileStructure(data), case .ref(let rootNum, _)? = file.trailer["Root"],
              let size = file.trailer["Size"]?.int, var catalog = try? file.object(rootNum),
              case .ref(let pagesRoot, _)? = catalog["Pages"] else { return data }
        // Widgets on pages and their top-level fields (a kid widget is reached through its /Parent).
        var widgets: [Int] = [], fields: [Int] = []
        var seen = Set<Int>()
        func top(_ num: Int, depth: Int = 0) -> Int? {
            guard depth < 32, let obj = try? file.object(num) else { return nil }
            if case .ref(let parent, _)? = obj["Parent"] { return top(parent, depth: depth + 1) }
            return obj["FT"] != nil ? num : nil
        }
        func walk(_ num: Int, depth: Int) {
            guard depth < 64, let node = try? file.object(num) else { return }
            if node["Type"]?.nameValue == "Page" {
                for case .ref(let a, _) in (try? file.resolve(node["Annots"]))?.items ?? [] {
                    guard let annot = try? file.object(a), annot["Subtype"]?.nameValue == "Widget" else { continue }
                    widgets.append(a)
                    if let f = top(a), !seen.contains(f) { seen.insert(f); fields.append(f) }
                }
                return
            }
            for case .ref(let kid, _) in (try? file.resolve(node["Kids"]))?.items ?? [] { walk(kid, depth: depth + 1) }
        }
        walk(pagesRoot, depth: 0)
        guard !fields.isEmpty else { return data }

        var updates: [Int: [UInt8]] = [:]
        var nextNum = size

        // (1) /AcroForm /Fields
        var formNum: Int?
        var form: PDFObject = .dict([])
        if case .ref(let n, _)? = catalog["AcroForm"], let f = try? file.object(n) { formNum = n; form = f }
        else if case .dict? = catalog["AcroForm"] { form = catalog["AcroForm"]! }
        let listed = (try? file.resolve(form["Fields"]))?.items ?? []
        let listedNums = Set(listed.compactMap { if case .ref(let n, _) = $0 { return n } else { return nil } })
        let missing = fields.filter { !listedNums.contains($0) }
        var formChanged = false
        if !missing.isEmpty {
            form = form.setting("Fields", .array(listed + missing.map { .ref($0, file.gen($0)) }))
            if form["DA"] == nil { form = form.setting("DA", .raw("(/Helv 0 Tf 0 g)")) }
            formChanged = true
        }
        // Fonts named by the default appearances written below.
        var dr = ((try? file.resolve(form["DR"])) ?? nil) ?? .dict([])
        var fonts = ((try? file.resolve(dr["Font"])) ?? nil) ?? .dict([])
        for (key, base) in [("Helv", "/Helvetica /Encoding /WinAnsiEncoding"), ("ZaDb", "/ZapfDingbats")] where fonts[key] == nil {
            fonts = fonts.setting(key, .raw("<< /Type /Font /Subtype /Type1 /BaseFont \(base) >>"))
            formChanged = true
        }
        if formChanged {
            dr = dr.setting("Font", fonts)
            form = form.setting("DR", dr)
            if let formNum { updates[formNum] = Array(form.serialized.utf8) } else {
                catalog = catalog.setting("AcroForm", form)
                updates[rootNum] = Array(catalog.serialized.utf8)
            }
        }

        for w in widgets {
            guard var widget = try? file.object(w) else { continue }
            var changed = false
            let field = widget["FT"] != nil ? widget : ((try? file.resolve(widget["Parent"])) ?? nil) ?? widget
            let isButton = field["FT"]?.nameValue == "Btn"
            // (2a) /DA written as a name ("/Helvetica#2012#20Tf…") → a proper string
            if case .name(let n)? = widget["DA"] {
                let parts = Self.unescapeName(Self.unescapeName(n)).split(separator: " ")
                let size = parts.firstIndex(of: "Tf").flatMap { $0 > 0 ? Double(parts[$0 - 1]) : nil } ?? 0
                widget = widget.setting("DA", .raw("(/\(isButton ? "ZaDb" : "Helv") \(String(format: "%g", size)) Tf 0 g)"))
                changed = true
            }
            defer { if changed { updates[w] = Array(widget.serialized.utf8) } }
            // (2b) check boxes: /AP /N << /Yes … /Off … >>, /AS and /V as names
            let flags = field["Ff"]?.int ?? 0
            guard isButton, flags & (1 << 15 | 1 << 16) == 0 else { continue }
            let value = widget["V"] ?? field["V"]
            let on: Bool
            switch value {
            case .name(let v)?: on = v != "Off"
            case .raw(let v)?: on = !["(Off)", "()"].contains(v)
            // PDFKit removes /V when an imported checked checkbox is cleared, but leaves its old
            // /AS name behind. Absence of a field value means Off, not the stale appearance state.
            default: on = false
            }
            if widget["FT"] != nil, value?.nameValue == nil, value != nil { widget = widget.setting("V", .name(on ? "Yes" : "Off")); changed = true }
            if widget["MK"]?["CA"] == nil { widget = widget.setting("MK", (widget["MK"] ?? .dict([])).setting("CA", .raw("(4)"))); changed = true }
            guard let rect = widget["Rect"]?.items?.compactMap({ $0.number }), rect.count == 4,
                  let ap = (try? file.resolve(widget["AP"])) ?? nil, let normal = (try? file.resolve(ap["N"])) ?? nil,
                  normal["BBox"] != nil else {
                if let states = ((try? file.resolve(widget["AP"])) ?? nil)?["N"], case .dict(let pairs) = states,
                   let onName = pairs.map(\.0).first(where: { $0 != "Off" }), widget["AS"]?.nameValue != (on ? onName : "Off") {
                    widget = widget.setting("AS", .name(on ? onName : "Off"))
                    changed = true
                }
                continue
            }
            changed = true
            let w0 = abs(rect[2] - rect[0]), h0 = abs(rect[3] - rect[1])
            let box = String(format: "0.9 0.94 1 rg 0 0 %.2f %.2f re f 0.45 G 1 w 0.5 0.5 %.2f %.2f re S", w0, h0, w0 - 1, h0 - 1)
            let check = String(format: " 0 G %.2f w 1 J 1 j %.2f %.2f m %.2f %.2f l %.2f %.2f l S",
                               max(1, w0 * 0.12), w0 * 0.22, h0 * 0.52, w0 * 0.42, h0 * 0.28, w0 * 0.80, h0 * 0.76)
            func stream(_ content: String) -> [UInt8] {
                Array("<< /Type /XObject /Subtype /Form /BBox [0 0 \(String(format: "%.2f %.2f", w0, h0))] /Length \(content.utf8.count) >>\nstream\n\(content)\nendstream".utf8)
            }
            let yes = nextNum, off = nextNum + 1
            nextNum += 2
            updates[yes] = stream(box + check)
            updates[off] = stream(box)
            widget = widget.setting("AP", .raw("<< /N << /Yes \(yes) 0 R /Off \(off) 0 R >> /D << /Yes \(yes) 0 R /Off \(off) 0 R >> >>"))
                .setting("AS", .name(on ? "Yes" : "Off"))
            if widget["FT"] != nil { widget = widget.setting("V", .name(on ? "Yes" : "Off")) }
        }

        // (3) streams without /Length
        for (num, entry) in file.offsets where updates[num] == nil {
            var lexer = PDFLexer(bytes: file.bytes, pos: entry.offset)
            _ = lexer.word(); _ = lexer.word()
            guard (try? lexer.expect("obj")) != nil, let dict = try? lexer.parse(), case .dict = dict, dict["Length"] == nil else { continue }
            lexer.skipWhitespace()
            guard lexer.peek("stream") else { continue }
            var start = lexer.pos + 6
            if start < file.bytes.count, file.bytes[start] == 0x0D { start += 1 }
            if start < file.bytes.count, file.bytes[start] == 0x0A { start += 1 }
            guard let end = find("endstream", in: file.bytes, from: start) else { continue }
            var stop = end
            if stop > start, file.bytes[stop - 1] == 0x0A { stop -= 1 }
            if stop > start, file.bytes[stop - 1] == 0x0D { stop -= 1 }
            updates[num] = Array(dict.setting("Length", .raw("\(stop - start)")).serialized.utf8) + Array("\nstream\n".utf8)
                + Array(file.bytes[start..<stop]) + Array("\nendstream".utf8)
        }
        guard !updates.isEmpty else { return data }

        var out = [UInt8](data)
        if out.last != 0x0A { out.append(0x0A) }
        var entries: [(num: Int, offset: Int)] = []
        for num in updates.keys.sorted() {
            entries.append((num, out.count))
            let gen = num >= size ? 0 : file.gen(num)
            out += Array("\(num) \(gen) obj\n".utf8) + updates[num]! + Array("\nendobj\n".utf8)
        }
        let xrefOffset = out.count
        var xref = "xref\n"
        for e in entries { xref += "\(e.num) 1\n" + String(format: "%010d %05d n\r\n", e.offset, e.num >= size ? 0 : file.gen(e.num)) }
        let trailer = file.trailer.removing("XRefStm").removing("Prev")
            .setting("Size", .raw("\(nextNum)")).setting("Prev", .raw("\(file.startxref)"))
        xref += "trailer\n\(trailer.serialized)\nstartxref\n\(xrefOffset)\n%%EOF\n"
        out += Array(xref.utf8)
        return Data(out)
    }

    /// "#20" → " " in a PDF name.
    static func unescapeName(_ s: String) -> String {
        var out = [UInt8](), b = Array(s.utf8), i = 0
        while i < b.count {
            if b[i] == UInt8(ascii: "#"), i + 2 < b.count, let v = UInt8(String(decoding: b[i + 1...i + 2], as: UTF8.self), radix: 16) {
                out.append(v); i += 3
            } else { out.append(b[i]); i += 1 }
        }
        return String(decoding: out, as: UTF8.self)
    }

    static func find(_ s: String, in bytes: [UInt8], from: Int) -> Int? {
        let needle = Array(s.utf8)
        var i = from
        while i + needle.count <= bytes.count {
            if bytes[i] == needle[0], Array(bytes[i..<i + needle.count]) == needle { return i }
            i += 1
        }
        return nil
    }
}

private extension PDFObject {
    var number: Double? { if case .raw(let s) = self { return Double(s) } else { return nil } }
}
