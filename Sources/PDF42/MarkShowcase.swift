import AppKit
import Kit42

/// `PDF42 --mark-showcase out.pdf`: a four-page sample of the signature and stamp generator — a short description,
/// ten signature styles of one name with five copies each, stamps of many countries (three impressions each,
/// scattered), and five signature sections of documents between companies of different countries.
enum InkDrawing {
    static func scaled(_ image: CGImage, width: Int) -> CGImage? {
        let height = max(1, Int(CGFloat(image.height) * CGFloat(width) / CGFloat(image.width)))
        guard let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        ctx.interpolationQuality = .high
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        return ctx.makeImage()
    }
}

enum MarkShowcase {
    private static let page = CGRect(x: 0, y: 0, width: 595, height: 842)   // A4
    private static let blue = InkColor.blue.pen

    static func write(to url: URL) throws {
        let data = NSMutableData()
        var box = page
        guard let consumer = CGDataConsumer(data: data), let ctx = CGContext(consumer: consumer, mediaBox: &box, nil) else { return }
        descriptionPage(ctx)
        signaturesPage(ctx)
        stampsPage(ctx)
        documentsPage(ctx)
        ctx.closePDF()
        try (data as Data).write(to: url)
    }

    // MARK: - Drawing helpers

    private static func font(_ size: CGFloat, bold: Bool = false) -> NSFont {
        NSFont(name: bold ? "HelveticaNeue-Bold" : "HelveticaNeue", size: size) ?? .systemFont(ofSize: size)
    }

    /// One line of text with its baseline at `y`.
    private static func text(_ ctx: CGContext, _ s: String, x: CGFloat, y: CGFloat, size: CGFloat, bold: Bool = false,
                             color: NSColor = .black) {
        let line = CTLineCreateWithAttributedString(NSAttributedString(string: s, attributes: [.font: font(size, bold: bold), .foregroundColor: color]))
        ctx.textPosition = CGPoint(x: x, y: y)
        CTLineDraw(line, ctx)
    }

    /// Wrapped paragraph in a rectangle; returns the height used.
    @discardableResult
    private static func paragraph(_ ctx: CGContext, _ s: NSAttributedString, in rect: CGRect) -> CGFloat {
        let setter = CTFramesetterCreateWithAttributedString(s)
        let size = CTFramesetterSuggestFrameSizeWithConstraints(setter, CFRange(location: 0, length: 0), nil,
                                                                CGSize(width: rect.width, height: .greatestFiniteMagnitude), nil)
        let frameRect = CGRect(x: rect.minX, y: rect.maxY - size.height, width: rect.width, height: size.height)
        CTFrameDraw(CTFramesetterCreateFrame(setter, CFRange(location: 0, length: 0), CGPath(rect: frameRect, transform: nil), nil), ctx)
        return size.height
    }

    /// Ink is multiplied with the page, like a pen or a stamp pad on paper.
    private static func ink(_ ctx: CGContext, _ image: CGImage?, center: CGPoint, width: CGFloat) {
        guard var image else { return }
        // About 300 dpi at the printed size keeps the file small.
        let target = Int(width * 300 / 72)
        if image.width > target, let small = InkDrawing.scaled(image, width: target) { image = small }
        let h = width * CGFloat(image.height) / CGFloat(max(1, image.width))
        ctx.saveGState()
        ctx.setBlendMode(.multiply)
        ctx.interpolationQuality = .high
        ctx.draw(image, in: CGRect(x: center.x - width / 2, y: center.y - h / 2, width: width, height: h))
        ctx.restoreGState()
    }

    private static func signature(_ name: String, form: SignatureForm, variant: Int, seed: UInt64, copy: Int,
                                  pen: PenType = .ballpoint, inkColor: NSColor = blue) -> CGImage? {
        SignatureGenerator.render(name, design: SignatureGenerator.design(form: form, variant: variant, seed: seed),
                                  impression: UInt64(copy + 1) &* 7919 &+ seed, ink: inkColor, pen: pen)
    }

    private static func seal(_ id: String, _ values: [String: String] = [:], impression: UInt64) -> CGImage? {
        guard let t = SealTemplate.named(id) else { return nil }
        return SealGenerator.render(SealSpec(template: t, values: values, ink: t.ink.stampPad, designSeed: 1), impression: impression)
    }

    private static func header(_ ctx: CGContext, _ title: String, _ subtitle: String) {
        text(ctx, title, x: 40, y: 790, size: 20, bold: true)
        text(ctx, subtitle, x: 40, y: 770, size: 10, color: .darkGray)
        ctx.setFillColor(NSColor(white: 0.85, alpha: 1).cgColor)
        ctx.fill(CGRect(x: 40, y: 760, width: 515, height: 0.7))
    }

    // MARK: - Page 1: description

    private static func descriptionPage(_ ctx: CGContext) {
        ctx.beginPDFPage(nil)
        header(ctx, "Подписи и печати в Suite42", "PDF42 · DOC42 · IMG42 · PPT42 · XLS42 — работает локально, без облака")
        let body = NSMutableAttributedString()
        func h(_ s: String) { body.append(NSAttributedString(string: s + "\n", attributes: [.font: font(12.5, bold: true)])) }
        func p(_ s: String) {
            let style = NSMutableParagraphStyle()
            style.paragraphSpacing = 7
            style.headIndent = 12
            style.firstLineHeadIndent = 0
            body.append(NSAttributedString(string: s + "\n", attributes: [.font: font(10.5), .paragraphStyle: style]))
        }
        h("Рукописная подпись")
        p("• Генератор пишет подпись моделью быстрой руки: гладкая линия через нарисованные вручную скелеты букв (латиница, кириллица, турецкие буквы), скорость пера — из сигма-логнормальной модели быстрых движений (Plamondon). Разгон, торможение и «вылет» пера получаются сами.")
        p("• Формы: полностью, инициал и фамилия, фамилия, инициалы с росчерком, заглавная с росчерком. 1320 стилей; окно показывает три разных, «Новые варианты» — ещё три.")
        p("• Ручки: шариковая (больше чернил на нисходящих штрихах, капли на медленных участках, пропуски на быстрых, бороздки от шарика), гелевая, перьевая (ширина зависит от угла штриха), фломастер; волокна бумаги под любой ручкой.")
        p("• «Моя подпись»: распишитесь несколько раз на трекпаде (записываются время и нажим), откройте скан или эскиз с iPhone — и генератор продолжит писать вашей рукой.")
        p("• Каждая вставка — новый экземпляр: форма та же, но размер, наклон, нажим и оттенок чернил немного другие, как у живой подписи.")
        h("Печати и штампы")
        p("• 132 вида в 15 категориях: Россия и СНГ, Европа, Великобритания, Турция, США и Канада, Гонконг и Тайвань (chop с местом для подписи, малая круглая), Китай, Япония (ханко, какуин, датер), Корея, Индия, Ближний Восток, Латинская Америка, офисные штампы, датеры, личные. Все тексты редактируются.")
        p("• Свою печать можно загрузить как SVG, PDF или картинку.")
        p("• Каждый оттиск разный: наклон (круглые 2–8°, прямоугольные 0,3–2°), неравномерный нажим подушки, зерно, пропуски, иногда лёгкий двойной оттиск.")
        p("• Печать и подпись ложатся поверх текста как чернила (умножение): текст под ними остаётся чёрным, пересечения темнее. В PDF это сохраняется в файл.")
        p("• Только обобщённые образцы: без гербов, без нотариальных, судебных, таможенных и рецептурных печатей.")
        h("Качество измеряется")
        p("• Стенд PDF42 --signature-bench считает беглость (рывок, «зависания»), дрожание линии, разброс копий и различимость стилей по эталонам беглой и неуверенной руки, а также читаемость через Vision. С его помощью доля «зависаний» снижена с 36 % до 8 % — у беглого эталона 5 %.")
        let used = paragraph(ctx, body, in: CGRect(x: 40, y: 230, width: 515, height: 520))
        // A small example under the text: a signature line with a stamp over it.
        let y = 750 - used - 70
        text(ctx, "Пример:", x: 40, y: y + 40, size: 9, color: .gray)
        text(ctx, "Генеральный директор", x: 60, y: y + 8, size: 11)
        text(ctx, "ООО «ЗОРИН ГРУПП»", x: 60, y: y - 7, size: 11)
        text(ctx, "_______________________  / С. Зорин /", x: 250, y: y, size: 11)
        text(ctx, "М. П.", x: 250, y: y - 36, size: 11)
        ink(ctx, seal("ru.ooo", ["edge": "ООО «ЗОРИН ГРУПП» * г. МОСКВА *", "inner": "ОГРН 1267700000011 * ИНН 7700000011",
                                  "centre": "ДЛЯ\nДОКУМЕНТОВ"], impression: 11), center: CGPoint(x: 300, y: y - 28), width: 100)
        ink(ctx, signature("Сергей Зорин", form: .initialSurname, variant: 4, seed: 902, copy: 0), center: CGPoint(x: 305, y: y + 12), width: 120)
        ctx.endPDFPage()
    }

    // MARK: - Page 2: signatures

    private static func signaturesPage(_ ctx: CGContext) {
        ctx.beginPDFPage(nil)
        header(ctx, "Подписи «Сергей Зорин»", "10 стилей, по 5 экземпляров каждого: форма одна, каждый экземпляр написан немного иначе")
        let styles: [(SignatureForm, Int, String, PenType)] = [
            (.fullName, 0, "Полностью", .ballpoint), (.fullName, 1, "Полностью", .gel),
            (.initialSurname, 2, "Инициал и фамилия", .ballpoint), (.initialSurname, 3, "Инициал и фамилия", .fountain),
            (.surname, 4, "Фамилия", .ballpoint), (.surname, 5, "Фамилия", .ballpoint),
            (.initialsScribble, 6, "Инициалы и росчерк", .ballpoint), (.initialsScribble, 7, "Инициалы и росчерк", .gel),
            (.capitalScribble, 8, "Заглавная и росчерк", .ballpoint), (.capitalScribble, 9, "Заглавная и росчерк", .felt),
        ]
        let rowHeight: CGFloat = 70
        for (r, s) in styles.enumerated() {
            let top = 745 - CGFloat(r) * rowHeight
            let seed = UInt64(500 + r * 17)
            let number = SignatureGenerator.styleNumber(SignatureGenerator.design(form: s.0, variant: s.1, seed: seed))
            text(ctx, "\(s.2) · стиль \(number) из \(SignatureGenerator.styleCount) · \(s.3.title)", x: 40, y: top - 10, size: 8.5, color: .gray)
            for c in 0..<5 {
                ink(ctx, signature("Сергей Зорин", form: s.0, variant: s.1, seed: seed, copy: c, pen: s.3),
                    center: CGPoint(x: 92 + CGFloat(c) * 103, y: top - 40), width: 96)
            }
        }
        ctx.endPDFPage()
    }

    // MARK: - Page 3: stamps

    private static func stampsPage(_ ctx: CGContext) {
        ctx.beginPDFPage(nil)
        header(ctx, "Печати разных стран", "13 печатей, по 3 оттиска каждой; у каждого оттиска свой наклон, нажим и зерно")
        let list: [(String, CGFloat, String)] = [
            ("ru.ooo", 86, "Россия"), ("kz.round", 86, "Казахстан"), ("de.firm", 122, "Германия"), ("uk.common", 86, "Великобритания"),
            ("tr.kase", 122, "Турция"), ("us.corporate", 88, "США"), ("hk.round.small", 78, "Гонконг"), ("cn.company", 86, "Китай"),
            ("jp.hanko", 54, "Япония"), ("kr.dojang.square", 58, "Корея"), ("in.round", 84, "Индия"), ("ae.round", 86, "ОАЭ"),
            ("br.cnpj", 122, "Бразилия"),
        ]
        var rng = SeededRandom(2026)
        var placed: [CGRect] = []
        let area = CGRect(x: 40, y: 60, width: 515, height: 680)
        for (i, item) in list.enumerated() {
            for k in 0..<3 {
                guard let image = seal(item.0, impression: UInt64(i * 10 + k + 1) &* 2_654_435_761) else { continue }
                let w = item.1, h = w * CGFloat(image.height) / CGFloat(image.width)
                // Random place with little overlap (stamps on a sheet overlap a bit, not much).
                var best = CGRect.zero, bestOverlap = CGFloat.infinity
                for _ in 0..<1500 {
                    let r = CGRect(x: area.minX + rng.range(0...(area.width - w)), y: area.minY + rng.range(0...(area.height - h)), width: w, height: h)
                    let overlap = placed.reduce(CGFloat(0)) { acc, p in
                        let x = r.intersection(p.insetBy(dx: 6, dy: 6))
                        return acc + (x.isNull ? 0 : x.width * x.height)
                    }
                    if overlap < bestOverlap { best = r; bestOverlap = overlap }
                    if overlap == 0 { break }
                }
                placed.append(best)
                ink(ctx, image, center: CGPoint(x: best.midX, y: best.midY), width: w)
            }
        }
        paragraph(ctx, NSAttributedString(string: "Страны: " + list.map(\.2).joined(separator: ", ") + ". Тексты — образцы, их можно заменить на свои.",
                                          attributes: [.font: font(8.5), .foregroundColor: NSColor.gray]), in: CGRect(x: 40, y: 20, width: 515, height: 30))
        ctx.endPDFPage()
    }

    // MARK: - Page 4: signature sections of documents

    private struct Party {
        var role: String, company: String, position: String, name: String, signer: String, date: String, place: String
        var form: SignatureForm, variant: Int, seed: UInt64
        var stamp: String, values: [String: String], stampWidth: CGFloat
        var pen: PenType = .ballpoint
    }

    private static func documentsPage(_ ctx: CGContext) {
        ctx.beginPDFPage(nil)
        header(ctx, "Подписи в документах", "Пять договоров: слева иностранная сторона (Sergey Zorin), справа — Россия и СНГ (Сергей Зорин)")
        let pairs: [(String, Party, Party)] = [
            ("Договор поставки · Германия — Россия",
             Party(role: "For the Supplier", company: "Zorin Consulting GmbH", position: "Managing Director", name: "Sergey Zorin",
                   signer: "/ S. Zorin /", date: "Berlin, 6 November 2026", place: "", form: .initialSurname, variant: 2, seed: 101,
                   stamp: "de.firm", values: ["name": "Zorin Consulting GmbH", "lines": "Friedrichstraße 10\n10117 Berlin", "numbers": "USt-IdNr. DE326000011"], stampWidth: 138),
             Party(role: "От Покупателя", company: "ООО «ЗОРИН ГРУПП»", position: "Генеральный директор", name: "Сергей Зорин",
                   signer: "/ С. Зорин /", date: "«6» ноября 2026 г.", place: "М. П.", form: .initialSurname, variant: 4, seed: 902,
                   stamp: "ru.ooo", values: ["edge": "ООО «ЗОРИН ГРУПП» * г. МОСКВА *", "inner": "ОГРН 1267700000011 * ИНН 7700000011", "centre": "ДЛЯ\nДОКУМЕНТОВ"], stampWidth: 100)),
            ("Договор оказания услуг · Великобритания — Казахстан",
             Party(role: "For the Contractor", company: "Zorin Trading Ltd", position: "Director", name: "Sergey Zorin",
                   signer: "/ S. Zorin /", date: "London, 6 November 2026", place: "", form: .fullName, variant: 1, seed: 202,
                   stamp: "uk.company", values: ["name": "Zorin Trading Ltd", "lines": "1 Bishopsgate, London EC2N 3AS", "numbers": "Registered in England No. 16000011"], stampWidth: 140, pen: .gel),
             Party(role: "От Заказчика", company: "ТОО «ЗОРИН КАЗАХСТАН»", position: "Директор", name: "Сергей Зорин",
                   signer: "/ С. Зорин /", date: "«6» ноября 2026 г.", place: "М. П.", form: .surname, variant: 5, seed: 903,
                   stamp: "kz.round", values: ["edge": "«ЗОРИН ҚАЗАҚСТАН» ЖШС * АЛМАТЫ", "bottom": "ТОО «ЗОРИН КАЗАХСТАН» * АЛМАТЫ", "centre": "БСН\n260540000011"], stampWidth: 100, pen: .gel)),
            ("Соглашение о сотрудничестве · Гонконг — Россия (ИП)",
             Party(role: "For the Company", company: "Zorin Holdings Limited", position: "Authorised Signatory", name: "Sergey Zorin",
                   signer: "", date: "Hong Kong, 6 November 2026", place: "", form: .initialsScribble, variant: 6, seed: 303,
                   stamp: "hk.chop", values: ["top": "For and on behalf of", "name": "ZORIN HOLDINGS LIMITED", "local": "左林控股有限公司", "caption": "Authorised Signature(s)"], stampWidth: 150),
             Party(role: "Индивидуальный предприниматель", company: "ИП Зорин Сергей", position: "", name: "Сергей Зорин",
                   signer: "/ С. Зорин /", date: "«6» ноября 2026 г.", place: "М. П.", form: .capitalScribble, variant: 8, seed: 904,
                   stamp: "ru.ip", values: ["edge": "ИНДИВИДУАЛЬНЫЙ ПРЕДПРИНИМАТЕЛЬ * г. МОСКВА *", "inner": "ОГРНИП 326770000000011", "centre": "ЗОРИН\nСЕРГЕЙ"], stampWidth: 98)),
            ("Агентский договор · Турция — Беларусь",
             Party(role: "Acente adına / For the Agent", company: "Zorin Danışmanlık Ltd. Şti.", position: "Genel Müdür", name: "Sergey Zorin",
                   signer: "/ S. Zorin /", date: "İstanbul, 6 Kasım 2026", place: "", form: .surname, variant: 4, seed: 404,
                   stamp: "tr.kase", values: ["name": "ZORİN DANIŞMANLIK LTD. ŞTİ.", "lines": "Büyükdere Cad. No: 10\nŞişli / İSTANBUL", "numbers": "Şişli V.D. 9990000011"], stampWidth: 140),
             Party(role: "От Принципала", company: "ООО «ЗОРИН БЕЛ»", position: "Директор", name: "Сергей Зорин",
                   signer: "/ С. Зорин /", date: "«6» ноября 2026 г.", place: "М. П.", form: .fullName, variant: 0, seed: 905,
                   stamp: "by.round", values: ["edge": "ОБЩЕСТВО С ОГРАНИЧЕННОЙ ОТВЕТСТВЕННОСТЬЮ «ЗОРИН БЕЛ»", "centre": "УНП\n193000011\nг. МИНСК"], stampWidth: 100)),
            ("Лицензионный договор · США — Россия",
             Party(role: "For the Licensor", company: "Zorin Technologies Inc.", position: "President", name: "Sergey Zorin",
                   signer: "/ S. Zorin /", date: "New York, November 6, 2026", place: "", form: .capitalScribble, variant: 9, seed: 505,
                   stamp: "us.corporate", values: ["edge": "ZORIN TECHNOLOGIES INC.", "bottom": "DELAWARE", "centre": "CORPORATE\nSEAL\n2026"], stampWidth: 100, pen: .fountain),
             Party(role: "От Лицензиата", company: "АО «ЗОРИН ТЕХ»", position: "Генеральный директор", name: "Сергей Зорин",
                   signer: "/ С. Зорин /", date: "«6» ноября 2026 г.", place: "М. П.", form: .initialsScribble, variant: 7, seed: 906,
                   stamp: "ru.rope", values: ["edge": "АКЦИОНЕРНОЕ ОБЩЕСТВО «ЗОРИН ТЕХ» * САНКТ-ПЕТЕРБУРГ *", "centre": "ДЛЯ\nДОГОВОРОВ"], stampWidth: 100, pen: .fountain)),
        ]
        let blockHeight: CGFloat = 142
        for (i, (title, left, right)) in pairs.enumerated() {
            let top = 742 - CGFloat(i) * blockHeight
            text(ctx, "\(i + 1). \(title)", x: 40, y: top - 12, size: 8.5, bold: true, color: .darkGray)
            party(ctx, left, x: 40, top: top - 26, impression: UInt64(i * 2 + 1))
            party(ctx, right, x: 310, top: top - 26, impression: UInt64(i * 2 + 2))
            if i < pairs.count - 1 {
                ctx.setFillColor(NSColor(white: 0.88, alpha: 1).cgColor)
                ctx.fill(CGRect(x: 40, y: top - blockHeight + 6, width: 515, height: 0.5))
            }
        }
        ctx.endPDFPage()
    }

    /// One side of a signature section: role, company, position, the signature line with the signature over it,
    /// date, and the stamp pressed over part of the text.
    private static func party(_ ctx: CGContext, _ p: Party, x: CGFloat, top: CGFloat, impression: UInt64) {
        text(ctx, p.role, x: x, y: top, size: 9.5, bold: true)
        text(ctx, p.company, x: x, y: top - 14, size: 9.5)
        if !p.position.isEmpty { text(ctx, p.position, x: x, y: top - 27, size: 9.5) }
        let lineY = top - 62
        let chop = p.stamp == "hk.chop"
        if !chop {
            text(ctx, "____________________  " + p.signer, x: x, y: lineY, size: 9.5)
            text(ctx, p.date, x: x, y: lineY - 18, size: 9)
            if !p.place.isEmpty { text(ctx, p.place, x: x + 138, y: lineY - 18, size: 9) }
        } else {
            text(ctx, p.date, x: x, y: lineY - 46, size: 9)
        }
        let stamp = seal(p.stamp, p.values, impression: impression &* 7_777_777)
        let sig = signature(p.name, form: p.form, variant: p.variant, seed: p.seed, copy: Int(impression), pen: p.pen)
        if chop {
            // Hong Kong chop: the signature goes on the chop's own line.
            // The chop's signature line is in its lower part; the signature crosses it.
            let center = CGPoint(x: x + 120, y: lineY - 4)
            let h = stamp.map { p.stampWidth * CGFloat($0.height) / CGFloat($0.width) } ?? 60
            ink(ctx, stamp, center: center, width: p.stampWidth)
            ink(ctx, sig, center: CGPoint(x: center.x - 4, y: center.y - h * 0.24), width: 88)
        } else {
            let round = p.stampWidth < 120
            // Round seals over "М. П." to the right of the signature, rectangular stamps beside the line.
            let stampCenter = round ? CGPoint(x: x + 192, y: lineY - 22) : CGPoint(x: x + 178, y: lineY - 6)
            ink(ctx, stamp, center: stampCenter, width: p.stampWidth)
            ink(ctx, sig, center: CGPoint(x: x + 62, y: lineY + 12), width: 108)
        }
    }
}
