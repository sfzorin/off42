import os, sys, json
from pathlib import Path
import numpy as np
from PIL import Image
S = Path(__file__).resolve().parent
website = Path(os.environ.get('OFF42_WEBSITE', S.parent))
out = website / 'public/assets/shots'; out.mkdir(parents=True, exist_ok=True)
manifest = website / 'screenshots/sizes.json'
sizes = json.loads(manifest.read_text()) if manifest.exists() else {}
for lang in sys.argv[1:]:
    for p in sorted((S / 'raw').glob(f'*-{lang}.png')):
        im = Image.open(p).convert('RGB'); sizes[p.stem] = [im.width, im.height]
        for w in ((1600, 900) if im.width > 2000 else (1100, 640)):
            im.resize((w, round(im.height * w / im.width)), Image.LANCZOS).save(out / f'{p.stem}-{w}.webp', 'WEBP', quality=82, method=6)
    # Signed section of the agreement, from the 4x render.
    im = Image.open(S / f'docs/hi-{lang}.png').convert('RGB'); a = np.asarray(im).astype(int)
    ink = ((a[:, :, 2] - a[:, :, 0] > 40) & (a[:, :, 2] > 120)) | ((a[:, :, 0] - a[:, :, 2] > 60) & (a[:, :, 0] > 150) & (a[:, :, 1] < 120))
    rows = np.where(ink.sum(1) > 3)[0]; rows = rows[rows > 1400]
    dark = a.sum(2) < 450; drows = np.where(dark.sum(1) > 3)[0]; drows = drows[(drows > rows.min()) & (drows < rows.max() + 240)]
    c = im.crop((144, rows.min() - 380, im.width - 120, max(rows.max(), drows.max()) + 56))
    c.save(S / f'docs/signed-{lang}.png'); sizes[f'signed-{lang}'] = [c.width, c.height]
    for w in (1600, 900):
        c.resize((w, round(c.height * w / c.width)), Image.LANCZOS).save(out / f'signed-{lang}-{w}.webp', 'WEBP', quality=86, method=6)
    print(lang, 'signed', c.size)
manifest.write_text(json.dumps(dict(sorted(sizes.items())), indent=1) + '\n')
