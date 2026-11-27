import AppKit

/// A Keynote file is an import source, never an editable document's backing file.
final class PresentationDocumentController: NSDocumentController {
    override func openDocument(withContentsOf url: URL, display displayDocument: Bool,
                               completionHandler: @escaping (NSDocument?, Bool, Error?) -> Void) {
        guard url.pathExtension.lowercased() == "key" else {
            super.openDocument(withContentsOf: url, display: displayDocument, completionHandler: completionHandler)
            return
        }
        do {
            guard let document = try makeUntitledDocument(ofType: "org.openxmlformats.presentationml.presentation") as? PresentationDocument else {
                throw NSError(domain: "PPT42", code: 1, userInfo: [NSLocalizedDescriptionKey: "Could not create a presentation for Keynote import."])
            }
            try document.read(from: url, ofType: "key")
            addDocument(document)
            document.updateChangeCount(.changeDone)
            noteNewRecentDocumentURL(url)
            if displayDocument { document.makeWindowControllers(); document.showWindows() }
            completionHandler(document, false, nil)
        } catch { completionHandler(nil, false, error) }
    }
}
