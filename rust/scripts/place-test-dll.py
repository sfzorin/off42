"""Build the given cargo test targets without running them and copy a DLL next to every test executable.

Cargo 1.100 moved test executables from target/<triple>/debug/deps to per-unit build directories
(target/<triple>/debug/build/<crate>/<hash>/out), so a DLL copied only into deps is no longer found by
LoadLibraryEx(LOAD_LIBRARY_SEARCH_APPLICATION_DIR). This asks cargo where the executables are instead.

Usage: python scripts/place-test-dll.py DLL [cargo test arguments...]
"""
import json
import shutil
import subprocess
import sys
from pathlib import Path


def main():
    dll = Path(sys.argv[1]).resolve()
    if not dll.is_file():
        sys.exit(f"Missing DLL: {dll}")
    command = ["cargo", "test", *sys.argv[2:], "--no-run", "--message-format=json-render-diagnostics"]
    result = subprocess.run(command, stdout=subprocess.PIPE, text=True, encoding="utf-8")
    if result.returncode != 0:
        sys.exit(result.returncode)
    folders = set()
    for line in result.stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        executable = message.get("executable") if message.get("reason") == "compiler-artifact" else None
        if executable and message.get("profile", {}).get("test"):
            folders.add(Path(executable).parent)
    if not folders:
        sys.exit("cargo reported no test executables")
    for folder in sorted(folders):
        shutil.copy2(dll, folder / dll.name)
    print(f"{dll.name} placed next to test executables in {len(folders)} folders")


if __name__ == "__main__":
    main()
