#!/usr/bin/env python3
"""Compare vector label geometry against native CoreGraphics output using Poppler."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from PIL import Image,ImageChops,ImageFilter


def main():
    parser=argparse.ArgumentParser();parser.add_argument('fixtures',type=Path);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=False)
    results=[]
    def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
    def blue(path):
        image=Image.open(path).convert('RGB')
        return Image.frombytes('L',image.size,bytes(255 if b>r+40 and b>g+40 else 0 for r,g,b in image.getdata()))
    for rotation in [0,90,180,270]:
        files={}
        for prefix in ['labels','reference']:
            pdf=args.fixtures/f'{prefix}-{rotation}.pdf';out=args.output/f'{prefix}-{rotation}'
            subprocess.run(['pdftoppm','-r','144','-png',str(pdf),str(out)],capture_output=True,check=True)
            text=subprocess.run(['pdftotext','-raw',str(pdf),'-'],capture_output=True,check=True).stdout.decode('utf-8');assert text.count('VECTOR-ART')==2,(pdf,text)
            files[prefix]={'sha256':sha(pdf),'text':text,'pages':sorted(args.output.glob(f'{prefix}-{rotation}-*.png'))}
            assert len(files[prefix]['pages'])==2
        for index,(actual,reference)in enumerate(zip(files['labels']['pages'],files['reference']['pages'])):
            a,b=blue(actual),blue(reference);assert a.size==b.size
            for mask,other in [(a,b),(b,a)]:
                delta=ImageChops.subtract(mask,other.filter(ImageFilter.MaxFilter(3)));assert delta.getbbox()is None,(rotation,index,a.getbbox(),b.getbbox())
            results.append({'rotation':rotation,'page':index+1,'actual_bounds':a.getbbox(),'reference_bounds':b.getbbox(),'actual_sha256':sha(actual),'reference_sha256':sha(reference)})
        for value in files.values():value['pages']=[str(p)for p in value['pages']]
        results.append({'rotation':rotation,'sources':files})
    record={'engine':subprocess.run(['pdftotext','-v'],capture_output=True,text=True,check=True).stderr.splitlines()[0],'comparisons':results}
    (args.output/'results.json').write_text(json.dumps(record,indent=2)+'\n');print('All eight page/rotation comparisons match native CoreGraphics within one pixel; text remains extractable.')


if __name__=='__main__':main()
