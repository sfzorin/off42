#!/usr/bin/env python3
"""Independent Poppler checks: invisible Unicode text must leave rendered pixels intact."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('fixtures',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    args.output.mkdir(parents=True,exist_ok=False)
    result={'engine':subprocess.run(['pdftotext','-v'],capture_output=True,text=True,check=True).stderr.splitlines()[0],'files':{},'comparisons':[]}
    texts={}
    pixels={}
    names=['source','recognized','replaced','undo','scan-source','scan-recognized','scan-compressed']
    for name in names:
        source=args.fixtures/(name+'.pdf')
        text=subprocess.run(['pdftotext','-raw',str(source),'-'],capture_output=True,check=True).stdout.decode('utf-8')
        texts[name]=''.join(text.split())
        subprocess.run(['pdftoppm','-r','144','-cropbox',str(source),str(args.output/name)],capture_output=True,check=True)
        rendered=sorted(args.output.glob(name+'-*.ppm'))
        pixels[name]=[digest(p)for p in rendered]
        assert len(rendered)==(1 if name.startswith('scan-') else 4),(name,rendered)
        result['files'][name]={'sha256':digest(source),'text':text,'pixels':pixels[name]}
    for name in ['recognized','replaced']:
        for word in ['Привет','中文','ไทย','😀']:
            assert texts[name].count(word)==4,(name,word,texts[name])
        for rotation in [0,90,180,270]:assert f'VISIBLE-{rotation}' in texts[name],(name,rotation)
    assert texts['recognized'].count('OCR-original')==4
    assert texts['replaced'].count('OCR-replaced')==4 and 'OCR-original'not in texts['replaced']
    assert 'OCR-'not in texts['source'] and 'OCR-'not in texts['undo']
    for name in ['recognized','replaced','undo']:
        assert pixels[name]==pixels['source'],name
        result['comparisons'].append(f'{name}: all four rotated/cropped pages retain exact source pixels')
    for name in ['scan-recognized','scan-compressed']:
        for word in ['SCAN-original','Привет','中文','ไทย','😀']: assert word in texts[name],(name,word,texts[name])
        assert pixels[name]==pixels['scan-source'],name
        result['comparisons'].append(f'{name}: Unicode text is extractable; scan pixels remain identical')
    (args.output/'results.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({'comparisons':result['comparisons'],'engine':result['engine']},ensure_ascii=False))


if __name__=='__main__':main()
