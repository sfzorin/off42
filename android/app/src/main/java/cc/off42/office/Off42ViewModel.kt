package cc.off42.office

import android.app.Application
import android.net.Uri
import android.provider.OpenableColumns
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import cc.off42.mobile.FileKind
import cc.off42.mobile.Off42Exception
import cc.off42.mobile.mimeType
import cc.off42.mobile.presentationConvert
import cc.off42.mobile.spreadsheetConvert
import cc.off42.mobile.spreadsheetValuesCsv
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import cc.off42.mobile.documentConvert as coreDocumentConvert

enum class Section { DOCUMENTS, SPREADSHEETS, PRESENTATIONS, PDF, LABELS, MARKS }

fun sectionFor(kind: FileKind): Section = when (kind) {
    FileKind.DOCUMENT -> Section.DOCUMENTS
    FileKind.SPREADSHEET -> Section.SPREADSHEETS
    FileKind.PRESENTATION -> Section.PRESENTATIONS
    FileKind.PDF -> Section.PDF
}

fun Section.fileKind(): FileKind? = when (this) {
    Section.DOCUMENTS -> FileKind.DOCUMENT
    Section.SPREADSHEETS -> FileKind.SPREADSHEET
    Section.PRESENTATIONS -> FileKind.PRESENTATION
    Section.PDF -> FileKind.PDF
    else -> null
}

/** Bytes waiting for the user to pick a destination (ACTION_CREATE_DOCUMENT). */
data class PendingOutput(val name: String, val mimeType: String, val bytes: ByteArray) {
    override fun equals(other: Any?) = other is PendingOutput && other.name == name && other.bytes.contentEquals(bytes)
    override fun hashCode() = name.hashCode()
}

/** A document conversion that would drop package parts, waiting for the user's consent. */
data class ConversionQuestion(val fileId: Long, val to: String, val error: String)

/** A message for the snackbar; [error] messages are shown longer. */
data class Notice(val text: String, val error: Boolean, val id: Long = System.nanoTime())

data class UiState(
    val section: Section = Section.DOCUMENTS,
    val files: List<OpenedFile> = emptyList(),
    val selected: Map<FileKind, Long> = emptyMap(),
    val busy: Boolean = false,
    val notice: Notice? = null,
    val pendingOutput: PendingOutput? = null,
    val question: ConversionQuestion? = null,
) {
    fun filesOf(kind: FileKind) = files.filter { it.kind == kind }
    fun selectedFile(kind: FileKind): OpenedFile? = selected[kind]?.let { id -> files.firstOrNull { it.id == id } }
}

class Off42ViewModel(application: Application) : AndroidViewModel(application) {
    private val _state = MutableStateFlow(UiState())
    val state: StateFlow<UiState> = _state.asStateFlow()
    private var nextId = 1L

    private val app get() = getApplication<Application>()

    fun show(section: Section) = _state.update { it.copy(section = section) }

    fun select(kind: FileKind, id: Long?) = _state.update {
        it.copy(selected = if (id == null) it.selected - kind else it.selected + (kind to id))
    }

    fun close(file: OpenedFile) = _state.update { s ->
        s.copy(files = s.files - file, selected = if (s.selected[file.kind] == file.id) s.selected - file.kind else s.selected)
    }

    fun notify(text: String, error: Boolean = false) = _state.update { it.copy(notice = Notice(text, error)) }

    fun noticeShown() = _state.update { it.copy(notice = null) }

    /** Opens a file chosen in the system picker or received through "Open with"/share. */
    fun open(uri: Uri, mimeHint: String? = null) {
        viewModelScope.launch {
            busy(true)
            try {
                val (name, bytes) = withContext(Dispatchers.IO) {
                    val resolver = app.contentResolver
                    val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                        if (c.moveToFirst()) c.getString(0) else null
                    } ?: uri.lastPathSegment?.substringAfterLast('/') ?: "file"
                    val bytes = resolver.openInputStream(uri)?.use { it.readBytes() }
                        ?: throw java.io.IOException(app.getString(R.string.file_unreadable))
                    name to bytes
                }
                openBytes(name, mimeHint ?: app.contentResolver.getType(uri), bytes)
            } catch (e: Exception) {
                notify(e.message ?: e.toString(), error = true)
            } finally {
                busy(false)
            }
        }
    }

    /** Opens bytes under a name; also the entry point of the instrumented tests. */
    suspend fun openBytes(name: String, mimeType: String?, bytes: ByteArray): OpenedFile? {
        val extension = extensionFor(name, mimeType)
        val kind = kindOf(extension)
        if (kind == null) {
            notify(app.getString(R.string.file_unsupported, extension), error = true)
            return null
        }
        busy(true)
        try {
            val id = nextId++
            val file = engine { load(id, name, extension, kind, bytes) }
            _state.update { it.copy(files = it.files + file, selected = it.selected + (kind to id), section = sectionFor(kind)) }
            return file
        } finally {
            busy(false)
        }
    }

    // MARK: - Saving

    /** Converts an opened file and asks for a destination. */
    fun saveAs(file: OpenedFile, to: String, dropPackageParts: Boolean = false) {
        viewModelScope.launch {
            busy(true)
            try {
                val bytes = engine {
                    when (file.kind) {
                        FileKind.DOCUMENT -> coreDocumentConvert(file.bytes, file.extension, to, dropPackageParts)
                        FileKind.SPREADSHEET -> spreadsheetConvert(file.bytes, file.extension, to)
                        FileKind.PRESENTATION -> presentationConvert(file.bytes, file.extension, to)
                        FileKind.PDF -> file.bytes
                    }
                }
                output("${file.stem}.$to", bytes)
            } catch (e: Off42Exception.PackagePartsWouldBeLost) {
                _state.update { it.copy(question = ConversionQuestion(file.id, to, e.message ?: "")) }
            } catch (e: Off42Exception) {
                notify(e.message ?: e.toString(), error = true)
            } finally {
                busy(false)
            }
        }
    }

    fun answer(question: ConversionQuestion, convert: Boolean) {
        _state.update { it.copy(question = null) }
        if (!convert) return
        _state.value.files.firstOrNull { it.id == question.fileId }?.let { saveAs(it, question.to, dropPackageParts = true) }
    }

    fun saveSheetValues(file: OpenedFile, sheet: Int, sheetName: String) {
        viewModelScope.launch {
            busy(true)
            try {
                output("${file.stem} - $sheetName.csv", engine { spreadsheetValuesCsv(file.bytes, file.extension, sheet.toUInt()) })
            } catch (e: Off42Exception) {
                notify(e.message ?: e.toString(), error = true)
            } finally {
                busy(false)
            }
        }
    }

    /** Asks for a destination for generated bytes. */
    fun output(name: String, bytes: ByteArray) = _state.update {
        it.copy(pendingOutput = PendingOutput(name, mimeType(name.substringAfterLast('.')), bytes))
    }

    /** Writes the pending output to the destination the user picked (null: cancelled). */
    fun writePending(uri: Uri?) {
        val pending = _state.value.pendingOutput ?: return
        _state.update { it.copy(pendingOutput = null) }
        if (uri == null) return
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    app.contentResolver.openOutputStream(uri, "wt")?.use { it.write(pending.bytes) }
                        ?: throw java.io.IOException(app.getString(R.string.file_unwritable))
                }
                notify(app.getString(R.string.saved, pending.name))
            } catch (e: Exception) {
                notify(e.message ?: e.toString(), error = true)
            }
        }
    }

    private fun busy(value: Boolean) = _state.update { it.copy(busy = value) }
}
