#!/usr/bin/env python3
"""Independent LibreOffice recalculation; known external numerical/error differences stay explicit."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess
import tempfile
import xml.etree.ElementTree as ET
import zipfile

parser=argparse.ArgumentParser()
parser.add_argument('--fixtures',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
parser.add_argument('--cached-only',action='store_true',help='Inspect saved XML caches without asking an office application to recalculate')
parser.add_argument('--soffice',default='/Applications/LibreOffice.app/Contents/MacOS/soffice')
args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
cases=json.loads((args.fixtures/'cases.json').read_text())
ns={'t':'urn:oasis:names:tc:opendocument:xmlns:table:1.0','o':'urn:oasis:names:tc:opendocument:xmlns:office:1.0','x':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
def values(path):
    with zipfile.ZipFile(path)as archive:
        if path.suffix=='.xlsx':
            root=ET.fromstring(archive.read('xl/worksheets/sheet1.xml'))
            return {cell.attrib['r']:('TRUE'if cell.findtext('x:v',namespaces=ns)=='1'else'FALSE')if cell.attrib.get('t')=='b'else cell.findtext('x:v',namespaces=ns)for cell in root.findall('.//x:c',ns)}
        root=ET.fromstring(archive.read('content.xml'));result={}
        for index,row in enumerate(root.findall('.//t:table-row',ns)):
            column=0
            for cell in row:
                repeat=int(cell.attrib.get('{'+ns['t']+'}number-columns-repeated',1))
                if column<=2<column+repeat:result['C'+str(index+1)]=cell.attrib.get('{'+ns['o']+'}value',''.join(cell.itertext()))
                column+=repeat
        return result
reports=[]
with tempfile.TemporaryDirectory(prefix='off42-calc-profile-')as profile:
    for extension,target in [('xlsx','ods'),('ods','xlsx')]:
        source=(args.fixtures/('statistics.'+extension)).resolve();out=(args.output/('from-'+extension)).resolve();out.mkdir(exist_ok=True)
        if args.cached_only:result=source
        else:
            run=subprocess.run([args.soffice,'-env:UserInstallation='+Path(profile).as_uri(),'--headless','--convert-to',target,'--outdir',str(out),str(source)],text=True,capture_output=True,check=True)
            (out/'conversion.log').write_text(run.stdout+run.stderr)
            result=out/('statistics.'+target);assert result.exists(),run.stdout+run.stderr
        actual=values(result);rows=[]
        for case in cases:
            value=actual.get(case['cell']);expected=case['expected']
            try:matched=math.isclose(float(value),float(expected),rel_tol=2e-8,abs_tol=0.0)
            except (ValueError,TypeError):matched=value==expected
            status='pass'
            if not matched:
                # These are external Calc 25.2 behaviors. Native results remain checked against mpmath.
                if not args.cached_only and case['formula']=='=ERF(8,9)'and value=='0':status='known_external_precision_loss'
                elif not args.cached_only and case['formula']=='=PERCENTILE.EXC(A1:A2,0)'and value in ('Err:502','#VALUE!'):status='known_external_error_classification'
                else:raise AssertionError((extension,case,value))
            rows.append({**case,'actual':value,'status':status})
        reports.append({'mode':'saved_cache'if args.cached_only else'office_recalculation','source_format':extension,'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'output_sha256':hashlib.sha256(result.read_bytes()).hexdigest(),'passed':sum(r['status']=='pass'for r in rows),'known_external_differences':[r for r in rows if r['status']!='pass'],'cases':rows})
(args.output/'results.json').write_text(json.dumps(reports,indent=2)+'\n')
print(json.dumps([{k:v for k,v in r.items()if k!='cases'}for r in reports],indent=2))
