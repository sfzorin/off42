package cc.off42.office.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import cc.off42.mobile.Workbook
import cc.off42.mobile.saveFormats
import cc.off42.office.MAX_COLS
import cc.off42.office.MAX_ROWS
import cc.off42.office.Off42ViewModel
import cc.off42.office.OpenedFile
import cc.off42.office.R
import cc.off42.office.columnName
import cc.off42.office.looksNumeric
import cc.off42.office.sheetGrid

private val CellWidth = 112.dp
private val HeaderWidth = 52.dp
private val RowHeight = 32.dp

@Composable
fun SpreadsheetViewer(viewModel: Off42ViewModel, file: OpenedFile, workbook: Workbook) {
    var current by rememberSaveable(file.id) { mutableIntStateOf(0) }
    val sheet = workbook.sheets.getOrNull(current) ?: return
    Column(Modifier.fillMaxSize()) {
        // Read-only workbooks (XLS) export their values as CSV only, as in the web app.
        SaveAsRow(if (workbook.readOnly) emptyList() else saveFormats(file.kind), extra = {
            OutlinedButton(onClick = { viewModel.saveSheetValues(file, current, sheet.name) }, modifier = Modifier.testTag("values-csv")) {
                Text(stringResource(R.string.sheet_values_csv))
            }
        }) { viewModel.saveAs(file, it) }
        if (workbook.readOnly) Note(stringResource(R.string.sheet_read_only))
        if (sheet.truncated) Note(stringResource(R.string.sheet_truncated, MAX_ROWS, MAX_COLS))
        if (workbook.sheets.size > 1) {
            PrimaryScrollableTabRow(selectedTabIndex = current, edgePadding = 8.dp) {
                workbook.sheets.forEachIndexed { index, s ->
                    Tab(selected = index == current, onClick = { current = index }, text = { Text(s.name) },
                        modifier = Modifier.testTag("sheet-tab-$index"))
                }
            }
        }
        val grid = remember(file.id, current) { sheetGrid(sheet) }
        SheetGrid(grid, Modifier.weight(1f))
    }
}

@Composable
private fun Note(text: String) {
    Text(text, Modifier.padding(horizontal = 16.dp, vertical = 2.dp), style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant)
}

/**
 * A virtualised grid: rows are a LazyColumn with the column letters as a sticky header; every row and the header
 * share one horizontal scroll state, and the row numbers stay at the start edge.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SheetGrid(grid: List<Array<String>>, modifier: Modifier) {
    val horizontal = rememberScrollState()
    val columns = grid.firstOrNull()?.size ?: 0
    val headerColor = MaterialTheme.colorScheme.surfaceVariant
    val line = MaterialTheme.colorScheme.outline.copy(alpha = 0.35f)
    LazyColumn(modifier.testTag("sheet-grid")) {
        stickyHeader {
            Row(Modifier.background(headerColor)) {
                GridCell("", HeaderWidth, header = true, line = line)
                Row(Modifier.horizontalScroll(horizontal)) {
                    for (c in 0 until columns) GridCell(columnName(c), CellWidth, header = true, line = line, center = true)
                }
            }
        }
        itemsIndexed(grid) { r, row ->
            Row {
                Box(Modifier.background(headerColor)) { GridCell((r + 1).toString(), HeaderWidth, header = true, line = line, center = true) }
                Row(Modifier.horizontalScroll(horizontal)) {
                    row.forEachIndexed { c, text ->
                        GridCell(text, CellWidth, header = false, line = line, end = looksNumeric(text),
                            modifier = Modifier.testTag("cell-$r-$c"))
                    }
                }
            }
        }
    }
}

@Composable
private fun GridCell(
    text: String,
    width: Dp,
    header: Boolean,
    line: androidx.compose.ui.graphics.Color,
    center: Boolean = false,
    end: Boolean = false,
    modifier: Modifier = Modifier,
) {
    Box(modifier.semantics(mergeDescendants = true) {}.width(width).height(RowHeight).border(0.5.dp, line).padding(horizontal = 6.dp),
        contentAlignment = Alignment.CenterStart) {
        Text(
            text,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            style = MaterialTheme.typography.bodySmall,
            fontWeight = if (header) FontWeight.SemiBold else FontWeight.Normal,
            textAlign = when {
                center -> TextAlign.Center
                end -> TextAlign.End
                else -> TextAlign.Start
            },
            modifier = Modifier.fillMaxSize().padding(top = 8.dp),
        )
    }
}
