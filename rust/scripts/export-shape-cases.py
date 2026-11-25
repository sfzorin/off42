#!/usr/bin/env python3
"""Regenerate the geometry oracle from the actual macOS engine (requires macOS/Swift)."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCES = [ROOT / "Sources/PPT42" / name for name in ("PresetData.swift", "ShapeGeometry.swift")]
PROGRAM = r'''
import AppKit
let emuPerPoint: CGFloat = 12700
func point(_ p: CGPoint) -> [Double] { [Double(p.x), Double(p.y)] }
var cases: [[String: Any]] = []
for name in PresetDef.all.keys.sorted() {
    let def = PresetDef.all[name]!
    for size in [CGSize(width: 180, height: 90), CGSize(width: 53, height: 247)] {
        var variants: [[String: Int]] = [[:]]
        if !def.adjNames.isEmpty { variants.append(Dictionary(uniqueKeysWithValues: zip(def.adjNames, def.adjDefaults).map { ($0.0, Int($0.1 * 0.8)) })) }
        for adjust in variants {
            let paths: [[String: Any]] = def.subpaths(size: size, adjust: adjust).map { sub in
                var commands: [[Any]] = []
                sub.path.applyWithBlock { ptr in
                    let e = ptr.pointee
                    switch e.type {
                    case .moveToPoint: commands.append(["M"] + point(e.points[0]))
                    case .addLineToPoint: commands.append(["L"] + point(e.points[0]))
                    case .addCurveToPoint: commands.append(["C"] + point(e.points[0]) + point(e.points[1]) + point(e.points[2]))
                    case .addQuadCurveToPoint: commands.append(["Q"] + point(e.points[0]) + point(e.points[1]))
                    case .closeSubpath: commands.append(["Z"])
                    @unknown default: fatalError("Unknown geometry element")
                    }
                }
                return ["fill": sub.fill.rawValue, "stroke": sub.stroke, "commands": commands]
            }
            let rect = def.textRect(size: size, adjust: adjust) ?? CGRect(origin: .zero, size: size)
            cases.append(["name":name,"width":size.width,"height":size.height,"adjust":adjust,"paths":paths,"text_rect":[rect.minX,rect.minY,rect.width,rect.height],"handles":def.handlePoints(size:size,adjust:adjust).map(point)])
        }
    }
}
let data = try JSONSerialization.data(withJSONObject: cases, options: [.sortedKeys])
FileHandle.standardOutput.write(data)
'''
with tempfile.TemporaryDirectory(prefix="off42-shape-oracle-") as directory:
    directory = Path(directory)
    main = directory / "main.swift"
    main.write_text(PROGRAM, encoding="utf-8")
    binary = directory / "geometry-oracle"
    subprocess.run(["swiftc", "-module-cache-path", str(directory / "modules"), *map(str, SOURCES), str(main), "-o", str(binary)], check=True)
    cases = json.loads(subprocess.check_output([str(binary)]))
result = {"sources": {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in SOURCES}, "cases":cases}
out = ROOT / "rust/crates/ppt42-core/fixtures/macos-shape-cases.json"
out.write_text(json.dumps(result, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
print(f"{len(cases)} actual macOS geometry cases -> {out}")
