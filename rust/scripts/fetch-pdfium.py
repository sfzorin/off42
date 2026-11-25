#!/usr/bin/env python3
"""Provision a pinned PDFium Windows x64 runtime and its bundled licenses.

PDFIUM_ARCHIVE can point to the pinned archive instead of downloading it.
PDFIUM_DLL + PDFIUM_DLL_SHA256 + PDFIUM_LICENSE_DIR select a verified custom DLL.
"""
import hashlib
import os
from pathlib import Path
import shutil
import struct
import sys
import tarfile
import tempfile
import urllib.request

VERSION = "chromium/8125"
URL = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8125/pdfium-win-x64.tgz"
ARCHIVE_SHA256 = "9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b"
DLL_SHA256 = "4b0dec92555d69437bef0df37be630181ab3e83a6823ca264ede355a4f2ef994"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def check_dll(data):
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("PDFium DLL is not a PE executable")
    offset = struct.unpack_from("<I", data, 60)[0]
    if offset + 6 > len(data) or data[offset:offset + 4] != b"PE\0\0" or struct.unpack_from("<H", data, offset + 4)[0] != 0x8664:
        raise ValueError("PDFium DLL must be Windows x64")


def main():
    if len(sys.argv) != 2:
        raise ValueError("Usage: fetch-pdfium.py OUTPUT_DIRECTORY")
    output = Path(sys.argv[1])
    output.mkdir(parents=True, exist_ok=True)
    licenses = output / "licenses" / "pdfium"
    licenses.mkdir(parents=True, exist_ok=True)
    custom = os.environ.get("PDFIUM_DLL")
    if custom:
        expected = os.environ.get("PDFIUM_DLL_SHA256", "").lower()
        license_path = os.environ.get("PDFIUM_LICENSE_DIR")
        license_dir = Path(license_path) if license_path else None
        if not license_dir or len(expected) != 64 or not all(c in "0123456789abcdef" for c in expected) or not license_dir.is_dir():
            raise ValueError("Custom PDFIUM_DLL requires PDFIUM_DLL_SHA256 and PDFIUM_LICENSE_DIR")
        data = Path(custom).read_bytes()
        if sha(data) != expected:
            raise ValueError("Custom PDFium DLL SHA-256 mismatch")
        files = [p for p in license_dir.iterdir() if p.is_file()]
        if not files:
            raise ValueError("PDFIUM_LICENSE_DIR must contain license files")
        for p in files:
            shutil.copyfile(p, licenses / p.name)
    else:
        archive_path = os.environ.get("PDFIUM_ARCHIVE")
        if archive_path:
            archive = Path(archive_path).read_bytes()
        else:
            with urllib.request.urlopen(URL, timeout=60) as response:
                archive = response.read(100_000_001)
        if sha(archive) != ARCHIVE_SHA256:
            raise ValueError("Pinned PDFium archive SHA-256 mismatch")
        with tempfile.TemporaryFile() as f:
            f.write(archive)
            f.seek(0)
            with tarfile.open(fileobj=f, mode="r:gz") as tar:
                members = {m.name: m for m in tar if m.isfile()}
                required = ["bin/pdfium.dll", "LICENSE"]
                if any(name not in members for name in required):
                    raise ValueError("Pinned PDFium archive is missing required files")
                data = tar.extractfile(members["bin/pdfium.dll"]).read()
                if sha(data) != DLL_SHA256:
                    raise ValueError("Pinned PDFium DLL SHA-256 mismatch")
                names = [name for name in members if name == "LICENSE" or name.startswith("licenses/")]
                if len(names) < 2:
                    raise ValueError("Pinned PDFium archive has no third-party notices")
                for name in names:
                    target = licenses / ("PDFium-LICENSE" if name == "LICENSE" else Path(name).name)
                    target.write_bytes(tar.extractfile(members[name]).read())
    check_dll(data)
    (output / "pdfium.dll").write_bytes(data)
    print(f"PDFium x64: {sha(data)} ({'custom' if custom else VERSION})")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, tarfile.TarError) as e:
        sys.exit(str(e))
