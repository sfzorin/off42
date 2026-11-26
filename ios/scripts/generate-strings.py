#!/usr/bin/env python3
"""Generates ios/OFF42/Localizable.xcstrings (a String Catalog) in the twenty languages of the site and the web app.

The strings are the Android app's: the web app's texts (web/src/locales/<lang>.json, mapped by the table in
android/scripts/generate-strings.py) and android/scripts/strings-android.json, with iOS wording and iOS-only strings
from ios/scripts/strings-ios.json on top. Placeholders {name} become positional format arguments (%1$@, %2$lld);
strings with arguments are used with String(format:), so their literal % signs are doubled. Missing translations
fall back to English and are reported. The catalog is committed; rerun after changing any source:

    python3 ios/scripts/generate-strings.py
"""
import importlib.util
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
WEB = ROOT / "web" / "src" / "locales"
ANDROID_SCRIPT = ROOT / "android" / "scripts" / "generate-strings.py"
ANDROID_EXTRA = ROOT / "android" / "scripts" / "strings-android.json"
IOS_EXTRA = ROOT / "ios" / "scripts" / "strings-ios.json"
OUT = ROOT / "ios" / "OFF42" / "Localizable.xcstrings"

# Site language code -> iOS localization (the .lproj name). They coincide; Portuguese is the site's Brazilian text and
# is offered as "pt" so that every Portuguese system language uses it.
IOS_LOCALIZATION = {"zh-Hans": "zh-Hans", "zh-Hant": "zh-Hant"}

IOS_ARGS = {"slide_n": [("number", "d")]}


def android_module():
    spec = importlib.util.spec_from_file_location("android_strings", ANDROID_SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def ios_text(text, args, where):
    if args:
        text = text.replace("%", "%%")
    for i, (name, kind) in enumerate(args, 1):
        if "{" + name + "}" not in text:
            sys.exit(f"{where}: placeholder {{{name}}} missing in {text!r}")
        text = text.replace("{" + name + "}", f"%{i}${'@' if kind == 's' else 'lld'}")
    if re.search(r"\{\w+\}", text):
        sys.exit(f"{where}: unmapped placeholder in {text!r}")
    return text


def main():
    android = android_module()
    android_extra = json.loads(ANDROID_EXTRA.read_text(encoding="utf-8"))
    ios_extra = json.loads(IOS_EXTRA.read_text(encoding="utf-8"))
    args = {name: spec[1] for name, spec in android.WEB_STRINGS.items()}
    args.update(android.EXTRA_ARGS)
    args.update(IOS_ARGS)

    texts = {}  # key -> {lang: text}
    missing = []
    for lang in android.LANGUAGES:
        web = json.loads((WEB / f"{lang}.json").read_text(encoding="utf-8"))
        for name, (key, _) in android.WEB_STRINGS.items():
            if key in web:
                texts.setdefault(name, {})[lang] = web[key]
            else:
                missing.append(f"{lang}: {key}")
        for source in (android_extra, ios_extra):
            for name, text in source.get(lang, {}).items():
                texts.setdefault(name, {})[lang] = text
    for name, by_lang in texts.items():
        if "en" not in by_lang:
            sys.exit(f"{name}: no English text")
        for lang in android.LANGUAGES:
            if lang not in by_lang:
                missing.append(f"{lang}: {name}")

    strings = {}
    for name in sorted(texts):
        localizations = {}
        for lang in android.LANGUAGES:
            text = texts[name].get(lang)
            if text is None:
                continue
            localizations[IOS_LOCALIZATION.get(lang, lang)] = {
                "stringUnit": {"state": "translated", "value": ios_text(text, args.get(name, []), f"{lang} {name}")}
            }
        strings[name] = {"extractionState": "manual", "localizations": localizations}
    catalog = {"sourceLanguage": "en", "strings": strings, "version": "1.0"}
    OUT.write_text(json.dumps(catalog, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if missing:
        print("Falling back to English for:\n  " + "\n  ".join(missing))
    print(f"Wrote {len(strings)} strings in {len(android.LANGUAGES)} languages to {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
