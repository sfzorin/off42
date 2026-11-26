package cc.off42.office

import cc.off42.mobile.DocumentInfo
import cc.off42.mobile.FileKind
import cc.off42.mobile.Off42Exception
import cc.off42.mobile.PdfInfo
import cc.off42.mobile.Slide
import cc.off42.mobile.Workbook
import java.security.SecureRandom
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import cc.off42.mobile.documentHtml as coreDocumentHtml
import cc.off42.mobile.documentInfo as coreDocumentInfo
import cc.off42.mobile.fileKind as coreFileKind
import cc.off42.mobile.normalizedExtension as coreExtension
import cc.off42.mobile.pdfInfo as corePdfInfo
import cc.off42.mobile.presentationOutline as coreOutline
import cc.off42.mobile.spreadsheetOpen as coreSpreadsheetOpen

/** Limits of the spreadsheet grid, as in the web app. */
const val MAX_ROWS = 1000
const val MAX_COLS = 100

/**
 * Runs engine calls off the main thread. The engine is synchronous and some calls take seconds on large files;
 * the UI awaits them from coroutines.
 */
suspend fun <T> engine(block: () -> T): T = withContext(Dispatchers.Default) { block() }

private val random = SecureRandom()

/** A random non-zero seed for signature designs and impressions (the engine has no entropy of its own). */
fun randomSeed(): ULong {
    while (true) {
        val value = random.nextLong().toULong()
        if (value != 0UL) return value
    }
}

/** What an opened file shows. */
sealed interface FileContent {
    data class Document(val html: String, val info: DocumentInfo) : FileContent
    data class Spreadsheet(val workbook: Workbook) : FileContent
    data class Presentation(val slides: List<Slide>) : FileContent
    data class Pdf(val info: PdfInfo) : FileContent
}

/** A file opened in this session. Bytes stay in memory; nothing is written unless the user saves or shares. */
data class OpenedFile(
    val id: Long,
    val name: String,
    val extension: String,
    val kind: FileKind,
    val bytes: ByteArray,
    val content: FileContent?,
    val error: String?,
) {
    val stem: String get() = name.substringBeforeLast('.', name)

    override fun equals(other: Any?) = other is OpenedFile && other.id == id && other.content == content && other.error == error
    override fun hashCode() = id.hashCode()
}

/** The engine's extension for a name, falling back to the MIME type for names without one. */
fun extensionFor(name: String, mimeType: String?): String {
    val fromName = coreExtension(name)
    if (coreFileKind(fromName) != null && name.contains('.')) return fromName
    return when (mimeType?.lowercase()) {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" -> "docx"
        "application/vnd.oasis.opendocument.text" -> "odt"
        "application/rtf", "text/rtf" -> "rtf"
        "text/markdown", "text/x-markdown" -> "md"
        "text/html" -> "html"
        "text/plain" -> "txt"
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" -> "xlsx"
        "application/vnd.oasis.opendocument.spreadsheet" -> "ods"
        "application/vnd.ms-excel" -> "xls"
        "text/csv", "text/comma-separated-values" -> "csv"
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" -> "pptx"
        "application/vnd.oasis.opendocument.presentation" -> "odp"
        "application/pdf" -> "pdf"
        else -> fromName
    }
}

fun kindOf(extension: String): FileKind? = coreFileKind(extension)

/** Reads a file with the engine. Call from [engine]. Errors are returned as the file's error text. */
fun load(id: Long, name: String, extension: String, kind: FileKind, bytes: ByteArray): OpenedFile {
    val content: FileContent?
    var error: String? = null
    content = try {
        when (kind) {
            FileKind.DOCUMENT -> FileContent.Document(coreDocumentHtml(bytes, extension), coreDocumentInfo(bytes, extension))
            FileKind.SPREADSHEET ->
                FileContent.Spreadsheet(coreSpreadsheetOpen(bytes, extension, MAX_ROWS.toUInt(), MAX_COLS.toUInt()))
            FileKind.PRESENTATION -> FileContent.Presentation(coreOutline(bytes, extension))
            FileKind.PDF -> FileContent.Pdf(corePdfInfo(bytes))
        }
    } catch (e: Off42Exception) {
        error = e.message
        null
    }
    return OpenedFile(id, name, extension, kind, bytes, content, error)
}

/**
 * The site/web-app language code for a locale (the codes label42 and marks42 use): zh-Hans/zh-Hant by script or
 * region, and the modern codes for Indonesian and Filipino.
 */
fun siteLanguage(locale: java.util.Locale = java.util.Locale.getDefault()): String = when (val language = locale.language) {
    "zh" -> if (locale.script == "Hant" || (locale.script.isEmpty() && locale.country in setOf("TW", "HK", "MO"))) "zh-Hant" else "zh-Hans"
    "in" -> "id"
    "tl" -> "fil"
    else -> language
}

/** Spreadsheet column name: 0 → A, 25 → Z, 26 → AA. */
fun columnName(index: Int): String {
    val name = StringBuilder()
    var n = index + 1
    while (n > 0) {
        name.insert(0, ('A' + (n - 1) % 26))
        n = (n - 1) / 26
    }
    return name.toString()
}

/** Text that reads as a number is right-aligned in the grid, as in the web app. */
fun looksNumeric(text: String): Boolean = text.isNotEmpty() && Regex("^-?[\\d.,\\s%]+$").matches(text)

/** A grid of a sheet's displayed text, sized to its used range within the limits. */
fun sheetGrid(sheet: cc.off42.mobile.Sheet): List<Array<String>> {
    val rows = minOf(maxOf(sheet.rows.toInt(), (sheet.cells.maxOfOrNull { it.row.toInt() } ?: -1) + 1, 1), MAX_ROWS)
    val cols = minOf(maxOf(sheet.cols.toInt(), (sheet.cells.maxOfOrNull { it.col.toInt() } ?: -1) + 1, 1), MAX_COLS)
    val grid = List(rows) { Array(cols) { "" } }
    for (cell in sheet.cells) {
        val r = cell.row.toInt()
        val c = cell.col.toInt()
        if (r < rows && c < cols) grid[r][c] = cell.text
    }
    return grid
}
