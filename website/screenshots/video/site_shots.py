"""Website images for the codes section: window + Create Labels sheet (with the PDF its preview showed), and the
Insert Symbol or Code dialog. Writes WebP at two widths and updates sizes.json."""
import json, sys
from pathlib import Path
from PIL import Image, ImageDraw
V = Path(__file__).resolve().parent; R = V / 'raw'
SITE = Path(__import__('os').environ['OFF42_REPO']) / 'website'; out = SITE / 'public/assets/shots'
sizes = json.loads((SITE / 'screenshots/sizes.json').read_text())
def flat(p):
    im = Image.open(p).convert('RGBA'); f = Image.new('RGB', im.size, 'white'); f.paste(im, (0, 0), im); return f
def rounded(im, r):
    m = Image.new('L', im.size, 0); ImageDraw.Draw(m).rounded_rectangle((0, 0, im.width - 1, im.height - 1), r, fill=255)
    o = im.convert('RGBA'); o.putalpha(m); return o
for lang in sys.argv[1:]:
    win = flat(R / f'xls-products-{lang}.png'); sheet = flat(R / f'xls-labels-{lang}-dialog.png'); page = flat(R / f'labels-page-{lang}.png')
    area = (860, 22, 1880, 1262); pw = area[2] - area[0] - 24
    p = page.resize((pw, round(page.height * pw / page.width)), Image.LANCZOS).crop((0, 0, pw, area[3] - area[1] - 24))
    sheet.paste(Image.new('RGB', (area[2] - area[0], area[3] - area[1]), (236, 236, 236)), area[:2]); sheet.paste(p, (area[0] + 12, area[1] + 12))
    x, y = (win.width - sheet.width) // 2, 216
    shadow = Image.new('RGBA', win.size, (0, 0, 0, 0)); ImageDraw.Draw(shadow).rectangle((x - 2, y, x + sheet.width + 2, y + sheet.height + 6), fill=(0, 0, 0, 40))
    from PIL import ImageFilter
    win = win.convert('RGBA'); win.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(14)))
    s = rounded(sheet, 18); s.paste(sheet.crop((0, 0, sheet.width, 18)), (0, 0)); win.paste(s, (x, y), s)
    shots = {f'codes-labels-{lang}': win.convert('RGB'), f'codes-qr-{lang}': flat(R / f'xls-qr-{lang}-dialog.png')}
    for name, im in shots.items():
        small = im.width < 2000; widths = (640, 1100) if small else (900, 1600)
        for w in widths:
            im.resize((w, round(im.height * w / im.width)), Image.LANCZOS).save(out / f'{name}-{w}.webp', 'WEBP', quality=86, method=6)
        sizes[name] = [im.width, im.height]; print(name, im.size)
(SITE / 'screenshots/sizes.json').write_text(json.dumps(sizes, indent=None if '\n' not in (SITE / 'screenshots/sizes.json').read_text().strip() else 1) + '\n')
