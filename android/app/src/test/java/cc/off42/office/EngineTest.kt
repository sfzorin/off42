package cc.off42.office

import cc.off42.mobile.FileKind
import cc.off42.mobile.Off42Exception
import cc.off42.mobile.documentConvert
import cc.off42.mobile.labelStocks
import cc.off42.mobile.signatureDesigns
import cc.off42.mobile.signaturePng
import cc.off42.mobile.spreadsheetOpen
import java.util.Locale
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * JVM tests of the app's engine layer. They call the real Rust engine through the generated Kotlin bindings and JNA,
 * with the library built for the build machine (android/scripts/build-rust.sh puts it in app/rust-out/host).
 */
class EngineTest {
    @Test
    fun columnNamesAndNumbers() {
        assertEquals("A", columnName(0))
        assertEquals("Z", columnName(25))
        assertEquals("AA", columnName(26))
        assertEquals("CV", columnName(99))
        assertTrue(looksNumeric("1 234,50"))
        assertTrue(looksNumeric("-12%"))
        assertFalse(looksNumeric("Tea"))
        assertFalse(looksNumeric(""))
    }

    @Test
    fun siteLanguagesMatchTheWebApp() {
        assertEquals("zh-Hant", siteLanguage(Locale.forLanguageTag("zh-TW")))
        assertEquals("zh-Hant", siteLanguage(Locale.forLanguageTag("zh-Hant-CN")))
        assertEquals("zh-Hans", siteLanguage(Locale.forLanguageTag("zh-CN")))
        assertEquals("id", siteLanguage(Locale("in")))
        assertEquals("fil", siteLanguage(Locale.forLanguageTag("fil-PH")))
        assertEquals("de", siteLanguage(Locale.GERMANY))
    }

    @Test
    fun extensionsFromNamesOrMimeTypes() {
        assertEquals("docx", extensionFor("Report.DOCX", null))
        assertEquals("md", extensionFor("notes.markdown", "text/plain"))
        assertEquals("pdf", extensionFor("download", "application/pdf"))
        assertEquals("xls", extensionFor("export", "application/vnd.ms-excel"))
        assertEquals(FileKind.SPREADSHEET, kindOf("ods"))
        assertEquals(null, kindOf("jpeg"))
    }

    @Test
    fun loadingFilesThroughTheEngine() {
        val md = "# Report\n\nHello **world**.\n".toByteArray()
        val doc = load(1, "a.md", "md", FileKind.DOCUMENT, md)
        val content = doc.content as FileContent.Document
        assertTrue(content.html.contains("world"))
        assertTrue(content.info.words >= 3uL)

        val csv = "Item,Price\nTea,2\nCake,3\nTotal,=SUM(B2:B3)\n".toByteArray()
        val sheet = load(2, "b.csv", "csv", FileKind.SPREADSHEET, csv).content as FileContent.Spreadsheet
        val grid = sheetGrid(sheet.workbook.sheets[0])
        assertEquals("5", grid[3][1])
        assertEquals(4, grid.size)

        val broken = load(3, "c.docx", "docx", FileKind.DOCUMENT, "not a zip".toByteArray())
        assertEquals(null, broken.content)
        assertNotNull(broken.error)
    }

    @Test
    fun typedExceptions() {
        val docx = documentConvert("# Title\n".toByteArray(), "md", "docx", false)
        try {
            documentConvert(docx, "docx", "odt", false)
            fail("expected PackagePartsWouldBeLost")
        } catch (e: Off42Exception.PackagePartsWouldBeLost) {
            assertTrue(e.message!!.contains("package parts"))
        }
        try {
            spreadsheetOpen(ByteArray(0), "numbers", 10u, 10u)
            fail("expected UnsupportedFormat")
        } catch (e: Off42Exception.UnsupportedFormat) {
            assertTrue(e.message!!.isNotEmpty())
        }
    }

    @Test
    fun generatorsAreDeterministic() {
        assertTrue(labelStocks().size > 100)
        val designs = signatureDesigns(3u, 0u, 42uL)
        assertEquals(3, designs.size)
        val a = signaturePng("Anna Smith", designs[0], 7uL, "blue", 0u, 130.0)
        assertArrayEquals(a, signaturePng("Anna Smith", designs[0], 7uL, "blue", 0u, 130.0))
        assertTrue(randomSeed() != 0uL)
    }
}
