#!/usr/bin/env python3
"""Run Suite42's native UI regressions and capture windows in isolated app bundles.

Requires `swift build`, a logged-in macOS desktop, and access to WindowServer.
Results go to build/ui-audit. No physical scanner is used.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parent.parent
APPS = ('PDF42', 'DOC42', 'XLS42', 'IMG42', 'PPT42', 'SCN42')
parser = argparse.ArgumentParser()
parser.add_argument('--mode', choices=('all', 'tests', 'screenshots'), default='all')
parser.add_argument('--app', choices=APPS)
parser.add_argument('--case', action='append', default=[], help='Run only named behavioral cases; requires --mode tests')
parser.add_argument('--binary', type=Path, help='Use this executable for the selected --app, including a packaged release')
parser.add_argument('--output', type=Path, default=REPO / 'build/ui-audit', help='Directory for isolated audit apps, logs and screenshots')
args = parser.parse_args()
if args.case and args.mode != "tests":
    parser.error("--case requires --mode tests")
if args.binary:
    if not args.app:
        parser.error('--binary requires --app')
    args.binary = args.binary.resolve()
    if not args.binary.is_file():
        parser.error('--binary must name an existing executable')
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
shots = out / 'screenshots'
shots.mkdir(exist_ok=True)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def resource_manifest(folder):
    return {str(path.relative_to(folder)): sha256(path)
            for path in sorted(folder.rglob('*')) if path.is_file()} if folder else {}


def unsigned_executable_sha256(path):
    # Audit bundles need a different identifier/signature. Compare their Mach-O bytes
    # after removing signatures from disposable copies, never from either executable.
    with tempfile.TemporaryDirectory(prefix='suite42-ui-code-') as folder:
        copy = Path(folder) / path.name
        shutil.copy2(path, copy)
        subprocess.run(['codesign', '--remove-signature', str(copy)], check=True, capture_output=True)
        return sha256(copy)


prepared_identities = {}


def prepare(app):
    bundle = out / 'apps' / f'{app}Audit.app'
    contents = bundle / 'Contents'
    (contents / 'MacOS').mkdir(parents=True, exist_ok=True)
    (contents / 'Resources').mkdir(exist_ok=True)
    binary = contents / 'MacOS' / app
    source_binary = (args.binary or REPO / '.build/debug' / app).resolve()
    source_hash = sha256(source_binary)
    source_code_hash = unsigned_executable_sha256(source_binary)
    shutil.copy2(source_binary, binary)
    if sha256(binary) != source_hash:
        raise RuntimeError(f'Executable changed while copying {source_binary}')
    # Use the tested bundle's own metadata/icon, including when checking an archived release.
    source_contents = source_binary.parent.parent
    packaged = source_contents.name == 'Contents' and (source_contents / 'Info.plist').is_file()
    metadata = source_contents / 'Info.plist' if packaged else REPO / 'Resources' / app / 'Info.plist'
    with metadata.open('rb') as f:
        info = plistlib.load(f)
    resource_candidates = (source_binary.parent / 'Suite42_Kit42.bundle',
                           source_binary.parent.parent / 'Resources/Suite42_Kit42.bundle')
    resource_bundle = next((path for path in resource_candidates if path.is_dir()), None)
    source_resource_hashes = resource_manifest(resource_bundle)
    # Legacy EN/RU/TR binaries embed their tables in Swift. Expanded locales require a resource bundle.
    if resource_bundle is None and set(info.get('CFBundleLocalizations', [])) - {'en', 'ru', 'tr'}:
        raise RuntimeError(f'Missing Kit42 translations for {source_binary}')
    destination = contents / 'Resources/Suite42_Kit42.bundle'
    if destination.exists():
        shutil.rmtree(destination)
    if resource_bundle:
        shutil.copytree(resource_bundle, destination)
        if resource_manifest(destination) != source_resource_hashes:
            raise RuntimeError(f'Resources changed while copying {resource_bundle}')
    info['CFBundleIdentifier'] = f'com.sfzorin.uiaudit.{app.lower()}'
    info['CFBundleName'] = info['CFBundleDisplayName'] = app + ' Audit'
    with (contents / 'Info.plist').open('wb') as f:
        plistlib.dump(info, f)
    icon = source_contents / 'Resources/AppIcon.icns' if packaged else REPO / 'Resources' / app / 'AppIcon.icns'
    shutil.copy2(icon, contents / 'Resources/AppIcon.icns')
    subprocess.run(['codesign', '--force', '-s', '-', str(bundle)], check=True, capture_output=True)
    tested_code_hash = unsigned_executable_sha256(binary)
    if tested_code_hash != source_code_hash:
        raise RuntimeError(f'Audit executable differs from {source_binary} beyond its signature')
    prepared_identities[app] = {
        'app': app, 'binary': str(source_binary), 'binary_sha256': source_hash,
        'tested_executable': str(binary), 'tested_executable_sha256': sha256(binary),
        'source_unsigned_sha256': source_code_hash, 'tested_unsigned_sha256': tested_code_hash,
        'code_equivalent': True,
        'equivalence_method': 'Byte-identical disposable Mach-O copies after codesign --remove-signature; source and tested signatures remain intact.',
        'source_resource_bundle': str(resource_bundle) if resource_bundle else None,
        'source_resource_manifest': source_resource_hashes,
        'tested_resource_manifest': resource_manifest(contents / 'Resources'),
        'tested_info_plist_sha256': sha256(contents / 'Info.plist'),
    }
    return binary


binaries = {app: prepare(app) for app in APPS if not args.app or args.app == app}
sources = {app: (args.binary or REPO / '.build/debug' / app).resolve() for app in binaries}
run_records = []


def run(app, name, arguments=(), settings=None, marker=None, snapshot=False, timeout=60):
    env = {k: v for k, v in os.environ.items() if not k.startswith('K42_')}
    env.update(K42_APPEARANCE='light', K42_UI_TEST_DIR=str(shots), K42_SCAN_DIR=str(out / 'saved-scans'))
    env.update(settings or {})
    target = shots / (name + '.png')
    dialog_target = shots / (name + '-dialog.png') if app == 'XLS42' and env.get('K42_ACTION') else None
    if snapshot:
        target.unlink(missing_ok=True)
        if dialog_target:
            dialog_target.unlink(missing_ok=True)
        env['K42_SNAPSHOT'] = str(target)
    log = out / (name + '.log')
    # LaunchServices gives PDFKit a real foreground application and key window.
    # A directly spawned executable can draw windows while nil-target menu commands
    # and native field editors still have no active application on recent macOS.
    foreground = (app == 'PDF42' and snapshot) or app in ('IMG42', 'SCN42')
    stderr_log = out / (name + '.stderr.log')
    if foreground:
        log.write_text('')
        stderr_log.write_text('')
        command = ['/usr/bin/open', '-n', '-W', '-a', str(binaries[app].parents[2]),
                   '--stdout', str(log), '--stderr', str(stderr_log), '--env', 'NSUnbufferedIO=YES']
        for key, value in env.items():
            if key.startswith('K42_'):
                command.extend(['--env', key + '=' + value])
        command.extend(['--args', *map(str, arguments)])
    else:
        command = [str(binaries[app]), *map(str, arguments)]
    try:
        result = subprocess.run(command, env=env, cwd=REPO, capture_output=True, timeout=timeout)
        text = (result.stdout + result.stderr).decode(errors='replace')
        if foreground:
            text = log.read_text(errors='replace') + stderr_log.read_text(errors='replace') + text
        log.write_text(text)
        ok = result.returncode == 0 and 'FAIL' not in text
        if marker:
            ok = ok and marker in text
        if snapshot:
            ok = ok and target.is_file() and target.stat().st_size > 1000
            if dialog_target:
                ok = ok and dialog_target.is_file() and dialog_target.stat().st_size > 1000
        print(f'{"PASS" if ok else "FAIL"} {name} (exit {result.returncode})', flush=True)
        run_records.append({'name': name, 'app': app, 'kind': 'behavioral' if marker else 'screenshot', 'passed': ok, 'launcher_exit_code': result.returncode})
        if not ok:
            print(text[-3500:], flush=True)
        return ok
    except subprocess.TimeoutExpired as error:
        if foreground:
            # Terminate only processes from this isolated audit bundle, never the user's app.
            processes = subprocess.check_output(['ps', '-axo', 'pid=,args='], text=True)
            for process in processes.splitlines():
                parts = process.strip().split(None, 1)
                if len(parts) == 2 and (parts[1] == str(binaries[app]) or parts[1].startswith(str(binaries[app]) + ' ')):
                    try: os.kill(int(parts[0]), signal.SIGTERM)
                    except ProcessLookupError: pass
            log.write_text(log.read_text(errors='replace') + stderr_log.read_text(errors='replace'))
        else:
            log.write_bytes((error.stdout or b'') + (error.stderr or b''))
        print(f'FAIL {name}: timeout; see {log}', flush=True)
        run_records.append({'name': name, 'app': app, 'kind': 'behavioral' if marker else 'screenshot', 'passed': False, 'timeout': True})
        return False


def fixture(app, arguments):
    executable = args.binary if app == args.app and args.binary else REPO / '.build/debug' / app
    result = subprocess.run([str(executable), *map(str, arguments)],
                            capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError(result.stderr.decode(errors='replace'))


results = []
if args.mode in ('all', 'tests'):
    if 'DOC42' in binaries and not (out / 'sample.docx').exists():
        fixture('DOC42', ['--demo', out / 'sample.docx'])
    if 'PDF42' in binaries and not (out / 'sample.pdf').exists():
        if not (out / 'sample.docx').exists(): fixture('DOC42', ['--demo', out / 'sample.docx'])
        fixture('DOC42', ['--pdf', out / 'sample.docx', out / 'sample.pdf'])
    tests = [
        ('XLS42', 'xls-ui', ('--uitest',), {'K42_PASTEBOARD': '1'}, 'ALL PASSED', False),
        ('IMG42', 'img-ui', ('--uitest',), {}, 'IMG42 UI: 0 failures', False),
        ('IMG42', 'img-markup', ('--audit', 'markup'), {}, 'IMG42 markup regressions:', False),
        ('IMG42', 'img-editing', ('--audit', 'editing'), {}, 'IMG42 editing regressions:', False),
        ('IMG42', 'img-files', ('--audit', 'files'), {}, 'IMG42 file UI regressions:', False),
        ('IMG42', 'img-core', ('--audit', 'core'), {}, 'IMG42 core regressions:', False),
        ('IMG42', 'img-window-modes', ('--audit', 'window-modes'), {}, 'IMG42 window mode regressions:', False),
        ('IMG42', 'img-live-text', ('--audit', 'live-text'), {}, 'IMG42 Live Text regressions:', False),
        ('IMG42', 'img-vision', ('--audit', 'vision'), {}, 'IMG42 Vision regressions:', False),
        ('SCN42', 'scn-ui', ('--uitest',), {}, 'SCN42 UI: 0 failures', False),
        ('SCN42', 'scn-core', ('--audit', 'core'), {}, 'SCN42 core regressions:', False),
        ('SCN42', 'scn-scanner', ('--audit', 'scanner'), {}, 'SCN42 scanner regressions:', False),
        ('SCN42', 'scn-editing', ('--audit', 'editing'), {}, 'SCN42 editing regressions:', False),
        ('SCN42', 'scn-files', ('--audit', 'files'), {}, 'SCN42 file UI regressions:', False),
        ('SCN42', 'scn-marks', ('--audit', 'marks'), {}, 'SCN42 mark regressions:', False),
        ('DOC42', 'doc-technical', (), {'K42_NEW': '1', 'K42_SCRIPT': 'audittechnical'}, 'DOC technical:', True),
        ('PDF42', 'pdf-technical', (out / 'sample.pdf',), {'K42_AUDIT': 'technical'}, 'PDF technical:', True),
        ('SCN42', 'scn-vision', ('--audit', 'vision'), {}, 'SCN42 Vision regressions:', False),
        ('DOC42', 'doc-panels', (), {'K42_NEW': '1', 'K42_SCRIPT': 'auditpanels', 'K42_AUDIT_RECENT': '1'}, 'DOC panel UI regressions: 16/16 passed', True),
        ('DOC42', 'doc-rulers', (), {'K42_NEW': '1', 'K42_SCRIPT': 'auditrulers'}, 'DOC ruler UI regressions:', True),
        ('DOC42', 'doc-editing', (), {'K42_NEW': '1', 'K42_SCRIPT': 'auditediting'}, 'DOC editing UI regressions:', True),
        ('DOC42', 'doc-document', (), {'K42_NEW': '1', 'K42_SCRIPT': 'auditdocument'}, 'DOC document UI regressions:', True),
        ('DOC42', 'doc-pagination', (out / 'sample.docx',), {'K42_SCRIPT': 'auditpagination'}, 'DOC pagination UI regressions:', True),
        ('PDF42', 'pdf-markup', (out / 'sample.pdf',), {'K42_AUDIT': 'markup'}, 'PDF markup UI regressions:', True),
        ('PDF42', 'pdf-pages', (out / 'sample.pdf',), {'K42_AUDIT': 'pages'}, 'PDF pages UI regressions:', True),
        ('PDF42', 'pdf-core', (out / 'sample.pdf',), {'K42_AUDIT': 'core'}, 'PDF core UI regressions:', True),
        ('PDF42', 'pdf-files', (out / 'sample.pdf',), {'K42_AUDIT': 'files'}, 'PDF document lifecycle regressions:', True),
        ('PDF42', 'mark-samples', ('--mark-samples', out / 'marks'), {}, 'Mark samples: ', False),
        ('PDF42', 'signature-fidelity', ('--signature-fidelity-synthetic', out / 'signature-fidelity'), {}, 'Synthetic checks: passed', False),
        ('PPT42', 'ppt-notes', (), {'K42_NEW': '1', 'K42_DO': 'auditnotes', 'K42_AUDIT_RECENT': '1'}, 'PPT notes UI regressions: 6/6 passed', True),
        ('PPT42', 'ppt-editing', (), {'K42_NEW': '1', 'K42_DO': 'auditediting', 'K42_PASTEBOARD': '1'}, 'PPT editing UI regressions:', True),
        ('PPT42', 'ppt-text', (), {'K42_NEW': '1', 'K42_DO': 'audittext', 'K42_PASTEBOARD': '1'}, 'PPT text/table/show UI regressions:', True),
        ('PPT42', 'ppt-files', (), {'K42_NEW': '1', 'K42_DO': 'auditfiles'}, 'PPT document lifecycle regressions:', True),
        ('PPT42', 'ppt-inspector', (), {'K42_NEW': '1', 'K42_DO': 'auditinspector'}, 'PPT inspector UI regressions:', True),
    ]
    available = {name for app, name, *_ in tests if app in binaries}
    if set(args.case) - available:
        parser.error("Unknown or unselected cases: " + ", ".join(sorted(set(args.case) - available)))
    for app, name, arguments, settings, marker, snapshot in tests:
        if app in binaries and (not args.case or name in args.case):
            # The mark sample sheet renders ~240 signatures and stamps in a debug build.
            timeout = 300 if name in ('mark-samples', 'signature-fidelity') else 180 if name in ('img-vision', 'scn-vision', 'scn-scanner') else 60
            results.append(run(app, name, arguments, settings, marker, snapshot, timeout=timeout))

if args.mode in ('all', 'screenshots'):
    # Fixtures come from the apps' existing headless exporters, independent of UI hooks.
    if not (out / 'sample.docx').exists(): fixture('DOC42', ['--demo', out / 'sample.docx'])
    if not (out / 'sample.pdf').exists(): fixture('DOC42', ['--pdf', out / 'sample.docx', out / 'sample.pdf'])
    if not (out / 'sample.pptx').exists(): fixture('PPT42', ['--demo', out / 'sample.pptx'])
    if not (out / 'sample.xlsx').exists(): fixture('XLS42', ['--feature-demo', out / 'sample.xlsx'])
    if not (out / 'scans/feed-1.png').exists(): fixture('SCN42', ['--make-test-inputs', out / 'scans'])
    configs = {
        'PDF42': ([out / 'sample.pdf'], {'K42_MARKUP': '1'}, '900x650'),
        'DOC42': ([], {'K42_NEW': '1', 'K42_SCRIPT': 'type Interface audit — проверка интерфейса;findbar'}, '700x420'),
        'XLS42': ([], {'K42_OPEN': str(out / 'sample.xlsx')}, '900x600'),
        'IMG42': ([out / 'scans/feed-1.png'], {'K42_MARKUP': '1', 'K42_INFO': '1', 'K42_FILMSTRIP': '1'}, '800x600'),
        'PPT42': ([out / 'sample.pptx'], {'K42_INSPECTOR': 'text', 'K42_SELECT': '0'}, '820x520'),
        'SCN42': ([], {'K42_DEMO_PAGES': '3', 'K42_FAKE_SCANNER': str(out / 'scans')}, '860x560'),
    }
    for app, (arguments, settings, small) in configs.items():
        if app not in binaries: continue
        for appearance, size in [('light', '1180x780'), ('dark', small)]:
            options = dict(settings, K42_APPEARANCE=appearance, K42_SIZE=size, K42_AUDIT_RECENT='1')
            results.append(run(app, app.lower() + '-' + appearance, arguments, options, snapshot=True))
    if 'PDF42' in binaries:
        # Opening a popover is visual evidence, not a functional generator regression.
        for generator in ('signature', 'seal', 'learn'):
            results.append(run('PDF42', 'mark-' + generator, [out / 'sample.pdf'],
                               {'K42_OPEN_GENERATOR': generator, 'K42_SNAPSHOT_SHEET': '1'}, snapshot=True))
        # Insert preview with resolution choice: maximum on a vector page, the scan's own dpi on a scanned page.
        results.append(run('PDF42', 'mark-insert-vector', [out / 'sample.pdf'], {'K42_OPEN_MARK_INSERT': 'seal', 'K42_SNAPSHOT_SHEET': '1'}, snapshot=True))
        results.append(run('PDF42', 'mark-insert-scan', [out / 'sample.pdf'], {'K42_OPEN_MARK_INSERT': 'signature', 'K42_MARK_SCAN_DPI': '300', 'K42_SNAPSHOT_SHEET': '1'}, snapshot=True))
        # Regression 2026-11-06: long translations squeezed the Form/Pen/Ink row ("Любая…", missing "Форма:").
        for lang in ('ru', 'tr'):
            results.append(run('PDF42', f'mark-signature-layout-{lang}', [out / 'sample.pdf', '-Suite42Language', lang],
                               {'K42_OPEN_GENERATOR': 'signature', 'K42_SNAPSHOT_SHEET': '1', 'K42_GENERATOR_LAYOUT_AUDIT': '1'},
                               f'Generator layout: PASS {lang}', snapshot=True))
        results.append(run('PDF42', 'pdf-properties', [out / 'sample.pdf'], {'K42_ACTION': 'propertiesAction'}, snapshot=True))
    if 'DOC42' in binaries:
        for name, script, appearance in [('doc-book', 'viewmode 1', 'light'),
                                          ('doc-columns', 'cols 2', 'dark'),
                                          ('doc-web', 'viewmode 3', 'light')]:
            results.append(run('DOC42', name, [out / 'sample.docx'],
                               {'K42_SCRIPT': script, 'K42_APPEARANCE': appearance, 'K42_SIZE': '1180x780'}, snapshot=True))
    if 'XLS42' in binaries:
        results.append(run('XLS42', 'xls-format', [], {'K42_OPEN': str(out / 'sample.xlsx'), 'K42_ACTION': 'formatCellsAction:'}, snapshot=True))
print(f'{sum(results)}/{len(results)} UI runs passed', flush=True)
identities = []
for app, path in sources.items():
    identity = dict(prepared_identities[app])
    contents = binaries[app].parent.parent
    resource_bundle = identity['source_resource_bundle']
    identity['unchanged_during_audit'] = sha256(path) == identity['binary_sha256']
    identity['tested_executable_unchanged'] = sha256(binaries[app]) == identity['tested_executable_sha256']
    identity['source_resources_unchanged'] = resource_manifest(Path(resource_bundle) if resource_bundle else None) == identity['source_resource_manifest']
    identity['tested_resources_unchanged'] = resource_manifest(contents / 'Resources') == identity['tested_resource_manifest']
    identity['tested_metadata_unchanged'] = sha256(contents / 'Info.plist') == identity['tested_info_plist_sha256']
    identities.append(identity)
unchanged = all(all(item[key] for key in ('unchanged_during_audit', 'tested_executable_unchanged',
                                        'source_resources_unchanged', 'tested_resources_unchanged',
                                        'tested_metadata_unchanged')) for item in identities)
(out / 'result.json').write_text(json.dumps({'binaries': identities, 'runs': run_records}, indent=2) + '\n')
if not unchanged: print('FAIL: an executable, resource bundle or audit metadata changed during the UI audit', flush=True)
sys.exit(0 if all(results) and unchanged else 1)
