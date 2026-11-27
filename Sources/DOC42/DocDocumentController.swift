import AppKit

/// EPUB is a lossy import: its source is never the backing file of an editable document.
final class DocDocumentController: NSDocumentController {
    override func openDocument(withContentsOf url: URL, display displayDocument: Bool,
                               completionHandler: @escaping (NSDocument?, Bool, Error?) -> Void) {
        guard url.pathExtension.lowercased() == "epub" else {
            super.openDocument(withContentsOf: url, display: displayDocument, completionHandler: completionHandler)
            return
        }
        do {
            let document = try makeUntitledDocument(ofType: "org.openxmlformats.wordprocessingml.document")
            guard let text = document as? TextDocument else {
                throw NSError(domain: "DOC42", code: 1, userInfo: [NSLocalizedDescriptionKey: "Could not create a document for EPUB import."])
            }
            try text.read(from: url, ofType: "epub")
            addDocument(text)
            text.updateChangeCount(.changeDone)
            noteNewRecentDocumentURL(url)
            if displayDocument { text.makeWindowControllers(); text.showWindows() }
            completionHandler(text, false, nil)
        } catch { completionHandler(nil, false, error) }
    }
}
