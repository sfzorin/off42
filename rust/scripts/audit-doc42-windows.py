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


SCENARIOS = {"pages": audit_page_canvas}


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
