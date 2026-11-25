#!/usr/bin/env python3
"""Optional independent barcode QA. Requires Pillow, zxing-cpp and Poppler, only in the QA environment."""
import argparse
import json
from pathlib import Path
import subprocess
from PIL import Image
import zxingcpp

parser=argparse.ArgumentParser()
parser.add_argument("folder",type=Path)
args=parser.parse_args()
cases=json.loads((args.folder/"cases.json").read_text(encoding="utf-8"))
formats={"qr":zxingcpp.BarcodeFormat.QRCode,"dataMatrix":zxingcpp.BarcodeFormat.DataMatrix,
         "aztec":zxingcpp.BarcodeFormat.Aztec,"code128":zxingcpp.BarcodeFormat.Code128}
results=[]
for case in cases:
    name=case["name"]
    output=args.folder/(name+"-pdf-render")
    subprocess.run(["pdftoppm","-singlefile","-scale-to","1800","-png",str(args.folder/(name+".pdf")),str(output)],check=True,capture_output=True)
    for path in [args.folder/(name+".png"),output.with_suffix(".png")]:
        codes=zxingcpp.read_barcodes(Image.open(path))
        assert len(codes)==1,(path,[(str(code.format),code.text)for code in codes])
        code=codes[0]
        assert code.text==case["value"] and code.format==formats[case["format"]],(path,code.text,case["value"])
        results.append({"file":path.name,"format":str(code.format),"text":code.text})
print(f"Decoded {len(results)} PNG/PDF outputs independently")
(args.folder/"decoded-results.json").write_text(json.dumps(results,ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
