import SwiftUI
import UniformTypeIdentifiers

/// Sections | opened files (or a section's tools) | the open file. iPad shows the three columns side by side; on
/// iPhone the same split view collapses into a navigation stack (sections → files → file).
struct RootView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        @Bindable var model = model
        NavigationSplitView(columnVisibility: $model.columnVisibility, preferredCompactColumn: $model.compactColumn) {
            Sidebar()
        } content: {
            if let section = model.section {
                if section.fileKind != nil {
                    FileListView(section: section)
                } else {
                    ToolListView(section: section)
                }
            } else {
                EmptyDetail(section: .documents, text: "")
            }
        } detail: {
            NavigationStack {
                DetailColumn()
            }
        }
        .navigationSplitViewStyle(.balanced)
        .overlay(alignment: .top) {
            if model.busy > 0 {
                ProgressView()
                    .padding(10)
                    .background(.regularMaterial, in: Capsule())
                    .padding(.top, 4)
                    .accessibilityIdentifier("busy")
            }
        }
        .overlay(alignment: .bottom) { NoticeView() }
        .fileExporter(
            isPresented: Binding(get: { model.pendingExport != nil }, set: { if !$0 { model.pendingExport = nil } }),
            document: model.pendingExport.map { ExportDocument(bytes: $0.bytes) },
            contentType: Files.contentType(ext: model.pendingExport?.ext ?? ""),
            defaultFilename: model.pendingExport?.name
        ) { result in
            let name = model.pendingExport?.name ?? ""
            model.pendingExport = nil
            model.exportFinished(result, name: name)
        }
        .fileImporter(
            isPresented: Binding(get: { model.importing != nil }, set: { if !$0 { model.importing = nil } }),
            allowedContentTypes: Files.pickerTypes(model.importing?.fileKind ?? .document)
        ) { result in
            model.importing = nil
            switch result {
            case let .success(url): Task { await model.open(url: url) }
            case let .failure(error): model.notify(error.localizedDescription, error: true)
            }
        }
        .alert(
            tr("convert_title", model.question?.format.uppercased() ?? ""),
            isPresented: Binding(get: { model.question != nil }, set: { if !$0 { model.question = nil } }),
            presenting: model.question
        ) { question in
            Button(tr("convert")) { Task { await model.answer(question, convert: true) } }
                .accessibilityIdentifier("convert-anyway")
            Button(tr("cancel"), role: .cancel) { Task { await model.answer(question, convert: false) } }
        } message: { question in
            Text(tr("doc_convert_anyway", question.message, question.format.uppercased()))
        }
        .onAppear { applySizeClass(sizeClass) }
        .onChange(of: sizeClass) { _, new in applySizeClass(new) }
    }

    /// Side by side, every column shows something; in the stack only what the user navigated to.
    private func applySizeClass(_ sizeClass: UserInterfaceSizeClass?) {
        guard sizeClass == .regular else { return }
        if model.section == nil { model.section = .documents }
        for section in AppSection.allCases where !section.tools.isEmpty && model.selectedTool[section] == nil {
            model.selectedTool[section] = section.tools.first
        }
    }
}

private struct Sidebar: View {
    @Environment(AppModel.self) private var model
    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        // In the iPhone stack, choosing a section shows its list rather than jumping into the file last open there.
        let selection = Binding<AppSection?>(get: { model.section }, set: { section in
            if sizeClass == .compact, let section {
                model.selectedFile[section] = nil
                model.selectedTool[section] = nil
            }
            model.section = section
        })
        List(selection: selection) {
            Section {
                ForEach(AppSection.allCases) { section in
                    NavigationLink(value: section) {
                        Label {
                            HStack {
                                Text(section.title)
                                Spacer()
                                let count = model.files(of: section).count
                                if count > 0 {
                                    Text(count, format: .number).foregroundStyle(.secondary)
                                }
                            }
                        } icon: {
                            SectionIcon(symbol: section.symbol, color: section.accent)
                        }
                    }
                    .accessibilityIdentifier("section-\(section.rawValue)")
                }
            } footer: {
                Text(tr("privacy_note"))
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("OFF42")
    }
}

/// The opened files of one kind, with "Open file".
private struct FileListView: View {
    @Environment(AppModel.self) private var model
    let section: AppSection

    var body: some View {
        let files = model.files(of: section)
        List(selection: Binding(get: { model.selectedFile[section] }, set: { model.selectedFile[section] = $0 })) {
            Section {
                Button {
                    model.importing = section
                } label: {
                    Label(tr("open_file"), systemImage: "folder")
                }
                .accessibilityIdentifier("open-file")
            } footer: {
                Text(tr("formats_hint", section.formats))
            }
            if files.isEmpty {
                Section {
                    Text(tr("no_files")).foregroundStyle(.secondary)
                } footer: {
                    Text(tr("privacy_note"))
                }
            } else {
                Section {
                    ForEach(files) { file in
                        NavigationLink(value: file.id) {
                            FileRow(file: file, section: section)
                        }
                        .accessibilityIdentifier("file-\(file.name)")
                        .swipeActions {
                            Button(role: .destructive) { model.close(file) } label: {
                                Label(tr("close_file"), systemImage: "xmark")
                            }
                        }
                        .contextMenu {
                            Button(role: .destructive) { model.close(file) } label: {
                                Label(tr("close_file"), systemImage: "xmark")
                            }
                        }
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle(section.title)
        .tint(section.accent)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    model.importing = section
                } label: {
                    Label(tr("open_file"), systemImage: "plus")
                }
            }
        }
    }
}

private struct FileRow: View {
    let file: OpenedFile
    let section: AppSection

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 2) {
                Text(file.name).lineLimit(2)
                Text(ByteCountFormatter.string(fromByteCount: Int64(file.bytes.count), countStyle: .file))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        } icon: {
            Image(systemName: section.symbol).foregroundStyle(section.accent)
        }
    }
}

/// The generators of the Labels & codes and Signatures & stamps sections.
private struct ToolListView: View {
    @Environment(AppModel.self) private var model
    let section: AppSection

    var body: some View {
        List(selection: Binding(get: { model.selectedTool[section] }, set: { model.selectedTool[section] = $0 })) {
            Section {
                ForEach(section.tools) { tool in
                    NavigationLink(value: tool) {
                        Label {
                            Text(tool.title)
                        } icon: {
                            SectionIcon(symbol: tool.symbol, color: section.accent)
                        }
                    }
                    .accessibilityIdentifier("tool-\(tool.rawValue)")
                }
            } footer: {
                if section == .labels { Text(tr("marks_hint")) }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle(section.title)
    }
}

private struct DetailColumn: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        if let section = model.section {
            Group {
                if section.fileKind != nil {
                    if let file = model.file(in: section) {
                        FileDetailView(file: file).id(file.id)
                    } else {
                        EmptyDetail(section: section) { model.importing = section }
                    }
                } else if let tool = model.selectedTool[section] {
                    ToolView(tool: tool).id(tool)
                } else {
                    EmptyDetail(section: section, text: "")
                }
            }
            .tint(section.accent)
        } else {
            EmptyDetail(section: .documents, text: "")
        }
    }
}

private struct ToolView: View {
    let tool: Tool

    var body: some View {
        switch tool {
        case .codes: CodesView()
        case .labelSheet: LabelSheetView()
        case .marks: MarksView()
        case .signature: SignatureView()
        case .stamp: StampView()
        }
    }
}

/// Saved files and errors, shown briefly at the bottom and announced to VoiceOver.
private struct NoticeView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Group {
            if let notice = model.notice {
                Label(notice.text, systemImage: notice.isError ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                    .font(.callout)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .background(.regularMaterial, in: Capsule())
                    .foregroundStyle(notice.isError ? Color.red : Color.primary)
                    .padding(.bottom, 24)
                    .padding(.horizontal, 16)
                    .accessibilityIdentifier("notice")
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                    .task(id: notice.id) {
                        UIAccessibility.post(notification: .announcement, argument: notice.text)
                        try? await Task.sleep(for: .seconds(notice.isError ? 6 : 3))
                        if model.notice?.id == notice.id { model.notice = nil }
                    }
                    .onTapGesture { model.notice = nil }
            }
        }
        .animation(.default, value: model.notice)
    }
}
