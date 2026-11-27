import Observation
import SwiftUI
import UniformTypeIdentifiers

/// Bytes waiting for the user to pick a destination in the system save dialog.
struct PendingExport: Identifiable {
    let id = UUID()
    let name: String
    let bytes: Data
    var ext: String { (name as NSString).pathExtension.lowercased() }
}

/// A document conversion that would drop package parts, waiting for the user's consent.
struct ConversionQuestion: Identifiable {
    let id = UUID()
    let fileID: Int
    let format: String
    let message: String
}

struct Notice: Identifiable, Equatable {
    let id = UUID()
    let text: String
    let isError: Bool
}

/// The app's state: open files, navigation, pending saves and messages. Engine work runs off the main thread;
/// everything here is changed on the main actor. Open files live only in memory for the session.
@MainActor
@Observable
final class AppModel {
    var section: AppSection?
    var files: [OpenedFile] = []
    var selectedFile: [AppSection: Int] = [:]
    var selectedTool: [AppSection: Tool] = [:]
    var columnVisibility: NavigationSplitViewVisibility = .all
    var compactColumn: NavigationSplitViewColumn = .sidebar
    var busy = 0
    var notice: Notice?
    var question: ConversionQuestion?
    var pendingExport: PendingExport?
    var importing: AppSection?

    private var nextID = 1

    /// UI tests write saved files here instead of showing the system save dialog (OFF42_UITEST_EXPORT_DIR).
    let testExportDirectory: URL?

    init(environment: [String: String] = ProcessInfo.processInfo.environment) {
        testExportDirectory = environment["OFF42_UITEST_EXPORT_DIR"].map { URL(fileURLWithPath: $0, isDirectory: true) }
    }

    func files(of section: AppSection) -> [OpenedFile] {
        files.filter { AppSection.of($0.kind) == section }
    }

    func file(in section: AppSection) -> OpenedFile? {
        guard let id = selectedFile[section] else { return nil }
        return files.first { $0.id == id }
    }

    func close(_ file: OpenedFile) {
        files.removeAll { $0.id == file.id }
        let section = AppSection.of(file.kind)
        if selectedFile[section] == file.id { selectedFile[section] = nil }
    }

    func notify(_ text: String, error: Bool = false) {
        notice = Notice(text: text, isError: error)
    }

    func show(_ section: AppSection, tool: Tool? = nil) {
        self.section = section
        if let tool { selectedTool[section] = tool }
        compactColumn = tool != nil ? .detail : .content
    }

    // MARK: - Opening

    /// Opens a file from the document picker, "Open in"/share (onOpenURL) or a UI test.
    func open(url: URL) async {
        busy += 1
        defer { busy -= 1 }
        do {
            let (name, type, bytes) = try await Engine.run { () -> (String, UTType?, Data) in
                let scoped = url.startAccessingSecurityScopedResource()
                defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                let values = try? url.resourceValues(forKeys: [.contentTypeKey, .localizedNameKey])
                let data = try Data(contentsOf: url)
                return (url.lastPathComponent.isEmpty ? (values?.localizedName ?? "file") : url.lastPathComponent,
                        values?.contentType, data)
            }
            removeInboxCopy(url)
            await openBytes(name: name, type: type, bytes: bytes)
        } catch {
            notify(tr("file_unreadable") + " " + error.localizedDescription, error: true)
        }
    }

    /// "Open in" from another app copies the file into Documents/Inbox; the bytes are kept in memory only.
    private func removeInboxCopy(_ url: URL) {
        let inbox = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first?
            .appendingPathComponent("Inbox", isDirectory: true).standardizedFileURL.path
        if let inbox, url.standardizedFileURL.path.hasPrefix(inbox + "/") {
            try? FileManager.default.removeItem(at: url)
        }
    }

    @discardableResult
    func openBytes(name: String, type: UTType?, bytes: Data) async -> OpenedFile? {
        let ext = Files.engineExtension(name: name, type: type)
        guard let kind = fileKind(name: ext) else {
            notify(tr("file_unsupported", ext), error: true)
            return nil
        }
        busy += 1
        defer { busy -= 1 }
        let id = nextID
        nextID += 1
        guard let file = try? await Engine.run({ OpenedFile.load(id: id, name: name, ext: ext, kind: kind, bytes: bytes) }) else {
            return nil
        }
        files.append(file)
        let section = AppSection.of(kind)
        selectedFile[section] = id
        self.section = section
        compactColumn = .detail
        return file
    }

    // MARK: - Saving

    /// Converts an opened file and asks for a destination.
    func saveAs(_ file: OpenedFile, to format: String, dropPackageParts: Bool = false) async {
        busy += 1
        defer { busy -= 1 }
        do {
            let bytes = try await Engine.run { () -> Data in
                switch file.kind {
                case .document:
                    return try documentConvert(bytes: file.bytes, from: file.ext, to: format, dropPackageParts: dropPackageParts)
                case .spreadsheet: return try spreadsheetConvert(bytes: file.bytes, from: file.ext, to: format)
                case .presentation: return try presentationConvert(bytes: file.bytes, from: file.ext, to: format)
                case .pdf: return file.bytes
                }
            }
            output(name: "\(file.stem).\(format)", bytes: bytes)
        } catch let Off42Error.PackagePartsWouldBeLost(message) {
            question = ConversionQuestion(fileID: file.id, format: format, message: message)
        } catch {
            notify(error.engineMessage, error: true)
        }
    }

    func answer(_ question: ConversionQuestion, convert: Bool) async {
        self.question = nil
        guard convert, let file = files.first(where: { $0.id == question.fileID }) else { return }
        await saveAs(file, to: question.format, dropPackageParts: true)
    }

    func saveSheetValues(_ file: OpenedFile, sheet: Int, sheetName: String) async {
        busy += 1
        defer { busy -= 1 }
        do {
            let bytes = try await Engine.run { try spreadsheetValuesCsv(bytes: file.bytes, from: file.ext, sheet: UInt32(sheet)) }
            output(name: "\(file.stem) - \(sheetName).csv", bytes: bytes)
        } catch {
            notify(error.engineMessage, error: true)
        }
    }

    /// Asks for a destination for bytes (the system save dialog through fileExporter).
    func output(name: String, bytes: Data) {
        let name = Files.safeName(name)
        if let directory = testExportDirectory {
            do {
                try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
                try bytes.write(to: directory.appendingPathComponent(name), options: .atomic)
                notify(tr("saved", name))
            } catch {
                notify(tr("file_unwritable") + " " + error.localizedDescription, error: true)
            }
            return
        }
        pendingExport = PendingExport(name: name, bytes: bytes)
    }

    func exportFinished(_ result: Result<URL, Error>, name: String) {
        switch result {
        case .success: notify(tr("saved", name))
        case let .failure(error):
            if (error as NSError).code != NSUserCancelledError { notify(error.localizedDescription, error: true) }
        }
    }
}
