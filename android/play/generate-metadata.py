#!/usr/bin/env python3
"""Writes the Play listing from android/play/listing.json into the layout that `fastlane supply` and the Play
Console bulk upload use: android/play/metadata/<locale>/{title,short_description,full_description}.txt.
Checks Google Play's limits: title 30, short description 80, full description 4000 characters.

    python3 android/play/generate-metadata.py
"""
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
LIMITS = {"title": 30, "short": 80, "full": 4000}
FILES = {"title": "title.txt", "short": "short_description.txt", "full": "full_description.txt"}


def main():
    listing = json.loads((HERE / "listing.json").read_text(encoding="utf-8"))
    problems = []
    for locale, texts in listing.items():
        folder = HERE / "metadata" / locale
        folder.mkdir(parents=True, exist_ok=True)
        for key, limit in LIMITS.items():
            text = texts[key]
            if len(text) > limit:
                problems.append(f"{locale} {key}: {len(text)} > {limit} characters")
            (folder / FILES[key]).write_text(text + "\n", encoding="utf-8")
    if problems:
        sys.exit("\n".join(problems))
    print(f"{len(listing)} locales written to {HERE / 'metadata'}")


if __name__ == "__main__":
    main()
