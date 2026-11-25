#!/usr/bin/env python3
import json, pathlib, shutil, sys
m = json.load(open(sys.argv[1], encoding='utf-8'))
out = pathlib.Path(sys.argv[2])
# Include all resolved crates (including build-time dependencies), conservatively.
resolved = {n['id'] for n in m['resolve']['nodes']}
with (out / 'DEPENDENCIES.txt').open('w', encoding='utf-8') as f:
    for p in sorted(m['packages'], key=lambda p: (p['name'], p['version'])):
        if p['id'] not in resolved or p['source'] is None:
            continue
        f.write(f"{p['name']} {p['version']}: {p.get('license') or 'see included license files'}\n")
        directory = pathlib.Path(p['manifest_path']).parent
        files = [x for x in directory.iterdir() if x.is_file() and x.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE'))]
        if p.get('license_file'):
            files.append(directory / p['license_file'])
        dest = out / f"{p['name']}-{p['version']}"
        for index, src in enumerate(dict.fromkeys(files)):
            if src.is_file():
                dest.mkdir(exist_ok=True)
                shutil.copyfile(src, dest / f'{index}-{src.name}')

# The generated OOXML preset data is MPL-2.0 covered; distribute its source beside the notice.
presets = pathlib.Path(__file__).resolve().parent.parent / 'crates/ppt42-core/src/geometry'
dest = out / 'drawingml-presets'
dest.mkdir(exist_ok=True)
for name in ('presets.txt', 'NOTICE.txt', 'MPL-2.0.txt'):
    shutil.copyfile(presets / name, dest / name)
