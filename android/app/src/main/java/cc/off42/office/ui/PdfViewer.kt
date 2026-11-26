package cc.off42.office.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Color as AndroidColor
import android.graphics.pdf.PdfRenderer
import android.os.ParcelFileDescriptor
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import cc.off42.mobile.PdfInfo
import cc.off42.office.Off42ViewModel
import cc.off42.office.OpenedFile
import cc.off42.office.R
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/**
 * Renders PDF pages on the device with Android's PdfRenderer. It needs a file descriptor, so the bytes are copied
 * to a private cache file for as long as the pages are shown. One page is open at a time (PdfRenderer is not
 * thread-safe).
 */
class PdfPages private constructor(private val file: File, private val renderer: PdfRenderer) : AutoCloseable {
    private val lock = Mutex()
    val count: Int get() = renderer.pageCount

    suspend fun aspect(index: Int): Float = lock.withLock {
        withContext(Dispatchers.IO) { renderer.openPage(index).use { it.width.toFloat() / it.height.toFloat() } }
    }

    suspend fun render(index: Int, widthPx: Int): Bitmap = lock.withLock {
        withContext(Dispatchers.IO) {
            renderer.openPage(index).use { page ->
                val width = widthPx.coerceIn(64, 2400)
                val height = (width.toLong() * page.height / page.width).toInt().coerceAtLeast(1)
                val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
                bitmap.eraseColor(AndroidColor.WHITE)
                page.render(bitmap, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                bitmap
            }
        }
    }

    override fun close() {
        runCatching { renderer.close() }
        file.delete()
    }

    companion object {
        /** Opens PDF bytes, or returns the reason PdfRenderer refused them (for example a password). */
        suspend fun open(context: Context, bytes: ByteArray): Result<PdfPages> = withContext(Dispatchers.IO) {
            val folder = File(context.cacheDir, "render").apply { mkdirs() }
            val file = File.createTempFile("page", ".pdf", folder)
            runCatching {
                file.writeBytes(bytes)
                PdfPages(file, PdfRenderer(ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)))
            }.onFailure { file.delete() }
        }
    }
}

/** Opens PDF bytes for rendering while the composable is shown. */
@Composable
fun rememberPdfPages(key: Any, bytes: ByteArray): Result<PdfPages>? {
    val context = LocalContext.current
    val pages by produceState<Result<PdfPages>?>(null, key) { value = PdfPages.open(context, bytes) }
    DisposableEffect(pages) { onDispose { pages?.getOrNull()?.close() } }
    return pages
}

@Composable
fun PdfViewer(viewModel: Off42ViewModel, file: OpenedFile, info: PdfInfo) {
    val share = rememberShare()
    if (info.encrypted) {
        Text(stringResource(R.string.pdf_encrypted), Modifier.padding(16.dp).testTag("pdf-encrypted"))
        return
    }
    val pages = rememberPdfPages(file.id, file.bytes)
    LazyColumn(Modifier.fillMaxSize().testTag("pdf-pages"), contentPadding = PaddingValues(bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        item {
            Column(Modifier.fillMaxWidth()) {
                SaveAsRow(listOf("pdf"), extra = {
                    androidx.compose.material3.OutlinedButton(onClick = { share(file.name, "application/pdf", file.bytes) }) {
                        Text(stringResource(R.string.share))
                    }
                }) { viewModel.output(file.name, file.bytes) }
                PdfInfoTable(info)
            }
        }
        when (val result = pages) {
            null -> item { CircularProgressIndicator(Modifier.padding(24.dp)) }
            else -> result.fold(
                onSuccess = { renderer -> items((0 until renderer.count).toList(), key = { it }) { PdfPage(renderer, it) } },
                onFailure = { e -> item { ErrorText(e.message ?: e.toString(), Modifier.padding(16.dp)) } },
            )
        }
    }
}

@Composable
private fun PdfInfoTable(info: PdfInfo) {
    val rows = buildList {
        add(stringResource(R.string.pdf_pages) to info.pages.toString())
        info.metadata.forEach { add(it.key to it.value) }
        if (info.bookmarks.isNotEmpty()) add(stringResource(R.string.pdf_bookmarks) to info.bookmarks.joinToString(" · "))
        if (info.attachments.isNotEmpty()) add(stringResource(R.string.pdf_attachments) to info.attachments.joinToString(", "))
        if (info.formFields.isNotEmpty()) add(stringResource(R.string.pdf_form_fields) to info.formFields.joinToString(", ") { it.name })
    }
    Column(Modifier.padding(horizontal = 16.dp).testTag("pdf-info"), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        rows.forEach { (key, value) ->
            Row {
                Text(key, Modifier.widthIn(min = 120.dp, max = 160.dp).padding(end = 12.dp), fontWeight = FontWeight.SemiBold,
                    style = MaterialTheme.typography.bodySmall)
                Text(value, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}

/** One page, rendered at the width it is shown. */
@Composable
fun PdfPage(pages: PdfPages, index: Int, maxWidth: androidx.compose.ui.unit.Dp = 900.dp) {
    val label = stringResource(R.string.pdf_page, index + 1, pages.count)
    BoxWithConstraints(Modifier.fillMaxWidth().padding(horizontal = 16.dp), contentAlignment = Alignment.Center) {
        val width = minOf(this.maxWidth, maxWidth)
        val px = with(LocalDensity.current) { width.roundToPx() }
        val aspect by produceState(0.7071f, pages, index) { value = pages.aspect(index) }
        val bitmap by produceState<Bitmap?>(null, pages, index, px) { value = pages.render(index, px) }
        Surface(shadowElevation = 2.dp, modifier = Modifier.widthIn(max = width).fillMaxWidth().aspectRatio(aspect)
            .semantics { contentDescription = label }.testTag("pdf-page-$index")) {
            Box(Modifier.background(Color.White), contentAlignment = Alignment.Center) {
                val image = remember(bitmap) { bitmap?.asImageBitmap() }
                if (image != null) Image(image, contentDescription = null, modifier = Modifier.fillMaxSize())
                else CircularProgressIndicator()
            }
        }
    }
}
