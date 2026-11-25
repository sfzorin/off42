"""Real app renders for the demo video in every interface language (K42_SNAPSHOT, isolated *Audit bundles).
Usage: render_i18n.py LANG[,LANG…] [name…]. Captions/products/typing come from video-i18n/<lang>.json."""
import json, os, re, subprocess, sys
from pathlib import Path

V = Path(__file__).resolve().parent; S = V / 'shots'; apps = S / 'audit/apps'; docs = S / 'docs'; out = V / 'raw'
out.mkdir(exist_ok=True)
REPO = Path(os.environ['OFF42_REPO'])
I18N = Path(os.environ.get('VIDEO_I18N', V / 'video-i18n'))
hero = str(REPO / 'website/public/assets/office-still-life.png')

def rows(path):
    text = path.read_text()
    return {m[0]: m for m in (re.findall(r'"((?:[^"\\]|\\.)*)"', line) for line in text.splitlines()) if len(m) >= 3}

LEGACY = {**rows(REPO / 'Sources/Kit42/Strings.swift'), **rows(REPO / 'Sources/Kit42/ProductionStrings.swift')}

def T(key, lang):
    """The interface string the app shows for `key` (same lookup order as Kit42.Localization)."""
    if lang == 'en': return key
    if lang in ('ru', 'tr'):
        row = LEGACY.get(key); return row[1 if lang == 'ru' else 2] if row else key
    catalog = json.loads((REPO / f'Sources/Kit42/Resources/Translations/{lang}.json').read_text())
    return catalog.get(key, key)

def make_products(lang, copy):
    import openpyxl
    from openpyxl.styles import Font, PatternFill
    target = docs / f'products-{lang}.xlsx'
    wb = openpyxl.Workbook(); ws = wb.active; ws.title = copy['headers'][0][:30]
    ws.append(copy['headers'])
    for c in ws[1]: c.font = Font(bold=True, color='FFFFFF'); c.fill = PatternFill('solid', fgColor='2F5D50')
    for i, name in enumerate(copy['names']):
        sku = f'NL-{24017 + i * 9 + (i * i) % 7:05d}'
        ws.append([name, sku, f'B-{2611 + i // 2}', f'https://example.com/p/{sku}'])
    for col, w in zip('ABCD', (32, 13, 10, 36)): ws.column_dimensions[col].width = w
    ws.sheet_view.rightToLeft = lang == 'ar'
    wb.save(target)

def plan(lang):
    d = lambda n, ext: str(docs / f'{n}-{lang}.{ext}')
    sample = json.loads((V / 'samples' / f'{lang}.json').read_text())
    copy = json.loads((I18N / f'{lang}.json').read_text())
    make_products(lang, copy['products'])
    big = '1320x860'
    xls = lambda sel, action=None: {'K42_OPEN': d('products', 'xlsx'), 'K42_SIZE': big, 'K42_SELECT': sel, **({'K42_ACTION': action} if action else {})}
    # Keep writing the letter: replace the end of its first body paragraph with itself plus more of the new sentence.
    paragraph = sample['letter']['body'][1]
    anchor = paragraph[-28:].lstrip()
    typed = copy['typing']
    assert ';' not in anchor + typed, 'K42_SCRIPT separates steps with semicolons'
    steps = [round(len(typed) * k / 5) for k in range(1, 6)]
    shots = [(f'doc-type{i + 1}-{lang}', 'DOC42', [d('letter', 'docx')], {'K42_SIZE': big, 'K42_SCRIPT': f'find {anchor}; type {anchor}{typed[:n]}'})
             for i, n in enumerate(steps)]
    mark = 'EAC' if lang == 'ru' else 'CE'
    shots += [
        (f'doc-letter-{lang}', 'DOC42', [d('letter', 'docx')], {'K42_SIZE': big}),
        (f'xls-formula-{lang}', 'XLS42', [], {'K42_OPEN': d('budget', 'xlsx'), 'K42_SIZE': big, 'K42_SELECT': 'H8'}),
        (f'ppt-deck-{lang}', 'PPT42', [d('deck', 'pptx')], {'K42_SLIDE': '2', 'K42_SIZE': big}),
        (f'ppt-presenter-{lang}', 'PPT42', [d('deck', 'pptx')], {'K42_SLIDE': '2', 'K42_SIZE': big, 'K42_PRESENTER': '1', 'K42_SNAPSHOT_DELAY': '3'}),
        (f'pdf-edit-{lang}', 'PDF42', [d('agreement', 'pdf')], {'K42_SIDEBAR': 'thumbnails', 'K42_SIZE': big, 'K42_EDIT_TEXT': sample['title']}),
        (f'pdf-compress-win-{lang}', 'PDF42', [str(docs / 'portfolio-en.pdf')], {'K42_SIDEBAR': 'thumbnails', 'K42_SIZE': big}),
        (f'pdf-compress-{lang}', 'PDF42', [str(docs / 'portfolio-en.pdf')], {'K42_SIDEBAR': 'thumbnails', 'K42_SIZE': big, 'K42_COMPRESS': '1', 'K42_SNAPSHOT_SHEET': '1'}),
        (f'scn-scan-{lang}', 'SCN42', [], {'K42_FAKE_SCANNER': str(S / f'scanner-{lang}'), 'K42_AUTOSCAN': 'feeder', 'K42_SIZE': big, 'K42_SNAPSHOT_DELAY': '9'}),
        (f'xls-products-{lang}', 'XLS42', [], xls('A1:D11')),
        (f'xls-qr-{lang}', 'XLS42', [], {**xls('D2', 'insertTechnicalElementAction:'), 'K42_SNAPSHOT_KEEP': '1', 'K42_DIALOG_DELAY': '1',
            'K42_DIALOG_STEPS': f"click:{T('Show readable value below code', lang)};field:{T('Width in millimetres', lang)}=30"}),
        (f'xls-mark-{lang}', 'XLS42', [], {**xls('B2', 'insertTechnicalElementAction:'), 'K42_SNAPSHOT_KEEP': '1', 'K42_DIALOG_DELAY': '1',
            'K42_DIALOG_STEPS': f"segment:{T('Symbols', lang)};field:{T('Find a technical symbol', lang)}={mark};field:{T('Width in millimetres', lang)}=25"}),
        (f'xls-labels-{lang}', 'XLS42', [], {**xls('A1:D11', 'createLabelsAction:'), 'K42_SNAPSHOT_KEEP': '1', 'K42_DIALOG_DELAY': '4',
            'K42_DIALOG_STEPS': f"popup:{T('None', lang)}={copy['products']['headers'][1]};click:{T('Update Preview', lang)}",
            'K42_DIALOG_PDF': str(out / f'xls-labels-{lang}.pdf')}),
    ]
    return shots

def run(lang, only):
    results = []
    for name, app, args, env in plan(lang):
        if only and name not in only: continue
        target = out / f'{name}.png'; target.unlink(missing_ok=True)
        Path(str(target).replace('.png', '-dialog.png')).unlink(missing_ok=True)
        e = {'K42_APPEARANCE': 'light', 'K42_SNAPSHOT': str(target), 'K42_UI_TEST_DIR': str(V / 'tmp'), 'K42_SCAN_DIR': str(V / 'tmp'), **env}
        cmd = ['/usr/bin/open', '-n', '-W', '-a', str(apps / f'{app}Audit.app')]
        for k, v in e.items(): cmd += ['--env', f'{k}={v}']
        cmd += ['--args', *args, '-Suite42Language', lang]
        try: subprocess.run(cmd, timeout=90, capture_output=True)
        except subprocess.TimeoutExpired: subprocess.run(['pkill', '-f', f'{app}Audit.app'])
        ok = target.is_file()
        if name.startswith(('xls-qr', 'xls-mark', 'xls-labels')): ok = ok and Path(str(target).replace('.png', '-dialog.png')).is_file()
        results.append(ok); print('PASS' if ok else 'FAIL', name, flush=True)
    pdf = out / f'xls-labels-{lang}.pdf'
    if pdf.is_file():
        subprocess.run(['sips', '-s', 'format', 'png', '-Z', '2200', str(pdf), '--out', str(out / f'labels-page-{lang}.png')], capture_output=True)
    return results

if __name__ == '__main__':
    langs = sys.argv[1].split(','); only = set(sys.argv[2:]); total = []
    for lang in langs: total += run(lang, only)
    print(f'{sum(total)}/{len(total)}')
