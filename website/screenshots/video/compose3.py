"""OFF42 demo video in any interface language. Frames are the apps' own renders in that language (render_i18n.py);
captions come from video-i18n/<lang>.json and are drawn by AppKit (textpng) so Arabic, Thai and CJK shape correctly.
Usage: compose3.py LANG OUT.mp4 [--preview T1,T2,…]"""
import hashlib, json, subprocess, sys
from pathlib import Path
from PIL import Image, ImageDraw
import compose as C
from compose import (Scene, window_world, load, fit, focus, shoot, mix_cam, ease, shadow, rounded, brand, brand_width,
                     font, VIEW, VW, VH, W, H, BG, INK, MUTED, APP, MARGIN, PDF_TOP, XLS_TOP, LABELS_TOP, draw_cursor, to_screen)

V = Path(__file__).resolve().parent
SHOTS = Path(__import__('os').environ['OFF42_REPO']) / 'website/public/assets/shots'
LANG = sys.argv[1]
COPY = json.loads((V / 'video-i18n' / f'{LANG}.json').read_text())
CAP = COPY['captions']
RTL = LANG == 'ar'
TXT = V / 'textcache'; TXT.mkdir(exist_ok=True)

def text_img(text, size, weight='regular', color=INK):
    key = hashlib.sha1(f'{text}|{size}|{weight}|{color}'.encode()).hexdigest()[:16]
    path = TXT / f'{key}.png'
    if not path.exists():
        subprocess.run([str(V / 'textpng'), str(path), str(size), weight, *map(str, color[:3]), text], check=True, capture_output=True)
    im = Image.open(path).convert('RGBA')
    return im

def paste_text(frame, im, x, y, alpha=1.0, anchor='start'):
    if alpha <= 0: return
    if alpha < 1:
        im = im.copy(); im.putalpha(im.getchannel('A').point(lambda v: int(v * alpha)))
    pad = int(im.height * 0.0)
    if anchor == 'center': x = x - im.width / 2
    elif anchor == 'end': x = x - im.width
    frame.paste(im, (int(x), int(y)), im)

def caption(frame, app, title, alpha=1.0, sub=None):
    if alpha <= 0: return
    edge = W - 96 if RTL else 96
    anchor = 'end' if RTL else 'start'
    y = 50
    if app:
        col = APP.get(app, (221, 87, 56))
        tag = text_img(app, 22, 'bold', col)
        d = ImageDraw.Draw(frame, 'RGBA')
        a = int(255 * alpha)
        if RTL:
            d.rounded_rectangle((edge - 14, y + 13, edge, y + 27), 3, fill=col + (a,))
            paste_text(frame, tag, edge - 24, y + 2, alpha, 'end')
        else:
            d.rounded_rectangle((edge, y + 13, edge + 14, y + 27), 3, fill=col + (a,))
            paste_text(frame, tag, edge + 24, y + 2, alpha)
        y += 36
    paste_text(frame, text_img(title, 48, 'medium', INK), edge - (0 if not RTL else -7), y - 7, alpha, anchor)
    if sub: paste_text(frame, text_img(sub, 23, 'regular', MUTED), edge, y + 60, alpha, anchor)

def blue_button(name):
    """Centre of the accent-blue default button in a dialog render (window coordinates of the image)."""
    im = load(name).resize((load(name).width // 4, load(name).height // 4))
    px = im.load(); pts = [(x, y) for y in range(im.height) for x in range(im.width)
                           if px[x, y][2] > 200 and px[x, y][0] < 90 and 90 < px[x, y][1] < 160]
    if not pts: return (load(name).width * 0.6, load(name).height * 0.93)
    xs = sorted(p[0] for p in pts); ys = sorted(p[1] for p in pts)
    bottom = [p for p in pts if p[1] >= ys[-1] - 20] or pts
    return (sum(p[0] for p in bottom) / len(bottom) * 4, sum(p[1] for p in bottom) / len(bottom) * 4)

def wscene(win, app, title, cams, sub=None, sheet=None, sheet_top=0, sheet_at=None, pointer=None, extra=None, extra_at=0.0):
    base = window_world(win)
    plain = C.with_sheet(base, sheet, sheet_top, 1) if sheet else None
    full = extra(plain) if extra and plain is not None else plain
    def cam_at(t):
        for (t0, c0), (t1, c1) in zip(cams, cams[1:]):
            if t <= t1: return mix_cam(c0, c1, (t - t0) / max(1e-6, t1 - t0))
        return cams[-1][1]
    def render(t, frame):
        if sheet and t >= sheet_at:
            p = (t - sheet_at) / 0.45
            world = (full if t >= extra_at else plain) if p >= 1 else C.with_sheet(base, sheet, sheet_top, p)
        else: world = base
        cam = cam_at(t)
        frame.paste(shoot(world, cam), (VIEW[0], VIEW[1]))
        grad = Image.linear_gradient('L').resize((W, 60)).transpose(Image.FLIP_TOP_BOTTOM)
        frame.paste(Image.new('RGB', (W, 60), BG), (0, VIEW[1]), grad)
        caption(frame, app, title, ease(t / 0.5), sub)
        if pointer:
            pos = None
            for (t0, x0, y0, _), (t1, x1, y1, _) in zip(pointer, pointer[1:]):
                if t0 <= t <= t1: k = ease((t - t0) / max(1e-6, t1 - t0)); pos = (x0 + (x1 - x0) * k, y0 + (y1 - y0) * k); break
            if pos is None and t >= pointer[-1][0]: pos = pointer[-1][1:3]
            if pos is not None and t >= pointer[0][0]:
                press = max([(t - tc) / 0.45 for tc, _, _, click in pointer if click and 0 <= t - tc <= 0.45] or [0])
                sx, sy = to_screen(cam, MARGIN + pos[0], MARGIN + pos[1])
                draw_cursor(frame, sx, sy, press)
    return render

def sequence_scene(frames, app, title, cams, sub=None, fade=0.18):
    worlds = [(t, window_world(name)) for t, name in frames]
    def cam_at(t):
        for (t0, c0), (t1, c1) in zip(cams, cams[1:]):
            if t <= t1: return mix_cam(c0, c1, (t - t0) / max(1e-6, t1 - t0))
        return cams[-1][1]
    def render(t, frame):
        cam = cam_at(t)
        k = max(i for i, (t0, _) in enumerate(worlds) if t0 <= t or i == 0)
        img = shoot(worlds[k][1], cam)
        if k > 0 and t - worlds[k][0] < fade:
            img = Image.blend(shoot(worlds[k - 1][1], cam), img, (t - worlds[k][0]) / fade)
        frame.paste(img, (VIEW[0], VIEW[1]))
        grad = Image.linear_gradient('L').resize((W, 60)).transpose(Image.FLIP_TOP_BOTTOM)
        frame.paste(Image.new('RGB', (W, 60), BG), (0, VIEW[1]), grad)
        caption(frame, app, title, ease(t / 0.5), sub)
    return render

def typed_region(lang):
    """Where the letter changed while typing: the paragraph to zoom into (window coordinates)."""
    from PIL import ImageChops
    a = load(f'doc-letter-{lang}.png'); b = load(f'doc-type5-{lang}.png')
    d = ImageChops.difference(a.crop((0, 300, a.width, a.height - 120)), b.crop((0, 300, b.width, b.height - 120))).convert('L').point(lambda v: 255 if v > 40 else 0)
    box = d.getbbox()
    if not box: return (a.width / 2, 560)
    # the first changed rows are the typed lines; later rows are text that moved down
    rows = [y for y in range(box[1], box[3], 6) if d.crop((0, y, d.width, y + 6)).getbbox()]
    top = rows[0]; end = top
    for y in rows:
        if y - end > 30: break
        end = y
    return ((box[0] + box[2]) / 2, 300 + (top + end) / 2)

def labels_extra_for(lang):
    def extra(world):
        page = load(f'labels-page-{lang}.png')
        sx, sy = C.sheet_origin(world, f'xls-labels-{lang}-dialog.png', LABELS_TOP)
        dlg = load(f'xls-labels-{lang}-dialog.png')
        # the preview occupies the right part of the dialog (the sheet is not mirrored in Arabic)
        area = (860, 22, 1880, 1262)
        area = (area[0], area[1], min(area[2], dlg.width - 40), area[3])
        pw = area[2] - area[0] - 24; ph = round(page.height * pw / page.width)
        p = page.resize((pw, ph), Image.LANCZOS).crop((0, 0, pw, area[3] - area[1] - 24))
        out = world.copy(); out.paste(Image.new('RGB', (area[2] - area[0], area[3] - area[1]), (236, 236, 236)), (sx + area[0], sy + area[1]))
        out.paste(p, (sx + area[0] + 12, sy + area[1] + 12))
        return out
    return extra

def build():
    L = LANG; S = []
    def intro(t, frame):
        d = ImageDraw.Draw(frame, 'RGBA')
        brand(d, (W - brand_width(120)) / 2, 250, 120, int(255 * ease(t / 0.5)))
        paste_text(frame, text_img(CAP['introTitle'], 60, 'medium', INK), W / 2, 420, ease((t - 0.35) / 0.6), 'center')
        a3 = int(255 * ease((t - 0.9) / 0.5)); x = (W - 6 * 190 + 60) / 2
        for name, col in APP.items():
            d.rounded_rectangle((x, 560, x + 14, 574), 3, fill=col + (a3,))
            d.text((x + 24, 553), name, font=font(26, 1), fill=col + (a3,)); x += 190
        line = text_img(CAP['introLine'], 26, 'regular', MUTED)
        if line.width > W - 160: line = text_img(CAP['introLine'], 22, 'regular', MUTED)
        paste_text(frame, line, W / 2, 625, ease((t - 1.3) / 0.5), 'center')
    S.append(Scene(3.6, intro))

    doc = window_world(f'doc-letter-{L}.png'); c = fit(doc, 0.02); tx, ty = typed_region(L)
    S.append(Scene(6.0, sequence_scene(
        [(0, f'doc-letter-{L}.png')] + [(1.7 + 0.6 * i, f'doc-type{i + 1}-{L}.png') for i in range(5)],
        'DOC42', CAP['docTitle'], [(0, c), (1.4, focus(None, tx, ty + 60, 1.12)), (6.0, focus(None, tx, ty + 70, 1.2))], sub=CAP['docSub'])))

    S.append(Scene(5.0, wscene(f'xls-formula-{L}.png', 'XLS42', CAP['xlsTitle'],
        [(0, focus(None, 600, 520, 1.0)), (1.8, focus(None, 600, 520, 1.0)), (5.0, focus(None, 820, 1200, 0.78))], sub=CAP['xlsSub'])))

    deck = window_world(f'ppt-deck-{L}.png'); pres = window_world(f'ppt-presenter-{L}.png')
    def ppt(t, frame):
        a = shoot(deck, mix_cam(fit(deck, 0.02), fit(deck, -0.04), t / 2.4))
        b = shoot(pres, mix_cam(fit(pres, 0.0), fit(pres, -0.05), (t - 2.2) / 2.8))
        img = a if t < 2.2 else (b if t > 2.6 else Image.blend(a, b, ease((t - 2.2) / 0.4)))
        frame.paste(img, (VIEW[0], VIEW[1])); caption(frame, 'PPT42', CAP['pptTitle'], ease(t / 0.5), CAP['pptSub'])
    S.append(Scene(5.0, ppt))

    S.append(Scene(4.5, wscene(f'pdf-edit-{L}.png', 'PDF42', CAP['pdfEditTitle'],
        [(0, fit(window_world(f'pdf-edit-{L}.png'), 0.02)), (1.2, focus(None, 1250, 330, 1.1)), (4.5, focus(None, 1250, 340, 1.22))], sub=CAP['pdfEditSub'])))

    comp = f'pdf-compress-{L}.png'; cx = (2640 - load(comp).width) // 2; bx, by = blue_button(comp)
    S.append(Scene(4.2, wscene(f'pdf-compress-win-{L}.png', 'PDF42', CAP['compressTitle'],
        [(0, fit(window_world(f'pdf-compress-win-{L}.png'), 0.02)), (1.0, focus(None, 1320, 600, 0.95)), (4.2, focus(None, 1320, 560, 1.05))],
        sub=CAP['compressSub'], sheet=comp, sheet_top=PDF_TOP, sheet_at=0.5,
        pointer=[(1.2, cx + bx - 160, PDF_TOP + by + 400, False), (2.8, cx + bx, PDF_TOP + by, True), (4.2, cx + bx, PDF_TOP + by, False)])))

    sw = window_world(f'scn-scan-{L}.png')
    S.append(Scene(4.0, wscene(f'scn-scan-{L}.png', 'SCN42', CAP['scanTitle'], [(0, fit(sw, 0.02)), (4.0, focus(None, 1320, 900, 0.72))], sub=CAP['scanSub'])))

    qr = f'xls-qr-{L}-dialog.png'; qx = (2640 - load(qr).width) // 2; qbx, qby = blue_button(qr)
    S.append(Scene(4.6, wscene(f'xls-qr-{L}.png', 'XLS42', CAP['qrTitle'],
        [(0, fit(window_world(f'xls-qr-{L}.png'), 0.02)), (0.9, focus(None, 1320, 790, 0.68)), (4.6, focus(None, 1320, 810, 0.72))],
        sub=CAP['qrSub'], sheet=qr, sheet_top=XLS_TOP, sheet_at=1.0,
        pointer=[(0.0, 1320, 900, False), (0.6, 1320, 344, True), (1.6, 1320, 344, False), (3.4, qx + qbx, XLS_TOP + qby, True), (4.6, qx + qbx, XLS_TOP + qby, False)])))

    mk = f'xls-mark-{L}-dialog.png'; mx = (2640 - load(mk).width) // 2; mbx, mby = blue_button(mk)
    S.append(Scene(4.4, wscene(f'xls-mark-{L}.png', 'XLS42', CAP['marksTitle'],
        [(0, focus(None, 1320, 800, 0.7)), (4.4, focus(None, 1320, 815, 0.74))], sub=CAP['marksSub'],
        sheet=mk, sheet_top=XLS_TOP, sheet_at=0.0,
        pointer=[(0.4, mx + mbx - 200, XLS_TOP + mby - 300, False), (3.0, mx + mbx, XLS_TOP + mby, True), (4.4, mx + mbx, XLS_TOP + mby, False)])))

    lab = f'xls-labels-{L}-dialog.png'; lx = (2640 - load(lab).width) // 2
    preview_x = lx + 1370
    S.append(Scene(6.0, wscene(f'xls-products-{L}.png', 'XLS42', CAP['labelsTitle'],
        [(0, fit(window_world(f'xls-products-{L}.png'), 0.02)), (1.2, fit(window_world(f'xls-products-{L}.png'), 0.0)),
         (2.4, focus(None, 1320, 860, 0.66)), (4.2, focus(None, preview_x, LABELS_TOP + 520, 0.95)), (6.0, focus(None, preview_x, LABELS_TOP + 560, 1.02))],
        sub=CAP['labelsSub'], sheet=lab, sheet_top=LABELS_TOP, sheet_at=1.2, extra=labels_extra_for(L), extra_at=3.4)))

    signed = Image.open(SHOTS / f'signed-{L}-1600.webp').convert('RGB')
    def signed_scene(t, frame):
        world = Image.new('RGB', (signed.width + 2 * MARGIN, signed.height + 2 * MARGIN), BG)
        sh, pad = shadow(signed.size, 10, 30, 60); world.paste(sh, (MARGIN - pad, MARGIN - pad + 14), sh)
        world.paste(rounded(signed, 10), (MARGIN, MARGIN), rounded(signed, 10))
        c0 = (world.width / 2, world.height / 2, (VW - 200) / signed.width); c1 = (world.width / 2, world.height / 2 + 20, (VW - 200) / signed.width * 1.1)
        frame.paste(shoot(world, mix_cam(c0, c1, t / 3.6)), (VIEW[0], VIEW[1]))
        caption(frame, 'PDF42', CAP['signTitle'], ease(t / 0.5), CAP['signSub'])
    S.append(Scene(3.6, signed_scene))

    order = ['ja', 'ar', 'de', 'ko', 'es', 'zh-Hans', 'ru', 'en']
    langs = [x for x in order if x != L][:7]
    shots = [(x, Image.open(SHOTS / f'xls-budget-{x}-1600.webp').convert('RGB')) for x in langs]
    step = 0.62
    def languages(t, frame):
        i = min(len(shots) - 1, int(t / step)); code, im = shots[i]
        world = Image.new('RGB', (im.width + 2 * MARGIN, im.height + 2 * MARGIN), BG)
        sh, pad = shadow(im.size, 14, 30, 60); world.paste(sh, (MARGIN - pad, MARGIN - pad + 14), sh)
        r = rounded(im, 14); world.paste(r, (MARGIN, MARGIN), r)
        cam = (world.width / 2, world.height / 2, min((VW - 120) / im.width, (VH - 60) / im.height) * (1 + 0.02 * (t - i * step)))
        frame.paste(shoot(world, cam), (VIEW[0], VIEW[1]))
        caption(frame, None, CAP['langTitle'], ease(t / 0.4), CAP['langSub'].replace('{language}', COPY['languageNames'][code]))
    S.append(Scene(len(shots) * step, languages))

    def outro(t, frame):
        d = ImageDraw.Draw(frame, 'RGBA'); a = int(255 * ease(t / 0.5))
        brand(d, (W - brand_width(150)) / 2, 270, 150, a)
        paste_text(frame, text_img(CAP['introTitle'], 54, 'medium', INK), W / 2, 478, ease((t - 0.3) / 0.6), 'center')
        a3 = ease((t - 0.8) / 0.6)
        line = text_img(CAP['outroLine'], 27, 'regular', MUTED)
        if line.width > W - 160: line = text_img(CAP['outroLine'], 22, 'regular', MUTED)
        paste_text(frame, line, W / 2, 572, a3, 'center')
        d.rounded_rectangle((W / 2 - 125, 670, W / 2 + 125, 734), 6, fill=(189, 65, 37, int(255 * a3)))
        d.text((W / 2 - d.textlength('off42.cc', font=font(32, 1)) / 2, 683), 'off42.cc', font=font(32, 1), fill=(255, 255, 255, int(255 * a3)))
    S.append(Scene(4.4, outro))
    return S

if __name__ == '__main__':
    C.build = build
    # previews per language, so parallel runs do not overwrite each other
    C.V = V / 'tmp' / f'pv-{LANG}'; C.V.mkdir(parents=True, exist_ok=True)
    sys.argv = [sys.argv[0]] + sys.argv[2:]
    C.main()
