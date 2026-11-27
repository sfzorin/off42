import PDFKit
import SwiftUI

let codeFormats = ["qr", "datamatrix", "code128", "aztec"]

func codeFormatTitle(_ format: String?) -> String {
    switch format {
    case "qr": return tr("code_qr")
    case "datamatrix": return "Data Matrix"
    case "code128": return "Code 128"
    case "aztec": return "Aztec"
    default: return tr("labels_none")
    }
}

// MARK: - Codes

/// QR, Data Matrix, Code 128 and Aztec with a live preview; save PNG or SVG, share.
struct CodesView: View {
    @Environment(AppModel.self) private var model
    @State private var format = "qr"
    @State private var content = "https://off42.cc"
    @State private var width = 30.0
    @State private var caption = false
    @State private var png: Data?
    @State private var error: String?
    @FocusState private var editing: Bool

    var body: some View {
        Form {
            Section {
                Group {
                    if let error {
                        ErrorText(text: error)
                    } else {
                        EngineImage(png: png, label: tr("code_preview"), identifier: "code-preview")
                    }
                }
                .frame(maxWidth: .infinity, minHeight: 220, maxHeight: 260)
                .background(Color.white, in: RoundedRectangle(cornerRadius: 10))
                .listRowInsets(EdgeInsets(top: 8, leading: 8, bottom: 8, trailing: 8))
            }
            Section {
                Picker(tr("code_type"), selection: $format) {
                    ForEach(codeFormats, id: \.self) { Text(codeFormatTitle($0)).tag($0) }
                }
                TextField(tr("code_content"), text: $content, axis: .vertical)
                    .lineLimit(1...4)
                    .focused($editing)
                    .accessibilityIdentifier("code-value")
                NumberField(title: tr("width_mm"), value: $width)
                Toggle(tr("code_caption"), isOn: $caption)
            }
            Section {
                Button(tr("save_png")) {
                    editing = false
                    if let png { model.output(name: "\(format).png", bytes: png) }
                }
                .disabled(png == nil)
                .accessibilityIdentifier("code-save-png")
                Button(tr("save_svg")) {
                    editing = false
                    Task { await saveSVG() }
                }
                .disabled(png == nil)
                .accessibilityIdentifier("code-save-svg")
                ShareButton(name: "\(format).png", bytes: png)
            }
        }
        .navigationTitle(tr("tab_codes"))
        .keyboardDoneButton()
        .navigationBarTitleDisplayMode(.inline)
        .scrollDismissesKeyboard(.interactively)
        .task(id: "\(format)|\(content)|\(width)") {
            // Redrawn shortly after the last change.
            try? await Task.sleep(for: .milliseconds(200))
            guard !Task.isCancelled else { return }
            let (format, content, width) = (format, content, min(max(width, 5), 200))
            do {
                png = try await Engine.run { try barcodePng(format: format, value: content, widthMm: width) }
                error = nil
            } catch {
                png = nil
                self.error = error.engineMessage
            }
        }
    }

    private func saveSVG() async {
        let (format, content, width, caption) = (format, content, min(max(width, 5), 200), caption)
        do {
            let svg = try await Engine.run { try barcodeSvg(format: format, value: content, widthMm: width, caption: caption) }
            model.output(name: "\(format).svg", bytes: Data(svg.utf8))
        } catch {
            model.notify(error.engineMessage, error: true)
        }
    }
}

// MARK: - Label sheets

private let customStock = "custom"

func stockTitle(_ stock: LabelStock) -> String {
    "\(stock.name) — \(stock.columns)×\(stock.rows), \(NumberField.format(stock.width))×\(NumberField.format(stock.height)) mm"
}

/// A4 label sheets from `text | code | copies` lines on a built-in stock or a custom size.
struct LabelSheetView: View {
    @Environment(AppModel.self) private var model
    @State private var stocks: [LabelStock] = []
    @State private var stockID: String?
    @State private var customWidth = 70.0
    @State private var customHeight = 37.0
    @State private var gapX = 0.0
    @State private var gapY = 0.0
    @State private var first = 1.0
    @State private var codeFormat: String? = "qr"
    @State private var lines = "Box 1 | BOX-1 | 3\nBox 2 | BOX-2\nShelf A"
    @State private var pdf: Data?
    @State private var preview: UIImage?
    @State private var status = ""
    @State private var error: String?
    @FocusState private var editing: Bool

    private var stock: LabelStock? { stocks.first { $0.id == stockID } ?? stocks.first }

    var body: some View {
        Form {
            Section {
                NavigationLink {
                    StockPicker(stocks: stocks, selection: $stockID)
                } label: {
                    LabeledContent(tr("labels_stock")) {
                        Text(stockID == customStock ? tr("labels_custom") : stock.map(stockTitle) ?? "…")
                            .multilineTextAlignment(.trailing)
                    }
                }
                .accessibilityIdentifier("stock-picker")
                if stockID == customStock {
                    NumberField(title: tr("width_mm"), value: $customWidth)
                    NumberField(title: tr("height_mm"), value: $customHeight)
                    NumberField(title: tr("labels_gap_x"), value: $gapX)
                    NumberField(title: tr("labels_gap_y"), value: $gapY)
                }
                NumberField(title: tr("labels_first"), value: $first)
                Picker(tr("labels_code"), selection: $codeFormat) {
                    ForEach([nil] + codeFormats.map(Optional.some), id: \.self) { Text(codeFormatTitle($0)).tag($0) }
                }
            }
            Section {
                TextEditor(text: $lines)
                    .font(.body.monospaced())
                    .frame(minHeight: 120)
                    .focused($editing)
                    .accessibilityLabel(tr("labels_lines"))
                    .accessibilityIdentifier("label-lines")
            } header: {
                Text(tr("labels_lines"))
            } footer: {
                Text(tr("labels_lines_format"))
            }
            Section {
                Button(tr("labels_make")) {
                    editing = false
                    Task { await make() }
                }
                .accessibilityIdentifier("make-sheet")
                if let error { ErrorText(text: error) }
            }
            if let pdf {
                Section {
                    Text(status).accessibilityIdentifier("sheet-status")
                    if let preview {
                        Image(uiImage: preview)
                            .resizable()
                            .scaledToFit()
                            .frame(maxWidth: 420, maxHeight: 520)
                            .frame(maxWidth: .infinity)
                            .shadow(radius: 2)
                            .accessibilityLabel(tr("pdf_page", 1, PDFDocument(data: pdf)?.pageCount ?? 1))
                            .accessibilityIdentifier("sheet-preview")
                    }
                    Button(tr("save_pdf")) { model.output(name: "labels.pdf", bytes: pdf) }
                        .accessibilityIdentifier("sheet-save")
                    ShareButton(name: "labels.pdf", bytes: pdf)
                }
            }
        }
        .navigationTitle(tr("labels_title"))
        .keyboardDoneButton()
        .navigationBarTitleDisplayMode(.inline)
        .scrollDismissesKeyboard(.interactively)
        .task {
            if stocks.isEmpty { stocks = (try? await Engine.run { try labelStocks() }) ?? [] }
        }
    }

    private func make() async {
        error = nil
        let labels = parseLabelLines(text: lines)
        let request = LabelSheetRequest(
            stock: stockID == customStock ? customStock : stock?.id ?? "",
            width: customWidth, height: customHeight, gapX: gapX, gapY: gapY,
            firstSlot: UInt32(max(first, 0)), codeFormat: codeFormat, labels: labels)
        do {
            let (bytes, pages) = try await Engine.run { () -> (Data, UInt32) in
                let bytes = try labelSheetPdf(request: request)
                return (bytes, try pdfInfo(bytes: bytes).pages)
            }
            pdf = bytes
            status = tr("labels_status", labels.reduce(0) { $0 + Int($1.copies) }, Int(pages))
            preview = PDFDocument(data: bytes)?.page(at: 0)?.thumbnail(of: CGSize(width: 840, height: 1188), for: .mediaBox)
        } catch {
            pdf = nil
            preview = nil
            self.error = error.engineMessage
        }
    }
}

/// The built-in stocks, searchable, and "Custom size…".
private struct StockPicker: View {
    let stocks: [LabelStock]
    @Binding var selection: String?
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    var body: some View {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        List {
            Button(tr("labels_custom")) { pick(customStock) }
            Section {
                ForEach(stocks.filter { q.isEmpty || stockTitle($0).lowercased().contains(q) }, id: \.id) { stock in
                    Button {
                        pick(stock.id)
                    } label: {
                        HStack {
                            Text(stockTitle(stock)).foregroundStyle(Color.primary)
                            Spacer()
                            if stock.id == selection { Image(systemName: "checkmark").foregroundStyle(.tint) }
                        }
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: tr("marks_search"))
        .navigationTitle(tr("labels_stock"))
        .navigationBarTitleDisplayMode(.inline)
    }

    private func pick(_ id: String) {
        selection = id
        dismiss()
    }
}

// MARK: - Product marks

/// Thumbnails of the marks, drawn once per session.
@MainActor
private final class MarkThumbnails {
    static let shared = MarkThumbnails()
    private var cache: [String: Data] = [:]

    func png(_ id: String) async -> Data? {
        if let data = cache[id] { return data }
        let data = try? await Engine.run { try productMarkPng(id: id, widthMm: 12) }
        cache[id] = data
        return data
    }
}

/// The product mark catalogue with search, category filter and a detail sheet (purpose, warnings, licence notes);
/// save SVG or PNG at any width.
struct MarksView: View {
    @Environment(AppModel.self) private var model
    @State private var marks: [ProductMark] = []
    @State private var query = ""
    @State private var category: String?
    @State private var detail: ProductMark?
    @State private var afterDismiss: (name: String, bytes: Data)?
    @ScaledMetric private var cellWidth: CGFloat = 104

    var body: some View {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        let shown = marks.filter {
            (category == nil || $0.category == category)
                && (q.isEmpty || "\($0.name) \($0.purpose) \($0.id)".lowercased().contains(q))
        }
        ScrollView {
            Text(tr("marks_hint"))
                .font(.footnote)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal)
                .padding(.top, 8)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: cellWidth), spacing: 10)], spacing: 10) {
                ForEach(shown, id: \.id) { mark in
                    Button {
                        detail = mark
                    } label: {
                        MarkCell(mark: mark)
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("mark-\(mark.id)")
                }
            }
            .padding()
        }
        .background(Color(.systemGroupedBackground))
        .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: tr("marks_search_placeholder"))
        .navigationTitle(tr("marks_title"))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Picker(tr("marks_category"), selection: $category) {
                        Text(tr("marks_all")).tag(String?.none)
                        ForEach(Array(Set(marks.map(\.category))).sorted(), id: \.self) { Text($0).tag(Optional($0)) }
                    }
                } label: {
                    Label(tr("marks_category"), systemImage: category == nil ? "line.3.horizontal.decrease.circle" : "line.3.horizontal.decrease.circle.fill")
                }
                .accessibilityIdentifier("marks-category")
            }
        }
        .sheet(item: Binding(get: { detail.map(IdentifiedMark.init) }, set: { detail = $0?.mark }), onDismiss: {
            // The save dialog is presented once the sheet is gone.
            if let pending = afterDismiss {
                afterDismiss = nil
                model.output(name: pending.name, bytes: pending.bytes)
            }
        }) { item in
            MarkDetail(mark: item.mark) { name, bytes in
                afterDismiss = (name, bytes)
                detail = nil
            }
        }
        .task {
            let language = siteLanguage()
            if marks.isEmpty { marks = (try? await Engine.run { try productMarks(language: language) }) ?? [] }
        }
    }
}

private struct IdentifiedMark: Identifiable {
    let mark: ProductMark
    var id: String { mark.id }
}

private struct MarkCell: View {
    let mark: ProductMark
    @State private var png: Data?

    var body: some View {
        VStack(spacing: 6) {
            EngineImage(png: png, label: mark.name)
                .frame(width: 72, height: 72)
                .background(Color.white, in: RoundedRectangle(cornerRadius: 8))
            Text(mark.name)
                .font(.caption)
                .multilineTextAlignment(.center)
                .lineLimit(2)
                .frame(maxWidth: .infinity)
        }
        .padding(8)
        .frame(maxWidth: .infinity, minHeight: 130, alignment: .top)
        .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 12))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(mark.name)
        .accessibilityAddTraits(.isButton)
        .task(id: mark.id) { png = await MarkThumbnails.shared.png(mark.id) }
    }
}

private struct MarkDetail: View {
    @Environment(AppModel.self) private var model
    let mark: ProductMark
    let save: (String, Data) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var width = 20.0
    @State private var preview: Data?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    EngineImage(png: preview, label: mark.name)
                        .frame(maxWidth: .infinity, minHeight: 160, maxHeight: 220)
                        .background(Color.white, in: RoundedRectangle(cornerRadius: 10))
                        .listRowInsets(EdgeInsets(top: 8, leading: 8, bottom: 8, trailing: 8))
                }
                Section {
                    Text(mark.purpose)
                    if let warning = mark.warning { Text(warning).foregroundStyle(.red) }
                    if mark.unverified { Text(tr("marks_unverified")).foregroundStyle(.red) }
                    if mark.needsNumber { Text(tr("marks_needs_number")) }
                }
                Section {
                    NumberField(title: tr("width_mm"), value: $width)
                    Button(tr("save_png")) { Task { await export(svg: false) } }
                        .accessibilityIdentifier("mark-save-png")
                    Button(tr("save_svg")) { Task { await export(svg: true) } }
                        .accessibilityIdentifier("mark-save-svg")
                }
            }
            .navigationTitle(mark.name)
            .keyboardDoneButton()
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { DoneButton { dismiss() } }
        }
        .task {
            let id = mark.id
            preview = try? await Engine.run { try productMarkPng(id: id, widthMm: 20) }
        }
    }

    private func export(svg: Bool) async {
        let (id, width) = (mark.id, min(max(width, 3), 300))
        do {
            if svg {
                let text = try await Engine.run { try productMarkSvg(id: id, widthMm: width) }
                save("\(id).svg", Data(text.utf8))
            } else {
                save("\(id).png", try await Engine.run { try productMarkPng(id: id, widthMm: width) })
            }
        } catch {
            model.notify(error.engineMessage, error: true)
        }
    }
}
