#!/usr/bin/env python3
"""Export the existing Swift UI catalogs for the native Windows host."""
import argparse
import json
from pathlib import Path
import re

LANGUAGES = ["zh-Hans", "zh-Hant", "es", "ar", "fr", "pt", "id", "ru", "de", "ja", "vi", "tr", "fil", "ko", "th", "it", "pl", "ms", "hu"]
LITERAL = r'"(?:[^"\\]|\\.)*"'
ROW = re.compile(r'^\s*\[\s*(' + LITERAL + r')\s*,\s*(' + LITERAL + r')\s*,\s*(' + LITERAL + r')\s*\]\s*,?\s*(?://.*)?$')


def swift_string(value):
    value = re.sub(r'\\u\{([0-9a-fA-F]+)\}', lambda match: chr(int(match[1], 16)), value)
    return json.loads(value)


def export(source, output):
    source = source.resolve()
    output.mkdir(parents=True, exist_ok=True)
    catalogs = {language: {} for language in LANGUAGES}
    shared = source / "Kit42"
    for language in LANGUAGES:
        path = shared / "Resources" / "Translations" / (language + ".json")
        if language not in ("ru", "tr"):
            catalogs[language] = json.loads(path.read_text(encoding="utf-8"))
    paths = [shared / "Strings.swift", shared / "ProductionStrings.swift"]
    paths += [source / app / "Strings.swift" for app in ("PDF42", "DOC42", "XLS42", "IMG42", "PPT42", "SCN42")]
    count = 0
    for path in paths:
        for line in path.read_text(encoding="utf-8").splitlines():
            row = ROW.fullmatch(line)
            if row:
                key, ru, tr = [swift_string(value) for value in row.groups()]
                if ru:
                    catalogs["ru"][key] = ru
                if tr:
                    catalogs["tr"][key] = tr
                count += 1
    assert count > 2000, count
    for language, catalog in catalogs.items():
        assert len(catalog) > 2000, (language, len(catalog))
        (output / (language + ".json")).write_text(json.dumps(catalog, ensure_ascii=False, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"swift_rows": count, "languages": {key: len(value) for key, value in catalogs.items()}}, ensure_ascii=False))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True, help="Path to the Swift Sources directory")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    export(args.source, args.output)
