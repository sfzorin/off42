package cc.off42.office.ui

import android.graphics.BitmapFactory
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp

/** A labelled drop-down list. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun <T> Choice(label: String, options: List<T>, selected: T, title: @Composable (T) -> String, onSelect: (T) -> Unit, modifier: Modifier = Modifier) {
    var open by remember { mutableStateOf(false) }
    ExposedDropdownMenuBox(expanded = open, onExpandedChange = { open = it }, modifier = modifier) {
        OutlinedTextField(
            value = title(selected),
            onValueChange = {},
            readOnly = true,
            singleLine = true,
            label = { Text(label) },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = open) },
            modifier = Modifier.menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable).fillMaxWidth(),
        )
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { option ->
                DropdownMenuItem(text = { Text(title(option)) }, onClick = { onSelect(option); open = false })
            }
        }
    }
}

/** A number field in millimetres (or any decimal). Invalid text keeps the last valid value. */
@Composable
fun NumberField(label: String, value: Double, onChange: (Double) -> Unit, modifier: Modifier = Modifier, testTag: String? = null) {
    var text by remember { mutableStateOf(format(value)) }
    OutlinedTextField(
        value = text,
        onValueChange = { new ->
            text = new
            new.replace(',', '.').toDoubleOrNull()?.let(onChange)
        },
        label = { Text(label) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
        modifier = modifier.then(if (testTag != null) Modifier.testTag(testTag) else Modifier),
    )
}

private fun format(value: Double) = if (value == Math.floor(value)) value.toLong().toString() else value.toString()

/** A PNG from the engine, on a checkerboard when it is transparent. */
@Composable
fun PngImage(bytes: ByteArray?, description: String, modifier: Modifier = Modifier, checker: Boolean = false) {
    val bitmap = remember(bytes) { bytes?.let { BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() } }
    Box(modifier, contentAlignment = Alignment.Center) {
        if (checker) Checkerboard(Modifier.matchParentSize())
        if (bitmap != null) {
            Image(bitmap, contentDescription = description, contentScale = ContentScale.Fit,
                modifier = Modifier.padding(8.dp).matchParentSize())
        }
    }
}

@Composable
private fun Checkerboard(modifier: Modifier) {
    val dark = MaterialTheme.colorScheme.surfaceVariant
    Canvas(modifier) {
        drawRect(Color.White)
        val cell = 8.dp.toPx()
        var y = 0f
        var row = 0
        while (y < size.height) {
            var x = if (row % 2 == 0) 0f else cell
            while (x < size.width) {
                drawRect(dark.copy(alpha = 0.35f), Offset(x, y), Size(cell, cell))
                x += cell * 2
            }
            y += cell
            row++
        }
    }
}
