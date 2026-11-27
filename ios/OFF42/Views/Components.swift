import CoreTransferable
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// Bytes handed to the system save dialog (fileExporter).
struct ExportDocument: FileDocument {
    static var readableContentTypes: [UTType] { [.data] }
    let bytes: Data

    init(bytes: Data) { self.bytes = bytes }

    init(configuration: ReadConfiguration) throws {
        bytes = configuration.file.regularFileContents ?? Data()
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: bytes)
    }
}

/// Bytes shared through ShareLink as a file with its own name. The file is written to the app's temporary folder
/// only when a share destination asks for it.
struct ShareFile: Transferable {
    let name: String
    /// Produces the bytes when they are needed (a stamp is drawn at full size only for sharing).
    let make: () throws -> Data

    init(name: String, bytes: Data) {
        self.name = name
        make = { bytes }
    }

    init(name: String, make: @escaping () throws -> Data) {
        self.name = name
        self.make = make
    }

    var ext: String { (name as NSString).pathExtension.lowercased() }

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(exportedContentType: .png) { try $0.write() }.exportingCondition { $0.ext == "png" }
        FileRepresentation(exportedContentType: .pdf) { try $0.write() }.exportingCondition { $0.ext == "pdf" }
        FileRepresentation(exportedContentType: .svg) { try $0.write() }.exportingCondition { $0.ext == "svg" }
        FileRepresentation(exportedContentType: .data) { try $0.write() }
    }

    func write() throws -> SentTransferredFile {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("shared", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let url = folder.appendingPathComponent(Files.safeName(name))
        try make().write(to: url, options: .atomic)
        return SentTransferredFile(url)
    }
}

/// A ShareLink for bytes, labelled "Share".
struct ShareButton: View {
    let item: ShareFile?
    var identifier = "share"

    init(name: String, bytes: Data?, identifier: String = "share") {
        item = bytes.map { ShareFile(name: name, bytes: $0) }
        self.identifier = identifier
    }

    init(item: ShareFile?, identifier: String = "share") {
        self.item = item
        self.identifier = identifier
    }

    var body: some View {
        if let item {
            ShareLink(item: item, preview: SharePreview(item.name)) {
                Label(tr("share"), systemImage: "square.and.arrow.up")
            }
            .accessibilityIdentifier(identifier)
        } else {
            Label(tr("share"), systemImage: "square.and.arrow.up").foregroundStyle(.secondary)
        }
    }
}

/// A PNG from the engine, optionally on a checkerboard that shows transparency.
struct EngineImage: View {
    let png: Data?
    let label: String
    var checker = false
    var identifier: String?

    var body: some View {
        ZStack {
            if checker { Checkerboard() }
            if let png, let image = UIImage(data: png) {
                Image(uiImage: image)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
                    .padding(8)
                    .accessibilityLabel(label)
                    .accessibilityAddTraits(.isImage)
                    .accessibilityIdentifier(identifier ?? "")
            } else {
                ProgressView()
            }
        }
    }
}

/// White and light grey squares, as image editors show transparency (kept light in dark mode: inks are dark).
struct Checkerboard: View {
    var body: some View {
        Canvas { context, size in
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(.white))
            let cell: CGFloat = 8
            var y: CGFloat = 0
            var row = 0
            while y < size.height {
                var x: CGFloat = row % 2 == 0 ? 0 : cell
                while x < size.width {
                    context.fill(Path(CGRect(x: x, y: y, width: cell, height: cell)), with: .color(Color(white: 0.9)))
                    x += cell * 2
                }
                y += cell
                row += 1
            }
        }
        .accessibilityHidden(true)
    }
}

/// A decimal field (millimetres and the like). Invalid text keeps the last valid value.
struct NumberField: View {
    let title: String
    @Binding var value: Double
    var identifier: String?
    @State private var text = ""

    var body: some View {
        LabeledContent(title) {
            TextField(title, text: $text)
                .keyboardType(.decimalPad)
                .multilineTextAlignment(.trailing)
                .accessibilityIdentifier(identifier ?? "")
                .onChange(of: text) { _, new in
                    if let number = Double(new.replacingOccurrences(of: ",", with: ".")) { value = number }
                }
        }
        .onAppear { text = Self.format(value) }
    }

    static func format(_ value: Double) -> String {
        value == value.rounded() ? String(Int(value)) : String(value)
    }
}

/// An engine error in the section's form or detail.
struct ErrorText: View {
    let text: String

    var body: some View {
        Label(text, systemImage: "exclamationmark.triangle")
            .foregroundStyle(.red)
            .font(.callout)
            .accessibilityIdentifier("error")
    }
}

/// A placeholder for an empty detail column: the section's icon, a hint and the privacy note.
struct EmptyDetail: View {
    let section: AppSection
    var text = tr("no_files")
    var action: (() -> Void)?

    var body: some View {
        ContentUnavailableView {
            Label(section.title, systemImage: section.symbol)
        } description: {
            VStack(spacing: 8) {
                Text(text)
                Text(tr("privacy_note")).font(.footnote)
            }
        } actions: {
            if let action {
                Button(tr("open_file"), action: action).buttonStyle(.borderedProminent)
            }
        }
        .tint(section.accent)
    }
}

extension View {
    /// A Done button above the keyboard (number pads and multi-line fields have no Return key that closes them).
    func keyboardDoneButton() -> some View {
        toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()
                Button(tr("done")) {
                    UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
                }
                .accessibilityIdentifier("keyboard-done")
            }
        }
    }
}

/// A done button for sheets.
struct DoneButton: ToolbarContent {
    let action: () -> Void

    var body: some ToolbarContent {
        ToolbarItem(placement: .confirmationAction) {
            Button(tr("done"), action: action).accessibilityIdentifier("done")
        }
    }
}
