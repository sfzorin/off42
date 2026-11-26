package cc.off42.office.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
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
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import cc.off42.mobile.Off42Exception
import cc.off42.mobile.SignatureOptions
import cc.off42.mobile.StampTemplate
import cc.off42.mobile.signatureDesigns
import cc.off42.mobile.signatureOptions
import cc.off42.mobile.signaturePng
import cc.off42.mobile.stampPng
import cc.off42.mobile.stampTemplates
import cc.off42.office.Off42ViewModel
import cc.off42.office.R
import cc.off42.office.engine
import cc.off42.office.randomSeed
import cc.off42.office.siteLanguage
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
fun MarksScreen(viewModel: Off42ViewModel, wide: Boolean) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    Column(Modifier.fillMaxSize()) {
        PrimaryTabRow(selectedTabIndex = tab) {
            listOf(R.string.sig_title, R.string.stamp_title).forEachIndexed { i, title ->
                Tab(selected = tab == i, onClick = { tab = i }, text = { Text(stringResource(title), maxLines = 1) },
                    modifier = Modifier.testTag("marks-tab-$i"))
            }
        }
        if (tab == 0) Signatures(viewModel) else Stamps(viewModel, wide)
    }
}

@Composable
private fun formTitle(value: UByte): String = when (value.toInt()) {
    0 -> stringResource(R.string.sig_form_0)
    1 -> stringResource(R.string.sig_form_1)
    2 -> stringResource(R.string.sig_form_2)
    3 -> stringResource(R.string.sig_form_3)
    4 -> stringResource(R.string.sig_form_4)
    else -> stringResource(R.string.sig_form_5)
}

@Composable
private fun penTitle(value: UByte): String = when (value.toInt()) {
    0 -> stringResource(R.string.sig_pen_0)
    1 -> stringResource(R.string.sig_pen_1)
    2 -> stringResource(R.string.sig_pen_2)
    else -> stringResource(R.string.sig_pen_3)
}

@Composable
private fun inkTitle(value: String): String = when (value) {
    "blue" -> stringResource(R.string.ink_blue)
    "violet" -> stringResource(R.string.ink_violet)
    "black" -> stringResource(R.string.ink_black)
    "red" -> stringResource(R.string.ink_red)
    "green" -> stringResource(R.string.ink_green)
    else -> stringResource(R.string.stamp_default_ink)
}

private class Design(val seed: ULong, impression: ULong) {
    var impression by mutableStateOf(impression)
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Signatures(viewModel: Off42ViewModel) {
    val options by produceState<SignatureOptions?>(null) { value = runCatching { engine { signatureOptions() } }.getOrNull() }
    val opts = options ?: return
    var name by rememberSaveable { mutableStateOf("") }
    var form by rememberSaveable { mutableIntStateOf(opts.forms.first().value.toInt()) }
    var pen by rememberSaveable { mutableIntStateOf(opts.pens.first().value.toInt()) }
    var ink by rememberSaveable { mutableStateOf("blue") }
    val designs = remember { mutableStateListOf<Design>() }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val share = rememberShare()

    LazyVerticalGrid(GridCells.Adaptive(300.dp), Modifier.fillMaxSize().testTag("signatures"), contentPadding = PaddingValues(16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item(span = { GridItemSpan(maxLineSpan) }) {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(name, { name = it }, label = { Text(stringResource(R.string.sig_name)) }, singleLine = true,
                        modifier = Modifier.testTag("sig-name"))
                    Choice(stringResource(R.string.sig_form), opts.forms.map { it.value }, form.toUByte(), { formTitle(it) },
                        { form = it.toInt() })
                    Choice(stringResource(R.string.sig_pen), opts.pens.map { it.value }, pen.toUByte(), { penTitle(it) },
                        { pen = it.toInt() })
                    Choice(stringResource(R.string.sig_ink), opts.inks.map { it.value }, ink, { inkTitle(it) }, { ink = it })
                }
                Button(onClick = {
                    scope.launch {
                        error = null
                        try {
                            val seeds = engine { signatureDesigns(6u, form.toUByte(), randomSeed()) }
                            designs.clear()
                            designs.addAll(seeds.map { Design(it, randomSeed()) })
                        } catch (e: Off42Exception) {
                            error = e.message
                        }
                    }
                }, enabled = name.isNotBlank(), modifier = Modifier.testTag("sig-generate")) { Text(stringResource(R.string.sig_generate)) }
                error?.let { ErrorText(it) }
            }
        }
        itemsIndexed(designs) { index, design ->
            val png by produceState<Result<ByteArray>?>(null, design.seed, design.impression, name, ink, pen) {
                delay(100)
                value = runCatching { engine { signaturePng(name, design.seed, design.impression, ink, pen.toUByte(), 130.0) } }
            }
            val alt = stringResource(R.string.sig_alt, name)
            Card(Modifier.testTag("signature-$index")) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    val result = png
                    if (result != null && result.isFailure) ErrorText(result.exceptionOrNull()?.message ?: "")
                    else PngImage(result?.getOrNull(), alt, Modifier.fillMaxWidth().height(120.dp), checker = true)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(onClick = { design.impression = randomSeed() }) { Text(stringResource(R.string.new_impression)) }
                        val fileName = "signature-${name.trim().replace(Regex("\\s+"), "-")}.png"
                        TextButton(onClick = { result?.getOrNull()?.let { viewModel.output(fileName, it) } }) {
                            Text(stringResource(R.string.save_png))
                        }
                        TextButton(onClick = { result?.getOrNull()?.let { share(fileName, "image/png", it) } }) {
                            Text(stringResource(R.string.share))
                        }
                    }
                }
            }
        }
    }
}

private fun today(): String = LocalDate.now().format(DateTimeFormatter.ofPattern("dd/MM/yyyy"))

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Stamps(viewModel: Off42ViewModel, wide: Boolean) {
    val language = siteLanguage(androidx.compose.ui.platform.LocalConfiguration.current.locales[0])
    val templates by produceState<List<StampTemplate>>(emptyList(), language) {
        value = runCatching { engine { stampTemplates(language) } }.getOrDefault(emptyList())
    }
    if (templates.isEmpty()) return
    var templateId by rememberSaveable { mutableStateOf(templates.first().id) }
    val template = templates.firstOrNull { it.id == templateId } ?: templates.first()
    val values = remember(template.id) {
        mutableStateMapOf<String, String>().apply { template.fields.forEach { put(it.key, if (it.date) today() else it.example) } }
    }
    var ink by rememberSaveable { mutableStateOf("") }
    var impression by rememberSaveable { mutableStateOf(randomSeed().toString()) }
    var picking by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val share = rememberShare()
    val snapshot = values.toMap()
    val preview by produceState<Result<ByteArray>?>(null, template.id, snapshot, ink, impression) {
        delay(250)
        value = runCatching { engine { stampPng(template.id, snapshot, ink, impression.toULong(), 0.5) } }
    }
    val fileName = "stamp-${template.id}.png"
    val saveLarge: (Boolean) -> Unit = { toShare ->
        scope.launch {
            try {
                val bytes = engine { stampPng(template.id, snapshot, ink, impression.toULong(), 2.0) }
                if (toShare) share(fileName, "image/png", bytes) else viewModel.output(fileName, bytes)
            } catch (e: Off42Exception) {
                viewModel.notify(e.message ?: "", error = true)
            }
        }
    }

    FormAndPreview(wide, form = {
        OutlinedButton(onClick = { picking = true }, modifier = Modifier.fillMaxWidth().testTag("stamp-template")) {
            Text("${stringResource(R.string.stamp_template)}: ${template.title}")
        }
        Choice(stringResource(R.string.sig_ink), listOf("", "blue", "violet", "black", "red", "green"), ink, { inkTitle(it) }, { ink = it })
        template.fields.forEach { field ->
            OutlinedTextField(values[field.key] ?: "", { values[field.key] = it }, label = { Text(field.label) },
                singleLine = !field.multiline, minLines = if (field.multiline) 2 else 1, modifier = Modifier.fillMaxWidth())
        }
    }, preview = {
        Card(Modifier.fillMaxWidth().heightIn(min = 280.dp)) {
            val result = preview
            if (result != null && result.isFailure) ErrorText(result.exceptionOrNull()?.message ?: "", Modifier.padding(16.dp))
            else PngImage(result?.getOrNull(), template.title, Modifier.fillMaxWidth().height(280.dp).testTag("stamp-preview"), checker = true)
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = { impression = randomSeed().toString() }) { Text(stringResource(R.string.new_impression)) }
            Button(onClick = { saveLarge(false) }, enabled = preview?.isSuccess == true, modifier = Modifier.testTag("stamp-save")) {
                Text(stringResource(R.string.save_png))
            }
            OutlinedButton(onClick = { saveLarge(true) }, enabled = preview?.isSuccess == true) { Text(stringResource(R.string.share)) }
        }
    })

    if (picking) {
        AlertDialog(
            onDismissRequest = { picking = false },
            title = { Text(stringResource(R.string.stamp_template)) },
            text = {
                LazyColumn(Modifier.heightIn(max = 480.dp)) {
                    templates.groupBy { it.category }.forEach { (category, list) ->
                        item(key = "c-$category") {
                            Text(category, Modifier.padding(top = 12.dp, bottom = 4.dp), fontWeight = FontWeight.SemiBold,
                                color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge)
                        }
                        items(list, key = { it.id }) { t ->
                            ListItem(headlineContent = { Text(t.title) }, modifier = Modifier.clickable { templateId = t.id; picking = false })
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { picking = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}
