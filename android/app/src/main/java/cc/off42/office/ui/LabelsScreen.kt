package cc.off42.office.ui

import android.util.LruCache
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableDoubleStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cc.off42.mobile.LabelSheetRequest
import cc.off42.mobile.LabelStock
import cc.off42.mobile.Off42Exception
import cc.off42.mobile.ProductMark
import cc.off42.mobile.barcodePng
import cc.off42.mobile.barcodeSvg
import cc.off42.mobile.labelSheetPdf
import cc.off42.mobile.labelStocks
import cc.off42.mobile.parseLabelLines
import cc.off42.mobile.pdfInfo
import cc.off42.mobile.productMarkPng
import cc.off42.mobile.productMarkSvg
import cc.off42.mobile.productMarks
import cc.off42.office.Off42ViewModel
import cc.off42.office.R
import cc.off42.office.engine
import cc.off42.office.siteLanguage
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

val CodeFormats = listOf("qr", "datamatrix", "code128", "aztec")

@Composable
fun codeFormatTitle(format: String?): String = when (format) {
    "qr" -> stringResource(R.string.code_qr)
    "datamatrix" -> "Data Matrix"
    "code128" -> "Code 128"
    "aztec" -> "Aztec"
    else -> stringResource(R.string.labels_none)
}

@Composable
fun LabelsScreen(viewModel: Off42ViewModel, wide: Boolean) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    Column(Modifier.fillMaxSize()) {
        PrimaryTabRow(selectedTabIndex = tab) {
            listOf(R.string.tab_codes, R.string.tab_label_sheet, R.string.marks_title).forEachIndexed { i, title ->
                Tab(selected = tab == i, onClick = { tab = i }, text = { Text(stringResource(title), maxLines = 1) },
                    modifier = Modifier.testTag("labels-tab-$i"))
            }
        }
        when (tab) {
            0 -> CodeGenerator(viewModel, wide)
            1 -> LabelSheet(viewModel, wide)
            else -> ProductMarks(viewModel)
        }
    }
}

/** Two columns on large screens (form | preview), one below the other on phones. */
@Composable
fun FormAndPreview(wide: Boolean, form: @Composable () -> Unit, preview: @Composable () -> Unit) {
    if (wide) {
        Row(Modifier.fillMaxSize().padding(16.dp), horizontalArrangement = Arrangement.spacedBy(24.dp)) {
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) { form() }
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) { preview() }
        }
    } else {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            form()
            preview()
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun CodeGenerator(viewModel: Off42ViewModel, wide: Boolean) {
    var format by rememberSaveable { mutableStateOf("qr") }
    var content by rememberSaveable { mutableStateOf("https://off42.cc") }
    var width by rememberSaveable { mutableDoubleStateOf(30.0) }
    var caption by rememberSaveable { mutableStateOf(false) }
    val share = rememberShare()
    val scope = rememberCoroutineScope()
    // Live preview, redrawn shortly after the last change.
    val preview by produceState<Result<ByteArray>?>(null, format, content, width) {
        delay(200)
        value = runCatching { engine { barcodePng(format, content, width.coerceIn(5.0, 200.0)) } }
    }
    FormAndPreview(wide, form = {
        Text(stringResource(R.string.code_title), style = MaterialTheme.typography.titleLarge)
        Choice(stringResource(R.string.code_type), CodeFormats, format, { codeFormatTitle(it) }, { format = it })
        OutlinedTextField(content, { content = it }, label = { Text(stringResource(R.string.code_content)) },
            modifier = Modifier.fillMaxWidth().testTag("code-value"), minLines = 1, maxLines = 4)
        NumberField(stringResource(R.string.width_mm), width, { width = it }, Modifier.fillMaxWidth())
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(caption, { caption = it })
            Text(stringResource(R.string.code_caption), style = MaterialTheme.typography.bodyMedium)
        }
    }, preview = {
        val result = preview
        Card(Modifier.fillMaxWidth().heightIn(min = 240.dp)) {
            when {
                result == null -> Spacer(Modifier.height(240.dp))
                result.isFailure -> ErrorText(result.exceptionOrNull()?.message ?: "", Modifier.padding(16.dp))
                else -> PngImage(result.getOrNull(), stringResource(R.string.code_preview),
                    Modifier.fillMaxWidth().height(260.dp).testTag("code-preview"), checker = false)
            }
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = { result(preview)?.let { viewModel.output("$format.png", it) } }, enabled = preview?.isSuccess == true,
                modifier = Modifier.testTag("code-save-png")) { Text(stringResource(R.string.save_png)) }
            OutlinedButton(onClick = {
                scope.launch {
                    try {
                        viewModel.output("$format.svg", engine { barcodeSvg(format, content, width, caption) }.toByteArray())
                    } catch (e: Off42Exception) {
                        viewModel.notify(e.message ?: "", error = true)
                    }
                }
            }, enabled = preview?.isSuccess == true) { Text(stringResource(R.string.save_svg)) }
            OutlinedButton(onClick = { result(preview)?.let { share("$format.png", "image/png", it) } },
                enabled = preview?.isSuccess == true) { Text(stringResource(R.string.share)) }
        }
    })
}

private fun result(value: Result<ByteArray>?): ByteArray? = value?.getOrNull()

private const val CUSTOM = "custom"

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun LabelSheet(viewModel: Off42ViewModel, wide: Boolean) {
    val stocks by produceState<List<LabelStock>>(emptyList()) { value = runCatching { engine { labelStocks() } }.getOrDefault(emptyList()) }
    var stockId by rememberSaveable { mutableStateOf<String?>(null) }
    var picking by remember { mutableStateOf(false) }
    var customWidth by rememberSaveable { mutableDoubleStateOf(70.0) }
    var customHeight by rememberSaveable { mutableDoubleStateOf(37.0) }
    var gapX by rememberSaveable { mutableDoubleStateOf(0.0) }
    var gapY by rememberSaveable { mutableDoubleStateOf(0.0) }
    var first by rememberSaveable { mutableDoubleStateOf(1.0) }
    var codeFormat by rememberSaveable { mutableStateOf<String?>("qr") }
    var lines by rememberSaveable { mutableStateOf("Box 1 | BOX-1 | 3\nBox 2 | BOX-2\nShelf A") }
    var pdf by remember { mutableStateOf<ByteArray?>(null) }
    var status by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val share = rememberShare()
    val stock = stocks.firstOrNull { it.id == stockId } ?: stocks.firstOrNull()
    val statusFormat = stringResource(R.string.labels_status)

    FormAndPreview(wide, form = {
        Text(stringResource(R.string.labels_title), style = MaterialTheme.typography.titleLarge)
        OutlinedButton(onClick = { picking = true }, modifier = Modifier.fillMaxWidth().testTag("stock-picker")) {
            Text(
                if (stockId == CUSTOM) stringResource(R.string.labels_custom) else stock?.let(::stockTitle) ?: "…",
                maxLines = 2, overflow = TextOverflow.Ellipsis,
            )
        }
        if (stockId == CUSTOM) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                NumberField(stringResource(R.string.width_mm), customWidth, { customWidth = it }, Modifier.width(140.dp))
                NumberField(stringResource(R.string.height_mm), customHeight, { customHeight = it }, Modifier.width(140.dp))
                NumberField(stringResource(R.string.labels_gap_x), gapX, { gapX = it }, Modifier.width(140.dp))
                NumberField(stringResource(R.string.labels_gap_y), gapY, { gapY = it }, Modifier.width(140.dp))
            }
        }
        NumberField(stringResource(R.string.labels_first), first, { first = it }, Modifier.fillMaxWidth())
        Choice(stringResource(R.string.labels_code), listOf(null) + CodeFormats, codeFormat, { codeFormatTitle(it) }, { codeFormat = it })
        OutlinedTextField(lines, { lines = it }, label = { Text(stringResource(R.string.labels_lines)) },
            supportingText = { Text(stringResource(R.string.labels_lines_format)) },
            modifier = Modifier.fillMaxWidth().testTag("label-lines"), minLines = 4)
        Button(onClick = {
            scope.launch {
                error = null
                try {
                    val labels = parseLabelLines(lines)
                    val request = LabelSheetRequest(
                        stock = if (stockId == CUSTOM) CUSTOM else stock?.id ?: "",
                        width = customWidth, height = customHeight, gapX = gapX, gapY = gapY,
                        firstSlot = first.toInt().coerceAtLeast(0).toUInt(), codeFormat = codeFormat, labels = labels,
                    )
                    val bytes = engine { labelSheetPdf(request) }
                    val pages = engine { pdfInfo(bytes).pages }
                    pdf = bytes
                    status = String.format(statusFormat, labels.sumOf { it.copies.toInt() }, pages.toInt())
                } catch (e: Off42Exception) {
                    pdf = null
                    error = e.message
                }
            }
        }, modifier = Modifier.testTag("make-sheet")) { Text(stringResource(R.string.labels_make)) }
    }, preview = {
        error?.let { ErrorText(it) }
        val bytes = pdf
        if (bytes != null) {
            Text(status, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.testTag("sheet-status"))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = { viewModel.output("labels.pdf", bytes) }, modifier = Modifier.testTag("sheet-save")) {
                    Text(stringResource(R.string.save_pdf))
                }
                OutlinedButton(onClick = { share("labels.pdf", "application/pdf", bytes) }) { Text(stringResource(R.string.share)) }
            }
            rememberPdfPages(bytes, bytes)?.getOrNull()?.let { pages -> PdfPage(pages, 0, maxWidth = 520.dp) }
        }
    })

    if (picking) {
        StockPicker(stocks, onPick = { stockId = it; picking = false }, onDismiss = { picking = false })
    }
}

private fun stockTitle(s: LabelStock): String {
    fun mm(v: Double) = if (v == Math.floor(v)) v.toLong().toString() else v.toString()
    return "${s.name} — ${s.columns}×${s.rows}, ${mm(s.width)}×${mm(s.height)} mm"
}

@Composable
private fun StockPicker(stocks: List<LabelStock>, onPick: (String) -> Unit, onDismiss: () -> Unit) {
    var query by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.labels_stock)) },
        text = {
            Column {
                OutlinedTextField(query, { query = it }, label = { Text(stringResource(R.string.marks_search)) }, singleLine = true,
                    modifier = Modifier.fillMaxWidth())
                LazyColumn(Modifier.heightIn(max = 420.dp)) {
                    item {
                        ListItem(headlineContent = { Text(stringResource(R.string.labels_custom)) }, modifier = Modifier.clickable { onPick(CUSTOM) })
                        HorizontalDivider()
                    }
                    val q = query.trim().lowercase()
                    items(stocks.filter { q.isEmpty() || stockTitle(it).lowercase().contains(q) }, key = { it.id }) { s ->
                        ListItem(headlineContent = { Text(stockTitle(s)) }, modifier = Modifier.clickable { onPick(s.id) })
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun ProductMarks(viewModel: Off42ViewModel) {
    val language = siteLanguage(androidx.compose.ui.platform.LocalConfiguration.current.locales[0])
    val marks by produceState<List<ProductMark>>(emptyList(), language) {
        value = runCatching { engine { productMarks(language) } }.getOrDefault(emptyList())
    }
    var query by rememberSaveable { mutableStateOf("") }
    var category by rememberSaveable { mutableStateOf<String?>(null) }
    var detail by remember { mutableStateOf<ProductMark?>(null) }
    val thumbnails = remember { LruCache<String, ByteArray>(160) }
    val all = stringResource(R.string.marks_all)
    Column(Modifier.fillMaxSize()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.marks_hint), style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(query, { query = it }, label = { Text(stringResource(R.string.marks_search)) },
                    placeholder = { Text(stringResource(R.string.marks_search_placeholder)) }, singleLine = true,
                    modifier = Modifier.weight(1f).testTag("marks-search"))
                Choice(stringResource(R.string.marks_category), listOf(null) + marks.map { it.category }.distinct().sorted(), category,
                    { it ?: all }, { category = it }, Modifier.weight(1f))
            }
        }
        val q = query.trim().lowercase()
        val shown = marks.filter {
            (category == null || it.category == category) && (q.isEmpty() || "${it.name} ${it.purpose} ${it.id}".lowercase().contains(q))
        }
        LazyVerticalGrid(GridCells.Adaptive(112.dp), Modifier.fillMaxSize().testTag("marks-grid"),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(12.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            items(shown, key = { it.id }) { mark ->
                val png by produceState(thumbnails.get(mark.id), mark.id) {
                    if (value == null) {
                        value = runCatching { engine { productMarkPng(mark.id, 12.0) } }.getOrNull()?.also { thumbnails.put(mark.id, it) }
                    }
                }
                Card(Modifier.clickable { detail = mark }.testTag("mark-${mark.id}")) {
                    Column(Modifier.padding(8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        PngImage(png, mark.name, Modifier.size(72.dp))
                        Text(mark.name, style = MaterialTheme.typography.labelSmall, maxLines = 2, overflow = TextOverflow.Ellipsis,
                            textAlign = TextAlign.Center)
                    }
                }
            }
        }
    }
    detail?.let { mark -> MarkDetail(viewModel, mark) { detail = null } }
}

@Composable
private fun MarkDetail(viewModel: Off42ViewModel, mark: ProductMark, onDismiss: () -> Unit) {
    var width by rememberSaveable(mark.id) { mutableDoubleStateOf(20.0) }
    val scope = rememberCoroutineScope()
    val preview by produceState<ByteArray?>(null, mark.id) { value = runCatching { engine { productMarkPng(mark.id, 20.0) } }.getOrNull() }
    val save: (Boolean) -> Unit = { svg ->
        scope.launch {
            try {
                val w = width.coerceIn(3.0, 300.0)
                if (svg) viewModel.output("${mark.id}.svg", engine { productMarkSvg(mark.id, w) }.toByteArray())
                else viewModel.output("${mark.id}.png", engine { productMarkPng(mark.id, w) })
            } catch (e: Off42Exception) {
                viewModel.notify(e.message ?: "", error = true)
            }
            onDismiss()
        }
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(mark.name) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                PngImage(preview, mark.name, Modifier.fillMaxWidth().aspectRatio(2f))
                Text(mark.purpose, style = MaterialTheme.typography.bodyMedium)
                mark.warning?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
                if (mark.unverified) Text(stringResource(R.string.marks_unverified), color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall)
                if (mark.needsNumber) Text(stringResource(R.string.marks_needs_number), style = MaterialTheme.typography.bodySmall)
                NumberField(stringResource(R.string.width_mm), width, { width = it }, Modifier.widthIn(max = 200.dp))
            }
        },
        confirmButton = { TextButton(onClick = { save(false) }) { Text(stringResource(R.string.save_png)) } },
        dismissButton = { TextButton(onClick = { save(true) }) { Text(stringResource(R.string.save_svg)) } },
    )
}
