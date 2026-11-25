import Kit42
import AppKit

/// Renders values with Excel number-format codes ("#,##0.00", "0%", "dd.mm.yyyy", "[Red]-0.0", …).
/// Separators follow the system locale, like Excel does.
enum NumberFormat {
    struct Result {
        var text: String
        var color: NSColor?
    }

    static let decimal = Locale.current.decimalSeparator ?? "."
    static let grouping: String = {
        let g = Locale.current.groupingSeparator ?? ","
        return g == "\u{202F}" || g == " " ? "\u{00A0}" : g
    }()

    /// Built-in numFmtId → format code.
    static let builtin: [Int: String] = [
        0: "General", 1: "0", 2: "0.00", 3: "#,##0", 4: "#,##0.00", 9: "0%", 10: "0.00%", 11: "0.00E+00",
        12: "# ?/?", 13: "# ??/??", 14: "dd.mm.yyyy", 15: "d-mmm-yy", 16: "d-mmm", 17: "mmm-yy",
        18: "h:mm AM/PM", 19: "h:mm:ss AM/PM", 20: "h:mm", 21: "h:mm:ss", 22: "dd.mm.yyyy h:mm",
        37: "#,##0 ;(#,##0)", 38: "#,##0 ;[Red](#,##0)", 39: "#,##0.00;(#,##0.00)", 40: "#,##0.00;[Red](#,##0.00)",
        44: "_-* #,##0.00_-;-* #,##0.00_-;_-* \"-\"??_-;_-@_-",
        45: "mm:ss", 46: "[h]:mm:ss", 47: "mm:ss.0", 48: "##0.0E+0", 49: "@",
    ]

    static func isDateFormat(_ code: String) -> Bool {
        let section = splitSections(code).first ?? code
        let s = strip(section).lowercased()
        let elapsed = section.range(of: #"\[[hms]+\]"#, options: [.regularExpression, .caseInsensitive]) != nil
        let withoutFractions = s.replacingOccurrences(of: #"s[.,]0+"#, with: "s", options: .regularExpression)
        return (elapsed || s.contains(where: { "ymdhs".contains($0) })) && !withoutFractions.contains("0") && !s.contains("#")
    }

    static func format(_ value: Value, _ code: String, date1904: Bool = false) -> Result {
        switch value {
        case .empty: return Result(text: "")
        case .bool(let b): return Result(text: b ? L("TRUE") : L("FALSE"))
        case .error(let e): return Result(text: errorText(e))
        case .string(let s):
            let sections = splitSections(code)
            if sections.count >= 4 { return Result(text: sections[3].replacingOccurrences(of: "@", with: s).filter { $0 != "\"" }) }
            return Result(text: s)
        case .number(let d):
            guard d.isFinite else { return Result(text: errorText("#NUM!")) }
            if code == "General" || code.isEmpty { return Result(text: general(d)) }
            return formatNumber(d, code, date1904: date1904)
        }
    }

    static func errorText(_ e: String) -> String {
        switch Language.current {
        case .ru: return Formula.displayError(e)
        case .tr:
            let tr = ["#DIV/0!": "#SAYI/0!", "#VALUE!": "#DEĞER!", "#REF!": "#BAŞV!", "#NAME?": "#AD?", "#N/A": "#YOK", "#NUM!": "#SAYI!", "#NULL!": "#BOŞ!"]
            return tr[e] ?? e
        default: return e
        }
    }

    /// "General": up to 11 significant digits, locale decimal separator.
    static func general(_ d: Double) -> String {
        if d.isNaN || d.isInfinite { return errorText("#NUM!") }
        if d == 0 { return "0" }
        let a = abs(d)
        var s: String
        if a >= 1e11 || a < 1e-9 {
            s = String(format: "%.5E", d)
            // 1.23450E+12 → 1,2345E+12
            if let e = s.firstIndex(of: "E") {
                var mant = String(s[..<e])
                while mant.contains("."), mant.hasSuffix("0") { mant.removeLast() }
                if mant.hasSuffix(".") { mant.removeLast() }
                s = mant + s[e...]
            }
        } else {
            let digits = max(0, 10 - Int(floor(log10(a))))
            s = String(format: "%.\(min(digits, 10))f", d)
            if s.contains(".") {
                while s.hasSuffix("0") { s.removeLast() }
                if s.hasSuffix(".") { s.removeLast() }
            }
        }
        return s.replacingOccurrences(of: ".", with: decimal)
    }

    static func splitSections(_ code: String) -> [String] {
        var sections: [String] = [], cur = "", quoted = false, escaped = false
        for ch in code {
            if escaped { cur.append(ch); escaped = false; continue }
            if ch == "\\" { cur.append(ch); escaped = true; continue }
            if ch == "\"" { quoted.toggle() }
            if ch == ";", !quoted { sections.append(cur); cur = ""; continue }
            cur.append(ch)
        }
        sections.append(cur)
        return sections
    }

    /// Removes [Color], [$-locale], quotes and escapes for analysis.
    static func strip(_ s: String) -> String {
        var out = "", quoted = false, bracket = false, escaped = false
        for ch in s {
            if escaped { escaped = false; continue }
            if ch == "\\" { escaped = true; continue }
            if ch == "\"" { quoted.toggle(); continue }
            if quoted { continue }
            if ch == "[" { bracket = true; continue }
            if ch == "]" { bracket = false; continue }
            if bracket { continue }
            out.append(ch)
        }
        return out
    }

    static let colors: [String: NSColor] = [
        "red": .systemRed, "blue": .systemBlue, "green": .systemGreen, "black": .black,
        "white": .white, "yellow": .systemYellow, "magenta": .systemPink, "cyan": .systemTeal,
    ]

    private static func formatNumber(_ d: Double, _ code: String, date1904: Bool) -> Result {
        let sections = splitSections(code)
        var section = sections[0]
        var value = d
        // Conditions may follow color/locale tags and either of the first two
        // sections may have one. A third section is their fallback.
        func parseCondition(_ s: String) -> (String, Double)? {
            var rest = s[...]
            while let open = rest.firstIndex(of: "["), let close = rest[open...].firstIndex(of: "]") {
                let inner = String(rest[rest.index(after: open)..<close])
                for op in [">=", "<=", "<>", ">", "<", "="] where inner.hasPrefix(op) {
                    if let value = Double(inner.dropFirst(op.count)) { return (op, value) }
                }
                rest = rest[rest.index(after: close)...]
            }
            return nil
        }
        func matches(_ condition: (String, Double)) -> Bool {
            let (op, limit) = condition
            switch op { case ">=": return d >= limit; case "<=": return d <= limit; case "<>": return d != limit
            case ">": return d > limit; case "<": return d < limit; default: return d == limit }
        }
        if sections.prefix(2).contains(where: { parseCondition($0) != nil }) {
            section = sections.prefix(3).first { parseCondition($0).map(matches) ?? true } ?? ""
        } else if d < 0, sections.count > 1 {
            section = sections[1]
            if !section.isEmpty { value = -d }
        } else if d == 0, sections.count > 2 {
            section = sections[2]
        }

        // Colour and locale tokens
        var color: NSColor?
        var cleaned = ""
        var i = section.startIndex
        while i < section.endIndex {
            if section[i] == "[", let close = section[i...].firstIndex(of: "]") {
                let token = String(section[section.index(after: i)..<close])
                if let c = colors[token.lowercased()] { color = c }
                else if token.hasPrefix("$") {
                    // [$₽-419] → ₽, [$-419] → nothing
                    let symbol = token.dropFirst().split(separator: "-", maxSplits: 1).first.map(String.init) ?? ""
                    if !symbol.isEmpty { cleaned += "\"" + symbol + "\"" }
                } else if token.lowercased().hasPrefix("h") || token.lowercased().hasPrefix("m") || token.lowercased().hasPrefix("s") {
                    cleaned += "[" + token + "]"   // elapsed time
                }
                i = section.index(after: close)
                continue
            }
            cleaned.append(section[i])
            i = section.index(after: i)
        }

        if cleaned.lowercased() == "general" { return Result(text: general(value), color: color) }
        if isDateFormat(cleaned) { return Result(text: formatDate(value, cleaned, date1904: date1904), color: color) }
        return Result(text: formatPlain(value, cleaned, negativeSign: value < 0), color: color)
    }

    // MARK: - Numbers

    private static func formatPlain(_ value: Double, _ code: String, negativeSign: Bool) -> String {
        // Tokenise into literals and the numeric pattern.
        var prefix = "", suffix = "", pattern = ""
        var inPattern = false, patternDone = false
        var chars = Array(code)
        var i = 0
        func literal(_ s: String) { if patternDone || inPattern { suffix += s } else { prefix += s } }
        while i < chars.count {
            let ch = chars[i]
            switch ch {
            case "\"":
                var j = i + 1, s = ""
                while j < chars.count, chars[j] != "\"" { s.append(chars[j]); j += 1 }
                if inPattern { patternDone = true; inPattern = false }
                literal(s); i = j + 1; continue
            case "\\":
                if i + 1 < chars.count {
                    if inPattern { patternDone = true; inPattern = false }
                    literal(String(chars[i + 1]))
                }
                i += 2; continue
            case "_":
                if inPattern, i + 1 < chars.count, chars[i + 1] == ")" || chars[i + 1] == "-" { }
                literal(" "); i += 2; continue
            case "*":
                i += 2; continue
            case "0", "#", "?", ".", ",", "E", "e", "+", "/":
                if patternDone && !"Ee+".contains(ch) {
                    suffix.append(ch)
                } else if "Ee".contains(ch), !inPattern {
                    literal(String(ch))
                } else {
                    inPattern = true
                    pattern.append(ch)
                }
            case "-" where inPattern && pattern.last.map({ "Ee".contains($0) }) == true:
                pattern.append(ch)
            case "%":
                if inPattern { patternDone = true; inPattern = false }
                literal("%")
            case "@":
                literal("")
            default:
                if inPattern { patternDone = true; inPattern = false }
                literal(String(ch))
            }
            i += 1
        }
        if pattern.isEmpty { return (negativeSign ? "-" : "") + prefix + suffix }
        chars = Array(pattern)

        var v = abs(value)
        if (prefix + suffix).contains("%") { v *= 100 }
        guard v.isFinite else { return errorText("#NUM!") }

        // Scientific
        if let e = pattern.firstIndex(where: { $0 == "E" || $0 == "e" }) {
            let mantissa = String(pattern[..<e])
            let decimals = mantissa.split(separator: ".").dropFirst().first?.filter { "0#?".contains($0) }.count ?? 0
            guard v.isFinite else { return errorText("#NUM!") }
            // Let the formatter round the mantissa and carry into the exponent.
            let scientific = String(format: "%.*E", decimals, v).split(separator: "E")
            guard scientific.count == 2, let exp = Int(scientific[1]) else { return errorText("#NUM!") }
            let ms = String(scientific[0]).replacingOccurrences(of: ".", with: decimal)

            let expDigits = pattern[pattern.index(after: e)...].filter { $0 == "0" }.count
            let es = String(format: "%0\(max(1, expDigits))d", abs(exp))
            return (negativeSign ? "-" : "") + prefix + ms + "E" + (exp < 0 ? "-" : "+") + es + suffix
        }

        // Fractions (# ?/?)
        if pattern.contains("/") {
            let whole = floor(v)
            let frac = v - whole
            var best = (0, 1), err = 1.0
            for den in 1...99 {
                let num = Int((frac * Double(den)).rounded())
                let e = abs(frac - Double(num) / Double(den))
                if e < err - 1e-12 { err = e; best = (num, den) }
            }
            let w = whole > 0 || best.0 == 0 ? String(format: "%.0f", whole) : ""
            let f = best.0 == 0 ? "" : "\(best.0)/\(best.1)"
            return (negativeSign ? "-" : "") + prefix + [w, f].filter { !$0.isEmpty }.joined(separator: " ") + suffix
        }

        // Trailing commas scale by 1000
        var p = pattern
        while p.hasSuffix(",") { p.removeLast(); v /= 1000 }
        let parts = p.split(separator: ".", maxSplits: 1, omittingEmptySubsequences: false)
        let intPattern = String(parts[0])
        let fracPattern = parts.count > 1 ? String(parts[1]) : ""
        let decimals = fracPattern.filter { "0#?".contains($0) }.count
        let minDecimals = fracPattern.filter { $0 == "0" }.count
        let useGrouping = intPattern.contains(",")
        let minInt = intPattern.filter { $0 == "0" }.count

        var s = String(format: "%.\(decimals)f", v)
        var intPart = String(s.split(separator: ".").first ?? "0")
        var fracPart = s.contains(".") ? String(s.split(separator: ".")[1]) : ""
        while fracPart.count > minDecimals, fracPart.hasSuffix("0") { fracPart.removeLast() }
        if intPart == "0", minInt == 0 { intPart = "" }
        while intPart.count < minInt { intPart = "0" + intPart }
        if useGrouping, intPart.count > 3 {
            var grouped = ""
            for (k, ch) in intPart.reversed().enumerated() {
                if k > 0, k % 3 == 0 { grouped = grouping + grouped }
                grouped = String(ch) + grouped
            }
            intPart = grouped
        }
        s = intPart + (fracPart.isEmpty && !(fracPattern.contains("0")) ? "" : (fracPart.isEmpty ? "" : decimal + fracPart))
        if s.isEmpty { s = "0" }
        let isZero = Double(String(format: "%.\(decimals)f", v)) == 0
        return (negativeSign && !isZero ? "-" : "") + prefix + s + suffix
    }

    // MARK: - Dates

    static let monthNames: [String] = {
        let f = DateFormatter()
        f.locale = Locale(identifier: Language.current == .en ? "en_US" : Language.current.rawValue)
        return f.standaloneMonthSymbols
    }()
    static let shortMonthNames: [String] = {
        let f = DateFormatter()
        f.locale = Locale(identifier: Language.current == .en ? "en_US" : Language.current.rawValue)
        return f.shortStandaloneMonthSymbols.map { $0.replacingOccurrences(of: ".", with: "") }
    }()
    static let dayNames: [String] = {
        let f = DateFormatter()
        f.locale = Locale(identifier: Language.current == .en ? "en_US" : Language.current.rawValue)
        return f.weekdaySymbols
    }()
    static let shortDayNames: [String] = {
        let f = DateFormatter()
        f.locale = Locale(identifier: Language.current == .en ? "en_US" : Language.current.rawValue)
        return f.shortWeekdaySymbols
    }()

    private static func formatDate(_ serial: Double, _ code: String, date1904: Bool) -> String {
        guard serial >= 0, serial < 3_000_000 else { return String(repeating: "#", count: 6) }
        let fractionRange = code.range(of: #"[sS][.,]0+"#, options: .regularExpression)
        let fractionDigits = fractionRange.map { min(9, code[$0].count - 2) } ?? 0
        let scale = pow(10, Double(fractionDigits))
        let totalSeconds = (serial * 86400 * scale).rounded() / scale
        let day = floor(totalSeconds / 86400)
        let secs = Int(floor(totalSeconds - day * 86400))
        let fraction = Int(((totalSeconds - floor(totalSeconds)) * scale).rounded())
        guard let c = DateMath.components(day + (date1904 ? 1462 : 0)) else { return String(repeating: "#", count: 6) }
        let hour = secs / 3600, minute = secs / 60 % 60, second = secs % 60
        let ampm = code.uppercased().contains("AM/PM")

        let lower = Array(code)
        var out = ""
        var i = 0
        var lastWasHour = false
        func run(_ ch: Character) -> Int {
            var j = i
            while j < lower.count, lower[j].lowercased() == String(ch) { j += 1 }
            return j - i
        }
        func upcomingSeconds(from k: Int) -> Bool {
            var j = k
            while j < lower.count, !lower[j].isLetter { j += 1 }
            return j < lower.count && lower[j].lowercased() == "s"
        }
        while i < lower.count {
            let ch = lower[i]
            let l = Character(ch.lowercased())
            switch l {
            case "\"":
                var j = i + 1
                while j < lower.count, lower[j] != "\"" { out.append(lower[j]); j += 1 }
                i = j + 1; continue
            case "\\":
                if i + 1 < lower.count { out.append(lower[i + 1]) }
                i += 2; continue
            case "[":
                // elapsed [h] / [mm] / [ss]
                if let close = lower[i...].firstIndex(of: "]") {
                    let token = String(lower[(i + 1)..<close]).lowercased()
                    if token.hasPrefix("h") { out += String(format: "%0\(token.count)d", Int(totalSeconds) / 3600); lastWasHour = true }
                    else if token.hasPrefix("m") { out += String(format: "%0\(token.count)d", Int(totalSeconds) / 60) }
                    else if token.hasPrefix("s") { out += String(format: "%0\(token.count)d", Int(totalSeconds)) }
                    i = close + 1; continue
                }
            case "y":
                let n = run("y")
                out += n <= 2 ? String(format: "%02d", c.year! % 100) : String(c.year!)
                i += n; continue
            case "m":
                let n = run("m")
                if (lastWasHour || upcomingSeconds(from: i + n)) && n <= 2 {
                    out += n == 2 ? String(format: "%02d", minute) : String(minute)
                } else {
                    let m = c.month!
                    switch n {
                    case 1: out += String(m)
                    case 2: out += String(format: "%02d", m)
                    case 3: out += shortMonthNames[m - 1]
                    case 5: out += String(monthNames[m - 1].prefix(1))
                    default: out += monthNames[m - 1]
                    }
                }
                lastWasHour = false
                i += n; continue
            case "d":
                let n = run("d")
                switch n {
                case 1: out += String(c.day!)
                case 2: out += String(format: "%02d", c.day!)
                case 3: out += shortDayNames[c.weekday! - 1]
                default: out += dayNames[c.weekday! - 1]
                }
                i += n; continue
            case "h":
                let n = run("h")
                var h = hour
                if ampm { h = h % 12; if h == 0 { h = 12 } }
                out += n >= 2 ? String(format: "%02d", h) : String(h)
                lastWasHour = true
                i += n; continue
            case "s":
                let n = run("s")
                out += n >= 2 ? String(format: "%02d", second) : String(second)
                lastWasHour = false
                i += n; continue
            case "a":
                let rest = String(lower[i...]).uppercased()
                if rest.hasPrefix("AM/PM") { out += hour < 12 ? "AM" : "PM"; i += 5; continue }
                if rest.hasPrefix("A/P") { out += hour < 12 ? "A" : "P"; i += 3; continue }
            case "_":
                out += " "; i += 2; continue
            case let punctuation where (punctuation == "." || punctuation == ",") && i > 0 && lower[i - 1].lowercased() == "s":
                var end = i + 1
                while end < lower.count, lower[end] == "0" { end += 1 }
                if end > i + 1 {
                    out += decimal + String(format: "%0*d", end - i - 1, fraction)
                    i = end; continue
                }
            case "@":
                i += 1; continue
            default:
                break
            }
            if l == "." || l == "," { out.append(l == "." && code.contains("ss.") ? Character(decimal) : ch) } else { out.append(ch) }
            if !(l == ":" || l == " ") { lastWasHour = lastWasHour && l == ":" }
            i += 1
        }
        return out
    }
}
