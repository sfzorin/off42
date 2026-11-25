#!/usr/bin/env python3
"""Extract the macOS reference's fixed formula examples for a portable gap report."""
import argparse
import hashlib
import json
from pathlib import Path
import re

root = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument("--source", type=Path, default=root / "Sources/XLS42/SelfTestFunctions.swift")
parser.add_argument("--output", type=Path, default=root / "rust/crates/xls42-core/fixtures/macos-function-cases.json")
args = parser.parse_args()
data = args.source.read_bytes()
source = data.decode("utf-8")
source = source[source.index("static let functionCases:"):source.index("/// Data + one case per row")]
quoted = r'"(?:[^"\\]|\\.)*"'
cases = []
for formula, expected in re.findall(r'\((' + quoted + r'),\s*(' + quoted + r'|nil)\)', source):
    formula = json.loads(formula)
    if not formula.startswith("="):
        raise ValueError("Unexpected reference case")
    cases.append({"formula": formula, "expected": None if expected == "nil" else json.loads(expected)})
if len(cases) < 400:
    raise ValueError("Incomplete reference extraction")
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps({
    "source": "Sources/XLS42/SelfTestFunctions.swift functionCases; expected values from the macOS reference, not a claim of Excel conformance",
    "source_sha256": hashlib.sha256(data).hexdigest(),
    "cases": cases,
}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"Exported {len(cases)} formula cases")
