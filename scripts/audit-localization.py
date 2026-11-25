#!/usr/bin/env python3
"""Smoke-test localized welcome windows without changing the suite's language.

Requires a built executable and a logged-in macOS desktop. Copies the binary and
its Kit42 resource bundle into an isolated, ad-hoc-signed app, then launches each
requested language using -Suite42Language. Checks the native picker, menu,
catalog loading and label fit. Screenshots still need visual review; this does
not prove translation quality or document editing behavior.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import struct
import subprocess
import sys

from check_strings import APPS, LANGUAGES

REPO = Path(__file__).resolve().parent.parent


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def resource_manifest(bundle):
    return {str(path.relative_to(bundle)): sha256(path)
            for path in sorted(bundle.rglob('*')) if path.is_file()}


def prepare(app, binary, output):
    bundle = output / "apps" / f"{app}LocalizationAudit.app"
    contents = bundle / "Contents"
    (contents / "MacOS").mkdir(parents=True, exist_ok=True)
    resources = contents / "Resources"
    resources.mkdir(exist_ok=True)
    executable = contents / "MacOS" / app
    shutil.copy2(binary, executable)
    candidates = (binary.parent / "Suite42_Kit42.bundle",
                  binary.parent.parent / "Resources/Suite42_Kit42.bundle")
    source = next((candidate for candidate in candidates if candidate.is_dir()), None)
    if source is None:
        raise RuntimeError(f"No Suite42_Kit42.bundle beside {binary} or in its app resources")
    destination = resources / source.name
    if destination.exists():
        shutil.rmtree(destination)
    shutil.copytree(source, destination)
    packaged_info = binary.parent.parent / 'Info.plist'
    info_source = packaged_info if packaged_info.is_file() else REPO / "Resources" / app / "Info.plist"
    with info_source.open("rb") as file:
        info = plistlib.load(file)
    if info.get('CFBundleLocalizations') != list(LANGUAGES):
        raise RuntimeError(f'{info_source} does not declare the expected 20 languages')
    info["CFBundleIdentifier"] = f"com.sfzorin.localizationaudit.{app.lower()}"
    info["CFBundleName"] = info["CFBundleDisplayName"] = f"{app} Localization Audit"
    with (contents / "Info.plist").open("wb") as file:
        plistlib.dump(info, file)
    packaged_icon = binary.parent.parent / 'Resources/AppIcon.icns'
    icon = packaged_icon if packaged_icon.is_file() else REPO / "Resources" / app / "AppIcon.icns"
    shutil.copy2(icon, resources / "AppIcon.icns")
    subprocess.run(["codesign", "--force", "-s", "-", str(bundle)], check=True, capture_output=True)
    return executable, source, destination


def png_dimensions(path):
    if not path.is_file() or path.stat().st_size <= 1000:
        return None
    header = path.read_bytes()[:24]
    if len(header) != 24 or header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        return None
    width, height = struct.unpack(">II", header[16:24])
    return [width, height] if width > 0 and height > 0 else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", choices=APPS[1:], default="PDF42")
    parser.add_argument("--binary", type=Path, help="Debug or packaged executable (default: .build/debug/<app>)")
    parser.add_argument("--language", action="append", choices=LANGUAGES, help="Repeat to select languages; default: all 20")
    parser.add_argument("--appearance", choices=("light", "dark"), default="light")
    parser.add_argument("--output", type=Path, default=REPO / "build/localization-audit")
    args = parser.parse_args()
    binary = (args.binary or REPO / ".build/debug" / args.app).resolve()
    if not binary.is_file():
        parser.error(f"executable does not exist: {binary}")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    shots = output / "screenshots"
    shots.mkdir(exist_ok=True)
    source_hash = sha256(binary)
    executable, resources, copied_resources = prepare(args.app, binary, output)
    # Re-signing the isolated app changes its Mach-O signature. Record the exact
    # executable that is launched, separately from the input binary.
    tested_hash = sha256(executable)
    resource_hashes = resource_manifest(copied_resources)
    languages = list(dict.fromkeys(args.language or LANGUAGES))
    runs = []
    for language in languages:
        name = f"{args.app.lower()}-{language}-{args.appearance}"
        snapshot = shots / (name + ".png")
        snapshot.unlink(missing_ok=True)
        native_report = output / (name + '-native.json')
        native_report.unlink(missing_ok=True)
        environment = {key: value for key, value in os.environ.items() if not key.startswith("K42_")}
        environment.update(K42_APPEARANCE=args.appearance, K42_SNAPSHOT=str(snapshot),
                           K42_SNAPSHOT_DELAY="1", K42_UI_TEST_DIR=str(shots),
                           K42_LOCALIZATION_AUDIT=str(native_report), K42_WELCOME="1")
        command = [str(executable), "-Suite42Language", language]
        record = {"language": language, "command": command, "screenshot": str(snapshot)}
        log = output / (name + ".log")
        try:
            result = subprocess.run(command, capture_output=True, env=environment, cwd=output, timeout=30)
            log.write_bytes(result.stdout + result.stderr)
            dimensions = png_dimensions(snapshot)
            native = json.loads(native_report.read_text()) if native_report.is_file() else {}
            native_passed = native.get('language') == language and native.get('failures') == []
            record.update(returncode=result.returncode, dimensions=dimensions,
                          native_report=str(native_report),
                          passed=result.returncode == 0 and dimensions is not None and native_passed)
        except subprocess.TimeoutExpired as error:
            log.write_bytes((error.stdout or b"") + (error.stderr or b""))
            record.update(passed=False, error="30-second timeout")
        runs.append(record)
        print(f"{'PASS' if record['passed'] else 'FAIL'} {name}", flush=True)
    copy_unchanged = tested_hash == sha256(executable) and resource_hashes == resource_manifest(copied_resources)
    report = {"app": args.app, "binary": str(binary), "binary_sha256": source_hash,
              "source_binary_unchanged": source_hash == sha256(binary),
              "tested_executable": str(executable), "tested_executable_sha256": tested_hash,
              "tested_copy_unchanged": copy_unchanged, "resource_manifest": resource_hashes,
              "resource_bundle": str(resources), "appearance": args.appearance,
              "coverage": "Native 20-item picker, selected language, File menu, catalog loading, welcome label/action fit, and PNG capture. Screenshots require visual review; no editing coverage.",
              "runs": runs}
    (output / f"{args.app.lower()}-{args.appearance}-result.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"{sum(run['passed'] for run in runs)}/{len(runs)} localized launches captured", flush=True)
    return 0 if all(run["passed"] for run in runs) and copy_unchanged else 1


if __name__ == "__main__":
    sys.exit(main())
