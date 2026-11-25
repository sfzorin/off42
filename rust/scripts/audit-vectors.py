#!/usr/bin/env python3
"""Compare rendered technical PDFs against independently rendered PNG recipes.
Run the label42 vector-fixtures example first; requires Poppler and Pillow for QA only.
"""
import argparse
import json
from pathlib import Path
import subprocess
from PIL import Image, ImageChops, ImageFilter, ImageOps


def audit(folder):
    results = []
    previews = []
    for pdf in sorted(folder.glob("*.pdf")):
        reference = Image.open(pdf.with_name(pdf.stem + "-reference.png")).convert("RGBA")
        white = Image.new("RGBA", reference.size, "white")
        white.alpha_composite(reference)
        reference = white.convert("RGB")
        prefix = pdf.with_name(pdf.stem + "-rendered")
        subprocess.run(["pdftoppm", "-singlefile", "-png", "-scale-to-x", str(reference.width), "-scale-to-y", str(reference.height), str(pdf), str(prefix)], check=True, capture_output=True)
        actual = Image.open(prefix.with_suffix(".png")).convert("RGB")
        assert actual.size == reference.size
        # Include antialiased hairlines. A 50% threshold drops the <1-pixel
        # inner triangle in the source high-voltage SVG on one renderer.
        masks = [ImageOps.grayscale(image).point(lambda value: 255 if value < 245 else 0) for image in (reference, actual)]
        black = [image.histogram()[255] for image in masks]
        missing = ImageChops.subtract(masks[0], masks[1].filter(ImageFilter.MaxFilter(3))).histogram()[255]
        extra = ImageChops.subtract(masks[1], masks[0].filter(ImageFilter.MaxFilter(3))).histogram()[255]
        error = (missing + extra) / max(1, black[0])
        result = {"fixture": pdf.stem, "pixels": actual.size, "black_reference": black[0], "black_pdf": black[1], "unmatched_outside_one_pixel": error}
        results.append(result)
        panel = Image.new("RGB", (440, 270), "#eeeeee")
        for x, image in [(0, reference), (220, actual)]:
            thumbnail = ImageOps.contain(image, (210, 240))
            panel.paste(thumbnail, (x + (220-thumbnail.width)//2, (250-thumbnail.height)//2))
        from PIL import ImageDraw
        ImageDraw.Draw(panel).text((8, 250), pdf.stem + " | SVG/PNG left, PDF right", fill="black")
        previews.append(panel)
    assert len(results) == 18, len(results)
    sheet = Image.new("RGB", (440*3, 270*6), "white")
    for index, panel in enumerate(previews):
        sheet.paste(panel, ((index % 3)*440, (index//3)*270))
    sheet.save(folder / "comparison.png")
    (folder / "comparison.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    for result in results:
        print(json.dumps(result))
    assert all(result["black_reference"] > 0 and result["unmatched_outside_one_pixel"] < 0.005 for result in results), results


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("folder", type=Path)
    audit(parser.parse_args().folder.resolve())
