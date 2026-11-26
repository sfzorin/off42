package cc.off42.office.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.unit.dp
import cc.off42.mobile.Slide
import cc.off42.mobile.saveFormats
import cc.off42.mobile.slideTitleObject
import cc.off42.office.Off42ViewModel
import cc.off42.office.OpenedFile
import cc.off42.office.R

@Composable
fun PresentationViewer(viewModel: Off42ViewModel, file: OpenedFile, slides: List<Slide>) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = 320.dp),
        modifier = Modifier.fillMaxSize().testTag("slides"),
        contentPadding = PaddingValues(bottom = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(span = { GridItemSpan(maxLineSpan) }) {
            Column {
                SaveAsRow(saveFormats(file.kind)) { viewModel.saveAs(file, it) }
                Text(stringResource(R.string.slides_outline_note), Modifier.padding(horizontal = 16.dp),
                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        itemsIndexed(slides) { index, slide -> SlideCard(index, slide) }
    }
}

@Composable
private fun SlideCard(index: Int, slide: Slide) {
    val titleIndex = slideTitleObject(slide)?.toInt()
    val title = slide.title.ifEmpty { titleIndex?.let { slide.objects[it].text } ?: "" }
    Card(
        Modifier.padding(horizontal = 16.dp).fillMaxWidth().then(if (slide.hidden) Modifier.alpha(0.6f) else Modifier)
            .testTag("slide-$index"),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            val number = if (slide.hidden) "${index + 1} · ${stringResource(R.string.slides_hidden)}" else "${index + 1}"
            Text(number, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.secondary)
            Text(title.ifEmpty { stringResource(R.string.slides_untitled) }, style = MaterialTheme.typography.titleMedium)
            if (slide.body.isNotEmpty()) Bullets(slide.body)
            slide.objects.forEachIndexed { i, obj ->
                if (i == titleIndex) return@forEachIndexed
                when {
                    obj.cells.isNotEmpty() -> MiniTable(obj.cells)
                    obj.kind == "Picture" -> Text(stringResource(R.string.slides_picture), style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                    obj.text.isNotEmpty() -> if (obj.role == "body") Bullets(obj.text)
                    else Text(obj.text, style = MaterialTheme.typography.bodyMedium)
                }
            }
            if (slide.notes.isNotEmpty()) {
                Text("${stringResource(R.string.slides_notes_title)}: ${slide.notes}", style = MaterialTheme.typography.bodySmall,
                    fontStyle = FontStyle.Italic, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

@Composable
private fun Bullets(text: String) {
    Column {
        text.lines().filter { it.isNotBlank() }.forEach { line ->
            Row { Text("•  ", style = MaterialTheme.typography.bodyMedium); Text(line, style = MaterialTheme.typography.bodyMedium) }
        }
    }
}

@Composable
private fun MiniTable(cells: List<List<String>>) {
    val line = MaterialTheme.colorScheme.outline.copy(alpha = 0.4f)
    Column(Modifier.border(0.5.dp, line)) {
        cells.forEach { row ->
            Row {
                row.forEach { text ->
                    Text(text, Modifier.weight(1f).border(0.5.dp, line).padding(4.dp), style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
}
