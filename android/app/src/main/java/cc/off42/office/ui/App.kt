package cc.off42.office.ui

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContract
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.Article
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.outlined.Draw
import androidx.compose.material.icons.outlined.FileOpen
import androidx.compose.material.icons.outlined.PictureAsPdf
import androidx.compose.material.icons.outlined.QrCode2
import androidx.compose.material.icons.outlined.Slideshow
import androidx.compose.material.icons.outlined.TableChart
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.windowsizeclass.WindowSizeClass
import androidx.compose.material3.windowsizeclass.WindowWidthSizeClass
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import cc.off42.mobile.FileKind
import cc.off42.office.FileContent
import cc.off42.office.Off42ViewModel
import cc.off42.office.OpenedFile
import cc.off42.office.PendingOutput
import cc.off42.office.R
import cc.off42.office.Section
import cc.off42.office.fileKind
import java.io.File

val Section.icon: ImageVector
    get() = when (this) {
        Section.DOCUMENTS -> Icons.AutoMirrored.Outlined.Article
        Section.SPREADSHEETS -> Icons.Outlined.TableChart
        Section.PRESENTATIONS -> Icons.Outlined.Slideshow
        Section.PDF -> Icons.Outlined.PictureAsPdf
        Section.LABELS -> Icons.Outlined.QrCode2
        Section.MARKS -> Icons.Outlined.Draw
    }

val Section.title: Int
    get() = when (this) {
        Section.DOCUMENTS -> R.string.section_documents
        Section.SPREADSHEETS -> R.string.section_spreadsheets
        Section.PRESENTATIONS -> R.string.section_presentations
        Section.PDF -> R.string.section_pdf
        Section.LABELS -> R.string.section_labels
        Section.MARKS -> R.string.section_marks
    }

/** Short labels for the phone navigation bar. */
val Section.shortTitle: Int
    get() = when (this) {
        Section.DOCUMENTS -> R.string.nav_documents
        Section.SPREADSHEETS -> R.string.nav_spreadsheets
        Section.PRESENTATIONS -> R.string.nav_presentations
        Section.PDF -> R.string.section_pdf
        Section.LABELS -> R.string.nav_labels
        Section.MARKS -> R.string.nav_marks
    }

/** MIME types offered in the system picker for each kind (octet-stream: files that providers do not type). */
fun pickerTypes(kind: FileKind): Array<String> = when (kind) {
    FileKind.DOCUMENT -> arrayOf(
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/vnd.oasis.opendocument.text", "application/rtf", "text/rtf", "text/markdown", "text/x-markdown",
        "text/html", "text/plain", "application/octet-stream",
    )
    FileKind.SPREADSHEET -> arrayOf(
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "application/vnd.oasis.opendocument.spreadsheet", "application/vnd.ms-excel", "text/csv",
        "text/comma-separated-values", "application/octet-stream",
    )
    FileKind.PRESENTATION -> arrayOf(
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "application/vnd.oasis.opendocument.presentation", "application/octet-stream",
    )
    FileKind.PDF -> arrayOf("application/pdf")
}

/** ACTION_CREATE_DOCUMENT with the file's own MIME type and suggested name. */
private class CreateDocument : ActivityResultContract<PendingOutput, Uri?>() {
    override fun createIntent(context: Context, input: PendingOutput) = Intent(Intent.ACTION_CREATE_DOCUMENT)
        .addCategory(Intent.CATEGORY_OPENABLE)
        .setType(input.mimeType)
        .putExtra(Intent.EXTRA_TITLE, input.name)

    override fun parseResult(resultCode: Int, intent: Intent?): Uri? = intent?.data
}

/** Hands bytes to the share sheet through the app's FileProvider (cache/shared only). */
fun share(context: Context, name: String, mimeType: String, bytes: ByteArray) {
    val folder = File(context.cacheDir, "shared").apply { mkdirs() }
    folder.listFiles()?.forEach { it.delete() }
    val safe = name.replace(Regex("[\\\\/:*?\"<>|]"), "_")
    val file = File(folder, safe).apply { writeBytes(bytes) }
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    val send = Intent(Intent.ACTION_SEND)
        .setType(mimeType)
        .putExtra(Intent.EXTRA_STREAM, uri)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(Intent.createChooser(send, null))
}

@Composable
fun Off42App(viewModel: Off42ViewModel, windowSize: WindowSizeClass) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val width = windowSize.widthSizeClass
    val snackbar = remember { SnackbarHostState() }

    // Saving: the picker is launched once per pending output, also across rotation.
    val saver = rememberLauncherForActivityResult(CreateDocument()) { viewModel.writePending(it) }
    var launchedFor by rememberSaveable { mutableLongStateOf(0L) }
    val pending = state.pendingOutput
    LaunchedEffect(pending) {
        if (pending != null) {
            val key = pending.name.hashCode().toLong() * 31 + pending.bytes.size
            if (launchedFor != key) {
                launchedFor = key
                saver.launch(pending)
            }
        } else {
            launchedFor = 0L
        }
    }
    LaunchedEffect(state.notice) {
        val notice = state.notice ?: return@LaunchedEffect
        snackbar.showSnackbar(notice.text, duration = if (notice.error) SnackbarDuration.Long else SnackbarDuration.Short)
        viewModel.noticeShown()
    }
    state.question?.let { question ->
        AlertDialog(
            onDismissRequest = { viewModel.answer(question, false) },
            title = { Text(stringResource(R.string.convert_title, question.to.uppercase())) },
            text = { Text(stringResource(R.string.doc_convert_anyway, question.error, question.to.uppercase())) },
            confirmButton = {
                TextButton(onClick = { viewModel.answer(question, true) }, modifier = Modifier.testTag("convert-anyway")) {
                    Text(stringResource(R.string.convert))
                }
            },
            dismissButton = { TextButton(onClick = { viewModel.answer(question, false) }) { Text(stringResource(R.string.cancel)) } },
        )
    }

    val content: @Composable (Modifier) -> Unit = { modifier ->
        Column(modifier) {
            if (state.busy) LinearProgressIndicator(Modifier.fillMaxWidth().testTag("busy"))
            else Spacer(Modifier.padding(top = 4.dp))
            val section = state.section
            val kind = section.fileKind()
            Box(Modifier.weight(1f)) {
                when {
                    kind != null -> FileSection(viewModel, state.filesOf(kind), state.selectedFile(kind), kind, section,
                        twoPane = width == WindowWidthSizeClass.Expanded)
                    section == Section.LABELS -> LabelsScreen(viewModel, wide = width != WindowWidthSizeClass.Compact)
                    else -> MarksScreen(viewModel, wide = width != WindowWidthSizeClass.Compact)
                }
            }
        }
    }

    if (width == WindowWidthSizeClass.Compact) {
        Scaffold(
            snackbarHost = { SnackbarHost(snackbar) },
            bottomBar = {
                NavigationBar {
                    Section.entries.forEach { section ->
                        NavigationBarItem(
                            selected = state.section == section,
                            onClick = { viewModel.show(section) },
                            icon = { Icon(section.icon, contentDescription = null) },
                            label = { Text(stringResource(section.shortTitle), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                            alwaysShowLabel = false,
                            modifier = Modifier.testTag("nav-${section.name}"),
                        )
                    }
                }
            },
        ) { padding -> content(Modifier.padding(padding).fillMaxSize()) }
    } else {
        Scaffold(snackbarHost = { SnackbarHost(snackbar) }, contentWindowInsets = WindowInsets(0)) { padding ->
            Row(Modifier.padding(padding).fillMaxSize()) {
                NavigationRail(Modifier.windowInsetsPadding(WindowInsets.safeDrawing)) {
                    Spacer(Modifier.padding(top = 8.dp))
                    Section.entries.forEach { section ->
                        NavigationRailItem(
                            selected = state.section == section,
                            onClick = { viewModel.show(section) },
                            icon = { Icon(section.icon, contentDescription = null) },
                            label = { Text(stringResource(section.shortTitle), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                            modifier = Modifier.testTag("nav-${section.name}"),
                        )
                    }
                }
                content(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing))
            }
        }
    }
}

/** A file section: the opened files of one kind and the selected file's viewer (side by side on large screens). */
@Composable
private fun FileSection(
    viewModel: Off42ViewModel,
    files: List<OpenedFile>,
    selected: OpenedFile?,
    kind: FileKind,
    section: Section,
    twoPane: Boolean,
) {
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) viewModel.open(uri)
    }
    val open = { picker.launch(pickerTypes(kind)) }
    if (twoPane) {
        Row(Modifier.fillMaxSize()) {
            FileList(files, selected, section, open, { viewModel.select(kind, it.id) }, { viewModel.close(it) },
                Modifier.width(320.dp).fillMaxHeight())
            VerticalDivider()
            Box(Modifier.weight(1f).fillMaxHeight()) {
                if (selected != null) FileDetail(viewModel, selected, onBack = null)
                else EmptyDetail(section, open)
            }
        }
    } else if (selected != null) {
        BackHandler { viewModel.select(kind, null) }
        FileDetail(viewModel, selected, onBack = { viewModel.select(kind, null) })
    } else {
        FileList(files, null, section, open, { viewModel.select(kind, it.id) }, { viewModel.close(it) }, Modifier.fillMaxSize())
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FileList(
    files: List<OpenedFile>,
    selected: OpenedFile?,
    section: Section,
    open: () -> Unit,
    onSelect: (OpenedFile) -> Unit,
    onClose: (OpenedFile) -> Unit,
    modifier: Modifier,
) {
    LazyColumn(modifier.testTag("file-list")) {
        item {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(stringResource(section.title), style = MaterialTheme.typography.headlineSmall)
                Button(onClick = open, modifier = Modifier.testTag("open-file")) {
                    Icon(Icons.Outlined.FileOpen, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.open_file))
                }
                Text(stringResource(R.string.formats_hint, formatsOf(section)), style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            HorizontalDivider()
        }
        if (files.isEmpty()) {
            item {
                Text(stringResource(R.string.no_files), Modifier.padding(16.dp), style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(stringResource(R.string.privacy_note), Modifier.padding(horizontal = 16.dp),
                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        items(files, key = { it.id }) { file ->
            ListItem(
                headlineContent = { Text(file.name, maxLines = 2, overflow = TextOverflow.Ellipsis) },
                supportingContent = { Text(sizeText(file.bytes.size)) },
                trailingContent = {
                    IconButton(onClick = { onClose(file) }) {
                        Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.close_file))
                    }
                },
                colors = if (file.id == selected?.id) ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.secondaryContainer)
                else ListItemDefaults.colors(),
                modifier = Modifier.clickable { onSelect(file) }.testTag("file-${file.name}"),
            )
        }
    }
}

private fun formatsOf(section: Section) = when (section) {
    Section.DOCUMENTS -> "DOCX, ODT, RTF, Markdown, HTML, TXT"
    Section.SPREADSHEETS -> "XLSX, ODS, XLS, CSV"
    Section.PRESENTATIONS -> "PPTX, ODP"
    else -> "PDF"
}

@Composable
fun sizeText(bytes: Int): String =
    stringResource(R.string.file_size, String.format(LocalConfiguration.current.locales[0], "%.1f", bytes / 1024.0))

@Composable
private fun EmptyDetail(section: Section, open: () -> Unit) {
    Column(Modifier.fillMaxSize().padding(32.dp), verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally) {
        Icon(section.icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(bottom = 16.dp))
        Text(stringResource(R.string.no_files), style = MaterialTheme.typography.bodyLarge)
        Spacer(Modifier.padding(8.dp))
        OutlinedButton(onClick = open) { Text(stringResource(R.string.open_file)) }
        Spacer(Modifier.padding(8.dp))
        Text(stringResource(R.string.privacy_note), style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun FileDetail(viewModel: Off42ViewModel, file: OpenedFile, onBack: (() -> Unit)?) {
    Column(Modifier.fillMaxSize().testTag("file-detail")) {
        Surface(tonalElevation = 2.dp) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                if (onBack != null) {
                    IconButton(onClick = onBack, modifier = Modifier.testTag("back")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                } else {
                    Spacer(Modifier.width(8.dp))
                }
                Column(Modifier.weight(1f).padding(vertical = 8.dp)) {
                    Text(file.name, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold,
                        maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.testTag("file-title"))
                    Text(sizeText(file.bytes.size), style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        val error = file.error
        when (val content = file.content) {
            null -> ErrorText(error ?: "", Modifier.padding(16.dp))
            is FileContent.Document -> DocumentViewer(viewModel, file, content)
            is FileContent.Spreadsheet -> SpreadsheetViewer(viewModel, file, content.workbook)
            is FileContent.Presentation -> PresentationViewer(viewModel, file, content.slides)
            is FileContent.Pdf -> PdfViewer(viewModel, file, content.info)
        }
    }
}

@Composable
fun ErrorText(text: String, modifier: Modifier = Modifier) {
    Text(text, modifier.testTag("error"), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
}

/** "Save as" buttons for the formats a file converts to. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SaveAsRow(formats: List<String>, extra: @Composable () -> Unit = {}, onSave: (String) -> Unit) {
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
        itemVerticalAlignment = Alignment.CenterVertically,
    ) {
        Text(stringResource(R.string.save_as), style = MaterialTheme.typography.labelLarge)
        formats.forEach { format ->
            FilledTonalButton(onClick = { onSave(format) }, modifier = Modifier.testTag("save-$format")) { Text(format.uppercase()) }
        }
        extra()
    }
}

/** The current context, for sharing. */
@Composable
fun rememberShare(): (String, String, ByteArray) -> Unit {
    val context = LocalContext.current
    return remember(context) { { name, mime, bytes -> share(context, name, mime, bytes) } }
}
