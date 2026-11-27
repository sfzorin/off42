#!/usr/bin/env python3
"""Independent PPT42 conversion/export regression fixtures and a dated JSON report."""
from __future__ import annotations
import argparse
import hashlib
import json
import re
import runpy
import shutil
import struct
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path
from xml.sax.saxutils import escape
from zipfile import ZipFile, ZIP_DEFLATED

P = 'http://schemas.openxmlformats.org/presentationml/2006/main'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
PKG = 'http://schemas.openxmlformats.org/package/2006/relationships'
NS = {'p': P, 'a': A, 'r': R}


def require(value, message):
    if not value:
        raise AssertionError(message)


def xml_text(value):
    return escape(value).replace('\r', '&#13;').replace('\n', '&#10;').replace('\t', '&#9;')


def fixture(path):
    notes = '\n'.join('Complete note line %03d' % i for i in range(180)) + '\nLAST-NOTE-TOKEN'
    labels = ['VISIBLE-FIRST', 'HIDDEN-MIDDLE', 'VISIBLE-LAST']
    pres = f'<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="master"/></p:sldMasterIdLst><p:sldIdLst>' + ''.join(f'<p:sldId id="{i + 255}" r:id="rId{i}"/>' for i in range(1, 4)) + '</p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>'
    files = {'ppt/presentation.xml': pres,
             'ppt/_rels/presentation.xml.rels': f'<Relationships xmlns="{PKG}"><Relationship Id="master" Type="{R}/slideMaster" Target="slideMasters/slideMaster1.xml"/>' + ''.join(f'<Relationship Id="rId{i}" Type="{R}/slide" Target="slides/slide{i}.xml"/>' for i in range(1, 4)) + '</Relationships>',
             '[Content_Types].xml': '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/></Types>',
             '_rels/.rels': f'<Relationships xmlns="{PKG}"><Relationship Id="main" Type="{R}/officeDocument" Target="ppt/presentation.xml"/></Relationships>'}
    group = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>'
    files['ppt/slideMasters/slideMaster1.xml'] = f'<p:sldMaster xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}"><p:cSld><p:spTree>{group}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="layout"/></p:sldLayoutIdLst><p:txStyles/></p:sldMaster>'
    files['ppt/slideMasters/_rels/slideMaster1.xml.rels'] = f'<Relationships xmlns="{PKG}"><Relationship Id="layout" Type="{R}/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="theme" Type="{R}/theme" Target="../theme/theme1.xml"/></Relationships>'
    files['ppt/slideLayouts/slideLayout1.xml'] = f'<p:sldLayout xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}" type="blank" preserve="1"><p:cSld name="Blank"><p:spTree>{group}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>'
    files['ppt/slideLayouts/_rels/slideLayout1.xml.rels'] = f'<Relationships xmlns="{PKG}"><Relationship Id="master" Type="{R}/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>'
    colours = {'dk1': '000000', 'lt1': 'FFFFFF', 'dk2': '44546A', 'lt2': 'E7E6E6', 'accent1': '4472C4', 'accent2': 'ED7D31', 'accent3': 'A5A5A5', 'accent4': 'FFC000', 'accent5': '5B9BD5', 'accent6': '70AD47', 'hlink': '0563C1', 'folHlink': '954F72'}
    solid = '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    files['ppt/theme/theme1.xml'] = f'<a:theme xmlns:a="{A}" name="Audit"><a:themeElements><a:clrScheme name="Audit">' + ''.join(f'<a:{key}><a:srgbClr val="{value}"/></a:{key}>' for key, value in colours.items()) + '</a:clrScheme><a:fontScheme name="Audit"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Audit"><a:fillStyleLst>' + solid * 3 + '</a:fillStyleLst><a:lnStyleLst>' + ''.join(f'<a:ln w="{width}">{solid}</a:ln>' for width in [6350, 12700, 25400]) + '</a:lnStyleLst><a:effectStyleLst>' + '<a:effectStyle><a:effectLst/></a:effectStyle>' * 3 + '</a:effectStyleLst><a:bgFillStyleLst>' + solid * 3 + '</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>'
    for i, label in enumerate(labels, 1):
        text = label + ('\rCarriage\tTab & Русский Türkçe' if i == 1 else '')
        shape = f'<p:sp><p:nvSpPr><p:cNvPr id="2" name="Audit shape"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="508000" y="508000"/><a:ext cx="10160000" cy="1270000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="FFF0C0"/></a:solidFill><a:ln w="38100"><a:solidFill><a:srgbClr val="2244AA"/></a:solidFill><a:prstDash val="dash"/></a:ln></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr sz="2800" b="1"/><a:t xml:space="preserve">{xml_text(text)}</a:t></a:r></a:p></p:txBody></p:sp>'
        files[f'ppt/slides/slide{i}.xml'] = f'<p:sld xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}" show="{0 if i == 2 else 1}"><p:cSld><p:spTree>{group}{shape}</p:spTree></p:cSld></p:sld>'
        note = notes if i == 1 else '  Notes %d with tab\tand CR\r  ' % i
        files[f'ppt/notesSlides/notesSlide{i}.xml'] = f'<p:notes xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Notes"/><p:cNvSpPr/><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>' + ''.join(f'<a:p><a:r><a:t xml:space="preserve">{xml_text(line)}</a:t></a:r></a:p>' for line in note.split('\n')) + '</p:txBody></p:sp></p:spTree></p:cSld></p:notes>'
        files[f'ppt/slides/_rels/slide{i}.xml.rels'] = f'<Relationships xmlns="{PKG}"><Relationship Id="notes" Type="{R}/notesSlide" Target="../notesSlides/notesSlide{i}.xml"/><Relationship Id="layout" Type="{R}/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>'
    main_types = {'ppt/presentation.xml': 'presentation.main', 'ppt/slideMasters/slideMaster1.xml': 'slideMaster', 'ppt/slideLayouts/slideLayout1.xml': 'slideLayout'}
    main_types.update({f'ppt/slides/slide{i}.xml': 'slide' for i in range(1, 4)})
    main_types.update({f'ppt/notesSlides/notesSlide{i}.xml': 'notesSlide' for i in range(1, 4)})
    files['[Content_Types].xml'] = '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>' + ''.join(f'<Override PartName="/{path}" ContentType="application/vnd.openxmlformats-officedocument.presentationml.{kind}+xml"/>' for path, kind in main_types.items()) + '<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/></Types>'
    with ZipFile(path, 'w', ZIP_DEFLATED) as archive:
        for name, data in files.items():
            ET.fromstring(data)
            archive.writestr(name, data.encode())
    return notes


def inspect(path, notes):
    with ZipFile(path) as archive:
        presentation = ET.fromstring(archive.read('ppt/presentation.xml'))
        require(len(presentation.findall('p:sldIdLst/p:sldId', NS)) == 3, 'slide count changed')
        for i, token in enumerate(['VISIBLE-FIRST', 'HIDDEN-MIDDLE', 'VISIBLE-LAST'], 1):
            slide = ET.fromstring(archive.read(f'ppt/slides/slide{i}.xml'))
            require((slide.get('show') == '0') == (i == 2), 'hidden slide state changed')
            texts = [node.text or '' for node in slide.findall('.//a:t', NS)]
            require(any(token in text for text in texts), f'lost {token}')
            require(slide.find('.//a:rPr[@b="1"]', NS) is not None, 'lost bold text')
            line = slide.find('.//p:spPr/a:ln', NS)
            require(line is not None and abs(int(line.get('w', '0')) / 12700 - 3) < 0.02, 'lost line width')
            color = line.find('a:solidFill/a:srgbClr', NS)
            require(color is not None and color.get('val').upper() == '2244AA', 'lost line colour')
            require(line.find('a:prstDash', NS).get('val') == 'dash', 'lost dashed line')
            if i == 1:
                require('\rCarriage\tTab & Русский Türkçe' in ''.join(texts), 'normalized or lost special text')
        note = ET.fromstring(archive.read('ppt/notesSlides/notesSlide1.xml'))
        body = next(sp for sp in note.findall('.//p:sp', NS) if sp.find('p:nvSpPr/p:nvPr/p:ph[@type="body"]', NS) is not None)
        paragraphs = [''.join(p.itertext()) for p in body.findall('p:txBody/a:p', NS)]
        require('\n'.join(paragraphs) == notes, 'long notes were truncated or normalized')


def main():
    repo = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=repo / '.build/debug/PPT42')
    parser.add_argument('--output-dir', type=Path, default=repo / 'build/ppt-cli-audit')
    parser.add_argument('--skip-pdf', action='store_true')
    parser.add_argument('--skip-movie', action='store_true')
    args = parser.parse_args()
    binary = args.binary.resolve()
    output = args.output_dir.resolve(); output.mkdir(parents=True, exist_ok=True)
    source = output / 'source.pptx'; notes = fixture(source)
    report = {'started_at': datetime.now(timezone.utc).isoformat(), 'binary': str(binary), 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': []}
    command_log = output / 'commands.log'; command_log.write_text('')
    def run(*arguments, expected=0):
        command = [str(binary), *map(str, arguments)]
        completed = subprocess.run(command, capture_output=True, text=True, timeout=90)
        with command_log.open('a') as handle:
            handle.write(json.dumps(command, ensure_ascii=False) + '\n' + completed.stdout + completed.stderr + f'\nexit={completed.returncode}\n')
        require(completed.returncode == expected, f'command exited {completed.returncode}: {completed.stderr[-2000:]}')
        return completed.stdout
    def check(name, operation, skip=None):
        started = time.monotonic()
        if skip:
            result = {'name': name, 'status': 'SKIP', 'detail': skip}
        else:
            try:
                detail = operation()
                result = {'name': name, 'status': 'PASS', 'detail': detail or 'verified'}
            except Exception as error:
                result = {'name': name, 'status': 'FAIL', 'detail': f'{type(error).__name__}: {error}'}
        result['seconds'] = round(time.monotonic() - started, 3)
        report['checks'].append(result)
        print(f"{result['status']} {name}: {result['detail']}", flush=True)
    def keynote():
        runpy.run_path(str(repo / 'scripts/keynote-fixture-audit.py'))['run'](binary)
        return 'editable slide order, Unicode, geometry, shapes, image, PPTX reopen, source preservation and unsupported object rejection'
    check('Keynote native import', keynote)
    def conversions():
        run('--convert', source, output / 'converted.odp')
        run('--convert', output / 'converted.odp', output / 'roundtrip.pptx')
        inspect(output / 'roundtrip.pptx', notes)
        return 'slide order, visibility, text, CR/tab, bold, line width/colour/dash and full notes preserved'
    check('PPTX → ODP → PPTX round trip', conversions)
    def templates():
        for template in ['potx', 'otp']:
            run('--convert', source, output / ('template.' + template))
            run('--convert', output / ('template.' + template), output / ('from-' + template + '.pptx'))
            inspect(output / ('from-' + template + '.pptx'), notes)
        return 'both template formats retain the supported fixture'
    check('POTX and OTP template round trips', templates)
    pdf_skip = 'explicit --skip-pdf' if args.skip_pdf else None
    if not shutil.which('pdfinfo') or not shutil.which('pdftotext'):
        pdf_skip = 'Poppler pdfinfo/pdftotext unavailable'
    def pdf(name, flags, pages=None, tail=False):
        path = output / name
        run('--export', source, path, *flags)
        info = subprocess.check_output(['pdfinfo', str(path)], text=True)
        text = subprocess.check_output(['pdftotext', str(path), '-'], text=True)
        count = int(re.search(r'^Pages:\s+(\d+)', info, re.M).group(1))
        require(pages is None or count == pages, f'wrong page count: {count}')
        require('VISIBLE-FIRST' in text and 'VISIBLE-LAST' in text, 'visible slides missing')
        require(('HIDDEN-MIDDLE' in text) == ('--hidden' in flags), 'hidden-slide scope wrong')
        if tail:
            require(count > 2 and 'Complete note line 000' in text and 'LAST-NOTE-TOKEN' in text, 'long notes not fully exported')
        return f'{count} pages, expected scope/text verified'
    check('PDF visible slides', lambda: pdf('slides.pdf', [], 2), pdf_skip)
    check('PDF includes hidden slides', lambda: pdf('all.pdf', ['--hidden'], 3), pdf_skip)
    check('PDF complete long notes', lambda: pdf('notes.pdf', ['--notes'], tail=True), pdf_skip)
    def images():
        for dirname, hidden, count in [('images-visible', False, 2), ('images-all', True, 3)]:
            directory = output / dirname
            if directory.exists(): shutil.rmtree(directory)
            run('--export', source, directory, '--width', '320', *(['--hidden'] if hidden else []))
            pngs = list(directory.glob('*.png'))
            require(len(pngs) == count, f'wrong image count for {dirname}: {len(pngs)}')
            require(all(struct.unpack('>II', p.read_bytes()[16:24]) == (320, 180) for p in pngs), 'wrong image dimensions')
        return 'visible/all scopes and 320×180 PNG dimensions verified'
    check('Image export visibility and dimensions', images)
    def invalid():
        destination = output / 'preserved-images'; destination.mkdir(exist_ok=True)
        existing = destination / 'Slide 01.png'; existing.write_bytes(b'KEEP')
        run('--export', source, destination, '--width', '-1', expected=1)
        require(existing.read_bytes() == b'KEEP', 'failed image export damaged existing output')
        broken = output / 'broken.pptx'
        with ZipFile(source) as old, ZipFile(broken, 'w', ZIP_DEFLATED) as new:
            for name in old.namelist():
                if name != 'ppt/slides/slide2.xml': new.writestr(name, old.read(name))
        target = output / 'preserved.pptx'; target.write_bytes(b'KEEP')
        run('--convert', broken, target, expected=1)
        require(target.read_bytes() == b'KEEP', 'failed conversion damaged existing output')
        return 'invalid dimensions and missing slides fail without replacing destinations'
    check('Failed outputs preserve existing files', invalid)
    def movie():
        for ext, brand in [('mp4', b'mp42'), ('mov', b'qt  ')]:
            target = output / ('movie.' + ext)
            run('--export', source, target, '--seconds', '0.1', '--width', '160', '--hidden')
            data = target.read_bytes()
            require(len(data) > 1000 and data[4:8] == b'ftyp', 'movie is not a completed ISO media container')
            require((data[8:12] == b'qt  ') == (ext == 'mov'), f'{ext} uses wrong container brand: {data[8:12]!r}')
        return 'MP4 and actual QuickTime MOV containers generated'
    check('Movie export container formats', movie, 'explicit --skip-movie' if args.skip_movie else None)
    report['finished_at'] = datetime.now(timezone.utc).isoformat()
    (output / 'result.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    counts = {state: sum(c['status'] == state for c in report['checks']) for state in ['PASS', 'FAIL', 'SKIP']}
    print(f"{counts['PASS']} passed, {counts['FAIL']} failed, {counts['SKIP']} skipped; {output / 'result.json'}")
    return int(counts['FAIL'] > 0)


if __name__ == '__main__':
    sys.exit(main())
