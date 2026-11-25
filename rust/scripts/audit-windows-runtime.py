#!/usr/bin/env python3
"""Inspect packaged PE imports without executing binaries; only OS DLLs/PDFium are allowed."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct

APPS = ["PDF42", "DOC42", "XLS42", "IMG42", "PPT42", "SCN42"]
SYSTEM = set("ADVAPI32 BCRYPT BCRYPTPRIMITIVES CFGMGR32 COMBASE COMCTL32 COMDLG32 CRYPT32 CRYPTBASE CRYPTSP CRYPTUI D2D1 D3D11 DBGHELP DWRITE DXGI DWMAPI GDI32 GDI32FULL IMM32 IPHLPAPI KERNEL32 KERNELBASE MSIMG32 MSVCRT NCRYPT NORMALIZ NTDLL OLE32 OLEAUT32 POWRPROF PROPSYS RPCRT4 RUNTIMEOBJECT SECUR32 SETUPAPI SHCORE SHELL32 SHLWAPI USER32 USERENV UCRTBASE UXTHEME VERSION WINDOWSCODECS WINDOWSAPP WINHTTP WININET WINMM WINTRUST WLDAP32 WS2_32 WTSAPI32".split())


def imports(blob):
    def unpack(fmt, offset):
        size = struct.calcsize(fmt)
        if not 0 <= offset <= len(blob) - size:
            raise ValueError("Truncated PE structure")
        return struct.unpack_from(fmt, blob, offset)
    if blob[:2] != b"MZ":
        raise ValueError("Not a PE executable")
    pe, = unpack("<I", 0x3C)
    if blob[pe:pe + 4] != b"PE\0\0":
        raise ValueError("Invalid PE signature")
    machine, count, _, _, _, optional_size, _ = unpack("<HHIIIHH", pe + 4)
    optional = pe + 24
    magic, = unpack("<H", optional)
    if machine != 0x8664 or magic != 0x20B or not 1 <= count <= 96 or optional_size < 128:
        raise ValueError("Expected a bounded x64 PE32+ image")
    image_base, = unpack("<Q", optional + 24)
    directory_count, = unpack("<I", optional + 108)
    if 112 + directory_count * 8 > optional_size:
        raise ValueError("Invalid PE directory table")
    sections = []
    for index in range(count):
        position = optional + optional_size + index * 40
        virtual_size, address, raw_size, raw = unpack("<IIII", position + 8)
        if raw + raw_size > len(blob):
            raise ValueError("Invalid PE section extent")
        sections.append((address, max(virtual_size, raw_size), raw, raw_size))
    def offset(rva, size):
        for address, virtual, raw, available in sections:
            distance = rva - address
            if 0 <= distance < virtual and distance + size <= available:
                return raw + distance
        raise ValueError("PE import points outside file data")
    def name(rva):
        position = offset(rva, 1)
        end = blob.find(b"\0", position, min(len(blob), position + 261))
        if end < 0:
            raise ValueError("Unterminated PE dependency name")
        value = blob[position:end].decode("ascii").upper()
        if not re.fullmatch(r"[A-Z0-9_.-]+\.DLL", value):
            raise ValueError("Invalid PE dependency name")
        return value
    dependencies = set()
    for directory, width in [(1, 20), (13, 32)]:
        if directory >= directory_count:
            continue
        rva, size = unpack("<II", optional + 112 + directory * 8)
        if not rva and not size:
            continue
        if not rva or size < width or size > width * 1024:
            raise ValueError("Invalid PE import directory")
        terminated = False
        for index in range(min(size // width, 1024)):
            fields = unpack("<" + "I" * (width // 4), offset(rva + index * width, width))
            if not any(fields):
                terminated = True
                break
            if directory == 1:
                dependency = fields[3]
            else:
                dependency = fields[1]
                if fields[0] == 0:
                    dependency -= image_base
                elif fields[0] != 1:
                    raise ValueError("Unsupported delayed-import flags")
            dependencies.add(name(dependency))
        if not terminated:
            raise ValueError("Unterminated PE import directory")
    return sorted(dependencies)


def allowed(name):
    stem = name.removesuffix(".DLL")
    if stem.startswith(("VCRUNTIME", "MSVCP", "CONCRT")):
        return False
    return stem in SYSTEM or stem == "PDFIUM" or stem.startswith(("API-MS-WIN-", "EXT-MS-WIN-"))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--report-only", action="store_true")
    args = parser.parse_args()
    results = []
    failed = False
    for filename in [*(app + ".exe" for app in APPS), "pdfium.dll"]:
        path = args.bin_dir / filename
        result = {"file": filename}
        try:
            if not path.is_file() or path.stat().st_size > 512 * 1024 * 1024:
                raise ValueError("Missing or oversized packaged binary")
            blob = path.read_bytes()
            dependencies = imports(blob)
            result.update(sha256=hashlib.sha256(blob).hexdigest(), size=len(blob), imports=dependencies)
            unexpected = [name for name in dependencies if not allowed(name)]
            if unexpected:
                result["error"] = "Unpackaged non-system runtime dependencies: " + ", ".join(unexpected)
        except (ValueError, UnicodeError, OSError, struct.error) as error:
            result["error"] = str(error)
        failed |= "error" in result
        results.append(result)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    for result in results:
        print(result["file"] + ": " + result.get("error", "OS-only imports; bundled PDFium permitted"))
    if failed and not args.report_only:
        raise SystemExit(1)

if __name__ == "__main__":
    main()
