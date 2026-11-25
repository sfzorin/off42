#!/usr/bin/env python3
"""Independent LibreOffice/Poppler QA for preset-fixtures (not an app dependency)."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

parser=argparse.ArgumentParser()
parser.add_argument('--fixtures',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
parser.add_argument('--soffice',default='/Applications/LibreOffice.app/Contents/MacOS/soffice')
args=parser.parse_args()
args.output.mkdir(parents=True,exist_ok=True)
root=Path(__file__).resolve().parents[1]
names=[s.split('|')[0]for s in (root/'crates/ppt42-core/src/geometry/presets.txt').read_text().splitlines()]
results=[]
with tempfile.TemporaryDirectory(prefix='off42-office-profile-')as profile:
    for extension in ('pptx','odp'):
        source=(args.fixtures/('presets.'+extension)).resolve()
        output=(args.output/extension).resolve();output.mkdir(exist_ok=True)
        run=subprocess.run([args.soffice,'-env:UserInstallation='+Path(profile).as_uri(),'--headless','--convert-to','pdf','--outdir',str(output),str(source)],capture_output=True,text=True,check=True)
        (output/'conversion.log').write_text(run.stdout+run.stderr)
        pdf=output/'presets.pdf'
        info=subprocess.check_output(['pdfinfo',str(pdf)],text=True)
        count=int(re.search(r'^Pages:\s+(\d+)',info,re.M)[1]);assert count==16,(extension,count)
        width,height=map(float,re.search(r'^Page size:\s+([\d.]+) x ([\d.]+)',info,re.M).groups())
        assert abs(width-720)<0.03 and abs(height-405)<0.03,(extension,width,height)
        text=subprocess.check_output(['pdftotext','-layout',str(pdf),'-'],text=True)
        (output/'text.txt').write_text(text)
        compact=''.join(text.split())
        assert all(name in compact for name in names),[name for name in names if name not in compact]
        results.append({'format':extension,'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'pdf_sha256':hashlib.sha256(pdf.read_bytes()).hexdigest(),'pages':count,'size_points':[width,height],'catalog_labels':len(names)})
(args.output/'results.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps(results,indent=2))
