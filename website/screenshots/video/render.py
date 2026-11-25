"""Real app renders (K42_SNAPSHOT, isolated *Audit bundles) for the codes section and the demo video."""
import json, os, subprocess, sys
from pathlib import Path
V = Path(__file__).resolve().parent; S = V/'shots'; apps = S/'audit/apps'; docs = S/'docs'; out = V/'raw'; out.mkdir(exist_ok=True)
hero = os.environ['OFF42_REPO'] + '/website/public/assets/office-still-life.png'
def plan(lang):
    d = lambda n, ext: str(docs/f'{n}-{lang}.{ext}')
    c = json.loads((V/'samples'/f'{lang}.json').read_text()); seal = c['parties'][0]['stamp']['id']
    big = '1320x860'
    xls = lambda sel, action=None: {'K42_OPEN': d('products','xlsx'), 'K42_SIZE': big, 'K42_SELECT': sel, **({'K42_ACTION': action} if action else {})}
    return [
      (f'xls-products-{lang}', 'XLS42', [], xls('A1:D11')),
      (f'xls-qr-{lang}', 'XLS42', [], {**xls('D2', 'insertTechnicalElementAction:'), 'K42_SNAPSHOT_KEEP': '1', 'K42_DIALOG_STEPS': {'en': 'click:Show readable value below code;field:Width in millimetres=30', 'ru': 'click:Показывать значение под кодом;field:Ширина в миллиметрах=30'}[lang], 'K42_DIALOG_DELAY': '1'}),
      (f'xls-symbol-{lang}', 'XLS42', [], {**xls('B2', 'insertTechnicalElementAction:'), 'K42_SNAPSHOT_KEEP': '1', 'K42_DIALOG_STEPS': 'segment:Symbols;popup:Technical symbol=Class II / double insulation;field:Width in millimetres=30', 'K42_DIALOG_DELAY': '1'}),
      (f'xls-mark-{lang}', 'XLS42', [], {**xls('B2', 'insertTechnicalElementAction:'), 'K42_SNAPSHOT_KEEP': '1', 'K42_DIALOG_STEPS': {'en': 'segment:Symbols;field:Find a technical symbol=CE;field:Width in millimetres=25', 'ru': 'segment:Знаки;field:Найти технический знак=EAC;field:Ширина в миллиметрах=25'}[lang], 'K42_DIALOG_DELAY': '1'}),
      (f'pdf-compress-win-{lang}', 'PDF42', [d('portfolio','pdf')], {'K42_SIDEBAR': 'thumbnails', 'K42_SIZE': big}),
      (f'xls-labels-{lang}', 'XLS42', [], {**xls('A1:D11', 'createLabelsAction:'), 'K42_SNAPSHOT_KEEP': '1', 'K42_DIALOG_STEPS': {'en': 'popup:None=SKU;click:Update Preview', 'ru': 'popup:Нет=Артикул;click:Обновить предпросмотр'}[lang], 'K42_DIALOG_DELAY': '4', 'K42_DIALOG_PDF': str(out/f'xls-labels-{lang}.pdf')}),
      (f'doc-type0-{lang}', 'DOC42', [d('letter','docx')], {'K42_SIZE': big, 'K42_SCRIPT': 'find a website that puts your documentaries first.'}),
      (f'doc-type1-{lang}', 'DOC42', [d('letter','docx')], {'K42_SIZE': big, 'K42_SCRIPT': 'find a website that puts your documentaries first.; type a website that puts your documentaries first. We can a'}),
      (f'doc-type2-{lang}', 'DOC42', [d('letter','docx')], {'K42_SIZE': big, 'K42_SCRIPT': 'find a website that puts your documentaries first.; type a website that puts your documentaries first. We can also film '}),
      (f'doc-type3-{lang}', 'DOC42', [d('letter','docx')], {'K42_SIZE': big, 'K42_SCRIPT': 'find a website that puts your documentaries first.; type a website that puts your documentaries first. We can also film a short st'}),
      (f'doc-type4-{lang}', 'DOC42', [d('letter','docx')], {'K42_SIZE': big, 'K42_SCRIPT': 'find a website that puts your documentaries first.; type a website that puts your documentaries first. We can also film a short studio intro'}),
      (f'doc-type5-{lang}', 'DOC42', [d('letter','docx')], {'K42_SIZE': big, 'K42_SCRIPT': 'find a website that puts your documentaries first.; type a website that puts your documentaries first. We can also film a short studio intro for your channel.'}),
      (f'xls-formula-{lang}', 'XLS42', [], {'K42_OPEN': d('budget','xlsx'), 'K42_SIZE': big, 'K42_SELECT': 'H8'}),
      (f'ppt-presenter-{lang}', 'PPT42', [d('deck','pptx')], {'K42_SLIDE': '2', 'K42_SIZE': big, 'K42_PRESENTER': '1', 'K42_SNAPSHOT_DELAY': '3'}),
      (f'pdf-edit-{lang}', 'PDF42', [d('agreement','pdf')], {'K42_SIDEBAR': 'thumbnails', 'K42_SIZE': big, 'K42_EDIT_TEXT': 'Design Services Agreement'}),
      (f'pdf-compress-{lang}', 'PDF42', [d('portfolio','pdf')], {'K42_SIDEBAR': 'thumbnails', 'K42_SIZE': big, 'K42_COMPRESS': '1', 'K42_SNAPSHOT_SHEET': '1'}),
      (f'pdf-contract-{lang}', 'PDF42', [d('agreement','pdf')], {'K42_SIDEBAR': 'thumbnails', 'K42_SIZE': big}),
      (f'pdf-signature-{lang}', 'PDF42', [d('agreement','pdf')], {'K42_OPEN_GENERATOR': 'signature', 'K42_SNAPSHOT_SHEET': '1', 'K42_GENERATOR_NAME': c['generatorName'], 'K42_GENERATOR_DESIGNS': '2424868,2425117,2425381', 'K42_SIZE': big}),
      (f'pdf-signature-win-{lang}', 'PDF42', [d('agreement','pdf')], {'K42_OPEN_GENERATOR': 'signature', 'K42_GENERATOR_NAME': c['generatorName'], 'K42_GENERATOR_DESIGNS': '2424868,2425117,2425381', 'K42_SIZE': big}),
      (f'pdf-seal-{lang}', 'PDF42', [d('agreement','pdf')], {'K42_OPEN_GENERATOR': 'seal', 'K42_SNAPSHOT_SHEET': '1', 'K42_SIZE': big, 'K42_GENERATOR_SEAL': seal}),
      (f'doc-letter-{lang}', 'DOC42', [d('letter','docx')], {'K42_SIZE': big}),
      (f'xls-budget-{lang}', 'XLS42', [], {'K42_OPEN': d('budget','xlsx'), 'K42_SIZE': big}),
      (f'ppt-deck-{lang}', 'PPT42', [d('deck','pptx')], {'K42_SLIDE': '2', 'K42_SIZE': big}),
      (f'img-markup-{lang}', 'IMG42', [hero], {'K42_ADJUST': '1', 'K42_SIZE': big}),
      (f'scn-scan-{lang}', 'SCN42', [], {'K42_FAKE_SCANNER': str(S/f'scanner-{lang}'), 'K42_AUTOSCAN': 'feeder', 'K42_SIZE': big, 'K42_SNAPSHOT_DELAY': '9'}),
    ]
only = set(sys.argv[2:]); res = []
for lang in sys.argv[1].split(','):
    for name, app, args, env in plan(lang):
        if only and name not in only: continue
        t = out/f'{name}.png'; t.unlink(missing_ok=True); Path(str(t).replace('.png','-dialog.png')).unlink(missing_ok=True)
        e = {'K42_APPEARANCE': 'light', 'K42_SNAPSHOT': str(t), 'K42_UI_TEST_DIR': str(V/'tmp'), 'K42_SCAN_DIR': str(V/'tmp'), **env}
        cmd = ['/usr/bin/open', '-n', '-W', '-a', str(apps/f'{app}Audit.app')]
        for k, v in e.items(): cmd += ['--env', f'{k}={v}']
        cmd += ['--args', *args, '-Suite42Language', lang]
        try: subprocess.run(cmd, timeout=90, capture_output=True)
        except subprocess.TimeoutExpired: subprocess.run(['pkill', '-f', f'{app}Audit.app'])
        ok = t.is_file(); res.append(ok); print('PASS' if ok else 'FAIL', name, sorted(p.name for p in out.glob(name+'*')), flush=True)
print(f'{sum(res)}/{len(res)}')
