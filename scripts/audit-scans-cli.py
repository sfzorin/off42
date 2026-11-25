#!/usr/bin/env python3
"""Independent SCN42 output checks. Uses locally generated fixtures, Pillow and pypdf; no physical scanner."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import time

from PIL import Image, ImageDraw, ImageFont
from pypdf import PdfReader

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, default=ROOT / '.build/debug/SCN42')
parser.add_argument('--output-dir', type=Path, default=ROOT / 'build/scn-cli-audit')
args = parser.parse_args()
binary = args.binary.resolve()
out = args.output_dir.resolve(); out.mkdir(parents=True, exist_ok=True)
started = datetime.now(timezone.utc).isoformat()
identity = hashlib.sha256(binary.read_bytes()).hexdigest()
checks, commands = [], []


def run(*arguments, success=True, timeout=150):
    command = [str(binary), *map(str, arguments)]
    result = subprocess.run(command, text=True, capture_output=True, timeout=timeout)
    commands.append({'arguments': command, 'exit_code': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr})
    assert (result.returncode == 0) == success, f'Unexpected exit {result.returncode}: {command}\n{result.stdout}\n{result.stderr}'
    return result.stdout


def check(name, action):
    begin = time.monotonic()
    try:
        detail = action()
        result = {'name': name, 'status': 'PASS', 'detail': detail}
    except Exception as error:
        result = {'name': name, 'status': 'FAIL', 'detail': str(error)}
    result['seconds'] = round(time.monotonic() - begin, 3)
    checks.append(result); print(result['status'], name + ':', result['detail'], flush=True)


source = out / 'source.png'
image = Image.new('RGB', (120, 80), 'white'); draw = ImageDraw.Draw(image)
draw.rectangle((0, 0, 59, 39), fill=(220, 30, 30)); draw.rectangle((60, 0, 119, 39), fill=(30, 210, 30))
draw.rectangle((0, 40, 59, 79), fill=(30, 30, 220)); draw.rectangle((60, 40, 119, 79), fill=(220, 210, 30))
image.save(source, dpi=(144, 144))
second = out / 'second.png'; Image.new('RGB', (90, 130), (10, 100, 220)).save(second, dpi=(72, 72))


def process_roundtrip():
    for extension, expected in [('png', 'PNG'), ('jpg', 'JPEG')]:
        target = out / ('processed.' + extension)
        run('--process', source, target, '--dpi', '200')
        with Image.open(target) as result:
            assert result.format == expected and result.size == image.size
            assert abs(result.info['dpi'][0] - 200) < 0.1
            red, blue = result.convert('RGB').getpixel((10, 10)), result.convert('RGB').getpixel((10, 60))
            assert red[0] > red[2] + 100 and blue[2] > blue[0] + 100, (red, blue)
    return 'PNG/JPEG process outputs preserve dimensions, quadrants and explicit DPI'


def image_formats():
    for extension, expected in [('png', 'PNG'), ('jpg', 'JPEG'), ('tiff', 'TIFF')]:
        target = out / ('pipeline.' + extension)
        run('--pipeline', source, target, '--photo', '--compress', 'original')
        with Image.open(target) as result:
            assert result.format == expected and result.size == image.size
    return 'Pipeline selects PNG/JPEG/TIFF from the requested output extension'


def pdf_geometry():
    for compression in ['original', 'high', 'balanced', 'smallest']:
        target = out / ('physical-' + compression + '.pdf')
        run('--pipeline', source, second, target, '--photo', '--compress', compression)
        pages = PdfReader(target).pages
        assert len(pages) == 2
        for page, expected in zip(pages, [(60, 40), (90, 130)]):
            assert abs(float(page.mediabox.width) - expected[0]) < 0.1
            assert abs(float(page.mediabox.height) - expected[1]) < 0.1
            assert len(page.images) >= 1
    return 'All PDF compression presets retain page count, raster content and per-page physical dimensions'


def multipage():
    tiff = out / 'input-pages.tiff'
    image.save(tiff, save_all=True, append_images=[Image.open(second)], compression='tiff_lzw', dpi=(144, 144))
    target = out / 'all-pages.pdf'; run('--pipeline', tiff, target, '--photo', '--compress', 'original')
    assert len(PdfReader(target).pages) == 2
    result = out / 'all-pages.tiff'; run('--pipeline', source, second, result, '--photo')
    with Image.open(result) as output:
        assert output.n_frames == 2
        assert output.size == (120, 80); output.seek(1); assert output.size == (90, 130)
    sentinel = out / 'single-page.png'; sentinel.write_bytes(b'KEEP')
    run('--process', tiff, sentinel, success=False)
    assert sentinel.read_bytes() == b'KEEP'
    return 'Multipage TIFF is retained by pipeline; single-page process rejects it without replacing output'


def separate_files():
    base = out / 'numbered.png'
    run('--pipeline', source, second, base, '--photo')
    a, b = out / 'numbered 001.png', out / 'numbered 002.png'
    assert Image.open(a).size == (120, 80) and Image.open(b).size == (90, 130)
    run('--pipeline', second, source, base, '--photo')
    assert Image.open(a).size == (90, 130) and Image.open(b).size == (120, 80)
    assert not (out / 'numbered 001 2.png').exists()
    per_page = out / 'separate.pdf'
    run('--pipeline', source, second, per_page, '--photo', '--format', 'pdf-per-page')
    assert all(len(PdfReader(out / f'separate {i:03d}.pdf').pages) == 1 for i in [1, 2])
    return 'One-file-per-page formats use deterministic destinations and explicit repeated export replaces them'


def orientation():
    oriented = out / 'oriented.jpg'
    exif = Image.Exif(); exif[274] = 6
    image.save(oriented, quality=100, subsampling=0, exif=exif, dpi=(144, 144))
    target = out / 'oriented.png'; run('--process', oriented, target)
    with Image.open(target) as result:
        assert result.size == (80, 120)
        expected = image.transpose(Image.Transpose.ROTATE_270)
        for xy in [(10, 10), (10, 100), (60, 20), (60, 100)]:
            assert max(abs(a-b) for a,b in zip(result.convert('RGB').getpixel(xy), expected.getpixel(xy))) < 15
    return 'EXIF orientation is applied once to the processed pixel geometry'


def colour_modes():
    for mode in ['--gray', '--bw']:
        target = out / ('colour' + mode + '.tiff')
        run('--pipeline', source, target, '--photo', mode)
        with Image.open(target) as result:
            if mode == '--bw':
                assert result.mode == '1' and result.tag_v2.get(259) == 4, (result.mode, result.tag_v2.get(259))
            else:
                assert result.mode in ['L', 'I;16']
    return 'Gray output is grayscale; B&W TIFF uses packed one-bit CCITT Group 4'


def archival():
    target = out / 'archival.pdf'; run('--pipeline', source, target, '--photo', '--pdfa')
    doc = PdfReader(target); root = doc.trailer['/Root']
    assert len(doc.pages) == 1 and '/Metadata' in root and '/OutputIntents' in root
    metadata = root['/Metadata'].get_data()
    assert b'<pdfaid:part>2</pdfaid:part>' in metadata and b'<pdfaid:conformance>B</pdfaid:conformance>' in metadata
    assert len(root['/OutputIntents']) > 0
    return 'PDF/A-style XMP and output intent exist; this check does not certify PDF/A conformance'


def failure_preservation():
    target = out / 'preserve.png'; sentinel = b'KEEP EXISTING OUTPUT'
    invalid = [('--unknown',), ('--dpi',), ('--dpi', 'nan'), ('--dpi', '0'), ('--dpi', '5000'),
               ('--bw', '--gray'), ('--crop', '--crop'), ('--dpi', '100', '--extra')]
    for flags in invalid:
        target.write_bytes(sentinel)
        run('--process', source, target, *flags, success=False)
        assert target.read_bytes() == sentinel
    pdf = out / 'preserve.pdf'
    for flags in [('--compress', 'invalid'), ('--format', 'invalid'), ('--text', str(out / 'text.txt')), ('--ocr', '--text', str(out / 'text.bad'))]:
        pdf.write_bytes(sentinel); run('--pipeline', source, pdf, *flags, success=False); assert pdf.read_bytes() == sentinel
    pdf.write_bytes(sentinel); run('--pipeline', source, out / 'missing.png', pdf, '--photo', success=False); assert pdf.read_bytes() == sentinel
    run('--process', source, out, success=False)
    run('--process', source, out / 'absent-folder' / 'file.png', success=False)
    run('--fake-scan', out / 'no-input', out / 'empty.pdf', success=False)
    run('--list-devices', 'nan', success=False)
    run('--unknown-command', success=False)
    return 'Invalid flags, values, mixed bad inputs, write failures and empty scan sources fail visibly and preserve destinations'


def text_output():
    text_image = Image.new('RGB', (1200, 420), 'white')
    font = ImageFont.truetype('/System/Library/Fonts/Helvetica.ttc', 62)
    ImageDraw.Draw(text_image).text((65, 90), 'ALPHA INVOICE 4217', fill='black', font=font)
    fixture = out / 'text.png'; text_image.save(fixture, dpi=(150, 150))
    target = out / 'searchable.pdf'; txt = out / 'recognized.txt'
    run('--pipeline', fixture, target, '--photo', '--ocr', '--text', txt, '--compress', 'original')
    assert 'ALPHA INVOICE 4217' in txt.read_text()
    assert 'ALPHA INVOICE 4217' in PdfReader(target).pages[0].extract_text()
    rtf = out / 'recognized.rtf'
    run('--pipeline', fixture, out / 'searchable-rtf.pdf', '--photo', '--ocr', '--text', rtf)
    assert rtf.read_bytes().startswith(b'{\\rtf') and b'ALPHA INVOICE 4217' in rtf.read_bytes()
    return 'Actual Vision OCR reaches searchable PDF, Unicode TXT and readable RTF output'


for name, body in [('Process pixels and DPI', process_roundtrip), ('Image containers', image_formats),
                   ('PDF pages and compression', pdf_geometry), ('Multipage inputs and outputs', multipage),
                   ('Separate page files', separate_files), ('EXIF orientation', orientation),
                   ('Grayscale and one-bit output', colour_modes), ('Archival metadata', archival),
                   ('Failure preservation', failure_preservation), ('Actual OCR and text', text_output)]:
    check(name, body)
unchanged = hashlib.sha256(binary.read_bytes()).hexdigest() == identity
report = {'started_at': started, 'binary': str(binary), 'binary_sha256': identity, 'unchanged_during_audit': unchanged,
          'checks': checks, 'commands': commands}
(out / 'result.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
passed = sum(item['status'] == 'PASS' for item in checks)
print(f'{passed} passed, {len(checks)-passed} failed, 0 skipped; {out / "result.json"}', flush=True)
raise SystemExit(0 if passed == len(checks) and unchanged else 1)
