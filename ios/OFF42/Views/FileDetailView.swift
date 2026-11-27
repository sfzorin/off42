import SwiftUI
import WebKit

/// The open file: its viewer, with Save as / Share in the toolbar.
struct FileDetailView: View {
    let file: OpenedFile

    var body: some View {
        Group {
            switch file.content {
            case let .document(html, info)?: DocumentView(file: file, html: html, info: info)
            case let .spreadsheet(workbook)?: SpreadsheetView(file: file, workbook: workbook)
            case let .presentation(slides)?: PresentationView(file: file, slides: slides)
            case let .pdf(info)?: PDFDocumentView(file: file, info: info)
            case nil:
                ScrollView {
                    ErrorText(text: file.error ?? "").padding()
                }
            }
        }
        .navigationTitle(file.name)
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// "Save as" with the formats a file converts to, plus optional extra actions.
struct SaveMenu<Extra: View>: View {
    @Environment(AppModel.self) private var model
    let file: OpenedFile
    let formats: [String]
    @ViewBuilder var extra: () -> Extra

    var body: some View {
        Menu {
            ForEach(formats, id: \.self) { format in
                Button(format.uppercased()) {
                    Task { await model.saveAs(file, to: format) }
                }
                .accessibilityIdentifier("save-\(format)")
            }
            extra()
        } label: {
            Label(tr("save_as"), systemImage: "square.and.arrow.down")
        }
        .accessibilityIdentifier("save-menu")
    }
}

extension SaveMenu where Extra == EmptyView {
    init(file: OpenedFile, formats: [String]) {
        self.init(file: file, formats: formats) { EmptyView() }
    }
}

// MARK: - Documents

/// The engine's HTML as a white page on a grey canvas, with the document's statistics.
struct DocumentView: View {
    let file: OpenedFile
    let html: String
    let info: DocumentInfo

    var body: some View {
        WebPage(html: DocumentPage.html(html), label: tr("doc_preview"))
            .background(Color(.systemGray5))
            .safeAreaInset(edge: .bottom, spacing: 0) {
                Text(tr("doc_stats", String(info.words), String(info.characters), String(info.paragraphs)))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 8)
                    .padding(.horizontal)
                    .background(.bar)
                    .accessibilityIdentifier("doc-stats")
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    SaveMenu(file: file, formats: saveFormats(kind: file.kind))
                }
            }
    }
}

/// A WKWebView that runs no script and loads nothing: JavaScript off, a non-persistent data store, every
/// navigation other than the initial HTML cancelled, and the page's Content-Security-Policy (DocumentPage) blocking
/// network, frames and scripts.
struct WebPage: UIViewRepresentable {
    let html: String
    let label: String

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = false
        configuration.websiteDataStore = .nonPersistent()
        configuration.dataDetectorTypes = []
        let view = WKWebView(frame: .zero, configuration: configuration)
        view.isOpaque = false
        view.backgroundColor = .clear
        view.scrollView.backgroundColor = .clear
        view.navigationDelegate = context.coordinator
        view.accessibilityIdentifier = "doc-preview"
        view.accessibilityHint = label
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {
        guard context.coordinator.loaded != html else { return }
        context.coordinator.loaded = html
        view.loadHTMLString(html, baseURL: nil)
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        var loaded: String?

        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                     decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
            let scheme = action.request.url?.scheme ?? "about"
            decisionHandler(action.navigationType == .other && scheme == "about" ? .allow : .cancel)
        }
    }
}

// MARK: - Spreadsheets

/// The sheet's displayed values in a grid, with the sheet tabs at the bottom.
struct SpreadsheetView: View {
    @Environment(AppModel.self) private var model
    let file: OpenedFile
    let workbook: Workbook
    @State private var current = 0
    @State private var grid: [[String]] = []

    var body: some View {
        let sheet = workbook.sheets.indices.contains(current) ? workbook.sheets[current] : nil
        VStack(spacing: 0) {
            if workbook.readOnly || sheet?.truncated == true {
                VStack(alignment: .leading, spacing: 4) {
                    if workbook.readOnly { Text(tr("sheet_read_only")) }
                    if sheet?.truncated == true { Text(tr("sheet_truncated", Engine.maxRows, Engine.maxCols)) }
                }
                .font(.footnote)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal)
                .padding(.vertical, 8)
                .background(Color(.secondarySystemBackground))
                .accessibilityIdentifier("sheet-note")
            }
            SheetGrid(grid: grid).id(current)
            SheetTabs(names: workbook.sheets.map(\.name), current: $current)
        }
        .task(id: current) {
            if let sheet { grid = Sheets.grid(sheet) }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                // Read-only workbooks (XLS) export their values as CSV only, as on Android and the web.
                SaveMenu(file: file, formats: workbook.readOnly ? [] : saveFormats(kind: file.kind)) {
                    Divider()
                    Button(tr("sheet_values_csv")) {
                        Task { await model.saveSheetValues(file, sheet: current, sheetName: sheet?.name ?? "") }
                    }
                    .accessibilityIdentifier("values-csv")
                }
            }
        }
    }
}

/// Rows are lazy; the column letters stay pinned at the top while scrolling.
struct SheetGrid: View {
    let grid: [[String]]
    @ScaledMetric(relativeTo: .footnote) private var rowHeight: CGFloat = 30
    @ScaledMetric(relativeTo: .footnote) private var cellWidth: CGFloat = 104
    @ScaledMetric(relativeTo: .footnote) private var headerWidth: CGFloat = 46

    var body: some View {
        let columns = grid.first?.count ?? 0
        ScrollView([.horizontal, .vertical]) {
            LazyVStack(alignment: .leading, spacing: 0, pinnedViews: [.sectionHeaders]) {
                Section {
                    ForEach(grid.indices, id: \.self) { row in
                        HStack(spacing: 0) {
                            header(String(row + 1), width: headerWidth)
                            ForEach(0..<columns, id: \.self) { column in
                                cell(grid[row][column], row: row, column: column)
                            }
                        }
                    }
                } header: {
                    HStack(spacing: 0) {
                        header("", width: headerWidth)
                        ForEach(0..<columns, id: \.self) { column in
                            header(Sheets.columnName(column), width: cellWidth)
                        }
                    }
                }
            }
        }
        .defaultScrollAnchor(.topLeading)
        .background(Color(.systemBackground))
        .accessibilityIdentifier("sheet-grid")
    }

    private func header(_ text: String, width: CGFloat) -> some View {
        Text(text)
            .font(.footnote.weight(.semibold))
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .frame(width: width, height: rowHeight)
            .background(Color(.secondarySystemBackground))
            .overlay(Rectangle().stroke(Color(.separator), lineWidth: 0.5))
            .accessibilityHidden(true)
    }

    private func cell(_ text: String, row: Int, column: Int) -> some View {
        Text(text)
            .font(.footnote)
            .lineLimit(1)
            .padding(.horizontal, 6)
            .frame(width: cellWidth, height: rowHeight, alignment: Sheets.looksNumeric(text) ? .trailing : .leading)
            .overlay(Rectangle().stroke(Color(.separator).opacity(0.6), lineWidth: 0.5))
            .accessibilityLabel("\(Sheets.columnName(column))\(row + 1)")
            .accessibilityValue(text)
            .accessibilityIdentifier("cell-\(row)-\(column)")
            .accessibilityHidden(text.isEmpty)
    }
}

/// Sheet tabs along the bottom edge, as in desktop spreadsheets.
struct SheetTabs: View {
    let names: [String]
    @Binding var current: Int

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                ForEach(names.indices, id: \.self) { index in
                    let selected = index == current
                    Button {
                        current = index
                    } label: {
                        Text(names[index])
                            .font(.subheadline.weight(selected ? .semibold : .regular))
                            .padding(.horizontal, 14)
                            .padding(.vertical, 7)
                            .background(selected ? Color(.systemBackground) : Color.clear,
                                        in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                            .foregroundStyle(selected ? AnyShapeStyle(.tint) : AnyShapeStyle(.primary))
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(selected ? .isSelected : [])
                    .accessibilityIdentifier("sheet-tab-\(index)")
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
        }
        .background(Color(.secondarySystemBackground))
        .overlay(alignment: .top) { Divider() }
    }
}
