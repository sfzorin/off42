#!/usr/bin/env python3
"""Independent IMG42 artifact checks using Pillow; fixtures and JSON evidence stay in build/."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import time

from PIL import Image, ImageChops, ImageDraw, ImageFont, ImageOps, TiffImagePlugin

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, default=ROOT / '.build/debug/IMG42')
parser.add_argument('--output-dir', type=Path, default=ROOT / 'build/img-cli-audit')
args = parser.parse_args()
binary = args.binary.resolve()
out = args.output_dir.resolve(); out.mkdir(parents=True, exist_ok=True)
checks = []
commands = []


def run(*arguments, success=True):
    command = [str(binary), *map(str, arguments)]
    result = subprocess.run(command, capture_output=True, text=True, timeout=90)
    commands.append({'arguments': command, 'exit_code': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr})
    assert (result.returncode == 0) == success, f'Unexpected exit {result.returncode}: {command}\n{result.stdout}\n{result.stderr}'
    return result.stdout


def check(name, body):
    start = time.monotonic()
    try:
        detail = body()
        record = {'name': name, 'status': 'PASS', 'detail': detail, 'seconds': round(time.monotonic() - start, 3)}
    except Exception as error:
        record = {'name': name, 'status': 'FAIL', 'detail': str(error), 'seconds': round(time.monotonic() - start, 3)}
    checks.append(record); print(record['status'], name + ':', record['detail'], flush=True)


source = out / 'quadrants.png'
image = Image.new('RGB', (96, 64), 'white')
draw = ImageDraw.Draw(image)
draw.rectangle((0, 0, 47, 31), fill='red'); draw.rectangle((48, 0, 95, 31), fill='lime')
draw.rectangle((0, 32, 47, 63), fill='blue'); draw.rectangle((48, 32, 95, 63), fill='yellow')
image.save(source, dpi=(150, 150))


def formats():
    text = run('--info', source)
    assert '96x64' in text and 'OK' in text
    for suffix, reported in [('png', 'PNG'), ('jpg', 'JPEG'), ('tiff', 'TIFF'), ('bmp', 'BMP')]:
        target = out / ('format.' + suffix); run('--edit', source, target)
        with Image.open(target) as result:
            assert result.format == reported and result.size == (96, 64)
    return 'PNG/JPEG/TIFF/BMP are readable and use the requested container and dimensions'


def orientation():
    jpeg = out / 'oriented.jpg'; image.save(jpeg, quality=100, subsampling=0)
    raw = Image.open(jpeg).convert('RGB'); before = raw.tobytes()
    run('--rotate', jpeg, 'right')
    with Image.open(jpeg) as rotated:
        assert rotated.getexif().get(274) == 6 and rotated.convert('RGB').tobytes() == before
        expected = ImageOps.exif_transpose(rotated).convert('RGB')
    target = out / 'orientation.png'; run('--edit', jpeg, target)
    with Image.open(target) as result:
        assert result.size == (64, 96)
        assert ImageChops.difference(result.convert('RGB'), expected).getbbox() is None
    run('--rotate', jpeg, 'left')
    assert Image.open(jpeg).getexif().get(274, 1) == 1
    return 'lossless right/left rotation changes EXIF without re-encoding pixels; pixel export applies orientation once'


def transforms():
    variants = [(['--rotate', 'right'], Image.Transpose.ROTATE_270), (['--rotate', 'left'], Image.Transpose.ROTATE_90),
                (['--flip', 'h'], Image.Transpose.FLIP_LEFT_RIGHT), (['--flip', 'v'], Image.Transpose.FLIP_TOP_BOTTOM)]
    for index, (options, method) in enumerate(variants):
        target = out / f'transform-{index}.png'; run('--edit', source, target, *options)
        with Image.open(target) as result:
            assert ImageChops.difference(result.convert('RGB'), image.transpose(method)).getbbox() is None
    return 'four pixel transformations match independent Pillow orientation maps'


def geometry():
    resized = out / 'resized.png'; run('--edit', source, resized, '--resize', '48x32')
    with Image.open(resized) as result:
        assert result.size == (48, 32) and result.convert('RGB').getpixel((6, 6)) == (255, 0, 0)
    crop = out / 'crop.png'; run('--edit', source, crop, '--crop', '48,0,48,32')
    with Image.open(crop) as result:
        assert result.size == (48, 32) and result.convert('RGB').getpixel((20, 12)) == (0, 255, 0)
    return 'resize dimensions and top-left crop coordinates agree with independently inspected pixels'


def alpha():
    transparent = out / 'transparent.png'
    rgba = Image.new('RGBA', (96, 64), (0, 0, 0, 0)); ImageDraw.Draw(rgba).rectangle((0, 0, 47, 63), fill=(255, 0, 0, 255)); rgba.save(transparent)
    png = out / 'alpha.png'; jpeg = out / 'alpha-white.jpg'
    run('--edit', transparent, png); run('--edit', transparent, jpeg)
    assert Image.open(png).convert('RGBA').getpixel((80, 30))[3] == 0
    assert min(Image.open(jpeg).convert('RGB').getpixel((80, 30))) > 245
    return 'PNG retains transparency; JPEG uses a white rather than black background'


def metadata():
    start = out / 'metadata-source.tiff'
    tags = TiffImagePlugin.ImageFileDirectory_v2(); tags[270] = 'Original caption'; tags[274] = 1
    image.save(start, tiffinfo=tags, dpi=(144, 144))
    target = out / 'metadata-edited.tiff'; run('--edit', start, target, '--description', 'Updated caption', '--resize', '48x32')
    with Image.open(target) as result:
        assert result.size == (48, 32) and result.tag_v2.get(270) == 'Updated caption'
        assert abs(float(result.tag_v2[282]) - 144) < 0.05
        assert result.tag_v2.get(274, 1) == 1
    run('--edit', target, target, '--description', '')
    assert Image.open(target).tag_v2.get(270, '') == ''
    return 'TIFF description, explicit clearing, DPI and new pixel dimensions survive same-path replacement'


def marks():
    white = out / 'white.png'; Image.new('RGB', (800, 600), 'white').save(white)
    target = out / 'marks-transformed.png'
    run('--edit', white, target, '--demo-marks', '--resize', '400x300', '--crop', '200,0,200,150')
    result = Image.open(target).convert('RGB'); assert result.size == (200, 150), f'Unexpected crop size: {result.size}'
    # Separate the red rectangle from the adjacent orange rounded rectangle in the demo fixture.
    points = [(x, y) for y in range(result.height) for x in range(result.width)
              if (lambda c: c[0] > 150 and c[1] < 100 and c[2] < 100)(result.getpixel((x, y)))]
    assert len(points) > 100, f'Only {len(points)} red pixels remain after transforms'
    bounds = (min(p[0] for p in points), max(p[0] for p in points), min(p[1] for p in points), max(p[1] for p in points))
    assert 43 <= bounds[0] <= 52 and 108 <= bounds[1] <= 118, f'Red rectangle x bounds: {bounds}; count={len(points)}'
    assert 12 <= bounds[2] <= 22 and 61 <= bounds[3] <= 72, f'Red rectangle y bounds: {bounds}; count={len(points)}'
    return 'red annotation follows both resizing and cropping to its expected pixel position'


def adjustments():
    neutral = out / 'neutral.png'; Image.new('RGB', (64, 48), (90, 90, 90)).save(neutral)
    target = out / 'bright.png'; run('--edit', neutral, target, '--brightness', '50')
    assert min(Image.open(target).convert('RGB').getpixel((20, 20))) > 100
    target = out / 'inverted.png'; run('--edit', source, target, '--look', 'invert')
    assert Image.open(target).convert('RGB').getpixel((10, 10)) == (0, 255, 255)
    return 'brightness changes actual pixels and Invert maps red to cyan'


def multiple():
    tiff = out / 'multipage.tiff'; image.save(tiff, save_all=True, append_images=[Image.new('RGB', (40, 60), 'purple')])
    assert 'pages=2' in run('--info', tiff)
    gif = out / 'animated.gif'; image.save(gif, save_all=True, append_images=[Image.new('RGB', (96, 64), 'purple')], duration=[100, 200], loop=0)
    assert 'frames=2' in run('--info', gif)
    target = out / 'first-frame.png'; run('--edit', gif, target)
    assert Image.open(target).size == (96, 64) and Image.open(gif).n_frames == 2
    return 'TIFF page count and GIF animation frames are detected; still export does not modify the animation source'


def bounded():
    wide = out / 'wide.png'; Image.new('RGB', (17000, 2), 'white').save(wide)
    assert '16384x' in run('--info', wide)
    target = out / 'wide-copy.png'; target.write_bytes(b'KEEP')
    run('--edit', wide, target, success=False); assert target.read_bytes() == b'KEEP'
    animation = out / 'wide.gif'
    Image.new('RGB', (17000, 2), 'white').save(animation, save_all=True, append_images=[Image.new('RGB', (17000, 2), 'black')], duration=100)
    run('--edit', animation, target, success=False); assert target.read_bytes() == b'KEEP'
    return 'still and animated display decoding are bounded; CLI refuses exporting either reduced proxy as full-resolution editing'


def failures():
    target = out / 'failure.png'; sentinel = b'UNCHANGED DESTINATION'
    cases = [('--unknown',), ('--brightness', 'nan'), ('--contrast', '999'), ('--resize', 'nanx2'), ('--resize', '1e30x2'),
             ('--resize', '0x20'), ('--resize', '2x3x4'), ('--crop', '0,0,-5,10'), ('--crop', '0,0,nan,4'),
             ('--crop', '999,999,10,10'), ('--look', 'nonsense'), ('--rotate', 'diagonal'), ('--flip', 'oops'), ('--description',)]
    for options in cases:
        target.write_bytes(sentinel); run('--edit', source, target, *options, success=False); assert target.read_bytes() == sentinel
    unsupported = out / 'unsupported.bogus'; unsupported.write_bytes(sentinel)
    run('--edit', source, unsupported, success=False); assert unsupported.read_bytes() == sentinel
    directory = out / 'destination-directory.png'; directory.mkdir(exist_ok=True)
    child = directory / 'keep'; child.write_bytes(sentinel)
    run('--edit', source, directory, success=False); assert child.read_bytes() == sentinel
    for command in [('--info',), ('--stats',), ('--ocr',), ('--rotate', source), ('--rotate', source, 'bad'), ('--types', 'extra'), ('--unknown-command',)]:
        run(*command, success=False)
    run('--info', out / 'does-not-exist.png', success=False)
    return f'{len(cases)} invalid edits plus unsupported format/directory/arity/read failures preserve destinations'


def ocr():
    path = out / 'text.png'; text = Image.new('RGB', (640, 180), 'white')
    ImageDraw.Draw(text).text((25, 55), 'IMAGE AUDIT 42', font=ImageFont.load_default(size=54), fill='black'); text.save(path)
    recognized = run('--ocr', path).upper(); assert 'IMAGE' in recognized and 'AUDIT' in recognized and '42' in recognized, recognized
    return 'Vision recognizes the generated three-token text fixture'


started = datetime.now(timezone.utc).isoformat()
for name, body in [('Containers and dimensions', formats), ('EXIF orientation', orientation), ('Pixel rotate and flip', transforms),
                   ('Resize and crop', geometry), ('Alpha and JPEG background', alpha), ('Description and DPI', metadata),
                   ('Markup follows geometry', marks), ('Colour adjustments', adjustments), ('Pages and animation', multiple),
                   ('Bounded decoding', bounded), ('Failure preservation', failures), ('Actual OCR', ocr)]:
    check(name, body)
report = {'started_at': started, 'binary': str(binary), 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': checks}
(out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
(out / 'commands.json').write_text(json.dumps(commands, indent=2) + '\n')
failed = sum(c['status'] != 'PASS' for c in checks)
print(f'{len(checks)-failed} passed, {failed} failed, 0 skipped; {out / "result.json"}')
raise SystemExit(bool(failed))
