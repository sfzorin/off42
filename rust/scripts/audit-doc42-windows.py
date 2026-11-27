#!/usr/bin/env python3
"""Native DOC42 document-model audits: page canvas, sections and tracked changes.

Each scenario drives the packaged DOC42.exe through its real controls and commands,
checks Undo/Redo, then saves, reopens and inspects the written output.
"""
import argparse
import importlib.util
import json
import re
import struct
import subprocess
import traceback
import zipfile
from pathlib import Path

_spec = importlib.util.spec_from_file_location("audit_ui", Path(__file__).with_name("audit-ui-windows.py"))
ui = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ui)
u, c = ui.u, ui.c

PAGE = re.compile(r"Page (\d+) / (\d+)")
EDIT = 101


def launch(binary, source, output, name):
    environment = dict(__import__("os").environ)
    environment["OFF42_UI_TRACE_FILE"] = str(output / (name + "-commands.log"))
    process = subprocess.Popen([str(binary), str(source)], env=environment)
    main = ui.modal(process, native_class="Suite42Native")
    ui.wait_for(lambda: source.name in ui.text(main) and u.IsWindowEnabled(main), "fixture open", process)
    return process, main


def page_status(process, main):
    for hwnd in ui.windows(process.pid, main):
        match = PAGE.search(ui.text(hwnd))
        if match:
            return int(match.group(1)), int(match.group(2))
    return None


def wait_pages(process, main, accept, description):
    return ui.wait_for(lambda: (lambda s: s if s and accept(s) else None)(page_status(process, main)), description, process)


def save(process, main, target, format_name, signature):
    ui.save_as(main)
    ui.choose_file(process, target, format_name=format_name)
    ui.output_ready(process, main, target, signature)


def lines_rtf(count, prefix):
    body = "".join(rf"\pard {prefix} {index}\par " for index in range(1, count + 1))
    return (r"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}\fs24 " + body + "}").encode("ascii")


def audit_page_canvas(binary, output):
    source = output / "doc-pages.rtf"
    source.write_bytes(lines_rtf(150, "Paragraph"))
    process, main = launch(binary, source, output, "doc-pages")
    try:
        edit = u.GetDlgItem(main, EDIT)
        first = wait_pages(process, main, lambda s: s[1] >= 3, "multi-page count in status")
        assert first[0] == 1, first
        count = first[1]
        u.SendMessageW(edit, 0xB1, -1, -1)
        wait_pages(process, main, lambda s: s == (count, count), "caret on the last page")
        u.SendMessageW(edit, 0xB1, 0, 0)
        wait_pages(process, main, lambda s: s[0] == 1, "caret back on page 1")
        u.SendMessageW(edit, 0xB1, -1, -1)
        ui.replace_selection(edit, "\r".join("Added %d" % index for index in range(120)))
        grown = wait_pages(process, main, lambda s: s[1] > count, "typed paragraphs add pages")[1]
        ui.capture_window(main, output / "DOC42-page-canvas.bmp")
        ui.command(main, 4)
        wait_pages(process, main, lambda s: s[1] == count, "Undo restores page count")
        ui.command(main, 5)
        wait_pages(process, main, lambda s: s[1] == grown, "Redo restores added pages")
        target = output / "doc-pages-saved.doc42"
        save(process, main, target, "DOC42 project", b"{")
        ui.open_fixture(process, main, target)
        wait_pages(process, main, lambda s: s[1] == grown, "reopened project keeps page count")
        return {"scenario": "DOC42 page canvas", "checks": [
            f"status shows Page 1 / {count} and follows the caret to Page {count} / {count}",
            f"typing adds pages ({count} -> {grown}); Undo/Redo update the count; saved project reopens with {grown} pages",
        ]}
    finally:
        process.kill()


CHANGES = re.compile(r"(\d+) changes")


def review_status(process, main):
    for hwnd in ui.windows(process.pid, main):
        value = ui.text(hwnd)
        if " words · " in value:
            match = CHANGES.search(value)
            return ("Track changes" in value, int(match.group(1)) if match else 0)
    return None


def wait_review(process, main, expected, description):
    return ui.wait_for(lambda: review_status(process, main) == expected, description, process)


def docx_part(path, name):
    with zipfile.ZipFile(path) as package:
        return package.read(name).decode("utf-8")


def audit_tracked_changes(binary, output):
    source = output / "doc-review.rtf"
    source.write_bytes(br"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}\fs24 \pard Alpha beta gamma\par}")
    process, main = launch(binary, source, output, "doc-review")
    try:
        edit = u.GetDlgItem(main, EDIT)
        wait_review(process, main, (False, 0), "review status before tracking")
        ui.command(main, 370)
        wait_review(process, main, (True, 0), "Track changes on")
        u.SendMessageW(edit, 0xB1, 6, 6)
        ui.replace_selection(edit, "new ")
        wait_review(process, main, (True, 1), "typed text recorded as an insertion")
        u.SendMessageW(edit, 0xB1, 15, 20)
        assert ui.copied_selection(edit) == "gamma", ui.copied_selection(edit)
        ui.replace_selection(edit, "")
        wait_review(process, main, (True, 2), "deleted text recorded as a deletion")
        ui.wait_for(lambda: ui.text(edit).rstrip("\r\n") == "Alpha new beta gamma", "deleted text stays visible", process)
        ui.capture_window(main, output / "DOC42-tracked-changes.bmp")
        ui.command(main, 4)
        wait_review(process, main, (True, 1), "Undo removes the recorded deletion")
        ui.command(main, 5)
        wait_review(process, main, (True, 2), "Redo restores the recorded deletion")
        target = output / "doc-review-tracked.docx"
        save(process, main, target, "Word document (supported subset)", b"PK")
        document = docx_part(target, "word/document.xml")
        assert re.search(r"<w:ins [^>]*w:author=\"[^\"]+\"[^>]*>.*?new .*?</w:ins>", document), document
        assert "<w:delText xml:space=\"preserve\">gamma</w:delText>" in document, document
        assert "<w:trackRevisions/>" in docx_part(target, "word/settings.xml")
        ui.open_fixture(process, main, target)
        wait_review(process, main, (True, 2), "reopened DOCX keeps both changes and tracking")
        u.SendMessageW(edit, 0xB1, 0, 0)
        ui.command(main, 371)
        ui.wait_for(lambda: ui.copied_selection(edit) == "new ", "Next change selects the insertion", process)
        ui.command(main, 373)
        wait_review(process, main, (True, 1), "Accept change keeps the insertion as normal text")
        ui.command(main, 376)
        wait_review(process, main, (True, 0), "Reject all keeps the deleted text")
        assert ui.text(edit).rstrip("\r\n") == "Alpha new beta gamma", ui.text(edit)
        ui.command(main, 4)
        wait_review(process, main, (True, 1), "Undo restores the rejected deletion")
        ui.command(main, 375)
        wait_review(process, main, (True, 0), "Accept all removes the deleted text")
        ui.wait_for(lambda: ui.text(edit).rstrip("\r\n") == "Alpha new beta ", "accepted deletion removed", process)
        ui.command(main, 370)
        wait_review(process, main, (False, 0), "Track changes off")
        final = output / "doc-review-accepted.docx"
        save(process, main, final, "Word document (supported subset)", b"PK")
        document = docx_part(final, "word/document.xml")
        assert "<w:ins " not in document and "<w:del " not in document and "gamma" not in document, document
        return {"scenario": "DOC42 tracked changes", "checks": [
            "Track Changes records typed text as an insertion and a deleted selection as visible struck text",
            "Undo/Redo, DOCX w:ins / w:delText / w:trackRevisions output and reopen keep both changes",
            "Next change, Accept change, Reject all, Undo, Accept all and the saved accepted DOCX behave as on macOS",
        ]}
    finally:
        process.kill()


PRINT_LAYOUT = 65496
SHEET_CAPTION = re.compile(r"(\d+) / (\d+)$")
# win42 chrome::DESK #eef0f3 (paper-like desk around Print Layout sheets), as BGR.
DESK_BGR = bytes([0xF3, 0xF0, 0xEE])
RTF_FORMAT = "Rich Text Format (supported subset)"


def desk_window(process, main):
    return next((h for h in ui.windows(process.pid, main) if ui.classname(h) == "Off42PageView"), None)


def sheet_caption(desk):
    """(sheet holding the editor, number of sheets) from the desk's accessible caption."""
    match = SHEET_CAPTION.search(ui.text(desk)) if desk else None
    return (int(match.group(1)), int(match.group(2))) if match else None


def wait_sheets(process, main, accept, description):
    def ready():
        caption = sheet_caption(desk_window(process, main))
        return caption if caption and accept(caption) else None
    return ui.wait_for(ready, description, process)


def visible_sheets(path):
    """Runs of sheet rows separated by desk-coloured gap rows along the desk centre."""
    data = path.read_bytes()
    width, height = struct.unpack_from("<ii", data, 18)

    def pixel(x, y):
        offset = 54 + (height - 1 - y) * width * 4 + x * 4
        return data[offset:offset + 3]
    centre = width // 2
    probes = [centre - 30, centre, centre + 30]
    runs, start = [], None
    for y in range(height):
        gap = all(pixel(x, y) == DESK_BGR for x in probes)
        if not gap and start is None:
            start = y
        elif gap and start is not None:
            runs.append((start, y))
            start = None
    if start is not None:
        runs.append((start, height))
    return [run for run in runs if run[1] - run[0] >= 20], pixel, centre


def header_inked(pixel, centre, sheet):
    """Dark pixels in the header band (between 3% and 8.5% of the sheet height)."""
    top, bottom = sheet
    rows = range(top + (bottom - top) * 3 // 100, top + (bottom - top) * 85 // 1000)
    return any(min(pixel(x, y)) < 200 for y in rows for x in range(centre - 40, centre + 41))


def plain(value):
    return value.replace("\r\n", "\r")


def audit_print_layout(binary, output):
    paragraphs = 150
    source = output / "doc-print-layout.rtf"
    body = "".join(rf"\pard Paragraph {index}\par " for index in range(1, paragraphs + 1))
    source.write_bytes((
        r"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}\paperw12240\paperh15840\margl1440\margr1440"
        r"\margt1440\margb1440\headery720\footery720"
        r"{\header \pard\qc\b\fs36 HEADER PAGE \chpgn\par}{\footer \pard\qc\fs24 Footer \chpgn\par}"
        r"\fs24 " + body + "}").encode("ascii"))
    offsets, position = [], 0
    for index in range(1, paragraphs + 1):
        offsets.append(position)
        position += len(f"Paragraph {index}") + 1
    process, main = launch(binary, source, output, "doc-print-layout")
    try:
        edit = u.GetDlgItem(main, EDIT)
        count = wait_pages(process, main, lambda s: s[1] >= 3, "multi-page count in status")[1]
        original = ui.text(edit)
        baseline = output / "doc-print-layout-before.rtf"
        save(process, main, baseline, RTF_FORMAT, b"{\\rtf")
        assert desk_window(process, main) is None, "Print Layout is off by default"
        ui.command(main, PRINT_LAYOUT)
        wait_sheets(process, main, lambda s: s == (1, count), f"Print Layout with {count} sheets, editor on sheet 1")
        assert u.GetMenuState(u.GetMenu(main), PRINT_LAYOUT, 0) & 8, "Print Layout menu item checked"
        for _ in range(3):
            ui.command(main, 318)
        ui.wait_for(lambda: any(" 25%" in ui.text(h) for h in ui.windows(process.pid, main)), "zoom 25%", process)
        capture = output / "DOC42-print-layout.bmp"

        def sheets_drawn():
            u.SetForegroundWindow(main)
            ui.capture_window(desk_window(process, main), capture)
            found = visible_sheets(capture)
            return found if len(found[0]) >= 2 else None
        runs, pixel, centre = ui.wait_for(sheets_drawn, "two or more separate sheets with a desk gap", process)
        assert runs[0][0] > 0, ("desk gap above the first sheet", runs)
        assert header_inked(pixel, centre, runs[0]), ("header band drawn on sheet 1", runs)
        ui.capture_window(main, output / "DOC42-print-layout-window.bmp")
        desk = desk_window(process, main)

        def sheet_at(position):
            u.SendMessageW(edit, 0xB1, position, position)
            return ui.wait_for(lambda: sheet_caption(desk), "sheet caption", process)[0]
        low, high = 0, paragraphs - 1
        while low < high:
            middle = (low + high) // 2
            if sheet_at(offsets[middle]) >= 2:
                high = middle
            else:
                low = middle + 1
        assert low > 0 and sheet_at(offsets[low]) == 2, ("first paragraph on sheet 2", low)
        last_on_one = low - 1
        end_of_page_one = offsets[last_on_one] + len(f"Paragraph {last_on_one + 1}")
        assert sheet_at(end_of_page_one) == 1, "caret at the end of page 1"
        typed = " continues" + " with more words typed at the end of the first page" * 5
        ui.replace_selection(edit, typed)
        grown = wait_sheets(process, main, lambda s: s[0] == 2 and s[1] >= count,
                            "typed text at the end of page 1 flows onto sheet 2 with the editor")[1]
        wait_pages(process, main, lambda s: s[0] == 2, "status follows the caret to page 2")
        assert typed in plain(ui.text(edit))
        ui.capture_window(main, output / "DOC42-print-layout-typed.bmp")
        ui.command(main, 4)
        ui.wait_for(lambda: ui.text(edit) == original, "Undo restores the original text", process)
        wait_sheets(process, main, lambda s: s[1] == count, f"Undo restores {count} sheets")
        ui.command(main, 5)
        ui.wait_for(lambda: typed in plain(ui.text(edit)), "Redo restores the typed text", process)
        wait_sheets(process, main, lambda s: s[1] == grown, "Redo restores the sheet count")
        ui.command(main, 4)
        ui.wait_for(lambda: ui.text(edit) == original, "second Undo restores the original text", process)
        after = output / "doc-print-layout-after.rtf"
        save(process, main, after, RTF_FORMAT, b"{\\rtf")
        assert after.read_bytes() == baseline.read_bytes(), "saved RTF changed by Print Layout editing and Undo"
        ui.command(main, 5)
        ui.wait_for(lambda: typed in plain(ui.text(edit)), "Redo before saving the project", process)
        edited = ui.text(edit)
        project = output / "doc-print-layout.doc42"
        save(process, main, project, "DOC42 project", b"{")
        ui.open_fixture(process, main, project)
        ui.wait_for(lambda: ui.text(edit) == edited, "reopened project keeps the typed text", process)
        wait_sheets(process, main, lambda s: s[1] == grown, f"reopened project shows {grown} sheets")
        ui.command(main, PRINT_LAYOUT)
        ui.wait_for(lambda: desk_window(process, main) is None, "Print Layout off hides the sheets", process)
        assert ui.text(edit) == edited
        return {"scenario": "DOC42 print layout", "checks": [
            f"Print Layout shows {count} sheets; a 25% capture has {len(runs)} visible sheets separated by desk gaps "
            "and an inked header band",
            f"typing at the end of page 1 (paragraph {last_on_one + 1}) moves the caret and the editor onto sheet 2 "
            f"({count} -> {grown} sheets); status follows",
            "Undo/Redo restore text and sheet count; RTF saved after edit and Undo is byte-identical to the RTF "
            "saved before Print Layout",
            "saved DOC42 project reopens with the typed text and the same sheets; turning Print Layout off keeps the text",
        ]}
    finally:
        process.kill()


SCENARIOS = {"pages": audit_page_canvas, "review": audit_tracked_changes, "layout": audit_print_layout}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--only", nargs="*", choices=sorted(SCENARIOS))
    args = parser.parse_args()
    output = args.output.resolve() / "doc42-model"
    output.mkdir(parents=True, exist_ok=True)
    binary = args.bin_dir.resolve() / "DOC42.exe"
    results = []
    for name in args.only or list(SCENARIOS):
        try:
            results.append(SCENARIOS[name](binary, output))
        except Exception as error:
            results.append({"scenario": name, "error": str(error), "traceback": traceback.format_exc()})
        print(json.dumps(results[-1]), flush=True)
        (output / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    assert all("error" not in result for result in results), results


if __name__ == "__main__":
    main()
