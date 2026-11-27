#!/usr/bin/env python3
"""DOC42 headless format routing regression (requires pdfinfo and pdftotext from Poppler)."""
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import zipfile
import xml.etree.ElementTree as ET
import struct
import zlib


binary = Path(sys.argv[1] if len(sys.argv) > 1 else ".build/debug/DOC42").resolve()


def run(*arguments):
    result = subprocess.run([str(binary), *map(str, arguments)], capture_output=True, timeout=30, check=True)
    return result.stdout.decode(errors="replace")


def stats(path):
    return {key: int(value) for key, value in re.findall(r"^([\w-]+) (\d+)$", run("--stats", path), re.MULTILINE)}


with tempfile.TemporaryDirectory(prefix="suite42-document-cli-") as directory:
    root = Path(directory)
    source, document, restored = root / "source.txt", root / "document.docx", root / "restored.txt"
    text = "Headless document round trip\nРусский текст и Türkçe metin."
    source.write_text(text, encoding="utf-8")
    run("--export", source, document)
    run("--export", document, restored)
    assert restored.read_text(encoding="utf-8").strip() == text
    assert stats(document)["pages"] == 1
    print("PASS --export opens DOCX as a document and preserves Unicode text")

    demo = root / "demo.docx"
    run("--demo", demo)
    information = stats(demo)
    assert 1 <= information["pages"] < 20 and 1000 < information["characters"] < 15000, information
    run("--export", demo, root / "demo.txt")
    plain = (root / "demo.txt").read_text(encoding="utf-8")
    assert all(heading in plain for heading in ["Feature Test", "Introduction", "Conclusion"])
    print(f"PASS --stats reopens the real demo: {information['pages']} pages, {information['characters']} characters")

    # Independent EPUB fixture: the package is produced by Python, not DOC42.
    epub = root / "external.epub"
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(b"\x00\xff\x00\x00")) + chunk(b"IEND", b"")
    container = '<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>'
    opf = '<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="b" href="b.xhtml" media-type="application/xhtml+xml"/><item id="img" href="pic.png" media-type="image/png"/></manifest><spine><itemref idref="a"/><itemref idref="b"/></spine></package>'
    a = '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>A</title></head><body><h1>Heading 世界</h1><p>Link <a href="https://example.com/">example</a>.</p><ul><li>First</li><li>Second</li></ul><table><tr><td>Alpha</td>\n<td>Beta</td></tr><tr><td>Gamma</td><td>Delta</td></tr></table><pre>  code  x\n    next</pre><p><img src="pic.png" alt="picture"/></p></body></html>'
    b = '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>B</title></head><body><p>Second chapter Türkçe</p></body></html>'
    def fixture(path, package=opf, chapter=a, extra=None):
        with zipfile.ZipFile(path, "w") as z:
            z.writestr("mimetype", "application/epub+zip", compress_type=zipfile.ZIP_STORED)
            z.writestr("META-INF/container.xml", container)
            z.writestr("OPS/book.opf", package)
            z.writestr("OPS/a.xhtml", chapter)
            z.writestr("OPS/b.xhtml", b)
            z.writestr("OPS/pic.png", png)
            if extra: z.writestr(*extra)
    fixture(epub)
    imported = root / "epub.txt"
    run("--export", epub, imported)
    value = imported.read_text(encoding="utf-8")
    assert all(t in value for t in ["Heading 世界", "• First", "• Second", "Alpha\tBeta", "Gamma\tDelta", "  code  x\n    next", "Second chapter Türkçe"]), repr(value)
    assert value.index("Heading 世界") < value.index("Second chapter Türkçe")
    assert 'link "example"' in run("--dump", epub)
    exported = root / "exported.epub"
    run("--export", epub, exported)
    with zipfile.ZipFile(exported) as z:
        entries = z.infolist()
        assert entries[0].filename == "mimetype" and entries[0].compress_type == zipfile.ZIP_STORED
        assert z.read("mimetype") == b"application/epub+zip"
        for name in ["META-INF/container.xml", "OEBPS/package.opf", "OEBPS/nav.xhtml", "OEBPS/content.xhtml"]: ET.fromstring(z.read(name))
        assert any(name.startswith("OEBPS/image") and name.endswith(".png") for name in z.namelist())
    run("--export", exported, root / "epub-reopened.txt")
    assert "Second chapter Türkçe" in (root / "epub-reopened.txt").read_text(encoding="utf-8")
    print("PASS independently authored EPUB chapters, links, lists, table, preformatted text and image; exported package reopens")
    for name, options in [
        ("script", {"chapter": a.replace("</body>", "<script>bad()</script></body>")}),
        ("fixed", {"package": opf.replace("<manifest>", '<metadata><meta property="rendition:layout">pre-paginated</meta></metadata><manifest>')}),
        ("traversal", {"package": opf.replace('href="a.xhtml"', 'href="../../escape.xhtml"')}),
        ("encrypted", {"extra": ("META-INF/encryption.xml", "<encryption/>")}),
        ("oversized", {"extra": ("large.bin", b"x" * (32 * 1024 * 1024 + 1))}),
    ]:
        bad = root / (name + ".epub")
        fixture(bad, **options)
        result = subprocess.run([str(binary), "--export", str(bad), str(root / "bad.txt")], capture_output=True, timeout=30)
        assert result.returncode != 0, name
    for name, options in [
        ("deep-xml", {"chapter": '<html xmlns="http://www.w3.org/1999/xhtml"><body>' + '<div>' * 70 + 'deep' + '</div>' * 70 + '</body></html>'}),
        ("repeated-spine", {"package": opf.replace('<itemref idref="a"/><itemref idref="b"/>', '<itemref idref="a"/>' * 1025)}),
    ]:
        bad = root / (name + ".epub")
        fixture(bad, **options)
        result = subprocess.run([str(binary), "--export", str(bad), str(root / "bad.txt")], capture_output=True, timeout=30)
        assert result.returncode != 0, name
    print("PASS EPUB excessive nesting and repeated spine limits")
    malformed = root / "malformed.epub"
    malformed.write_bytes(b"not an epub")
    assert subprocess.run([str(binary), "--export", str(malformed), str(root / "bad.txt")], capture_output=True, timeout=30).returncode != 0
    print("PASS malformed, oversized, traversal, encrypted, fixed-layout and script EPUB rejection")

    for command in ["--pdf", "--export"]:
        pdf = root / (command[2:] + ".pdf")
        run(command, demo, pdf)
        metadata = subprocess.check_output(["pdfinfo", str(pdf)], text=True)
        pages = int(re.search(r"^Pages:\s+(\d+)", metadata, re.MULTILINE).group(1))
        extracted = subprocess.check_output(["pdftotext", str(pdf), "-"], text=True)
        assert pages == information["pages"], (command, pages, information)
        assert all(heading in extracted for heading in ["Feature Test", "Introduction", "Conclusion"])
        print(f"PASS {command} produces {pages} PDF pages containing document text")
print("6/6 passed")
