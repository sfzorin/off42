import AppKit
import Kit42
import PDFKit
import UniformTypeIdentifiers

/// Embedded files: the document's /EmbeddedFiles and files attached to pages.
extension EditorWindowController {
    /// Changes the document-level parts (properties, labels, view, files) undoably.
    func setExtras(_ change: (inout DocumentExtras) -> Void, part: DocumentExtras.Part, name: String) {
        guard let doc = pdfDoc else { return }
        let old = doc.extras
        var new = old
        change(&new)
        new.dirty.insert(part)
        replaceExtras(with: new, name: name)
        _ = old
    }

    private func replaceExtras(with new: DocumentExtras, name: String) {
        guard let doc = pdfDoc else { return }
        let old = doc.extras
        doc.extras = new
        extrasChanged()
        undo?.registerUndo(withTarget: self) { $0.replaceExtras(with: old, name: name) }
        undo?.setActionName(name)
    }

    /// Refreshes everything that shows labels, attachments or bookmark styles.
    func extrasChanged() {
        sidebar.reloadAttachments()
        sidebar.reloadOutline()
        sidebar.thumbnails.reloadLabels()
        updateSubtitleNow()
    }

    func attachmentInfo(_ row: SidebarViewController.AttachmentRow) -> AttachmentInfo? {
        guard let doc = pdfDoc else { return nil }
        let size = { (n: Int) in ByteCountFormatter.string(fromByteCount: Int64(n), countStyle: .file) }
        switch row {
        case .document(let i):
            guard i >= 0, i < doc.extras.files.count else { return nil }
            let f = doc.extras.files[i]
            var detail = size(f.data.count)
            if !f.desc.isEmpty { detail += " · " + f.desc }
            return AttachmentInfo(name: f.name, detail: detail, data: f.data)
        case .annotation(let a, let page):
            let file = doc.extras.annotationFile(for: a, on: page)
            let name = file?.name ?? a.contents ?? L("Attachment")
            return AttachmentInfo(name: name, detail: L("Page %@", doc.extras.displayLabel(page)) + (file.map { " · " + size($0.data.count) } ?? ""),
                                  data: file?.data ?? Data())
        }
    }

    @objc func showAttachmentsAction(_ sender: Any?) { showSidebarMode(.attachments) }

    @objc func addAttachmentAction(_ sender: Any?) {
        guard let window else { return }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.message = L("Choose files to embed in the PDF")
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK else { return }
            self?.addAttachments(panel.urls)
        }
    }

    func addAttachments(_ urls: [URL]) {
        let files: [EmbeddedFile] = urls.compactMap { url in
            guard let data = try? Data(contentsOf: url) else { return nil }
            let date = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
            return EmbeddedFile(name: url.lastPathComponent, data: data, modified: date, mime: Writer.mime(for: url.lastPathComponent))
        }
        guard !files.isEmpty else { NSSound.beep(); return }
        setExtras({ x in
            for f in files {
                x.files.removeAll { $0.name == f.name }
                x.files.append(f)
            }
        }, part: .files, name: L("Attach File"))
        showSidebarMode(.attachments)
    }

    @objc func deleteAttachmentAction(_ sender: Any?) {
        guard let row = sidebar.selectedAttachment else { return }
        switch row {
        case .document(let i):
            setExtras({ x in if i >= 0, i < x.files.count { x.files.remove(at: i) } }, part: .files, name: L("Delete Attachment"))
        case .annotation(let a, let page):
            guard let p = pdf?.page(at: page) else { return }
            remove(a, from: p, name: L("Delete Attachment"))
            sidebar.reloadAttachments()
        }
    }

    @objc func saveAttachmentAction(_ sender: Any?) {
        guard let window, let row = sidebar.selectedAttachment, let info = attachmentInfo(row) else { return }
        let panel = NSSavePanel()
        panel.nameFieldStringValue = AttachmentExport.fileName(info.name)
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            do { try info.data.write(to: url, options: .atomic) } catch { self?.presentError(error) }
        }
    }

    /// Opens a copy of the attachment (in a temporary folder) with its default app.
    @objc func openAttachmentAction(_ sender: Any?) {
        guard let row = sidebar.selectedAttachment, let info = attachmentInfo(row) else { return }
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("PDF42 Attachments/\(UUID().uuidString)")
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let url = try AttachmentExport.write(info.data, name: info.name, to: dir)
            NSWorkspace.shared.open(url)
        } catch { presentError(error) }
    }

    var hasAttachmentSelection: Bool { sidebar.mode == .attachments && sidebar.selectedAttachment != nil }
}
