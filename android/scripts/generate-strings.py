#!/usr/bin/env python3
"""Generates android/app/src/main/res/values*/strings.xml in the twenty languages of the site and the web app.

Most strings are the web app's (web/src/locales/<lang>.json, mapped below); Android-only strings live in
android/scripts/strings-android.json. Placeholders {name} become positional Android arguments (%1$s, %2$d).
Missing translations fall back to English and are reported. The generated files are committed; rerun this
script after changing either source:  python3 android/scripts/generate-strings.py
"""
import json
import pathlib
import re
import sys
from xml.sax.saxutils import escape

ROOT = pathlib.Path(__file__).resolve().parents[2]
WEB = ROOT / "web" / "src" / "locales"
EXTRA = ROOT / "android" / "scripts" / "strings-android.json"
RES = ROOT / "android" / "app" / "src" / "main" / "res"

LANGUAGES = ["en", "ar", "de", "es", "fil", "fr", "hu", "id", "it", "ja", "ko", "ms", "pl", "pt", "ru", "th", "tr",
             "vi", "zh-Hans", "zh-Hant"]

# Android resource folder qualifiers (Indonesian keeps Android's legacy "in"; BCP 47 tags for Filipino and Chinese).
FOLDERS = {"en": "values", "id": "values-in", "fil": "values-b+fil", "zh-Hans": "values-b+zh+Hans",
           "zh-Hant": "values-b+zh+Hant"}

# Android name -> (web key, [(placeholder, type)])
WEB_STRINGS = {
    "section_labels": ("tab.labels", []),
    "section_marks": ("tab.marks", []),
    "save_as": ("common.saveAs", []),
    "width_mm": ("common.widthMm", []),
    "height_mm": ("common.heightMm", []),
    "new_impression": ("common.newImpression", []),
    "file_unsupported": ("files.unsupported", [("extension", "s")]),
    "slides_notes_title": ("slides.notesTitle", []),
    "doc_preview": ("doc.preview", []),
    "doc_stats": ("doc.stats", [("words", "s"), ("characters", "s"), ("paragraphs", "s")]),
    "doc_convert_anyway": ("doc.convertAnyway", [("error", "s"), ("format", "s")]),
    "sheet_values_csv": ("sheet.valuesCsv", []),
    "sheet_read_only": ("sheet.readOnly", []),
    "sheet_truncated": ("sheet.truncated", [("rows", "d"), ("columns", "d")]),
    "slides_hidden": ("slides.hidden", []),
    "slides_untitled": ("slides.untitled", []),
    "slides_picture": ("slides.pictureLabel", []),

    "slides_outline_note": ("slides.outlineNote", []),
    "pdf_encrypted": ("pdf.encrypted", []),
    "pdf_pages": ("pdf.pages", []),
    "pdf_bookmarks": ("pdf.bookmarks", []),
    "pdf_attachments": ("pdf.attachments", []),
    "pdf_form_fields": ("pdf.formFields", []),
    "code_title": ("labels.tab.codes", []),
    "code_type": ("code.type", []),
    "code_qr": ("code.qr", []),
    "code_content": ("code.content", []),
    "code_caption": ("code.caption", []),
    "code_preview": ("code.preview", []),
    "labels_title": ("labels.tab.sheets", []),
    "labels_stock": ("labels.stock", []),
    "labels_custom": ("labels.custom", []),
    "labels_gap_x": ("labels.gapX", []),
    "labels_gap_y": ("labels.gapY", []),
    "labels_first": ("labels.first", []),
    "labels_code": ("labels.code", []),
    "labels_none": ("labels.none", []),
    "labels_lines": ("labels.lines", []),
    "labels_lines_format": ("labels.linesFormat", []),
    "labels_status": ("labels.status", [("labels", "d"), ("pages", "d")]),
    "marks_title": ("marks.title", []),
    "marks_hint": ("marks.hint", []),
    "marks_search": ("marks.search", []),
    "marks_search_placeholder": ("marks.searchPlaceholder", []),

    "marks_all": ("marks.all", []),
    "marks_unverified": ("marks.unverified", []),
    "marks_needs_number": ("marks.needsNumber", []),
    "sig_title": ("sig.title", []),
    "sig_name": ("sig.name", []),
    "sig_form": ("sig.form", []),
    "sig_pen": ("sig.pen", []),
    "sig_ink": ("sig.ink", []),
    "sig_generate": ("sig.newVariants", []),
    "sig_alt": ("sig.alt", [("name", "s")]),
    **{f"sig_form_{i}": (f"sig.form.{i}", []) for i in range(6)},
    **{f"sig_pen_{i}": (f"sig.pen.{i}", []) for i in range(4)},
    **{f"ink_{c}": (f"ink.{c}", []) for c in ["blue", "violet", "black", "red", "green"]},
    "stamp_title": ("stamp.title", []),
    "stamp_template": ("stamp.template", []),
    "stamp_default_ink": ("stamp.defaultInk", []),
}

EXTRA_ARGS = {
    "formats_hint": [("formats", "s")],
    "convert_title": [("format", "s")],
    "saved": [("name", "s")],
    "pdf_page": [("page", "d"), ("count", "d")],
}


def android_text(text, args, where):
    for i, (name, kind) in enumerate(args, 1):
        if "{" + name + "}" not in text:
            sys.exit(f"{where}: placeholder {{{name}}} missing in {text!r}")
        text = text.replace("{" + name + "}", f"\u0000{i}${kind}")
    if re.search(r"\{\w+\}", text):
        sys.exit(f"{where}: unmapped placeholder in {text!r}")
    text = escape(text).replace("\\", "\\\\").replace("'", "\\'").replace('"', '\\"').replace("\n", "\\n")
    text = text.replace("%", "%%").replace("\u0000", "%")
    if text.startswith("@") or text.startswith("?"):
        text = "\\" + text
    return text


def main():
    extra = json.loads(EXTRA.read_text(encoding="utf-8"))
    web_en = json.loads((WEB / "en.json").read_text(encoding="utf-8"))
    missing = []
    for lang in LANGUAGES:
        web = json.loads((WEB / f"{lang}.json").read_text(encoding="utf-8"))
        lines = ['<?xml version="1.0" encoding="utf-8"?>',
                 "<!-- Generated by android/scripts/generate-strings.py from web/src/locales and "
                 "android/scripts/strings-android.json. Do not edit. -->",
                 '<resources xmlns:tools="http://schemas.android.com/tools" tools:ignore="MissingTranslation">']
        if lang == "en":
            lines.append('    <string name="app_name" translatable="false">OFF42</string>')
            lines.append('    <string name="section_pdf" translatable="false">PDF</string>')
            lines.append('    <string name="file_size" translatable="false">%1$s KB</string>')
        for name, (key, args) in WEB_STRINGS.items():
            text = web.get(key)
            if text is None:
                missing.append(f"{lang}: {key}")
                if lang != "en":
                    continue
                text = web_en[key]
            lines.append(f'    <string name="{name}">{android_text(text, args, f"{lang} {key}")}</string>')
        for name, text in extra[lang].items():
            lines.append(f'    <string name="{name}">{android_text(text, EXTRA_ARGS.get(name, []), f"{lang} {name}")}</string>')
        for name in extra["en"]:
            if name not in extra[lang]:
                missing.append(f"{lang}: {name}")
        lines.append("</resources>")
        folder = RES / FOLDERS.get(lang, f"values-{lang}")
        folder.mkdir(parents=True, exist_ok=True)
        (folder / "strings.xml").write_text("\n".join(lines) + "\n", encoding="utf-8")
    if missing:
        print("Falling back to English for:\n  " + "\n  ".join(missing))
    print(f"Wrote strings.xml for {len(LANGUAGES)} languages.")


if __name__ == "__main__":
    main()
