import AppKit
import Kit42

enum Cmd {
    typealias W = DocWindowController
    typealias A = AppDelegate

    static let exportPDF = Command(L("Export as PDF…"), #selector(W.exportPDFAction(_:)), symbol: "doc.richtext", shortcut: Shortcut("e", [.command, .option]))

    static let exportAs = Command(L("Export As…"), #selector(W.exportAsAction(_:)), shortcut: Shortcut("e", [.command, .option, .shift]))
    static let find = Command(L("Find…"), #selector(W.findAction(_:)), shortcut: Shortcut("f"))
    static let findReplace = Command(L("Find and Replace…"), #selector(W.findReplaceAction(_:)), symbol: "text.magnifyingglass",
                                     shortcut: Shortcut("f", [.command, .option]))
    static let findNext = Command(L("Find Next"), #selector(W.findNextAction(_:)), shortcut: Shortcut("g"))
    static let findPrevious = Command(L("Find Previous"), #selector(W.findPreviousAction(_:)), shortcut: Shortcut("g", [.command, .shift]))
    static let useSelectionForFind = Command(L("Use Selection for Find"), #selector(W.useSelectionForFindAction(_:)), shortcut: Shortcut("e"))
    static let replaceBar = Command(L("Replace in Find Bar"), #selector(W.showReplaceBarAction(_:)))

    static let zoomIn = Command(L("Zoom In"), #selector(W.zoomInAction(_:)), symbol: "plus.magnifyingglass", shortcut: Shortcut("+"))
    static let zoomOut = Command(L("Zoom Out"), #selector(W.zoomOutAction(_:)), symbol: "minus.magnifyingglass", shortcut: Shortcut("-"))
    static let actualSize = Command(L("Actual Size"), #selector(W.actualSizeAction(_:)), shortcut: Shortcut("0"))
    static let formatBar = Command(L("Hide Format Bar"), #selector(W.toggleFormatBarAction(_:)), symbol: "textformat",
                                   shortcut: Shortcut("a", [.command, .shift]), toolTip: L("Format bar"), label: L("Format"))
    static let ruler = Command(L("Hide Ruler"), #selector(W.toggleRulerAction(_:)), shortcut: Shortcut("r", [.command, .option]))

    static let insertImage = Command(L("Image…"), #selector(W.insertImageAction(_:)), symbol: "photo", shortcut: Shortcut("i", [.command, .shift]))
    static let technicalElement = Command(L("Symbol or Code…"), #selector(W.insertTechnicalElementAction(_:)), symbol: "qrcode")
    static let editTechnicalElement = Command(L("Edit Symbol or Code…"), #selector(W.editTechnicalElementAction(_:)))
    static let createLabels = Command(L("Create Labels…"), #selector(W.createLabelsAction(_:)), symbol: "tag")
    static let insertTable = Command(L("Table…"), #selector(W.insertTableAction(_:)), symbol: "tablecells", shortcut: Shortcut("t", [.command, .option]))
    static let stamps = Command(L("Signature or Stamp…"), #selector(W.showStampsAction(_:)), symbol: "signature",
                                shortcut: Shortcut("k", [.command, .shift]), toolTip: L("Insert a signature or stamp"), label: L("Stamp"))
    static let pageBreak = Command(L("Page Break"), #selector(W.insertPageBreakAction(_:)), shortcut: Shortcut("\r", [.command, .shift]))
    static let date = Command(L("Date"), #selector(W.insertDateAction(_:)))
    static let sectionBreak = Command(L("Section Break (Next Page)"), #selector(W.insertSectionBreakAction(_:)))
    static let link = Command(L("Link…"), #selector(W.insertLinkAction(_:)), symbol: "link", shortcut: Shortcut("k"), toolTip: L("Insert or edit a link"), label: L("Link"))
    static let removeLink = Command(L("Remove Link"), #selector(W.removeLinkAction(_:)))
    static let footnote = Command(L("Footnote"), #selector(W.insertFootnoteAction(_:)), symbol: "textformat.superscript",
                                  shortcut: Shortcut("n", [.command, .option]), toolTip: L("Insert a footnote"), label: L("Footnote"))
    static let editFootnote = Command(L("Edit Footnote…"), #selector(W.editFootnoteAction(_:)))
    static let deleteFootnote = Command(L("Delete Footnote"), #selector(W.deleteFootnoteAction(_:)))
    static let comment = Command(L("New Comment"), #selector(W.newCommentAction(_:)), symbol: "text.bubble",
                                 shortcut: Shortcut("m", [.command, .option]), toolTip: L("Comment on the selection"), label: L("Comment"))
    static let resolveComment = Command(L("Resolve Comment"), #selector(W.resolveCommentAction(_:)))
    static let deleteComment = Command(L("Delete Comment"), #selector(W.deleteCommentAction(_:)))
    static let toggleComments = Command(L("Hide Comments"), #selector(W.toggleCommentsAction(_:)))
    // Review (track changes)
    static let trackChanges = Command(L("Track Changes"), #selector(W.toggleTrackChangesAction(_:)), symbol: "pencil.line",
                                      shortcut: Shortcut("e", [.command, .shift]), toolTip: L("Record insertions and deletions"), label: L("Track"))
    static let trackChangesItem = Command(L("Track Changes"), #selector(W.toggleTrackChangesAction(_:)), symbol: "pencil.line")
    static let showMarkup = Command(L("Show Markup"), #selector(W.toggleMarkupAction(_:)))
    static let nextChange = Command(L("Next Change"), #selector(W.nextChangeAction(_:)), symbol: "chevron.down")
    static let previousChange = Command(L("Previous Change"), #selector(W.previousChangeAction(_:)), symbol: "chevron.up")
    static let acceptChange = Command(L("Accept Change"), #selector(W.acceptChangeAction(_:)), symbol: "checkmark")
    static let rejectChange = Command(L("Reject Change"), #selector(W.rejectChangeAction(_:)), symbol: "xmark")
    static let acceptAllChanges = Command(L("Accept All Changes"), #selector(W.acceptAllChangesAction(_:)))
    static let rejectAllChanges = Command(L("Reject All Changes"), #selector(W.rejectAllChangesAction(_:)))
    static let specialCharacter = Command(L("Special Character…"), #selector(W.specialCharacterAction(_:)), symbol: "character")
    static let characterViewer = Command(L("Emoji & Symbols"), #selector(W.characterViewerAction(_:)))
    static let insertTOC = Command(L("Table of Contents"), #selector(W.insertTOCAction(_:)), symbol: "list.bullet.indent")
    static let updateTOC = Command(L("Update Table of Contents"), #selector(W.updateTOCAction(_:)))
    static let paragraph = Command(L("Paragraph…"), #selector(W.paragraphAction(_:)), symbol: "paragraphsign")
    static let formatPainter = Command(L("Clone Formatting"), #selector(W.formatPainterAction(_:)), symbol: "paintbrush",
                                       shortcut: Shortcut("c", [.command, .option]),
                                       toolTip: L("Clone formatting: click, then select text (double-click to keep it on)"), label: L("Clone"))
    static let statistics = Command(L("Statistics…"), #selector(W.statisticsAction(_:)), symbol: "chart.bar")
    static let columns: [Command] = [L("One Column"), L("Two Columns"), L("Three Columns")].enumerated().map {
        Command($0.element, #selector(W.columnsAction(_:)), symbol: ["rectangle", "rectangle.split.2x1", "rectangle.split.3x1"][$0.offset], tag: $0.offset)
    }

    static let bold = Command(L("Bold"), #selector(W.toggleBold(_:)), symbol: "bold", shortcut: Shortcut("b"))
    static let italic = Command(L("Italic"), #selector(W.toggleItalic(_:)), symbol: "italic", shortcut: Shortcut("i"))
    static let underline = Command(L("Underline"), #selector(W.toggleUnderline(_:)), symbol: "underline", shortcut: Shortcut("u"))
    static let strike = Command(L("Strikethrough"), #selector(W.toggleStrike(_:)), symbol: "strikethrough", shortcut: Shortcut("x", [.command, .shift]))
    static let bigger = Command(L("Bigger"), #selector(W.biggerAction(_:)), shortcut: Shortcut(".", [.command, .shift]))
    static let smaller = Command(L("Smaller"), #selector(W.smallerAction(_:)), shortcut: Shortcut(",", [.command, .shift]))
    static let alignLeft = Command(L("Align Left"), #selector(W.alignAction(_:)), symbol: "text.alignleft", shortcut: Shortcut("{"), tag: 0)
    static let alignCenter = Command(L("Align Centre"), #selector(W.alignAction(_:)), symbol: "text.aligncenter", shortcut: Shortcut("|"), tag: 1)
    static let alignRight = Command(L("Align Right"), #selector(W.alignAction(_:)), symbol: "text.alignright", shortcut: Shortcut("}"), tag: 2)
    static let justify = Command(L("Justify"), #selector(W.alignAction(_:)), symbol: "text.justify", tag: 3)
    static let bulletList = Command(L("Bulleted List"), #selector(W.bulletListAction(_:)), symbol: "list.bullet", shortcut: Shortcut("7", [.command, .shift]))
    static let numberedList = Command(L("Numbered List"), #selector(W.numberedListAction(_:)), symbol: "list.number", shortcut: Shortcut("8", [.command, .shift]))
    static let indent = Command(L("Increase Indent"), #selector(W.indentAction(_:)), symbol: "increase.indent", shortcut: Shortcut("]"))
    static let outdent = Command(L("Decrease Indent"), #selector(W.outdentAction(_:)), symbol: "decrease.indent", shortcut: Shortcut("["))
    static let removeHighlight = Command(L("Remove Highlight"), #selector(W.removeHighlightAction(_:)))
    static let clearFormatting = Command(L("Clear Formatting"), #selector(W.clearFormattingAction(_:)))
    static var styles: [Command] {
        Styles.all.enumerated().map { i, s in
            Command(s.name, #selector(W.applyStyleAction(_:)), shortcut: i < 4 ? Shortcut("\(i + 1)", [.command, .option]) : nil, tag: i)
        }
    }
    static let lineSpacing: [Command] = ["1.0", "1.15", "1.5", "2.0"].enumerated().map {
        Command($0.element, #selector(W.lineSpacingAction(_:)), tag: $0.offset)
    }

    // Document & tables
    static let documentSetup = Command(L("Document Setup…"), #selector(W.documentSetupAction(_:)), symbol: "doc.text",
                                       shortcut: Shortcut("p", [.command, .option]))
    static let headerFooter = Command(L("Header and Footer…"), #selector(W.headerFooterAction(_:)), symbol: "rectangle.topthird.inset.filled")
    static var margins: [Command] {
        DocWindowController.marginPresets.enumerated().map { i, p in
            Command(p.name, #selector(W.marginPresetAction(_:)), tag: i)
        } + [Command(L("Custom Margins…"), #selector(W.documentSetupAction(_:)))]
    }
    static let portrait = Command(L("Portrait"), #selector(W.orientationAction(_:)), symbol: "rectangle.portrait", tag: 0)
    static let landscape = Command(L("Landscape"), #selector(W.orientationAction(_:)), symbol: "rectangle", tag: 1)
    static let pageNumbers = Command(L("Page Numbers"), #selector(W.togglePageNumbersAction(_:)), symbol: "number")
    static let tableRows: [Command] = [(L("Insert Row Above"), "arrow.up.to.line"), (L("Insert Row Below"), "arrow.down.to.line"), (L("Delete Row"), "minus.rectangle")]
        .enumerated().map { Command($0.element.0, #selector(W.tableRowAction(_:)), symbol: $0.element.1, tag: $0.offset) }
    static let tableColumns: [Command] = [(L("Insert Column Left"), "arrow.left.to.line"), (L("Insert Column Right"), "arrow.right.to.line"), (L("Delete Column"), "minus.rectangle.portrait")]
        .enumerated().map { Command($0.element.0, #selector(W.tableColumnAction(_:)), symbol: $0.element.1, tag: $0.offset) }
    static let deleteTable = Command(L("Delete Table"), #selector(W.deleteTableAction(_:)), symbol: "trash")
    static let print = Command(L("Print…"), #selector(NSDocument.printDocument(_:)), symbol: "printer", toolTip: L("Print"), label: L("Print"))

    static let welcome = Command(L("Welcome Window"), #selector(A.showWelcomeAction(_:)), shortcut: Shortcut("0", [.command, .shift]))

    // Writer-like commands: styles, navigator, fields, numbering, tables, pictures, page look, view.
    static func fkey(_ code: Int) -> String { String(Character(UnicodeScalar(UInt32(code))!)) }
    static let stylesPanel = Command(L("Styles"), #selector(W.toggleStylesAction(_:)), symbol: "paintpalette", shortcut: Shortcut("t"),
                                     toolTip: L("Styles panel"), label: L("Styles"))
    static let navigator = Command(L("Navigator"), #selector(W.toggleNavigatorAction(_:)), symbol: "list.bullet.rectangle",
                                   shortcut: Shortcut(fkey(NSF5FunctionKey), []), toolTip: L("Headings, tables, images, bookmarks, comments…"), label: L("Navigator"))
    static let statusBar = Command(L("Hide Status Bar"), #selector(W.toggleStatusBarAction(_:)))
    static let viewModes: [Command] = [L("Single Page"), L("Book View (Two Pages)"), L("Multiple Pages"), L("Web Layout")].enumerated().map {
        Command($0.element, #selector(W.viewModeAction(_:)), symbol: ["doc", "book", "square.grid.2x2", "globe"][$0.offset], tag: $0.offset)
    }
    static let fields: [Command] = [L("Page Number"), L("Page Count"), L("Date"), L("Time"), L("Title"), L("Author"), L("File Name"), L("Chapter")]
        .enumerated().map { Command($0.element, #selector(W.insertFieldAction(_:)), tag: $0.offset) }
    static let moreFields = Command(L("More Fields…"), #selector(W.moreFieldsAction(_:)))
    static let updateFields = Command(L("Update Fields"), #selector(W.updateFieldsAction(_:)), shortcut: Shortcut(fkey(NSF9FunctionKey), []))
    static let caption = Command(L("Caption…"), #selector(W.insertCaptionAction(_:)))
    static let bookmark = Command(L("Bookmark…"), #selector(W.insertBookmarkAction(_:)))
    static let crossReference = Command(L("Cross-reference…"), #selector(W.insertCrossReferenceAction(_:)))
    static let editHeader = Command(L("Edit Header"), #selector(W.editHeaderAction(_:)))
    static let editFooter = Command(L("Edit Footer"), #selector(W.editFooterAction(_:)))
    static let character = Command(L("Character…"), #selector(W.characterAction(_:)), symbol: "textformat", shortcut: Shortcut("t", [.command, .shift]))
    static let bulletsNumbering = Command(L("Bullets and Numbering…"), #selector(W.bulletsAndNumberingAction(_:)), symbol: "list.number")
    static let outlineNumbering = Command(L("Outline Numbering…"), #selector(W.outlineNumberingAction(_:)))
    static let demote = Command(L("Demote One Level"), #selector(W.demoteListAction(_:)))
    static let promote = Command(L("Promote One Level"), #selector(W.promoteListAction(_:)))
    static let borders = Command(L("Borders and Background…"), #selector(W.paragraphBordersAction(_:)))
    static let dropCap = Command(L("Drop Cap…"), #selector(W.dropCapAction(_:)))
    static let newStyle = Command(L("New Style from Selection…"), #selector(W.newStyleAction(_:)))
    static let updateStyle = Command(L("Update Style to Match Selection"), #selector(W.updateStyleAction(_:)))
    static let modifyStyle = Command(L("Modify Style…"), #selector(W.modifyStyleAction(_:)))
    static let characterStyles: [Command] = [L("Default Character Style"), L("Emphasis"), L("Strong"), L("Code")].enumerated().map {
        Command($0.element, #selector(W.applyCharacterStyleAction(_:)), tag: $0.offset)
    }
    static let pageBackground = Command(L("Background Colour…"), #selector(W.pageBackgroundAction(_:)))
    static let watermark = Command(L("Watermark…"), #selector(W.watermarkAction(_:)))
    static let hyphenation = Command(L("Hyphenation"), #selector(W.toggleHyphenationAction(_:)))
    static let imageWrap: [Command] = W.wrapTitles.enumerated().map { Command($0.element, #selector(W.imageWrapAction(_:)), tag: $0.offset) }
    static let imageProperties = Command(L("Image Properties…"), #selector(W.imagePropertiesAction(_:)))
    static let mergeCells = Command(L("Merge Cells"), #selector(W.mergeCellsAction(_:)), symbol: "rectangle.compress.vertical")
    static let splitCells = Command(L("Split Cells…"), #selector(W.splitCellsAction(_:)), symbol: "rectangle.split.2x1")
    static let tableProperties = Command(L("Table Properties…"), #selector(W.tablePropertiesAction(_:)), symbol: "tablecells")
    static let autoFormats: [Command] = W.autoFormats.enumerated().map { Command($0.element, #selector(W.tableAutoFormatAction(_:)), tag: $0.offset) }
    static let sortTable = Command(L("Sort…"), #selector(W.sortTableAction(_:)), symbol: "arrow.up.arrow.down")
    static let tableFormula = Command(L("Formula…"), #selector(W.tableFormulaAction(_:)), symbol: "function")
    static let textToTable = Command(L("Convert Text to Table…"), #selector(W.textToTableAction(_:)))
    static let tableToText = Command(L("Convert Table to Text"), #selector(W.tableToTextAction(_:)))
    static let properties = Command(L("Properties…"), #selector(W.documentPropertiesAction(_:)))
    static let exportPDFOptions = Command(L("Export as PDF…"), #selector(W.exportPDFAction(_:)))

    /// F11 also opens the Styles panel (as in Writer), from a hidden item.
    static var stylesF11: MenuElement {
        let item = NSMenuItem(title: L("Styles"), action: #selector(W.toggleStylesAction(_:)), keyEquivalent: fkey(NSF11FunctionKey))
        item.keyEquivalentModifierMask = []
        item.isHidden = true
        item.allowsKeyEquivalentWhenHidden = true
        return .item(item)
    }

    static var menuBar: MenuBarSpec {
        var spec = MenuBarSpec(appName: "DOC42")
        spec.fileExtras = [.command(exportPDF), .command(exportAs), .command(createLabels), .separator, .command(properties)]
        spec.editExtras = [.command(trackChanges)]
        spec.find = [.command(find), .command(findReplace), .separator, .command(findNext), .command(findPrevious),
                     .command(useSelectionForFind), .separator, .command(replaceBar)]
        spec.view = [.command(zoomIn), .command(zoomOut), .command(actualSize), .separator] + viewModes.menuElements + [.separator,
                     .command(formatBar), .command(ruler), .command(statusBar), .command(toggleComments), .separator,
                     .command(navigator), .command(stylesPanel), stylesF11]
        spec.insert = [.command(insertImage), .command(insertTable), .command(stamps), .command(technicalElement), .command(editTechnicalElement), .separator,
                       .command(link), .command(removeLink), .command(bookmark), .command(crossReference), .separator,
                       .command(footnote), .command(editFootnote), .command(deleteFootnote), .separator,
                       .command(comment), .command(resolveComment), .command(deleteComment), .separator,
                       .submenu(L("Field"), fields.menuElements + [.separator, .command(moreFields), .command(updateFields)]),
                       .command(caption), .separator,
                       .submenu(L("Header and Footer"), [.command(headerFooter), .command(editHeader), .command(editFooter), .separator, .command(pageNumbers)]),
                       .separator,
                       .command(specialCharacter), .command(characterViewer), .command(date), .separator,
                       .command(pageBreak), .command(sectionBreak), .separator, .command(insertTOC), .command(updateTOC)]
        spec.format = [.command(character), .command(paragraph), .command(bulletsNumbering), .command(outlineNumbering),
                       .command(borders), .command(dropCap), .separator,
                       .submenu(L("Styles"), [.command(stylesPanel), .separator, .command(newStyle), .command(updateStyle), .command(modifyStyle), .separator]
                                + styles.menuElements + [.separator] + characterStyles.menuElements),
                       .submenu(L("Page"), [.command(documentSetup), .submenu(L("Margins"), margins.menuElements),
                                            .submenu(L("Orientation"), [.command(portrait), .command(landscape)]), .separator,
                                            .command(pageBackground), .command(watermark), .separator, .command(headerFooter)]),
                       .submenu(L("Columns"), columns.menuElements), .command(hyphenation),
                       .submenu(L("Image"), [.submenu(L("Wrap"), imageWrap.menuElements), .command(imageProperties)]), .separator,
                       .command(bold), .command(italic), .command(underline), .command(strike), .command(bigger), .command(smaller), .separator,
                       .command(alignLeft), .command(alignCenter), .command(alignRight), .command(justify), .separator,
                       .command(bulletList), .command(numberedList), .command(demote), .command(promote), .command(indent), .command(outdent),
                       .submenu(L("Line Spacing"), lineSpacing.menuElements), .separator,
                       .command(formatPainter), .command(removeHighlight), .command(clearFormatting)]
        spec.custom = [(L("Review"), [.command(trackChangesItem), .command(showMarkup), .separator,
                                       .command(nextChange), .command(previousChange), .separator,
                                       .command(acceptChange), .command(rejectChange), .command(acceptAllChanges), .command(rejectAllChanges), .separator,
                                       .command(comment), .command(resolveComment), .command(deleteComment), .command(toggleComments)]),
                       (L("Table"), [.command(insertTable), .separator] + tableRows.menuElements + [.separator] + tableColumns.menuElements
                        + [.separator, .command(mergeCells), .command(splitCells), .separator, .command(tableProperties),
                           .submenu(L("AutoFormat"), autoFormats.menuElements), .command(sortTable), .command(tableFormula), .separator,
                           .command(textToTable), .command(tableToText), .separator, .command(deleteTable)]),
                       (L("Document"), [.command(documentSetup), .submenu(L("Margins"), margins.menuElements),
                                         .submenu(L("Orientation"), [.command(portrait), .command(landscape)]),
                                         .submenu(L("Columns"), columns.menuElements), .command(sectionBreak), .separator,
                                         .command(headerFooter), .command(pageNumbers), .separator,
                                         .command(insertTOC), .command(updateTOC), .separator, .command(statistics), .command(properties)])]
        spec.help = [.command(welcome)]
        return spec
    }

    /// Default toolbar: insert actions, panels and search. Zoom lives in the status bar slider;
    /// print/export are in the File menu. The rest stays available in Customize Toolbar.
    static var toolbar: [ToolbarElement] {
        [
            .command(insertImage), .command(insertTable), .command(stamps), .command(link), .command(comment),
            .flexibleSpace,
            .command(stylesPanel), .command(navigator), .command(formatBar),
            .search(id: "search", placeholder: L("Search"), action: #selector(W.searchChanged(_:))),
        ]
    }

    static var toolbarOptional: [ToolbarElement] {
        [.command(footnote), .command(trackChanges), .group(id: "zoom", label: L("Zoom"), [zoomOut, zoomIn]),
         .command(print), .command(exportPDF), .command(technicalElement), .command(createLabels)]
    }
}
