"""Regression coverage for missing/unsafe localization catalogs.

Run: python3 -m unittest discover -s scripts/tests -p 'test_*.py'
"""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("check_strings", Path(__file__).resolve().parents[1] / "check_strings.py")
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


class CatalogTests(unittest.TestCase):
    def check(self, content, required):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "de.json"
            path.write_text(content, encoding="utf-8")
            return audit.check_catalog(path, set(required))

    def test_missing_and_extra_keys_fail(self):
        problems = self.check('{"Save": "Speichern", "Obsolete": "Veraltet"}', ["Save", "Open…"])
        self.assertEqual(len(problems), 2)
        self.assertTrue(any("missing translation: 'Open…'" in problem for problem in problems))
        self.assertTrue(any("unknown source key: 'Obsolete'" in problem for problem in problems))

    def test_duplicate_key_is_not_silently_overwritten(self):
        problems = self.check('{"Save": "Speichern", "Save": ""}', ["Save"])
        self.assertEqual(len(problems), 1)
        self.assertIn("duplicate key", problems[0])

    def test_missing_file_and_malformed_json_fail(self):
        self.assertTrue(audit.check_catalog(Path("/no-such-localization-catalog.json"), {"Save"}))
        self.assertTrue(self.check('{"Save":', ["Save"]))
        self.assertTrue(self.check('["Save"]', ["Save"]))

    def test_empty_and_non_string_translations_fail(self):
        for value in ("", "  \n", None, 123, ["Speichern"], {}):
            with self.subTest(value=value):
                self.assertTrue(self.check(json.dumps({"Save": value}), ["Save"]))

    def test_invalid_unicode_and_nul_are_rejected(self):
        self.assertTrue(self.check('{"Save": "\\ud800"}', ["Save"]))
        self.assertTrue(self.check('{"Save": "\\u0000"}', ["Save"]))

    def test_valid_multiscript_catalog_passes(self):
        self.assertEqual(self.check(json.dumps({"Save": "保存", "%d pages": "%d صفحات"}), ["Save", "%d pages"]), [])


class FormatTests(unittest.TestCase):
    def test_document_template_fields_are_preserved(self):
        self.assertIsNone(audit.translation_problem('Codes: <<page>>, <<pages>>', 'Felder: <<pages>>, <<page>>'))
        self.assertIsNotNone(audit.translation_problem('Page <<page>>', 'Seite <<Seite>>'))

    def test_shortcuts_are_not_translated(self):
        self.assertIsNone(audit.translation_problem('⌘F / ⌘G — find', '⌘F / ⌘G — suchen'))
        self.assertIsNotNone(audit.translation_problem('⌘F — find', '⌘S — suchen'))
        self.assertIsNone(audit.translation_problem('⌥-click edits one word.', '⌥を押しながらクリックすると単語を編集します。'))
        self.assertIsNone(audit.translation_problem('⌥-click edits one word.', '⌥-клик редактирует слово.'))
        self.assertIsNotNone(audit.translation_problem('⌥-click edits one word.', 'クリックすると単語を編集します。'))
        self.assertIsNotNone(audit.translation_problem('⌘- — zoom out', '⌘+ — verkleinern'))

    def test_literal_formula_and_cli_examples_remain_executable(self):
        self.assertIsNone(audit.translation_problem('e.g. =SUM(A1:A10)', 'например =СУММ(A1:A10)'))
        self.assertIsNotNone(audit.translation_problem('e.g. =SUM(A1:A10)', 'z. B. =SUMME(A1:A10)'))
        self.assertIsNotNone(audit.translation_problem('Run soffice --headless --convert-to pptx file.ppt', 'soffice --konvertieren pptx datei.ppt'))

    def test_sheet_names_can_be_localized_but_reference_syntax_cannot(self):
        self.assertIsNone(audit.translation_problem('or #Sheet2!A1', 'или #Лист2!A1'))
        self.assertIsNone(audit.translation_problem('or #Sheet2!A1', 'veya #Sayfa2!A1'))
        self.assertIsNotNone(audit.translation_problem('or #Sheet2!A1', 'oder #Tabelle2.A1'))
        self.assertIsNotNone(audit.translation_problem('or #Sheet2!A1', 'oder #Tabelle2!Z1'))

    def test_argument_position_reordering_preserves_identity(self):
        self.assertIsNone(audit.translation_problem("%@ has %d pages", "%2$d Seiten in %1$@"))
        self.assertIsNotNone(audit.translation_problem("%@ has %d pages", "%d Seiten in %@"))

    def test_type_length_precision_width_and_count_changes_fail(self):
        source = "%@ %d %ld %.1f %02d"
        for translated in ("%@ %d %d %.1f %02d", "%@ %d %ld %.2f %02d", "%@ %d %ld %.1f %d",
                           "%@ %d %ld %.1f %02d %d", "%@ %d %ld %.1f", "%@ %d %ld %.1f %02n"):
            with self.subTest(translated=translated):
                self.assertIsNotNone(audit.translation_problem(source, translated))

    def test_literal_percent_escape_is_preserved(self):
        self.assertIsNone(audit.translation_problem("Scanning… %d%%", "%%%d taranıyor"))
        self.assertIsNotNone(audit.translation_problem("Scanning… %d%%", "Scannen… %d%"))
        self.assertIsNotNone(audit.translation_problem("Scanning… %d%%", "Scannen… %d"))

    def test_plain_percent_labels_are_not_formats(self):
        self.assertIsNone(audit.translation_problem("100% Stacked", "100 % gestapelt"))
        self.assertIsNone(audit.translation_problem("Largest %", "Größte %"))
        self.assertIsNotNone(audit.translation_problem("Save", "Speichern %@"))
        self.assertIsNotNone(audit.translation_problem("Save", "Speichern %n"))

    def test_duplicate_or_mixed_positions_fail(self):
        self.assertIsNotNone(audit.translation_problem("%@ %@", "%1$@ %1$@"))
        self.assertIsNotNone(audit.translation_problem("%@ %@", "%1$@ %@"))

    def test_dynamic_width_and_precision(self):
        self.assertIsNone(audit.translation_problem("%*.*f", "%3$*1$.*2$f"))
        self.assertIsNotNone(audit.translation_problem("%*.*f", "%3$*2$.*2$f"))


class SwiftTests(unittest.TestCase):
    def test_swift_escapes_keep_unicode(self):
        self.assertEqual(audit.decode_swift(r'Русский\n"中文"\t\u{2026}\\'), 'Русский\n"中文"\t…\\')

    def test_multiline_row_is_collected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "Strings.swift"
            path.write_text('["Open\\nfile",\n"Открыть\\nфайл",\n"Dosya\\naç"]', encoding="utf-8")
            self.assertEqual(audit.table_rows(path), [("Open\nfile", "Открыть\nфайл", "Dosya\naç")])


if __name__ == "__main__":
    unittest.main()
