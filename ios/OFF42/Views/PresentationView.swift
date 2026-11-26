import SwiftUI

/// A deck as the engine reads it: every slide's title, text, tables and pictures, shown as 16:9 slides with a
/// slide strip (at the side on iPad, along the bottom on iPhone) and the speaker notes below the slide.
struct PresentationView: View {
    let file: OpenedFile
    let slides: [Slide]
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var current = 0

    var body: some View {
        Group {
            if slides.isEmpty {
                ContentUnavailableView(tr("slides_untitled"), systemImage: "rectangle.on.rectangle")
            } else if sizeClass == .regular {
                HStack(spacing: 0) {
                    SlideStrip(slides: slides, current: $current, axis: .vertical)
                        .frame(width: 196)
                    Divider()
                    stage
                }
            } else {
                VStack(spacing: 0) {
                    stage
                    Divider()
                    SlideStrip(slides: slides, current: $current, axis: .horizontal)
                        .frame(height: 118)
                }
            }
        }
        .background(Color(.systemGray5))
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                SaveMenu(file: file, formats: saveFormats(kind: file.kind))
            }
        }
    }

    private var stage: some View {
        GeometryReader { proxy in
            ScrollView {
                let index = min(current, slides.count - 1)
                VStack(alignment: .leading, spacing: 16) {
                    SlideCanvas(slide: slides[index], index: index, width: max(proxy.size.width - 40, 120))
                        .accessibilityIdentifier("slide-canvas")
                    if !slides[index].notes.isEmpty {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(tr("slides_notes_title")).font(.headline)
                            Text(slides[index].notes).font(.body)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding()
                        .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 12))
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("slide-notes")
                    }
                }
                .padding(20)
            }
        }
    }
}

/// The slides as small cards; the current one is outlined in the section colour.
private struct SlideStrip: View {
    let slides: [Slide]
    @Binding var current: Int
    let axis: Axis

    var body: some View {
        ScrollViewReader { reader in
            ScrollView(axis == .vertical ? .vertical : .horizontal) {
                let layout = axis == .vertical ? AnyLayout(VStackLayout(spacing: 12)) : AnyLayout(HStackLayout(spacing: 12))
                layout {
                    ForEach(slides.indices, id: \.self) { index in
                        Button {
                            current = index
                        } label: {
                            VStack(spacing: 4) {
                                SlideCanvas(slide: slides[index], index: index, width: axis == .vertical ? 156 : 128, thumbnail: true)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 4)
                                            .stroke(index == current ? AnyShapeStyle(.tint) : AnyShapeStyle(.clear), lineWidth: 3)
                                    )
                                Text(index + 1, format: .number)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .buttonStyle(.plain)
                        .id(index)
                        .accessibilityElement(children: .ignore)
                        .accessibilityLabel(tr("slide_n", index + 1) + ", " + SlideCanvas.title(of: slides[index]))
                        .accessibilityAddTraits(index == current ? [.isButton, .isSelected] : .isButton)
                        .accessibilityIdentifier("slide-\(index)")
                    }
                }
                .padding(12)
            }
            .onChange(of: current) { _, index in withAnimation { reader.scrollTo(index) } }
        }
        .background(Color(.secondarySystemBackground))
    }
}

/// One slide as a white 16:9 card, its type scaled to the card's width like a slide.
struct SlideCanvas: View {
    let slide: Slide
    let index: Int
    let width: CGFloat
    var thumbnail = false

    /// The title a slide shows (the engine picks the title placeholder or first text box when it has none).
    static func title(of slide: Slide) -> String {
        if !slide.title.isEmpty { return slide.title }
        if let object = slideTitleObject(slide: slide).map(Int.init), slide.objects.indices.contains(object) {
            return slide.objects[object].text
        }
        return ""
    }

    var body: some View {
        let scale = width / 960
        let titleIndex = slide.title.isEmpty ? slideTitleObject(slide: slide).map(Int.init) : nil
        let title = Self.title(of: slide)
        VStack(alignment: .leading, spacing: 18 * scale) {
            HStack(alignment: .firstTextBaseline) {
                Text(title.isEmpty ? tr("slides_untitled") : title)
                    .font(.system(size: 44 * scale, weight: .bold))
                    .foregroundStyle(title.isEmpty ? Color.gray : Color.black)
                    .lineLimit(thumbnail ? 3 : nil)
                Spacer(minLength: 0)
                if slide.hidden && !thumbnail {
                    Text(tr("slides_hidden"))
                        .font(.system(size: 22 * scale, weight: .medium))
                        .padding(.horizontal, 10 * scale)
                        .padding(.vertical, 4 * scale)
                        .background(Color.gray.opacity(0.18), in: Capsule())
                        .foregroundStyle(Color.gray)
                }
            }
            if !thumbnail {
                if !slide.body.isEmpty { Bullets(text: slide.body, scale: scale) }
                ForEach(slide.objects.indices, id: \.self) { i in
                    if i != titleIndex { object(slide.objects[i], scale: scale) }
                }
            }
        }
        .padding(48 * scale)
        .frame(width: width, alignment: .topLeading)
        .frame(minHeight: width * 9 / 16, alignment: .topLeading)
        .background(Color.white)
        .clipShape(RoundedRectangle(cornerRadius: thumbnail ? 4 : 6))
        .shadow(color: .black.opacity(0.18), radius: thumbnail ? 1.5 : 4, y: 1)
        .opacity(slide.hidden ? 0.6 : 1)
        .environment(\.colorScheme, .light)
    }

    @ViewBuilder
    private func object(_ object: SlideObject, scale: CGFloat) -> some View {
        if !object.cells.isEmpty {
            Grid(horizontalSpacing: 0, verticalSpacing: 0) {
                ForEach(object.cells.indices, id: \.self) { r in
                    GridRow {
                        ForEach(object.cells[r].indices, id: \.self) { c in
                            Text(object.cells[r][c])
                                .font(.system(size: 22 * scale, weight: r == 0 ? .semibold : .regular))
                                .foregroundStyle(Color.black)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(8 * scale)
                                .border(Color.gray.opacity(0.5), width: 0.5)
                        }
                    }
                }
            }
        } else if object.kind == "Picture" {
            Label(tr("slides_picture"), systemImage: "photo")
                .font(.system(size: 22 * scale))
                .foregroundStyle(Color.gray)
                .frame(maxWidth: .infinity, minHeight: 120 * scale)
                .background(Color.gray.opacity(0.12), in: RoundedRectangle(cornerRadius: 8 * scale))
        } else if !object.text.isEmpty {
            if object.role == "body" {
                Bullets(text: object.text, scale: scale)
            } else {
                Text(object.text).font(.system(size: 26 * scale)).foregroundStyle(Color.black)
            }
        }
    }
}

private struct Bullets: View {
    let text: String
    let scale: CGFloat

    var body: some View {
        VStack(alignment: .leading, spacing: 10 * scale) {
            ForEach(Array(text.split(separator: "\n").map(String.init).filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }.enumerated()),
                    id: \.offset) { _, line in
                HStack(alignment: .firstTextBaseline, spacing: 12 * scale) {
                    Text("•")
                    Text(line)
                }
                .font(.system(size: 28 * scale))
                .foregroundStyle(Color.black)
            }
        }
    }
}
