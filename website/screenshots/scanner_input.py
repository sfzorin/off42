import sys, subprocess
from pathlib import Path
from PIL import Image, ImageFilter
S = Path(__file__).resolve().parent
for lang in sys.argv[1:]:
    subprocess.run(['swift', str(S / 'render.swift'), str(S / f'docs/agreement-{lang}.pdf'), str(S / f'docs/hi-{lang}.png')], check=True)
    im = Image.open(S / f'docs/hi-{lang}.png').convert('RGB')
    im = im.resize((1240, round(im.height * 1240 / im.width)), Image.LANCZOS)
    d = S / f'scanner-{lang}'; d.mkdir(exist_ok=True)
    paper = Image.new('RGB', (im.width + 80, im.height + 80), (238, 236, 229))
    tone = Image.eval(im, lambda v: int(v * 0.95 + 8))
    paper.paste(tone.rotate(-0.9, expand=False, fillcolor=(243, 241, 234), resample=Image.BICUBIC), (40, 40))
    paper.filter(ImageFilter.GaussianBlur(0.6)).save(d / 'feed-1.png')
