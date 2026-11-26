import SwiftUI

private func formTitle(_ value: UInt8) -> String { tr("sig_form_\(min(value, 5))") }
private func penTitle(_ value: UInt8) -> String { tr("sig_pen_\(min(value, 3))") }

func inkTitle(_ ink: String) -> String {
    ["blue", "violet", "black", "red", "green"].contains(ink) ? tr("ink_\(ink)") : tr("stamp_default_ink")
}

// MARK: - Signatures

private struct Variant: Identifiable {
    let id = UUID()
    let design: UInt64
    var impression: UInt64
}

/// Six signature variants per name, form, pen and ink; each with a new impression, save and share as transparent PNG.
struct SignatureView: View {
    @State private var options: SignatureOptions?
    @State private var name = ""
    @State private var form: UInt8 = 0
    @State private var pen: UInt8 = 0
    @State private var ink = "blue"
    @State private var variants: [Variant] = []
    @State private var error: String?
    @FocusState private var editing: Bool

    var body: some View {
        Form {
            Section {
                TextField(tr("sig_name"), text: $name)
                    .textContentType(.name)
                    .focused($editing)
                    .submitLabel(.done)
                    .accessibilityIdentifier("sig-name")
                if let options {
                    Picker(tr("sig_form"), selection: $form) {
                        ForEach(options.forms, id: \.value) { Text(formTitle($0.value)).tag($0.value) }
                    }
                    Picker(tr("sig_pen"), selection: $pen) {
                        ForEach(options.pens, id: \.value) { Text(penTitle($0.value)).tag($0.value) }
                    }
                    Picker(tr("sig_ink"), selection: $ink) {
                        ForEach(options.inks, id: \.value) { Text(inkTitle($0.value)).tag($0.value) }
                    }
                }
                Button(tr("sig_generate")) {
                    editing = false
                    Task { await generate() }
                }
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                .accessibilityIdentifier("sig-generate")
                if let error { ErrorText(text: error) }
            }
            ForEach(Array(variants.enumerated()), id: \.element.id) { index, variant in
                Section {
                    SignatureVariantRow(name: name, design: variant.design, impression: variant.impression, ink: ink, pen: pen,
                                        index: index) {
                        variants[index].impression = Engine.randomSeed()
                    }
                }
            }
        }
        .navigationTitle(tr("sig_title"))
        .keyboardDoneButton()
        .navigationBarTitleDisplayMode(.inline)
        .scrollDismissesKeyboard(.interactively)
        .task {
            guard options == nil, let loaded = try? await Engine.run({ try signatureOptions() }) else { return }
            options = loaded
            form = loaded.forms.first?.value ?? 0
            pen = loaded.pens.first?.value ?? 0
        }
    }

    private func generate() async {
        error = nil
        let form = form
        do {
            let designs = try await Engine.run { try signatureDesigns(count: 6, form: form, seed: Engine.randomSeed()) }
            variants = designs.map { Variant(design: $0, impression: Engine.randomSeed()) }
        } catch {
            self.error = error.engineMessage
        }
    }
}

private struct SignatureVariantRow: View {
    @Environment(AppModel.self) private var model
    let name: String
    let design: UInt64
    let impression: UInt64
    let ink: String
    let pen: UInt8
    let index: Int
    let newImpression: () -> Void
    @State private var png: Data?
    @State private var error: String?

    private var fileName: String {
        "signature-" + name.trimmingCharacters(in: .whitespaces).split(whereSeparator: \.isWhitespace).joined(separator: "-") + ".png"
    }

    var body: some View {
        VStack(spacing: 10) {
            if let error {
                ErrorText(text: error)
            } else {
                EngineImage(png: png, label: tr("sig_alt", name), checker: true, identifier: "signature-image-\(index)")
                    .frame(height: 120)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
            }
            HStack {
                Button(tr("new_impression"), action: newImpression)
                Spacer()
                Button(tr("save_png")) { if let png { model.output(name: fileName, bytes: png) } }
                    .disabled(png == nil)
                    .accessibilityIdentifier("signature-save-\(index)")
                ShareButton(name: fileName, bytes: png, identifier: "signature-share-\(index)")
                    .labelStyle(.iconOnly)
            }
            .buttonStyle(.borderless)
        }
        .padding(.vertical, 4)
        .accessibilityIdentifier("signature-\(index)")
        .task(id: "\(name)|\(design)|\(impression)|\(ink)|\(pen)") {
            try? await Task.sleep(for: .milliseconds(100))
            guard !Task.isCancelled else { return }
            let (name, design, impression, ink, pen) = (name, design, impression, ink, pen)
            do {
                png = try await Engine.run {
                    try signaturePng(name: name, design: design, impression: impression, ink: ink, pen: pen, pixelHeight: 130)
                }
                error = nil
            } catch {
                self.error = error.engineMessage
            }
        }
    }
}

// MARK: - Stamps

/// Stamps from the built-in templates with editable fields (dates default to today), ink, live preview, new
/// impression; saved or shared at 2× scale.
struct StampView: View {
    @Environment(AppModel.self) private var model
    @State private var templates: [StampTemplate] = []
    @State private var templateID: String?
    @State private var values: [String: String] = [:]
    @State private var ink = ""
    @State private var impression = Engine.randomSeed()
    @State private var preview: Data?
    @State private var error: String?

    private var template: StampTemplate? { templates.first { $0.id == templateID } ?? templates.first }

    var body: some View {
        Form {
            if let template {
                Section {
                    Group {
                        if let error {
                            ErrorText(text: error)
                        } else {
                            EngineImage(png: preview, label: template.title, checker: true, identifier: "stamp-preview")
                        }
                    }
                    .frame(maxWidth: .infinity, minHeight: 240, maxHeight: 280)
                    .clipShape(RoundedRectangle(cornerRadius: 10))
                    .listRowInsets(EdgeInsets(top: 8, leading: 8, bottom: 8, trailing: 8))
                    HStack {
                        Button(tr("new_impression")) { impression = Engine.randomSeed() }
                        Spacer()
                        Button(tr("save_png")) { Task { await save() } }
                            .disabled(preview == nil)
                            .accessibilityIdentifier("stamp-save")
                        ShareButton(item: preview == nil ? nil : shareItem(template), identifier: "stamp-share")
                            .labelStyle(.iconOnly)
                    }
                    .buttonStyle(.borderless)
                }
                Section {
                    NavigationLink {
                        TemplatePicker(templates: templates, selection: $templateID)
                    } label: {
                        LabeledContent(tr("stamp_template"), value: template.title)
                    }
                    .accessibilityIdentifier("stamp-template")
                    Picker(tr("sig_ink"), selection: $ink) {
                        ForEach(["", "blue", "violet", "black", "red", "green"], id: \.self) { Text(inkTitle($0)).tag($0) }
                    }
                }
                Section {
                    ForEach(template.fields, id: \.key) { field in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(field.label).font(.caption).foregroundStyle(.secondary)
                            TextField(field.label, text: Binding(get: { values[field.key] ?? "" }, set: { values[field.key] = $0 }),
                                      axis: field.multiline ? .vertical : .horizontal)
                                .lineLimit(field.multiline ? 2...5 : 1...1)
                        }
                    }
                }
            } else if let error {
                ErrorText(text: error)
            }
        }
        .navigationTitle(tr("stamp_title"))
        .keyboardDoneButton()
        .navigationBarTitleDisplayMode(.inline)
        .scrollDismissesKeyboard(.interactively)
        .task {
            let language = siteLanguage()
            if templates.isEmpty { templates = (try? await Engine.run { try stampTemplates(language: language) }) ?? [] }
            if templateID == nil { templateID = templates.first?.id }
        }
        .onChange(of: template?.id) { _, _ in resetValues() }
        .task(id: previewKey) {
            guard let template else { return }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            let (id, values, ink, impression) = (template.id, values, ink, impression)
            do {
                preview = try await Engine.run { try stampPng(template: id, values: values, ink: ink, impression: impression, pixelScale: 0.5) }
                error = nil
            } catch {
                self.error = error.engineMessage
            }
        }
    }

    private var previewKey: String {
        "\(template?.id ?? "")|\(values.sorted { $0.key < $1.key })|\(ink)|\(impression)"
    }

    private func resetValues() {
        guard let template else { return }
        let formatter = DateFormatter()
        formatter.dateFormat = "dd/MM/yyyy"
        let today = formatter.string(from: Date())
        values = Dictionary(uniqueKeysWithValues: template.fields.map { ($0.key, $0.date ? today : $0.example) })
    }

    private func large(_ template: StampTemplate) -> () throws -> Data {
        let (id, values, ink, impression) = (template.id, values, ink, impression)
        return { try stampPng(template: id, values: values, ink: ink, impression: impression, pixelScale: 2) }
    }

    private func shareItem(_ template: StampTemplate) -> ShareFile {
        ShareFile(name: "stamp-\(template.id).png", make: large(template))
    }

    private func save() async {
        guard let template else { return }
        let make = large(template)
        do {
            model.output(name: "stamp-\(template.id).png", bytes: try await Engine.run(make))
        } catch {
            model.notify(error.engineMessage, error: true)
        }
    }
}

private struct TemplatePicker: View {
    let templates: [StampTemplate]
    @Binding var selection: String?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let categories = templates.reduce(into: [String]()) { if !$0.contains($1.category) { $0.append($1.category) } }
        List {
            ForEach(categories, id: \.self) { category in
                Section(category) {
                    ForEach(templates.filter { $0.category == category }, id: \.id) { template in
                        Button {
                            selection = template.id
                            dismiss()
                        } label: {
                            HStack {
                                Text(template.title).foregroundStyle(Color.primary)
                                Spacer()
                                if template.id == selection { Image(systemName: "checkmark").foregroundStyle(.tint) }
                            }
                        }
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle(tr("stamp_template"))
        .navigationBarTitleDisplayMode(.inline)
    }
}
