"""OFF42 demo video: real app renders (K42_SNAPSHOT) composed with camera moves, macOS-style sheets,
a pointer and captions. Writes raw RGB frames to ffmpeg. Usage: compose.py OUT.mp4 [--preview T1,T2,…]"""
import math, subprocess, sys
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter, ImageFont

V = Path(__file__).resolve().parent; R = V / 'raw'
SITE = Path(__import__('os').environ['OFF42_REPO']) / 'website/public/assets'
W, H, FPS = 1920, 1080, 30
BG = (247, 246, 242); INK = (37, 42, 38); MUTED = (99, 104, 95); CORAL = (221, 87, 56)
APP = {'PDF42': (184, 81, 53), 'DOC42': (74, 116, 145), 'XLS42': (88, 129, 68), 'PPT42': (185, 128, 56),
       'IMG42': (140, 105, 164), 'SCN42': (70, 136, 119)}
HN = '/System/Library/Fonts/HelveticaNeue.ttc'
font = lambda size, face=10: ImageFont.truetype(HN, size, index=face)  # 10 Medium, 1 Bold, 0 Regular

def ease(x): x = min(1, max(0, x)); return x * x * (3 - 2 * x)
def ease_out(x): x = min(1, max(0, x)); return 1 - (1 - x) ** 3
def lerp(a, b, t): return a + (b - a) * t

# ---------- world building ----------
MARGIN = 360
def rounded(im, r):
    mask = Image.new('L', im.size, 0); ImageDraw.Draw(mask).rounded_rectangle((0, 0, im.width - 1, im.height - 1), r, fill=255)
    out = im.convert('RGBA'); out.putalpha(mask); return out

def shadow(size, r, blur, alpha):
    pad = blur * 3
    s = Image.new('RGBA', (size[0] + 2 * pad, size[1] + 2 * pad), (0, 0, 0, 0))
    ImageDraw.Draw(s).rounded_rectangle((pad, pad, pad + size[0], pad + size[1]), r, fill=(30, 35, 30, alpha))
    return s.filter(ImageFilter.GaussianBlur(blur)), pad

_cache = {}
def load(name):
    if name not in _cache:
        p = R / name if (R / name).exists() else Path(name)
        im = Image.open(p).convert('RGBA'); flat = Image.new('RGB', im.size, (255, 255, 255)); flat.paste(im, (0, 0), im)
        _cache[name] = flat
    return _cache[name]

def window_world(win, radius=22):
    im = load(win)
    world = Image.new('RGB', (im.width + 2 * MARGIN, im.height + 2 * MARGIN), BG)
    sh, pad = shadow(im.size, radius, 38, 70)
    world.paste(sh, (MARGIN - pad, MARGIN - pad + 18), sh)
    w = rounded(im, radius); world.paste(w, (MARGIN, MARGIN), w)
    return world

def with_sheet(world, sheet_img, top, progress):
    """Paste a sheet hanging from y=top of the window (window coords), sliding down as progress 0→1."""
    sheet = sheet_img if isinstance(sheet_img, Image.Image) else load(sheet_img)
    out = world.copy()
    win_w = world.width - 2 * MARGIN
    x = MARGIN + (win_w - sheet.width) // 2; y0 = MARGIN + top
    shift = round((1 - ease_out(progress)) * sheet.height)
    visible = sheet.height - shift
    if visible <= 0: return out
    part = sheet.crop((0, shift, sheet.width, sheet.height))
    sh, pad = shadow(part.size, 18, 26, 90)
    region = Image.new('RGBA', out.size, (0, 0, 0, 0)); region.paste(sh, (x - pad, y0 - pad + 10), sh)
    # sheets emerge from under the toolbar: clip the shadow above it
    clip = Image.new('L', out.size, 0); ImageDraw.Draw(clip).rectangle((0, y0, out.width, out.height), fill=255)
    region.putalpha(Image.composite(region.getchannel('A'), Image.new('L', out.size, 0), clip))
    out.paste(region, (0, 0), region)
    p = rounded(part, 18) if shift == 0 else part.convert('RGBA')
    if shift == 0:
        # flat top edge where the sheet meets the toolbar
        p.paste(part.crop((0, 0, part.width, 18)), (0, 0))
    out.paste(p, (x, y0), p)
    return out

def sheet_origin(world, sheet, top):
    sw = (sheet if isinstance(sheet, Image.Image) else load(sheet)).width
    return MARGIN + (world.width - 2 * MARGIN - sw) // 2, MARGIN + top

# ---------- camera ----------
VIEW = (0, 196, W, H)          # region below the caption band
VW, VH = VIEW[2] - VIEW[0], VIEW[3] - VIEW[1]
def fit(world, pad=0.0):
    """Camera (cx, cy, scale) framing the whole window, scale = screen px per world px."""
    ww, wh = world.width - 2 * MARGIN, world.height - 2 * MARGIN
    s = min((VW - 120) / ww, (VH - 60) / wh) * (1 - pad)
    return (world.width / 2, world.height / 2, s)
def focus(world, x, y, s): return (MARGIN + x, MARGIN + y, s)

def shoot(world, cam):
    cx, cy, s = cam
    w, h = VW / s, VH / s
    box = (cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2)
    return world.transform((VW, VH), Image.EXTENT, box, Image.BICUBIC, fillcolor=BG)

def to_screen(cam, wx, wy):
    cx, cy, s = cam
    return VIEW[0] + VW / 2 + (wx - cx) * s, VIEW[1] + VH / 2 + (wy - cy) * s

def mix_cam(a, b, t):
    t = ease(t)
    # interpolate scale geometrically so zooms feel even
    return (lerp(a[0], b[0], t), lerp(a[1], b[1], t), math.exp(lerp(math.log(a[2]), math.log(b[2]), t)))

# ---------- overlays ----------
CURSOR = [(0, 0), (0, 25), (6, 19), (10.5, 29), (14.5, 27.3), (10.2, 17.8), (18, 17.8)]
def draw_cursor(frame, x, y, press=0.0):
    d = ImageDraw.Draw(frame, 'RGBA')
    if press > 0:
        r = 10 + 34 * press
        d.ellipse((x - r, y - r, x + r, y + r), outline=(221, 87, 56, int(200 * (1 - press))), width=4)
        d.ellipse((x - 14, y - 14, x + 14, y + 14), fill=(221, 87, 56, int(70 * (1 - press))))
    k = 1.35
    pts = [(x + px * k, y + py * k) for px, py in CURSOR]
    sh = [(px + 2, py + 3) for px, py in pts]
    d.polygon(sh, fill=(0, 0, 0, 60))
    d.polygon(pts, fill=(20, 20, 20, 255), outline=(255, 255, 255, 255), width=3)
    d.polygon(pts, fill=(20, 20, 20, 255))

def caption(frame, app, title, alpha=1.0, sub=None):
    if alpha <= 0: return
    layer = Image.new('RGBA', (W, 196), (0, 0, 0, 0)); d = ImageDraw.Draw(layer)
    y = 58
    if app:
        col = APP.get(app, CORAL)
        d.rounded_rectangle((96, y, 96 + 14, y + 14), 3, fill=col + (255,))
        d.text((122, y - 5), app, font=font(22, 1), fill=col + (255,))
        y += 34
    d.text((96, y), title, font=font(50, 10), fill=INK + (255,))
    if sub: d.text((96, y + 64), sub, font=font(24, 0), fill=MUTED + (255,))
    layer.putalpha(layer.getchannel('A').point(lambda v: int(v * alpha)))
    frame.paste(layer, (0, 0), layer)

def brand(d, x, y, size, alpha=255):
    f1, f2 = font(size, 1), font(size, 10)
    d.text((x, y), 'off', font=f1, fill=INK + (alpha,))
    x2 = x + d.textlength('off', font=f1) - size * 0.04
    d.text((x2, y), '42', font=f2, fill=INK + (alpha,))
    x3 = x2 + d.textlength('42', font=f2) + size * 0.04
    cx, cy, r = x3 + size * 0.17, y + size * 0.2, size * 0.15
    for k in range(4):
        a = math.pi * k / 4
        d.line((cx - r * math.cos(a), cy - r * math.sin(a), cx + r * math.cos(a), cy + r * math.sin(a)), fill=CORAL + (alpha,), width=max(3, int(size * 0.055)))
    return x3 + size * 0.4

def brand_width(size):
    d = ImageDraw.Draw(Image.new('RGB', (1, 1)))
    return d.textlength('off', font=font(size, 1)) + d.textlength('42', font=font(size, 10)) + size * 0.34

def centered(d, y, text, f, fill):
    d.text(((W - d.textlength(text, font=f)) / 2, y), text, font=f, fill=fill)

# ---------- scenes ----------
class Scene:
    def __init__(self, dur, render): self.dur, self.render = dur, render

def window_scene(win, app, title, cams, sub=None, sheet=None, sheet_top=0, sheet_at=None, pointer=None, extra=None, extra_at=0.0):
    """cams: list of (time, cam) keyframes; pointer: list of (time, wx, wy, click?) in window coords;
    sheet: image/name shown from sheet_at (slide 0.45 s)."""
    base = window_world(win)
    plain = with_sheet(base, sheet, sheet_top, 1) if sheet else None
    full = extra(plain) if extra and plain is not None else plain
    def cam_at(t):
        for (t0, c0), (t1, c1) in zip(cams, cams[1:]):
            if t <= t1: return mix_cam(c0, c1, (t - t0) / max(1e-6, t1 - t0))
        return cams[-1][1]
    def render(t, frame):
        if sheet and t >= sheet_at:
            p = (t - sheet_at) / 0.45
            world = (full if t >= extra_at else plain) if p >= 1 else with_sheet(base, sheet, sheet_top, p)
        else: world = base
        cam = cam_at(t)
        frame.paste(shoot(world, cam), (VIEW[0], VIEW[1]))
        # soft fade under the caption band
        grad = Image.linear_gradient('L').resize((W, 60)).transpose(Image.FLIP_TOP_BOTTOM)
        frame.paste(Image.new('RGB', (W, 60), BG), (0, VIEW[1]), grad)
        caption(frame, app, title, ease(t / 0.5), sub)
        if pointer:
            pos = None
            for (t0, x0, y0, _), (t1, x1, y1, _) in zip(pointer, pointer[1:]):
                if t0 <= t <= t1: k = ease((t - t0) / max(1e-6, t1 - t0)); pos = (lerp(x0, x1, k), lerp(y0, y1, k)); break
            if pos is None and t >= pointer[-1][0]: pos = pointer[-1][1:3]
            if pos is not None and t >= pointer[0][0]:
                press = 0
                for (tc, _, _, click) in pointer:
                    if click and 0 <= t - tc <= 0.45: press = (t - tc) / 0.45
                sx, sy = to_screen(cam, MARGIN + pos[0], MARGIN + pos[1])
                draw_cursor(frame, sx, sy, press)
    return render, base

def labels_extra(world):
    """The label dialog's PDFView preview is layer-backed and missing from the window cache:
    draw the very PDF that preview showed (saved by K42_DIALOG_PDF), fitted to its width."""
    page = load('labels-page-en.png')
    sx, sy = sheet_origin(world, 'xls-labels-en-dialog.png', LABELS_TOP)
    area = (860, 22, 1880, 1262)
    pw = area[2] - area[0] - 24; ph = round(page.height * pw / page.width)
    p = page.resize((pw, ph), Image.LANCZOS).crop((0, 0, pw, area[3] - area[1] - 24))
    out = world.copy(); out.paste(Image.new('RGB', (area[2] - area[0], area[3] - area[1]), (236, 236, 236)), (sx + area[0], sy + area[1]))
    out.paste(p, (sx + area[0] + 12, sy + area[1] + 12))
    return out

PDF_TOP, XLS_TOP, LABELS_TOP = 132, 216, 216

def build():
    scenes = []
    # 0 — intro
    def intro(t, frame):
        d = ImageDraw.Draw(frame, 'RGBA')
        a = int(255 * ease(t / 0.6))
        brand(d, (W - brand_width(150)) / 2, 300, 150, a)
        a2 = int(255 * ease((t - 0.5) / 0.6))
        centered(d, 520, 'Six native apps for everyday office work.', font(48, 10), INK + (a2,))
        a3 = int(255 * ease((t - 1.0) / 0.6))
        x = (W - 6 * 190 + 60) / 2
        for name, col in APP.items():
            d.rounded_rectangle((x, 640, x + 14, 654), 3, fill=col + (a3,))
            d.text((x + 24, 633), name, font=font(26, 1), fill=col + (a3,)); x += 190
        a4 = int(255 * ease((t - 1.4) / 0.6))
        centered(d, 708, 'Free  ·  Works offline  ·  Open source', font(28, 0), MUTED + (a4,))
    scenes.append(Scene(4.0, intro))

    # 1 — PDF contract
    r, wd = window_scene('pdf-contract-en.png', 'PDF42', 'Open a contract and get it signed.',
                         [(0, fit(window_world('pdf-contract-en.png'), 0.05)), (4.5, focus(None, 1640, 1180, 0.82))])
    scenes.append(Scene(4.5, r))

    # 2 — signature generator
    sig = 'pdf-signature-en.png'; w0 = window_world('pdf-contract-en.png')
    sx = (2640 - load(sig).width) // 2
    r, _ = window_scene('pdf-contract-en.png', 'PDF42', 'Write a signature from a name.',
                        [(0, focus(None, 1640, 1180, 0.82)), (1.0, focus(None, 1320, 520, 0.95)), (5.5, focus(None, 1320, 470, 1.04))],
                        sub='Every insertion comes out slightly different, like real ink.',
                        sheet=sig, sheet_top=PDF_TOP, sheet_at=0.5,
                        pointer=[(1.2, 1900, 1100, False), (2.3, sx + 230, PDF_TOP + 425, True), (3.6, sx + 230, PDF_TOP + 425, False), (4.6, sx + 1181, PDF_TOP + 611, True), (5.5, sx + 1181, PDF_TOP + 611, False)])
    scenes.append(Scene(5.5, r))

    # 3 — stamp generator
    seal = 'pdf-seal-en.png'; sx2 = (2640 - load(seal).width) // 2
    r, _ = window_scene('pdf-contract-en.png', 'PDF42', 'Press a company stamp.',
                        [(0, focus(None, 1320, 820, 0.62)), (5.0, focus(None, 1320, 900, 0.68))],
                        sub='132 stamp styles: round seals, company stamps, “Paid”, “Received”.',
                        sheet=seal, sheet_top=PDF_TOP, sheet_at=0.2,
                        pointer=[(1.0, sx2 + 640, PDF_TOP + 1500, False), (3.6, sx2 + 1235, PDF_TOP + 1300, True), (5.0, sx2 + 1235, PDF_TOP + 1300, False)])
    scenes.append(Scene(5.0, r))

    # 4 — signed result
    signed = Image.open(SITE / 'shots/signed-en-1600.webp').convert('RGB')
    def signed_scene(t, frame):
        world = Image.new('RGB', (signed.width + 2 * MARGIN, signed.height + 2 * MARGIN), BG)
        sh, pad = shadow(signed.size, 10, 30, 60); world.paste(sh, (MARGIN - pad, MARGIN - pad + 14), sh)
        world.paste(rounded(signed, 10), (MARGIN, MARGIN), rounded(signed, 10))
        c0 = (world.width / 2, world.height / 2, (VW - 200) / signed.width); c1 = (world.width / 2, world.height / 2 + 20, (VW - 200) / signed.width * 1.12)
        frame.paste(shoot(world, mix_cam(c0, c1, t / 4.5)), (VIEW[0], VIEW[1]))
        caption(frame, 'PDF42', 'Signed and stamped. No printer, no scanner.', ease(t / 0.5), 'A drawn signature is not a certificate signature — PDF42 does both.')
    scenes.append(Scene(4.5, signed_scene))

    # 5 — QR code in XLS42
    qr = 'xls-qr-en-dialog.png'; qx = (2640 - load(qr).width) // 2
    r, _ = window_scene('xls-qr-en.png', 'XLS42', 'Turn a cell into a QR code.',
                        [(0, fit(window_world('xls-qr-en.png'), 0.02)), (0.9, focus(None, 1320, 620, 0.8)), (5.5, focus(None, 1320, 700, 0.86))],
                        sub='QR, Data Matrix, Code 128 and Aztec, sized in millimetres.',
                        sheet=qr, sheet_top=XLS_TOP, sheet_at=1.0,
                        pointer=[(0.0, 1500, 640, False), (0.6, 1000, 344, True), (1.6, 1000, 344, False), (3.0, qx + 460, XLS_TOP + 290, False), (4.3, qx + 580, XLS_TOP + 925, True), (5.5, qx + 580, XLS_TOP + 925, False)])
    scenes.append(Scene(5.5, r))

    # 6 — safety symbol
    sym = 'xls-symbol-en-dialog.png'
    r, _ = window_scene('xls-symbol-en.png', 'XLS42', 'Add standard product marks.',
                        [(0, focus(None, 1320, 700, 0.86)), (4.0, focus(None, 1320, 690, 0.9))],
                        sub='14 symbols: protective earth, Class II, ESD, AC / DC, polarity, fuse…',
                        sheet=sym, sheet_top=XLS_TOP, sheet_at=0.0,
                        pointer=[(0.4, qx + 400, XLS_TOP + 900, False), (1.5, qx + 520, XLS_TOP + 158, True), (2.6, qx + 520, XLS_TOP + 158, False), (3.4, qx + 580, XLS_TOP + 925, True), (4.0, qx + 580, XLS_TOP + 925, False)])
    scenes.append(Scene(4.0, r))

    # 7 — label sheet from rows
    lab = 'xls-labels-en-dialog.png'; lx = (2640 - load(lab).width) // 2
    r, _ = window_scene('xls-products-en.png', 'XLS42', 'Print a whole label sheet from your rows.',
                        [(0, fit(window_world('xls-products-en.png'), 0.02)), (1.3, fit(window_world('xls-products-en.png'), 0.0)),
                         (2.6, focus(None, 1320, 860, 0.66)), (4.6, focus(None, lx + 1370, LABELS_TOP + 520, 0.95)), (7.0, focus(None, lx + 1370, LABELS_TOP + 560, 1.02))],
                        sub='Code from any column · 559 label stocks · printer calibration in the print panel.',
                        sheet=lab, sheet_top=LABELS_TOP, sheet_at=1.4, extra=labels_extra, extra_at=3.9,
                        pointer=[(0.0, 600, 700, False), (0.3, 180, 610, True), (0.8, 1100, 1330, False), (1.2, 1100, 1330, False),
                                 (2.0, lx + 260, LABELS_TOP + 902, True), (2.8, lx + 260, LABELS_TOP + 902, False), (3.5, lx + 1420, LABELS_TOP + 1385, True), (4.4, lx + 1420, LABELS_TOP + 1385, False)])
    scenes.append(Scene(7.0, r))

    # 8 — montage of the other apps
    for win, app, title in [('doc-letter-en.png', 'DOC42', 'Write documents that open in Word.'),
                            ('xls-budget-en.png', 'XLS42', 'Budgets, formulas and charts.'),
                            ('ppt-deck-en.png', 'PPT42', 'Present with slides and speaker tools.'),
                            ('img-markup-en.png', 'IMG42', 'Adjust and mark up images.'),
                            ('scn-scan-en.png', 'SCN42', 'Scan to searchable PDF with OCR.')]:
        ww = window_world(win); c = fit(ww, 0.02)
        r, _ = window_scene(win, app, title, [(0, (c[0], c[1] + 10, c[2] * 0.98)), (2.4, (c[0], c[1] - 10, c[2] * 1.05))])
        scenes.append(Scene(2.4, r))

    # 9 — outro
    def outro(t, frame):
        d = ImageDraw.Draw(frame, 'RGBA')
        a = int(255 * ease(t / 0.6))
        brand(d, (W - brand_width(150)) / 2, 270, 150, a)
        a2 = int(255 * ease((t - 0.4) / 0.6))
        centered(d, 490, 'Free. Works offline. Open source.', font(56, 10), INK + (a2,))
        a3 = int(255 * ease((t - 0.9) / 0.6))
        centered(d, 580, 'macOS and Windows  ·  20 languages  ·  MIT license', font(28, 0), MUTED + (a3,))
        d.rounded_rectangle((W / 2 - 125, 670, W / 2 + 125, 734), 6, fill=(189, 65, 37, a3))
        centered(d, 683, 'off42.cc', font(32, 1), (255, 255, 255, a3))
    scenes.append(Scene(4.5, outro))
    return scenes

def main():
    out = sys.argv[1]
    scenes = build()
    total = sum(s.dur for s in scenes)
    XF = 0.4  # crossfade
    starts = []; t = 0
    for s in scenes: starts.append(t); t += s.dur
    def frame_at(T):
        for i, (s0, sc) in enumerate(zip(starts, scenes)):
            if T < s0 + sc.dur or i == len(scenes) - 1:
                f = Image.new('RGB', (W, H), BG); sc.render(min(T - s0, sc.dur), f)
                if i + 1 < len(scenes) and T > s0 + sc.dur - XF:
                    g = Image.new('RGB', (W, H), BG); scenes[i + 1].render(0, g)
                    f = Image.blend(f, g, ease((T - (s0 + sc.dur - XF)) / XF))
                if i == len(scenes) - 1 and T > total - 0.5:
                    f = Image.blend(f, Image.new('RGB', (W, H), BG), ease((T - (total - 0.5)) / 0.5) * 0.0)
                return f
    if '--preview' in sys.argv:
        for x in sys.argv[sys.argv.index('--preview') + 1].split(','):
            frame_at(float(x)).save(V / f'preview-{x}.png')
        return
    n = int(total * FPS)
    p = subprocess.Popen(['ffmpeg', '-y', '-loglevel', 'error', '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-s', f'{W}x{H}', '-r', str(FPS), '-i', '-',
                          '-c:v', 'libx264', '-preset', 'veryslow', '-crf', '27', '-tune', 'animation', '-g', '120', '-bf', '4', '-pix_fmt', 'yuv420p',
                          '-profile:v', 'high', '-movflags', '+faststart', '-an', out], stdin=subprocess.PIPE)
    for k in range(n):
        p.stdin.write(frame_at(k / FPS).tobytes())
        if k % 150 == 0: print(f'{k}/{n}', flush=True)
    p.stdin.close(); p.wait(); print('done', total, 's')

if __name__ == '__main__':
    main()
