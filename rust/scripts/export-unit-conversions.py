#!/usr/bin/env python3
"""Export the suite's shared unit constants from the macOS source, without executing it."""
import argparse
import ast
import json
from pathlib import Path
import re


def number(expression):
    node = ast.parse(expression, mode="eval").body
    def read(node):
        if isinstance(node, ast.Constant) and type(node.value) in (int, float):
            return node.value
        if isinstance(node, ast.BinOp) and isinstance(node.op, (ast.Mult, ast.Div)):
            a, b = read(node.left), read(node.right)
            return a * b if isinstance(node.op, ast.Mult) else a / b
        raise ValueError("Unsupported unit constant")
    return read(node)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source = args.source.read_text(encoding="utf-8").split("enum UnitConversion {", 1)[1]
    units = [[name, kind, number(value), prefix == "true"] for name, kind, value, prefix in re.findall(r'"([^"]+)":\s*\(\.(\w+),\s*([^,]+),\s*(true|false)\)', source)]
    # Excel's flb means foot pound-force, not foot poundal. The source
    # currently uses the poundal factor. Derive it from the shared lbf and ft.
    # https://support.microsoft.com/en-us/excel/functions/convert-function
    # https://www.nist.gov/pml/special-publication-811/nist-guide-si-appendix-b-conversion-factors/nist-guide-si-appendix-b9
    factors = {entry[0]: entry[2] for entry in units}
    for entry in units:
        if entry[0] == "flb":
            entry[2] = factors["lbf"] * factors["ft"]
    prefix_source = source.split("static let prefixes:", 1)[1].split("static let temperatures:", 1)[0]
    prefixes = [[name, number(value)] for name, value in re.findall(r'"([^"]+)":\s*([\deE.+*/ -]+)', prefix_source)]
    assert len(units) > 100 and len(prefixes) > 20
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({"units": units, "prefixes": prefixes}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"{len(units)} units, {len(prefixes)} prefixes")

if __name__ == "__main__":
    main()
