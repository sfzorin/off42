#!/usr/bin/env python3
"""Independent PDF42 output regressions (pypdf; optional signing requires openssl).

All fixtures and signing keys are generated locally. No Keychain changes or timestamp/network requests.
"""
from __future__ import annotations
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import time
from zipfile import ZipFile

from pypdf import PdfReader


def require(value, message):
    if not value:
        raise AssertionError(message)


def fixture(path: Path):
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'', b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>']
    pages = []
    for text in ['SECRET-ONE', 'VISIBLE-TWO', 'VISIBLE-THREE']:
        page, stream = len(objects) + 1, len(objects) + 2
        pages.append(f'{page} 0 R')
        objects.append(f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 500] /Resources << /Font << /F1 3 0 R >> >> /Contents {stream} 0 R >>'.encode())
        commands = f'BT /F1 20 Tf 40 420 Td ({text}) Tj ET'.encode()
        objects.append(f'<< /Length {len(commands)} >>\nstream\n'.encode() + commands + b'\nendstream')
    objects[1] = f'<< /Type /Pages /Count 3 /Kids [{" ".join(pages)}] >>'.encode()
    output, offsets = bytearray(b'%PDF-1.4\n'), [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(output))
        output.extend(f'{index} 0 obj\n'.encode() + obj + b'\nendobj\n')
    xref = len(output)
    output.extend(f'xref\n0 {len(objects) + 1}\n0000000000 65535 f \n'.encode())
    for offset in offsets[1:]:
        output.extend(f'{offset:010d} 00000 n \n'.encode())
    output.extend(f'trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode())
    path.write_bytes(output)


def main():
    repo = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=repo / '.build/debug/PDF42')
    parser.add_argument('--output-dir', type=Path, default=repo / 'build/pdf-cli-audit')
    parser.add_argument('--skip-signing', action='store_true', help='Explicitly skip isolated self-signed signature checks')
    args = parser.parse_args()
    binary, out = args.binary.resolve(), args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    source = out / 'fixture.pdf'
    fixture(source)
    report = {'started_at': datetime.now(timezone.utc).isoformat(), 'binary': str(binary),
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': []}

    def run(*arguments, success=True):
        result = subprocess.run([str(binary), *map(str, arguments)], capture_output=True, timeout=90)
        text = (result.stdout + result.stderr).decode(errors='replace')
        require((result.returncode == 0) == success, f'{arguments[0]} exit {result.returncode}: {text[-1800:]}')
        return text

    def text(path):
        return '\n'.join(page.extract_text() or '' for page in PdfReader(path).pages)

    def check(name, operation):
        start = time.monotonic()
        try:
            detail = operation()
            status = 'PASS'
        except Exception as error:
            status, detail = 'FAIL', str(error)
        report['checks'].append({'name': name, 'status': status, 'detail': detail, 'seconds': round(time.monotonic() - start, 3)})
        print(status, name + ':', detail, flush=True)

    def forms():
        fields, filled, cleared = [out / name for name in ['fields.pdf', 'filled.pdf', 'cleared.pdf']]
        run('--add-field', source, fields)
        run('--fill', fields, filled, 'Name=A=B', 'Agree=yes', 'Colour=Green')
        values = PdfReader(filled).get_fields()
        require(len(values) == 3, 'expected three catalog form fields')
        require(str(values['Name']['/V']) == 'A=B' and str(values['Agree']['/V']) == '/Yes' and str(values['Colour']['/V']) == 'Green', values)
        run('--fill', filled, cleared, 'Name=', 'Agree=off')
        values = PdfReader(cleared).get_fields()
        require(str(values['Name'].get('/V', '')) == '' and str(values['Agree']['/V']) == '/Off', values)
        return 'text/checkbox/choice values and explicit empty text survive reopen'
    check('Fillable form fields', forms)

    def redaction():
        target = out / 'redacted.pdf'
        run('--redact', source, target, '35,410,170,40@1')
        content = text(target)
        require('SECRET-ONE' not in content and 'VISIBLE-TWO' in content and 'VISIBLE-THREE' in content, content)
        require(b'SECRET-ONE' not in target.read_bytes(), 'old literal retained in output bytes')
        reader = PdfReader(target)
        require(len(reader.pages) == 3 and b'SECRET-ONE' not in reader.pages[0].get_contents().get_data(), 'old content stream retained')
        partial = out / 'partially-redacted-field.pdf'
        run('--redact', out / 'filled.pdf', partial, '72,380,4,4@1')
        fields = PdfReader(partial).get_fields() or {}
        require('Name' not in fields and set(fields) == {'Agree', 'Colour'}, fields)
        require(all(ref.get_object().get('/T') != 'Name' for ref in PdfReader(partial).pages[0].get('/Annots', [])), 'partial-overlap widget remained on page')
        return 'source glyphs removed; a <1% overlap removes the whole widget and its catalog field without deleting other fields'
    check('Redaction removes source content', redaction)

    def edited():
        target = out / 'edited.pdf'
        run('--edit-text', source, target, 'SECRET-ONE', 'REPLACED')
        content = text(target)
        require('REPLACED' in content and 'SECRET-ONE' not in content and 'VISIBLE-TWO' in content, content)
        return 'replacement is searchable and the replaced source text is absent'
    check('Edit page text', edited)

    def document_parts():
        properties, bookmarks, labeled, attached, detached = [out / name for name in ['properties.pdf', 'bookmarks.pdf', 'labels.pdf', 'attached.pdf', 'detached.pdf']]
        run('--props', source, properties, 'title=Audit Title', 'author=sfzorin', 'openpage=2', 'zoom=FitH')
        run('--bookmarks', properties, bookmarks, 'First@1:b', '>Child@2', 'Third@3')
        run('--labels', bookmarks, labeled, '1-2:r;3-:D:A-:10')
        attachment = out / 'payload.txt'; attachment.write_text('attachment bytes\nРусский Türkçe', encoding='utf-8')
        run('--attach', labeled, attached, attachment, 'desc=Audit attachment')
        reader = PdfReader(attached)
        require(reader.metadata.title == 'Audit Title' and reader.metadata.author == 'sfzorin', reader.metadata)
        require(reader.page_labels == ['i', 'ii', 'A-10'], reader.page_labels)
        outline = reader.outline
        require(outline[0].title == 'First' and outline[1][0].title == 'Child' and outline[2].title == 'Third', outline)
        require(reader.get_destination_page_number(outline[1][0]) == 1, 'child bookmark destination changed')
        require(reader.attachments['payload.txt'][0] == attachment.read_bytes(), 'attachment bytes changed')
        extraction = out / 'extracted'; extraction.mkdir(exist_ok=True)
        existing = extraction / 'payload.txt'; existing.write_bytes(b'KEEP')
        run('--attachments', attached, extraction)
        require(existing.read_bytes() == b'KEEP', 'attachment extraction overwrote existing file')
        require(any(path.read_bytes() == attachment.read_bytes() for path in extraction.glob('payload*.txt')), 'attachment was not extracted')
        run('--detach', attached, detached, 'all')
        require(not PdfReader(detached).attachments, 'detached files remain')
        return 'metadata, nested bookmark destination, labels and attachment bytes preserved; extraction avoids overwrite'
    check('Document parts and attachments', document_parts)

    def crop_size():
        cropped, sized = out / 'cropped.pdf', out / 'sized.pdf'
        run('--crop', source, cropped, '10,20,30,40', '2')
        reader = PdfReader(cropped)
        require(list(reader.pages[0].cropbox) == [0, 0, 400, 500], 'unselected page was cropped')
        require(list(reader.pages[1].cropbox) == [10, 40, 370, 480], list(reader.pages[1].cropbox))
        run('--page-size', source, sized, '210x297', 'extend', '1')
        reader = PdfReader(sized)
        require(abs(float(reader.pages[0].mediabox.width) - 210 * 72 / 25.4) < 0.1, 'page width incorrect')
        require('SECRET-ONE' in text(sized) and float(reader.pages[1].mediabox.width) == 400, 'resizing changed content/scope')
        return 'crop margins and page-size scope verified from PDF boxes'
    check('Crop and page size', crop_size)

    def links():
        linked = out / 'linked.pdf'
        run('--link', source, linked, '40,100,90,30@1', 'https://example.com/a', '150,100,90,30@1', '3:FitH')
        reader = PdfReader(linked)
        links = [ref.get_object() for ref in reader.pages[0]['/Annots']]
        require(len(links) == 2, 'missing links')
        require(str(links[0]['/A']['/URI']) == 'https://example.com/a', links[0])
        destination = links[1]['/A']['/D']
        require(destination[0] == reader.pages[2].indirect_reference and str(destination[1]) == '/FitH', destination)
        return 'URI and internal FitH destination point at the expected targets'
    check('Link destinations', links)

    def split_merge():
        directory = out / 'split'; directory.mkdir(exist_ok=True)
        run('--split', source, directory, '1-2,3')
        first, second = directory / 'fixture — 1-2.pdf', directory / 'fixture — 3.pdf'
        require(len(PdfReader(first).pages) == 2 and len(PdfReader(second).pages) == 1, 'split page counts')
        merged = out / 'merged.pdf'; run('--merge', merged, second, first)
        reader = PdfReader(merged)
        require([page.extract_text().strip() for page in reader.pages] == ['VISIBLE-THREE', 'SECRET-ONE', 'VISIBLE-TWO'], 'merged page order')
        return 'split scopes and reordered merge retain all selected pages'
    check('Split and merge pages', split_merge)

    def overlays():
        numbered, header, bates, marked = [out / name for name in ['numbers.pdf', 'headers.pdf', 'bates.pdf', 'watermark.pdf']]
        run('--number-pages', source, numbered, '1', '0')
        require('Page 1 of 3' in text(numbered), text(numbered))
        run('--header-footer', source, header, '|AUDIT HEADER|', '||<<page>>', '2')
        reader = PdfReader(header)
        require('AUDIT HEADER' not in reader.pages[0].extract_text() and 'AUDIT HEADER' in reader.pages[1].extract_text(), 'header page scope')
        run('--bates', source, bates, 'CASE-', '7', '4')
        require(all(token in text(bates) for token in ['CASE-0007', 'CASE-0008', 'CASE-0009']), text(bates))
        run('--watermark', source, marked, 'AUDIT', '2')
        require(len(PdfReader(marked).pages) == 3 and 'VISIBLE-TWO' in text(marked), 'watermark lost content')
        require(PdfReader(marked).pages[1].get_contents().get_data() != PdfReader(source).pages[1].get_contents().get_data(), 'watermark did not add page drawing commands')
        return 'page numbering, scoped header, sequential Bates and watermark drawing commands verified'
    check('Page overlays', overlays)

    def exports():
        for suffix in ['txt', 'rtf', 'html', 'docx']:
            target = out / ('export.' + suffix)
            run('--export', source, target)
            if suffix == 'docx':
                with ZipFile(target) as archive: content = archive.read('word/document.xml').decode()
            else: content = target.read_text(errors='replace')
            require(all(token in content for token in ['SECRET-ONE', 'VISIBLE-TWO', 'VISIBLE-THREE']), suffix + ' lost text')
        return 'TXT, RTF, HTML and DOCX contain all three pages of source text'
    check('Text exports', exports)

    def compression():
        target = out / 'compressed.pdf'
        run('--compress', source, target, 'balanced')
        require(target.stat().st_size <= source.stat().st_size and text(target) == text(source), 'compression changed text or enlarged file')
        return 'compressed/no-gain output is written, readable, complete and no larger than the source'
    check('Compression output', compression)

    def failures():
        target = out / 'preserved.pdf'
        cases = [('--watermark', source, target, 'AUDIT', '0'), ('--split', source, out / 'invalid-split', 'every:0'),
                 ('--merge', target, source, out / 'missing.pdf'), ('--redact', source, target, 'nan,1,2,3'),
                 ('--redact', source, target, '1,2,3,4@-9223372036854775808'),
                 ('--crop', source, target, 'nan,0,0,0'), ('--page-size', source, target, '1e100x297'),
                 ('--link', source, target, '1,2,3,4@0', 'https://example.com'),
                 ('--props', source, target, 'openpage=-9223372036854775808'),
                 ('--bates', source, target, 'CASE-', '9223372036854775807'),
                 ('--export', source, target.with_suffix('.unsupported')), ('--compress', source, target, 'unknown')]
        for arguments in cases:
            target.write_bytes(b'KEEP EXISTING DESTINATION')
            run(*arguments, success=False)
            require(target.read_bytes() == b'KEEP EXISTING DESTINATION', f'{arguments[0]} changed existing destination')
        run('--merge', out, source, success=False)
        return f'{len(cases)} invalid operations reject input without replacing an existing destination; directory write fails'
    check('Failure preservation and numeric bounds', failures)

    if args.skip_signing:
        report['checks'].append({'name': 'Isolated digital signatures', 'status': 'SKIP', 'detail': '--skip-signing explicitly requested'})
    else:
        def signatures():
            openssl = shutil.which('openssl')
            require(openssl, 'openssl is required; pass --skip-signing to record an explicit skip')
            key, certificate, identity = [out / name for name in ['audit-key.pem', 'audit-cert.pem', 'audit-identity.p12']]
            try:
                subprocess.run([openssl, 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(key), '-out', str(certificate), '-days', '1', '-subj', '/CN=PDF42 Local Audit'], check=True, capture_output=True, timeout=30)
                key.chmod(0o600)
                subprocess.run([openssl, 'pkcs12', '-export', '-out', str(identity), '-inkey', str(key), '-in', str(certificate), '-passout', 'pass:audit-password', '-keypbe', 'PBE-SHA1-3DES', '-certpbe', 'PBE-SHA1-3DES', '-macalg', 'sha1'], check=True, capture_output=True, timeout=30)
                identity.chmod(0o600)
                signed, twice, tampered = [out / name for name in ['signed.pdf', 'twice-signed.pdf', 'tampered.pdf']]
                run('--sign', source, identity, 'audit-password', signed, 'visible')
                first = run('--verify', signed)
                require('invalid' not in first.lower() and 'whole=true' in first and 'PDF42 Local Audit' in first, first)
                run('--sign', signed, identity, 'audit-password', twice)
                second = run('--verify', twice)
                rows = [line for line in second.splitlines() if ' | whole=' in line]
                require(len(rows) == 2 and all('invalid' not in row.lower() for row in rows) and 'whole=true' in rows[-1], second)
                changed = bytearray(twice.read_bytes()); changed[7] = ord('7') if changed[7] != ord('7') else ord('6'); tampered.write_bytes(changed)
                detected = run('--verify', tampered)
                require('invalid' in detected.lower(), detected)
                return 'temporary self-signed CMS, visible appearance, incremental second signature and same-length tamper detection verified; no TSA/Keychain'
            finally:
                for path in [key, certificate, identity]: path.unlink(missing_ok=True)
        check('Isolated digital signatures', signatures)

    report['finished_at'] = datetime.now(timezone.utc).isoformat()
    (out / 'result.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    totals = {status: sum(item['status'] == status for item in report['checks']) for status in ['PASS', 'FAIL', 'SKIP']}
    print(f"{totals['PASS']} passed, {totals['FAIL']} failed, {totals['SKIP']} skipped; {out / 'result.json'}", flush=True)
    return 1 if totals['FAIL'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
