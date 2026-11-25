#!/usr/bin/env python3
"""Check Suite42 localization coverage, untranslated UI literals and format safety.

Usage: python3 scripts/check_strings.py [--sources PATH]
English/Russian/Turkish rows remain in Swift; the other 17 languages are JSON
catalogs in Kit42/Resources/Translations. No network or app launch is needed.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent / "Sources"
APPS = ("Kit42", "PDF42", "DOC42", "XLS42", "IMG42", "PPT42", "SCN42")
LANGUAGES = ("en", "zh-Hans", "zh-Hant", "es", "ar", "fr", "pt", "id", "ru", "de",
             "ja", "vi", "tr", "fil", "ko", "th", "it", "pl", "ms", "hu")
CATALOG_LANGUAGES = tuple(language for language in LANGUAGES if language not in ("en", "ru", "tr"))
LITERAL = r'"((?:[^"\\]|\\.)*)"'
ROW = re.compile(r'\[\s*' + LITERAL + r'\s*,\s*' + LITERAL + r'\s*,\s*' + LITERAL + r'\s*\]')
CALL = re.compile(r'\b(?:L|PL)\(\s*' + LITERAL)
# A space after % is deliberately not treated as a flag: UI labels such as
# "100% Stacked" and "Largest %" are ordinary text, not printf formats.
FORMAT = re.compile(r'%(?:(?P<position>[1-9]\d*)\$)?(?P<flags>[-+#0\']*)'
                    r'(?P<width>\d+|\*(?:[1-9]\d*\$)?)?'
                    r'(?P<precision>\.(?:\d+|\*(?:[1-9]\d*\$)?))?'
                    r'(?P<length>hh|ll|h|l|q|L|z|t|j)?(?P<type>[@diuoxXfFeEgGaAcCsSpn])')
ALLOWED = {"name@example.com", "PDF42", "DOC42", "XLS42", "IMG42", "PPT42", "SCN42", "PDF", "HTML", "CSV"}


def decode_swift(text):
    """Decode Swift's simple string-literal escapes without corrupting UTF-8."""
    escapes = {"0": "\0", "n": "\n", "r": "\r", "t": "\t", '"': '"', "'": "'", "\\": "\\"}
    def replace(match):
        value = match.group(1)
        if value.startswith("u{"):
            return chr(int(value[2:-1], 16))
        return escapes.get(value, "\\" + value)
    return re.sub(r'\\(u\{[0-9a-fA-F]+\}|.)', replace, text)


def without_comments(text):
    # Preserve line numbers. Inline comments are left intact, as in the original
    # audit, so a URL inside a quoted UI string is never cut in half.
    return "\n".join("" if line.strip().startswith("//") else line for line in text.split("\n"))


def table_rows(path):
    return [tuple(decode_swift(value) for value in match.groups())
            for match in ROW.finditer(without_comments(path.read_text(encoding="utf-8")))]


def table(path):
    return {row[0] for row in table_rows(path)}


def format_signature(text, strict=False):
    """Return argument identities/formats and escaped-percent count.

    An explicit position may reorder arguments, but cannot change their type,
    width, precision or number of uses. Dynamic width/precision arguments are
    included. Mixed implicit/explicit argument positions are rejected.
    """
    arguments = Counter()
    next_argument = 1
    modes = set()
    escaped_percent = 0
    offset = 0

    def argument(position, descriptor):
        nonlocal next_argument
        if position:
            index = int(position)
            modes.add("explicit")
        else:
            index = next_argument
            next_argument += 1
            modes.add("implicit")
        arguments[(index, descriptor)] += 1

    while offset < len(text):
        if text[offset] != "%":
            offset += 1
            continue
        if text.startswith("%%", offset):
            escaped_percent += 1
            offset += 2
            continue
        match = FORMAT.match(text, offset)
        if match is None:
            if strict:
                raise ValueError(f"invalid percent sequence near {text[offset:offset + 12]!r}")
            offset += 1
            continue
        width = match["width"] or ""
        precision = match["precision"] or ""
        for component in (width, precision.removeprefix(".")):
            if component.startswith("*"):
                argument(component[1:].removesuffix("$"), "int:dynamic-width/precision")
        descriptor = (match["flags"] or "") + re.sub(r'\*[1-9]\d*\$', '*', width + precision)
        descriptor += (match["length"] or "") + match["type"]
        argument(match["position"], descriptor)
        offset = match.end()
    if len(modes) > 1:
        raise ValueError("mixed positional and sequential format arguments")
    return arguments, escaped_percent


def translation_problem(source, translated):
    if not isinstance(translated, str) or not translated.strip():
        return "translation must be a nonempty string"
    if any(0xD800 <= ord(character) <= 0xDFFF for character in translated):
        return "translation contains an invalid Unicode surrogate"
    if "\0" in translated:
        return "translation contains a NUL character"
    try:
        expected = format_signature(source)
        actual = format_signature(translated, strict=bool(expected[0] or expected[1]))
    except ValueError as error:
        return str(error)
    if expected != actual:
        return f"format placeholders differ: {expected!r} != {actual!r}"
    for pattern, name in ((r'<<[^<>]+>>', 'document template fields'),
                          (r'[⇧⌃⌥⌘]', 'modifier keys'),
                          # The hyphen in "⌥-click" joins prose; it is not a minus-key shortcut.
                          (r'[⇧⌃⌥⌘]+(?:[A-Za-z0-9⌫]|[+\-](?!\w))', 'keyboard shortcuts')):
        if Counter(re.findall(pattern, source)) != Counter(re.findall(pattern, translated)):
            return f"{name} differ"
    # Russian function aliases are accepted by XLS42's parser; sheet names are
    # user text, so the existing Лист2/Sayfa2 examples are also valid references.
    if '=SUM(A1:A10)' in source and not any(example in translated for example in ('=SUM(A1:A10)', '=СУММ(A1:A10)')):
        return 'missing supported SUM formula example'
    if '#Sheet2!A1' in source and not re.search(r'#\w+!A1\b', translated):
        return 'missing sheet reference example with cell A1'
    for literal in ('=$H$1:$H$5',
                    'soffice --headless --convert-to pptx file.ppt'):
        if literal in source and literal not in translated:
            return f"missing literal command or formula example: {literal}"
    return None


def load_catalog(path):
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate key {key!r}")
            result[key] = value
        return result
    result = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    if not isinstance(result, dict):
        raise ValueError("catalog must be a JSON object")
    return result


def check_catalog(path, required):
    try:
        catalog = load_catalog(path)
    except (OSError, UnicodeError, ValueError) as error:
        return [f"{path.name}: {error}"]
    problems = [f"{path.name}: missing translation: {key!r}" for key in sorted(required - catalog.keys())]
    problems.extend(f"{path.name}: unknown source key: {key!r}" for key in sorted(catalog.keys() - required))
    for key, value in sorted(catalog.items()):
        problem = translation_problem(key, value)
        if problem:
            problems.append(f"{path.name}: {key!r}: {problem}")
    return problems


def source_catalog(root):
    """All source keys, including dynamic stamp names and literal L/PL calls."""
    tables = [root / app / "Strings.swift" for app in (*APPS, "Previews42")]
    tables += sorted((root / "Kit42").glob("ProductionStrings.swift"))
    tables += sorted((root / "Kit42").glob("SealCatalog*.swift"))
    rows = [(path, row) for path in tables for row in table_rows(path)]
    required = {row[0] for _, row in rows}
    for app in (*APPS, "Previews42"):
        for path in sorted((root / app).glob("*.swift")):
            if path.name != "Strings.swift":
                required.update(decode_swift(match[1]) for match in CALL.finditer(
                    without_comments(path.read_text(encoding="utf-8"))))
    return required, rows


def check_sources(root):
    problems = []
    kit = table(root / "Kit42/Strings.swift")
    for path in (root / "Kit42").glob("ProductionStrings.swift"):
        kit |= table(path)
    # Stamp template titles are registered separately at runtime.
    for path in (root / "Kit42").glob("SealCatalog*.swift"):
        kit |= table(path)
    for app in APPS:
        known = kit | (table(root / app / "Strings.swift") if app != "Kit42" else set())
        missing = {}
        for path in sorted((root / app).glob("*.swift")):
            if path.name == "Strings.swift":
                continue
            text = without_comments(path.read_text(encoding="utf-8"))
            for match in re.finditer(r'\bL\(\s*' + LITERAL, text):
                key = decode_swift(match[1])
                if key not in known:
                    missing.setdefault(key, path.name)
            for match in re.finditer(r'(labelWithString|wrappingLabelWithString|withTitle|title|messageText|informativeText|placeholderString|toolTip|label)\s*[:=]\s*' + LITERAL, text):
                value = decode_swift(match[2])
                if (re.search(r'[A-Za-zА-Яа-я]{3}', value)
                    and not value.startswith(("http", "com.", "public.")) and "\\(" not in value
                    and "L(" not in value and value not in ALLOWED
                    and not re.fullmatch(r'[a-z0-9]+\.[a-z0-9.]+', value)):
                    line = text[:match.start()].count("\n") + 1
                    problems.append(f"{app}: not localized: {path.name}:{line}: {value}")
        problems.extend(f"{app}: no translation: {key!r} ({file})" for key, file in sorted(missing.items()))
    quicklook = table(root / "Previews42/Strings.swift")
    for path in sorted((root / "Previews42").glob("*.swift")):
        for match in re.finditer(r'\bPL\(\s*' + LITERAL, without_comments(path.read_text(encoding="utf-8"))):
            key = decode_swift(match[1])
            if key not in quicklook:
                problems.append(f"Previews42: no translation: {key!r} ({path.name})")
    return problems


def audit(root):
    required, rows = source_catalog(root)
    problems = check_sources(root)
    for path, row in rows:
        for language, translation in zip(("ru", "tr"), row[1:]):
            problem = translation_problem(row[0], translation)
            if problem:
                problems.append(f"{path.relative_to(root)} ({language}): {row[0]!r}: {problem}")
    directory = root / "Kit42/Resources/Translations"
    for language in CATALOG_LANGUAGES:
        problems.extend(check_catalog(directory / f"{language}.json", required))
    unexpected = {path.stem for path in directory.glob("*.json")} - set(CATALOG_LANGUAGES)
    problems.extend(f"unexpected language catalog: {language}" for language in sorted(unexpected))
    return problems, len(required)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sources", type=Path, default=ROOT, help="Sources directory (default: this repository)")
    args = parser.parse_args()
    problems, count = audit(args.sources)
    for problem in problems:
        print(problem)
    print(f"{len(problems)} problems" if problems else
          f"OK: {count} source keys; 20 languages; 17 JSON catalogs; format placeholders checked")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
