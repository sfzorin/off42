#!/usr/bin/env python3
"""Regression for printer rendering of cropped, rotated and annotated PDF pages."""
import os, re, subprocess, sys, tempfile
from pathlib import Path

binary = Path(sys.argv[1] if len(sys.argv) > 1 else '.build/debug/PDF42').resolve()
swift = r'''import AppKit
import PDFKit
let output = PDFDocument()
for annotated in [true, false] {
 for rotation in [0, 90, 180, 270] {
  let p = PDFPage(image: NSImage(size: NSSize(width: 36, height: 18), flipped: false) { rect in
   NSColor.white.setFill(); rect.fill(); NSColor.black.setFill(); NSRect(x: 2, y: 2, width: 5, height: 4).fill(); NSRect(x: 24, y: 11, width: 7, height: 3).fill(); return true
  })!
  p.setBounds(NSRect(x: 1, y: 1, width: 32, height: 15), for: .cropBox)
  if annotated { let a = PDFAnnotation(bounds: NSRect(x: 10, y: 5, width: 6, height: 5), forType: .square, withProperties: nil); a.interiorColor = .black; a.color = .black; p.addAnnotation(a) }
  p.rotation = rotation; output.insert(p, at: output.pageCount)
 }
}
assert(output.write(to: URL(fileURLWithPath: CommandLine.arguments[1])))
'''

def decode(stream):
    pages=[]
    for job in re.findall(rb'\^XA.*?\^XZ', stream, re.S):
        w=int(re.search(rb'\^PW(\d+)',job).group(1)); h=int(re.search(rb'\^LL(\d+)',job).group(1)); row=(w+7)//8
        bands=re.findall(rb'\^GFA,\d+,\d+,\d+,([0-9A-F]+)\^FS',job)
        packed=b''.join(bytes.fromhex(b.decode()) for b in bands)
        assert len(packed)==row*h
        pages.append([[bool(packed[y*row+x//8] & (128>>(x%8))) for x in range(w)] for y in range(h)])
    return pages

def rotate(p):
    return [list(row) for row in zip(*p[::-1])]

with tempfile.TemporaryDirectory() as tmp:
    root=Path(tmp); source=root/'main.swift'; source.write_text(swift); tool=root/'fixture'; pdf=root/'fixture.pdf'; job=root/'output.zpl'
    env=dict(os.environ, CLANG_MODULE_CACHE_PATH=str(root/'cache'))
    subprocess.run(['swiftc', str(source), '-o', str(tool)], check=True, env=env)
    subprocess.run([str(tool),str(pdf)], check=True)
    subprocess.run([str(binary),'--pdf-to-printer',str(pdf),str(job),'zpl'],check=True)
    pages=decode(job.read_bytes()); assert len(pages)==8
    for i in range(1,4): assert pages[i] == rotate(pages[i-1]), f'annotated rotation {i*90}'
    for i in range(5,8): assert pages[i] == rotate(pages[i-1]), f'plain rotation {(i-4)*90}'
    assert len(pages[0])==round(15*25.4/72*8) and len(pages[0][0])==round(32*25.4/72*8), 'crop dimensions'
    assert sum(sum(row) for row in pages[0]) > sum(sum(row) for row in pages[4]), 'annotation not rendered'
    print('PASS printer PDF crop, rotations and annotation')
