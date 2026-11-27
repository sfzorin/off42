#!/usr/bin/env python3
"""Exercise XLS42 file conversions and PDF export using independent XML/CSV fixtures.

No office suite or Python dependencies are required. PDF text/page checks use
Poppler when available; the JSON report explicitly records skipped PDF checks.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path
from xml.sax.saxutils import escape, quoteattr
from zipfile import ZIP_DEFLATED, ZipFile


S = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
ODF = {
    "office": "urn:oasis:names:tc:opendocument:xmlns:office:1.0",
    "table": "urn:oasis:names:tc:opendocument:xmlns:table:1.0",
    "text": "urn:oasis:names:tc:opendocument:xmlns:text:1.0",
}


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def fixture(path):
    """A small real workbook, including state that cannot be checked via CSV."""
    sheet_name = "Plan 1, O'Brien"
    sheet_ref = "'Plan 1, O''Brien'!"

    def text_cell(address, value):
        return f'<c r="{address}" t="inlineStr"><is><t xml:space="preserve">{escape(value)}</t></is></c>'

    rows = []
    literal = {"A1": "Roundtrip audit", "A2": "=1+2", "A3": "00123", "A4": "TRUE", "A5": "  spaced  ", "A6": "Русский Türkçe", "A7": "comma, text"}
    additions = {
        2: '<c r="B2" s="1"><v>0</v></c><c r="C2" s="2"/>',
        3: '<c r="B3" s="1"><v>1.5</v></c><c r="C3" s="3" t="inlineStr"><is><t>Styled</t></is></c>',
        5: '<c r="B5"><f>DATE(1904,1,1)</f><v>0</v></c>',
        6: '<c r="B6"><f>YEAR(B2)</f><v>1904</v></c><c r="F6"/>',
        7: '<c r="B7"><f>SUM(B3,2)</f><v>3.5</v></c>',
    }
    for number in range(1, 8):
        rows.append(f'<row r="{number}"' + (' ht="30" customHeight="1"' if number == 4 else '') + '>'
                    + text_cell(f"A{number}", literal[f"A{number}"]) + additions.get(number, "") + '</row>')
    first = f'''<worksheet xmlns="{S}"><sheetViews><sheetView workbookViewId="0"><pane state="frozen" xSplit="1" ySplit="1" topLeftCell="B2"/></sheetView></sheetViews><cols><col min="1" max="1" width="26" customWidth="1"/><col min="2" max="3" width="19" customWidth="1"/></cols><sheetData>{''.join(rows)}</sheetData><sheetProtection sheet="1"/><mergeCells count="1"><mergeCell ref="F6:G7"/></mergeCells></worksheet>'''
    extra = lambda label: f'<worksheet xmlns="{S}"><sheetData><row r="1">{text_cell("A1", label)}</row></sheetData></worksheet>'
    names = (f'<definedName name="TaxRate">0.2</definedName><definedName name="TotalValue">{escape(sheet_ref)}$B$3*2</definedName>'
             f'<definedName name="InputRange">{escape(sheet_ref)}$A$1:$A$3</definedName>'
             f'<definedName name="_xlnm.Print_Area" localSheetId="0">{escape(sheet_ref)}$A$1:$G$8</definedName>')
    workbook = f'''<workbook xmlns="{S}" xmlns:r="{R}"><workbookPr date1904="1"/><sheets><sheet name={quoteattr(sheet_name)} sheetId="1" r:id="rId1"/><sheet name="Second" sheetId="2" r:id="rId2"/><sheet name="Hidden" sheetId="3" state="hidden" r:id="rId3"/></sheets><definedNames>{names}</definedNames></workbook>'''
    border = ''.join(f'<{edge} style="thin"><color rgb="FFCC0000"/></{edge}>' for edge in ["left", "right", "top", "bottom"])
    styles = f'''<styleSheet xmlns="{S}"><numFmts count="1"><numFmt numFmtId="164" formatCode="dd.mm.yyyy hh:mm"/></numFmts><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><sz val="11"/><name val="Calibri"/><b/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFCCFFCC"/></patternFill></fill></fills><borders count="2"><border/><border>{border}</border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="164" fontId="0" fillId="0" borderId="0" applyNumberFormat="1"/><xf numFmtId="2" fontId="0" fillId="0" borderId="0" applyProtection="1"><protection locked="0"/></xf><xf numFmtId="0" fontId="1" fillId="2" borderId="1"/></cellXfs></styleSheet>'''
    relationship_ns = "http://schemas.openxmlformats.org/package/2006/relationships"
    rels = f'<Relationships xmlns="{relationship_ns}">' + ''.join(f'<Relationship Id="rId{i}" Type="{R}/worksheet" Target="worksheets/sheet{i}.xml"/>' for i in range(1, 4)) + f'<Relationship Id="styles" Type="{R}/styles" Target="styles.xml"/></Relationships>'
    content_types = '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>' + ''.join(f'<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>' for i in range(1, 4)) + '</Types>'
    files = {"[Content_Types].xml": content_types,
             "_rels/.rels": f'<Relationships xmlns="{relationship_ns}"><Relationship Id="rId1" Type="{R}/officeDocument" Target="xl/workbook.xml"/></Relationships>',
             "xl/workbook.xml": workbook, "xl/_rels/workbook.xml.rels": rels, "xl/styles.xml": styles,
             "xl/worksheets/sheet1.xml": first, "xl/worksheets/sheet2.xml": extra("Second sheet marker"), "xl/worksheets/sheet3.xml": extra("Hidden sheet marker")}
    with ZipFile(path, "w", ZIP_DEFLATED) as archive:
        for name, data in files.items():
            ET.fromstring(data)
            archive.writestr(name, data.encode("utf-8"))
    return literal


def inspect_xlsx(path, literal):
    with ZipFile(path) as archive:
        book = ET.fromstring(archive.read("xl/workbook.xml"))
        sheet = ET.fromstring(archive.read("xl/worksheets/sheet1.xml"))
        styles = ET.fromstring(archive.read("xl/styles.xml"))
        shared = []
        if "xl/sharedStrings.xml" in archive.namelist():
            shared = [''.join(e.itertext()) for e in ET.fromstring(archive.read("xl/sharedStrings.xml"))]
    cells = {e.attrib["r"]: e for e in sheet.findall(f".//{{{S}}}c")}
    for address, expected in literal.items():
        cell = cells[address]
        kind = cell.get("t")
        text = ''.join(cell.find(f"{{{S}}}is").itertext()) if kind == "inlineStr" else shared[int(cell.findtext(f"{{{S}}}v"))] if kind == "s" else cell.findtext(f"{{{S}}}v", "")
        require(kind in {"inlineStr", "s", "str"} and cell.find(f"{{{S}}}f") is None and text == expected, f"{address}: literal {expected!r} became {text!r} ({kind})")
    require(book.find(f"{{{S}}}workbookPr").get("date1904") in {"1", "true"}, "lost 1904 epoch")
    for address, expected in {"B2": 0, "B3": 1.5, "B5": 0, "B6": 1904, "B7": 3.5}.items():
        require(float(cells[address].findtext(f"{{{S}}}v")) == expected, f"{address}: incorrect date serial or recalculation")
    for address in ["B5", "B6", "B7"]:
        require(cells[address].find(f"{{{S}}}f") is not None, f"lost {address} formula")
    formats = {e.get("numFmtId"): e.get("formatCode") for e in styles.findall(f"{{{S}}}numFmts/{{{S}}}numFmt")}
    xfs = list(styles.find(f"{{{S}}}cellXfs"))
    blank = xfs[int(cells["C2"].get("s", "0"))]
    require(blank.find(f"{{{S}}}protection").get("locked") in {"0", "false"}, "blank input cell lost unlock")
    require(blank.get("numFmtId") == "2" or formats.get(blank.get("numFmtId")) == "0.00", "blank input cell lost number format")
    styled = xfs[int(cells["C3"].get("s", "0"))]
    require(list(styles.find(f"{{{S}}}fonts"))[int(styled.get("fontId", "0"))].find(f"{{{S}}}b") is not None, "lost bold")
    fill = list(styles.find(f"{{{S}}}fills"))[int(styled.get("fillId", "0"))].find(f"{{{S}}}patternFill/{{{S}}}fgColor")
    require(fill is not None and fill.get("rgb", "").upper().endswith("CCFFCC"), "lost fill")
    border = list(styles.find(f"{{{S}}}borders"))[int(styled.get("borderId", "0"))]
    for edge in ["left", "right", "top", "bottom"]:
        side = border.find(f"{{{S}}}{edge}")
        require(side is not None and side.get("style") == "thin", f"lost {edge} border")
        require(side.find(f"{{{S}}}color").get("rgb", "").upper().endswith("CC0000"), f"lost {edge} border color")
    require(sheet.find(f"{{{S}}}sheetProtection") is not None, "lost sheet protection")
    require([e.get("ref") for e in sheet.findall(f"{{{S}}}mergeCells/{{{S}}}mergeCell")] == ["F6:G7"], "lost empty merge")
    require(abs(float(sheet.find(f"{{{S}}}sheetData/{{{S}}}row[@r='4']").get("ht", "0")) - 30) < 0.1, "lost custom row height")
    pane = sheet.find(f"{{{S}}}sheetViews/{{{S}}}sheetView/{{{S}}}pane")
    require(pane is not None and float(pane.get("xSplit", "0")) == 1 and float(pane.get("ySplit", "0")) == 1, "lost freeze panes")
    names = {e.get("name"): e.text for e in book.findall(f"{{{S}}}definedNames/{{{S}}}definedName")}
    require(names.get("TaxRate") == "0.2", "lost named constant")
    require(names.get("TotalValue") == "'Plan 1, O''Brien'!$B$3*2", "lost named expression")
    require(names.get("_xlnm.Print_Area") == "'Plan 1, O''Brien'!$A$1:$G$8", "lost quoted-sheet print area")
    sheets = book.findall(f"{{{S}}}sheets/{{{S}}}sheet")
    require(len(sheets) == 3 and sheets[2].get("state") == "hidden", "lost sheet visibility")
    return "literals, 1904 dates/formulas, formats, borders, blank unlocked cell, merge, row height, panes, names and visibility preserved"


def inspect_ods(path):
    with ZipFile(path) as archive:
        require(archive.read("mimetype") == b"application/vnd.oasis.opendocument.spreadsheet", "wrong ODS mimetype")
        root = ET.fromstring(archive.read("content.xml"))
    date = root.find(".//table:null-date", ODF)
    require(date is not None and date.get(f"{{{ODF['table']}}}date-value") == "1904-01-01", "ODS lost epoch")
    dates = [e.get(f"{{{ODF['office']}}}date-value") for e in root.findall(".//table:table-cell", ODF)]
    require("1904-01-01" in dates and "1904-01-02T12:00:00" in dates, "ODS ISO dates do not match serials")
    require(root.find(".//table:named-expression[@table:name='TaxRate']", ODF) is not None, "ODS lost named constant")
    return "valid ODS package with 1904 null date, ISO dates and named constant"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path(".build/debug/XLS42"))
    parser.add_argument("--output-dir", type=Path, default=Path("build/xls-cli-audit"))
    parser.add_argument("--skip-pdf", action="store_true", help="explicitly skip Poppler PDF checks")
    args = parser.parse_args()
    binary, output = args.binary.resolve(), args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    # A failed conversion must not let a later check inspect last run's output.
    for name in ["converted.ods", "restored.xlsx", "direct.xlsx", "csv.xlsx", "restored.csv", "dates.csv", "active-sheet.pdf", "all-sheets.pdf", "active-sheet.txt", "all-sheets.txt"]:
        (output / name).unlink(missing_ok=True)
    report = {"started_at": datetime.now(timezone.utc).isoformat(), "binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest() if binary.is_file() else None, "checks": [], "commands": []}
    log = output / "commands.log"
    log.write_text("", encoding="utf-8")

    def command(*arguments):
        argv = list(map(str, arguments))
        start = time.monotonic()
        try:
            result = subprocess.run(argv, capture_output=True, text=True, errors="replace", timeout=60)
        except (OSError, subprocess.TimeoutExpired) as error:
            report["commands"].append({"argv": argv, "error": str(error)})
            raise
        report["commands"].append({"argv": argv, "returncode": result.returncode, "seconds": round(time.monotonic() - start, 3)})
        with log.open("a", encoding="utf-8") as stream:
            stream.write(json.dumps(argv, ensure_ascii=False) + "\n" + result.stdout + result.stderr + "\n")
        require(result.returncode == 0, f"command exited {result.returncode}: {argv!r}; see commands.log")
        return result.stdout

    def check(name, operation):
        try:
            detail = operation()
            item = {"name": name, "status": "PASS", "detail": detail}
        except Exception as error:
            item = {"name": name, "status": "FAIL", "detail": f"{type(error).__name__}: {error}"}
        report["checks"].append(item)
        print(f"{item['status']} {name}: {item['detail']}", flush=True)

    source, intermediate, restored = output / "source.xlsx", output / "converted.ods", output / "restored.xlsx"
    literal = fixture(source)

    def roundtrip():
        command(binary, "--convert", source, intermediate)
        inspect_ods(intermediate)
        command(binary, "--convert", intermediate, restored)
        return inspect_xlsx(restored, literal)

    check("XLSX → ODS → XLSX metadata and value round trip", roundtrip)

    def direct():
        destination = output / "direct.xlsx"
        command(binary, "--roundtrip", source, destination)
        return inspect_xlsx(destination, literal)

    check("XLSX direct round trip and recalculation", direct)

    def spreadsheetml_roundtrip():
        ns = "urn:schemas-microsoft-com:office:spreadsheet"
        initial, back, xlsx = output / "excel2003.xml", output / "excel2003-back.xml", output / "excel2003.xlsx"
        initial.write_text('''<?xml version="1.0"?><Workbook xmlns="urn:schemas-microsoft-com:office:spreadsheet" xmlns:x="urn:schemas-microsoft-com:office:spreadsheet"><Styles><Style x:ID="bold"><Font x:Bold="1" x:Color="#112233"/><Interior x:Color="#AABBCC" x:Pattern="Solid"/><NumberFormat x:Format="0.00"/></Style></Styles><Worksheet x:Name="First &amp; One"><Table><Column x:Index="3" x:Width="90"/><Row x:Index="2"><Cell x:Index="2" x:StyleID="bold"><Data x:Type="Number">12.5</Data></Cell><Cell x:Index="4" x:MergeAcross="1"><Data x:Type="String">A &amp; B</Data></Cell><Cell><Data x:Type="Boolean">1</Data></Cell><Cell xmlns:x="urn:unrelated" x:Index="12"><Data x:Type="Number">007</Data></Cell></Row><Row x:Index="4"><Cell x:Index="3" x:Formula="=SUM(R[-2]C:R[-1]C)+R1C1+R1C[-1]+&apos;Other Sheet&apos;!R2C3+IF(RC=&quot;A1 R1C1&quot;,1,0)"><Data x:Type="Number">5</Data></Cell></Row></Table></Worksheet><Worksheet x:Name="Other Sheet"><Table><Row><Cell><Data x:Type="DateTime">2024-01-02T12:00:00.000</Data></Cell></Row></Table></Worksheet></Workbook>''', encoding="utf-8")
        command(binary, "--convert", initial, xlsx)
        with ZipFile(xlsx) as archive:
            sheet = ET.fromstring(archive.read("xl/worksheets/sheet1.xml"))
            styles = ET.fromstring(archive.read("xl/styles.xml"))
            cells = {c.get("r"): c for c in sheet.findall(f".//{{{S}}}c")}
            xf = list(styles.find(f"{{{S}}}cellXfs"))[int(cells["B2"].get("s"))]
            font = list(styles.find(f"{{{S}}}fonts"))[int(xf.get("fontId"))]
            fill = list(styles.find(f"{{{S}}}fills"))[int(xf.get("fillId"))]
            require(font.find(f"{{{S}}}b") is not None and font.find(f"{{{S}}}color").get("rgb").endswith("112233"), "font style changed")
            require(fill.find(f"{{{S}}}patternFill/{{{S}}}fgColor").get("rgb").endswith("AABBCC"), "fill changed")
            require(cells["C4"].findtext(f"{{{S}}}f") == 'SUM(C2:C3)+$A$1+B$1+\'Other Sheet\'!$C$2+IF(C4="A1 R1C1",1,0)', "wrong converted A1 formula")
            require(cells["G2"].get("t") in ("inlineStr", "s", "str"), "rebound unrelated Type or Index was accepted")
            require(cells["B2"].findtext(f"{{{S}}}v") == "12.5", "alternate-prefix numeric type lost")
            require(cells["F2"].findtext(f"{{{S}}}v") == "1", "boolean or merged cell position changed")
            require(sheet.find(f"{{{S}}}mergeCells/{{{S}}}mergeCell").get("ref") == "D2:E2", "merge changed")
        command(binary, "--convert", xlsx, back)
        tree = ET.parse(back); q = "{" + ns + "}"
        formulas = [c.get(q + "Formula") for c in tree.findall(".//" + q + "Cell") if c.get(q + "Formula")]
        require(formulas == ['=SUM(R[-2]C:R[-1]C)+R1C1+R1C[-1]+\'Other Sheet\'!R2C3+IF(RC="A1 R1C1",1,0)'], f"wrong emitted R1C1 {formulas}")
        require(len(tree.findall(q + "Worksheet")) == 2, "lost worksheet")
        command(binary, "--convert", back, output / "excel2003-reopened.xlsx")
        with ZipFile(output / "excel2003-reopened.xlsx") as archive:
            reopened = ET.fromstring(archive.read("xl/worksheets/sheet1.xml"))
            require(reopened.find(f".//{{{S}}}c[@r='C4']/{{{S}}}f").text == cells["C4"].findtext(f"{{{S}}}f"), "reopened formula changed")
        bad = output / "unknown.xml"; bad.write_text("<not-a-workbook/>", encoding="utf-8")
        result = subprocess.run([str(binary), "--convert", str(bad), str(output / "bad.xlsx")], capture_output=True, timeout=60)
        require(result.returncode != 0, "unknown XML was accepted as CSV")
        wrong = output / "wrong-namespace.xml"; wrong.write_text('<Workbook xmlns="urn:unrelated"/>', encoding="utf-8")
        result = subprocess.run([str(binary), "--convert", str(wrong), str(output / "bad.xlsx")], capture_output=True, timeout=60)
        require(result.returncode != 0, "wrong-namespace workbook accepted")
        return "alternate attribute prefix, nested rebinding, two sheets, merge, typed values and mixed/relative/cross-sheet formulas; emitted XML and reopened XLSX inspected"

    check("Excel 2003 XML conversion and reopen", spreadsheetml_roundtrip)

    def csv_roundtrip():
        rows = [["plain", "=1+2", "quotes \"hello\"", "Русский Türkçe"], ["comma, text", "line1\nline2", "first\rsecond", "42"]]
        csv_source, csv_book, csv_back = output / "source.csv", output / "csv.xlsx", output / "restored.csv"
        with csv_source.open("w", encoding="utf-8", newline="") as stream:
            csv.writer(stream).writerows(rows)
        command(binary, "--convert", csv_source, csv_book)
        command(binary, "--convert", csv_book, csv_back)
        with csv_back.open(encoding="utf-8-sig", newline="") as stream:
            result = list(csv.reader(stream))
        require(result == rows, f"CSV round trip changed fields: {result!r}")
        return "Unicode, quoted delimiters, quotes, LF, CR and equals-prefixed text preserved"

    check("CSV → XLSX → CSV text round trip", csv_roundtrip)

    def date_csv():
        destination = output / "dates.csv"
        command(binary, "--convert", source, destination)
        with destination.open(encoding="utf-8-sig", newline="") as stream:
            rows = list(csv.reader(stream))
        require(rows[1][1] == "01.01.1904 00:00" and rows[2][1] == "02.01.1904 12:00", f"CSV used wrong date epoch: {rows[1][1]!r}, {rows[2][1]!r}")
        return "CSV emits displayed dates from the workbook's 1904 epoch"

    check("1904 dates in CSV export", date_csv)
    pdfinfo, pdftotext = shutil.which("pdfinfo"), shutil.which("pdftotext")
    if args.skip_pdf or not pdfinfo or not pdftotext:
        reason = "requested --skip-pdf" if args.skip_pdf else "pdfinfo/pdftotext not found on PATH"
        report["checks"].append({"name": "PDF export text/pages", "status": "SKIP", "detail": reason})
        print(f"SKIP PDF export text/pages: {reason}", flush=True)
    else:
        def pdf_case(name, source_file, options):
            destination = output / f"{name}.pdf"
            command(binary, "--export-pdf", source_file, destination, *options)
            require(destination.read_bytes().startswith(b"%PDF-"), "output is not a PDF")
            metadata = command(pdfinfo, destination)
            text = command(pdftotext, destination, "-")
            (output / f"{name}.txt").write_text(text, encoding="utf-8")
            pages_match = re.search(r"^Pages:\s+(\d+)", metadata, re.MULTILINE)
            require(pages_match is not None, "pdfinfo returned no page count")
            pages = int(pages_match.group(1))
            require("Roundtrip audit" in text and "=1+2" in text and "01.01.1904" in text, "PDF lost text or used wrong date epoch")
            require("Hidden sheet marker" not in text, "PDF exported hidden sheet")
            require(pages >= 2 and "Second sheet marker" in text if "all" in options else pages == 1 and "Second sheet marker" not in text, "incorrect PDF scope or page count")
            if "landscape" in options:
                size = re.search(r"^Page size:\s+([\d.]+) x ([\d.]+)", metadata, re.MULTILINE)
                require(size is not None and float(size.group(1)) > float(size.group(2)), "PDF is not landscape")
            return f"{pages} pages; scope, visible text, 1904 dates" + (", landscape orientation" if "landscape" in options else "")

        check("PDF active-sheet export", lambda: pdf_case("active-sheet", source, []))
        check("PDF ODS workbook landscape export", lambda: pdf_case("all-sheets", intermediate, ["all", "landscape", "nogrid"]))
    report["finished_at"] = datetime.now(timezone.utc).isoformat()
    report["summary"] = {status.lower(): sum(item["status"] == status for item in report["checks"]) for status in ["PASS", "FAIL", "SKIP"]}
    report["artifacts"] = sorted(str(path.relative_to(output)) for path in output.iterdir() if path.is_file())
    (output / "result.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    counts = report["summary"]
    print(f"{counts['pass']} passed, {counts['fail']} failed, {counts['skip']} skipped; {output / 'result.json'}")
    return 1 if counts["fail"] else 0


if __name__ == "__main__":
    sys.exit(main())
