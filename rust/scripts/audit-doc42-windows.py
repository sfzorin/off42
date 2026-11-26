#!/usr/bin/env python3
"""Native DOC42 document-model audits: page canvas, sections and tracked changes.

Each scenario drives the packaged DOC42.exe through its real controls and commands,
checks Undo/Redo, then saves, reopens and inspects the written output.
"""
import argparse
import importlib.util
import json
import re
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


SCENARIOS = {"pages": audit_page_canvas, "review": audit_tracked_changes}


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
