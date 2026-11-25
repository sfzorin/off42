"""OFF42 demo, second cut: the everyday office first (Word, Excel, PowerPoint files, PDFs, scans), then the special
tools (QR labels, conformity marks, signatures), then languages. Reuses compose.py's camera, sheets and pointer.
Usage: compose2.py OUT.mp4 [--preview T1,T2,…]"""
import sys
from pathlib import Path
from PIL import Image, ImageDraw
import compose as C
from compose import (Scene, window_scene, window_world, load, fit, focus, shoot, caption, mix_cam, ease, shadow,
                     rounded, brand, brand_width, centered, font, VIEW, VW, VH, W, H, BG, INK, MUTED, APP, MARGIN,
                     PDF_TOP, XLS_TOP, LABELS_TOP, labels_extra)

SHOTS = Path(__import__('os').environ['OFF42_REPO']) / 'website/public/assets/shots'

def sequence_scene(frames, app, title, cams, sub=None, fade=0.18):
    """frames: [(time, window image)] — successive real renders of one window (typing)."""
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

def languages_scene():
    langs = [('ja', 'Japanese'), ('ar', 'Arabic'), ('de', 'German'), ('ko', 'Korean'), ('es', 'Spanish'), ('zh-Hans', 'Chinese'), ('ru', 'Russian')]
    shots = [(name, Image.open(SHOTS / f'xls-budget-{code}-1600.webp').convert('RGB')) for code, name in langs]
    step = 0.62
    def render(t, frame):
        i = min(len(shots) - 1, int(t / step))
        name, im = shots[i]
        world = Image.new('RGB', (im.width + 2 * MARGIN, im.height + 2 * MARGIN), BG)
        sh, pad = shadow(im.size, 14, 30, 60); world.paste(sh, (MARGIN - pad, MARGIN - pad + 14), sh)
        r = rounded(im, 14); world.paste(r, (MARGIN, MARGIN), r)
        cam = (world.width / 2, world.height / 2, min((VW - 120) / im.width, (VH - 60) / im.height) * (1 + 0.02 * (t - i * step)))
        frame.paste(shoot(world, cam), (VIEW[0], VIEW[1]))
        caption(frame, None, 'In your language: 20 of them.', ease(t / 0.4), 'The same budget in XLS42, interface in ' + name + '.')
    return render

def build():
    S = []
    # 0 — the promise
    def intro(t, frame):
        d = ImageDraw.Draw(frame, 'RGBA')
        brand(d, (W - brand_width(120)) / 2, 250, 120, int(255 * ease(t / 0.5)))
        centered(d, 430, 'Your whole office. No subscription.', font(64, 10), INK + (int(255 * ease((t - 0.35) / 0.6)),))
        a3 = int(255 * ease((t - 0.9) / 0.5))
        x = (W - 6 * 190 + 60) / 2
        for name, col in APP.items():
            d.rounded_rectangle((x, 560, x + 14, 574), 3, fill=col + (a3,))
            d.text((x + 24, 553), name, font=font(26, 1), fill=col + (a3,)); x += 190
        centered(d, 630, 'Opens Word, Excel, PowerPoint and PDF files  ·  Works offline  ·  Free and open source', font(27, 0), MUTED + (int(255 * ease((t - 1.3) / 0.5)),))
    S.append(Scene(3.6, intro))

    # 1 — a Word letter: keep writing (each frame is DOC42 itself after typing more of the sentence)
    doc = window_world('doc-letter-en.png'); c = fit(doc, 0.02)
    S.append(Scene(6.0, sequence_scene(
        [(0, 'doc-letter-en.png'), (1.7, 'doc-type1-en.png'), (2.3, 'doc-type2-en.png'), (2.9, 'doc-type3-en.png'), (3.5, 'doc-type4-en.png'), (4.1, 'doc-type5-en.png')],
        'DOC42', 'Open a Word file and keep writing.', [(0, c), (1.4, focus(None, 1200, 560, 1.12)), (6.0, focus(None, 1210, 580, 1.2))],
        sub='Opens DOCX, DOC, ODT, RTF and Markdown · saves DOCX, ODT, RTF and PDF.')))

    # 2 — Excel budget: formula and chart
    xw = window_world('xls-formula-en.png')
    r, _ = window_scene('xls-formula-en.png', 'XLS42', 'Spreadsheets with real formulas and charts.',
                        [(0, focus(None, 600, 520, 1.0)), (1.8, focus(None, 600, 520, 1.0)), (5.0, focus(None, 820, 1200, 0.78))],
                        sub='XLSX, XLS, ODS and CSV · 384 functions · charts.')
    S.append(Scene(5.0, r))

    # 3 — PowerPoint deck, then presenter view
    deck = window_world('ppt-deck-en.png'); pres = window_world('ppt-presenter-en.png')
    def ppt(t, frame):
        a = shoot(deck, mix_cam(fit(deck, 0.02), fit(deck, -0.04), t / 2.4))
        b = shoot(pres, mix_cam(fit(pres, 0.0), fit(pres, -0.05), (t - 2.2) / 2.8))
        img = a if t < 2.2 else (b if t > 2.6 else Image.blend(a, b, ease((t - 2.2) / 0.4)))
        frame.paste(img, (VIEW[0], VIEW[1]))
        caption(frame, 'PPT42', 'Present straight from PowerPoint files.', ease(t / 0.5),
                'Presenter view with the next slide, notes and a timer · export to PDF or video.')
    S.append(Scene(5.0, ppt))

    # 4 — edit text inside a PDF
    r, _ = window_scene('pdf-edit-en.png', 'PDF42', 'Fix a typo right inside the PDF.',
                        [(0, fit(window_world('pdf-edit-en.png'), 0.02)), (1.2, focus(None, 1250, 330, 1.1)), (4.5, focus(None, 1250, 340, 1.22))],
                        sub='Edit text, pages, forms and links — then save a normal PDF.')
    S.append(Scene(4.5, r))

    # 5 — shrink a heavy PDF
    comp = 'pdf-compress-en.png'; cx = (2640 - load(comp).width) // 2
    r, _ = window_scene('pdf-compress-win-en.png', 'PDF42', 'Shrink a heavy PDF before you email it.',
                        [(0, fit(window_world('pdf-compress-win-en.png'), 0.02)), (1.0, focus(None, 1320, 600, 0.95)), (4.2, focus(None, 1320, 560, 1.05))],
                        sub='Pictures get smaller; text stays sharp and searchable.',
                        sheet=comp, sheet_top=PDF_TOP, sheet_at=0.5,
                        pointer=[(1.2, cx + 200, PDF_TOP + 700, False), (2.8, cx + 360, PDF_TOP + 300, True), (4.2, cx + 360, PDF_TOP + 300, False)])
    S.append(Scene(4.2, r))

    # 6 — scan with OCR
    sw = window_world('scn-scan-en.png')
    r, _ = window_scene('scn-scan-en.png', 'SCN42', 'Scan paper into a searchable PDF.',
                        [(0, fit(sw, 0.02)), (4.0, focus(None, 1320, 900, 0.72))],
                        sub='USB and network scanners or your iPhone camera · text recognition built in.')
    S.append(Scene(4.0, r))

    # 7 — QR code from a cell
    qr = 'xls-qr-en-dialog.png'; qx = (2640 - load(qr).width) // 2
    r, _ = window_scene('xls-qr-en.png', 'XLS42', 'Turn a product list into QR codes…',
                        [(0, fit(window_world('xls-qr-en.png'), 0.02)), (0.9, focus(None, 1320, 620, 0.8)), (4.6, focus(None, 1320, 700, 0.86))],
                        sub='QR, Data Matrix, Code 128 and Aztec, sized in millimetres.',
                        sheet=qr, sheet_top=XLS_TOP, sheet_at=1.0,
                        pointer=[(0.0, 1500, 640, False), (0.6, 1000, 344, True), (1.6, 1000, 344, False), (3.4, qx + 580, XLS_TOP + 925, True), (4.6, qx + 580, XLS_TOP + 925, False)])
    S.append(Scene(4.6, r))

    # 8 — conformity mark with its conditions
    mk = 'xls-mark-en-dialog.png'; mx = (2640 - load(mk).width) // 2
    r, _ = window_scene('xls-mark-en.png', 'XLS42', '…add CE, UKCA, EAC and other marks…',
                        [(0, focus(None, 1320, 720, 0.84)), (4.4, focus(None, 1320, 760, 0.9))],
                        sub='97 marks · each shows its conditions of use. A mark does not certify the product.',
                        sheet=mk, sheet_top=XLS_TOP, sheet_at=0.0,
                        pointer=[(0.4, mx + 300, XLS_TOP + 1100, False), (1.6, mx + 700, XLS_TOP + 262, False), (4.4, mx + 700, XLS_TOP + 262, False)])
    S.append(Scene(4.4, r))

    # 9 — label sheet from the rows
    lab = 'xls-labels-en-dialog.png'; lx = (2640 - load(lab).width) // 2
    r, _ = window_scene('xls-products-en.png', 'XLS42', '…and print a whole sheet of labels.',
                        [(0, fit(window_world('xls-products-en.png'), 0.02)), (1.2, fit(window_world('xls-products-en.png'), 0.0)),
                         (2.4, focus(None, 1320, 860, 0.66)), (4.2, focus(None, lx + 1370, LABELS_TOP + 520, 0.95)), (6.0, focus(None, lx + 1370, LABELS_TOP + 560, 1.02))],
                        sub='Code from any column · 559 label stocks · printer calibration.',
                        sheet=lab, sheet_top=LABELS_TOP, sheet_at=1.2, extra=labels_extra, extra_at=3.6,
                        pointer=[(0.0, 600, 700, False), (0.3, 180, 610, True), (0.9, 1100, 1330, False),
                                 (1.9, lx + 260, LABELS_TOP + 902, True), (2.6, lx + 260, LABELS_TOP + 902, False), (3.3, lx + 1420, LABELS_TOP + 1385, True), (4.0, lx + 1420, LABELS_TOP + 1385, False)])
    S.append(Scene(6.0, r))

    # 10 — sign and stamp, briefly
    signed = Image.open(SHOTS / 'signed-en-1600.webp').convert('RGB')
    def signed_scene(t, frame):
        world = Image.new('RGB', (signed.width + 2 * MARGIN, signed.height + 2 * MARGIN), BG)
        sh, pad = shadow(signed.size, 10, 30, 60); world.paste(sh, (MARGIN - pad, MARGIN - pad + 14), sh)
        world.paste(rounded(signed, 10), (MARGIN, MARGIN), rounded(signed, 10))
        c0 = (world.width / 2, world.height / 2, (VW - 200) / signed.width); c1 = (world.width / 2, world.height / 2 + 20, (VW - 200) / signed.width * 1.1)
        frame.paste(shoot(world, mix_cam(c0, c1, t / 3.6)), (VIEW[0], VIEW[1]))
        caption(frame, 'PDF42', 'Sign and stamp without printing.', ease(t / 0.5), 'Handwritten-style signatures, company stamps and certificate signing.')
    S.append(Scene(3.6, signed_scene))

    # 11 — languages
    S.append(Scene(7 * 0.62, languages_scene()))

    # 12 — outro
    def outro(t, frame):
        d = ImageDraw.Draw(frame, 'RGBA')
        a = int(255 * ease(t / 0.5))
        brand(d, (W - brand_width(150)) / 2, 270, 150, a)
        centered(d, 490, 'Your whole office. No subscription.', font(56, 10), INK + (int(255 * ease((t - 0.3) / 0.6)),))
        a3 = int(255 * ease((t - 0.8) / 0.6))
        centered(d, 580, 'Free  ·  Works offline  ·  Open source  ·  macOS and Windows', font(28, 0), MUTED + (a3,))
        d.rounded_rectangle((W / 2 - 125, 670, W / 2 + 125, 734), 6, fill=(189, 65, 37, a3))
        centered(d, 683, 'off42.cc', font(32, 1), (255, 255, 255, a3))
    S.append(Scene(4.4, outro))
    return S

if __name__ == '__main__':
    C.build = build
    C.main()
