package cc.off42.office.ui

import android.annotation.SuppressLint
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import cc.off42.mobile.saveFormats
import cc.off42.office.FileContent
import cc.off42.office.Off42ViewModel
import cc.off42.office.OpenedFile
import cc.off42.office.R
import java.io.ByteArrayInputStream

@Composable
fun DocumentViewer(viewModel: Off42ViewModel, file: OpenedFile, content: FileContent.Document) {
    Column(Modifier.fillMaxSize()) {
        SaveAsRow(saveFormats(file.kind)) { viewModel.saveAs(file, it) }
        val info = content.info
        Text(
            stringResource(R.string.doc_stats, info.words.toString(), info.characters.toString(), info.paragraphs.toString()),
            Modifier.padding(horizontal = 16.dp).testTag("doc-stats"),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        val description = stringResource(R.string.doc_preview)
        DocumentPreview(content.html, Modifier.weight(1f).padding(top = 8.dp).semantics { contentDescription = description }
            .testTag("doc-preview"))
    }
}

/**
 * The engine's HTML in a WebView that runs no scripts and loads nothing: no network (the app has no INTERNET
 * permission, and every subresource request is answered empty), no file or content access.
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
private fun DocumentPreview(html: String, modifier: Modifier) {
    AndroidView(
        modifier = modifier,
        factory = { context ->
            WebView(context).apply {
                settings.javaScriptEnabled = false
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                settings.blockNetworkLoads = true
                settings.blockNetworkImage = true
                settings.builtInZoomControls = true
                settings.displayZoomControls = false
                settings.loadWithOverviewMode = true
                settings.useWideViewPort = false
                webViewClient = object : WebViewClient() {
                    override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
                        if (request.url.scheme == "data") null
                        else WebResourceResponse("text/plain", "utf-8", ByteArrayInputStream(ByteArray(0)))

                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = true
                }
            }
        },
        update = { view ->
            if (view.tag != html) {
                view.tag = html
                view.loadDataWithBaseURL(null, html, "text/html", "utf-8", null)
            }
        },
    )
}
