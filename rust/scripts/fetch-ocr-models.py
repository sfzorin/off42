#!/usr/bin/env python3
"""Stage twenty pinned offline OCR models and their license, with SHA-256 checks."""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import shutil
import sys
import urllib.request

MANIFEST = Path(__file__).resolve().parent.parent / 'native/ocr/models.json'


def fetch(url, digest, limit, target):
    if target.exists():
        data = target.read_bytes()
        if len(data) <= limit and hashlib.sha256(data).hexdigest() == digest:
            return
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read(limit + 1)
    if len(data) > limit or hashlib.sha256(data).hexdigest() != digest:
        raise ValueError(f'OCR checksum/size mismatch: {target.name}')
    temporary = target.with_suffix(target.suffix + '.tmp')
    temporary.write_bytes(data)
    temporary.replace(target)


def main():
    if len(sys.argv) != 2:
        raise ValueError('Usage: fetch-ocr-models.py APP_DIRECTORY')
    root = Path(sys.argv[1])
    output = root / 'ocr/tessdata'
    output.mkdir(parents=True, exist_ok=True)
    manifest = json.loads(MANIFEST.read_text())
    base = f"https://raw.githubusercontent.com/{manifest['repository']}/{manifest['revision']}"
    def model(item):
        target = output / f"{item['code']}.traineddata"
        fetch(f'{base}/{target.name}', item['sha256'], item['bytes'], target)
        if target.stat().st_size != item['bytes']:
            raise ValueError(f'Invalid OCR size: {target.name}')
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
        list(executor.map(model, manifest['models']))
    licenses = root / 'licenses/ocr'
    licenses.mkdir(parents=True, exist_ok=True)
    fetch(f'{base}/LICENSE', manifest['license_sha256'], 100_000, licenses / 'tessdata-fast-LICENSE')
    shutil.copyfile(MANIFEST, root / 'ocr/models.json')
    print(f"Verified {len(manifest['models'])} offline OCR models ({sum(m['bytes'] for m in manifest['models'])} bytes)")


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError) as error:
        sys.exit(str(error))
