#!/usr/bin/env python3
"""Exercise native windows and production dialogs through their normal commands."""
import argparse
import csv
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import time
import traceback
import uuid
import zlib
import zipfile

if os.name != "nt":
    raise SystemExit("Run this audit on Windows.")

u = c.WinDLL("user32", use_last_error=True)
u.EnumWindows.argtypes = [c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM), w.LPARAM]
u.EnumChildWindows.argtypes = [w.HWND] + u.EnumWindows.argtypes
u.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
u.GetClassNameW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
u.GetWindowTextLengthW.argtypes = [w.HWND]
u.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
u.GetDlgItem.argtypes = [w.HWND, c.c_int]
u.GetDlgItem.restype = w.HWND
u.GetDlgCtrlID.argtypes = [w.HWND]
u.GetWindowLongW.argtypes=[w.HWND,c.c_int]
u.GetWindowLongW.restype=c.c_long
u.GetParent.argtypes, u.GetParent.restype = [w.HWND], w.HWND
u.GetMenu.argtypes, u.GetMenu.restype = [w.HWND], w.HMENU
u.GetMenuState.argtypes, u.GetMenuState.restype = [w.HMENU, w.UINT, w.UINT], w.UINT
u.IsWindowVisible.argtypes = [w.HWND]
u.IsWindowEnabled.argtypes = [w.HWND]
u.SetForegroundWindow.argtypes = [w.HWND]
u.PostMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
u.SendMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
u.SendMessageW.restype = w.LPARAM
u.SendMessageTimeoutW.argtypes=[w.HWND,w.UINT,w.WPARAM,w.LPARAM,w.UINT,w.UINT,c.POINTER(c.c_size_t)]
u.SendMessageTimeoutW.restype=w.LPARAM
WM_COMMAND, WM_SETTEXT, WM_CLOSE = 0x111, 0xC, 0x10


def bounded_send(hwnd,message,first=0,second=0):
    result=c.c_size_t()
    c.set_last_error(0)
    if not u.SendMessageTimeoutW(hwnd,message,first,second,2,5000,c.byref(result)):
        raise AssertionError(f'Native message {message:#x} timed out or failed; Windows code {c.get_last_error()}')
    return result.value


def text(hwnd):
    try:
        length = bounded_send(hwnd, 0xE)  # WM_GETTEXTLENGTH marshals cross-process controls.
        value = c.create_unicode_buffer(length + 1)
        bounded_send(hwnd, 0xD, len(value), c.cast(value, c.c_void_p).value)
        return value.value
    except AssertionError:
        if c.get_last_error()==1400 or not u.IsWindow(hwnd):return '' # A dialog can close between enumeration and reading its title.
        raise


def replace_selection(hwnd,value,undoable=1):
    data=c.create_unicode_buffer(value)
    try:bounded_send(hwnd,0xC2,undoable,c.cast(data,c.c_void_p).value)
    except AssertionError as error:
        pid=w.DWORD();u.GetWindowThreadProcessId(hwnd,c.byref(pid))
        dialogs=[]
        for h in windows(pid.value):
            if classname(h)=='#32770':
                dialogs.append((text(h),[(classname(child),text(child))for child in windows_for_dialog(h)]))
        raise AssertionError(f'{error}; modal dialogs: {dialogs}')from error


def classname(hwnd):
    value = c.create_unicode_buffer(256)
    u.GetClassNameW(hwnd, value, len(value))
    return value.value


def windows(pid, parent=None):
    found = []
    @c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    def collect(hwnd, _):
        owner = w.DWORD()
        u.GetWindowThreadProcessId(hwnd, c.byref(owner))
        if owner.value == pid and u.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True
    if parent:
        u.EnumChildWindows(parent, collect, 0)
    else:
        u.EnumWindows(collect, 0)
    return found


def wait_for(predicate, description, process, timeout=25):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        result = predicate()
        if result:
            return result
        if process.poll() is not None:
            raise AssertionError(f"App exited while waiting for {description}, code {process.returncode}")
        time.sleep(0.05)
    snapshot = [(classname(h), text(h), [(classname(c), text(c)) for c in windows_for_dialog(h)]) for h in windows(process.pid)]
    raise AssertionError(f"Timed out waiting for {description}. Windows: {snapshot}")


def modal(process, title=None, native_class=None):
    return wait_for(lambda: next((h for h in windows(process.pid)
        if u.IsWindowEnabled(h) and (title is None or text(h) == title)
        and (native_class is None or classname(h) == native_class)), None),
        title or native_class or "modal", process)


def command(hwnd, identifier):
    assert u.PostMessageW(hwnd, WM_COMMAND, identifier, 0)


def save_as(main):
    # Older downloadable previews used 18, which collided with image crop/PDF tools.
    identifier = 65507 if u.GetMenuState(u.GetMenu(main), 65507, 0) != 0xFFFFFFFF else 18
    command(main, identifier)


def click(dialog, identifier):
    button = u.GetDlgItem(dialog, identifier)
    if not button and identifier == 1:
        matches = [h for h in windows_for_dialog(dialog) if classname(h).lower() == "button" and text(h).replace("&", "").lower() in {"ok", "save", "open"}]
        if len(matches) == 1:
            button = matches[0]
    assert button, [(classname(h), u.GetDlgCtrlID(h), text(h)) for h in windows_for_dialog(dialog)]
    u.SetForegroundWindow(dialog)
    assert u.PostMessageW(button, 0xF5, 0, 0)  # BM_CLICK


def windows_for_dialog(dialog):
    pid = w.DWORD()
    u.GetWindowThreadProcessId(dialog, c.byref(pid))
    return windows(pid.value, dialog)


def set_text(hwnd, value):
    data = c.create_unicode_buffer(value)
    u.SendMessageW(hwnd, WM_SETTEXT, 0, c.cast(data, c.c_void_p).value)
    assert text(hwnd) == value, (classname(hwnd), u.GetDlgCtrlID(hwnd), text(hwnd), value)


def replace_editor_text(hwnd, value):
    # WM_SETTEXT does not send EN_CHANGE for a multiline standard EDIT.
    # Replace the selection through the same editing path as typed/pasted text.
    data = c.create_unicode_buffer(value)
    u.SendMessageW(hwnd, 0xB1, 0, -1)  # EM_SETSEL
    replace_selection(hwnd,value)
    assert text(hwnd) == value, (classname(hwnd), u.GetDlgCtrlID(hwnd), text(hwnd), value)


def fill_prompt(process, main, identifier, title, values):
    command(main, identifier)
    dialog = modal(process, title=title, native_class="Suite42InputDialog")
    for index, value in enumerate(values):
        control = u.GetDlgItem(dialog, 100 + index)
        assert control, f"Missing field {index} in {title}"
        set_text(control, value)
    click(dialog, 1)
    wait_for(lambda: not u.IsWindow(dialog), title + " applied and closed", process)
    return dialog


def open_fixture(process,main,path):
    """Replace this isolated audit process's previous fixture through the normal discard prompt."""
    command(main, 1)
    picker = choose_file(process, path)

    def opened():
        return (
            not u.IsWindowVisible(picker)
            and text(main).startswith(path.name + " - ")
            and u.IsWindowEnabled(main)
        )

    def opened_or_discard():
        dialog = next((h for h in windows(process.pid) if classname(h) == "#32770"
                       and all(u.GetDlgItem(h, identifier) for identifier in (2, 6, 7))), None)
        if dialog:
            return "discard", dialog
        if opened():
            return "opened", None
        return None

    state, dialog = wait_for(opened_or_discard, "fixture open or unsaved-change prompt", process)
    if state == "discard":
        click(dialog, 7)
        wait_for(lambda: not u.IsWindowVisible(dialog) and opened(), "previous audit fixture discarded", process)


def choose_file(process, path, format_name=None, keep_preview=False, open_preview=True):
    preview=[None]
    def filename_control():
        for dialog in windows(process.pid):
            if classname(dialog) != "#32770" or not u.IsWindowEnabled(dialog):
                continue
            edits = [h for h in windows(process.pid, dialog) if classname(h).lower() == "edit"]
            control = next((h for h in edits if u.GetDlgCtrlID(h) in (1148, 1001)), edits[0] if len(edits) == 1 else None)
            if control:
                return dialog, control
        if open_preview and preview[0]is None:
            window=next((h for h in windows(process.pid)if classname(h)=="Off42PdfPreviewWindow"and u.IsWindowEnabled(h)),None)
            if window:
                preview[0]=window;click(window,1)
        return None
    dialog, control = wait_for(filename_control, "file dialog filename field", process)
    if format_name is not None:
        def format_selector():
            for combo in windows(process.pid, dialog):
                if classname(combo).lower() != "combobox":
                    continue
                count = u.SendMessageW(combo, 0x146, 0, 0)  # CB_GETCOUNT
                if not 0 < count <= 64:
                    continue
                for index in range(count):
                    length = u.SendMessageW(combo, 0x149, index, 0)  # CB_GETLBTEXTLEN
                    if not 0 <= length < 1024:
                        continue
                    value = c.create_unicode_buffer(length + 1)
                    u.SendMessageW(combo, 0x148, index, c.cast(value, c.c_void_p).value)
                    if value.value == format_name:
                        return combo, index
            return None
        selector, index = wait_for(format_selector, "save format " + format_name, process)
        # CB_SETCURSEL does not emit CBN_* notifications itself. Notify the
        # actual parent so the shell updates the extension and selected filter.
        assert u.SendMessageW(selector, 0x14E, index, 0) == index
        parent, identifier = u.GetParent(selector), u.GetDlgCtrlID(selector)
        for notification in (1, 9):  # CBN_SELCHANGE, CBN_SELENDOK
            u.SendMessageW(parent, WM_COMMAND, identifier | (notification << 16), selector)
        assert u.SendMessageW(selector, 0x147, 0, 0) == index, (text(selector), index)
        dialog, control = wait_for(filename_control, "filename field after format selection", process)
    set_text(control, str(path))
    click(dialog, 1)
    if preview[0]is not None and not keep_preview:
        window=preview[0]
        wait_for(lambda:not u.IsWindow(dialog)and u.IsWindowEnabled(window),"label preview after Save",process)
        click(window,2)
        wait_for(lambda:not u.IsWindow(window),"label preview closed",process)
    return dialog


def output_ready(process, main, path, signature):
    def ready():
        try:
            return path.stat().st_size > 32 and u.IsWindowEnabled(main) and path.read_bytes().startswith(signature)
        except (FileNotFoundError, PermissionError):
            # Atomic replace can briefly remove/lock the destination between the existence check and read.
            return False
    wait_for(ready, str(path), process)


def minimal_pdf(path, labels=None, logical_prefix=None):
    if labels is None:
        objects = [b"<< /Type /Catalog /Pages 2 0 R >>", b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>", b"<< /Length 0 >>\nstream\n\nendstream"]
    else:
        kids = " ".join(f"{3 + i * 2} 0 R" for i in range(len(labels)))
        objects = [b"<< /Type /Catalog /Pages 2 0 R >>", f"<< /Type /Pages /Kids [{kids}] /Count {len(labels)} >>".encode()]
        font = 3 + len(labels) * 2
        for index, label in enumerate(labels):
            assert label.isascii() and not any(char in label for char in "()\\")
            content = f"BT /F1 18 Tf 40 760 Td ({label}) Tj ET".encode()
            objects.extend([f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 {font} 0 R >> >> /Contents {4 + index * 2} 0 R >>".encode(), f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"\nendstream"])
        objects.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    if logical_prefix is not None:
        assert labels and logical_prefix.isascii() and not any(c in logical_prefix for c in "()\\")
        outlines=len(objects)+1;item=outlines+1
        objects[0]=f"<< /Type /Catalog /Pages 2 0 R /PageLabels << /Nums [0 << /S /r /St 4 /P ({logical_prefix}) >>] >> /Outlines {outlines} 0 R >>".encode()
        objects.extend([f"<< /Type /Outlines /First {item} 0 R /Last {item} 0 R /Count 1 >>".encode(),f"<< /Title (Start {logical_prefix}) /Parent {outlines} 0 R /Dest [3 0 R /Fit] >>".encode()])
    data = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for number, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data.extend(f"{number} 0 obj\n".encode() + obj + b"\nendobj\n")
    xref = len(data)
    data.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        data.extend(f"{offset:010d} 00000 n \n".encode())
    data.extend(f"trailer\n<< /Root 1 0 R /Size {len(offsets)} >>\nstartxref\n{xref}\n%%EOF\n".encode())
    path.write_bytes(data)


def write_png(path, width, height, rgba):
    def chunk(name, data):
        return struct.pack(">I", len(data)) + name + data + struct.pack(">I", zlib.crc32(name + data))
    rows = b"".join(b"\0" + rgba[y * width * 4:(y + 1) * width * 4] for y in range(height))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def png_pixels(path):
    data = path.read_bytes()
    assert data.startswith(b"\x89PNG\r\n\x1a\n")
    offset, compressed = 8, bytearray()
    while offset < len(data):
        size = struct.unpack_from(">I", data, offset)[0]
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + size]
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", payload)
            assert depth == 8 and color in (2, 6) and interlace == 0
        elif kind == b"IDAT":
            compressed.extend(payload)
        offset += size + 12
    channels = 4 if color == 6 else 3
    stride = width * channels
    raw, rows, previous = zlib.decompress(compressed), [], bytearray(stride)
    for row in range(height):
        start = row * (stride + 1)
        mode, line = raw[start], bytearray(raw[start + 1:start + stride + 1])
        for index in range(stride):
            left = line[index - channels] if index >= channels else 0
            above = previous[index]
            corner = previous[index - channels] if index >= channels else 0
            if mode == 1:
                predictor = left
            elif mode == 2:
                predictor = above
            elif mode == 3:
                predictor = (left + above) // 2
            elif mode == 4:
                prediction = left + above - corner
                scores = [(abs(prediction - value), order, value) for order, value in enumerate((left, above, corner))]
                predictor = min(scores)[2]
            else:
                assert mode == 0
                predictor = 0
            line[index] = (line[index] + predictor) & 255
        rows.extend(line[i:i + 3] + b"\xff" for i in range(0, stride, channels))
        previous = line
    return width, height, b"".join(rows)


def finish_prompt(process, title, values):
    dialog = modal(process, title=title, native_class="Suite42InputDialog")
    for index, value in enumerate(values):
        control = u.GetDlgItem(dialog, 100 + index)
        assert control
        set_text(control, value)
    click(dialog, 1)


def audit_marks(process, main, binary, output, results):
    source = output / f"audit-mark-{uuid.uuid4().hex}.png"
    write_png(source, 2, 2, bytes([255,255,255,255, 0,0,0,255, 0,0,255,255, 255,255,255,255]))
    library = Path(os.environ["LOCALAPPDATA"]) / "off42" / "Signatures and Stamps" / ("Mark - " + source.stem + ".png")
    try:
        command(main, 65516)
        choose_file(process, source)
        finish_prompt(process, "Import Signature or Stamp", ["yes"])
        message = modal(process, native_class="#32770")
        click(message, 1)
        wait_for(lambda: library.is_file() and u.IsWindowEnabled(main), "shared library import", process)
        command(main, 65515)
        choose_file(process, library)
        finish_prompt(process, "Place Signature or Stamp", ["8", "12", "20", "20"])
        wait_for(lambda: u.IsWindowEnabled(main), "mark placed", process)
        def export(name):
            path = output / f"{binary.stem}-{name}.png"
            if binary.stem.lower() == "scn42":
                command(main, 31)
            else:
                save_as(main)
            choose_file(process, path)
            output_ready(process, main, path, b"\x89PNG\r\n\x1a\n")
            return png_pixels(path)
        marked = export("marked")
        assert marked[:2] == (64, 64)
        pixels = marked[2]
        assert any(value != 255 for value in pixels)
        for y in range(64):
            for x in range(64):
                if not (8 <= x < 28 and 12 <= y < 32):
                    assert pixels[(y * 64 + x) * 4:(y * 64 + x + 1) * 4] == bytes([255] * 4)
        command(main, 4)
        restored = export("undo")
        assert restored[2] == bytes([255] * (64 * 64 * 4))
        command(main, 5)
        assert export("redo") == marked
        if binary.stem.lower() == "scn42":
            project = output / "marked.scn42"
            save_as(main)
            choose_file(process, project)
            wait_for(lambda: project.is_file() and u.IsWindowEnabled(main), "scan project saved", process)
            command(main, 1)
            choose_file(process, project)
            wait_for(lambda: project.name in text(main) and u.IsWindowEnabled(main), "scan project reopened", process)
            assert export("reopened") == marked
        results["checks"].append("shared mark import, white cleanup, placement, pixel bounds, Undo/Redo and saved output")
    finally:
        if library.exists():
            library.unlink()


def audit_sheet_pictures(process, main, output, results):
    source = output / "sheet-own-stamp.png"
    write_png(source, 20, 10, bytes([255, 0, 0, 255] * 200))
    command(main, 76)
    choose_file(process, source)
    wait_for(lambda: u.IsWindowEnabled(main), "picture inserted", process)
    fill_prompt(process, main, 77, "Picture Position and Size", ["2", "2", "6", "6", "30", "15"])
    wait_for(lambda: u.IsWindowEnabled(main), "picture geometry applied", process)
    project = output / "pictures.xls42"
    save_as(main)
    choose_file(process, project)
    output_ready(process, main, project, b"{")
    def picture():
        images = json.loads(project.read_text(encoding="utf-8"))["sheets"][0]["images"]
        return images[0] if images else None
    original = picture()
    assert (original["row"], original["column"], original["width_points"], original["height_points"]) == (1, 1, 30, 15)
    grid = u.GetDlgItem(main, 102)
    assert classname(grid) == "Off42VariableGrid"
    gdi = c.WinDLL("gdi32", use_last_error=True)
    gdi.GetPixel.argtypes = [w.HDC, c.c_int, c.c_int]
    gdi.GetPixel.restype = w.DWORD
    u.GetDC.argtypes, u.GetDC.restype = [w.HWND], w.HDC
    u.ReleaseDC.argtypes = [w.HWND, w.HDC]
    def red(x, y):
        dc = u.GetDC(grid)
        try:
            return gdi.GetPixel(dc, x, y) == 0x0000FF
        finally:
            u.ReleaseDC(grid, dc)
    bounded_send(grid,0x000F)
    capture_window(grid,output/'XLS42-picture-before-drag.bmp')
    wait_for(lambda: red(166, 63), "rendered picture pixels", process)
    def point(x, y):
        return (y << 16) | x
    u.SendMessageW(grid, 0x201, 1, point(166, 63))
    u.SendMessageW(grid, 0x200, 1, point(246, 83))
    u.SendMessageW(grid, 0x202, 0, point(246, 83))
    command(main, 2)
    wait_for(lambda: picture()["column"] == 2 and picture()["row"] == 2, "drag saved at the new anchor", process)
    moved = picture()
    wait_for(lambda: red(246, 83), "picture renders after dragging", process)
    command(main, 4)
    command(main, 2)
    wait_for(lambda: picture() == original, "Undo restores complete picture geometry", process)
    command(main, 5)
    command(main, 2)
    wait_for(lambda: picture() == moved, "Redo restores complete picture geometry", process)
    command(main, 78)
    command(main, 2)
    wait_for(lambda: picture() is None, "picture deletion saved", process)
    command(main, 4)
    command(main, 2)
    wait_for(lambda: picture() == moved, "Undo restores deleted picture and data", process)
    command(main, 1)
    choose_file(process, project)
    wait_for(lambda: project.name in text(main) and u.IsWindowEnabled(main), "picture project reopened", process)
    wait_for(lambda: red(246, 83), "saved picture renders after reopening", process)
    results["checks"].append("spreadsheet picture insertion, exact geometry, native pixels, mouse drag, Undo/Redo, deletion and reopen")


def audit_technical_objects(process, main, binary, output, results):
    fill_prompt(process, main, 65509, "Symbol or Code", ["QR", "PCB-0007", "20", "no"])
    wait_for(lambda: u.IsWindowEnabled(main), "technical object inserted", process)
    def editor_value(expected, change=False):
        command(main, 65508)
        dialog = modal(process, title="Symbol or Code", native_class="Suite42InputDialog")
        actual = text(u.GetDlgItem(dialog, 101))
        assert actual == expected, {"expected": expected, "actual": actual, "saved": text(main)}
        if change:
            set_text(u.GetDlgItem(dialog, 101), "PCB-0008" if change is True else change)
            set_text(u.GetDlgItem(dialog, 102), "25")
        click(dialog, 1 if change else 2)
        wait_for(lambda: u.IsWindowEnabled(main), "technical editor closed", process)
    editor_value("PCB-0007", True)
    extension = {"doc42": "doc42", "xls42": "xls42", "pdf42": "pdf", "ppt42": "json"}[binary.stem.lower()]
    saved = output / f"{binary.stem}-technical.{extension}"
    save_as(main)
    choose_file(process, saved)
    output_ready(process, main, saved, b"%PDF-" if extension == "pdf" else b"{")
    command(main, 4)
    editor_value("PCB-0007")
    command(main, 5)
    editor_value("PCB-0008")
    editor_value("PCB-0008", "PCB-UNSAVED")
    command(main, 1)
    choose_file(process, saved)
    confirmation = wait_for(lambda: next((h for h in windows(process.pid)
        if classname(h) == "#32770" and u.IsWindowEnabled(h)
        and all(u.GetDlgItem(h, identifier) for identifier in (2, 6, 7))), None),
        "unsaved document Save/Discard/Cancel confirmation", process)
    click(confirmation, 7)
    wait_for(lambda: saved.name in text(main) and " * - " not in text(main) and u.IsWindowEnabled(main), "technical document reopened from saved bytes", process)
    editor_value("PCB-0008")
    results["checks"].append("technical object insertion, recipe editing, Undo/Redo and edit after saved-document reopen")


def pdf_text(dll_path, path, by_page=False):
    pdf = c.WinDLL(str(dll_path))
    pdf.FPDF_LoadDocument.argtypes, pdf.FPDF_LoadDocument.restype = [c.c_char_p, c.c_char_p], c.c_void_p
    pdf.FPDF_GetPageCount.argtypes = [c.c_void_p]
    pdf.FPDF_LoadPage.argtypes, pdf.FPDF_LoadPage.restype = [c.c_void_p, c.c_int], c.c_void_p
    pdf.FPDFText_LoadPage.argtypes, pdf.FPDFText_LoadPage.restype = [c.c_void_p], c.c_void_p
    pdf.FPDFText_CountChars.argtypes = [c.c_void_p]
    pdf.FPDFText_GetText.argtypes = [c.c_void_p, c.c_int, c.c_int, c.POINTER(c.c_ushort)]
    for name in ("FPDF_CloseDocument", "FPDF_ClosePage", "FPDFText_ClosePage"):
        getattr(pdf, name).argtypes = [c.c_void_p]
    pdf.FPDF_InitLibrary()
    document = pdf.FPDF_LoadDocument(str(path).encode("utf-8"), None)
    assert document, str(path)
    try:
        output = []
        assert 0 < pdf.FPDF_GetPageCount(document) <= 1000
        for index in range(pdf.FPDF_GetPageCount(document)):
            page = pdf.FPDF_LoadPage(document, index)
            assert page
            text_page = pdf.FPDFText_LoadPage(page)
            assert text_page
            try:
                count = pdf.FPDFText_CountChars(text_page)
                assert 0 <= count <= 500_000
                data = (c.c_ushort * (count + 1))()
                length = pdf.FPDFText_GetText(text_page, 0, count, data)
                output.append(bytes(data)[:max(0, length - 1) * 2].decode("utf-16-le"))
            finally:
                pdf.FPDFText_ClosePage(text_page)
                pdf.FPDF_ClosePage(page)
        return output if by_page else "\n".join(output)
    finally:
        pdf.FPDF_CloseDocument(document)
        pdf.FPDF_DestroyLibrary()


def audit_table_labels(process, main, binary, output, results):
    source = output / "table-label-source.rtf"
    body = br"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}"
    for row in [("Name", "Code", "Copies"), ("Board 0007", "PCB-0007", "2"), ("Board 0008", "PCB-0008", "1")]:
        body += br"\trowd\cellx1600\cellx3200\cellx4000\pard\intbl "
        body += b"\\cell ".join(value.encode("ascii") for value in row) + br"\cell\row "
    body += br"\pard\par}"
    source.write_bytes(body)
    command(main, 1)
    choose_file(process, source)
    wait_for(lambda: source.name in text(main) and u.IsWindowEnabled(main), "table document opened", process)
    command(main, 65518)
    finish_prompt(process, "Label Source", ["Table 1"])
    fields = ["1", "2, 3, 1", "QR", "70, 37, 0, 0", "1, 1"]
    finish_prompt(process, "Create Label Sheet", fields)
    target = output / "table-labels.pdf"
    choose_file(process, target)
    output_ready(process, main, target, b"%PDF-")
    actual = pdf_text(binary.parent / "pdfium.dll", target)
    assert actual.count("Board 0007") == 2, actual
    assert actual.count("Board 0008") == 1, actual
    assert "Name" not in actual and "Copies" not in actual, actual
    assert source.read_bytes() == body and "*" not in text(main)
    command(main, 65518)
    finish_prompt(process, "Label Source", ["Table 1"])
    dialog = modal(process, title="Create Label Sheet", native_class="Suite42InputDialog")
    actual_fields = [text(u.GetDlgItem(dialog, 100 + index)) for index in range(5)]
    assert actual_fields == fields, actual_fields
    click(dialog, 2)
    wait_for(lambda: u.IsWindowEnabled(main), "remembered label settings dismissed", process)
    results["checks"].append("DOC table label source, mapped text/code/copies, skipped header, PDFium text, unchanged source and restored preferences")




def pdf_page_labels(dll_path, path):
    # FPDF_GetPageLabel returns UTF-16LE byte count including the terminator.
    # https://pdfium.googlesource.com/pdfium/+/refs/heads/main/public/fpdf_doc.h
    pdf = c.WinDLL(str(dll_path))
    pdf.FPDF_LoadDocument.argtypes, pdf.FPDF_LoadDocument.restype = [c.c_char_p, c.c_char_p], c.c_void_p
    pdf.FPDF_GetPageCount.argtypes = [c.c_void_p]
    pdf.FPDF_GetPageLabel.argtypes, pdf.FPDF_GetPageLabel.restype = [c.c_void_p, c.c_int, c.c_void_p, c.c_ulong], c.c_ulong
    pdf.FPDF_CloseDocument.argtypes = [c.c_void_p]
    pdf.FPDF_InitLibrary()
    document = pdf.FPDF_LoadDocument(str(path).encode("utf-8"), None)
    assert document
    try:
        labels = []
        for index in range(pdf.FPDF_GetPageCount(document)):
            size = pdf.FPDF_GetPageLabel(document, index, None, 0)
            assert size <= 65536 and size % 2 == 0
            if size == 0:
                labels.append(None)
                continue
            value = c.create_string_buffer(size)
            assert pdf.FPDF_GetPageLabel(document, index, value, size) == size
            labels.append(value.raw[:-2].decode("utf-16-le"))
        return labels
    finally:
        pdf.FPDF_CloseDocument(document)


def audit_page_labels(process, main, binary, output, results):
    expected = ["Плата-aa", "Плата-bb", "Плата-cc"]
    fill_prompt(process, main, 83, "Page Labels", ["1", "3", "a, b, c", "Плата-", "27"])
    def save(name):
        path = output / name
        save_as(main)
        choose_file(process, path)
        output_ready(process, main, path, b"%PDF-")
        return pdf_page_labels(binary.parent / "pdfium.dll", path)
    assert save("page-labels.pdf") == expected
    command(main, 4)
    assert save("page-labels-undo.pdf") == [None, None, None]
    command(main, 5)
    assert save("page-labels-redo.pdf") == expected
    fill_prompt(process, main, 84, "Go to Page", ["Плата-bb"])
    wait_for(lambda: u.SendMessageW(u.GetDlgItem(main, 100), 0x188, 0, 0) == 1, "navigation by custom page label", process)
    command(main, 85)
    assert save("page-labels-clear.pdf") == [None, None, None]
    command(main, 4)
    assert save("page-labels-restored.pdf") == expected
    command(main, 1)
    choose_file(process, output / "page-labels.pdf")
    wait_for(lambda: "page-labels.pdf" in text(main) and u.IsWindowEnabled(main), "labelled document reopened", process)
    fill_prompt(process, main, 84, "Go to Page", ["Плата-cc"])
    wait_for(lambda: u.SendMessageW(u.GetDlgItem(main, 100), 0x188, 0, 0) == 2, "label navigation after reopen", process)
    results["checks"].append("native PDF label range/style/prefix, full Undo/Redo, clearing, independent PDFium labels and navigation after reopen")


def audit_pdf_page_exports(process, main, binary, output, results):
    source = output / "page-export-source.pdf"
    minimal_pdf(source, ["PAGE-ONE", "PAGE-TWO", "PAGE-THREE"])
    original = source.read_bytes()
    command(main, 1)
    choose_file(process, source)
    wait_for(lambda: source.name in text(main) and u.IsWindowEnabled(main), "three-page source opened", process)
    caption = text(main)
    command(main, 80)
    settings = modal(process, title="Extract PDF Pages", native_class="Suite42InputDialog")
    click(settings, 2)
    wait_for(lambda: u.IsWindowEnabled(main), "page settings cancelled", process)
    assert text(main) == caption
    fill_prompt(process, main, 80, "Extract PDF Pages", ["3,1"])
    target = output / "chosen-pages.pdf"
    choose_file(process, target)
    output_ready(process, main, target, b"%PDF-")
    wait_for(lambda: any("exported successfully" in text(h) for h in windows(process.pid, main)), "page export completed", process)
    extracted = pdf_text(binary.parent / "pdfium.dll", target, by_page=True)
    assert [page.strip() for page in extracted] == ["PAGE-THREE", "PAGE-ONE"], extracted
    fill_prompt(process, main, 81, "Split PDF Pages to ZIP", ["3-1"])
    archive = output / "split-pages.zip"
    choose_file(process, archive)
    output_ready(process, main, archive, b"PK")
    wait_for(lambda: any("exported successfully" in text(h) for h in windows(process.pid, main)), "split export completed", process)
    with zipfile.ZipFile(archive) as zip_file:
        expected = [("0001-page-0003.pdf", "PAGE-THREE"), ("0002-page-0002.pdf", "PAGE-TWO"), ("0003-page-0001.pdf", "PAGE-ONE")]
        assert zip_file.namelist() == [name for name, _ in expected]
        for name, label in expected:
            path = output / name
            path.write_bytes(zip_file.read(name))
            assert [page.strip() for page in pdf_text(binary.parent / "pdfium.dll", path, by_page=True)] == [label]
    invalid = output / "invalid-selection.pdf"
    fill_prompt(process, main, 80, "Extract PDF Pages", ["1-2,2"])
    choose_file(process, invalid)
    error = wait_for(lambda: next((h for h in windows(process.pid)
        if classname(h) == "#32770" and u.IsWindowEnabled(h)
        and "only once" in " ".join(text(control) for control in windows_for_dialog(h))), None),
        "overlapping page selection error", process)
    assert "only once" in " ".join(text(h) for h in windows_for_dialog(error))
    click(error, 1)
    wait_for(lambda: u.IsWindowEnabled(main), "invalid page range refused", process)
    assert not invalid.exists() and source.read_bytes() == original and text(main) == caption
    results["checks"].append("native page-list extraction/split ZIP, reverse ordering, independent PDFium text, settings Cancel and overlapping-range refusal preserve the source")
    audit_page_labels(process, main, binary, output, results)



def audit_crop_command(process, main, binary, output, results):
    def export(suffix):
        path = output / f"{binary.stem}-crop-{suffix}.png"
        if binary.stem.lower() == "scn42":
            command(main, 31)
        else:
            save_as(main)
        choose_file(process, path)
        output_ready(process, main, path, b"\x89PNG\r\n\x1a\n")
        return png_pixels(path)
    baseline = export("baseline")
    assert baseline[:2] == (64, 64)
    title = "Crop page" if binary.stem.lower() == "scn42" else "Crop rectangle"
    fill_prompt(process, main, 18, title, ["4", "8", "32", "32"])
    wait_for(lambda: u.IsWindowEnabled(main), "native crop applied", process)
    cropped = export("applied")
    expected = b"".join(baseline[2][(y * 64 + 4) * 4:(y * 64 + 36) * 4] for y in range(8, 40))
    assert cropped == (32, 32, expected)
    command(main, 4)
    assert export("undo") == baseline
    command(main, 5)
    assert export("redo") == cropped
    results["checks"].append("crop has its own native command, exact pixel bounds and full Undo/Redo; Save As remains separate")


def pdf_annotation_counts(dll_path, path):
    pdf = c.WinDLL(str(dll_path))
    pdf.FPDF_LoadDocument.argtypes, pdf.FPDF_LoadDocument.restype = [c.c_char_p, c.c_char_p], c.c_void_p
    pdf.FPDF_GetPageCount.argtypes = [c.c_void_p]
    pdf.FPDF_LoadPage.argtypes, pdf.FPDF_LoadPage.restype = [c.c_void_p, c.c_int], c.c_void_p
    pdf.FPDFPage_GetAnnotCount.argtypes = [c.c_void_p]
    pdf.FPDF_ClosePage.argtypes, pdf.FPDF_CloseDocument.argtypes = [c.c_void_p], [c.c_void_p]
    pdf.FPDF_InitLibrary()
    document = pdf.FPDF_LoadDocument(str(path).encode("utf-8"), None)
    assert document
    try:
        counts = []
        for index in range(pdf.FPDF_GetPageCount(document)):
            page = pdf.FPDF_LoadPage(document, index)
            assert page
            try:
                count = pdf.FPDFPage_GetAnnotCount(page)
                assert count >= 0
                counts.append(count)
            finally:
                pdf.FPDF_ClosePage(page)
        return counts
    finally:
        pdf.FPDF_CloseDocument(document)


def audit_rectangle_command(process, main, binary, output, results):
    def save(suffix):
        path = output / f"rectangle-{suffix}.pdf"
        save_as(main)
        choose_file(process, path)
        output_ready(process, main, path, b"%PDF-")
        return path, pdf_annotation_counts(binary.parent / "pdfium.dll", path)
    _, baseline = save("baseline")
    command(main, 18)
    wait_for(lambda: any("Tool: Rectangle" in text(h) for h in windows(process.pid, main)), "rectangle tool command", process)
    canvas = wait_for(lambda: next((h for h in windows(process.pid, main) if classname(h) == "Suite42Canvas"), None), "rendered PDF canvas", process)
    u.SendMessageW(canvas, 0x201, 1, (80 << 16) | 80)
    u.SendMessageW(canvas, 0x200, 1, (160 << 16) | 180)
    u.SendMessageW(canvas, 0x202, 0, (160 << 16) | 180)
    marked_path, marked = save("applied")
    assert sum(marked) == sum(baseline) + 1
    import re
    assert re.search(rb"/Subtype\s*/Square", marked_path.read_bytes())
    command(main, 4)
    assert save("undo")[1] == baseline
    command(main, 5)
    assert save("redo")[1] == marked
    results["checks"].append("rectangle command is separate from Save As; native mouse drawing, PDFium annotation count and Undo/Redo output")



def audit_pdf_text_exports(process, main, binary, output, results):
    import xml.etree.ElementTree as ET
    from html.parser import HTMLParser
    source = output / "pdf-text-source.pdf"
    minimal_pdf(source, ["PAGE-ONE", "PAGE-TWO", "PAGE-THREE"])
    original = source.read_bytes()
    command(main, 1)
    choose_file(process, source)
    wait_for(lambda: source.name in text(main) and u.IsWindowEnabled(main), "text export source opened", process)
    command(main, 11)  # Delete the first page without saving the PDF.
    wait_for(lambda: " * - " in text(main) and any("Page 1 of 2" in text(h) for h in windows(process.pid, main)), "current PDF page deletion", process)
    caption = text(main)
    command(main, 86)
    settings = modal(process, title="Export PDF Text", native_class="Suite42InputDialog")
    click(settings, 2)
    wait_for(lambda: u.IsWindowEnabled(main), "text settings cancelled", process)
    formats = [("txt", "Plain text (TXT)"), ("rtf", "Rich text (RTF)"), ("html", "Web page (HTML)"), ("docx", "Word document (DOCX)")]
    for extension, name in formats:
        fill_prompt(process, main, 86, "Export PDF Text", ["2,1", name])
        path = output / ("current-pdf-text." + extension)
        choose_file(process, path)
        wait_for(lambda: path.is_file() and any("PDF text exported successfully" in text(h) for h in windows(process.pid, main)), "text export " + extension, process)
        data = path.read_bytes()
        if extension == "docx":
            with zipfile.ZipFile(path) as archive:
                root = ET.fromstring(archive.read("word/document.xml"))
            ns = {"w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main"}
            value = "\n".join(node.text or "" for node in root.findall(".//w:t", ns))
            assert root.find(".//w:pageBreakBefore", ns) is not None
        elif extension == "html":
            class Extract(HTMLParser):
                def __init__(self):
                    super().__init__(); self.parts = []
                def handle_data(self, value):
                    self.parts.append(value)
            parser = Extract(); parser.feed(data.decode("utf-8")); value = "".join(parser.parts)
            assert "page-break-before:always" in data.decode("utf-8")
        elif extension == "rtf":
            assert data.startswith(b"{\\rtf") and b"\\pagebb" in data
            value = data.decode("ascii")
        else:
            value = data.decode("utf-8")
            assert "\f" in value
        assert value.index("PAGE-THREE") < value.index("PAGE-TWO"), value
        assert "PAGE-ONE" not in value, value
        assert source.read_bytes() == original and text(main) == caption
    fill_prompt(process, main, 86, "Export PDF Text", ["1-2", "Plain text (TXT)"])
    picker = modal(process, native_class="#32770")
    click(picker, 2)
    wait_for(lambda: u.IsWindowEnabled(main), "text file picker cancelled", process)
    assert source.read_bytes() == original and text(main) == caption
    command(main, 4)
    wait_for(lambda: " * - " not in text(main), "export leaves page Undo intact", process)
    results["checks"].append("native TXT/RTF/HTML/DOCX exports use current unsaved pages in requested order, retain page boundaries, cancel without changes and preserve PDF source/history")


def copied_selection(edit):
    u.SendMessageW(edit,0x301,0,0)  # WM_COPY operates inside the actual Rich Edit process.
    return clipboard_text()


def clipboard_text():
    u.OpenClipboard.argtypes=[w.HWND]
    u.GetClipboardData.argtypes,u.GetClipboardData.restype=[w.UINT],w.HANDLE
    kernel=c.WinDLL("kernel32",use_last_error=True)
    kernel.GlobalLock.argtypes,kernel.GlobalLock.restype=[w.HANDLE],c.c_void_p
    kernel.GlobalUnlock.argtypes=[w.HANDLE]
    kernel.GlobalSize.argtypes,kernel.GlobalSize.restype=[w.HANDLE],c.c_size_t
    if not u.OpenClipboard(None):return None
    try:
        data=u.GetClipboardData(13)
        if not data:return None
        size=kernel.GlobalSize(data)
        assert 0<size<=65536
        pointer=kernel.GlobalLock(data)
        if not pointer:return None
        try:
            value=c.string_at(pointer,size)
            return value.decode("utf-16-le").split("\0",1)[0]
        finally:kernel.GlobalUnlock(data)
    finally:u.CloseClipboard()


def audit_rich_format(process,main,binary,output,results):
    picture=output/'format-picture.png'
    write_png(picture,2,2,bytes([80,30,180,255])*4)
    source=output/'rich-format-source.rtf'
    source.write_bytes(br'{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}\pard Head \u-10179?\u-8704?\par '
        +br'\trowd\cellx2000\cellx4000\pard\intbl Cell\cell Other\cell\row '
        +br'\pard {\pict\pngblip\picw2\pich2\picwgoal240\pichgoal240 '+picture.read_bytes().hex().encode('ascii')
        +br'}\par After\par Tail\par}')
    original=source.read_bytes()
    open_fixture(process,main,source)
    wait_for(lambda:source.name in text(main)and u.IsWindowEnabled(main),'format fixture opened',process)
    edit=u.GetDlgItem(main,101)
    def saved(name):
        path=output/name;save_as(main);choose_file(process,path,format_name='DOC42 project');output_ready(process,main,path,b'{')
        return json.loads(path.read_text(encoding='utf-8'))['state']
    def selection():
        packed=u.SendMessageW(edit,0xB0,0,0);return packed&0xffff,(packed>>16)&0xffff
    def find(query):
        fill_prompt(process,main,7,'Find next',[query])
        wait_for(lambda:copied_selection(edit)==query,'native formatting selection '+query,process)
        return selection()
    def runs(state):
        paragraphs=[]
        for block in state['blocks']:
            if 'Paragraph'in block:paragraphs.append(block['Paragraph'])
            if 'Table'in block:
                for row in block['Table']['rows']:
                    for cell in row:paragraphs.extend(cell['paragraphs'])
        return [run for paragraph in paragraphs for run in paragraph['runs']]
    baseline=saved('rich-format-before.doc42')
    _,end=find('After');u.SendMessageW(edit,0xB1,end,end)
    assert u.GetMenuState(u.GetMenu(main),300,0)&3==0,'Bold is unavailable at the insertion point'
    caption=text(main)
    command(main,300)
    wait_for(lambda:u.GetMenuState(u.GetMenu(main),300,0)&8,'bold insertion attributes enabled',process)
    assert text(main)==caption,'Changing typing attributes dirtied existing content'
    click(main,303);font=modal(process,native_class='#32770');click(font,2)
    wait_for(lambda:u.IsWindowEnabled(main),'font Cancel',process);assert text(main)==caption
    class GuiThreadInfo(c.Structure):
        _fields_=[('size',w.DWORD),('flags',w.DWORD)]+[(name,w.HWND)for name in['active','focus','capture','menu_owner','move_size','caret']]+[('caret_rect',w.RECT)]
    info=GuiThreadInfo();info.size=c.sizeof(info);u.GetGUIThreadInfo.argtypes=[w.DWORD,c.POINTER(GuiThreadInfo)]
    wait_for(lambda:u.GetGUIThreadInfo(u.GetWindowThreadProcessId(main,None),c.byref(info))and info.focus==edit,'Font Cancel returns keyboard focus to the document',process)
    assert u.GetMenuState(u.GetMenu(main),300,0)&8,'Font Cancel changed insertion formatting'
    replace_selection(edit,' typed',undoable=0)
    wait_for(lambda:' * - 'in text(main),'typed styled text committed',process)
    typed=saved('rich-format-typed.doc42')
    assert any('typed'in run['text']and run['style']['bold']for run in runs(typed)),runs(typed)
    assert any(run['text']=='After'and not run['style']['bold']for run in runs(typed)),runs(typed)
    command(main,4);assert saved('rich-format-typing-undo.doc42')['blocks']==baseline['blocks']
    command(main,5);assert saved('rich-format-typing-redo.doc42')['blocks']==typed['blocks']
    find('Cell');assert u.GetMenuState(u.GetMenu(main),300,0)&3==0,'Bold unavailable in a table cell'
    command(main,300);wait_for(lambda:' * - 'in text(main),'table character style committed',process)
    table=saved('rich-format-table.doc42')
    assert any(run['text']=='Cell'and run['style']['bold']for run in runs(table)),runs(table)
    assert any(run['text']=='Other'and not run['style']['bold']for run in runs(table)),runs(table)
    assert [b for b in table['blocks']if 'Image'in b]==[b for b in baseline['blocks']if 'Image'in b]
    command(main,4);assert saved('rich-format-table-undo.doc42')['blocks']==typed['blocks']
    command(main,5);assert saved('rich-format-table-redo.doc42')['blocks']==table['blocks']
    start,_=find('After');_,end=find('Tail');u.SendMessageW(edit,0xB1,start,end)
    command(main,301);wait_for(lambda:' * - 'in text(main),'multiple paragraph character style',process)
    multi=saved('rich-format-multiple.doc42')
    for word in('After','typed','Tail'):
        assert any(word in run['text']and run['style']['italic']for run in runs(multi)),(word,runs(multi))
    command(main,4);assert saved('rich-format-multiple-undo.doc42')['blocks']==table['blocks']
    command(main,5);assert saved('rich-format-multiple-redo.doc42')['blocks']==multi['blocks']
    reopened=output/'rich-format-multiple.doc42';command(main,1);choose_file(process,reopened)
    wait_for(lambda:reopened.name in text(main)and' * - 'not in text(main),'formatted document reopened',process)
    assert saved('rich-format-reopened.doc42')['blocks']==multi['blocks']
    _,end=find('After');u.SendMessageW(edit,0xB1,end,end)
    command(main,307);wait_for(lambda:' * - 'in text(main),'caret paragraph aligned right',process)
    right=saved('rich-paragraph-right.doc42')
    def paragraph(state,word):
        return next(block['Paragraph']for block in state['blocks']if'Paragraph'in block and word in''.join(run['text']for run in block['Paragraph']['runs']))
    assert paragraph(right,'After')['style']['alignment']=='Right'
    assert paragraph(right,'Tail')['style']['alignment']=='Left'
    command(main,4);assert saved('rich-paragraph-right-undo.doc42')['blocks']==multi['blocks']
    command(main,5);assert saved('rich-paragraph-right-redo.doc42')['blocks']==right['blocks']
    start,_=find('After');_,end=find('Tail');u.SendMessageW(edit,0xB1,start,end)
    command(main,306);wait_for(lambda:' * - 'in text(main),'paragraph range centered',process)
    centered=saved('rich-paragraph-centered.doc42')
    for word in('After','Tail'):assert paragraph(centered,word)['style']['alignment']=='Center'
    assert [block for block in centered['blocks']if'Table'in block]==[block for block in right['blocks']if'Table'in block]
    command(main,4);assert saved('rich-paragraph-centered-undo.doc42')['blocks']==right['blocks']
    command(main,5);assert saved('rich-paragraph-centered-redo.doc42')['blocks']==centered['blocks']
    open_fixture(process,main,output/'rich-paragraph-centered.doc42')
    assert saved('rich-paragraph-reopened.doc42')['blocks']==centered['blocks']
    capture_window(main,output/'DOC42-rich-formatted.bmp')
    assert source.read_bytes()==original
    results['checks'].append('insertion-point formatting leaves existing content clean, typed bold after a picture survives save/Undo/Redo, Font Cancel is readonly, table formatting preserves other cells/images, multi-paragraph italic and caret/range paragraph alignment survive complete Undo/Redo and reopen')


def audit_rich_search(process,main,binary,output,results):
    picture=output/"search-picture.png"
    write_png(picture,2,2,bytes([20,40,60,255])*4)
    original_picture=picture.read_bytes()
    source=output/"rich-search-source.rtf"
    body=br"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}\pard Before \u-10179?\u-8704? TARGET\par "
    body+=br"\trowd\cellx2000\cellx4000\pard\intbl Table TARGET\cell Other cell\cell\row "
    body+=br"\pard {\pict\pngblip\picw2\pich2\picwgoal240\pichgoal240 "+original_picture.hex().encode("ascii")+br"}\par After TARGET\par}"
    source.write_bytes(body);original=source.read_bytes()
    command(main,1);choose_file(process,source)
    wait_for(lambda:source.name in text(main)and u.IsWindowEnabled(main),"rich table/picture source opened",process)
    edit=u.GetDlgItem(main,101)
    def statistics(characters):
        command(main,360)
        dialog=modal(process,title="Statistics",native_class="#32770")
        value=" ".join(text(h)for h in windows_for_dialog(dialog))
        assert "Words: 8" in value and f"Characters (with spaces): {characters}" in value and f"Characters (no spaces): {characters-5}" in value and "Paragraphs: 4" in value,value
        click(dialog,1)
        wait_for(lambda:u.IsWindowEnabled(main),"read-only statistics closed",process)
        wait_for(lambda:any("8 words"in text(h)for h in windows(process.pid,main)),"structured document status word count",process)
    statistics(49)
    u.SendMessageW(edit,0xB1,0,0)
    def selection():
        packed=u.SendMessageW(edit,0xB0,0,0)
        return packed&0xffff,(packed>>16)&0xffff
    positions=[]
    for index in range(4):
        previous=selection()
        fill_prompt(process,main,7,"Find next",["TARGET"])
        expected=positions[0] if index==3 else None
        wait_for(lambda:u.IsWindowEnabled(main)and selection()!=previous and (expected is None or selection()==expected),"find body/table/after-picture wrap",process)
        assert copied_selection(edit)=="TARGET"
        positions.append(selection())
    assert positions[0][0]<positions[1][0]<positions[2][0] and positions[3]==positions[0],positions
    assert " * - " not in text(main) and source.read_bytes()==original
    def saved(name):
        target=output/name
        save_as(main);choose_file(process,target,format_name="DOC42 project")
        output_ready(process,main,target,b"{")
        return json.loads(target.read_text(encoding="utf-8"))
    baseline=saved("rich-search-before.doc42")
    fill_prompt(process,main,8,"Replace all",["TARGET","REPLACED"])
    confirmation=wait_for(lambda:next((h for h in windows(process.pid)if classname(h)=="#32770"and u.IsWindowEnabled(h)and "Replaced 3"in" ".join(text(c)for c in windows_for_dialog(h))),None),"three body/table replacements",process)
    click(confirmation,1)
    modified=saved("rich-search-after.doc42")
    statistics(55)
    blocks=modified["state"]["blocks"]
    paragraphs=[]
    for block in blocks:
        if "Paragraph"in block:paragraphs.append(block["Paragraph"])
        if "Table"in block:
            for row in block["Table"]["rows"]:
                for cell in row:paragraphs.extend(cell["paragraphs"])
    visible="\n".join("".join(run["text"]for run in p["runs"])for p in paragraphs)
    assert visible.count("REPLACED")==3 and "TARGET"not in visible
    assert sum("Table"in block for block in blocks)==1
    pictures=[block["Image"]for block in blocks if "Image"in block]
    assert len(pictures)==1 and bytes(pictures[0]["data"])==original_picture
    assert pictures==[block["Image"]for block in baseline["state"]["blocks"]if "Image"in block]
    command(main,4)
    assert saved("rich-search-undo.doc42")["state"]["blocks"]==baseline["state"]["blocks"]
    command(main,5)
    assert saved("rich-search-redo.doc42")["state"]["blocks"]==blocks
    command(main,4)
    command(main,1);choose_file(process,output/"rich-search-after.doc42")
    confirmation=wait_for(lambda:next((h for h in windows(process.pid)if classname(h)=="#32770"and u.IsWindowEnabled(h)and all(u.GetDlgItem(h,i)for i in (2,6,7))),None),"discard rich replacement undo",process)
    click(confirmation,7)
    wait_for(lambda:"rich-search-after.doc42"in text(main)and" * - "not in text(main),"rich replacement reopened",process)
    u.SendMessageW(edit,0xB1,0,0)
    fill_prompt(process,main,7,"Find next",["REPLACED"])
    wait_for(lambda:selection()[1]-selection()[0]==8,"reopened rich text search",process)
    assert copied_selection(edit)=="REPLACED" and source.read_bytes()==original
    statistics(55)
    results["checks"].append("structured body/table/image statistics retain Unicode grapheme counts through replace/history/reopen; native rich Find traverses body/table/image offsets and wraps; Replace All retains table/image bytes, complete Undo/Redo/save and dirty/discard/reopen")


def audit_doc_search_options(process, main, output, results):
    picture = output / "search-picture.png"
    image = picture.read_bytes()
    source = output / "search-options.rtf"
    body = br"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}\pard \u-10179?\u-8704? \b Cat-17\b0  cat-23 SCAT-99\par "
    body += br"\trowd\cellx2000\cellx4000\pard\intbl cat-42\cell Other cell\cell\row "
    body += br"\pard {\pict\pngblip\picw2\pich2\picwgoal240\pichgoal240 " + image.hex().encode("ascii") + br"}\par After CAT-55\par}"
    source.write_bytes(body)
    open_fixture(process, main, source)
    edit = u.GetDlgItem(main, 101)

    def saved(suffix):
        target = output / ("search-options-" + suffix + ".doc42")
        save_as(main)
        choose_file(process, target, format_name="DOC42 project")
        output_ready(process, main, target, b"{")
        return json.loads(target.read_text(encoding="utf-8"))["state"]

    def find(pattern, match_case, regex, direction, expected):
        fill_prompt(process, main, 7, "Find next", [pattern, match_case, "Yes", regex, direction])
        wait_for(lambda: u.IsWindowEnabled(main) and copied_selection(edit) == expected,
                 "DOC42 find selected " + expected, process)

    baseline = saved("before")
    u.SendMessageW(edit, 0xB1, 0, 0)
    find("cat", "Yes", "No", "Next", "cat")
    u.SendMessageW(edit, 0xB1, 0, 0)
    pattern = r"cat-(\d+)"
    find(pattern, "No", "Yes", "Next", "Cat-17")
    find(pattern, "No", "Yes", "Previous", "CAT-55")
    fill_prompt(process, main, 8, "Replace all", [pattern, "REF:$1", "No", "Yes", "Yes", "Replace"])
    wait_for(lambda: u.IsWindowEnabled(main) and copied_selection(edit) == "Cat-17",
             "replace selection wrapped to next match", process)
    one = saved("one")
    assert one["blocks"][:-1] == baseline["blocks"][:-1]
    assert "REF:55" in "".join(run["text"] for run in one["blocks"][-1]["Paragraph"]["runs"])
    command(main, 4)
    assert saved("one-undo")["blocks"] == baseline["blocks"]
    command(main, 5)
    assert saved("one-redo")["blocks"] == one["blocks"]
    fill_prompt(process, main, 8, "Replace all", [pattern, "ID-$1", "No", "Yes", "Yes", "Replace All"])
    confirmation = wait_for(lambda: next((h for h in windows(process.pid) if classname(h) == "#32770"
        and "Replaced 3" in " ".join(text(c) for c in windows_for_dialog(h))), None),
        "three whole-word capture replacements", process)
    click(confirmation, 1)
    changed = saved("all")
    first = changed["blocks"][0]["Paragraph"]["runs"]
    assert "ID-17 ID-23 SCAT-99" in "".join(run["text"] for run in first)
    assert any("ID-17" in run["text"] and run["style"]["bold"] for run in first)
    table = next(block["Table"] for block in changed["blocks"] if "Table" in block)
    assert table["rows"][0][0]["paragraphs"][0]["runs"][0]["text"] == "ID-42"
    assert [block for block in changed["blocks"] if "Image" in block] == [block for block in baseline["blocks"] if "Image" in block]
    command(main, 4)
    assert saved("all-undo")["blocks"] == one["blocks"]
    command(main, 5)
    assert saved("all-redo")["blocks"] == changed["blocks"]
    open_fixture(process, main, output / "search-options-all.doc42")
    assert saved("reopened")["blocks"] == changed["blocks"]
    assert source.read_bytes() == body and picture.read_bytes() == image
    results["checks"].append("DOC42 case-sensitive whole words, Unicode-aware next/previous, regex captures, one rich replacement with next match, whole-word all replacement, intact bold/table/image, Undo/Redo and normal reopen")


def audit_goal_seek(process,main,binary,output,results):
    source=output/'goal-seek-source.csv'
    with source.open('w',encoding='utf-8',newline='')as stream:
        csv.writer(stream).writerows([['1','=A1^2','UNCHANGED'],['1000','=PMT(0.05/12,120,A2)','OTHER'],['0','=1','CONSTANT']])
    original=source.read_bytes();open_fixture(process,main,source)
    wait_for(lambda:source.name in text(main)and u.IsWindowEnabled(main),'Goal Seek fixture opened',process)
    caption=text(main)
    fill_prompt(process,main,79,'Goal Seek',['B1','2','A1'])
    found=modal(process,title='Goal Seek found a solution',native_class='#32770')
    assert text(main)==caption and source.read_bytes()==original,'Trial calculations changed the live workbook'
    assert 'A1' in' '.join(text(h)for h in windows_for_dialog(found))
    click(found,7);wait_for(lambda:u.IsWindowEnabled(main),'Goal Seek result discarded',process);assert text(main)==caption
    def saved(name):
        path=output/name;save_as(main);choose_file(process,path,format_name='XLS42 workbook');output_ready(process,main,path,b'{')
        return json.loads(path.read_text(encoding='utf-8'))
    baseline=saved('goal-seek-before.xls42')
    fill_prompt(process,main,79,'Goal Seek',['B1','2','A1']);found=modal(process,title='Goal Seek found a solution',native_class='#32770');click(found,6)
    wait_for(lambda:u.IsWindowEnabled(main)and' * - 'in text(main),'Goal Seek value accepted',process)
    changed=saved('goal-seek-after.xls42')
    cells={(cell['row'],cell['col']):cell for cell in changed['sheets'][0]['cells']}
    assert cells[(0,0)]['content']['type']=='Number'and abs(float(cells[(0,0)]['content']['value']['raw'])-2**0.5)<1e-8
    assert cells[(0,1)]['content']['value']['raw']=='=A1^2'
    before={(cell['row'],cell['col']):cell for cell in baseline['sheets'][0]['cells']}
    assert {key:value for key,value in cells.items()if key!=(0,0)}=={key:value for key,value in before.items()if key!=(0,0)}
    command(main,4);assert saved('goal-seek-undo.xls42')['sheets']==baseline['sheets']
    command(main,5);assert saved('goal-seek-redo.xls42')['sheets']==changed['sheets']
    caption=text(main);fill_prompt(process,main,79,'Goal Seek',['B3','2','A3'])
    failed=modal(process,title='Goal Seek',native_class='#32770');click(failed,1);wait_for(lambda:u.IsWindowEnabled(main),'unsolvable Goal Seek closed',process);assert text(main)==caption
    command(main,32);wait_for(lambda:u.GetMenuState(u.GetMenu(main),79,0)&3,'protected Goal Seek disabled',process)
    command(main,33);wait_for(lambda:u.GetMenuState(u.GetMenu(main),79,0)&3==0,'Goal Seek enabled after unprotect',process)
    dirty=' * - 'in text(main);command(main,1);choose_file(process,output/'goal-seek-after.xls42')
    if dirty:
        discard=wait_for(lambda:next((h for h in windows(process.pid)if classname(h)=='#32770'and all(u.GetDlgItem(h,i)for i in(2,6,7))),None),'discard protection history',process);click(discard,7)
    wait_for(lambda:'goal-seek-after.xls42'in text(main)and' * - 'not in text(main),'Goal Seek result reopened',process)
    assert saved('goal-seek-reopened.xls42')['sheets']==changed['sheets'] and source.read_bytes()==original
    results['checks'].append('background Goal Seek trials and No preserve the clean source, accepted square-root value changes one numeric cell, complete one-step Undo/Redo and reopen retain formulas and other cells, unsolvable trials are readonly, and protected sheets disable the command')


def audit_spelling(process,main,binary,output,results):
    import copy
    fixture=Path(__file__).resolve().parents[1]/'crates/doc42-core/fixtures/native-rich-edit-picture.doc42'
    project=json.loads(fixture.read_text(encoding='utf-8'));blocks=project['state']['blocks']
    blocks[0]['Paragraph']['runs'][0]['text']='😀 speling world'
    blocks[0]['Paragraph']['runs'][0]['style']['bold']=True
    table=next(b['Table']for b in blocks if 'Table'in b);table['rows'][0][0]['paragraphs'][0]['runs'][0]['text']='hello speling'
    source=output/'spelling-source.doc42';source.write_text(json.dumps(project,ensure_ascii=False),encoding='utf-8');original=source.read_bytes();open_fixture(process,main,source)
    edit=u.GetDlgItem(main,101)
    def saved(name):
        path=output/name;save_as(main);choose_file(process,path,format_name='DOC42 project');output_ready(process,main,path,b'{');return json.loads(path.read_text(encoding='utf-8'))['state']
    def select_word():
        bounded_send(edit,0xB1,0,0)
        fill_prompt(process,main,7,'Find next',['speling']);wait_for(lambda:copied_selection(edit)=='speling','misspelled word selected',process)
    def start():
        fill_prompt(process,main,65504,'Spelling',['en-US'])
        dialog=modal(process,title='Spelling',native_class='Suite42InputDialog')
        assert text(u.GetDlgItem(dialog,100))=='speling'
        assert u.GetWindowLongW(u.GetDlgItem(dialog,100),-16)&0x800,'Original word must be readonly'
        combo=u.GetDlgItem(dialog,101);count=bounded_send(combo,0x146)
        choices=[]
        for index in range(count):
            buf=c.create_unicode_buffer(2048);bounded_send(combo,0x148,index,c.cast(buf,c.c_void_p).value);choices.append(buf.value)
        assert 'spelling'in choices,choices
        return dialog
    def complete():
        dialog=modal(process,title='Spelling',native_class='#32770');assert 'Spelling check complete.'in' '.join(text(h)for h in windows_for_dialog(dialog));click(dialog,1);wait_for(lambda:u.IsWindowEnabled(main),'spelling completion closed',process)
    baseline=saved('spelling-before.doc42');select_word();caption=text(main)
    dialog=start();capture_window(dialog,output/'DOC42-spelling-dialog.bmp');click(dialog,2);wait_for(lambda:u.IsWindowEnabled(main),'spelling Cancel closed',process)
    assert text(main)==caption and saved('spelling-cancel.doc42')==baseline
    select_word();dialog=start();set_text(u.GetDlgItem(dialog,101),'spelling');set_text(u.GetDlgItem(dialog,102),'Replace');click(dialog,1);complete()
    first=saved('spelling-body.doc42');expected=copy.deepcopy(baseline);expected['blocks'][0]['Paragraph']['runs'][0]['text']='😀 spelling world';assert first==expected,'Body correction changed formatting, tables or picture bytes'
    command(main,4);assert saved('spelling-body-undo.doc42')==baseline
    command(main,5);assert saved('spelling-body-redo.doc42')==first
    select_word();dialog=start();set_text(u.GetDlgItem(dialog,102),'Skip');click(dialog,1);complete();assert saved('spelling-skip.doc42')==first
    select_word();dialog=start();set_text(u.GetDlgItem(dialog,101),'spelling');click(dialog,1);complete()
    second=saved('spelling-table.doc42');expected=copy.deepcopy(first);table=next(b['Table']for b in expected['blocks']if 'Table'in b);table['rows'][0][0]['paragraphs'][0]['runs'][0]['text']='hello spelling';assert second==expected,'Table correction lost native/object offsets or formatting'
    command(main,4);assert saved('spelling-table-undo.doc42')==first
    command(main,5);assert saved('spelling-table-redo.doc42')==second
    open_fixture(process,main,output/'spelling-table.doc42');assert saved('spelling-reopened.doc42')==second and source.read_bytes()==original
    results['checks'].append('real Windows en-US dictionary detects selected misspelling and suggests spelling; original word readonly; Cancel/Skip preserve complete model; corrections after emoji and inside table preserve all formatting/picture bytes, full Undo/Redo/save/reopen/source; no grammar or unavailable-language coverage claimed')


def audit_solver(process,main,binary,output,results):
    source=output/'solver-source.csv'
    with source.open('w',encoding='utf-8',newline='')as stream:
        csv.writer(stream).writerows([['0','0','=3*A1+5*B1','UNCHANGED'],['=A1'],['=2*B1'],['=3*A1+2*B1']])
    original=source.read_bytes();open_fixture(process,main,source)
    def saved(name,kind='XLS42 workbook'):
        path=output/name;save_as(main);choose_file(process,path,format_name=kind)
        output_ready(process,main,path,b'{'if kind=='XLS42 workbook'else b'PK')
        return json.loads(path.read_text(encoding='utf-8'))if kind=='XLS42 workbook'else path
    def cells(project):return {(v['row'],v['col']):v for v in project['sheets'][0]['cells']}
    def number(project,cell):
        content=cells(project)[cell]['content'];assert content['type']=='Number';return float(content['value']['raw'])
    values=['C1','Maximum','0','A1:B1','A2 <= 4\r\nA3 <= 12\r\nA4 <= 18','Yes','Linear (simplex)']
    before=saved('solver-before.xls42');caption=text(main)
    fill_prompt(process,main,82,'Solver',values);found=modal(process,title='Solver found a solution',native_class='#32770')
    assert text(main)==caption and source.read_bytes()==original,'Solver trials mutated the live book'
    details=' '.join(text(h)for h in windows_for_dialog(found));assert 'A1 = 2'in details and'B1 = 6'in details,details
    click(found,7);wait_for(lambda:u.IsWindowEnabled(main),'Solver No closed',process)
    model_only=saved('solver-model-only.xls42');assert cells(model_only)==cells(before)
    model=model_only['sheets'][0]['solver'];assert model['objective']==[0,2]and model['variables']==[[0,0],[0,1]]and len(model['constraints'])==3
    # A saved model prefills all seven native fields, including a multiline constraints control.
    command(main,82);dialog=modal(process,title='Solver',native_class='Suite42InputDialog')
    assert text(u.GetDlgItem(dialog,100))=='C1'and'A3 <= 12'in text(u.GetDlgItem(dialog,104))
    assert u.GetWindowLongW(u.GetDlgItem(dialog,104),-16)&4,'Constraints field must accept multiple lines'
    capture_window(dialog,output/'XLS42-solver-dialog.bmp');click(dialog,1)
    found=modal(process,title='Solver found a solution',native_class='#32770');click(found,6);wait_for(lambda:u.IsWindowEnabled(main),'Solver Yes closed',process)
    changed=saved('solver-after.xls42');assert abs(number(changed,(0,0))-2)<1e-8 and abs(number(changed,(0,1))-6)<1e-8
    assert {k:v for k,v in cells(changed).items()if k not in((0,0),(0,1))}=={k:v for k,v in cells(before).items()if k not in((0,0),(0,1))}
    command(main,4);assert saved('solver-undo.xls42')['sheets']==model_only['sheets']
    command(main,5);assert saved('solver-redo.xls42')['sheets']==changed['sheets']
    for ext,kind in [('xlsx','Excel workbook'),('ods','OpenDocument spreadsheet')]:
        open_fixture(process,main,output/'solver-after.xls42')
        path=saved('solver-office.'+ext,kind)
        with zipfile.ZipFile(path)as archive:
            xml=archive.read('xl/workbook.xml'if ext=='xlsx'else'content.xml').decode('utf-8')
            assert all(name in xml for name in('solver_opt','solver_adj','solver_lhs1','solver_rel3','solver_num'))
            independent=output/('solver-independent.'+ext)
            with zipfile.ZipFile(independent,'w')as target:
                for item in archive.infolist():
                    if item.filename!='xls42/project.json':target.writestr(item,archive.read(item.filename))
        open_fixture(process,main,independent)
        command(main,82);dialog=modal(process,title='Solver',native_class='Suite42InputDialog');assert text(u.GetDlgItem(dialog,100))=='C1'and'A4 <= 18'in text(u.GetDlgItem(dialog,104));click(dialog,2)
        reopened=saved('solver-independent-reopened.'+ext,kind)
        import xml.etree.ElementTree as ET
        with zipfile.ZipFile(reopened)as archive:
            if ext=='xlsx':
                root=ET.fromstring(archive.read('xl/worksheets/sheet1.xml'));ns={'s':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
                actual={cell.attrib['r']:cell.findtext('s:v',namespaces=ns)for cell in root.findall('.//s:c',ns)}
                assert float(actual['A1'])==2 and float(actual['B1'])==6
            else:
                ns={'t':'urn:oasis:names:tc:opendocument:xmlns:table:1.0','o':'urn:oasis:names:tc:opendocument:xmlns:office:1.0'}
                root=ET.fromstring(archive.read('content.xml'));row=root.find('.//t:table/t:table-row',ns)
                actual=[cell.attrib.get('{'+ns['o']+'}value')for cell in row.findall('t:table-cell',ns)]
                assert float(actual[0])==2 and float(actual[1])==6
    integer_source=output/'solver-integer.csv';integer_source.write_text('0,=A1,2.5\n',encoding='utf-8');open_fixture(process,main,integer_source)
    fill_prompt(process,main,82,'Solver',['B1','Maximum','0','A1','A1 <= C1\r\nA1 integer','Yes','Linear (simplex)']);found=modal(process,title='Solver found a solution',native_class='#32770');click(found,6);wait_for(lambda:u.IsWindowEnabled(main),'integer Solver accepted',process)
    integer=saved('solver-integer.xls42');assert number(integer,(0,0))==2
    nonlinear_source=output/'solver-nonlinear.csv';nonlinear_source.write_text('0,0,=(A1-3)^2+(B1-2)^2\n',encoding='utf-8');open_fixture(process,main,nonlinear_source)
    fill_prompt(process,main,82,'Solver',['C1','Minimum','0','A1:B1','','Yes','Nonlinear (Nelder–Mead)']);found=modal(process,title='Solver found a solution',native_class='#32770');click(found,6);wait_for(lambda:u.IsWindowEnabled(main),'nonlinear Solver accepted',process)
    nonlinear=saved('solver-nonlinear.xls42');assert abs(number(nonlinear,(0,0))-3)<1e-5 and abs(number(nonlinear,(0,1))-2)<1e-5
    command(main,32);wait_for(lambda:u.GetMenuState(u.GetMenu(main),82,0)&3,'protected Solver disabled',process)
    command(main,33);wait_for(lambda:u.GetMenuState(u.GetMenu(main),82,0)&3==0,'Solver enabled after unprotect',process)
    assert source.read_bytes()==original
    results['checks'].append('native Solver menu/form and multiline constraints; readonly LP trials; No retains model only; Yes writes exact two-cell optimum; full one-step Undo/Redo; real XLSX/ODS hidden names and independent imports prefill model; integer optimum and nonlinear minimum; protected-sheet disablement; source preserved')


def audit_sheet_tabs(process,main,binary,output,results):
    import xml.etree.ElementTree as ET
    seed=output/'tabs-seed.csv';seed.write_text('FIRST\n',encoding='utf-8');seed_bytes=seed.read_bytes();open_fixture(process,main,seed)
    fill_prompt(process,main,20,'Sheet name',['Second']);replace_editor_text(u.GetDlgItem(main,101),'SECOND')
    def saved(name,kind,extension):
        path=output/('tabs-'+name+'.'+extension);save_as(main);choose_file(process,path,format_name=kind);output_ready(process,main,path,b'{'if extension=='xls42'else b'PK');return path
    native=saved('source','XLS42 workbook','xls42');native_bytes=native.read_bytes()
    def parts(path):
        with zipfile.ZipFile(path)as archive:return {name:archive.read(name)for name in archive.namelist()}
    for extension,kind in [('xlsx','Excel workbook'),('ods','OpenDocument spreadsheet')]:
        open_fixture(process,main,native);source=saved('native-'+extension,kind,extension);independent=output/('tabs-independent.'+extension)
        with zipfile.ZipFile(source)as archive,zipfile.ZipFile(independent,'w')as out:
            for entry in archive.infolist():
                if entry.filename=='xls42/project.json':continue
                data=archive.read(entry)
                if entry.filename in('[Content_Types].xml','META-INF/manifest.xml'):
                    ns='http://schemas.openxmlformats.org/package/2006/content-types'if extension=='xlsx'else'urn:oasis:names:tc:opendocument:xmlns:manifest:1.0';ET.register_namespace(''if extension=='xlsx'else'manifest',ns);root=ET.fromstring(data)
                    for child in list(root):
                        if any(value.lstrip('/')=='xls42/project.json'for value in child.attrib.values()):root.remove(child)
                    data=ET.tostring(root,encoding='utf-8',xml_declaration=True)
                out.writestr(entry,data)
            out.writestr('custom/tab-opaque.bin',b'preserve tab source')
        original=independent.read_bytes();before=parts(independent);open_fixture(process,main,independent)
        wait_for(lambda:text(u.GetDlgItem(main,101))=='SECOND','standard active second sheet imported',process)
        fill_prompt(process,main,24,'Switch sheet',['Sheet1']);wait_for(lambda:text(u.GetDlgItem(main,101))=='FIRST','first sheet selected',process)
        changed=saved('selected-'+extension,kind,extension);after=parts(changed);assert set(after)==set(before)and'xls42/project.json'not in after
        for name,data in before.items():
            if name==('xl/workbook.xml'if extension=='xlsx'else'settings.xml'):continue
            if extension=='xlsx'and name.startswith('xl/worksheets/')and name.endswith('.xml'):
                strip=lambda value:re.sub(rb'<sheetViews\b[^>]*(?:/>|>.*?</sheetViews>)',b'',value,flags=re.S)
                assert strip(data)==strip(after[name]),'Tab selection rewrote worksheet data'
            else:assert data==after[name],(extension,name)
        if extension=='xlsx':
            ns={'s':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'};root=ET.fromstring(after['xl/workbook.xml']);assert root.find('s:bookViews/s:workbookView',ns).attrib['activeTab']=='0'
            for i in(1,2):
                view=ET.fromstring(after[f'xl/worksheets/sheet{i}.xml']).find('s:sheetViews/s:sheetView',ns);assert view is not None and view.attrib.get('tabSelected','0')==('1'if i==1 else'0')
        command(main,4);wait_for(lambda:text(u.GetDlgItem(main,101))=='SECOND','tab Undo restores second sheet',process);undone=saved('undo-'+extension,kind,extension);assert undone.read_bytes()==original
        command(main,5);wait_for(lambda:text(u.GetDlgItem(main,101))=='FIRST','tab Redo restores first sheet',process);assert parts(saved('redo-'+extension,kind,extension))==after
        open_fixture(process,main,changed);wait_for(lambda:text(u.GetDlgItem(main,101))=='FIRST','saved standard active first sheet reopened',process);assert independent.read_bytes()==original
    assert native.read_bytes()==native_bytes and seed.read_bytes()==seed_bytes
    results['checks'].append('normal add/switch-sheet commands and actual editor text; native and independent XLSX/ODS active-sheet import, external tab-only saves and standard single-tab flags, unchanged worksheet data/styles/unknown parts, complete Undo/Redo with byte-exact original restoration and saved reopen')


def audit_ods_number_formats(process,main,binary,output,results):
    import xml.etree.ElementTree as ET
    seed=output/'numbers-source.csv';seed.write_text('0.123456789,1.23456789,42\n=1/3,12345.6789,7\n999.999,12.30001,0.000125\n',encoding='utf-8');original=seed.read_bytes();open_fixture(process,main,seed)
    def saved(name,kind='XLS42 workbook',extension='xls42'):
        path=output/('numbers-'+name+'.'+extension);save_as(main);choose_file(process,path,format_name=kind);output_ready(process,main,path,b'{'if extension=='xls42'else b'PK');return json.loads(path.read_text(encoding='utf-8'))if extension=='xls42'else path
    cases = [
        (0, 0, '0.00%', '12.35%', 0.123456789),
        (1, 0, '0.000', '0.333', 1 / 3),
        (0, 1, '0.00', '1.23', 1.23456789),
        (1, 1, '#,##0.00', '12,345.68', 12345.6789),
        (0, 2, '000000', '000042', 42),
        (1, 2, '00', '07', 7),
        (2, 0, '0.00E+00', '1.00E+03', 999.999),
        (2, 1, '0.0##', '12.3', 12.30001),
        (2, 2, '0.0#E-000', '1.25E-004', 0.000125),
    ]
    for i,(row,col,code,_,_)in enumerate(cases):
        fill_prompt(process,main,25,'Range (1-based)',[str(row+1),str(col+1),str(row+1),str(col+1)])
        if i==len(cases)-1:before=saved('before-last-format')
        fill_prompt(process,main,38,'Number format',[code])
    changed=saved('formatted');command(main,4);undone=saved('format-undo');assert undone['sheets']==before['sheets']and undone['styles']==before['styles']and undone['selection']==before['selection'];command(main,5);redone=saved('format-redo');assert redone['sheets']==changed['sheets']and redone['styles']==changed['styles']and redone['selection']==changed['selection']
    native=saved('native','OpenDocument spreadsheet','ods');ns={'o':'urn:oasis:names:tc:opendocument:xmlns:office:1.0','s':'urn:oasis:names:tc:opendocument:xmlns:style:1.0','t':'urn:oasis:names:tc:opendocument:xmlns:table:1.0','n':'urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0','p':'urn:oasis:names:tc:opendocument:xmlns:text:1.0'}
    def inspect(path):
        with zipfile.ZipFile(path)as archive:root=ET.fromstring(archive.read('content.xml'))
        table=root.find('.//t:table',ns);rows=table.findall('t:table-row',ns);cells={(r,c):v for r,row in enumerate(rows)for c,v in enumerate(row.findall('t:table-cell',ns))};styles={v.attrib['{'+ns['s']+'}name']:v for v in root.findall('.//s:style',ns)};formats={v.attrib['{'+ns['s']+'}name']:v for v in root.iter()if v.tag in('{'+ns['n']+'}number-style','{'+ns['n']+'}percentage-style')}
        return cells,styles,formats
    cells,styles,formats=inspect(native)
    for row,col,code,display,value in cases:
        cell=cells[(row,col)];assert cell.findtext('p:p',namespaces=ns)==display,(row,col,ET.tostring(cell));assert float(cell.attrib['{'+ns['o']+'}value'])==value
        style=styles[cell.attrib['{'+ns['t']+'}style-name']];fmt=formats[style.attrib['{'+ns['s']+'}data-style-name']];assert fmt.tag=='{'+ns['n']+'}'+('percentage-style'if code.endswith('%')else'number-style');assert cell.attrib['{'+ns['o']+'}value-type']==('percentage'if code.endswith('%')else'float')
        if 'E' in code:
            scientific = fmt.find('n:scientific-number', ns)
            assert scientific is not None
            assert scientific.get('{' + ns['n'] + '}forced-exponent-sign') == str('E+' in code).lower()
            assert int(scientific.get('{' + ns['n'] + '}min-exponent-digits')) == len(code.split('E')[1]) - 1
        elif '#' in code.split('.')[-1]:
            decimal = fmt.find('n:number', ns)
            assert decimal.get('{' + ns['n'] + '}min-decimal-places') == '1'
            assert decimal.get('{' + ns['n'] + '}decimal-places') == '3'
    replace_editor_text(u.GetDlgItem(main,101),'REOPEN-STALE');open_fixture(process,main,native);restored=saved('reopened');assert restored['sheets']==changed['sheets']and restored['styles']==changed['styles']
    independent=output/'numbers-independent.ods'
    with zipfile.ZipFile(native)as archive,zipfile.ZipFile(independent,'w')as out:
        for entry in archive.infolist():
            if entry.filename=='xls42/project.json':continue
            data=archive.read(entry)
            if entry.filename=='META-INF/manifest.xml':
                manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0';ET.register_namespace('manifest',manifest);root=ET.fromstring(data)
                for child in list(root):
                    if any(v.lstrip('/')=='xls42/project.json'for v in child.attrib.values()):root.remove(child)
                data=ET.tostring(root,encoding='utf-8',xml_declaration=True)
            out.writestr(entry,data)
        out.writestr('custom/number-opaque.bin',b'keep foreign number part')
    source_bytes=independent.read_bytes();open_fixture(process,main,independent);wait_for(lambda:text(u.GetDlgItem(main,101))=='0.123456789','independent ODS full precision input',process)
    replace_editor_text(u.GetDlgItem(main,101),'0.987654321');edited=saved('edited','OpenDocument spreadsheet','ods');cells,_,_=inspect(edited);assert cells[(0,0)].findtext('p:p',namespaces=ns)=='98.77%'and float(cells[(0,0)].attrib['{'+ns['o']+'}value'])==0.987654321;assert cells[(0,0)].attrib['{'+ns['o']+'}value-type']=='percentage';assert cells[(1,0)].findtext('p:p',namespaces=ns)=='0.333'
    with zipfile.ZipFile(edited)as archive:assert archive.read('custom/number-opaque.bin')==b'keep foreign number part'and'xls42/project.json'not in archive.namelist()
    command(main,4);undo=saved('external-undo','OpenDocument spreadsheet','ods');assert undo.read_bytes()==source_bytes
    command(main,5);redo=saved('external-redo','OpenDocument spreadsheet','ods');assert inspect(redo)[0][(0,0)].findtext('p:p',namespaces=ns)=='98.77%'
    replace_editor_text(u.GetDlgItem(main,101),'STALE');open_fixture(process,main,edited);wait_for(lambda:text(u.GetDlgItem(main,101))=='0.987654321','independent formatted ODS edited value reopens',process)
    assert independent.read_bytes()==source_bytes and seed.read_bytes()==original
    results['checks'].append('normal Number Format controls for fixed and optional decimals, grouped numbers, percentages, zero padding and scientific notation; complete style/value/selection Undo/Redo; standard ODS number-style links, formatted text caches and exact unrounded numeric attributes independently checked; native and snapshot-free foreign reopen, percentage and scalar-formula cache edits, unknown part preserved, byte-exact external Undo and unchanged sources')


def audit_advanced_sort(process,main,binary,output,results):
    import xml.etree.ElementTree as ET
    seed=output/'sort-source.csv';seed.write_text('Priority,Part,ID\nlow,R10,first\nHIGH,R2,second\nMedium,R1,third\nlow,R02,fourth\n',encoding='utf-8');original=seed.read_bytes();open_fixture(process,main,seed)
    def saved(name,kind='XLS42 workbook',extension='xls42'):
        path=output/('sort-'+name+'.'+extension);save_as(main);choose_file(process,path,format_name=kind);output_ready(process,main,path,b'{'if extension=='xls42'else b'PK');return json.loads(path.read_text(encoding='utf-8'))if extension=='xls42'else path
    def cells(project):return {(v['row'],v['col']):v['content']['value']['raw']for v in project['sheets'][0]['cells']}
    fill_prompt(process,main,25,'Range (1-based)',['1','1','8','3']);before=saved('before')
    fill_prompt(process,main,85,'Advanced Sort',['1 asc\n2 asc','Rows','yes','no','yes','high;medium;low']);changed=saved('multi')
    assert [cells(changed)[(row,2)]for row in range(1,5)]==['second','third','fourth','first'];assert [cells(changed)[(0,col)]for col in range(3)]==['Priority','Part','ID']
    command(main,4);undo=saved('undo');assert undo['sheets']==before['sheets']and undo['selection']==before['selection'];command(main,5);redo=saved('redo');assert redo['sheets']==changed['sheets']and redo['selection']==changed['selection']
    for extension,kind in [('xlsx','Excel workbook'),('ods','OpenDocument spreadsheet')]:
        path=saved('multi-'+extension,kind,extension);replace_editor_text(u.GetDlgItem(main,101),'REOPEN-STALE');open_fixture(process,main,path);assert cells(saved('reopened-'+extension))==cells(changed)
    assert seed.read_bytes()==original
    horizontal=output/'sort-horizontal.csv';horizontal.write_text('Label,R10,R2,R1,R02\nValue,=B3*2,=C3*2,=D3*2,=E3*2\nN,10,20,30,40\n',encoding='utf-8');horizontal_bytes=horizontal.read_bytes();open_fixture(process,main,horizontal)
    fill_prompt(process,main,25,'Range (1-based)',['1','1','3','5']);before=saved('horizontal-before');fill_prompt(process,main,85,'Advanced Sort',['1 asc','Columns','yes','no','yes','']);changed=saved('horizontal')
    assert [cells(changed)[(0,col)]for col in range(5)]==['Label','R1','R2','R02','R10'];assert [cells(changed)[(1,col)]for col in range(1,5)]==['=B3*2','=C3*2','=D3*2','=E3*2'];assert [cells(changed)[(2,col)]for col in range(1,5)]==['30','20','40','10']
    path=saved('horizontal-xlsx','Excel workbook','xlsx');ns={'s':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
    with zipfile.ZipFile(path)as archive:root=ET.fromstring(archive.read('xl/worksheets/sheet1.xml'))
    cached={v.attrib['r']:v.findtext('s:v',namespaces=ns)for v in root.findall('.//s:c',ns)};assert [cached[key]for key in('B2','C2','D2','E2')]==['60','40','80','20']
    command(main,4);undo=saved('horizontal-undo');assert undo['sheets']==before['sheets']and undo['selection']==before['selection'];command(main,5);redo=saved('horizontal-redo');assert redo['sheets']==changed['sheets']and redo['selection']==changed['selection'];assert horizontal.read_bytes()==horizontal_bytes
    model=saved('full-seed');sheet=model['sheets'][0];sheet['rows']=1048576;sheet['cols']=16384;sheet['cells']=[]
    for row,col,kind,raw in[(0,0,'Number','3'),(0,16383,'Text','THREE'),(99,0,'Number','2'),(99,16383,'Text','TWO'),(1048575,0,'Number','1'),(1048575,16383,'Text','ONE')]:sheet['cells'].append({'row':row,'col':col,'content':{'type':kind,'value':{'raw':raw,'crlf':False}},'style':0})
    model['selection']={'top':0,'left':0,'bottom':0,'right':0};source=output/'sort-full-source.xls42';source.write_text(json.dumps(model),encoding='utf-8');source_bytes=source.read_bytes();open_fixture(process,main,source)
    fill_prompt(process,main,25,'Range (1-based)',['1','1','1048576','16384']);before=saved('full-before');fill_prompt(process,main,28,'Sort selection',['1','no']);changed=saved('full')
    assert cells(changed)=={(0,0):'1',(0,16383):'ONE',(1,0):'2',(1,16383):'TWO',(2,0):'3',(2,16383):'THREE'}
    command(main,4);undo=saved('full-undo');assert undo['sheets']==before['sheets']and undo['selection']==before['selection'];command(main,5);redo=saved('full-redo');assert redo['sheets']==changed['sheets']and redo['selection']==changed['selection']
    replace_editor_text(u.GetDlgItem(main,101),'99');open_fixture(process,main,output/'sort-full-redo.xls42');assert cells(saved('full-reopened'))==cells(changed);assert source.read_bytes()==source_bytes
    results['checks'].append('normal Advanced Sort multiline keys and choices; custom priorities, natural part numbers, stable header and complete rows; horizontal column sort translates every formula and independently checked XLSX caches; full Excel-size sparse row sort moves XFD data; complete dimensions/data/selection Undo/Redo, XLSX/ODS and native reopen; sources unchanged')


def audit_grid_expansion(process,main,binary,output,results):
    seed=output/'growth-source.csv';seed.write_text('KEEP\n',encoding='utf-8');original=seed.read_bytes();open_fixture(process,main,seed)
    def saved(name):
        path=output/('growth-'+name+'.xls42');save_as(main);choose_file(process,path,format_name='XLS42 workbook');output_ready(process,main,path,b'{');return json.loads(path.read_text(encoding='utf-8'))
    before=saved('before');grid=u.GetDlgItem(main,102)
    bounded_send(grid,0x115,7,0);bounded_send(grid,0x114,7,0)  # Native bottom-scroll commands extend viewport space.
    scrolled=saved('scrolled');assert scrolled['sheets']==before['sheets']and scrolled['selection']==before['selection'],'Scrolling changed workbook data'
    fill_prompt(process,main,25,'Range (1-based)',['501','31','501','31']);edit=u.GetDlgItem(main,101);wait_for(lambda:text(edit)=='','virtual empty cell selected',process)
    selected=saved('selected');assert selected['sheets']==before['sheets']and selected['selection']==before['selection'],'Blank viewport selection allocated document cells'
    replace_editor_text(edit,'=10+20');changed=saved('edited');sheet=changed['sheets'][0];assert (sheet['rows'],sheet['cols'])==(501,31)
    cells=lambda project:{(v['row'],v['col']):v for v in project['sheets'][0]['cells']}
    assert cells(changed)[(500,30)]['content']['value']['raw']=='=10+20'and {k:v for k,v in cells(changed).items()if k!=(500,30)}==cells(before)
    bounded_send(grid,0x000F);capture_window(grid,output/'XLS42-expanded-grid.bmp')
    command(main,4);undone=saved('undo');assert undone['sheets']==before['sheets']and undone['selection']==before['selection'],'Growth was not undone with the cell edit'
    command(main,5);redone=saved('redo');assert redone['sheets']==changed['sheets']and redone['selection']==changed['selection']
    fill_prompt(process,main,25,'Range (1-based)',['800','41','800','41']);command(main,31);formatted=saved('formatted');assert (formatted['sheets'][0]['rows'],formatted['sheets'][0]['cols'])==(800,41)
    style=cells(formatted)[(799,40)]['style'];assert formatted['styles'][style]['bold']and cells(formatted)[(799,40)]['content']['value']['raw']==''
    command(main,4);format_undo=saved('format-undo');assert format_undo['sheets']==changed['sheets']and format_undo['selection']==changed['selection'];command(main,5);format_redo=saved('format-redo');assert format_redo['sheets']==formatted['sheets']and format_redo['selection']==formatted['selection']
    fill_prompt(process,main,25,'Range (1-based)',['900','51','900','51'])
    # Copy real text from the standard editor, then invoke the spreadsheet's normal Paste command.
    replace_editor_text(edit,'10\t20\r\n30\t=1+2');u.SendMessageW(edit,0xB1,0,-1);u.SendMessageW(edit,0x301,0,0);assert clipboard_text()=='10\t20\r\n30\t=1+2'
    command(main,4)  # Undo the temporary source cell so Paste is the only operation under test.
    baseline=saved('paste-baseline');command(main,40);pasted=saved('pasted');assert (pasted['sheets'][0]['rows'],pasted['sheets'][0]['cols'])==(901,52)
    assert {(r,c):v['content']['value']['raw']for(r,c),v in cells(pasted).items()if r>=899}=={(899,50):'10',(899,51):'20',(900,50):'30',(900,51):'=1+2'}
    command(main,4);paste_undo=saved('paste-undo');assert paste_undo['sheets']==baseline['sheets']and paste_undo['selection']==baseline['selection'];command(main,5);paste_redo=saved('paste-redo');assert paste_redo['sheets']==pasted['sheets']and paste_redo['selection']==pasted['selection']
    replace_editor_text(edit,'REOPEN-STALE');open_fixture(process,main,output/'growth-paste-redo.xls42');wait_for(lambda:text(edit)=='10','expanded project reopens at saved selection',process)
    fill_prompt(process,main,25,'Range (1-based)',['1200','81','1200','81']);fill_prompt(process,main,65509,'Symbol or Code',['QR','PCB-GROW-42','20','no'])
    artwork=saved('artwork');assert (artwork['sheets'][0]['rows'],artwork['sheets'][0]['cols'])==(1200,81)
    import base64
    picture=artwork['sheets'][0]['images'][0];assert (picture['row'],picture['column'])==(1199,80)and picture['data'];assert artwork['sheets'][0]['cells']==pasted['sheets'][0]['cells']
    prefix,encoded=picture['description'].split(':',1);assert prefix=='Suite42.TechnicalElement.v1'
    assert json.loads(base64.b64decode(encoded,validate=True))=={'codeType':'qr','value':'PCB-GROW-42','widthMM':20.0,'caption':False}
    png=base64.b64decode(picture['data'],validate=True);assert png.startswith(b'\x89PNG\r\n\x1a\n');offset=8;descriptions=[]
    while offset+12<=len(png):
        size=int.from_bytes(png[offset:offset+4],'big');tag=png[offset+4:offset+8];chunk=png[offset+8:offset+8+size]
        if tag==b'tEXt'and chunk.startswith(b'Description\0'):descriptions.append(chunk.split(b'\0',1)[1].decode('utf-8'))
        offset+=size+12
    assert descriptions==[picture['description']],'Saved picture recipe and PNG recipe differ'
    command(main,4);art_undo=saved('artwork-undo');assert art_undo['sheets']==pasted['sheets']and art_undo['selection']==pasted['selection'];command(main,5);art_redo=saved('artwork-redo');assert art_redo['sheets']==artwork['sheets']and art_redo['selection']==artwork['selection']
    fill_prompt(process,main,77,'Picture Position and Size',['1400','101','5','7','60','60']);moved=saved('artwork-moved');assert (moved['sheets'][0]['rows'],moved['sheets'][0]['cols'])==(1400,101)
    next_picture=moved['sheets'][0]['images'][0];assert (next_picture['row'],next_picture['column'],next_picture['offset_x'],next_picture['offset_y'],next_picture['width_points'],next_picture['height_points'])==(1399,100,5,7,60,60)
    assert {k:v for k,v in picture.items()if k not in('row','column','offset_x','offset_y','width_points','height_points')}=={k:v for k,v in next_picture.items()if k not in('row','column','offset_x','offset_y','width_points','height_points')}
    command(main,4);move_undo=saved('artwork-move-undo');assert move_undo['sheets']==artwork['sheets']and move_undo['selection']==artwork['selection'];command(main,5);move_redo=saved('artwork-move-redo');assert move_redo['sheets']==moved['sheets']and move_redo['selection']==moved['selection']
    fill_prompt(process,main,25,'Range (1-based)',['1600','121','1602','121']);fill_prompt(process,main,75,'Fill Serial Numbers',['PCB-GROW-','7','2','4'])
    serial=saved('serial');assert (serial['sheets'][0]['rows'],serial['sheets'][0]['cols'])==(1602,121)
    assert [cells(serial)[(row,120)]['content']['value']['raw']for row in(1599,1600,1601)]==['PCB-GROW-0007','PCB-GROW-0009','PCB-GROW-0011']
    assert all(cells(serial)[(row,120)]['content']['type']=='Text'for row in(1599,1600,1601))
    command(main,4);serial_undo=saved('serial-undo');assert serial_undo['sheets']==moved['sheets']and serial_undo['selection']==moved['selection'];command(main,5);serial_redo=saved('serial-redo');assert serial_redo['sheets']==serial['sheets']and serial_redo['selection']==serial['selection']
    assert seed.read_bytes()==original
    results['checks'].append('native scroll extends blank grid without document mutation; normal far-range selection remains virtual; actual text edit, empty-cell Bold and clipboard Paste allocate only written bounds with complete one-step dimensions/data/style/selection Undo/Redo and saved reopen; technical QR insertion and geometry move extend allocation atomically with exact recipe/payload retention and complete history; source unchanged')


def audit_sparse_capacity(process,main,binary,output,results):
    import xml.etree.ElementTree as ET
    seed=output/'sparse-seed.csv';seed.write_text('2,KEEP\n',encoding='utf-8');open_fixture(process,main,seed)
    def saved(name,kind='XLS42 workbook'):
        path=output/('sparse-'+name+('.xls42'if kind=='XLS42 workbook'else '.xlsx'));save_as(main);choose_file(process,path,format_name=kind);output_ready(process,main,path,b'{'if kind=='XLS42 workbook'else b'PK')
        return json.loads(path.read_text(encoding='utf-8'))if kind=='XLS42 workbook'else path
    model=saved('seed');sheet=model['sheets'][0];sheet['rows']=1048576;sheet['cols']=16384
    sheet['cells'][0]['content']={'type':'Formula','value':{'raw':'=XFD1048576*3','crlf':False}}
    sheet['cells'].append({'row':1048575,'col':16383,'content':{'type':'Number','value':{'raw':'7','crlf':False}},'style':0})
    for row,kind,raw,col in [(1,'Formula','=SUM(XFD:XFD)',0),(2,'Formula','=COUNTBLANK(XFD:XFD)',0),(3,'Error','#N/A',0),(1,'Text','#N/A',1),(4,'Formula','=SUMIF(XFD:XFD,">0")',0),(5,'Formula','=COUNTIF(XFD:XFD,">0")',0),(6,'Formula','=SUMIFS(XFD:XFD,XFD:XFD,">0")',0),(7,'Formula','=AVERAGEIF(XFD:XFD,">0")',0),(8,'Formula','=MINIFS(XFD:XFD,XFD:XFD,">0")',0),(9,'Formula','=COUNTIFS(XFD:XFD,"")',0)]:sheet['cells'].append({'row':row,'col':col,'content':{'type':kind,'value':{'raw':raw,'crlf':False}},'style':0})
    source=output/'sparse-full-size.xls42';source.write_text(json.dumps(model),encoding='utf-8');original=source.read_bytes();open_fixture(process,main,source)
    grid=u.GetDlgItem(main,102);assert classname(grid)=='Off42VariableGrid'
    fill_prompt(process,main,25,'Range (1-based)',['1048576','16384','1048576','16384'])
    edit=u.GetDlgItem(main,101);wait_for(lambda:text(edit)=='7','last Excel cell selected',process)
    class ScrollInfo(c.Structure):_fields_=[('cbSize',w.UINT),('fMask',w.UINT),('nMin',c.c_int),('nMax',c.c_int),('nPage',w.UINT),('nPos',c.c_int),('nTrackPos',c.c_int)]
    u.GetScrollInfo.argtypes=[w.HWND,c.c_int,c.POINTER(ScrollInfo)];u.GetScrollInfo.restype=w.BOOL
    def scroll_position(bar):
        info=ScrollInfo();info.cbSize=c.sizeof(info);info.fMask=0x17;assert u.GetScrollInfo(grid,bar,c.byref(info));return info.nPos
    wait_for(lambda:scroll_position(1)>20_000_000 and scroll_position(0)>1_300_000,'far range selection scrolls both native axes',process)
    # Flush the invalidated paint before sampling; fast automation can otherwise capture the old A1 frame.
    bounded_send(grid,0x000F)
    rect=w.RECT();u.GetClientRect.argtypes=[w.HWND,c.POINTER(w.RECT)];assert u.GetClientRect(grid,c.byref(rect))
    gdi=c.WinDLL('gdi32',use_last_error=True);gdi.GetPixel.argtypes=[w.HDC,c.c_int,c.c_int];gdi.GetPixel.restype=w.DWORD
    u.GetDC.argtypes=[w.HWND];u.GetDC.restype=w.HDC;u.ReleaseDC.argtypes=[w.HWND,w.HDC]
    dc=u.GetDC(grid)
    try:assert gdi.GetPixel(dc,rect.right-5,rect.bottom-5)==(210|(228<<8)|(252<<16)),'Far selected cell did not render at the scrolled viewport edge'
    finally:u.ReleaseDC(grid,dc)
    capture_window(grid,output/'XLS42-last-cell.bmp');before=saved('before')
    replace_editor_text(edit,'11');changed=saved('after')
    def cells(project):return {(v['row'],v['col']):v for v in project['sheets'][0]['cells']}
    assert cells(changed)[(1048575,16383)]['content']['value']['raw']=='11'
    assert {k:v for k,v in cells(before).items()if k!=(1048575,16383)}=={k:v for k,v in cells(changed).items()if k!=(1048575,16383)}
    path=saved('after','Excel workbook')
    def verify_xlsx(path):
        ns={'s':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
        with zipfile.ZipFile(path)as archive:root=ET.fromstring(archive.read('xl/worksheets/sheet1.xml'))
        values={cell.attrib['r']:cell.findtext('s:v',namespaces=ns)for cell in root.findall('.//s:c',ns)}
        assert values['XFD1048576']=='11'and values['A1']=='33'and values['A2']=='11'and values['A3']=='1048575',values
        assert {key:values[key]for key in('A5','A6','A7','A8','A9','A10')}==dict(zip(('A5','A6','A7','A8','A9','A10'),('11','1','11','11','11','1048575'))),values
        cells={cell.attrib['r']:cell for cell in root.findall('.//s:c',ns)};assert cells['A4'].attrib.get('t')=='e'and values['A4']=='#N/A';assert cells['B2'].attrib.get('t')in('s','inlineStr'),'Error-looking text lost its distinct string type'

    verify_xlsx(path)
    command(main,4);undone=saved('undo');assert undone['sheets']==before['sheets']and undone['selection']==before['selection']
    command(main,5);redone=saved('redo');assert redone['sheets']==changed['sheets']and redone['selection']==changed['selection']
    open_fixture(process,main,output/'sparse-redo.xls42');verify_xlsx(saved('reopened','Excel workbook'))
    assert source.read_bytes()==original
    results['checks'].append('full 1048576 by 16384 sparse workbook opens through the normal dialog; selecting XFD1048576 scrolls both axes, native editing changes only that cell, saved XLSX has independently checked far value, dependent A1/SUM/COUNTBLANK caches and distinct literal-error/text cell types, complete one-step Undo/Redo and reopen preserve every sheet cell and original source')


def audit_engineering_formulas(process, main, binary, output, results, include_extended=False):
    source = output / "engineering-formulas.csv"
    records = [
        ["16", "", "", "", "", "", "", ""],
        ["13", "=BITAND(A2,25)", "", "", "", "", "", ""],
        ["1", '=CONVERT(A3,"Mibyte","bit")', "", "", "", "", "", ""],
        ["FF", "=HEX2DEC(A4)", "", "", "", "", "", ""],
        ["-1", "=DEC2BIN(A5)", "", "", "", "", "", ""],
    ]
    if include_extended:
        records.extend([
            ["a--b", '=TEXTJOIN("-",FALSE,"a","","b")', "", "", "", "", "", ""],
            ["1.234,5", '=NUMBERVALUE(A7,",",".")', "", "", "", "", "", ""],
            ["BOARD-42", '=TEXTAFTER(A8,"-")', "", "", "", "", "", ""],
            ["ПЛАТА", '=UNICODE(A9)', "", "", "", "", "", ""],
            ["", '=CONCAT(A6:A9)', "", "", "", "", "", ""],
            ["", '=TEXTJOIN("|",FALSE,A10:A11)', "", "", "", "", "", ""],
            ["10", '=COMBIN(A12,3)', "", "", "", "", "", ""],
            ["5.5", '=CEILING.MATH(-A13,2,1)', "", "", "", "", "", ""],
            ["100", '=SUMSQ(A12:A14)', "", "", "", "", "", ""],
            ["24", '=GCD(A12,A15)', "", "", "", "", "", ""],
            ["0", '=ATAN2(1,0)', "", "", "", "", "", ""],
            ["7", '=FACTDOUBLE(A17)', "", "", "", "", "", ""],
            ["4", '=MULTINOMIAL(2,3,A18)', "", "", "", "", "", ""],
            ["12", '=TRUNC(-A19/10,0)', "", "", "", "", "", ""],
            ["0", '=COT(A20)', "", "", "", "", "", ""],
            ["0", '=PERMUT(10,3)', "", "", "", "", "", ""],
        ])
    localized_start=len(records)+1
    records.extend([
        ["",'=СУММ({1,2;3,4};5)',"","","","","",""],
        ["",'=ЕСЛИОШИБКА(#ДЕЛ/0!;9)',"","","","","",""],
        ["",'=СЧЁТЕСЛИ(A1:A5;">0")',"","","","","",""],
        ["",'=СЦЕП("a;";"СУММ(1;2)")',"","","","","",""],
    ])
    with source.open("w", encoding="utf-8", newline="") as file:
        csv.writer(file).writerows(records)
    original = source.read_bytes()
    open_fixture(process,main,source)
    wait_for(lambda: source.name in text(main) and u.IsWindowEnabled(main), "engineering source opened", process)
    fill_prompt(process, main, 25, "Range (1-based)", ["1", "2", "1", "2"])
    wait_for(lambda: any("R1 C2" in text(h) for h in windows(process.pid, main)), "B1 selected", process)
    edit = u.GetDlgItem(main, 101)
    replace_editor_text(edit, "=DEC2HEX($A$1,4)")
    def save(name):
        target = output / name
        save_as(main)
        choose_file(process, target, format_name="Excel workbook")
        output_ready(process, main, target, b"PK")
        import xml.etree.ElementTree as ET
        with zipfile.ZipFile(target) as archive:
            sheet = ET.fromstring(archive.read("xl/worksheets/sheet1.xml"))
        ns = {"s": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
        cells = {cell.attrib["r"]: (cell.findtext("s:f", namespaces=ns), cell.findtext("s:v", namespaces=ns), cell.attrib.get("t")) for cell in sheet.findall(".//s:c", ns)}
        return target, cells
    saved, cells = save("engineering-results.xlsx")
    expected = {"B1":"0010", "B2":"9", "B3":"8388608", "B4":"255", "B5":"1111111111"}
    if include_extended:
        expected.update({"B6":"a--b", "B7":"1234.5", "B8":"42", "B9":"1055", "B10":"a--b1.234,5BOARD-42ПЛАТА", "B11":"|"})
        expected.update({"B12":"120", "B13":"-6", "B14":"10130.25", "B15":"2", "B16":"0", "B17":"105", "B18":"1260", "B19":"-1", "B20":"#DIV/0!", "B21":"720"})
    expected.update({'B'+str(localized_start+i):value for i,value in enumerate(['15','9','3','a;СУММ(1;2)'])})
    assert cells['B'+str(localized_start)][0]=='SUM({1,2;3,4},5)'
    assert cells['B'+str(localized_start+1)][0]=='IFERROR(#DIV/0!,9)'
    for address, value in expected.items():
        assert cells[address][1] == value, (address, cells[address])
    assert cells["B1"][0] == "DEC2HEX($A$1,4)"
    assert cells["B1"][2] == "str" and cells["B5"][2] == "str", "Formula strings must preserve leading zeroes and text type"
    if include_extended:
        assert all(cells[address][2] == "str" for address in ["B6", "B8", "B10", "B11"])
        assert cells["B20"][2] == "e"
        assert all(cells["B" + str(row)][2] in (None,"n") for row in [12,13,14,15,16,17,18,19,21])
    command(main, 4)
    wait_for(lambda: text(edit) == "", "Undo typed engineering formula", process)
    command(main, 5)
    wait_for(lambda: text(edit) == "=DEC2HEX($A$1,4)", "Redo engineering formula", process)
    replace_editor_text(edit, "=DEC2HEX($A$1,2)")
    command(main, 1)
    choose_file(process, saved)
    confirmation = wait_for(lambda: next((h for h in windows(process.pid)
        if classname(h) == "#32770" and u.IsWindowEnabled(h)
        and all(u.GetDlgItem(h, identifier) for identifier in (2, 6, 7))), None), "discard unsaved formula edit", process)
    click(confirmation, 7)
    wait_for(lambda: saved.name in text(main) and " * - " not in text(main) and u.IsWindowEnabled(main), "engineering XLSX reopened from saved bytes", process)
    fill_prompt(process, main, 25, "Range (1-based)", ["1", "2", "1", "2"])
    wait_for(lambda: text(u.GetDlgItem(main, 101)) == "=DEC2HEX($A$1,4)", "saved B1 formula after range selection", process)
    _, restored = save("engineering-reopened.xlsx")
    for address, value in expected.items():
        assert restored[address][1] == value
    replace_editor_text(u.GetDlgItem(main,101),'=СУММ(1;2)');_,localized=save('engineering-localized-typed.xlsx')
    assert localized['B1'][0]=='SUM(1,2)'and localized['B1'][1]=='3'
    command(main,4);wait_for(lambda:text(u.GetDlgItem(main,101))=='=DEC2HEX($A$1,4)','Undo localized formula preserves raw input',process)
    command(main,5);wait_for(lambda:text(u.GetDlgItem(main,101))=='=СУММ(1;2)','Redo localized formula preserves raw input',process)
    results['checks'].append('actual Russian formula entry retains editor text through Undo/Redo; native XLSX uses canonical English functions, comma arguments, intact inline-array rows/quoted semicolons and localized constant-error results')
    assert source.read_bytes() == original
    results["checks"].append("engineering formula typed through native editor, independent XLSX cached results, Undo/Redo and discard/reopen preserve formulas and source")

    if include_extended:
        results["checks"].append("native XLSX math/text functions, numeric/error/text cache types, explicit numeric separators, Unicode and blank range slots survive discard/reopen")


def audit_ppt_show(process, main, output, results):
    import xml.etree.ElementTree as ET

    def text_value(raw):
        return {"raw": raw, "crlf": False}

    colors = ([200, 20, 20], [20, 160, 20], [20, 20, 200])
    slides = []
    for index, color in enumerate(colors):
        slides.append({
            "id": index + 1, "title_text": text_value(f"Slide {index + 1}"),
            "body_text": text_value(""), "next_object_id": 2,
            "objects": [{"id": 1, "kind": "Rectangle", "x": 200, "y": 100, "width": 320,
                         "height": 205, "fill": color, "stroke": color, "text": ""}],
        })
    source = output / "ppt-show-source.json"
    source.write_text(json.dumps({"version": 2, "next_id": 4, "slides": slides}), encoding="utf-8")
    original = source.read_bytes()
    open_fixture(process, main, source)

    def saved(name, extension, kind):
        path = output / f"ppt-show-{name}.{extension}"
        save_as(main)
        choose_file(process, path, format_name=kind)
        output_ready(process, main, path, b"{" if extension == "json" else b"PK")
        return path

    listing = u.GetDlgItem(main, 100)
    u.SendMessageW(listing, 0x186, 1, 0)
    command(main, 100 | (1 << 16))
    wait_for(lambda: u.SendMessageW(listing, 0x188, 0, 0) == 1, "second slide selected", process)
    before = saved("before", "json", "off42 slides JSON")
    command(main, 73)
    hidden = saved("hidden", "json", "off42 slides JSON")
    assert json.loads(hidden.read_text(encoding="utf-8"))["slides"][1]["hidden"]
    command(main, 4)
    undo = saved("undo", "json", "off42 slides JSON")
    assert undo.read_bytes() == before.read_bytes()
    command(main, 5)
    redo = saved("redo", "json", "off42 slides JSON")
    assert redo.read_bytes() == hidden.read_bytes()
    pptx = saved("hidden", "pptx", "PowerPoint presentation")
    odp = saved("hidden", "odp", "OpenDocument presentation")
    with zipfile.ZipFile(pptx) as package:
        assert ET.fromstring(package.read("ppt/slides/slide2.xml")).get("show") == "0"
        assert ET.fromstring(package.read("ppt/slides/slide1.xml")).get("show") is None
    with zipfile.ZipFile(odp) as package:
        content = ET.fromstring(package.read("content.xml"))
        ns = {"s": "urn:oasis:names:tc:opendocument:xmlns:style:1.0",
              "d": "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0",
              "p": "urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"}
        pages = content.findall(".//d:page", ns)
        styles = {style.get("{" + ns["s"] + "}name"): style for style in content.findall(".//s:style", ns)}
        second = styles[pages[1].get("{" + ns["d"] + "}style-name")]
        assert second.find("s:drawing-page-properties", ns).get("{" + ns["p"] + "}visibility") == "hidden"
    open_fixture(process, main, pptx)
    fill_prompt(process, main, 72, "Slide Show Settings", ["No", "No"])
    gdi = c.WinDLL("gdi32", use_last_error=True)
    gdi.GetPixel.argtypes, gdi.GetPixel.restype = [w.HDC, c.c_int, c.c_int], w.DWORD
    u.GetDC.argtypes, u.GetDC.restype = [w.HWND], w.HDC
    u.ReleaseDC.argtypes = [w.HWND, w.HDC]
    u.GetWindowRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
    u.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
    u.RedrawWindow.argtypes = [w.HWND, c.c_void_p, c.c_void_p, w.UINT]

    def placement():
        rect = w.RECT()
        assert u.GetWindowRect(main, c.byref(rect))
        return rect.left, rect.top, rect.right, rect.bottom

    def playing():
        return not u.GetMenu(main) and not u.IsWindowVisible(listing)

    def current_canvas():
        return next((h for h in windows(process.pid, main)
                     if classname(h) == "Suite42Canvas" and u.IsWindowVisible(h)), None)

    def expect(color, description, uniform=False):
        value = color[0] | color[1] << 8 | color[2] << 16
        def actual():
            canvas = current_canvas()
            if not canvas:
                return False
            rect = w.RECT()
            u.GetClientRect(canvas, c.byref(rect))
            u.RedrawWindow(canvas, None, None, 0x101)
            dc = u.GetDC(canvas)
            try:
                points = [(rect.right // 2, rect.bottom // 2)]
                if uniform:
                    points.extend([(5, 5), (rect.right - 6, rect.bottom - 6)])
                return all(gdi.GetPixel(dc, x, y) == value for x, y in points)
            finally:
                u.ReleaseDC(canvas, dc)
        wait_for(actual, description, process)

    def key(value):
        assert u.PostMessageW(current_canvas(), 0x100, value, 0)

    prior = placement()
    command(main, 71)
    wait_for(playing, "PPT42 playing in its window", process)
    assert placement() == prior and not u.GetWindowLongW(main, -16) & 0x80000000
    expect(colors[0], "first slide rendered")
    key(66)
    expect([0, 0, 0], "black screen covers slide and surrounding area", uniform=True)
    key(39)
    expect(colors[0], "first navigation reveals without moving")
    key(ord("2"))
    key(13)
    expect(colors[2], "numeric jump skips the hidden slide")
    key(87)
    expect([255, 255, 255], "white screen covers slide and surrounding area", uniform=True)
    key(36)
    expect(colors[0], "Home returns to the first visible slide")
    key(35)
    expect(colors[2], "End goes to the final visible slide")
    assert u.PostMessageW(main, 0x10, 0, 0)
    wait_for(lambda: bool(u.GetMenu(main)) and u.IsWindowVisible(listing), "window close exits show", process)
    assert placement() == prior and u.SendMessageW(listing, 0x188, 0, 0) == 2
    fill_prompt(process, main, 72, "Slide Show Settings", ["Yes", "Yes"])
    open_fixture(process, main, odp)
    command(main, 67)
    wait_for(playing, "fullscreen slide show started", process)
    assert u.GetWindowLongW(main, -16) & 0x80000000
    expect(colors[0], "fullscreen first slide")
    key(39)
    expect(colors[1], "preference includes hidden slide after reopening")
    key(35)
    expect(colors[2], "fullscreen last slide")
    key(39)
    expect(colors[0], "loop returns to the beginning")
    key(ord("Q"))
    wait_for(lambda: bool(u.GetMenu(main)) and u.IsWindowVisible(listing), "fullscreen exit restores editor", process)
    fill_prompt(process, main, 72, "Slide Show Settings", ["No", "No"])
    assert source.read_bytes() == original
    results["checks"].append("PPT42 normal Hide Slide and complete history, native PPTX/ODP visibility metadata, windowed and fullscreen pixels, black/white screens, navigation and skipping, persisted include-hidden and loop, restored editor window")


def audit_preset_shapes(process,main,output,results):
    import xml.etree.ElementTree as ET
    command(main,68)
    picker=modal(process,title="Insert shape",native_class="Suite42InputDialog")
    selector=u.GetDlgItem(picker,100)
    assert classname(selector).lower()=="combobox" and u.SendMessageW(selector,0x146,0,0)==187
    set_text(selector,"Round Rect")
    click(picker,1)
    wait_for(lambda:u.IsWindowEnabled(main),"preset inserted",process)
    fill_prompt(process,main,26,"Object geometry",["300","150","180","90","0"])
    wait_for(lambda:u.IsWindowEnabled(main),"preset positioned",process)
    project=output/"preset-shapes.json"
    save_as(main)
    choose_file(process,project,format_name="off42 slides JSON")
    output_ready(process,main,project,b"{")
    def current():
        def available():
            try:source=project.read_text(encoding="utf-8")
            except (PermissionError,FileNotFoundError):return None
            objects=json.loads(source)["slides"][0]["objects"]
            return next(o for o in objects if o.get("preset"))
        return wait_for(available,"read preset project after atomic save",process)
    original=current()
    assert original["preset"]=={"name":"roundRect"}
    canvas=wait_for(lambda:next((h for h in windows(process.pid,main) if classname(h)=="Suite42Canvas"),None),"PPT canvas",process)
    gdi=c.WinDLL("gdi32",use_last_error=True)
    gdi.GetPixel.argtypes=[w.HDC,c.c_int,c.c_int]
    gdi.GetPixel.restype=w.DWORD
    u.GetDC.argtypes,u.GetDC.restype=[w.HWND],w.HDC
    u.ReleaseDC.argtypes=[w.HWND,w.HDC]
    def pixel(x,y):
        dc=u.GetDC(canvas)
        try:return gdi.GetPixel(dc,x,y)
        finally:u.ReleaseDC(canvas,dc)
    fill=220|(230<<8)|(245<<16)
    wait_for(lambda:pixel(390,195)==fill and pixel(303,153)==0xFFFFFF,"rounded shape filled curves",process)
    assert pixel(320,153)==fill,hex(pixel(320,153))
    def point(x,y):return (y<<16)|x
    u.SendMessageW(canvas,0x201,1,point(315,150))
    u.SendMessageW(canvas,0x200,1,point(330,150))
    u.SendMessageW(canvas,0x202,0,point(330,150))
    command(main,2)
    wait_for(lambda:current()!=original,"handle drag persisted",process)
    moved=current()
    assert 33000<moved["preset"]["adjustments"]["adj"]<33700,moved
    assert all(moved[k]==original[k] for k in ("id","x","y","width","height","rotation_degrees"))
    command(main,4);command(main,2)
    wait_for(lambda:current()==original,"handle Undo",process)
    command(main,5);command(main,2)
    wait_for(lambda:current()==moved,"handle Redo",process)
    fill_prompt(process,main,70,"Shape adjustments",["40000"])
    wait_for(lambda:u.IsWindowEnabled(main),"shape adjustment applied",process)
    wait_for(lambda:pixel(320,153)==0xFFFFFF,"curve radius changes actual pixels",process)
    fill_prompt(process,main,69,"Shape type",["Donut"])
    wait_for(lambda:u.IsWindowEnabled(main),"shape type changed",process)
    wait_for(lambda:pixel(390,195)==0xFFFFFF and pixel(310,195)==fill,"compound even-odd donut fill",process)
    capture_window(main,output/"PPT42-preset-donut.bmp")
    command(main,2)
    wait_for(lambda:current()["preset"]["name"]=="donut","shape type persisted",process)
    office=[]
    for extension,format_name in [("pptx","PowerPoint presentation"),("odp","OpenDocument presentation")]:
        path=output/("preset-shapes."+extension)
        save_as(main);choose_file(process,path,format_name=format_name);output_ready(process,main,path,b"PK")
        with zipfile.ZipFile(path) as archive:
            if extension=="pptx":
                root=ET.fromstring(archive.read("ppt/slides/slide1.xml"))
                assert any(n.attrib.get("prst")=="donut" for n in root.iter() if n.tag.endswith("}prstGeom"))
            else:
                root=ET.fromstring(archive.read("content.xml"))
                assert any(n.attrib.get("{urn:oasis:names:tc:opendocument:xmlns:drawing:1.0}type")=="ooxml-donut" for n in root.iter())
        office.append(path)
    fill_prompt(process,main,70,"Shape adjustments",["12000"])
    wait_for(lambda:u.IsWindowEnabled(main),"unsaved shape change",process)
    command(main,1);choose_file(process,office[-1])
    confirmation=wait_for(lambda:next((h for h in windows(process.pid) if classname(h)=="#32770" and u.IsWindowEnabled(h) and all(u.GetDlgItem(h,i) for i in (2,6,7))),None),"discard unsaved preset",process)
    click(confirmation,7)
    wait_for(lambda:office[-1].name in text(main) and " * - " not in text(main) and u.IsWindowEnabled(main),"ODP preset reopened",process)
    wait_for(lambda:pixel(390,195)==0xFFFFFF and pixel(310,195)==fill,"reopened preset actual pixels",process)
    u.SendMessageW(canvas,0x201,1,point(310,195));u.SendMessageW(canvas,0x202,0,point(310,195))
    fill_prompt(process,main,70,"Shape adjustments",["35000"])
    wait_for(lambda:u.IsWindowEnabled(main),"reopened preset editable",process)
    overlay=output/"preset-overlay.odp"
    save_as(main);choose_file(process,overlay,format_name="OpenDocument presentation");output_ready(process,main,overlay,b"PK")
    with zipfile.ZipFile(overlay) as archive:
        root=ET.fromstring(archive.read("content.xml"))
        geometries=[n for n in root.iter() if n.attrib.get("{urn:oasis:names:tc:opendocument:xmlns:drawing:1.0}type")=="ooxml-donut"]
        assert len(geometries)==1
        assert geometries[0].attrib["{urn:sfzorin:ppt42}adjust"]=="adj=35000"
    results["checks"].append("187-shape picker, actual curved/compound fill pixels, handle drag, one-step Undo/Redo, radius/type edits, PPTX/ODP output, dirty/discard/reopen and editable ODP overlay")


def pdf_pixels(dll_path,path,index=0,annotations=False):
    """Render the saved PDF independently of the app's WinRT canvas/cache."""
    pdf=c.WinDLL(str(dll_path))
    pdf.FPDF_LoadDocument.argtypes,pdf.FPDF_LoadDocument.restype=[c.c_char_p,c.c_char_p],c.c_void_p
    pdf.FPDF_LoadPage.argtypes,pdf.FPDF_LoadPage.restype=[c.c_void_p,c.c_int],c.c_void_p
    pdf.FPDF_GetPageWidth.argtypes,pdf.FPDF_GetPageWidth.restype=[c.c_void_p],c.c_double
    pdf.FPDF_GetPageHeight.argtypes,pdf.FPDF_GetPageHeight.restype=[c.c_void_p],c.c_double
    pdf.FPDFBitmap_Create.argtypes,pdf.FPDFBitmap_Create.restype=[c.c_int,c.c_int,c.c_int],c.c_void_p
    pdf.FPDFBitmap_FillRect.argtypes=[c.c_void_p,c.c_int,c.c_int,c.c_int,c.c_int,w.DWORD]
    pdf.FPDF_RenderPageBitmap.argtypes=[c.c_void_p,c.c_void_p,c.c_int,c.c_int,c.c_int,c.c_int,c.c_int,c.c_int]
    pdf.FPDFBitmap_GetBuffer.argtypes,pdf.FPDFBitmap_GetBuffer.restype=[c.c_void_p],c.c_void_p
    pdf.FPDFBitmap_GetStride.argtypes,pdf.FPDFBitmap_GetStride.restype=[c.c_void_p],c.c_int
    for name in ("FPDF_ClosePage","FPDF_CloseDocument","FPDFBitmap_Destroy"):getattr(pdf,name).argtypes=[c.c_void_p]
    pdf.FPDF_InitLibrary();document=pdf.FPDF_LoadDocument(str(path).encode(),None);assert document
    page=None;bitmap=None
    try:
        page=pdf.FPDF_LoadPage(document,index);assert page
        width,height=round(pdf.FPDF_GetPageWidth(page)),round(pdf.FPDF_GetPageHeight(page))
        assert 0<width*height<=4_000_000
        bitmap=pdf.FPDFBitmap_Create(width,height,1);assert bitmap
        pdf.FPDFBitmap_FillRect(bitmap,0,0,width,height,0xFFFFFFFF)
        pdf.FPDF_RenderPageBitmap(bitmap,page,0,0,width,height,0,int(annotations))
        stride=pdf.FPDFBitmap_GetStride(bitmap)
        pixels=c.string_at(pdf.FPDFBitmap_GetBuffer(bitmap),stride*height)
        return width,height,pixels
    finally:
        if bitmap:pdf.FPDFBitmap_Destroy(bitmap)
        if page:pdf.FPDF_ClosePage(page)
        pdf.FPDF_CloseDocument(document)


def audit_pdf_decorations(process,main,binary,output,results):
    source=output/"decorations-source.pdf"
    minimal_pdf(source,["BODY-ONE","BODY-TWO","BODY-THREE"]);original=source.read_bytes()
    def open_file(path):
        command(main,1);choose_file(process,path)
        def state():
            if path.name in text(main) and " * - " not in text(main) and u.IsWindowEnabled(main):return (True,None)
            prompt=next((h for h in windows(process.pid)if classname(h)=="#32770" and u.IsWindowEnabled(h) and all(u.GetDlgItem(h,i)for i in (2,6,7))),None)
            return (False,prompt) if prompt else None
        opened,prompt=wait_for(state,"PDF decoration open/discard",process)
        if not opened:
            click(prompt,7)
            wait_for(lambda:path.name in text(main) and " * - " not in text(main) and u.IsWindowEnabled(main),"PDF decoration committed open",process)
    open_file(source)
    command(main,10)
    def save(name):
        path=output/("decorations-"+name+".pdf")
        save_as(main);choose_file(process,path,format_name="PDF document");output_ready(process,main,path,b"%PDF-")
        return path
    baseline=save("baseline")
    fill_prompt(process,main,90,"Text Watermark",["AUDIT-WATERMARK","36","70","0","#D21923","1,3"])
    wait_for(lambda:u.IsWindowEnabled(main),"watermark added",process)
    marked=save("watermarked")
    assert "AUDIT-WATERMARK" not in pdf_text(binary.parent/"pdfium.dll",marked)
    def red(path,index=0):
        _,_,pixels=pdf_pixels(binary.parent/"pdfium.dll",path,index)
        return sum(pixels[i+2]>pixels[i+1]+30 and pixels[i+2]>pixels[i]+30 for i in range(0,len(pixels),4))
    assert red(baseline)==0 and red(marked)>100
    assert red(marked,1)==0 and red(marked,2)>100
    command(main,4);undone=save("watermark-undo");assert red(undone)==0 and red(undone,2)==0
    command(main,5);redone=save("watermark-redo");assert pdf_pixels(binary.parent/"pdfium.dll",redone)==pdf_pixels(binary.parent/"pdfium.dll",marked)
    fill_prompt(process,main,91,"Page Numbers",["Bottom center","Page n of total","5","Yes","10","28","3-1"])
    wait_for(lambda:u.IsWindowEnabled(main),"numbered selected pages",process)
    caption=text(main)
    fill_prompt(process,main,93,"Bates Numbering Settings",["PCB-","-X","6","7","2026-11-07"])
    wait_for(lambda:u.IsWindowEnabled(main),"Bates settings changed",process);assert text(main)==caption
    fill_prompt(process,main,92,"Header & Footer",["FILE <<file>>","Sheet <<label>> <<page>>/<<pages>>","<<date>>","<<bates>>","Проверено","END","10","28","36","3-1"])
    wait_for(lambda:u.IsWindowEnabled(main),"header/footer added",process)
    numbered=save("headers")
    pages=pdf_text(binary.parent/"pdfium.dll",numbered,by_page=True)
    for i,page in enumerate(pages):
        assert "PCB-00000"+str(i+7)+"-X" in page and "Проверено" in page and "2026-11-07" in page and redone.name in page,(i,page)
        assert ["BODY-ONE","BODY-TWO","BODY-THREE"][i] in page
    assert "Page 5 of 6" not in pages[0] and "Page 5 of 6" in pages[1] and "Page 6 of 6" in pages[2]
    command(main,96);removed=save("headers-removed");assert "PCB-" not in pdf_text(binary.parent/"pdfium.dll",removed)
    command(main,4);restored=save("headers-undo");assert pdf_text(binary.parent/"pdfium.dll",restored,by_page=True)==pages
    command(main,95);without=save("watermarks-removed");texts=pdf_text(binary.parent/"pdfium.dll",without,by_page=True)
    assert red(without)==0 and all("Page 5 of 6" not in page and "Page 6 of 6" not in page for page in texts)
    assert all("PCB-" in page for page in texts)
    command(main,4)
    image=output/"watermark-own-image.png";write_png(image,2,1,bytes([0,0,255,128,0,0,255,0]))
    fill_prompt(process,main,94,"Image Watermark",["80","0","2"]);choose_file(process,image)
    wait_for(lambda:u.IsWindowEnabled(main),"image watermark added",process)
    image_saved=save("image")
    _,_,pixels=pdf_pixels(binary.parent/"pdfium.dll",image_saved,1)
    assert sum(pixels[i]>pixels[i+1]+20 and pixels[i]>pixels[i+2]+20 for i in range(0,len(pixels),4))>1000
    fill_prompt(process,main,90,"Text Watermark",["UNSAVED","20","50","0","#D21923","1"])
    wait_for(lambda:u.IsWindowEnabled(main),"unsaved decoration edit",process)
    open_file(numbered)
    assert u.GetMenuState(u.GetMenu(main),95,0)&3, "Baked marks are page content after reopen"
    reopened=save("reopened")
    assert pdf_text(binary.parent/"pdfium.dll",reopened,by_page=True)==pages
    assert pdf_pixels(binary.parent/"pdfium.dll",reopened)==pdf_pixels(binary.parent/"pdfium.dll",numbered)
    assert source.read_bytes()==original
    results["checks"].append("text/image watermarks with opacity, selected-page pixels/rotation, searchable Unicode headers/Bates/date/file tokens, ordered numbering, atomic Undo/Redo/removal and saved PDFium text/pixels")


def audit_pdf_header_fonts(process,main,binary,output,results,include_print=False):
    source=output/'header-fonts-source.pdf';minimal_pdf(source,['FONT-BODY-ONE','FONT-BODY-TWO']);original=source.read_bytes()
    choices=[('Arial','Arial'),('Arial Bold','Arial-Bold'),('Times New Roman','TimesNewRoman'),('Times New Roman Bold','TimesNewRomanPS-Bold'),('Courier New','CourierNew')]
    rendered=[]
    def save(name):
        path=output/('header-font-'+name+'.pdf');save_as(main);choose_file(process,path,format_name='PDF document');output_ready(process,main,path,b'%PDF-');return path
    for index,(choice,font_name)in enumerate(choices):
        open_fixture(process,main,source)
        fill_prompt(process,main,92,'Header & Footer',['','WWW iii FONT-42 Привет','','','','','14','28','36','2','No',choice,'#AA0011'])
        saved=save(str(index));data=saved.read_bytes();assert b'/FontFile2'in data and b'/ToUnicode'in data
        names=re.findall(rb'/BaseFont\s*/([^\s/<>\[\]()]+)',data);assert any(font_name.encode()in n for n in names),(choice,names)
        pages=pdf_text(binary.parent/'pdfium.dll',saved,by_page=True);assert 'WWW iii FONT-42 Привет'in pages[1]and'FONT-42 Привет'not in pages[0]
        pixels=pdf_pixels(binary.parent/'pdfium.dll',saved,1);red=sum(pixels[2][i+2]>pixels[2][i]+40 and pixels[2][i+2]>pixels[2][i+1]+40 for i in range(0,len(pixels[2]),4));assert red>100,(choice,red)
        assert all(pixels!=other for other in rendered),'Different selected font families/styles rendered identically';rendered.append(pixels)
        command(main,4);undone=save('undo-'+str(index));assert 'FONT-42 Привет'not in pdf_text(binary.parent/'pdfium.dll',undone)
        command(main,5);redone=save('redo-'+str(index));assert pdf_pixels(binary.parent/'pdfium.dll',redone,1)==pixels
        open_fixture(process,main,redone);reopened=save('reopened-'+str(index));assert pdf_pixels(binary.parent/'pdfium.dll',reopened,1)==pixels and 'FONT-42 Привет'in pdf_text(binary.parent/'pdfium.dll',reopened)
    if include_print:
        caption=text(main);printed=print_to_virtual_pdf(process,main,lambda:command(main,52),output/'header-font-printed.pdf',output)
        assert len(pdf_text(binary.parent/'pdfium.dll',printed,by_page=True))==1
        _,_,pixels=pdf_pixels(binary.parent/'pdfium.dll',printed);assert sum(pixels[i+2]>pixels[i]+40 and pixels[i+2]>pixels[i+1]+40 for i in range(0,len(pixels),4))>50,'Selected-font colored header was lost by the real PDF printer'
        assert text(main)==caption
    assert source.read_bytes()==original
    results['checks'].append('five actual Windows header fonts and header color chosen in the normal dialog; embedded TrueType/ToUnicode and Cyrillic text, distinct independently rendered glyph pixels, selected page, full Undo/Redo and reopen retain exact appearance; virtual printing retains the colored header when enabled; source unchanged')


def audit_pdf_comparison(process,main,binary,output,results):
    source=output/"comparison-source.pdf";other=output/"comparison-other.pdf"
    minimal_pdf(source,["BOARD uses 10k resistor","REMOVE THIS"])
    minimal_pdf(other,["BOARD uses 20k resistor","","ADDED PAGE"])
    originals=(source.read_bytes(),other.read_bytes())
    def open_file(path):
        command(main,1);choose_file(process,path)
        def state():
            if path.name in text(main)and" * - "not in text(main)and u.IsWindowEnabled(main):return(True,None)
            prompt=next((h for h in windows(process.pid)if classname(h)=="#32770"and u.IsWindowEnabled(h)and all(u.GetDlgItem(h,i)for i in(2,6,7))),None)
            return(False,prompt)if prompt else None
        opened,prompt=wait_for(state,"comparison PDF open/discard",process)
        if not opened:click(prompt,7);wait_for(lambda:path.name in text(main)and" * - "not in text(main)and u.IsWindowEnabled(main),"comparison PDF committed open",process)
    def save(name):
        path=output/("comparison-"+name+".pdf");save_as(main);choose_file(process,path,format_name="PDF document");output_ready(process,main,path,b"%PDF-");return path
    open_file(source);baseline=save("baseline")
    def compare(path=other):
        command(main,97);choose_file(process,path)
        report=modal(process,native_class="Off42ReportWindow")
        assert"Comparison"in text(report)
        u.MoveWindow.argtypes=[w.HWND,c.c_int,c.c_int,c.c_int,c.c_int,w.BOOL]
        assert u.MoveWindow(report,1120,40,760,650,True)
        table=u.GetDlgItem(report,100);assert classname(table)=="SysListView32"
        return report,table
    report,table=compare()
    assert u.SendMessageW(table,0x1004,0,0)==3  # LVM_GETITEMCOUNT
    assert" * - "not in text(main)
    canvas=wait_for(lambda:next((h for h in windows(process.pid,main)if classname(h)=="Suite42Canvas"),None),"comparison canvas",process)
    def colored():
        path=output/"PDF42-comparison-temporary.bmp";capture_window(canvas,path);data=path.read_bytes()[54:]
        return sum(data[i+2]>data[i+1]+20 and data[i+1]>data[i]+20 for i in range(0,len(data),4))
    wait_for(lambda:colored()>20,"temporary orange comparison pixels",process)
    u.SendMessageW(table,0x100,0x24,0);u.SendMessageW(table,0x101,0x24,0)  # Home
    wait_for(lambda:"10k"in text(u.GetDlgItem(report,102))and"20k"in text(u.GetDlgItem(report,102)),"replacement row contents",process)
    u.SendMessageW(table,0x100,0x23,0);u.SendMessageW(table,0x101,0x23,0)  # End
    wait_for(lambda:"ADDED PAGE"in text(u.GetDlgItem(report,102)),"inserted page row",process)
    assert u.MoveWindow(report,20,20,min(760,u.GetSystemMetrics(0)-40),min(650,u.GetSystemMetrics(1)-40),True)
    u.RedrawWindow.argtypes=[w.HWND,c.c_void_p,c.c_void_p,w.UINT]
    assert u.RedrawWindow(report,None,None,0x181)  # invalidate/update all children
    capture_window(report,output/"PDF42-comparison-report.bmp")
    assert len(set((output/"PDF42-comparison-report.bmp").read_bytes()[54:]))>30

    click(report,2);wait_for(lambda:u.IsWindowEnabled(main),"comparison closed without keeping",process)
    unkept=save("unkept")
    assert pdf_annotation_counts(binary.parent/"pdfium.dll",unkept)==[0,0]
    assert pdf_text(binary.parent/"pdfium.dll",unkept,by_page=True)==pdf_text(binary.parent/"pdfium.dll",baseline,by_page=True)
    report,table=compare();click(report,10)
    wait_for(lambda:u.SendMessageW(u.GetDlgItem(report,10),0xF0,0,0)==1 and" * - "in text(main),"explicit keep highlights",process)
    click(report,2);wait_for(lambda:u.IsWindowEnabled(main),"kept comparison closed",process)
    kept=save("kept");assert pdf_annotation_counts(binary.parent/"pdfium.dll",kept)==[1,1]
    for index in [0,1]:
        _,_,pixels=pdf_pixels(binary.parent/"pdfium.dll",kept,index,annotations=True)
        assert sum(pixels[i+2]>pixels[i]+30 and pixels[i+2]>pixels[i+1]+20 for i in range(0,len(pixels),4))>20

    assert pdf_text(binary.parent/"pdfium.dll",kept,by_page=True)==pdf_text(binary.parent/"pdfium.dll",baseline,by_page=True)
    command(main,98);cleared=save("cleared");assert pdf_annotation_counts(binary.parent/"pdfium.dll",cleared)==[0,0]
    command(main,4);undone=save("undo-clear");assert pdf_annotation_counts(binary.parent/"pdfium.dll",undone)==[1,1]
    command(main,5);redone=save("redo-clear");assert pdf_annotation_counts(binary.parent/"pdfium.dll",redone)==[0,0]
    report,table=compare(source);assert u.SendMessageW(table,0x1004,0,0)==0
    assert not u.IsWindowEnabled(u.GetDlgItem(report,10));click(report,2);wait_for(lambda:u.IsWindowEnabled(main),"identical comparison closed",process)
    open_file(kept);reopened=save("reopened");assert pdf_annotation_counts(binary.parent/"pdfium.dll",reopened)==[1,1]
    assert source.read_bytes()==originals[0]and other.read_bytes()==originals[1]
    results["checks"].append("background word comparison, replacement/deletion/added-page report and navigation, temporary orange pixels without document edits, explicit kept highlights, clear/history and PDFium saved/reopened annotations/text, identical input and unchanged source files")


def pdf_bookmarks(dll_path,path):
    pdf=c.CDLL(str(dll_path));handle=c.c_void_p
    pdf.FPDF_LoadDocument.argtypes,pdf.FPDF_LoadDocument.restype=[c.c_char_p,c.c_char_p],handle
    pdf.FPDF_CloseDocument.argtypes=[handle]
    pdf.FPDFBookmark_GetFirstChild.argtypes,pdf.FPDFBookmark_GetFirstChild.restype=[handle,handle],handle
    pdf.FPDFBookmark_GetNextSibling.argtypes,pdf.FPDFBookmark_GetNextSibling.restype=[handle,handle],handle
    pdf.FPDFBookmark_GetTitle.argtypes,pdf.FPDFBookmark_GetTitle.restype=[handle,c.c_void_p,c.c_ulong],c.c_ulong
    pdf.FPDFBookmark_GetDest.argtypes,pdf.FPDFBookmark_GetDest.restype=[handle,handle],handle
    pdf.FPDFDest_GetDestPageIndex.argtypes,pdf.FPDFDest_GetDestPageIndex.restype=[handle,handle],c.c_int
    pdf.FPDF_InitLibrary();doc=pdf.FPDF_LoadDocument(str(path).encode("utf-8"),None);assert doc
    result=[]
    def visit(parent,depth=0):
        assert depth<32
        item=pdf.FPDFBookmark_GetFirstChild(doc,parent)
        while item:
            assert len(result)<10000
            length=pdf.FPDFBookmark_GetTitle(item,None,0);assert 2<=length<=65536
            title=c.create_string_buffer(length);assert pdf.FPDFBookmark_GetTitle(item,title,length)==length
            dest=pdf.FPDFBookmark_GetDest(doc,item);assert dest
            result.append((title.raw.decode("utf-16-le").rstrip("\0"),pdf.FPDFDest_GetDestPageIndex(doc,dest)))
            visit(item,depth+1);item=pdf.FPDFBookmark_GetNextSibling(doc,item)
    try:visit(None);return result
    finally:pdf.FPDF_CloseDocument(doc)


def audit_pdf_merge(process,main,binary,output,results):
    first=output/"merge-01-first.pdf";second=output/"merge-02-second.pdf";parent=output/"merge-parent.pdf"
    minimal_pdf(first,["MERGE A1","MERGE A2"],"A-");minimal_pdf(second,["MERGE B1"],"B-");minimal_pdf(parent,["DIRTY PARENT"])
    originals=[p.read_bytes()for p in(first,second,parent)]
    command(main,1);choose_file(process,parent)
    def opened():
        if parent.name in text(main)and u.IsWindowEnabled(main):return True
        dialog=next((h for h in windows(process.pid)if classname(h)=="#32770"and u.IsWindowEnabled(h)and all(u.GetDlgItem(h,i)for i in(2,6,7))),None)
        if dialog:click(dialog,7)
        return None
    wait_for(opened,"merge parent opened",process);command(main,10)
    wait_for(lambda:" * - "in text(main),"parent dirty before merge",process);caption=text(main)
    command(main,110);picker=modal(process,title="Merge Files…",native_class="#32770");click(picker,2)
    wait_for(lambda:u.IsWindowEnabled(main),"merge picker cancel",process);assert text(main)==caption
    before=set(child_processes(process.pid,binary.name));target=output/("merged-window-"+uuid.uuid4().hex+".pdf")
    command(main,110);picker=modal(process,title="Merge Files…",native_class="#32770")
    choose_file(process,'"'+str(first)+'" "'+str(second)+'"')
    wait_for(lambda:not u.IsWindowVisible(picker)or text(picker)!="Merge Files…","input picker closed before merge destination",process)
    choose_file(process,target)
    output_ready(process,main,target,b"%PDF-")
    pid=wait_for(lambda:next(iter(set(child_processes(process.pid,binary.name))-before),None),"merged PDF separate process",process)
    child=OwnedChildProcess(pid)
    try:
        window=modal(child,native_class="Suite42Native")
        wait_for(lambda:target.name in text(window)and" * - "not in text(window),"merged PDF clean separate window",child)
        assert text(main)==caption and u.IsWindowEnabled(main)
        assert[page.strip()for page in pdf_text(binary.parent/"pdfium.dll",target,by_page=True)]==["MERGE A1","MERGE A2","MERGE B1"]
        assert pdf_page_labels(binary.parent/"pdfium.dll",target)==["A-iv","A-v","B-iv"]
        assert pdf_bookmarks(binary.parent/"pdfium.dll",target)==[("Start A-",0),("Start B-",2)]

        command(main,4);wait_for(lambda:" * - "not in text(main),"parent history unchanged by batch merge",process)
        command(main,5);wait_for(lambda:text(main)==caption,"parent redo unchanged by batch merge",process)
        u.PostMessageW(window,WM_CLOSE,0,0);child.wait(timeout=5)
    finally:
        if child.poll()is None:child.terminate();child.wait(timeout=5)
        child.close_handle()
    assert[p.read_bytes()for p in(first,second,parent)]==originals
    results["checks"].append("native multi-file merge picker and cancellation, complete ordered PDFium text, atomic output and clean separate result process preserve dirty parent source/history")


def audit_pdf_batch_headers(process,main,binary,output,results):
    parent=output/"bates-parent.pdf";a=output/"bates-file2.pdf";b=output/"bates-file10.pdf";bad=output/"bates-file3.pdf"
    minimal_pdf(parent,["CURRENT BODY"]);minimal_pdf(a,["BODY A1","BODY A2"],"A-");minimal_pdf(b,["BODY B1"])
    bad.write_bytes(b"damaged PDF fixture");originals={p:p.read_bytes()for p in(parent,a,b,bad)}
    command(main,1);choose_file(process,parent)
    def opened():
        if parent.name in text(main)and" * - "not in text(main)and u.IsWindowEnabled(main):return True
        prompt=next((h for h in windows(process.pid)if classname(h)=="#32770"and u.IsWindowEnabled(h)and all(u.GetDlgItem(h,i)for i in(2,6,7))),None)
        if prompt:click(prompt,7)
        return None
    wait_for(opened,"Bates parent open/discard",process)
    caption=text(main);command(main,112);picker=modal(process,title="Apply Headers to Other PDFs…",native_class="#32770");click(picker,2)
    wait_for(lambda:u.IsWindowEnabled(main),"Bates picker Cancel",process);assert text(main)==caption
    fill_prompt(process,main,93,"Bates Numbering Settings",["CASE-","-X","6","41","2026-11-08"])
    command(main,92);dialog=modal(process,title="Header & Footer",native_class="Suite42InputDialog")
    rect=w.RECT();assert u.GetClientRect(dialog,c.byref(rect))
    # Every field and both buttons must be inside the usable dialog, including the long token label.
    for identifier in list(range(100,111))+[1,2]:
        control=u.GetDlgItem(dialog,identifier);assert control
        bounds=w.RECT();assert u.GetWindowRect(control,c.byref(bounds));point=w.POINT(bounds.left,bounds.top);assert u.ScreenToClient(dialog,c.byref(point))
        assert 0<=point.x<rect.right and 0<=point.y<rect.bottom,(identifier,point.x,point.y,rect.right,rect.bottom)
        # A combo's dropped-list rectangle may extend below the field while its visible closed field stays inside.
        if classname(control).lower()!="combobox":assert point.y+bounds.bottom-bounds.top<=rect.bottom
    values=["<<file>>","<<page>>/<<pages>>","<<date>>","CASE <<bates>>","","","10","28","36","1","Yes"]
    for index,value in enumerate(values):set_text(u.GetDlgItem(dialog,100+index),value)
    capture_window(dialog,output/"PDF42-bates-settings.bmp");click(dialog,1)
    picker=modal(process,title="Apply Headers to Other PDFs…",native_class="#32770")
    folder=output/("bates-copies-"+uuid.uuid4().hex);folder.mkdir()
    choose_file(process,'"'+str(b)+'" "'+str(bad)+'" "'+str(a)+'"')
    wait_for(lambda:not u.IsWindowVisible(picker)or text(picker)!="Apply Headers to Other PDFs…","Bates inputs selected",process)
    modal(process,title="Choose a Folder for PDF Copies",native_class="#32770");choose_file(process,folder)
    report=modal(process,title="Batch Headers & Bates Numbers",native_class="Off42ReportWindow")
    table=u.GetDlgItem(report,100);assert u.SendMessageW(table,0x1004,0,0)==3
    assert"2 / 3"in text(u.GetDlgItem(report,101))and"45"in text(u.GetDlgItem(report,101)),text(u.GetDlgItem(report,101))
    assert not u.IsWindowVisible(u.GetDlgItem(report,10))and not u.IsWindowVisible(u.GetDlgItem(report,11))
    u.SendMessageW(table,0x100,0x23,0);u.SendMessageW(table,0x101,0x23,0)
    wait_for(lambda:b.name in text(u.GetDlgItem(report,102))and"44–44"in text(u.GetDlgItem(report,102)),"natural order and copied Bates range",process)
    capture_window(report,output/"PDF42-bates-results.bmp");click(report,2);wait_for(lambda:u.IsWindowEnabled(main),"Bates report closed",process)
    assert" * - "in text(main)
    assert sorted(p.name for p in folder.iterdir())==sorted([a.name,b.name])
    atext=pdf_text(binary.parent/"pdfium.dll",folder/a.name,by_page=True);btext=pdf_text(binary.parent/"pdfium.dll",folder/b.name,by_page=True)
    for text_page,body,number in [(atext[0],"BODY A1",42),(atext[1],"BODY A2",43),(btext[0],"BODY B1",44)]:
        assert body in text_page and f"CASE-{number:06}-X"in text_page and"2026-11-08"in text_page,text_page
    assert all(a.name in page for page in atext)and b.name in btext[0]
    assert pdf_page_labels(binary.parent/"pdfium.dll",folder/a.name)==["A-iv","A-v"]
    assert pdf_bookmarks(binary.parent/"pdfium.dll",folder/a.name)==[("Start A-",0)]
    command(main,93);settings=modal(process,title="Bates Numbering Settings",native_class="Suite42InputDialog")
    assert text(u.GetDlgItem(settings,103))=="45";click(settings,2);wait_for(lambda:u.IsWindowEnabled(main),"continued Bates settings Cancel",process)
    def save(name):
        path=output/("bates-current-"+name+".pdf");save_as(main);choose_file(process,path,format_name="PDF document");output_ready(process,main,path,b"%PDF-");return path
    saved=save("saved");assert"CASE-000041-X"in pdf_text(binary.parent/"pdfium.dll",saved)
    command(main,4);undone=save("undo");assert"CASE-"not in pdf_text(binary.parent/"pdfium.dll",undone)
    command(main,5);redone=save("redo");assert pdf_text(binary.parent/"pdfium.dll",redone)==pdf_text(binary.parent/"pdfium.dll",saved)
    assert all(path.read_bytes()==data for path,data in originals.items())
    results["checks"].append("native batch-header continuation, all long-form controls onscreen, natural filename order, independent PDFium body/Bates/file/date/labels/bookmarks, damaged-file report without consuming a number, unchanged sources and current-document one-step Undo/Redo")


def audit_statistical_formulas(process,main,output,binary,results):
    import math
    import xml.etree.ElementTree as ET
    source=output/"statistical-source.csv"
    formulas=["=STDEV.S(A1:A5)","=AVERAGEA(A1:A5)","=COUNTBLANK(A1:A5)","=CORREL(A1:A5,B1:B5)","=NORM.DIST(A1,0,1,TRUE)","=BETA.INV(0.5,2,2)","=T.DIST.2T(A1,10)","=VAR.S(A1:A5)","=NORM.S.INV(1e-100)","=T.DIST.RT(20,100)","=CHISQ.INV.RT(1e-20,2)","=ERF(8,9)","=F.INV.RT(1e-20,5,8)","=PERCENTILE.EXC(A1:A2,0.5)","=PERCENTILE.EXC(A1:A2,0)"]
    formulas += ["=PV(.1/12,12,-100*A1)","=RATE(12,-100*A1,1000)","=CUMIPMT(.1/12,36,1000*A1,1,12,0)","=IRR(D3:D5)","=XIRR(D3:D5,D6:D8)","=XNPV(0,D3:D5,D6:D8)","=WORKDAY(DATE(2024,12,20),A1,D1:D2)","=NETWORKDAYS(DATE(2024,12,20),DATE(2024,12,31),D1:D2)","=EDATE(DATE(2024,1,31),A1)"]
    formulas += ['=IFNA(NA(),"fallback")','=IFS(A1>2,"large",TRUE,"small")','=SWITCH(A1,1,"one",4,"four","other")','=CHOOSE(2,"first","second")','=MAXIFS(B1:B5,A1:A5,">0")','=MINIFS(B1:B5,A1:A5,">0")','=ERROR.TYPE(1/0)','=TYPE(A4)','=ISFORMULA(C1)','=CELL("address",B3)','=CELL("contents",A1)','=ADDRESS(2,3)','=AND(RAND()>=0,RAND()<1)','=AND(RANDBETWEEN(1,6)>=1,RANDBETWEEN(1,6)<=6)','=OR(TRUE,1/0)','=TYPE(NA())']
    formulas += ['=SUM(OFFSET(A1,0,0,2,1))','=SUM(INDEX(A1:B5,0,2))','=INDIRECT("A"&1)','=SUM(INDIRECT("A1:A2"))','=INDIRECT("R2C1",FALSE)','=XMATCH(6,B1:B5)','=XMATCH(5,B1:B5,1)','=XMATCH(5,B1:B5,-1)','=HLOOKUP(2,A1:B2,2,FALSE)','=SUBTOTAL(9,A1:A5)','=AGGREGATE(9,6,A1:A5)','=ISREF(OFFSET(A1,1,1))','=CELL("address",OFFSET(A1,1,1))','=FORMULATEXT(INDEX(C1:C2,1))','=SUM(INDEX(A1:B5,2,0))','=OFFSET(A1,1,1)']
    dates=["2024-12-25","2025-01-01","-100","0","121","2021-01-01","2022-01-01","2023-01-01"]
    a=["1","2","text","TRUE",""];b=["2","4","6","8","10"]
    with source.open("w",encoding="utf-8",newline="")as file:
        csv.writer(file).writerows([[a[i]if i<len(a)else"",b[i]if i<len(b)else"",formula,dates[i]if i<len(dates)else"","","","",""]for i,formula in enumerate(formulas)])
    original=source.read_bytes()
    def open_file(path):
        command(main,1);choose_file(process,path)
        def state():
            if path.name in text(main)and" * - "not in text(main)and u.IsWindowEnabled(main):return(True,None)
            prompt=next((h for h in windows(process.pid)if classname(h)=="#32770"and u.IsWindowEnabled(h)and all(u.GetDlgItem(h,i)for i in(2,6,7))),None)
            return(False,prompt)if prompt else None
        opened,prompt=wait_for(state,"statistics file open/discard",process)
        if not opened:
            click(prompt,7);wait_for(lambda:path.name in text(main)and" * - "not in text(main)and u.IsWindowEnabled(main),"statistics file committed open",process)
    open_file(source)
    def save(name):
        path=output/("statistics-"+name+".xlsx")
        save_as(main);choose_file(process,path,format_name="Excel workbook");output_ready(process,main,path,b"PK")
        with zipfile.ZipFile(path)as archive:root=ET.fromstring(archive.read("xl/worksheets/sheet1.xml"))
        ns={"s":"http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
        cells={cell.attrib["r"]:(cell.findtext("s:f",namespaces=ns),cell.findtext("s:v",namespaces=ns),cell.attrib.get("t"))for cell in root.findall(".//s:c",ns)}
        return path,cells
    baseline,cells=save("baseline")
    def value(cells,address,expected):
        assert cells[address][2]in(None,"n"),cells[address]
        assert math.isclose(float(cells[address][1]),expected,rel_tol=2e-9,abs_tol=0.0),(address,cells[address],expected)
    value(cells,"C1",math.sqrt(.5));value(cells,"C2",1.0);value(cells,"C3",1.0);value(cells,"C4",1.0);value(cells,"C5",.8413447460685429);value(cells,"C6",.5);value(cells,"C8",.5);value(cells,"C14",1.5)
    assert cells["C15"][1:]==("#NUM!","e")
    oracle=json.loads((Path(__file__).resolve().parents[1]/"crates/xls42-core/fixtures/independent-distribution-cases.json").read_text())
    expected={case["formula"]:case["expected"]for case in oracle["cases"]}
    for address,formula in [("C9","=NORM.S.INV(1e-100)"),("C10","=T.DIST.RT(20.0,100.0)"),("C11","=CHISQ.INV.RT(1e-20,2.0)"),("C12","=ERF(8.0,9.0)"),("C13","=F.INV.RT(1e-20,5.0,8.0)")]:value(cells,address,expected[formula])
    finance_oracle=json.loads((Path(__file__).resolve().parents[1]/"crates/xls42-core/fixtures/independent-finance-cases.json").read_text())
    finance_expected={c["formula"]:c["expected"]for c in finance_oracle["cases"]}
    def finance(cells,payment,present):
        # PV annuity identity is independent of the application evaluator.
        rate=.1/12;pv=100*payment*(1-(1+rate)**-12)/rate
        value(cells,"C16",pv)
        value(cells,"C17",finance_expected[f"=RATE(12.0,{-100.0*payment},1000.0)"])
        value(cells,"C18",finance_expected[f"=CUMIPMT({rate!r},36.0,{present:.1f},1.0,12.0,0.0)"])
        value(cells,"C19",.1);value(cells,"C20",.1);value(cells,"C21",21.0);value(cells,"C23",7.0)
    finance(cells,1,1000)
    value(cells,"C22",45649.0);value(cells,"C24",45351.0)
    def information(cells,changed=False):
        for address,expected in [("C25","fallback"),("C26","large"if changed else"small"),("C27","four"if changed else"one"),("C28","second"),("C34","$B$3"),("C36","$C$2")]:assert cells[address][1:]==(expected,"str"),(address,cells[address])
        for address,expected in [("C29",4.0),("C30",2.0),("C31",2.0),("C32",4.0),("C35",4.0 if changed else 1.0),("C40",16.0)]:value(cells,address,expected)
        for address in ["C33","C37","C38"]:assert cells[address][1:]==("1","b"),(address,cells[address])
        assert cells["C39"][1:]==("#DIV/0!","e")
    information(cells)
    def references(cells,changed=False,filtered=False):
        for address,expected in [("C41",6 if changed else 3),("C42",30),("C43",4 if changed else 1),("C44",6 if changed else 3),("C45",2),("C46",3),("C47",3),("C48",2),("C49",4),("C50",1 if filtered else 6 if changed else 3),("C51",1 if filtered else 6 if changed else 3),("C55",6),("C56",4)]:value(cells,address,expected)
        assert cells["C52"][1:]==("1","b"),cells["C52"]
        assert cells["C53"][1:]==("$B$2","str"),cells["C53"]
        assert cells["C54"][1:]==("=STDEV.S(A1:A5)","str"),cells["C54"]
    references(cells)
    fill_prompt(process,main,25,"Range (1-based)",["1","1","1","1"])
    edit=u.GetDlgItem(main,101);replace_editor_text(edit,"4")
    _,changed=save("changed")
    value(changed,"C1",math.sqrt(2));value(changed,"C2",1.75);value(changed,"C4",-1.0);value(changed,"C5",.9999683287581669);value(changed,"C8",2.0);value(changed,"C14",3.0)
    finance(changed,4,4000)
    information(changed,True)
    references(changed,True)
    value(changed,"C22",45653.0);value(changed,"C24",45443.0)
    command(main,4);_,undone=save("undo")
    assert all(undone["C"+str(i+1)]==cells["C"+str(i+1)]for i in range(len(formulas)))
    command(main,5);_,redone=save("redo")
    assert all(redone["C"+str(i+1)]==changed["C"+str(i+1)]for i in range(len(formulas)))
    replace_editor_text(edit,"9");open_file(baseline);_,reopened=save("reopened")
    assert all(reopened["C"+str(i+1)]==cells["C"+str(i+1)]for i in range(len(formulas)))
    fill_prompt(process,main,25,"Range (1-based)",["1","1","5","2"])
    fill_prompt(process,main,63,"Filter selection (first row is header)",["1","1"])
    _,filtered=save("filtered");references(filtered,filtered=True)
    command(main,4);_,filter_undo=save("filter-undo");references(filter_undo)
    command(main,5);_,filter_redo=save("filter-redo");references(filter_redo,filtered=True)
    ods=output/"statistics-filtered.ods";save_as(main);choose_file(process,ods,format_name="OpenDocument spreadsheet");output_ready(process,main,ods,b"PK")
    with zipfile.ZipFile(ods)as archive:root=ET.fromstring(archive.read("content.xml"))
    ods_ns={"t":"urn:oasis:names:tc:opendocument:xmlns:table:1.0"}
    condition=root.find(".//t:filter-condition",ods_ns);assert condition is not None
    assert condition.attrib["{"+ods_ns["t"]+"}operator"]=="contains"and condition.attrib["{"+ods_ns["t"]+"}value"]=="1"
    open_file(baseline);open_file(ods);_,ods_reopened=save("ods-reopened");references(ods_reopened,filtered=True)
    def without_project(path):
        target=output/("external-"+path.name)
        with zipfile.ZipFile(path)as source_zip,zipfile.ZipFile(target,"w")as target_zip:
            for item in source_zip.infolist():
                if item.filename!="xls42/project.json":target_zip.writestr(item,source_zip.read(item.filename))
        return target
    external=without_project(baseline);external_original=external.read_bytes();open_file(external)
    fill_prompt(process,main,25,"Range (1-based)",["1","1","1","1"]);replace_editor_text(u.GetDlgItem(main,101),"4")
    external_changed,ext_cells=save("external-changed");references(ext_cells,True);information(ext_cells,True);finance(ext_cells,4,4000)
    with zipfile.ZipFile(external_changed)as archive:assert"xls42/project.json"not in archive.namelist()
    command(main,4);_,ext_undo=save("external-undo");assert all(ext_undo["C"+str(i+1)]==cells["C"+str(i+1)]for i in range(len(formulas)))
    command(main,5);_,ext_redo=save("external-redo");assert all(ext_redo["C"+str(i+1)]==ext_cells["C"+str(i+1)]for i in range(len(formulas)))
    assert external.read_bytes()==external_original
    external_ods=without_project(ods);external_ods_original=external_ods.read_bytes();open_file(external_ods)
    fill_prompt(process,main,25,"Range (1-based)",["1","1","1","1"]);replace_editor_text(u.GetDlgItem(main,101),"4")
    ods_saved=output/"statistics-external-edited.ods";save_as(main);choose_file(process,ods_saved,format_name="OpenDocument spreadsheet");output_ready(process,main,ods_saved,b"PK")
    with zipfile.ZipFile(ods_saved)as archive:
        assert"xls42/project.json"not in archive.namelist();root=ET.fromstring(archive.read("content.xml"))
    office="urn:oasis:names:tc:opendocument:xmlns:office:1.0";table="urn:oasis:names:tc:opendocument:xmlns:table:1.0";cache={}
    for row_index,row in enumerate(root.findall(".//{"+table+"}table-row")):
        column=0
        for cell in row:
            repeat=int(cell.attrib.get("{"+table+"}number-columns-repeated",1))
            if column<=2<column+repeat:cache[row_index+1]=cell
            column+=repeat
    for row,expected in [(41,"6"),(43,"4"),(50,"4"),(51,"4")]:assert cache[row].attrib.get("{"+office+"}value")==expected,(row,cache[row].attrib)
    assert cache[52].attrib.get("{"+office+"}boolean-value")=="true"
    assert cache[53].attrib.get("{"+office+"}string-value")=="$B$2"
    assert external_ods.read_bytes()==external_ods_original
    assert source.read_bytes()==original
    results["checks"].append("date entry and calendar/annuity/cash-flow calculations, statistical range/reference typing, distributions and 1e-100 literals, small-tail quantiles against an independent oracle, typed XLSX caches, computed references/lookup and filter-aware summaries, native filter/history and standard ODS output/reopen, external XLSX/ODS typed cache refresh without private snapshots, input recalculation, complete Undo/Redo and dirty/discard/reopen")


def audit_array_formulas(process,main,binary,output,results):
    import math
    import xml.etree.ElementTree as ET
    source=output/"array-source.csv";data=[[""]*16 for _ in range(20)]
    for r,n in enumerate(["3","1","2"]):data[r][0]=n
    for r,c,value in [(0,2,'=SORT(A1:A3)'),(0,3,'=SEQUENCE(2,3)'),(4,3,'=C2*10'),(0,7,'=TRANSPOSE(A1:A3)'),(2,7,'x'),(1,7,'=SEQUENCE(2)'),(1,9,'=IF(A1:A3>1,"y","n")'),(1,10,'=IFERROR(1/(A1:A3-1),"e")'),(1,11,'=LEN(A1:A3)&"!"'),(8,0,'=LINEST({3;5;8;10;13},{1;2;3;4;5},TRUE,TRUE)'),(8,3,'=TEXTSPLIT("αXβxγ;δ",{"x","XX"},";",FALSE,1,99)'),(8,7,'=REGEXEXTRACT("a12b34","[0-9]+",1)'),(8,9,'=REGEXEXTRACT("Ж12","(Ж)([0-9]+)",2)'),(8,13,'=REGEXREPLACE("a1b2","([a-z])([0-9])","$2$1")'),(9,13,'=FIXED(1234.567,-2,TRUE)'),(10,13,'=ROMAN(1999)'),(11,13,'=ARABIC("-iv")'),(12,13,'=TREND({3;5;8;10;13},{1;2;3;4;5},6)'),(13,13,'=TEXT(7,"0000")')]:data[r][c]=value
    with source.open("w",encoding="utf-8",newline="")as file:csv.writer(file).writerows(data)
    original=source.read_bytes()
    def open_file(path):
        command(main,1);choose_file(process,path)
        def opened():
            if path.name in text(main)and" * - "not in text(main)and u.IsWindowEnabled(main):return True
            prompt=next((h for h in windows(process.pid)if classname(h)=="#32770"and u.IsWindowEnabled(h)and all(u.GetDlgItem(h,i)for i in(2,6,7))),None)
            if prompt:click(prompt,7)
            return None
        wait_for(opened,"array source open/discard",process)
    shared_strings=[]
    def save(name):
        nonlocal shared_strings
        path=output/("arrays-"+name+".xlsx");save_as(main);choose_file(process,path,format_name="Excel workbook");output_ready(process,main,path,b"PK")
        with zipfile.ZipFile(path)as archive:
            root=ET.fromstring(archive.read("xl/worksheets/sheet1.xml"));assert"xl/metadata.xml"in archive.namelist()
            shared_strings=["".join(n.itertext())for n in ET.fromstring(archive.read("xl/sharedStrings.xml"))]if"xl/sharedStrings.xml"in archive.namelist()else[]
        ns={"s":"http://schemas.openxmlformats.org/spreadsheetml/2006/main"};cells={cell.attrib["r"]:cell for cell in root.findall(".//s:c",ns)}
        return path,cells,ns
    def cell_value(cells,ns,address):
        cell=cells[address];value=cell.findtext("s:v",namespaces=ns)
        if cell.attrib.get("t")=="inlineStr":
            inline=cell.find("s:is",ns);assert inline is not None
            return"".join(node.text or""for node in inline.findall("s:t",ns)+inline.findall("s:r/s:t",ns))
        return shared_strings[int(value)]if cell.attrib.get("t")=="s"else value
    def value(cells,ns,address,expected):
        cell=cells[address];assert cell_value(cells,ns,address)==expected,(address,ET.tostring(cell,encoding="unicode"),expected)
    def baseline_values(cells,ns):
        for address,wanted in [("C1","1"),("C2","2"),("C3","3"),("D1","1"),("E1","2"),("F1","3"),("D2","4"),("E2","5"),("F2","6"),("D5","20"),("H1","3"),("I1","1"),("J1","2"),("H2","#SPILL!"),("J2","n"),("K2","e"),("L2","1!")]:value(cells,ns,address,wanted)
        assert cells["H2"].attrib.get("t")=="e"
        for address,wanted in [("D9","α"),("E9","β"),("F9","γ"),("D10","δ"),("E10","99"),("F10","99"),("H9","12"),("H10","34"),("J9","Ж"),("K9","12"),("N9","1a2b"),("N10","1200"),("N11","MCMXCIX"),("N12","-4"),("N14","0007")]:value(cells,ns,address,wanted)
        for address,wanted in [("A9",2.5),("B9",0.3),("N13",15.3)]:
            actual=float(cells[address].findtext("s:v",namespaces=ns));assert math.isclose(actual,wanted,rel_tol=1e-12,abs_tol=1e-12),(address,actual,wanted)
        for address,span in [("C1","C1:C3"),("D1","D1:F2"),("H1","H1:J1"),("A9","A9:B13"),("D9","D9:F10"),("H9","H9:H10"),("J9","J9:K9")]:
            cell=cells[address];formula=cell.find("s:f",ns);assert cell.attrib.get("cm")=="1"and formula.attrib.get("t")=="array"and formula.attrib.get("ref")==span,(address,cell.attrib,formula.attrib)
        assert cells["C2"].find("s:f",ns)is None
    open_file(source);baseline,cells,ns=save("baseline");baseline_values(cells,ns)
    grid=u.GetDlgItem(main,102);assert classname(grid)=="Off42VariableGrid";capture_window(grid,output/"XLS42-arrays.bmp")
    fill_prompt(process,main,25,"Range (1-based)",["9","1","9","2"]);command(main,39)
    wait_for(lambda:clipboard_text()=="2.5\t0.3","formatted general clipboard values",process)
    fill_prompt(process,main,25,"Range (1-based)",["2","3","2","3"])
    editor=u.GetDlgItem(main,101);wait_for(lambda:text(editor)=="","spill child retains an empty input",process)
    replace_editor_text(editor,"blocked");_,blocked,ns=save("blocked");value(blocked,ns,"C1","#SPILL!");assert blocked["C1"].attrib.get("t")=="e"
    value(blocked,ns,"C2","blocked")
    command(main,4);_,undone,ns=save("undo");baseline_values(undone,ns);assert text(editor)==""
    command(main,5);_,redone,ns=save("redo");value(redone,ns,"C1","#SPILL!");assert text(editor)=="blocked"
    command(main,4)
    ods=output/"arrays-native.ods";save_as(main);choose_file(process,ods,format_name="OpenDocument spreadsheet");output_ready(process,main,ods,b"PK")
    with zipfile.ZipFile(ods)as archive:root=ET.fromstring(archive.read("content.xml"))
    table="urn:oasis:names:tc:opendocument:xmlns:table:1.0";office="urn:oasis:names:tc:opendocument:xmlns:office:1.0";row=root.find(".//{"+table+"}table-row");column=0;anchors={}
    for cell in row:
        repeat=int(cell.attrib.get("{"+table+"}number-columns-repeated",1));anchors[column]=cell;column+=repeat
    assert anchors[2].attrib["{"+table+"}number-matrix-rows-spanned"]=="3"
    assert anchors[3].attrib["{"+table+"}number-matrix-columns-spanned"]=="3"
    open_file(baseline);_,reopened,ns=save("reopened");baseline_values(reopened,ns)
    open_file(ods);_,ods_reopened,ns=save("ods-reopened");baseline_values(ods_reopened,ns)
    def strip_private(path,name):
        destination=output/name
        with zipfile.ZipFile(path)as archive,zipfile.ZipFile(destination,"w",compression=zipfile.ZIP_DEFLATED)as target:
            for item in archive.infolist():
                if item.filename=="xls42/project.json":continue
                data=archive.read(item.filename)
                if item.filename in("[Content_Types].xml","META-INF/manifest.xml"):
                    root=ET.fromstring(data)
                    for child in list(root):
                        if any(key.split("}")[-1]in("PartName","full-path")and value.lstrip("/")=="xls42/project.json"for key,value in child.attrib.items()):root.remove(child)
                    data=ET.tostring(root,encoding="utf-8",xml_declaration=True)
                target.writestr(item,data)
        return destination
    external_xlsx=strip_private(baseline,"arrays-external.xlsx");external_ods=strip_private(ods,"arrays-external.ods")
    external_sources={path:path.read_bytes()for path in(external_xlsx,external_ods)}
    open_file(external_xlsx);_,external_base,ns=save("external-baseline");baseline_values(external_base,ns)
    fill_prompt(process,main,25,"Range (1-based)",["1","4","1","4"]);editor=u.GetDlgItem(main,101)
    wait_for(lambda:text(editor)=="=SEQUENCE(2,3)","standard external array anchor formula",process)
    replace_editor_text(editor,"=SEQUENCE(3,2)");resized_path,resized,ns=save("external-resized")
    def resized_values(cells,ns):
        for address,wanted in[("D1","1"),("E1","2"),("D2","3"),("E2","4"),("D3","5"),("E3","6")]:value(cells,ns,address,wanted)
        assert all(address not in cells or cell_value(cells,ns,address)in(None,"")for address in("F1","F2"))
        assert cells["D1"].find("s:f",ns).attrib["ref"]=="D1:E3"
    resized_values(resized,ns)
    command(main,4);_,external_undo,ns=save("external-undo");baseline_values(external_undo,ns)
    command(main,5);_,external_redo,ns=save("external-redo");resized_values(external_redo,ns)
    fill_prompt(process,main,25,"Range (1-based)",["2","4","2","4"]);wait_for(lambda:text(editor)=="","external spill child empty source",process)
    replace_editor_text(editor,"stop");_,external_blocked,ns=save("external-blocked");value(external_blocked,ns,"D1","#SPILL!");value(external_blocked,ns,"D2","stop")
    assert all(address not in external_blocked or cell_value(external_blocked,ns,address)in(None,"")for address in("E1","E2","D3","E3"))
    command(main,4);_,external_unblock,ns=save("external-unblock");resized_values(external_unblock,ns)
    open_file(resized_path);_,external_reopened,ns=save("external-reopened");resized_values(external_reopened,ns)
    with zipfile.ZipFile(resized_path)as archive:assert"xls42/project.json"not in archive.namelist()
    def save_external_ods(name):
        path=output/("arrays-"+name+".ods");save_as(main);choose_file(process,path,format_name="OpenDocument spreadsheet");output_ready(process,main,path,b"PK")
        with zipfile.ZipFile(path)as archive:
            assert"xls42/project.json"not in archive.namelist();root=ET.fromstring(archive.read("content.xml"))
        values={};ranges={};row=0
        for node in root.findall(".//{"+table+"}table-row"):
            count=int(node.attrib.get("{"+table+"}number-rows-repeated",1));col=0
            for cell in node:
                repeat=int(cell.attrib.get("{"+table+"}number-columns-repeated",1))
                cached=next((cell.attrib["{"+office+"}"+key]for key in("value","string-value","boolean-value")if"{"+office+"}"+key in cell.attrib),"\n".join("".join(paragraph.itertext())for paragraph in cell.findall("{urn:oasis:names:tc:opendocument:xmlns:text:1.0}p")))
                for r in range(row,row+count):
                    for c in range(col,col+repeat):values[chr(65+c)+str(r+1)]=cached
                if"{"+table+"}formula"in cell.attrib:ranges[chr(65+col)+str(row+1)]=(int(cell.attrib.get("{"+table+"}number-matrix-rows-spanned",1)),int(cell.attrib.get("{"+table+"}number-matrix-columns-spanned",1)))
                col+=repeat
            row+=count
        return path,values,ranges
    open_file(external_ods);_,od_base,od_ranges=save_external_ods("external-ods-baseline");assert od_base["C2"]=="2"and od_base["F2"]=="6"and od_ranges["D1"]==(2,3)
    fill_prompt(process,main,25,"Range (1-based)",["1","4","1","4"]);wait_for(lambda:text(editor)=="=SEQUENCE(2,3)","external ODS array anchor",process);replace_editor_text(editor,"=SEQUENCE(3,2)")
    ods_resized,od_resized,od_ranges=save_external_ods("external-ods-resized");assert od_ranges["D1"]==(3,2)
    for address,wanted in[("D1","1"),("E1","2"),("D2","3"),("E2","4"),("D3","5"),("E3","6"),("F1",""),("F2","")]:assert od_resized.get(address,"")==wanted,(address,od_resized.get(address),wanted)
    command(main,4);_,od_undo,od_ranges=save_external_ods("external-ods-undo");assert od_ranges["D1"]==(2,3)and od_undo["F2"]=="6"
    command(main,5);_,od_redo,od_ranges=save_external_ods("external-ods-redo");assert od_ranges["D1"]==(3,2)and od_redo["E3"]=="6"
    fill_prompt(process,main,25,"Range (1-based)",["2","4","2","4"]);wait_for(lambda:text(editor)=="","external ODS spill child empty source",process);replace_editor_text(editor,"stop")
    _,od_blocked,_=save_external_ods("external-ods-blocked");assert od_blocked["D1"]=="#SPILL!"and od_blocked["D2"]=="stop"
    assert all(not od_blocked.get(address)for address in("E1","E2","D3","E3"))
    command(main,4);_,od_unblock,_=save_external_ods("external-ods-unblock");assert od_unblock["E3"]=="6"and text(editor)==""
    open_file(ods_resized);_,od_reopened,od_ranges=save_external_ods("external-ods-reopened");assert od_reopened["E3"]=="6"and od_ranges["D1"]==(3,2)
    assert all(path.read_bytes()==before for path,before in external_sources.items())
    assert source.read_bytes()==original
    results["checks"].append("dynamic arrays render/spill with empty child inputs, rounded General/clipboard text with full-precision numeric file caches, independent XLSX typed values/array ranges/metadata and ODS matrix spans, conflict errors, complete one-step Undo/Redo, saved/reopened results, standard external XLSX/ODS without private snapshots, resize/cache cleanup/blocking with complete Undo/Redo/reopening and unchanged sources")


def audit_bundled_ocr(process,main,binary,output,results):
    fixture=Path(__file__).resolve().parent.parent/'crates/win42/fixtures/ocr-english.png'
    original=fixture.read_bytes()
    dll=binary.parent/'pdfium.dll'
    def status():return ' '.join(text(h)for h in windows(process.pid,main))
    def open_pdf(path):
        command(main,1);choose_file(process,path)
        def state():
            if path.name in text(main) and ' * - 'not in text(main) and u.IsWindowEnabled(main):return (True,None)
            prompt=next((h for h in windows(process.pid)if classname(h)=='#32770'and u.IsWindowEnabled(h)and all(u.GetDlgItem(h,i)for i in(2,6,7))),None)
            return (False,prompt)if prompt else None
        opened,prompt=wait_for(state,'OCR source open/discard',process)
        if not opened:
            click(prompt,7);wait_for(lambda:path.name in text(main)and' * - 'not in text(main)and u.IsWindowEnabled(main),'OCR PDF opened',process)
    is_pdf=binary.stem.lower()=='pdf42'
    if is_pdf:
        width,height,rgba=png_pixels(fixture)
        rgb=b''.join(rgba[i:i+3]for i in range(0,len(rgba),4));image=zlib.compress(rgb)
        body=b'q 600 0 0 160 10 20 cm /Image Do Q\n'
        objects=[b'<< /Type /Catalog /Pages 2 0 R >>',b'<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] >>']
        for rotation in(0,90):
            objects.append(f'<< /Type /Page /Parent 2 0 R /MediaBox [10 20 610 180] /CropBox [20 30 600 170] /Rotate {rotation} /Resources << /XObject << /Image 5 0 R >> >> /Contents 6 0 R >>'.encode())
        objects.extend([f'<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {len(image)} >>\nstream\n'.encode()+image+b'\nendstream',f'<< /Length {len(body)} >>\nstream\n'.encode()+body+b'endstream'])
        data=bytearray(b'%PDF-1.4\n');offsets=[0]
        for number,obj in enumerate(objects,1):offsets.append(len(data));data.extend(f'{number} 0 obj\n'.encode()+obj+b'\nendobj\n')
        xref=len(data);data.extend(f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode())
        for offset in offsets[1:]:data.extend(f'{offset:010d} 00000 n \n'.encode())
        data.extend(f'trailer\n<< /Root 1 0 R /Size {len(offsets)} >>\nstartxref\n{xref}\n%%EOF\n'.encode())
        source=output/'ocr-image-source.pdf';source.write_bytes(data);before_source=source.read_bytes();open_pdf(source)
    else:
        command(main,20);choose_file(process,fixture)
        wait_for(lambda:status().startswith('2 pages |')or'2 pages |'in status(),'OCR image imported as second page',process)
    def save(name):
        path=output/f'{binary.stem}-ocr-{name}.pdf'
        if is_pdf:save_as(main)
        else:command(main,21)
        choose_file(process,path,format_name='PDF document'if is_pdf else None);output_ready(process,main,path,b'%PDF-');return path
    baseline=save('baseline');assert '1234567890'not in pdf_text(dll,baseline)
    if is_pdf:
        caption=text(main);command(main,114);dialog=modal(process,title='Recognize PDF Text',native_class='Suite42InputDialog');click(dialog,2)
        wait_for(lambda:u.IsWindowEnabled(main),'OCR settings cancelled',process);assert text(main)==caption
        fill_prompt(process,main,114,'Recognize PDF Text',['2','eng','180'])
        wait_for(lambda:'Text recognized on 1 of 1 pages'in status(),'PDF real OCR completed',process,timeout=90)
    else:
        fill_prompt(process,main,38,'Built-in OCR (offline; up to 3 languages)',['en-US','2','2'])
        wait_for(lambda:'Recognition complete'in status(),'scan real OCR completed',process,timeout=90)
    recognized=save('recognized');pages=pdf_text(dll,recognized,by_page=True)
    assert len(pages)==2 and '1234567890'not in pages[0],pages
    assert 'offline text recognition'in pages[1]and'1234567890'in pages[1],pages
    for i in range(2):assert pdf_pixels(dll,baseline,i)==pdf_pixels(dll,recognized,i),(binary.stem,i)
    command(main,4);undone=save('undo');assert '1234567890'not in pdf_text(dll,undone)
    command(main,5);redone=save('redo');assert pdf_text(dll,redone,by_page=True)==pages
    if is_pdf:
        open_pdf(redone)
        fill_prompt(process,main,114,'Recognize PDF Text',['2','eng','180'])
        wait_for(lambda:'Text recognized on 1 of 1 pages'in status(),'reopened PDF re-recognition',process,timeout=90)
        replaced=save('replaced');assert pdf_text(dll,replaced,by_page=True)==pages
        assert pdf_pixels(dll,replaced,1)==pdf_pixels(dll,baseline,1)
        open_pdf(source)
        fill_prompt(process,main,114,'Recognize PDF Text',['1-2','eng','300'])
        command(main,115)
        wait_for(lambda:'recognition cancelled'in status().lower(),'PDF OCR cancellation',process)
        cancelled=save('cancelled');assert '1234567890'not in pdf_text(dll,cancelled)
        assert source.read_bytes()==before_source
    else:
        project=output/'ocr-scan.scn42';save_as(main);choose_file(process,project);output_ready(process,main,project,b'SCN42')
        command(main,1);choose_file(process,project);wait_for(lambda:project.name in text(main)and u.IsWindowEnabled(main),'OCR scan reopened',process)
        reopened=save('reopened');assert pdf_text(dll,reopened,by_page=True)==pages
    assert fixture.read_bytes()==original
    results['checks'].append('bundled static OCR recognizes real image pixels offline through native settings; selected page only, independent PDFium text/exact pixels, one-step Undo/Redo and saved reopen'+('; PDF crop/rotation retained, own layer replaced without duplicate text, settings and worker Cancel leave no OCR'if is_pdf else '; project retains recognized text'))


def audit_image_live_text(process,main,binary,output,results):
    folder=output/'live-text-images';folder.mkdir()
    first=folder/'a-text.png';first.write_bytes((Path(__file__).resolve().parent.parent/'crates/win42/fixtures/ocr-english.png').read_bytes())
    second=folder/'b-green.png';write_png(second,320,240,bytes([0,200,0,255])*(320*240))
    original=first.read_bytes()
    command(main,1);choose_file(process,first)
    def opened():
        if first.name in text(main)and' * - 'not in text(main)and u.IsWindowEnabled(main):return(True,None)
        prompt=next((h for h in windows(process.pid)if classname(h)=='#32770'and u.IsWindowEnabled(h)and all(u.GetDlgItem(h,i)for i in(2,6,7))),None)
        return(False,prompt)if prompt else None
    ready,prompt=wait_for(opened,'Live Text source open',process)
    if not ready:click(prompt,7);wait_for(lambda:first.name in text(main)and u.IsWindowEnabled(main),'Live Text source after discard',process)
    def status():return ' '.join(text(h)for h in windows(process.pid,main))
    fill_prompt(process,main,43,'Recognize Image Text',['eng'])
    wait_for(lambda:'recognized words'in status(),'image words recognized',process,timeout=90)
    assert' * - 'not in text(main)
    command(main,45);all_text=wait_for(lambda:(v if(v:=clipboard_text())and'offline text recognition'in v and'1234567890'in v else None),'copy all image text',process)
    canvas=wait_for(lambda:next((h for h in windows(process.pid,main)if classname(h)=='Suite42Canvas'),None),'Live Text image canvas',process)
    rect=w.RECT();assert u.GetClientRect(canvas,c.byref(rect));scale=min(rect.right/1200,rect.bottom/320)
    def point(x,y):return(round((rect.bottom-320*scale)/2+y*scale)<<16)|round((rect.right-1200*scale)/2+x*scale)
    p=point(330,110)
    u.SendMessageW(canvas,0x201,1,p);u.SendMessageW(canvas,0x203,1,p);u.SendMessageW(canvas,0x202,0,p)
    wait_for(lambda:not(u.GetMenuState(u.GetMenu(main),44,0)&3),'copy command enabled after text selection',process)
    # A brief clipboard lease in another process must not turn Copy into an error dialog.
    api=c.WinDLL('user32',use_last_error=True);api.OpenClipboard.argtypes=[w.HWND]
    wait_for(lambda:bool(api.OpenClipboard(None)),'temporary clipboard contention',process)
    try:command(main,44);time.sleep(0.05)
    finally:assert api.CloseClipboard()
    wait_for(lambda:clipboard_text()=='offline','native double-click recognized word after clipboard contention',process)
    u.SendMessageW(canvas,0x201,1,point(330,110));u.SendMessageW(canvas,0x200,1,point(500,110));u.SendMessageW(canvas,0x202,0,point(500,110))
    command(main,44);wait_for(lambda:clipboard_text()=='offline text','native drag across recognized words',process)
    command(main,46);command(main,44);wait_for(lambda:clipboard_text()==all_text,'native select all recognized text',process)
    u.SetForegroundWindow(main)
    u.RedrawWindow.argtypes=[w.HWND,c.c_void_p,w.HANDLE,w.UINT]
    assert u.RedrawWindow(canvas,None,None,0x185)  # invalidate/erase/update now, including children
    capture_window(canvas,output/'IMG42-live-text-canvas.bmp')
    raster=(output/'IMG42-live-text-canvas.bmp').read_bytes()[54:]
    assert sum(raster[i]>raster[i+2]+30 and raster[i]>raster[i+1]+30 for i in range(0,len(raster),4))>2000,'Recognized selection is not visibly painted'
    assert sum(max(raster[i:i+3])<160 for i in range(0,len(raster),4))>500,'Image text glyphs are not visibly painted'
    capture_window(main,output/'IMG42-live-text-selection.bmp')
    u.SendMessageW(canvas,0x100,27,0)
    wait_for(lambda:bool(u.GetMenuState(u.GetMenu(main),44,0)&3),'Escape clears recognized selection',process)
    def save(name):
        path=output/f'IMG42-live-text-{name}.png';save_as(main);choose_file(process,path);output_ready(process,main,path,b'\x89PNG\r\n\x1a\n');return png_pixels(path)
    assert save('reading')==png_pixels(first)
    command(main,24);u.SendMessageW(canvas,0x201,1,point(280,100));u.SendMessageW(canvas,0x200,1,point(410,100));u.SendMessageW(canvas,0x202,0,point(410,100))
    assert save('ink')!=png_pixels(first)
    command(main,4);assert save('undo')==png_pixels(first)
    command(main,1);choose_file(process,first)
    wait_for(lambda:first.name in text(main)and' * - 'not in text(main)and u.IsWindowEnabled(main),'original image reopened for navigation',process)
    wait_for(lambda:'recognized words'in status(),'reopened image recognized',process,timeout=90)
    command(main,49);command(main,46)
    # Space must leave a text selection and advance the actual displayed file.
    u.SendMessageW(canvas,0x100,32,0)
    wait_for(lambda:second.name in text(main)and'320 × 240 pixels'in status(),'Space navigates after Live Text',process)
    assert u.GetMenuState(u.GetMenu(main),44,0)&3
    assert first.read_bytes()==original
    results['checks'].append('offline image OCR, copy all, native double-click/select-all/Escape, exact saved image pixels, drawing over text with complete Undo, and Space navigation invalidate old recognized text')


def audit_pdf_label_artwork(process,main,binary,output,results):
    source=output/'label-artwork-source.pdf';minimal_pdf(source,['ARTWORK-TARGET','PRIVATE-OTHER']);original=source.read_bytes();dll=binary.parent/'pdfium.dll'
    command(main,1);choose_file(process,source)
    def opened():
        if source.name in text(main)and' * - 'not in text(main)and u.IsWindowEnabled(main):return(True,None)
        prompt=next((h for h in windows(process.pid)if classname(h)=='#32770'and u.IsWindowEnabled(h)and all(u.GetDlgItem(h,i)for i in(2,6,7))),None)
        return(False,prompt)if prompt else None
    ready,prompt=wait_for(opened,'artwork PDF source',process)
    if not ready:click(prompt,7);wait_for(lambda:source.name in text(main)and' * - 'not in text(main)and u.IsWindowEnabled(main),'artwork source after discard',process)
    command(main,10)
    fill_prompt(process,main,21,'Crop page (PDF points)',['20','20','575','820'])
    command(main,18);wait_for(lambda:any('Tool: Rectangle'in text(h)for h in windows(process.pid,main)),'artwork rectangle tool',process)
    canvas=wait_for(lambda:next((h for h in windows(process.pid,main)if classname(h)=='Suite42Canvas'),None),'cropped artwork canvas rendered',process)
    u.SendMessageW(canvas,0x201,1,(90<<16)|90);u.SendMessageW(canvas,0x200,1,(170<<16)|170);u.SendMessageW(canvas,0x202,0,(170<<16)|170)
    fill_prompt(process,main,90,'Text Watermark',['UNSAVED-ART','60','100','0','#0000FF','1'])
    caption=text(main);assert' * - 'in caption
    def sheet(name,area='',copies='2, 24'):
        command(main,65518);finish_prompt(process,'Label Source',['Page 1 Artwork']);finish_prompt(process,'Create Artwork Label Sheet',[area,'70, 37, 0, 0',copies])
        path=output/f'artwork-labels-{name}.pdf';choose_file(process,path);output_ready(process,main,path,b'%PDF-');return path
    def colors(path):
        _,_,pixels=pdf_pixels(dll,path,1)
        red=sum(pixels[i+2]>pixels[i]+40 and pixels[i+2]>pixels[i+1]+40 for i in range(0,len(pixels),4))
        blue=sum(pixels[i]>pixels[i+2]+40 and pixels[i]>pixels[i+1]+40 for i in range(0,len(pixels),4))
        return red,blue
    full=sheet('full');pages=pdf_text(dll,full,by_page=True)
    assert len(pages)==2 and all(page.count('ARTWORK-TARGET')==1 and'PRIVATE-OTHER'not in page for page in pages),pages
    red,blue=colors(full);assert red>10 and blue>10,(red,blue)
    assert text(main)==caption,'Label output changed the live PDF or its save state'
    command(main,4);without=sheet('undo');red2,blue2=colors(without);assert red2>10 and blue2==0,(red2,blue2)
    command(main,5);redone=sheet('redo');assert pdf_pixels(dll,redone,1)==pdf_pixels(dll,full,1)
    region=sheet('region','30, 730, 450, 70','1, 1');assert 'ARTWORK-TARGET'in pdf_text(dll,region)and'PRIVATE-OTHER'not in pdf_text(dll,region)
    # Native mouse selection on the rotated, cropped page must follow the same
    # unrotated PDF geometry as an independently entered region.
    for _ in range(4):
        u.SendMessageW(main,WM_COMMAND,16,0)
        wait_for(lambda:not(u.GetMenuState(u.GetMenu(main),117,0)&3),'render before region gesture',process)
    canvas=wait_for(lambda:next((h for h in windows(process.pid,main)if classname(h)=='Suite42Canvas'),None),'PDF artwork canvas visible after rotation render',process)
    capture_window(main,output/'PDF42-label-area-ready.bmp')
    command(main,117)
    wait_for(lambda:any('Drag an area for labels'in text(h)for h in windows(process.pid,main)),'PDF area tool active',process)
    u.SendMessageW(canvas,0x201,1,(280<<16)|430);u.SendMessageW(canvas,0x200,1,(5<<16)|380)
    u.SendMessageW(canvas,0x100,27,0)
    wait_for(lambda:not any('Drag an area for labels'in text(h)for h in windows(process.pid,main)),'Escape cancels PDF label region',process)
    assert not any(classname(h)=='Suite42InputDialog'for h in windows(process.pid))
    command(main,117)
    wait_for(lambda:any('Drag an area for labels'in text(h)for h in windows(process.pid,main)),'PDF area tool active',process)
    u.SendMessageW(canvas,0x201,1,(280<<16)|430);u.SendMessageW(canvas,0x200,1,(5<<16)|380)
    u.RedrawWindow.argtypes=[w.HWND,c.c_void_p,w.HANDLE,w.UINT]
    assert u.RedrawWindow(canvas,None,None,0x185)
    capture_window(main,output/'PDF42-label-area-drag.bmp')
    u.SendMessageW(canvas,0x202,0,(5<<16)|380)
    finish_prompt(process,'Create Artwork Label Sheet',['','70, 37, 0, 0','1, 1'])
    mouse=output/'artwork-labels-mouse.pdf';choose_file(process,mouse);output_ready(process,main,mouse,b'%PDF-')
    assert text(main)==caption and'ARTWORK-TARGET'in pdf_text(dll,mouse)
    # WinRT reports the 90-degree CropBox in 96-DPI units; four zoom-outs give 437 x 303 pixels.
    left=20+5/303*555;bottom=20+380/437*800;right=20+280/303*555;top=20+430/437*800
    manual=sheet('mouse-reference',f'{left}, {bottom}, {right-left}, {top-bottom}','1, 1')
    mw,mh,mp=pdf_pixels(dll,mouse,0);rw,rh,rp=pdf_pixels(dll,manual,0)
    assert(mw,mh)==(rw,rh)
    difference=sum(abs(a-b)for a,b in zip(mp,rp))/(255*len(mp))
    assert difference<0.001,('Mouse region differs from independent PDF-point region',difference)
    assert source.read_bytes()==original
    results['checks'].append('PDF area gesture supports reverse drag and Escape, opens the common preview after release, maps rotated/cropped pixels to independently checked PDF geometry and preserves live history/source')
    assert source.read_bytes()==original
    results['checks'].append('current unsaved PDF artwork and optional region retain vector text, rotation/crop, visible annotations, repeated copies/first slot and no other page payload; label export preserves source/history, Undo removes only the watermark and Redo restores exact PDFium pixels')


def audit_doc_picture_labels(process,main,binary,output,results):
    picture=output/'doc-artwork-source.png';rgba=b''.join(bytes([255,0,0,255]if x<50 else[0,0,255,128])for y in range(50)for x in range(100));write_png(picture,100,50,rgba)
    source=output/'doc-picture-artwork-source.rtf';source.write_bytes(br'{\rtf1\ansi PRIVATE-CAPTION\par{\pict\pngblip\picw100\pich50\picwgoal2400\pichgoal1200 '+picture.read_bytes().hex().encode()+br'}\par}');original=source.read_bytes()
    command(main,1);choose_file(process,source)
    def opened():
        if source.name in text(main)and' * - 'not in text(main)and u.IsWindowEnabled(main):return(True,None)
        prompt=next((h for h in windows(process.pid)if classname(h)=='#32770'and u.IsWindowEnabled(h)and all(u.GetDlgItem(h,i)for i in(2,6,7))),None)
        return(False,prompt)if prompt else None
    ready,prompt=wait_for(opened,'picture label source opened',process)
    if not ready:click(prompt,7);wait_for(lambda:source.name in text(main)and' * - 'not in text(main)and u.IsWindowEnabled(main),'picture label source after discard',process)
    caption=text(main)
    command(main,65518);finish_prompt(process,'Label Source',['Picture 1 Artwork']);finish_prompt(process,'Create Artwork Label Sheet',['','70, 37, 0, 0','2, 24'])
    path=output/'DOC42-picture-artwork-labels.pdf';choose_file(process,path);output_ready(process,main,path,b'%PDF-')
    pages=pdf_text(binary.parent/'pdfium.dll',path,by_page=True);assert len(pages)==2 and all('PRIVATE-CAPTION'not in page for page in pages)
    for index in range(2):
        _,_,pixels=pdf_pixels(binary.parent/'pdfium.dll',path,index)
        red=sum(pixels[i+2]>pixels[i]+40 and pixels[i+2]>pixels[i+1]+40 for i in range(0,len(pixels),4))
        blue=sum(pixels[i]>pixels[i+2]+40 and pixels[i]>pixels[i+1]+40 for i in range(0,len(pixels),4))
        assert red>8000 and blue>8000,(index,red,blue)
    assert text(main)==caption and source.read_bytes()==original
    results['checks'].append('DOC picture artwork uses displayed aspect ratio and original pixels/alpha on paper, supports stock copies/first slot, omits surrounding document text and preserves source/save state')
    mixed_source=output/'doc-mixed-artwork-source.rtf'
    mixed_source.write_bytes(br'{\rtf1\ansi{\colortbl;\red0\green200\blue0;}\cf1 PRIVATE-BEFORE \u-10179?\u-8704?\par\cf0\fs40\b MIXED-SELECTED\b0\par{\pict\pngblip\picw100\pich50\picwgoal2400\pichgoal1200 '+picture.read_bytes().hex().encode()+br'}\par\cf1 PRIVATE-AFTER\par}')
    mixed_original=mixed_source.read_bytes();command(main,1);choose_file(process,mixed_source)
    wait_for(lambda:mixed_source.name in text(main)and' * - 'not in text(main)and u.IsWindowEnabled(main),'mixed content source opened',process)
    edit=u.GetDlgItem(main,101)
    def selection():
        value=u.SendMessageW(edit,0xB0,0,0);return(value&0xffff,(value>>16)&0xffff)
    fill_prompt(process,main,7,'Find next',['PRIVATE-AFTER']);end=selection()[0]
    fill_prompt(process,main,7,'Find next',['MIXED-SELECTED']);start=selection()[0];assert start<end
    u.SendMessageW(edit,0xB1,start,end);selected_range=selection();caption=text(main)
    command(main,65518);finish_prompt(process,'Label Source',['Selected Content Artwork']);finish_prompt(process,'Create Artwork Label Sheet',['','70, 37, 0, 0','2, 24'])
    mixed=output/'DOC42-selected-mixed-labels.pdf';choose_file(process,mixed);output_ready(process,main,mixed,b'%PDF-')
    assert len(pdf_text(binary.parent/'pdfium.dll',mixed,by_page=True))==2
    for index in range(2):
        _,_,pixels=pdf_pixels(binary.parent/'pdfium.dll',mixed,index)
        red=sum(pixels[i+2]>pixels[i]+40 and pixels[i+2]>pixels[i+1]+40 for i in range(0,len(pixels),4))
        blue=sum(pixels[i]>pixels[i+2]+40 and pixels[i]>pixels[i+1]+40 for i in range(0,len(pixels),4))
        green=sum(pixels[i+1]>pixels[i]+30 and pixels[i+1]>pixels[i+2]+30 for i in range(0,len(pixels),4))
        assert red>1000 and blue>1000 and green==0,('Selection lost picture or included surrounding green text',index,red,blue,green)
    assert text(main)==caption and selection()==selected_range and mixed_source.read_bytes()==mixed_original
    assert u.GetMenuState(u.GetMenu(main),4,0)&3,'Readonly label creation added a history step'
    command(main,65518);finish_prompt(process,'Label Source',['Selected Text']);finish_prompt(process,'Create Label Sheet',['MIXED-SELECTED','','QR','70, 37, 0, 0','1, 1'])
    # Check the actual prefilled native text before accepting custom label text in a separate scenario below.
    text_sheet=output/'DOC42-selected-text-labels.pdf';choose_file(process,text_sheet);output_ready(process,main,text_sheet,b'%PDF-')
    fill_prompt(process,main,7,'Find next',['PRIVATE-AFTER'])
    command(main,65518);finish_prompt(process,'Label Source',['Selected Text'])
    settings=modal(process,title='Create Label Sheet',native_class='Suite42InputDialog')
    assert text(u.GetDlgItem(settings,100))=='PRIVATE-AFTER','Native text selection after an image uses the wrong offsets'
    click(settings,2);wait_for(lambda:u.IsWindowEnabled(main),'native selected text label Cancel',process)
    assert mixed_source.read_bytes()==mixed_original and text(main)==caption
    results['checks'].append('native mixed-content selection after surrogate text preserves selected text styling/picture colors, omits surrounding green paragraphs, honors copies/first slot and keeps selection/history/source; selected-text labels after an image use actual native UTF-16 ranges')



def print_to_virtual_pdf(process,owner,request,printed,output):
    # Only an explicitly identified virtual queue may receive unattended jobs.
    # Both calls use the product's real Print button and standard PRINTDLG.
    request()
    printer=modal(process,title='Print',native_class='#32770')
    capture_window(printer,output/(printed.stem+'-dialog.bmp'))
    controls=[(u.GetDlgCtrlID(h),classname(h),text(h))for h in windows_for_dialog(printer)]
    (output/(printed.stem+'-controls.json')).write_text(json.dumps(controls,ensure_ascii=False,indent=2),encoding='utf-8')
    click(printer,2);wait_for(lambda:u.IsWindowEnabled(owner),'Print Cancel returns to preview',process)
    request();printer=modal(process,title='Print',native_class='#32770')
    combo=u.GetDlgItem(printer,1139)
    if combo:
        selected=u.SendMessageW(combo,0x147,0,0);assert selected>=0
        value=c.create_unicode_buffer(512);u.SendMessageW(combo,0x148,selected,c.cast(value,c.c_void_p).value)
        assert value.value=='Microsoft Print to PDF',('Refusing an unidentified printer',value.value)
        click(printer,1058)
        wait_for(lambda:u.SendMessageW(u.GetDlgItem(printer,1058),0xF0,0,0)==1,'Print page range selected',process)
        set_text(u.GetDlgItem(printer,1152),'2');set_text(u.GetDlgItem(printer,1153),'2')
    else:
        # ComCtl v6 presents printers through a shell list instead of cmb4.
        # Require the only installed/default queue to be the explicit virtual PDF driver.
        spool=c.WinDLL('winspool.drv',use_last_error=True)
        spool.GetDefaultPrinterW.argtypes=[w.LPWSTR,c.POINTER(w.DWORD)]
        size=w.DWORD();spool.GetDefaultPrinterW(None,c.byref(size));value=c.create_unicode_buffer(size.value)
        assert spool.GetDefaultPrinterW(value,c.byref(size))and value.value=='Microsoft Print to PDF',('Refusing the default printer',value.value)
        class Printer(c.Structure):_fields_=[('name',w.LPWSTR),('server',w.LPWSTR),('attributes',w.DWORD)]
        spool.EnumPrintersW.argtypes=[w.DWORD,w.LPWSTR,w.DWORD,c.c_void_p,w.DWORD,c.POINTER(w.DWORD),c.POINTER(w.DWORD)]
        needed=w.DWORD();count=w.DWORD();spool.EnumPrintersW(6,None,4,None,0,c.byref(needed),c.byref(count));buffer=c.create_string_buffer(needed.value)
        assert spool.EnumPrintersW(6,None,4,buffer,needed.value,c.byref(needed),c.byref(count))
        queues=c.cast(buffer,c.POINTER(Printer));names=[queues[i].name for i in range(count.value)]
        assert names==['Microsoft Print to PDF'],('Refusing ambiguous installed printers',names)
        selector=next(h for h in windows_for_dialog(printer)if classname(h)=='SysListView32')
        assert u.SendMessageW(selector,0x1004,0,0)==1 and u.SendMessageW(selector,0x100C,-1,2)==0,'Virtual printer must be selected'
        radio=next(h for h in windows_for_dialog(printer)if u.GetDlgCtrlID(h)==1059)
        click(u.GetParent(radio),1059)
        wait_for(lambda:u.SendMessageW(radio,0xF0,0,0)==1,'modern Print page range selected',process)
        entry=next(h for h in windows_for_dialog(printer)if u.GetDlgCtrlID(h)==1152)
        set_text(entry,'2')
    click(printer,1)
    choose_file(process,printed,keep_preview=True,open_preview=False)
    wait_for(lambda:printed.is_file()and printed.stat().st_size>100 and u.IsWindowEnabled(owner),'native PDF printer completes',process,timeout=60)
    return printed


def audit_label_preview(process,main,binary,output,results):
    caption=text(main)
    command(main,65518);finish_prompt(process,'Label Source',['Custom Text']);finish_prompt(process,'Create Label Sheet',['PREVIEW-AUDIT','PCB-PRINT-42','QR','100, 140, 2, 2','2, 4'])
    preview=modal(process,title='Label Sheet Preview',native_class='Off42PdfPreviewWindow')
    def ready(page,pages):return lambda:f'Page {page} of {pages}'in text(u.GetDlgItem(preview,100))
    wait_for(ready(1,2),'first label preview page',process)
    assert not u.IsWindowEnabled(main)and u.IsWindowEnabled(u.GetDlgItem(preview,3))
    u.RedrawWindow.argtypes=[w.HWND,c.c_void_p,w.HANDLE,w.UINT];assert u.RedrawWindow(preview,None,None,0x185)
    capture_window(preview,output/'labels-preview-fit.bmp')
    click(preview,6);wait_for(ready(2,2),'second preview page',process)
    click(preview,8);wait_for(lambda:'150%'in text(u.GetDlgItem(preview,100)),'preview zoom',process)
    u.RedrawWindow.argtypes=[w.HWND,c.c_void_p,w.HANDLE,w.UINT];assert u.RedrawWindow(preview,None,None,0x185)
    u.SendMessageW(preview,0x201,1,(300<<16)|300);u.SendMessageW(preview,0x200,1,(440<<16)|400);u.SendMessageW(preview,0x202,0,(440<<16)|400)
    assert u.RedrawWindow(preview,None,None,0x185)
    capture_window(preview,output/'labels-preview-zoom.bmp')
    rect=w.RECT();u.GetWindowRect(preview,c.byref(rect));assert u.MoveWindow(preview,rect.left,rect.top,540,540,True)
    time.sleep(0.1);u.GetWindowRect(preview,c.byref(rect))
    for identifier in range(1,10):
        button=u.GetDlgItem(preview,identifier);assert button
        bounds=w.RECT();u.GetWindowRect(button,c.byref(bounds));assert rect.left<=bounds.left<bounds.right<=rect.right and rect.top<=bounds.top<bounds.bottom<=rect.bottom,(identifier,tuple(getattr(bounds,k)for k in('left','top','right','bottom')))
    capture_window(preview,output/'labels-preview-narrow.bmp')
    saved=output/'labels-preview-first.pdf';choose_file(process,saved,keep_preview=True)
    wait_for(lambda:saved.is_file()and u.IsWindowEnabled(preview),'preview explicit Save',process)
    assert not u.IsWindowEnabled(main)and pdf_text(binary.parent/'pdfium.dll',saved).count('PREVIEW-AUDIT')==2
    printed=print_to_virtual_pdf(process,preview,lambda:click(preview,3),output/'labels-native-printed-page-2.pdf',output)
    assert printed.read_bytes().startswith(b'%PDF-')
    assert len(pdf_text(binary.parent/'pdfium.dll',printed,by_page=True))==1,'Page range was not honored'
    ow,oh,op=pdf_pixels(binary.parent/'pdfium.dll',printed,0);rw,rh,rp=pdf_pixels(binary.parent/'pdfium.dll',saved,1)
    assert(ow,oh)==(rw,rh)==(595,842),'Label job did not default to A4'
    def ink_bounds(width,height,pixels):
        points=[(i%width,i//width)for i in range(width*height)if min(pixels[i*4:i*4+3])<160]
        assert len(points)>15,'Printed label is blank'
        return(min(x for x,y in points),min(y for x,y in points),max(x for x,y in points),max(y for x,y in points))
    actual=ink_bounds(ow,oh,op);expected=ink_bounds(rw,rh,rp)
    assert all(abs(a-b)<=2 for a,b in zip(actual,expected)),('Native printer changed physical label placement/scale',actual,expected)
    assert not u.IsWindowEnabled(main)
    results['checks'].append('real Print button supports Cancel and Microsoft Print to PDF output, restricts to page 2, defaults to A4 and preserves independently measured actual-size label ink bounds')
    click(preview,4);settings=modal(process,title='Create Label Sheet',native_class='Suite42InputDialog');click(settings,2)
    wait_for(lambda:u.IsWindowEnabled(preview),'preview settings Cancel',process)
    click(preview,4);finish_prompt(process,'Create Label Sheet',['UPDATED-PREVIEW','','QR','70, 37, 0, 0','1, 1'])
    wait_for(ready(1,1),'regenerated preview page',process)
    updated=output/'labels-preview-updated.pdf';choose_file(process,updated,keep_preview=True)
    wait_for(lambda:updated.is_file()and u.IsWindowEnabled(preview),'updated preview Save',process)
    contents=pdf_text(binary.parent/'pdfium.dll',updated);assert contents.count('UPDATED-PREVIEW')==1 and'PREVIEW-AUDIT'not in contents
    click(preview,2);wait_for(lambda:not u.IsWindow(preview)and u.IsWindowEnabled(main),'preview Close',process)
    assert text(main)==caption
    results['checks'].append('embedded label preview renders/navigates pages, zooms, keeps controls onscreen in a narrow window, saves without closing, cancels/reapplies settings and regenerates exact PDF text without changing the parent document')


def audit_pdf_vector_print(process,main,binary,output,results):
    source=output/'native-vector-print-source.pdf'
    def stream(content,extra=''):
        return f'<< /Length {len(content)} {extra} >>\nstream\n'.encode()+content+b'\nendstream'
    objects=[b'<< /Type /Catalog /Pages 2 0 R >>',b'<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 7 0 R >> >> /Contents 4 0 R >>',
        stream(b'BT /F1 20 Tf 40 780 Td (PRIVATE-PAGE-ONE) Tj ET'),
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R /Annots [8 0 R 10 0 R] >>',
        stream(b'BT /F1 20 Tf 40 780 Td (PRINT-PAGE-TWO) Tj ET'),b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
        b'<< /Type /Annot /Subtype /Square /Rect [40 700 80 740] /F 0 /AP << /N 9 0 R >> >>',
        stream(b'0 1 0 rg 0 0 40 40 re f','/Type /XObject /Subtype /Form /BBox [0 0 40 40]'),
        b'<< /Type /Annot /Subtype /Square /Rect [100 700 140 740] /F 36 /AP << /N 11 0 R >> >>',
        stream(b'0 1 1 rg 0 0 40 40 re f','/Type /XObject /Subtype /Form /BBox [0 0 40 40]')]
    data=bytearray(b'%PDF-1.4\n');offsets=[0]
    for index,body in enumerate(objects,1):offsets.append(len(data));data.extend(f'{index} 0 obj\n'.encode()+body+b'\nendobj\n')
    crossref=len(data);data.extend(f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode())
    for offset in offsets[1:]:data.extend(f'{offset:010} 00000 n \n'.encode())
    data.extend(f'trailer\n<< /Root 1 0 R /Size {len(offsets)} >>\nstartxref\n{crossref}\n%%EOF\n'.encode());source.write_bytes(data);original=source.read_bytes()
    command(main,1);choose_file(process,source)
    def opened():
        if source.name in text(main)and' * - 'not in text(main)and u.IsWindowEnabled(main):return(True,None)
        prompt=next((h for h in windows(process.pid)if classname(h)=='#32770'and all(u.GetDlgItem(h,i)for i in(2,6,7))),None)
        return(False,prompt)if prompt else None
    ready,prompt=wait_for(opened,'normal PDF print source',process)
    if not ready:click(prompt,7);wait_for(lambda:source.name in text(main)and' * - 'not in text(main)and u.IsWindowEnabled(main),'normal print source after discard',process)
    fill_prompt(process,main,84,'Go to Page',['2']);u.SendMessageW(main,WM_COMMAND,10,0)
    fill_prompt(process,main,21,'Crop page (PDF points)',['20','20','575','820'])
    wait_for(lambda:not(u.GetMenuState(u.GetMenu(main),117,0)&3),'rotated/cropped print page rendered',process)
    command(main,18);wait_for(lambda:any('Tool: Rectangle'in text(h)for h in windows(process.pid,main)),'print annotation tool',process)
    canvas=next(h for h in windows(process.pid,main)if classname(h)=='Suite42Canvas')
    u.SendMessageW(canvas,0x201,1,(180<<16)|90);u.SendMessageW(canvas,0x200,1,(260<<16)|190);u.SendMessageW(canvas,0x202,0,(260<<16)|190)
    fill_prompt(process,main,90,'Text Watermark',['UNSAVED-PRINT','30','100','0','#0000FF','2'])
    caption=text(main);assert' * - 'in caption
    def reference(name):
        path=output/('native-print-reference-'+name+'.pdf')
        command(main,80);finish_prompt(process,'Extract PDF Pages',['2']);choose_file(process,path);output_ready(process,main,path,b'%PDF-');return path
    before=reference('before');assert text(main)==caption
    def color_points(width,pixels,color):
        matches=[]
        for i in range(0,len(pixels),4):
            blue,green,red=pixels[i:i+3]
            match={'red':red>green+50 and red>blue+50,'blue':blue>red+50 and blue>green+50,'green':green>red+50 and green>blue+50,'cyan':green>red+50 and blue>red+50}[color]
            if match:matches.append((i//4%width,i//4//width))
        return matches
    def bounds(points):
        assert len(points)>30,'Expected visible printed artwork'
        return(min(x for x,y in points),min(y for x,y in points),max(x for x,y in points),max(y for x,y in points))
    rw,rh,rp=pdf_pixels(binary.parent/'pdfium.dll',before,0,True)
    assert(rw,rh)==(800,555)and len(color_points(rw,rp,'green'))>500 and not color_points(rw,rp,'cyan')
    printed=print_to_virtual_pdf(process,main,lambda:command(main,52),output/'normal-pdf-printed-page-2.pdf',output)
    assert text(main)==caption and source.read_bytes()==original
    assert len(pdf_text(binary.parent/'pdfium.dll',printed,by_page=True))==1
    ow,oh,op=pdf_pixels(binary.parent/'pdfium.dll',printed,0)
    factor=min(1,ow/rw,oh/rh);offset=((ow-rw*factor)/2,(oh-rh*factor)/2)
    for color in['red','blue']:
        expected=bounds(color_points(rw,rp,color));actual=bounds(color_points(ow,op,color))
        mapped=[expected[0]*factor+offset[0],expected[1]*factor+offset[1],expected[2]*factor+offset[0],expected[3]*factor+offset[1]]
        assert all(abs(a-b)<=3 for a,b in zip(actual,mapped)),('Wrong rotated/cropped PDF print fit',color,actual,mapped)
    assert not color_points(ow,op,'green'),'A nonprinting annotation reached the printer'
    assert len(color_points(ow,op,'cyan'))>100,'A print-only annotation was lost'
    command(main,4);undo=reference('undo');_,_,pixels=pdf_pixels(binary.parent/'pdfium.dll',undo,0,True);assert not color_points(rw,pixels,'blue')and color_points(rw,pixels,'red')
    command(main,5);redo=reference('redo');assert pdf_pixels(binary.parent/'pdfium.dll',redo,0,True)==(rw,rh,rp)
    assert text(main)==caption and source.read_bytes()==original
    results['checks'].append('ordinary PDF printing uses a vector snapshot, native page range and Mac-style no-upscale centered fit, preserves crop/rotation and unsaved edits, honors print-only/nonprinting annotations, and leaves source/history/Undo/Redo intact')


def audit_common_print_command(process,main,app,results):
    identifier={'DOC42':315,'XLS42':62,'PDF42':52,'IMG42':36,'PPT42':40,'SCN42':27}[app]
    u.GetSubMenu.argtypes,u.GetSubMenu.restype=[w.HMENU,c.c_int],w.HMENU
    file_menu=u.GetSubMenu(u.GetMenu(main),0)
    assert u.GetMenuState(file_menu,identifier,0)!=0xffffffff,'Print is absent from File menu'
    if app=='DOC42':
        client=w.RECT();u.GetClientRect(main,c.byref(client))
        buttons=[h for h in windows_for_dialog(main)if classname(h)=='Button'and u.GetDlgCtrlID(h)!=65520]
        if client.right>=900:
            tops=[]
            for button in buttons:
                rect=w.RECT();u.GetWindowRect(button,c.byref(rect));tops.append(rect.top)
            assert len(set(tops))<=1,'DOC primary toolbar still consumes multiple rows'
        assert u.GetMenuState(u.GetMenu(main),323,0)!=0xffffffff,'A secondary command disappeared with toolbar compaction'
    caption=text(main);u.SetForegroundWindow(main)
    u.keybd_event.argtypes=[c.c_ubyte,c.c_ubyte,w.DWORD,c.c_size_t]
    u.keybd_event(0x11,0,0,0);u.keybd_event(0x50,0,0,0);u.keybd_event(0x50,0,2,0);u.keybd_event(0x11,0,2,0)
    printer=modal(process,title='Print',native_class='#32770');click(printer,2)
    wait_for(lambda:u.IsWindowEnabled(main),'Ctrl+P Cancel returns to '+app,process)
    assert text(main)==caption
    results['checks'].append('File Print and actual Ctrl+P reach the native printer dialog; Cancel preserves the document')


def audit(binary, output, extended, pictures=False, technical=False, labels=False, pages=False, host_commands=False, engineering=False, text_exports=False, extended_formulas=False, rich_search=False, preset_shapes=False, pdf_decorations=False, statistical_formulas=False, pdf_comparison=False, pdf_merge=False, pdf_batch_headers=False, array_formulas=False, ocr=False, ocr_only=False, label_artwork=False, label_preview=False, native_print=False):
    startup = None
    if extended:
        if binary.stem.lower() == "pdf42":
            startup = output / "startup.pdf"
            minimal_pdf(startup)
        elif binary.stem.lower() == "xls42":
            startup = output / "startup.csv"
            startup.write_text("Name,Code,,,,,,\nBoard 0007,PCB-0007,,,,,,\nBoard 0008,PCB-0008,,,,,,\n", encoding="utf-8")
        elif binary.stem.lower() in {"img42", "scn42"}:
            startup = output / (binary.stem + "-canvas.png")
            write_png(startup, 64, 64, bytes([255] * (64 * 64 * 4)))
        elif binary.stem.lower() == "doc42":
            startup = output / "startup.rtf"
            startup.write_bytes(br"{\rtf1\ansi Board 0007}")
    environment = os.environ.copy()
    environment["OFF42_UI_TRACE_FILE"] = str(output / (binary.stem + "-commands.log"))
    environment["OFF42_AUDIT_OUTPUT"]=str(output)
    process = subprocess.Popen([str(binary)] + ([str(startup)] if startup else []), env=environment)
    results = {"app": binary.name, "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "checks": []}
    try:
        main = modal(process, native_class="Suite42Native")
        results["checks"].append("native main window visible")
        if startup:
            if binary.stem.lower() == "scn42":
                wait_for(lambda: any(text(h).startswith("1 pages |") for h in windows(process.pid, main)), "scan image import", process)
            else:
                wait_for(lambda: startup.name in text(main) and u.IsWindowEnabled(main), "command-line file open", process)
            results["checks"].append("command-line file open reaches the normal document session")
        if extended and binary.stem.lower() in {"doc42", "ppt42"}:
            original_caption = text(main)
            command(main, 101 | (0x400 << 16))  # EN_UPDATE must not dispatch app action 101.
            command(main, 101 | (0x200 << 16))  # EN_KILLFOCUS is also only a notification.
            time.sleep(0.1)
            assert text(main) == original_caption, "Control notification changed document state"
            results["checks"].append("editor layout/focus notifications do not reload or dirty the document")
        command(main, 6)
        about = modal(process, native_class="#32770")
        content = " ".join(text(h) for h in windows(process.pid, about))
        assert binary.stem.upper() in content, content
        click(about, 1)
        wait_for(lambda: u.IsWindowEnabled(main), "About closed", process)
        results["checks"].append("About command identifies app and closes")
        if native_print:audit_common_print_command(process,main,binary.stem.upper(),results)
        if ocr_only:
            if binary.stem.lower()=="img42":audit_image_live_text(process,main,binary,output,results)
            else:audit_bundled_ocr(process,main,binary,output,results)
            return results
        if extended and binary.stem.lower() in {"doc42", "ppt42"}:
            edit = u.GetDlgItem(main, 101)
            assert u.SendMessageW(edit, 1024 + 121, 0, 0) & 0x0800
            command(main, 65510)
            wait_for(lambda: not (u.SendMessageW(edit, 1024 + 121, 0, 0) & 0x0800), "spelling disabled", process)
            command(main, 65510)
            wait_for(lambda: u.SendMessageW(edit, 1024 + 121, 0, 0) & 0x0800, "spelling enabled", process)
            results["checks"].append("native inline proofing flags follow the menu toggle")
        if extended and binary.stem.lower() in {"img42", "scn42"}:
            audit_marks(process, main, binary, output, results)
            if host_commands:
                audit_crop_command(process, main, binary, output, results)
        if binary.stem.lower() not in {"doc42", "pdf42", "xls42"}:
            if ocr and binary.stem.lower()=="scn42":audit_bundled_ocr(process,main,binary,output,results)
            if ocr and binary.stem.lower()=="img42":audit_image_live_text(process,main,binary,output,results)
            if technical and binary.stem.lower() == "ppt42":
                audit_technical_objects(process, main, binary, output, results)
            if preset_shapes and binary.stem.lower() == "ppt42":
                audit_preset_shapes(process,main,output,results)
                audit_ppt_show(process,main,output,results)
            return results
        if binary.stem.lower() == "pdf42" and not startup:
            source = output / "input.pdf"
            minimal_pdf(source)
            command(main, 1)
            choose_file(process, source)
            wait_for(lambda: source.name in text(main) and u.IsWindowEnabled(main), "PDF open", process)
        elif binary.stem.lower() == "xls42":
            if not startup:
                source = output / "input.csv"
                source.write_text("Name,Code\nBoard 0007,PCB-0007\nBoard 0008,PCB-0008\n", encoding="utf-8")
                command(main, 1)
                choose_file(process, source)
                wait_for(lambda: source.name in text(main) and u.IsWindowEnabled(main), "CSV open", process)
            fill_prompt(process, main, 25, "Range (1-based)", ["1", "1", "3", "2"])
            wait_for(lambda: u.IsWindowEnabled(main), "range selected", process)
        png = output / f"{binary.stem}-barcode.png"
        fill_prompt(process, main, 65517, "Create Barcode", ["PCB-0007", "Code 128", "55"])
        choose_file(process, png)
        output_ready(process, main, png, b"\x89PNG\r\n\x1a\n")
        width, height = struct.unpack_from(">II", png.read_bytes(), 16)
        assert width > 100 and height > 20
        results["checks"].append("barcode created through native dialog and saved as PNG")
        pdf = output / f"{binary.stem}-labels.pdf"
        fields = (["1", "2, 0, 1", "Code 128", "100, 140, 2, 2", "1, 4"]
            if binary.stem.lower() == "xls42" else
            ["Board 0007", "PCB-0007", "Code 128", "100, 140, 2, 2", "2, 4"])
        if binary.stem.lower()=="pdf42":
            command(main,65518);finish_prompt(process,"Label Source",["Custom Text"]);finish_prompt(process,"Create Label Sheet",fields)
        else:
            fill_prompt(process, main, 65518, "Create Label Sheet", fields)
        choose_file(process, pdf)
        output_ready(process, main, pdf, b"%PDF-")
        results["checks"].append("label sheet created through native dialog and saved as PDF")
        if extended:
            preset = output / f"{binary.stem}-stock.pdf"
            fields[3] = "AVERY L4732 — 35.6 × 16.9 mm"
            fields[2] = "QR"
            fields[4] = "1, 1"
            if binary.stem.lower() == "xls42":
                fields[0], fields[1] = "1", "0, 0, 1"
            else:
                fields[0], fields[1] = "Board", ""
            if binary.stem.lower()=="pdf42":
                command(main,65518);finish_prompt(process,"Label Source",["Custom Text"]);finish_prompt(process,"Create Label Sheet",fields)
            else:
                fill_prompt(process, main, 65518, "Create Label Sheet", fields)
            choose_file(process, preset)
            output_ready(process, main, preset, b"%PDF-")
            results["checks"].append("named stock selected through the native combo box")
            symbol = output / f"{binary.stem}-symbol.png"
            fill_prompt(process, main, 65514, "Symbol or Code", ["Protective earth", "", "20", "no"])
            choose_file(process, symbol)
            output_ready(process, main, symbol, b"\x89PNG\r\n\x1a\n")
            raw = symbol.read_bytes()
            assert b"Description\0Suite42.TechnicalElement.v1:" in raw
            results["checks"].append("technical symbol exported with a reusable recipe")
            calibration = output / f"{binary.stem}-calibration.pdf"
            command(main, 65511)
            choose_file(process, calibration)
            output_ready(process, main, calibration, b"%PDF-")
            results["checks"].append("calibration sheet exported through the normal menu")
        if pictures and binary.stem.lower() == "xls42":
            audit_sheet_pictures(process, main, output, results)
        if technical:
            audit_technical_objects(process, main, binary, output, results)
        if labels and binary.stem.lower() == "doc42":
            audit_table_labels(process, main, binary, output, results)
        if rich_search and binary.stem.lower() == "doc42":
            audit_rich_search(process, main, binary, output, results)
            audit_doc_search_options(process, main, output, results)
            audit_rich_format(process, main, binary, output, results)
            audit_spelling(process,main,binary,output,results)
        if engineering and binary.stem.lower() == "xls42":
            audit_engineering_formulas(process, main, binary, output, results, include_extended=extended_formulas)
            audit_goal_seek(process,main,binary,output,results)
            audit_solver(process,main,binary,output,results)
            audit_sparse_capacity(process,main,binary,output,results)
            audit_sheet_tabs(process,main,binary,output,results)
            audit_grid_expansion(process,main,binary,output,results)
            audit_advanced_sort(process,main,binary,output,results)
            audit_ods_number_formats(process,main,binary,output,results)
        if statistical_formulas and binary.stem.lower()=="xls42":
            audit_statistical_formulas(process,main,output,binary,results)
        if array_formulas and binary.stem.lower()=="xls42":
            audit_array_formulas(process,main,binary,output,results)
        if host_commands and binary.stem.lower() == "pdf42":
            audit_rectangle_command(process, main, binary, output, results)
        if pages and binary.stem.lower() == "pdf42":
            audit_pdf_page_exports(process, main, binary, output, results)
        if text_exports and binary.stem.lower() == "pdf42":
            audit_pdf_text_exports(process, main, binary, output, results)
        if pdf_decorations and binary.stem.lower()=="pdf42":
            audit_pdf_decorations(process,main,binary,output,results)
            audit_pdf_header_fonts(process,main,binary,output,results,native_print)
        if pdf_comparison and binary.stem.lower()=="pdf42":
            audit_pdf_comparison(process,main,binary,output,results)
        if pdf_merge and binary.stem.lower()=="pdf42":
            audit_pdf_merge(process,main,binary,output,results)
        if pdf_batch_headers and binary.stem.lower()=="pdf42":
            audit_pdf_batch_headers(process,main,binary,output,results)
        if ocr and binary.stem.lower()=='pdf42':
            audit_bundled_ocr(process,main,binary,output,results)
        if label_artwork and binary.stem.lower()=='pdf42':
            audit_pdf_label_artwork(process,main,binary,output,results)
        if label_artwork and binary.stem.lower()=='doc42':
            audit_doc_picture_labels(process,main,binary,output,results)
        if label_preview and binary.stem.lower()=='pdf42':
            audit_label_preview(process,main,binary,output,results)
        if native_print and binary.stem.lower()=='pdf42':audit_pdf_vector_print(process,main,binary,output,results)
        return results
    finally:
        if process.poll() is None:
            for hwnd in windows(process.pid):
                if classname(hwnd) == "Suite42Native":
                    u.PostMessageW(hwnd, WM_CLOSE, 0, 0)
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=5)



def capture_window(hwnd, path):
    """Capture the actual test-owned native window as a standalone BMP."""
    gdi = c.WinDLL("gdi32", use_last_error=True)
    u.GetWindowRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
    u.GetWindowDC.argtypes, u.GetWindowDC.restype = [w.HWND], w.HDC
    u.ReleaseDC.argtypes = [w.HWND, w.HDC]
    gdi.CreateCompatibleDC.argtypes, gdi.CreateCompatibleDC.restype = [w.HDC], w.HDC
    gdi.CreateCompatibleBitmap.argtypes, gdi.CreateCompatibleBitmap.restype = [w.HDC, c.c_int, c.c_int], w.HBITMAP
    gdi.SelectObject.argtypes, gdi.SelectObject.restype = [w.HDC, w.HANDLE], w.HANDLE
    gdi.BitBlt.argtypes = [w.HDC, c.c_int, c.c_int, c.c_int, c.c_int, w.HDC, c.c_int, c.c_int, w.DWORD]
    gdi.GetDIBits.argtypes = [w.HDC, w.HBITMAP, w.UINT, w.UINT, c.c_void_p, c.c_void_p, w.UINT]
    gdi.DeleteObject.argtypes, gdi.DeleteDC.argtypes = [w.HANDLE], [w.HDC]
    rect = w.RECT()
    assert u.GetWindowRect(hwnd, c.byref(rect))
    width, height = rect.right - rect.left, rect.bottom - rect.top
    assert 0 < width < 5000 and 0 < height < 5000
    dc = u.GetWindowDC(hwnd)
    target = gdi.CreateCompatibleDC(dc)
    bitmap = gdi.CreateCompatibleBitmap(dc, width, height)
    previous = gdi.SelectObject(target, bitmap)
    try:
        assert gdi.BitBlt(target, 0, 0, width, height, dc, 0, 0, 0x00CC0020)
        gdi.SelectObject(target, previous)
        info = struct.pack("<IiiHHIIiiII", 40, width, height, 1, 32, 0, width * height * 4, 0, 0, 0, 0)
        header = c.create_string_buffer(info)
        pixels = c.create_string_buffer(width * height * 4)
        assert gdi.GetDIBits(dc, bitmap, 0, height, pixels, header, 0) == height
        path.write_bytes(struct.pack("<2sIHHI", b"BM", 54 + len(pixels), 0, 0, 54) + info + pixels.raw)
    finally:
        gdi.SelectObject(target, previous)
        gdi.DeleteObject(bitmap)
        gdi.DeleteDC(target)
        u.ReleaseDC(hwnd, dc)


def audit_language(binary, output, code, index):
    source = output / "language-source.rtf"
    source.write_bytes(br"{\rtf1\ansi Board 0007}")
    environment = os.environ.copy()
    environment["OFF42_UI_TRACE_FILE"] = str(output / f"language-{code}-commands.log")
    process = subprocess.Popen([str(binary), "--language", code, "--", str(source)], env=environment)
    result = {"language": code, "checks": []}
    try:
        main = modal(process, native_class="Suite42Native")
        wait_for(lambda: source.name in text(main) and u.IsWindowEnabled(main), "localized file opening", process)
        catalogs = Path(__file__).resolve().parents[1] / "crates" / "win42" / "resources" / "translations"
        catalog = {} if code == "en" else json.loads((catalogs / (code + ".json")).read_text(encoding="utf-8"))
        overrides = json.loads((catalogs.parent / "windows-translations.json").read_text(encoding="utf-8"))
        catalog.update(overrides.get(code, {}))
        u.GetMenu.argtypes, u.GetMenu.restype = [w.HWND], w.HMENU
        u.GetMenuStringW.argtypes = [w.HMENU, w.UINT, w.LPWSTR, c.c_int, w.UINT]
        u.GetMenuState.argtypes = [w.HMENU, w.UINT, w.UINT]
        menu = u.GetMenu(main)
        label = c.create_unicode_buffer(512)
        assert u.GetMenuStringW(menu, 0, label, len(label), 0x400)
        assert label.value.replace("&", "") == catalog.get("File", "File"), (code, label.value)
        assert u.GetMenuState(menu, 64000 + index, 0) & 8, (code, "language not checked")
        assert text(u.GetDlgItem(main, 101)) == "Board 0007", (code, "document text changed")
        result["checks"].append("translated File menu, checked language and unchanged document text")
        command(main, 65509)
        dialog = modal(process, native_class="Suite42InputDialog")
        assert text(u.GetDlgItem(dialog, 2)) == catalog.get("Cancel", "Cancel")
        labels = [text(h) for h in windows_for_dialog(dialog) if classname(h).upper() == "STATIC"]
        for key in ["Symbol or barcode format", "Barcode value", "Width (mm)", "Caption (yes/no)"]:
            assert catalog.get(key, key) in labels, (code, key, labels)
        set_text(u.GetDlgItem(dialog, 101), "PCB-0007")
        set_text(u.GetDlgItem(dialog, 102), "20")
        # Leave the translated Yes choice selected: returning its stable original value is essential.
        if code in {"ru", "ar", "zh-Hans", "zh-Hant", "de", "ja"}:
            u.SetForegroundWindow(dialog)
            time.sleep(0.15)
            capture_window(dialog, output / f"language-{code}.bmp")
        click(dialog, 1)
        wait_for(lambda: u.IsWindowEnabled(main), "localized technical insertion", process)
        command(main, 65508)
        dialog = modal(process, native_class="Suite42InputDialog")
        assert text(u.GetDlgItem(dialog, 101)) == "PCB-0007", (code, "barcode value changed")
        yes = next((value for key, value in catalog.items() if key.lower() == "yes"), "yes")
        assert text(u.GetDlgItem(dialog, 103)) == yes, (code, "choice lost its value")
        click(dialog, 2)
        wait_for(lambda: u.IsWindowEnabled(main), "localized Cancel", process)
        command(main, 4)
        wait_for(lambda: text(u.GetDlgItem(main, 101)) == "Board 0007", "localized Undo preserves text", process)
        result["checks"].append("translated prompt actions and choice survive insertion/edit; barcode payload and text preserved")
        return result
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)



def audit_recents(binary, output):
    """Use isolated local preferences; never clear the user's actual recent list."""
    root = output / "lifecycle-settings"
    root.mkdir(exist_ok=True)
    environment = os.environ.copy()
    environment["LOCALAPPDATA"] = str(root)
    source_a, source_b = [output / ("recent-" + uuid.uuid4().hex + suffix + ".rtf") for suffix in ("-A", "-B")]
    source_a.write_bytes(br"{\rtf1\ansi Board A}")
    source_b.write_bytes(br"{\rtf1\ansi Board B}")
    process = subprocess.Popen([str(binary), "--language", "en", "--", str(source_a)], env=environment)
    result = {"app": binary.name, "scenario": "recent documents", "checks": []}
    prefs = root / "off42" / "Recent Documents" / "DOC42.json"
    def paths():
        try:
            return json.loads(prefs.read_text(encoding="utf-8"))["paths"]
        except (FileNotFoundError, ValueError):
            return []
    try:
        main = modal(process, native_class="Suite42Native")
        wait_for(lambda: source_a.name in text(main) and paths() == [str(source_a)], "first successful open recorded", process)
        command(main, 1)
        choose_file(process, source_b)
        wait_for(lambda: source_b.name in text(main) and paths() == [str(source_b), str(source_a)], "second successful open recorded", process)
        command(main, 62001)
        wait_for(lambda: source_a.name in text(main) and paths() == [str(source_a), str(source_b)], "recent command opens original file and moves it first", process)
        edit = u.GetDlgItem(main, 101)
        set_text(edit, "unsaved own text")
        wait_for(lambda: " * - DOC42" in text(main), "editor dirty state", process)
        command(main, 62001)
        confirmation = modal(process, native_class="#32770")
        click(confirmation, 2)  # Cancel must preserve all unsaved text.
        wait_for(lambda: u.IsWindowEnabled(main), "recent open cancelled", process)
        assert source_a.name in text(main) and text(edit) == "unsaved own text"
        assert paths() == [str(source_a), str(source_b)]
        command(main, 62001)
        confirmation = modal(process, native_class="#32770")
        click(confirmation, 7)  # Explicit discard in this test-owned document.
        wait_for(lambda: source_b.name in text(main) and u.IsWindowEnabled(main), "confirmed recent open", process)
        result["checks"].append("recent order/deduplication and real unsaved-change Cancel/Discard preserve expected document state")
        u.PostMessageW(main, WM_CLOSE, 0, 0)
        process.wait(timeout=5)
        process = subprocess.Popen([str(binary), "--language", "en"], env=environment)
        main = modal(process, native_class="Suite42Native")
        command(main, 62001)
        wait_for(lambda: source_a.name in text(main), "recent list restored after process restart", process)
        command(main, 62010)
        wait_for(lambda: paths() == [], "recent menu cleared persistently", process)
        result["checks"].append("recent paths survive restart and Clear Menu persists without altering source files")
        assert source_a.read_bytes() == br"{\rtf1\ansi Board A}"
        assert source_b.read_bytes() == br"{\rtf1\ansi Board B}"
        return result
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)



class OwnedChildProcess:
    """Handle only the new process displaying this test's unique document path."""
    def __init__(self, pid):
        self.pid = pid
        self.kernel = c.WinDLL("kernel32", use_last_error=True)
        self.kernel.OpenProcess.argtypes, self.kernel.OpenProcess.restype = [w.DWORD, w.BOOL, w.DWORD], w.HANDLE
        self.kernel.GetExitCodeProcess.argtypes = [w.HANDLE, c.POINTER(w.DWORD)]
        self.kernel.TerminateProcess.argtypes = [w.HANDLE, w.UINT]
        self.kernel.CloseHandle.argtypes = [w.HANDLE]
        self.handle = self.kernel.OpenProcess(0x1000 | 0x0001, False, pid)
        assert self.handle
    def poll(self):
        status = w.DWORD()
        assert self.kernel.GetExitCodeProcess(self.handle, c.byref(status))
        return None if status.value == 259 else status.value
    def terminate(self):
        assert self.kernel.TerminateProcess(self.handle, 1)
    def wait(self, timeout):
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            if self.poll() is not None:
                return self.poll()
            time.sleep(0.05)
        raise subprocess.TimeoutExpired("owned language child", timeout)
    def close_handle(self):
        if self.handle:
            self.kernel.CloseHandle(self.handle)
            self.handle = None


def child_processes(parent, executable):
    kernel = c.WinDLL("kernel32", use_last_error=True)
    class Entry(c.Structure):
        _fields_ = [("dwSize",w.DWORD),("cntUsage",w.DWORD),("th32ProcessID",w.DWORD),
            ("th32DefaultHeapID",c.c_size_t),("th32ModuleID",w.DWORD),("cntThreads",w.DWORD),
            ("th32ParentProcessID",w.DWORD),("pcPriClassBase",w.LONG),("dwFlags",w.DWORD),("szExeFile",w.WCHAR*260)]
    kernel.CreateToolhelp32Snapshot.argtypes, kernel.CreateToolhelp32Snapshot.restype = [w.DWORD,w.DWORD],w.HANDLE
    kernel.Process32FirstW.argtypes = kernel.Process32NextW.argtypes = [w.HANDLE,c.POINTER(Entry)]
    kernel.CloseHandle.argtypes = [w.HANDLE]
    snapshot = kernel.CreateToolhelp32Snapshot(2,0)
    assert snapshot and snapshot != w.HANDLE(-1).value
    found = []
    try:
        entry = Entry(); entry.dwSize = c.sizeof(entry)
        valid = kernel.Process32FirstW(snapshot,c.byref(entry))
        while valid:
            if entry.th32ParentProcessID == parent and entry.szExeFile.casefold() == executable.casefold():
                found.append(entry.th32ProcessID)
            valid = kernel.Process32NextW(snapshot,c.byref(entry))
    finally:
        kernel.CloseHandle(snapshot)
    return found


def audit_multiple_windows(bin_dir, output):
    root = output / "multiwindow-settings"
    root.mkdir(exist_ok=True)
    environment = os.environ.copy(); environment["LOCALAPPDATA"] = str(root)
    result = {"scenario":"independent document windows", "checks":[]}
    for app in ["DOC42","XLS42","PPT42","SCN42","PDF42","IMG42"]:
        binary = bin_dir / (app + ".exe")
        source = output / ("new-window-" + uuid.uuid4().hex + ".rtf")
        if app == "DOC42": source.write_bytes(br"{\rtf1\ansi Parent source}")
        process = subprocess.Popen([str(binary),"--language","de","--"] + ([str(source)] if app=="DOC42" else []),env=environment)
        owned = []
        try:
            main = modal(process,native_class="Suite42Native")
            if app == "DOC42":
                wait_for(lambda: source.name in text(main),"parent source opened",process)
                replace_editor_text(u.GetDlgItem(main,101),"parent unsaved text")
                wait_for(lambda: " * - " in text(main),"parent remains dirty",process)
            caption = text(main)
            for identifier in ([9,65506] if app=="DOC42" else [65506] if app in {"PDF42","IMG42"} else [9]):
                command(main,identifier)
                pid = wait_for(lambda: next(iter(child_processes(process.pid,binary.name)),None),"owned child for " + app,process)
                child = OwnedChildProcess(pid); owned.append(child)
                window = modal(child,native_class="Suite42Native")
                assert u.GetMenuState(u.GetMenu(window),64009,0)&8,"Child did not inherit the current German language override"
                assert u.IsWindowEnabled(main) and text(main)==caption and process.poll() is None
                if app=="DOC42":
                    edit=u.GetDlgItem(window,101)
                    wait_for(lambda: text(edit)=="","new document starts empty",child)
                    replace_editor_text(edit,"child window text")
                    command(window,4)
                    wait_for(lambda: text(edit)=="","child history is independent",child)
                    command(window,5)
                    wait_for(lambda: text(edit)=="child window text","child redo",child)
                    target=output/f"new-window-{identifier}.doc42"
                    save_as(window); choose_file(child,target)
                    output_ready(child,window,target,b"{")
                    assert b"child window text" in target.read_bytes()
                    assert text(u.GetDlgItem(main,101))=="parent unsaved text" and text(main)==caption
                    assert source.read_bytes()==br"{\rtf1\ansi Parent source}"
                if app=="PDF42":
                    u.PostMessageW(main,WM_CLOSE,0,0); process.wait(timeout=5)
                    assert child.poll() is None and u.IsWindowVisible(window)
                    command(window,6)
                    about=modal(child,native_class="#32770"); click(about,1)
                    wait_for(lambda: u.IsWindowEnabled(window),"child still works after parent close",child)
                u.PostMessageW(window,WM_CLOSE,0,0); child.wait(timeout=5); child.close_handle(); owned.remove(child)
                if app!="PDF42": assert process.poll() is None and text(main)==caption
            result["checks"].append(app + ": New/New Window opens an independent native session, inherits language and leaves the parent open")
        finally:
            for pid in child_processes(process.pid,binary.name):
                if all(child.pid!=pid for child in owned): owned.append(OwnedChildProcess(pid))
            for child in owned:
                if child.poll() is None: child.terminate()
                child.close_handle()
            if process.poll() is None: process.terminate(); process.wait(timeout=5)
    result["checks"].append("Closing the PDF parent leaves its child usable; closing other children leaves their parents usable")
    result["checks"].append("Dirty DOC parent text/source stay intact while both New commands create editable child documents with independent Undo/Redo/save/close")
    return result


def audit_window_layout(bin_dir,output):
    class Placement(c.Structure):
        _fields_=[("length",w.UINT),("flags",w.UINT),("showCmd",w.UINT),("ptMinPosition",w.POINT),("ptMaxPosition",w.POINT),("rcNormalPosition",w.RECT)]
    u.GetWindowPlacement.argtypes=[w.HWND,c.POINTER(Placement)]
    u.SetWindowPos.argtypes=[w.HWND,w.HWND,c.c_int,c.c_int,c.c_int,c.c_int,w.UINT]
    u.ShowWindow.argtypes=[w.HWND,c.c_int]
    u.IsZoomed.argtypes=[w.HWND]
    u.MonitorFromWindow.argtypes,u.MonitorFromWindow.restype=[w.HWND,w.DWORD],w.HANDLE
    def placement(hwnd):
        value=Placement();value.length=c.sizeof(value)
        assert u.GetWindowPlacement(hwnd,c.byref(value))
        r=value.rcNormalPosition
        return [r.left,r.top,r.right,r.bottom]
    root=output/"window-layout-settings";root.mkdir(exist_ok=True)
    environment=os.environ.copy();environment["LOCALAPPDATA"]=str(root)
    result={"scenario":"native window placement", "checks":[]}
    for app in ["DOC42","XLS42","PDF42","IMG42","PPT42","SCN42"]:
        process=None
        def launch():
            process=subprocess.Popen([str(bin_dir/(app+".exe")),"--language","en"],env=environment)
            return process,modal(process,native_class="Suite42Native")
        preferences=root/"off42"/"Window Layout"/(app+".json")
        try:
            process,main=launch()
            assert u.SetWindowPos(main,None,70,80,800,560,0x14)
            expected=placement(main)
            assert expected[2]-expected[0]==800 and expected[3]-expected[1]==560
            def body_rect():
                children=windows(process.pid,main)
                body=next((h for h in children if u.IsWindowVisible(h)and u.GetDlgCtrlID(h)==102),None)or next((h for h in children if u.IsWindowVisible(h)and u.GetDlgCtrlID(h)==101),None)or next(h for h in children if u.IsWindowVisible(h)and'canvas'in classname(h).lower())
                value=w.RECT();assert u.GetWindowRect(body,c.byref(value));return(value.left,value.top,value.right-value.left,value.bottom-value.top)
            def hidden_panels():
                children=windows(process.pid,main)
                return not any(u.IsWindowVisible(h)for h in children if classname(h).lower()in{'button','static'})
            caption=text(main);history=[u.GetMenuState(u.GetMenu(main),i,0)for i in(4,5)];before=body_rect()
            toggles=[i for i in(65503,65502,65501,65500)if u.GetMenuState(u.GetMenu(main),i,0)&3==0]
            assert 65501 in toggles
            if 65502 not in toggles:
                assert not any(u.IsWindowVisible(h)for h in windows(process.pid,main)if classname(h).lower()=='button'),app+' disabled a populated toolbar'
            names={65503:'sidebar',65502:'toolbar',65501:'status',65500:'rulers'}
            for identifier in toggles:
                assert u.GetMenuState(u.GetMenu(main),identifier,0)&8
                command(main,identifier);wait_for(lambda:u.GetMenuState(u.GetMenu(main),identifier,0)&8==0,'panel hidden '+str(identifier)+' '+app,process)
            wait_for(hidden_panels,'toolbar/status hidden '+app,process)
            after=body_rect();assert after[0]<=before[0]and after[1]<=before[1]and after[2]>=before[2]and after[3]>=before[3],(app,before,after)
            assert text(main)==caption and[u.GetMenuState(u.GetMenu(main),i,0)for i in(4,5)]==history
            stored=json.loads(preferences.read_text(encoding='utf-8'));assert all(stored['panels'][names[i]]is False for i in toggles)
            if app in{'DOC42','XLS42'}:capture_window(main,output/(app+'-panels-hidden.bmp'))
            u.PostMessageW(main,WM_CLOSE,0,0);process.wait(timeout=5)
            process,main=launch();wait_for(hidden_panels,'panel preferences restored '+app,process)
            assert all(u.GetMenuState(u.GetMenu(main),i,0)&8==0 for i in toggles)
            for identifier in toggles:
                command(main,identifier);wait_for(lambda:u.GetMenuState(u.GetMenu(main),identifier,0)&8,'panel restored '+str(identifier)+' '+app,process)
            wait_for(lambda:body_rect()==before,'exact document area restored '+app,process)
            assert text(main)==caption and[u.GetMenuState(u.GetMenu(main),i,0)for i in(4,5)]==history
            result['checks'].append(app+': toolbar/status and available sidebar/rulers toggle through View, reclaim document area, preserve caption/history, persist across relaunch and restore exact geometry')
            u.PostMessageW(main,WM_CLOSE,0,0);process.wait(timeout=5)
            saved=json.loads(preferences.read_text(encoding="utf-8"))
            assert saved=={"version":1,"normal":expected,"maximized":False}
            process,main=launch()
            wait_for(lambda: placement(main)==expected and not u.IsZoomed(main),"restored normal frame "+app,process)
            u.ShowWindow(main,3)
            wait_for(lambda: u.IsZoomed(main),"maximized frame "+app,process)
            u.PostMessageW(main,WM_CLOSE,0,0);process.wait(timeout=5)
            saved=json.loads(preferences.read_text(encoding="utf-8"))
            assert saved["maximized"] and saved["normal"]==expected
            process,main=launch()
            wait_for(lambda: u.IsZoomed(main),"restored maximized state "+app,process)
            u.ShowWindow(main,9)
            wait_for(lambda: placement(main)==expected and not u.IsZoomed(main),"maximized window restores original normal bounds "+app,process)
            u.PostMessageW(main,WM_CLOSE,0,0);process.wait(timeout=5)
            preferences.write_text(json.dumps({"version":1,"normal":[100000,100000,100800,100560],"maximized":False}),encoding="utf-8")
            process,main=launch()
            wait_for(lambda: bool(u.MonitorFromWindow(main,0)),"offscreen saved position relocated to a current monitor "+app,process)
            assert placement(main)!=[100000,100000,100800,100560]
            u.PostMessageW(main,WM_CLOSE,0,0);process.wait(timeout=5)
            result["checks"].append(app+": normal bounds, maximize/restore and offscreen recovery survive real process restarts")
        finally:
            if process and process.poll() is None:process.terminate();process.wait(timeout=5)
    return result


def audit_language_picker(bin_dir, output):
    root = output / "language-picker-settings"
    root.mkdir(exist_ok=True)
    environment = os.environ.copy()
    environment["LOCALAPPDATA"] = str(root)
    source = output / ("language-picker-" + uuid.uuid4().hex + ".rtf")
    source.write_bytes(br"{\rtf1\ansi Language seed}")
    process = subprocess.Popen([str(bin_dir / "DOC42.exe"), "--language", "en", "--", str(source)], env=environment)
    child = None
    result = {"scenario": "shared language picker", "checks": []}
    try:
        main = modal(process, native_class="Suite42Native")
        wait_for(lambda: source.name in text(main), "language source opened", process)
        set_text(u.GetDlgItem(main, 101), "unsaved own text")
        wait_for(lambda: " * - DOC42" in text(main), "language source dirty", process)
        preferences = root / "off42" / "Preferences" / "language.json"
        command(main, 64008)  # Native Russian entry in View > Language.
        confirmation = modal(process, native_class="#32770")
        click(confirmation, 2)
        wait_for(lambda: u.IsWindowEnabled(main), "language switch cancelled", process)
        assert not preferences.exists() and text(u.GetDlgItem(main, 101)) == "unsaved own text"
        result["checks"].append("Cancel retains unsaved text, original process and original shared language")
        command(main, 64008)
        confirmation = modal(process, native_class="#32770")
        click(confirmation, 7)
        # The app deliberately starts a new process. Identify it by the unique test-owned filename.
        until = time.monotonic() + 25
        replacement = []
        @c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
        def collect(hwnd, _):
            if u.IsWindowVisible(hwnd) and classname(hwnd) == "Suite42Native" and source.name in text(hwnd):
                pid = w.DWORD()
                u.GetWindowThreadProcessId(hwnd, c.byref(pid))
                if pid.value != process.pid:
                    replacement.append((hwnd, pid.value))
            return True
        while time.monotonic() < until and not replacement:
            u.EnumWindows(collect, 0)
            time.sleep(0.05)
        assert len(replacement) == 1, "Language switch did not reopen the saved test document"
        process.wait(timeout=5)
        child = OwnedChildProcess(replacement[0][1])
        main = replacement[0][0]
        wait_for(lambda: text(u.GetDlgItem(main, 101)) == "Language seed", "replacement process opens saved document", child)
        assert json.loads(preferences.read_text(encoding="utf-8")) == {"version": 1, "language": "ru"}
        assert u.GetMenuState(u.GetMenu(main), 64008, 0) & 8
        result["checks"].append("Discard restarts into selected language and reopens unchanged saved source")
        u.PostMessageW(main, WM_CLOSE, 0, 0)
        child.wait(timeout=5)
        child.close_handle()
        child = None
        for app in ["DOC42", "XLS42", "PDF42", "IMG42", "PPT42", "SCN42"]:
            next_process = subprocess.Popen([str(bin_dir / (app + ".exe"))], env=environment)
            try:
                main = modal(next_process, native_class="Suite42Native")
                wait_for(lambda: bool(u.GetMenuState(u.GetMenu(main), 64008, 0) & 8), "shared language read on next app launch", next_process)
                label = c.create_unicode_buffer(256)
                assert u.GetMenuStringW(u.GetMenu(main), 0, label, len(label), 0x400)
                assert label.value.replace("&", "") == "Файл", (app, label.value)
                u.PostMessageW(main, WM_CLOSE, 0, 0)
                next_process.wait(timeout=5)
            finally:
                if next_process.poll() is None:
                    next_process.terminate()
                    next_process.wait(timeout=5)
        result["checks"].append("all six apps read the shared saved language on their next launch")
        assert source.read_bytes() == br"{\rtf1\ansi Language seed}"
        return result
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
        if child is not None:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=5)
            child.close_handle()


def audit_virtual_rescan(binary,output):
    """Exercise STA/WIC acquisition and the real rescan UI with an explicit virtual source, not hardware."""
    root=output/'virtual-rescan-settings';root.mkdir(exist_ok=True)
    fixture=output/'virtual-rescan.png';width,height=40,20
    pixels=b''.join(bytes([255,220,0,255]if x<20 else[140,20,220,255])for y in range(height)for x in range(width))
    write_png(fixture,width,height,pixels)
    density=struct.pack('>IIB',round(300/0.0254),round(600/0.0254),1)
    chunk=struct.pack('>I',9)+b'pHYs'+density+struct.pack('>I',zlib.crc32(b'pHYs'+density)&0xffffffff)
    data=fixture.read_bytes();fixture.write_bytes(data[:33]+chunk+data[33:])
    source=output/'virtual-rescan-before.scn42';data=bytearray(b'SCN42\0\x04\0'+struct.pack('<I',3))
    for index,color in enumerate([[255,0,0,255],[0,160,40,255],[0,0,255,255]]):
        data.extend(struct.pack('<QIIII',index+1,32,24,150+index*50,200+index*50));data.extend(bytes(color)*(32*24))
        language=b'eng'if index==1 else b'';data.extend(struct.pack('<I',len(language)));data.extend(language)
        data.extend(struct.pack('<I',1 if index==1 else 0))
        if index==1:data.extend(struct.pack('<IIIII',1,1,10,5,3)+b'OLD')
    source.write_bytes(data);original=bytes(data)
    def pages(data):
        assert data[:8]==b'SCN42\0\x04\0';count=struct.unpack_from('<I',data,8)[0];position=12;result=[]
        for _ in range(count):
            identity,wid,hei,dx,dy=struct.unpack_from('<QIIII',data,position);position+=24
            samples=data[position:position+wid*hei*4];assert len(samples)==wid*hei*4;position+=len(samples)
            length=struct.unpack_from('<I',data,position)[0];position+=4;language=data[position:position+length];position+=length
            words=[];word_count=struct.unpack_from('<I',data,position)[0];position+=4
            for _ in range(word_count):
                x,y,wid_word,hei_word,length=struct.unpack_from('<IIIII',data,position);position+=20
                words.append((x,y,wid_word,hei_word,data[position:position+length]));position+=length
            result.append({'id':identity,'size':(wid,hei),'dpi':(dx,dy),'pixels':samples,'language':language,'words':words})
        assert position==len(data);return result
    before=pages(original);release=output/'virtual-rescan-release';release.unlink(missing_ok=True)
    environment=os.environ.copy();environment.update({'LOCALAPPDATA':str(root),'OFF42_AUDIT_OUTPUT':str(output),'OFF42_WIA_AUDIT_IMAGE':str(fixture),'OFF42_WIA_AUDIT_RELEASE':str(release)})
    process=subprocess.Popen([str(binary),'--language','en','--',str(source)],env=environment)
    try:
        main=modal(process,native_class='Suite42Native');wait_for(lambda:source.name in text(main)and u.IsWindowEnabled(main),'virtual rescan source opened',process)
        def select(index):
            u.SendMessageW(u.GetDlgItem(main,100),0x186,index,0);command(main,100|(1<<16))
        def status(part):return any(part in text(h)for h in windows(process.pid,main))
        def signal(mode):
            temporary=release.with_suffix('.tmp');temporary.write_text(mode,encoding='ascii');os.replace(temporary,release)
        def saved(name):
            path=output/name;save_as(main);choose_file(process,path);output_ready(process,main,path,b'SCN42\0\x04\0');return path.read_bytes()
        assert saved('virtual-rescan-loaded.scn42')==original,'Opening the fixture lost its original OCR or page metadata'
        select(1);wait_for(lambda:u.SendMessageW(u.GetDlgItem(main,100),0x188,0,0)==1,'rescan target selected',process)
        command(main,39);wait_for(lambda:u.GetMenuState(u.GetMenu(main),39,0)&3,'rescan disables duplicate acquisition',process)
        assert' * - 'not in text(main)and source.read_bytes()==original
        select(0);signal('acquire');wait_for(lambda:u.GetMenuState(u.GetMenu(main),39,0)&3==0 and' * - 'in text(main),'virtual WIC acquisition committed',process)
        after_bytes=saved('virtual-rescan-after.scn42');after=pages(after_bytes)
        assert len(after)==3 and after[0]==before[0]and after[2]==before[2]
        assert after[1]['id']!=before[1]['id']and after[1]['size']==(width,height)and after[1]['pixels']==pixels and after[1]['dpi']==(300,600)
        assert not after[1]['words']and not after[1]['language']
        capture_window(main,output/'SCN42-virtual-rescan.bmp')
        command(main,4);assert saved('virtual-rescan-undo.scn42')==original
        command(main,5);assert saved('virtual-rescan-redo.scn42')==after_bytes
        select(1);release.unlink();command(main,39);wait_for(lambda:u.GetMenuState(u.GetMenu(main),39,0)&3,'virtual canceled rescan pending',process)
        signal('cancel');wait_for(lambda:status('Scan cancelled or no pages acquired')and u.GetMenuState(u.GetMenu(main),39,0)&3==0,'virtual cancellation processed',process)
        assert' * - 'not in text(main)and saved('virtual-rescan-canceled.scn42')==after_bytes
        release.unlink();command(main,39);wait_for(lambda:u.GetMenuState(u.GetMenu(main),39,0)&3,'late virtual scan pending',process)
        open_fixture(process,main,output/'virtual-rescan-after.scn42');signal('acquire')
        wait_for(lambda:status('Scan result discarded because the project changed'),'late virtual acquisition discarded after reopen',process)
        assert' * - 'not in text(main)and saved('virtual-rescan-late.scn42')==after_bytes and source.read_bytes()==original
        return{'app':'SCN42','scenario':'virtual WIA rescan','sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'checks':['explicit virtual source through Windows STA/WIC, physical scanner not exercised; captured target survives selection change; exact replacement pixels/DPI and new identity; other pages preserved; old OCR cleared; one-step Undo/Redo restores every original byte; virtual Cancel and late acquisition after reopening preserve model/source; saves and reopen use normal dialogs']}
    finally:
        if process.poll()is None:
            process.terminate();process.wait(timeout=5)


def audit_recovery(binary,output):
    """Kill only the isolated audit process and recover through the real file/menu UI."""
    app=binary.stem;root=output/('recovery-settings-'+app);root.mkdir(exist_ok=True)
    environment=os.environ.copy();environment['LOCALAPPDATA']=str(root);environment['OFF42_RECOVERY_INTERVAL_MS']='1000'
    marker='RECOVERED-'+app;extension={'DOC42':'doc42','XLS42':'xls42','PDF42':'pdf','PPT42':'json','SCN42':'scn42','IMG42':'png'}[app]
    source=None;process=None;child=None
    if app=='DOC42':
        source=output/'recovery-original.rtf';source.write_bytes(br'{\rtf1\ansi ORIGINAL-DOC}')
    elif app=='XLS42':
        source=output/'recovery-original.csv';source.write_text('ORIGINAL-XLS,SECOND\n',encoding='utf-8')
    elif app=='PDF42':
        source=output/'recovery-original.pdf';minimal_pdf(source,['ORIGINAL-PDF'])
    elif app in{'SCN42','IMG42'}:
        source=output/'recovery-original.png';rgba=b''.join(bytes([255,0,0,255]if x<32 else[0,0,255,255])for y in range(32)for x in range(64));write_png(source,64,32,rgba)
    def launch(path=None):
        process=subprocess.Popen([str(binary),'--language','en']+(['--',str(path)]if path else[]),env=environment)
        main=wait_for(lambda:next((h for h in windows(process.pid)if classname(h)=='Suite42Native'),None),'recovery main window (possibly behind startup dialog)',process)
        if path:
            if app=='SCN42':wait_for(lambda:any('1 pages |'in text(h)for h in windows(process.pid,main)),'scan recovery fixture imported',process)
            else:wait_for(lambda:path.name in text(main)and u.IsWindowEnabled(main),'recovery source opened',process)
        return process,main
    def check_latest(process,main):
        if app in {'DOC42','XLS42'}:assert marker in text(u.GetDlgItem(main,101))
        elif app=='PPT42':
            command(main,24);dialog=modal(process,title='Edit slide text',native_class='Suite42InputDialog')
            assert text(u.GetDlgItem(dialog,100))==marker and text(u.GetDlgItem(dialog,101))=='LATEST'
            click(dialog,2);wait_for(lambda:u.IsWindowEnabled(main),'recovered slide text inspected',process)
    def records():return list((root/'off42'/'Recovery'/app).glob('*/document.off42r'))
    def complete_record():
        for path in records():
            try:
                data=path.read_bytes()
                if data[:8]!=b'OFF42RC1'or len(data)<12:continue
                size=struct.unpack_from('<I',data,8)[0];header=json.loads(data[12:12+size]);payload=data[12+size:]
                if len(payload)==header['length']and hashlib.sha256(payload).hexdigest()==header['sha256']:return path
            except(OSError,ValueError,KeyError):pass
        return None
    try:
        process,main=launch(source)
        if app in {'PPT42','SCN42'}:
            source=output/('recovery-original-'+app+'.'+extension);save_as(main);choose_file(process,source);output_ready(process,main,source,b'SCN42\0\x04\0'if app=='SCN42'else b'{')
            wait_for(lambda:source.name in text(main)and' * - 'not in text(main),'baseline saved before crash',process)
        original=source.read_bytes()
        if app in {'PDF42','SCN42'}:u.SendMessageW(main,WM_COMMAND,10,0)
        elif app=='IMG42':u.SendMessageW(main,WM_COMMAND,11,0)
        elif app=='PPT42':fill_prompt(process,main,24,'Edit slide text',[marker,'LATEST'])
        else:replace_editor_text(u.GetDlgItem(main,101),marker)
        wait_for(lambda:' * - 'in text(main),'document edited before backup',process)
        backup=wait_for(complete_record,'atomic recovery backup written',process,timeout=15)
        assert source.read_bytes()==original,'Backup changed the source'
        child,child_main=launch();time.sleep(0.25)
        assert u.IsWindowEnabled(child_main)and not any(classname(h)=='Suite42InputDialog'for h in windows(child.pid)),'Live session offered for recovery'
        u.PostMessageW(child_main,WM_CLOSE,0,0);child.wait(timeout=10);child=None
        process.terminate();process.wait(timeout=5);assert backup.is_file()
        if app=='DOC42':source.write_bytes(br'{\rtf1\ansi EXTERNAL-CHANGE}');original=source.read_bytes()
        process,main=launch();picker=modal(process,title='Recover Unsaved Document',native_class='Suite42InputDialog')
        selected=text(u.GetDlgItem(picker,100));assert source.name in selected
        click(picker,2);wait_for(lambda:u.IsWindowEnabled(main),'recovery Cancel',process);assert backup.is_file()
        command(main,65505);finish_prompt(process,'Recover Unsaved Document',[selected,'Recover'])
        wait_for(lambda:text(main).startswith('Untitled *')and u.IsWindowEnabled(main),'recovered independent dirty copy',process)
        assert source.read_bytes()==original
        check_latest(process,main)
        capture_window(main,output/(app+'-recovered.bmp'))
        u.PostMessageW(main,WM_CLOSE,0,0)
        confirmation=wait_for(lambda:next((h for h in windows(process.pid)if classname(h)=='#32770'and all(u.GetDlgItem(h,i)for i in(2,6,7))),None),'close recovered copy asks to save',process)
        click(confirmation,2);wait_for(lambda:u.IsWindowEnabled(main),'recovered close Cancel',process)
        command(main,2);dialog=modal(process,title='Save As',native_class='#32770');click(dialog,2)
        wait_for(lambda:u.IsWindowEnabled(main)and' * - 'in text(main),'recovered Save Cancel stays dirty',process)
        saved=output/('recovery-saved-'+app+'.'+extension);command(main,2);choose_file(process,saved);output_ready(process,main,saved,b'%PDF-'if app=='PDF42'else(b'SCN42\0\x04\0'if app=='SCN42'else(b'\x89PNG\r\n\x1a\n'if app=='IMG42'else b'{')))
        wait_for(lambda:saved.name in text(main)and' * - 'not in text(main),'recovery saved at the chosen destination',process)
        if app in {'DOC42','XLS42','PPT42'}:assert marker in saved.read_text(encoding='utf-8')
        elif app=='PDF42':
            pdf=c.WinDLL(str(binary.parent/'pdfium.dll'));pdf.FPDF_LoadDocument.argtypes,pdf.FPDF_LoadDocument.restype=[c.c_char_p,c.c_char_p],c.c_void_p
            pdf.FPDF_LoadPage.argtypes,pdf.FPDF_LoadPage.restype=[c.c_void_p,c.c_int],c.c_void_p;pdf.FPDFPage_GetRotation.argtypes=[c.c_void_p];pdf.FPDF_ClosePage.argtypes=[c.c_void_p];pdf.FPDF_CloseDocument.argtypes=[c.c_void_p]
            pdf.FPDF_InitLibrary();document=pdf.FPDF_LoadDocument(str(saved).encode(),None);assert document;page=pdf.FPDF_LoadPage(document,0);assert page
            try:assert pdf.FPDFPage_GetRotation(page)==1,'Recovered rotation lost'
            finally:pdf.FPDF_ClosePage(page);pdf.FPDF_CloseDocument(document)
        elif app in{'SCN42','IMG42'}:
            if app=='IMG42':png=saved
            else:
                png=output/'recovery-scan-rotated.png';command(main,31);choose_file(process,png);output_ready(process,main,png,b'\x89PNG\r\n\x1a\n')
            width,height,pixels=png_pixels(png);expected=b''.join(rgba[((31-x)*64+y)*4:((31-x)*64+y)*4+4]for y in range(64)for x in range(32))
            assert(width,height,pixels)==(32,64,expected),'Recovered scan pixels lost'
        wait_for(lambda:not records(),'saved backups removed',process);assert source.read_bytes()==original
        u.PostMessageW(main,WM_CLOSE,0,0);process.wait(timeout=10)
        process,main=launch(saved);assert' * - 'not in text(main)and not records()
        check_latest(process,main)
        u.PostMessageW(main,WM_CLOSE,0,0);process.wait(timeout=10)
        if app=='DOC42':
            saved_bytes=saved.read_bytes();process,main=launch(saved)
            replace_editor_text(u.GetDlgItem(main,101),marker+' SECOND')
            damaged=wait_for(complete_record,'second recovery copy',process,timeout=15)
            process.terminate();process.wait(timeout=5)
            payload=bytearray(damaged.read_bytes());payload[-1]^=1;damaged.write_bytes(payload)
            process,main=launch();picker=modal(process,title='Recover Unsaved Document',native_class='Suite42InputDialog')
            selected=text(u.GetDlgItem(picker,100));finish_prompt(process,'Recover Unsaved Document',[selected,'Delete Recovery Copy'])
            wait_for(lambda:not records()and u.IsWindowEnabled(main),'damaged recovery copy deleted',process)
            assert saved.read_bytes()==saved_bytes and source.read_bytes()==original
            u.PostMessageW(main,WM_CLOSE,0,0);process.wait(timeout=10)
        checks=['normal backup path with a one-second audit interval, active-window exclusion, forced process loss, startup/menu recovery, Cancel, independent dirty copy, close/Save Cancel, latest persisted data, selected destination, cleanup and reopen preserve the original']
        if app=='DOC42':checks.append('damaged recovery copy can be deleted without loading it or modifying the saved document/source')
        return{'app':app,'scenario':'crash recovery','checks':checks}
    finally:
        for target in(process,child):
            if target and target.poll()is None:target.terminate();target.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--extended", action="store_true")
    parser.add_argument("--pictures", action="store_true")
    parser.add_argument("--technical-objects", action="store_true")
    parser.add_argument("--label-sources", action="store_true")
    parser.add_argument("--languages", action="store_true")
    parser.add_argument("--recents", action="store_true")
    parser.add_argument("--multiwindow", action="store_true")
    parser.add_argument("--window-layout", action="store_true")
    parser.add_argument("--rich-search", action="store_true")
    parser.add_argument("--preset-shapes", action="store_true")
    parser.add_argument("--pdf-decorations", action="store_true")
    parser.add_argument("--pdf-comparison", action="store_true")
    parser.add_argument("--pdf-merge", action="store_true")
    parser.add_argument("--pdf-batch-headers", action="store_true")
    parser.add_argument("--statistical-formulas", action="store_true")
    parser.add_argument("--array-formulas", action="store_true")
    parser.add_argument("--ocr", action="store_true")
    parser.add_argument("--ocr-only", action="store_true")
    parser.add_argument("--label-artwork", action="store_true")
    parser.add_argument("--label-preview", action="store_true")
    parser.add_argument("--recovery",action="store_true")
    parser.add_argument("--native-print",action="store_true")
    parser.add_argument("--page-exports", action="store_true")
    parser.add_argument("--host-commands", action="store_true")
    parser.add_argument("--engineering", action="store_true")
    parser.add_argument("--text-exports", action="store_true")
    parser.add_argument("--extended-formulas", "--text-functions", dest="extended_formulas", action="store_true")
    args = parser.parse_args()
    args.bin_dir = args.bin_dir.resolve()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    results = []
    try:
        for app in (["PDF42","IMG42","SCN42"] if args.ocr_only else ["DOC42", "XLS42", "PDF42", "IMG42", "PPT42", "SCN42"]):
            try:
                results.append(audit(args.bin_dir / (app + ".exe"), args.output, args.extended, args.pictures, args.technical_objects, args.label_sources, args.page_exports, args.host_commands, args.engineering, args.text_exports, args.extended_formulas, args.rich_search, args.preset_shapes, args.pdf_decorations, args.statistical_formulas, args.pdf_comparison, args.pdf_merge, args.pdf_batch_headers, args.array_formulas, args.ocr, args.ocr_only, args.label_artwork, args.label_preview,args.native_print))
            except Exception as error:
                results.append({"app": app, "error": str(error), "traceback": traceback.format_exc()})
            print(json.dumps(results[-1]), flush=True)
    finally:
        (args.output / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    if args.recovery:
        for app in['DOC42','XLS42','PDF42','PPT42','SCN42','IMG42']:
            try:results.append(audit_recovery(args.bin_dir/(app+'.exe'),args.output))
            except Exception as error:results.append({'app':app,'scenario':'crash recovery','error':str(error),'traceback':traceback.format_exc()})
            print(json.dumps(results[-1]),flush=True)
            (args.output/'results.json').write_text(json.dumps(results,indent=2),encoding='utf-8')
        try:results.append(audit_virtual_rescan(args.bin_dir/'SCN42.exe',args.output))
        except Exception as error:results.append({'app':'SCN42','scenario':'virtual WIA rescan','error':str(error),'traceback':traceback.format_exc()})
        print(json.dumps(results[-1]),flush=True)
        (args.output/'results.json').write_text(json.dumps(results,indent=2),encoding='utf-8')
    if args.languages:
        languages = ["en", "zh-Hans", "zh-Hant", "es", "ar", "fr", "pt", "id", "ru", "de", "ja", "vi", "tr", "fil", "ko", "th", "it", "pl", "ms", "hu"]
        for index, code in enumerate(languages):
            try:
                results.append(audit_language(args.bin_dir / "DOC42.exe", args.output, code, index))
            except Exception as error:
                results.append({"language": code, "error": str(error), "traceback": traceback.format_exc()})
            print(json.dumps(results[-1]), flush=True)
        (args.output / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    if args.languages:
        try:
            results.append(audit_language_picker(args.bin_dir, args.output))
        except Exception as error:
            results.append({"scenario": "shared language picker", "error": str(error), "traceback": traceback.format_exc()})
        print(json.dumps(results[-1]), flush=True)
        (args.output / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    if args.recents:
        try:
            results.append(audit_recents(args.bin_dir / "DOC42.exe", args.output))
        except Exception as error:
            results.append({"scenario": "recent documents", "error": str(error), "traceback": traceback.format_exc()})
        print(json.dumps(results[-1]), flush=True)
        (args.output / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    if args.multiwindow:
        try:
            results.append(audit_multiple_windows(args.bin_dir,args.output))
        except Exception as error:
            results.append({"scenario":"independent document windows", "error":str(error), "traceback":traceback.format_exc()})
        print(json.dumps(results[-1]),flush=True)
        (args.output / "results.json").write_text(json.dumps(results,indent=2),encoding="utf-8")
    if args.window_layout:
        try:
            results.append(audit_window_layout(args.bin_dir,args.output))
        except Exception as error:
            results.append({"scenario":"native window placement", "error":str(error), "traceback":traceback.format_exc()})
        print(json.dumps(results[-1]),flush=True)
        (args.output/"results.json").write_text(json.dumps(results,indent=2),encoding="utf-8")
    assert all("error" not in result for result in results), results


if __name__ == "__main__":
    main()
