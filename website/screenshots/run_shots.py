"""Website screenshots of OFF42 apps through the isolated audit bundles (com.sfzorin.uiaudit.*), rendered by the apps
themselves (K42_SNAPSHOT) — no screen recording, the user's own apps and documents are not touched."""
import os, subprocess, sys
from pathlib import Path

S = Path(__file__).resolve().parent
apps = S / 'audit/apps'; docs = S / 'docs'; out = S / 'raw'; out.mkdir(exist_ok=True)
website = Path(os.environ.get('OFF42_WEBSITE', S.parent))
hero = website / 'public/assets/office-still-life.png'
samples = Path(os.environ.get('OFF42_SAMPLES', S / 'samples' if (S / 'samples').is_dir() else S.parent / 'samples'))
only = set(sys.argv[1:])
LANGS = os.environ.get('SHOT_LANGS', 'en,ru').split(',')

def shots(lang):
    d = lambda name: str(docs / f'{name}-{lang}.{ "pdf" if name == "agreement" else {"letter":"docx","budget":"xlsx","deck":"pptx"}[name]}')
    import json
    c = json.loads((samples / f'{lang}.json').read_text())
    gname = c['generatorName']; seal_id = c['parties'][0]['stamp']['id']
    big = '1320x860'
    return [
        (f'pdf-contract-{lang}', 'PDF42', [d('agreement')], {'K42_SIDEBAR': 'thumbnails', 'K42_SIZE': big}),
        (f'pdf-signature-{lang}', 'PDF42', [d('agreement')], {'K42_OPEN_GENERATOR': 'signature', 'K42_SNAPSHOT_SHEET': '1', 'K42_GENERATOR_NAME': gname, 'K42_GENERATOR_DESIGNS': '2424853,2425117,2425381' if lang == 'ru' else '2424868,2425117,2425381', 'K42_SIZE': big}),
        (f'pdf-seal-{lang}', 'PDF42', [d('agreement')], {'K42_OPEN_GENERATOR': 'seal', 'K42_SNAPSHOT_SHEET': '1', 'K42_SIZE': big, 'K42_GENERATOR_SEAL': seal_id}),
        (f'doc-letter-{lang}', 'DOC42', [d('letter')], {'K42_SIZE': big}),
        (f'xls-budget-{lang}', 'XLS42', [], {'K42_OPEN': d('budget'), 'K42_SIZE': big}),
        (f'ppt-deck-{lang}', 'PPT42', [d('deck')], {'K42_SLIDE': '2', 'K42_SIZE': big}),
        (f'img-markup-{lang}', 'IMG42', [str(hero)], {'K42_ADJUST': '1', 'K42_SIZE': big}),
        (f'scn-scan-{lang}', 'SCN42', [], {'K42_FAKE_SCANNER': str(S / f'scanner-{lang}'), 'K42_AUTOSCAN': 'feeder', 'K42_SIZE': big, 'K42_SNAPSHOT_DELAY': '9'}),
    ]

results = []
for lang in LANGS:
    for name, app, args, env in shots(lang):
        if only and name not in only: continue
        target = out / f'{name}.png'; target.unlink(missing_ok=True)
        e = {'K42_APPEARANCE': 'light', 'K42_SNAPSHOT': str(target), 'K42_UI_TEST_DIR': str(S / 'tmp'), 'K42_SCAN_DIR': str(S / 'tmp'), **env}
        cmd = ['/usr/bin/open', '-n', '-W', '-a', str(apps / f'{app}Audit.app')]
        for k, v in e.items(): cmd += ['--env', f'{k}={v}']
        cmd += ['--args', *args, '-Suite42Language', lang]
        try:
            subprocess.run(cmd, timeout=90, capture_output=True)
        except subprocess.TimeoutExpired:
            pass
        ok = target.is_file() and target.stat().st_size > 1000
        results.append(ok); print('PASS' if ok else 'FAIL', name, flush=True)
print(f'{sum(results)}/{len(results)}')
if not results or not all(results):
    sys.exit(1)
