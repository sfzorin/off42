#!/usr/bin/env python3
"""App Store listings of OFF42 for iPad and iPhone in the 20 site languages.

    python3 appstore/ios/make_listings.py            # writes appstore/ios/listings/<language>.json
    python3 appstore/ios/make_listings.py --check    # CI: the committed files are current and within the limits

Name, subtitle, keywords and the first paragraph (iPhone/iPad wording) are in appstore/ios/texts.json. The
promotional text is the Play short description and the rest of the description is the Play full description
(android/play/listing.json), so both stores say the same thing. Keywords avoid other companies' product names.
Every field is checked against the App Store Connect limits.
"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PLAY = ROOT / "android" / "play" / "listing.json"
OUT = HERE / "listings"

SUPPORT_URL = "https://off42.cc/en/support/"
PRIVACY_URL = "https://off42.cc/en/privacy/"
MARKETING_URL = "https://off42.cc/"

# Site language → (App Store Connect localization, Play locale). App Store Connect offers no Filipino localization.
LOCALES = {
    "en": ("en-US", "en-US"), "ar": ("ar-SA", "ar"), "de": ("de-DE", "de-DE"), "es": ("es-ES", "es-ES"),
    "fil": (None, "fil"), "fr": ("fr-FR", "fr-FR"), "hu": ("hu", "hu-HU"), "id": ("id", "id"), "it": ("it", "it-IT"),
    "ja": ("ja", "ja-JP"), "ko": ("ko", "ko-KR"), "ms": ("ms", "ms"), "pl": ("pl", "pl-PL"), "pt": ("pt-BR", "pt-BR"),
    "ru": ("ru", "ru-RU"), "th": ("th", "th"), "tr": ("tr", "tr-TR"), "vi": ("vi", "vi"), "zh-Hans": ("zh-Hans", "zh-CN"),
    "zh-Hant": ("zh-Hant", "zh-TW"),
}

LIMITS = {"name": 30, "subtitle": 30, "promotionalText": 170, "description": 4000, "keywords": 100}


def listing(language, texts, play):
    store, play_locale = LOCALES[language]
    full = play[play_locale]["full"]
    first, _, rest = full.partition("\n\n")
    if not rest:
        raise SystemExit(f"{play_locale}: Play description has no paragraphs")
    return {
        "appStoreLocale": store,
        "name": texts["name"],
        "subtitle": texts["subtitle"],
        "promotionalText": play[play_locale]["short"],
        "description": texts["intro"] + "\n\n" + rest,
        "keywords": texts["keywords"],
        "supportUrl": SUPPORT_URL,
        "marketingUrl": MARKETING_URL,
        "privacyPolicyUrl": PRIVACY_URL,
    }


def problems_of(language, entry):
    problems = []
    for field, limit in LIMITS.items():
        if len(entry[field]) > limit:
            problems.append(f"{language} {field}: {len(entry[field])} > {limit} characters")
    keywords = entry["keywords"].split(",")
    if any(k != k.strip() or not k for k in keywords):
        problems.append(f"{language} keywords: empty entries or spaces around commas")
    if len(set(keywords)) != len(keywords):
        problems.append(f"{language} keywords: duplicates")
    for word in ("Android", "Google Play", "Microsoft", "Word", "Excel", "PowerPoint"):
        if word.lower() in entry["keywords"].lower():
            problems.append(f"{language} keywords: other companies' product name {word!r}")
    if "<" in entry["description"]:
        problems.append(f"{language} description: '<' is not allowed")
    return problems


def main():
    check = "--check" in sys.argv
    texts = json.loads((HERE / "texts.json").read_text(encoding="utf-8"))
    play = json.loads(PLAY.read_text(encoding="utf-8"))
    problems = []
    if set(texts) != set(LOCALES):
        problems.append(f"texts.json languages differ: {sorted(set(LOCALES) ^ set(texts))}")
    OUT.mkdir(exist_ok=True)
    for language in LOCALES:
        entry = listing(language, texts[language], play)
        problems += problems_of(language, entry)
        text = json.dumps(entry, ensure_ascii=False, indent=2) + "\n"
        path = OUT / f"{language}.json"
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                problems.append(f"{path.relative_to(ROOT)} is not current: run python3 appstore/ios/make_listings.py")
        else:
            path.write_text(text, encoding="utf-8")
    if problems:
        sys.exit("\n".join(problems))
    print(f"{len(LOCALES)} listings {'checked' if check else 'written to ' + str(OUT.relative_to(ROOT))}")


if __name__ == "__main__":
    main()
