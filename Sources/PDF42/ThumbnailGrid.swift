import AppKit
import Kit42
import PDFKit
import UniformTypeIdentifiers

/// Page thumbnails with multi-selection, drag-to-reorder and drop of PDF/image files.
final class ThumbnailGrid: NSScrollView, NSCollectionViewDataSource, NSCollectionViewDelegateFlowLayout {
    weak var pdfView: PDFView?
    /// Called to move pages (indices) before `to`, and to insert files at an index.
    var onMove: (([Int], Int) -> Void)?
    var onDropFiles: (([URL], Int) -> Void)?
    /// Page label (logical number) for a page index, nil to show the physical number.
    var labelProvider: ((Int) -> String?)?

    private let collection = NSCollectionView()
    private var cache: [ObjectIdentifier: NSImage] = [:]
    private static let pageType = NSPasteboard.PasteboardType("com.sfzorin.pdf42.page-index")

    override init(frame: NSRect) {
        super.init(frame: frame)
        let layout = NSCollectionViewFlowLayout()
        layout.itemSize = NSSize(width: 130, height: 168)
        layout.minimumLineSpacing = 6
        layout.sectionInset = NSEdgeInsets(top: 6, left: 6, bottom: 12, right: 6)
        collection.collectionViewLayout = layout
        collection.dataSource = self
        collection.delegate = self
        collection.isSelectable = true
        collection.allowsMultipleSelection = true
        collection.backgroundColors = [.clear]
        collection.register(ThumbnailItem.self, forItemWithIdentifier: ThumbnailItem.id)
        collection.registerForDraggedTypes([Self.pageType, .fileURL])
        collection.setDraggingSourceOperationMask(.move, forLocal: true)
        collection.setDraggingSourceOperationMask(.copy, forLocal: false)
        documentView = collection
        hasVerticalScroller = true
        drawsBackground = false
        NotificationCenter.default.addObserver(self, selector: #selector(pageChanged), name: .PDFViewPageChanged, object: nil)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    var selectedPages: [PDFPage] {
        guard let doc = pdfView?.document else { return [] }
        return collection.selectionIndexPaths.map(\.item).sorted().compactMap { doc.page(at: $0) }
    }

    /// Restore page identities after a reorder or Undo, rather than retaining stale numeric indices.
    func selectPages(_ pages: [PDFPage]) {
        guard let doc = pdfView?.document else { return }
        collection.selectionIndexPaths = Set(pages.compactMap { page in
            let i = doc.index(for: page)
            return i != NSNotFound ? IndexPath(item: i, section: 0) : nil
        })
    }

    func reload() {
        cache.removeAll()
        collection.reloadData()
        pageChanged()
    }

    /// Page labels changed: only the captions.
    func reloadLabels() {
        let selection = collection.selectionIndexPaths
        collection.reloadData()
        collection.selectionIndexPaths = selection
    }

    /// Re-render one page (after annotations changed).
    func refresh(_ page: PDFPage) {
        cache[ObjectIdentifier(page)] = nil
        if let i = pdfView?.document?.index(for: page), i != NSNotFound { collection.reloadItems(at: [IndexPath(item: i, section: 0)]) }
    }

    @objc private func pageChanged(_ note: Notification? = nil) {
        if let note, note.object as? PDFView !== pdfView { return }
        guard let doc = pdfView?.document, let page = pdfView?.currentPage else { return }
        let i = doc.index(for: page)
        guard i != NSNotFound, i < collection.numberOfItems(inSection: 0) else { return }
        let path = IndexPath(item: i, section: 0)
        if collection.selectionIndexPaths.count <= 1, !collection.selectionIndexPaths.contains(path) {
            collection.selectionIndexPaths = [path]
            collection.scrollToItems(at: [path], scrollPosition: .nearestHorizontalEdge)
        }
    }

    private func image(for page: PDFPage) -> NSImage {
        if let i = cache[ObjectIdentifier(page)] { return i }
        let i = page.thumbnail(of: NSSize(width: 220, height: 280), for: .cropBox)
        cache[ObjectIdentifier(page)] = i
        return i
    }

    // MARK: Data source

    func collectionView(_ collectionView: NSCollectionView, numberOfItemsInSection section: Int) -> Int {
        pdfView?.document?.pageCount ?? 0
    }

    func collectionView(_ collectionView: NSCollectionView, itemForRepresentedObjectAt indexPath: IndexPath) -> NSCollectionViewItem {
        let item = collectionView.makeItem(withIdentifier: ThumbnailItem.id, for: indexPath) as! ThumbnailItem
        if let page = pdfView?.document?.page(at: indexPath.item) {
            let n = indexPath.item + 1
            let label = labelProvider?(indexPath.item).flatMap { $0 == "\(n)" ? nil : $0 }
            item.configure(image: image(for: page), number: label.map { "\($0) (\(n))" } ?? "\(n)", size: Self.pictureSize(page))
        }
        return item
    }

    /// Thumbnail keeps the page's proportions: landscape pages are wide, portrait ones tall.
    static func pictureSize(_ page: PDFPage) -> NSSize {
        let b = page.bounds(for: .cropBox).size
        let s = page.rotation % 180 == 0 ? b : NSSize(width: b.height, height: b.width)
        guard s.width > 0, s.height > 0 else { return NSSize(width: 110, height: 136) }
        let maxW: CGFloat = 118, maxH: CGFloat = 136
        let k = min(maxW / s.width, maxH / s.height)
        return NSSize(width: (s.width * k).rounded(), height: (s.height * k).rounded())
    }

    func collectionView(_ collectionView: NSCollectionView, layout collectionViewLayout: NSCollectionViewLayout,
                        sizeForItemAt indexPath: IndexPath) -> NSSize {
        let picture = pdfView?.document?.page(at: indexPath.item).map(Self.pictureSize) ?? NSSize(width: 110, height: 136)
        // One centred column at any sidebar width: a two-column flow staggered pages of different heights.
        let inset = (collectionViewLayout as? NSCollectionViewFlowLayout).map { $0.sectionInset.left + $0.sectionInset.right } ?? 12
        return NSSize(width: max(130, collectionView.bounds.width - inset), height: picture.height + 32)
    }

    private var laidOutWidth: CGFloat = 0
    override func tile() {
        super.tile()
        let width = contentView.bounds.width
        if abs(width - laidOutWidth) > 0.5 {
            laidOutWidth = width
            collection.collectionViewLayout?.invalidateLayout()
        }
    }

    func collectionView(_ collectionView: NSCollectionView, didSelectItemsAt indexPaths: Set<IndexPath>) {
        guard collectionView.selectionIndexPaths.count == 1, let i = indexPaths.first?.item, let page = pdfView?.document?.page(at: i) else { return }
        pdfView?.go(to: page)
    }

    // MARK: Drag & drop

    func collectionView(_ collectionView: NSCollectionView, pasteboardWriterForItemAt indexPath: IndexPath) -> NSPasteboardWriting? {
        let item = NSPasteboardItem()
        item.setString("\(indexPath.item)", forType: Self.pageType)
        return item
    }

    func collectionView(_ collectionView: NSCollectionView, validateDrop info: NSDraggingInfo,
                        proposedIndexPath: AutoreleasingUnsafeMutablePointer<NSIndexPath>,
                        dropOperation: UnsafeMutablePointer<NSCollectionView.DropOperation>) -> NSDragOperation {
        dropOperation.pointee = .before
        return info.draggingSource as? NSCollectionView === collection ? .move : .copy
    }

    func collectionView(_ collectionView: NSCollectionView, acceptDrop info: NSDraggingInfo, indexPath: IndexPath,
                        dropOperation: NSCollectionView.DropOperation) -> Bool {
        let target = indexPath.item
        if info.draggingSource as? NSCollectionView === collection {
            let indices = collection.selectionIndexPaths.map(\.item).sorted()
            let dragged = indices.isEmpty ? (info.draggingPasteboard.pasteboardItems?.compactMap { $0.string(forType: Self.pageType).flatMap(Int.init) } ?? []) : indices
            onMove?(dragged, target)
            return true
        }
        let urls = (info.draggingPasteboard.readObjects(forClasses: [NSURL.self]) as? [URL]) ?? []
        guard !urls.isEmpty else { return false }
        onDropFiles?(urls, target)
        return true
    }
}

final class ThumbnailItem: NSCollectionViewItem {
    static let id = NSUserInterfaceItemIdentifier("ThumbnailItem")
    private let picture = NSImageView()
    private let label = NSTextField(labelWithString: "")
    /// Accent ring around the page itself (selection), instead of a filled cell.
    private let ring = SelectionRing()
    private var pictureWidth: NSLayoutConstraint?
    private var pictureHeight: NSLayoutConstraint?

    override func loadView() {
        let v = NSView()
        v.wantsLayer = true
        picture.imageScaling = .scaleProportionallyUpOrDown
        picture.wantsLayer = true
        picture.layer?.shadowOpacity = 0.25
        picture.layer?.shadowRadius = 2
        picture.layer?.shadowOffset = CGSize(width: 0, height: -1)
        label.alignment = .center
        label.font = .systemFont(ofSize: 11, weight: .medium)
        ring.isHidden = true
        for s in [ring, picture, label] as [NSView] {
            s.translatesAutoresizingMaskIntoConstraints = false
            v.addSubview(s)
        }
        NSLayoutConstraint.activate([
            picture.topAnchor.constraint(equalTo: v.topAnchor, constant: 6),
            picture.centerXAnchor.constraint(equalTo: v.centerXAnchor),
            label.topAnchor.constraint(equalTo: picture.bottomAnchor, constant: 4),
            label.centerXAnchor.constraint(equalTo: v.centerXAnchor),
            ring.leadingAnchor.constraint(equalTo: picture.leadingAnchor, constant: -4),
            ring.trailingAnchor.constraint(equalTo: picture.trailingAnchor, constant: 4),
            ring.topAnchor.constraint(equalTo: picture.topAnchor, constant: -4),
            ring.bottomAnchor.constraint(equalTo: picture.bottomAnchor, constant: 4),
        ])
        pictureWidth = picture.widthAnchor.constraint(equalToConstant: 110)
        pictureHeight = picture.heightAnchor.constraint(equalToConstant: 136)
        pictureWidth?.isActive = true
        pictureHeight?.isActive = true
        view = v
    }

    func configure(image: NSImage, number: String, size: NSSize) {
        picture.image = image
        pictureWidth?.constant = size.width
        pictureHeight?.constant = size.height
        label.stringValue = number
    }

    override var isSelected: Bool {
        didSet {
            ring.isHidden = !isSelected
            label.textColor = isSelected ? .controlAccentColor : .secondaryLabelColor
        }
    }
}

/// Accent outline drawn around a selected page thumbnail; the colour is resolved in updateLayer,
/// so it follows appearance and accent-colour changes.
final class SelectionRing: NSView {
    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override var wantsUpdateLayer: Bool { true }

    override func updateLayer() {
        layer?.cornerRadius = 4
        layer?.borderWidth = 3
        layer?.borderColor = NSColor.controlAccentColor.cgColor
    }
}
