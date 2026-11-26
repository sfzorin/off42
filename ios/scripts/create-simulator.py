#!/usr/bin/env python3
"""Creates a fresh simulator for the UI tests and App Store screenshots and prints its UDID (CI only).

    python3 ios/scripts/create-simulator.py iphone   # the newest "Pro Max" iPhone: the 6.9-inch App Store size
    python3 ios/scripts/create-simulator.py ipad     # the newest 13-inch iPad Pro: the 13-inch App Store size

Uses the newest installed iOS runtime and the newest matching device type it supports.
"""
import json
import re
import subprocess
import sys

PATTERNS = {
    "iphone": re.compile(r"iPhone-\d+-Pro-Max$"),
    "ipad": re.compile(r"iPad-Pro-13-inch"),
}


def simctl(*args):
    return subprocess.run(["xcrun", "simctl", *args], check=True, capture_output=True, text=True).stdout


def version_key(text):
    return [int(n) for n in re.findall(r"\d+", text)]


def main():
    kind = sys.argv[1]
    runtimes = [r for r in json.loads(simctl("list", "runtimes", "-j"))["runtimes"]
                if r.get("isAvailable") and r.get("platform", "iOS") == "iOS" and "iOS" in r["name"]]
    if not runtimes:
        sys.exit("no iOS simulator runtime installed")
    runtime = max(runtimes, key=lambda r: version_key(r["version"]))
    types = [t for t in runtime.get("supportedDeviceTypes", []) if PATTERNS[kind].search(t["identifier"])]
    if not types:
        sys.exit(f"no {kind} device type for {runtime['name']}")
    device_type = max(types, key=lambda t: (version_key(t["identifier"]), t["identifier"]))
    udid = simctl("create", f"OFF42 {kind}", device_type["identifier"], runtime["identifier"]).strip()
    print(f"{device_type['name']} · {runtime['name']} · {udid}", file=sys.stderr)
    print(udid)


if __name__ == "__main__":
    main()
