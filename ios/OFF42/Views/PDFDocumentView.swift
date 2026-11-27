import PDFKit
import SwiftUI

/// Pages rendered on the device by PDFKit; the engine's page count, metadata, bookmarks, attachments and form fields
/// in an info sheet; save a copy or share.
struct PDFDocumentView: View {
    @Environment(AppModel.self) private var model
    let file: OpenedFile
    let info: PdfInfo
    @State private var showInfo = false
    @State private var page = 1
    @State private var pageCount = 0

    var body: some View {
        Group {
            if info.encrypted {
                ContentUnavailableView(tr("pdf_encrypted"), systemImage: "lock.doc")
                    .accessibilityIdentifier("pdf-encrypted")
            } else {
                PDFKitView(data: file.bytes, page: $page, pageCount: $pageCount)
                    .ignoresSafeArea(edges: .bottom)
                    .safeAreaInset(edge: .bottom, spacing: 0) {
                        if pageCount > 0 {
                            Text(tr("pdf_page", page, pageCount))
                                .font(.footnote.monospacedDigit())
                                .padding(.horizontal, 12)
                                .padding(.vertical, 6)
                                .background(.regularMaterial, in: Capsule())
                                .padding(.bottom, 12)
                                .accessibilityIdentifier("pdf-page-label")
                        }
                    }
            }
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button {
                    showInfo = true
                } label: {
                    Label(tr("pdf_info"), systemImage: "info.circle")
                }
                .accessibilityIdentifier("pdf-info-button")
                Button {
                    model.output(name: file.name, bytes: file.bytes)
                } label: {
                    Label(tr("save_pdf"), systemImage: "square.and.arrow.down")
                }
                .accessibilityIdentifier("save-pdf")
                ShareButton(name: file.name, bytes: file.bytes)
            }
        }
        .sheet(isPresented: $showInfo) {
            NavigationStack {
                PDFInfoList(info: info)
                    .navigationTitle(tr("pdf_info"))
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar { DoneButton { showInfo = false } }
            }
            .presentationDetents([.medium, .large])
        }
    }
}

struct PDFInfoList: View {
    let info: PdfInfo

    var body: some View {
        List {
            Section {
                LabeledContent(tr("pdf_pages"), value: String(info.pages))
                    .accessibilityIdentifier("pdf-info-pages")
                ForEach(info.metadata, id: \.key) { entry in
                    LabeledContent(entry.key, value: entry.value)
                }
            }
            if !info.bookmarks.isEmpty {
                Section(tr("pdf_bookmarks")) {
                    ForEach(Array(info.bookmarks.enumerated()), id: \.offset) { _, title in Text(title) }
                }
            }
            if !info.attachments.isEmpty {
                Section(tr("pdf_attachments")) {
                    ForEach(Array(info.attachments.enumerated()), id: \.offset) { _, name in Text(name) }
                }
            }
            if !info.formFields.isEmpty {
                Section(tr("pdf_form_fields")) {
                    ForEach(Array(info.formFields.enumerated()), id: \.offset) { _, field in
                        LabeledContent(field.name, value: field.value.isEmpty ? field.kind : field.value)
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .accessibilityIdentifier("pdf-info")
    }
}

/// PDFKit's view: continuous pages on a grey canvas, scaled to the width; reports the current page.
struct PDFKitView: UIViewRepresentable {
    let data: Data
    @Binding var page: Int
    @Binding var pageCount: Int

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> PDFView {
        let view = PDFView()
        view.displayMode = .singlePageContinuous
        view.displayDirection = .vertical
        view.autoScales = true
        view.pageShadowsEnabled = true
        view.backgroundColor = .systemGray5
        view.accessibilityIdentifier = "pdf-view"
        view.document = PDFDocument(data: data)
        context.coordinator.observe(view)
        let count = view.document?.pageCount ?? 0
        DispatchQueue.main.async { pageCount = count }
        return view
    }

    func updateUIView(_ view: PDFView, context: Context) {
        context.coordinator.parent = self
    }

    static func dismantleUIView(_ view: PDFView, coordinator: Coordinator) {
        NotificationCenter.default.removeObserver(coordinator)
    }

    final class Coordinator: NSObject {
        var parent: PDFKitView

        init(_ parent: PDFKitView) { self.parent = parent }

        func observe(_ view: PDFView) {
            NotificationCenter.default.addObserver(self, selector: #selector(pageChanged(_:)), name: .PDFViewPageChanged, object: view)
        }

        @objc func pageChanged(_ notification: Notification) {
            guard let view = notification.object as? PDFView, let page = view.currentPage,
                  let index = view.document?.index(for: page) else { return }
            parent.page = index + 1
        }
    }
}
