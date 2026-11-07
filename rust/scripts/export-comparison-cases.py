#!/usr/bin/env python3
"""Run the actual macOS PDFTools word comparison using Foundation and small PDF stubs."""
import hashlib,json,subprocess,tempfile
from pathlib import Path
root=Path(__file__).resolve().parents[2]
source=root/'Sources/PDF42/PDFTools.swift'
text=source.read_text();implementation=text[text.index('    // MARK: - Compare'):]
cases=[
 {'name':'electrical values','old':['BOARD uses 10k resistor','REMOVE THIS'],'new':['BOARD uses 20k resistor','','ADDED PAGE']},
 {'name':'punctuation','old':['Power-stage: R1, R2; 0603.'],'new':['Power stage R1 R2 0603!']},
 {'name':'Cyrillic','old':['Плата готова к проверке'],'new':['Плата готова к сборке']},
 {'name':'Arabic','old':['لوحة جديدة للاختبار'],'new':['لوحة جديدة للإنتاج']},
 {'name':'Greek','old':['Τάση 5V και ρεύμα 2A'],'new':['Τάση 3V και ρεύμα 2A']},
 {'name':'Turkish','old':['Yeni devre kartı hazır'],'new':['Yeni devre kartı tamamlandı']},
 {'name':'repeated words','old':['a b a c'],'new':['a a b c']},
 {'name':'insertions','old':['first last'],'new':['first middle last']},
 {'name':'empty inputs','old':['',''],'new':['']},
 {'name':'Unicode and spacing','old':['café Ω  é 😀\nUSB'],'new':['café Ω é\nUSB']},
]
stub='''import Foundation
final class PDFPage {let string:String?;init(_ s:String){string=s}}
final class PDFDocument {let pages:[PDFPage];init(_ s:[String]){pages=s.map(PDFPage.init)};var pageCount:Int{pages.count};func page(at:Int)->PDFPage?{pages.indices.contains(at) ? pages[at] : nil}}
enum PDFTools {
'''
suffix='''
let data=FileHandle.standardInput.readDataToEndOfFile()
let cases=try JSONSerialization.jsonObject(with:data)as![[String:Any]]
let output=cases.map{row -> [String:Any] in
 let a=PDFDocument(row["old"]as![String]);let b=PDFDocument(row["new"]as![String]);var row=row
 row["old_words"]=a.pages.map{PDFTools.words($0).map(\\.text)};row["new_words"]=b.pages.map{PDFTools.words($0).map(\\.text)}
 row["changes"]=PDFTools.compare(a,b).map{["page":$0.page,"removed":$0.removed.map(\\.text),"added":$0.added]as[String:Any]};return row
}
FileHandle.standardOutput.write(try JSONSerialization.data(withJSONObject:output,options:[.sortedKeys]))
'''
with tempfile.TemporaryDirectory(prefix='off42-compare-oracle-')as tmp:
 path=Path(tmp)/'oracle.swift';path.write_text(stub+implementation+suffix);binary=Path(tmp)/'oracle'
 subprocess.run(['swiftc','-module-cache-path',str(Path(tmp)/'module-cache'),str(path),'-o',str(binary)],check=True)
 result=json.loads(subprocess.check_output([str(binary)],input=json.dumps(cases).encode()))
out=root/'rust/apps/pdf42/fixtures/macos-comparison-cases.json';out.parent.mkdir(exist_ok=True)
out.write_text(json.dumps({'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'cases':result},ensure_ascii=False,indent=2)+'\n')
print(len(result),out)
