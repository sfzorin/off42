#!/usr/bin/env python3
"""Inspect saved array caches and optionally recalculate complete packages with macOS XLS42's CLI."""
import argparse
import csv
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET
import zipfile

S="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
T="urn:oasis:names:tc:opendocument:xmlns:table:1.0"
O="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
def address(row,column):
    result="";column+=1
    while column:column,digit=divmod(column-1,26);result=chr(65+digit)+result
    return result+str(row+1)
def matches(actual,expected):
    if not isinstance(actual,str):return False
    # Preserve literal identifiers with leading zeroes; ordinary numeric output may differ in decimal rounding.
    if re.fullmatch(r"[+-]?0\d+",expected):return actual==expected
    try:
        a,b=float(actual),float(expected)
        return math.isfinite(a)and math.isfinite(b)and math.isclose(a,b,rel_tol=1e-9,abs_tol=1e-12)
    except ValueError:return actual==expected

def saved_values(path):
    with zipfile.ZipFile(path)as archive:
        if path.suffix==".xlsx":
            root=ET.fromstring(archive.read("xl/worksheets/sheet1.xml"));strings=[];result={}
            if"xl/sharedStrings.xml"in archive.namelist():strings=["".join(n.itertext())for n in ET.fromstring(archive.read("xl/sharedStrings.xml"))]
            for cell in root.findall(".//{"+S+"}c"):
                value=cell.findtext("{"+S+"}v","");kind=cell.attrib.get("t")
                if kind=="s":value=strings[int(value)]
                elif kind=="inlineStr":value="".join(cell.find("{"+S+"}is").itertext())
                elif kind=="b":value="TRUE"if value=="1"else"FALSE"
                result[cell.attrib["r"]]=value
            return result
        root=ET.fromstring(archive.read("content.xml"));table=root.find(".//{"+T+"}table");result={};row_number=0
        assert table is not None
        for row in table.iter("{"+T+"}table-row"):
            row_repeat=int(row.attrib.get("{"+T+"}number-rows-repeated",1));column=0
            assert 0<row_repeat<=1000 and row_number+row_repeat<=1000
            for cell in row:
                repeat=int(cell.attrib.get("{"+T+"}number-columns-repeated",1));assert 0<repeat<=100 and column+repeat<=100
                value=cell.attrib.get("{"+O+"}value",cell.attrib.get("{"+O+"}string-value","".join(cell.itertext())))
                if cell.attrib.get("{"+O+"}value-type")=="boolean":value="TRUE"if cell.attrib["{"+O+"}boolean-value"]=="true"else"FALSE"
                for r in range(row_number,row_number+row_repeat):
                    for c in range(column,column+repeat):result[address(r,c)]=value
                column+=repeat
            row_number+=row_repeat
        return result

def main():
    parser=argparse.ArgumentParser();parser.add_argument("--fixtures",type=Path,required=True);parser.add_argument("--output",type=Path,required=True);parser.add_argument("--reference-binary",type=Path)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True);cases=json.loads((args.fixtures/"cases.json").read_text());reports=[]
    def check(mode,extension,values,metadata):
        checked=[{**case,"actual":values.get(case["cell"],""),"pass":matches(values.get(case["cell"],""),case["expected"])}for case in cases]
        report={"mode":mode,"format":extension,**metadata,"passed":sum(c["pass"]for c in checked),"cases":checked};reports.append(report)
        (args.output/"results.json").write_text(json.dumps(reports,ensure_ascii=False,indent=2)+"\n")
        assert all(c["pass"]for c in checked),[c for c in checked if not c["pass"]]
    for extension in ["xlsx","ods"]:
        source=(args.fixtures/("arrays."+extension)).resolve();source_hash=hashlib.sha256(source.read_bytes()).hexdigest()
        check("saved_standard_cache",extension,saved_values(source),{"source_sha256":source_hash})
        if args.reference_binary:
            binary=args.reference_binary.resolve();target=(args.output/("from-"+extension+".csv")).resolve();assert not target.exists(),target
            run=subprocess.run([str(binary),"--convert",str(source),str(target)],capture_output=True,text=True,timeout=60,check=True)
            (args.output/("from-"+extension+".log")).write_text(run.stdout+run.stderr)
            with target.open(encoding="utf-8-sig",newline="")as file:rows=list(csv.reader(file))
            values={address(r,c):value for r,row in enumerate(rows)for c,value in enumerate(row)}
            check("macos_XLS42_CLI_recalculation",extension,values,{"source_sha256":source_hash,"binary":str(binary),"binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"csv_sha256":hashlib.sha256(target.read_bytes()).hexdigest()})
        print(extension+": "+str(len(cases))+" saved values verified")
if __name__=="__main__":main()
