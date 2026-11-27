package cc.off42.office

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.res.Configuration
import android.net.Uri
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performTextReplacement
import androidx.core.content.FileProvider
import androidx.test.espresso.Espresso.closeSoftKeyboard
import androidx.test.core.graphics.writeToTestStorage
import androidx.test.espresso.intent.Intents
import androidx.test.espresso.intent.Intents.intending
import androidx.test.espresso.intent.matcher.IntentMatchers.hasAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import cc.off42.mobile.documentHtml
import cc.off42.mobile.presentationOutline
import cc.off42.mobile.spreadsheetOpen
import java.io.File
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * End-to-end tests on a device or emulator (CI runs them on a tablet and a phone profile): every fixture format
 * opens in its viewer, conversions go through the real "Save as" flow (with the system picker answered by a stub),
 * the generators draw, and the open file survives rotation and a switch to dark mode. Screenshots for the Play
 * listing are written to test storage.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class AppTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val target get() = instrumentation.targetContext
    private val activity get() = compose.activity
    private val device: String
        get() = if (target.resources.configuration.smallestScreenWidthDp >= 600) "tablet" else "phone"

    @Before
    fun setUp() {
        Intents.init()
        // Emulator system apps (the launcher) sometimes stall on CI machines; keep their "not responding" dialogs
        // out of the tests and the screenshots.
        shell("settings put global hide_error_dialogs 1")
        shell("am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS")
        shell("cmd uimode night no")
    }

    @After
    fun tearDown() {
        Intents.release()
        shell("cmd uimode night no")
    }

    private fun shell(command: String) {
        instrumentation.uiAutomation.executeShellCommand(command).close()
    }

    private fun fixture(name: String): ByteArray = instrumentation.context.assets.open("fixtures/$name").use { it.readBytes() }

    private fun waitFor(tag: String, timeout: Long = 30_000) {
        compose.waitUntil(timeout) { compose.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun waitForText(text: String, timeout: Long = 30_000) {
        compose.waitUntil(timeout) { compose.onAllNodes(hasText(text, substring = true)).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun open(name: String) {
        runBlocking { activity.viewModel.openBytes(name, null, fixture(name)) }
        compose.waitUntil(30_000) {
            compose.onAllNodes(hasTestTag("file-title") and hasText(name)).fetchSemanticsNodes().isNotEmpty()
        }
    }

    private fun node(tag: String): SemanticsNodeInteraction = compose.onNodeWithTag(tag, useUnmergedTree = true)

    private fun screenshot(name: String) {
        shell("am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS")
        compose.waitForIdle()
        Thread.sleep(600)
        // The system screenshot (SurfaceFlinger), so nothing in the app is drawn on the test thread.
        instrumentation.uiAutomation.takeScreenshot()?.writeToTestStorage("$device-$name")
    }

    /** Answers the next ACTION_CREATE_DOCUMENT with a file in the app's cache and returns that file. */
    private fun stubSaveDialog(name: String): File {
        val file = File(File(target.cacheDir, "shared").apply { mkdirs() }, name).apply { delete(); createNewFile() }
        val uri: Uri = FileProvider.getUriForFile(target, "${target.packageName}.files", file)
        intending(hasAction(Intent.ACTION_CREATE_DOCUMENT))
            .respondWith(Instrumentation.ActivityResult(Activity.RESULT_OK, Intent().setData(uri)))
        return file
    }

    private fun awaitWritten(file: File): ByteArray {
        // Written when the app has cleared its pending output and the file has stopped growing.
        var last = -1L
        compose.waitUntil(30_000) {
            val now = file.length()
            val done = now > 0 && now == last && activity.viewModel.state.value.pendingOutput == null
            last = now
            if (!done) Thread.sleep(200)
            done
        }
        return file.readBytes()
    }

    @Test
    fun documentsOpenInEveryFormat() {
        for (name in listOf("sample.docx", "sample.odt", "sample.rtf", "sample.md", "sample.html", "sample.txt")) {
            open(name)
            waitFor("doc-stats")
            node("doc-preview").assertIsDisplayed()
        }
        screenshot("1-document")
    }

    @Test
    fun spreadsheetsShowTheirCells() {
        for (name in listOf("sample.xlsx", "sample.ods", "sample.csv", "sample.xls")) {
            open(name)
            waitFor("cell-0-0")
            compose.onNode(hasTestTag("cell-0-0") and hasText("Item")).assertExists()
        }
        screenshot("2-spreadsheet")
    }

    @Test
    fun presentationsShowTheirOutline() {
        for (name in listOf("sample.pptx", "sample.odp")) {
            open(name)
            waitFor("slide-0")
        }
        screenshot("3-presentation")
    }

    @Test
    fun pdfPagesRender() {
        open("sample.pdf")
        waitFor("pdf-info")
        waitFor("pdf-page-0")
        screenshot("4-pdf")
    }

    @Test
    fun openWithIntentUsesAContentUri() {
        val file = File(File(target.cacheDir, "shared").apply { mkdirs() }, "intent.docx").apply { writeBytes(fixture("sample.docx")) }
        val uri = FileProvider.getUriForFile(target, "${target.packageName}.files", file)
        activity.startActivity(
            Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.openxmlformats-officedocument.wordprocessingml.document")
                .setClass(target, MainActivity::class.java).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION),
        )
        compose.waitUntil(30_000) {
            compose.onAllNodes(hasTestTag("file-title") and hasText("intent.docx")).fetchSemanticsNodes().isNotEmpty()
        }
    }

    @Test
    fun saveAsConvertsThroughTheSystemSaveDialog() {
        open("sample.md")
        val docx = stubSaveDialog("out.docx")
        node("save-docx").performClick()
        val bytes = awaitWritten(docx)
        assertTrue(documentHtml(bytes, "docx").contains("Quarterly"))

        // A Word file keeps package parts that ODT cannot carry: the app asks first.
        open("sample.docx")
        val odt = stubSaveDialog("out.odt")
        node("save-odt").performClick()
        waitFor("convert-anyway")
        node("convert-anyway").performClick()
        assertTrue(documentHtml(awaitWritten(odt), "odt").contains("Quarterly"))

        open("sample.csv")
        val ods = stubSaveDialog("out.ods")
        node("save-ods").performClick()
        val book = spreadsheetOpen(awaitWritten(ods), "ods", 100u, 100u)
        assertTrue(book.sheets[0].cells.any { it.text == "Item" })

        open("sample.pptx")
        val odp = stubSaveDialog("out.odp")
        node("save-odp").performClick()
        assertEquals(presentationOutline(fixture("sample.pptx"), "pptx").size, presentationOutline(awaitWritten(odp), "odp").size)
    }

    @Test
    fun codesAndLabelSheetsGenerate() {
        node("nav-LABELS").performClick()
        waitFor("code-preview")
        node("code-value").performTextReplacement("OFF42-0001")
        waitFor("code-preview")
        screenshot("5-codes")
        val png = stubSaveDialog("code.png")
        node("code-save-png").performScrollTo().performClick()
        assertTrue(awaitWritten(png).copyOfRange(1, 4).contentEquals("PNG".toByteArray()))

        node("labels-tab-1").performClick()
        node("make-sheet").performScrollTo().performClick()
        waitFor("sheet-status")
        node("labels-tab-2").performClick()
        waitFor("marks-search")
        node("marks-search").performTextReplacement("CE")
        waitFor("mark-ce")
    }

    @Test
    fun signaturesAndStampsDraw() {
        node("nav-MARKS").performClick()
        waitFor("sig-name")
        node("sig-name").performTextReplacement("Anna Smith")
        closeSoftKeyboard()
        node("sig-generate").performScrollTo().assertIsDisplayed().performClick()
        // The form occupies item 0 on both screen sizes. Bring item 1 into composition
        // when the background generator finishes, even if it was initially off screen.
        val grid = node("signatures")
        val firstDesign = compose.onAllNodes(hasTestTag("signature-0"), useUnmergedTree = true)
        compose.waitUntil(30_000) {
            grid.performScrollToIndex(1)
            firstDesign.fetchSemanticsNodes().isNotEmpty()
        }
        node("signature-0").assertIsDisplayed()
        screenshot("6-signatures")
        node("marks-tab-1").performClick()
        waitFor("stamp-preview")
        screenshot("7-stamp")
    }

    @Test
    fun openFileSurvivesRotation() {
        open("sample.xlsx")
        activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
        compose.waitUntil(15_000) { activity.resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE }
        compose.waitUntil(15_000) {
            compose.onAllNodes(hasTestTag("file-title") and hasText("sample.xlsx")).fetchSemanticsNodes().isNotEmpty()
        }
        waitFor("cell-0-0")
        activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        compose.waitUntil(15_000) { activity.resources.configuration.orientation == Configuration.ORIENTATION_PORTRAIT }
        waitFor("cell-0-0")
        activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
    }

    @Test
    fun darkModeFollowsTheSystem() {
        open("sample.pptx")
        shell("cmd uimode night yes")
        compose.waitUntil(15_000) {
            (activity.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        }
        compose.waitUntil(15_000) {
            compose.onAllNodes(hasTestTag("file-title") and hasText("sample.pptx")).fetchSemanticsNodes().isNotEmpty()
        }
        waitFor("slide-0")
        screenshot("8-dark")
    }
}
