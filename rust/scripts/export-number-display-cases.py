#!/usr/bin/env python3
"""Export the actual Swift General formatter as a locale-neutral Windows presentation oracle."""
import hashlib
import json
import math
from pathlib import Path
import random
import subprocess
import tempfile

root=Path(__file__).resolve().parents[2]
source=root/"Sources/XLS42/NumberFormat.swift"
text=source.read_text()
start=text.index("    static func general(_ d: Double) -> String {")
end=text.index("\n    }",start)+len("\n    }")
function=text[start:end].replace("static func general", "func general").replace('errorText("#NUM!")','"#NUM!"').replace('with: decimal','with: "."')
plain_start=text.index("    private static func formatPlain(")
plain_end=text.index("\n    }",plain_start)+len("\n    }")
plain=text[plain_start:plain_end].replace("private static func formatPlain","func formatPlain").replace('errorText("#NUM!")','"#NUM!"')
patterns=[(7.,"0000"),(-7.,"0000"),(0.,"000"),(-.0004,"0.00"),(2.5,"0"),(-2.5,"0"),(12345.6789,"#,##0.000"),(.256,"000.0%"),(-.256,"#,##0.0%"),(9999999.99,"0000"),(1e308,"0.00"),(1e308,"0%"),(1.005,"0.00"),(7.,"#,##0000")]
values=[0.,-0.,1/3,math.pi,2.4999999999999996,-0.125,1e-100,1e100,1.23456789e11,1.23456789e-10]
for threshold in [1e-9,1e11,1.,10.,.1]:
    values.extend([math.nextafter(threshold,0),threshold,math.nextafter(threshold,math.inf)])
rng=random.Random(42)
values.extend(rng.uniform(-9.9,9.9)*10**rng.randint(-300,300) for _ in range(80))
with tempfile.TemporaryDirectory(prefix="off42-general-")as tmp:
    tmp=Path(tmp);inputs=tmp/"input.json";program=tmp/"general.swift"
    inputs.write_text(json.dumps({"general":[repr(value)for value in values],"patterns":[{"input":repr(value),"pattern":code}for value,code in patterns]}))
    program.write_text('import Foundation\nlet decimal=".";let grouping=","\n'+function+'\n'+plain+'''\nlet data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
let inputs = try JSONSerialization.jsonObject(with: data) as! [String: Any]
let results = (inputs["general"] as! [String]).map { ["input": $0, "expected": general(Double($0)!)] }
let patterns = (inputs["patterns"] as! [[String:String]]).map { item -> [String:String] in
    let value=Double(item["input"]!)!
    return ["input":item["input"]!,"pattern":item["pattern"]!,"expected":formatPlain(value,item["pattern"]!,negativeSign:value<0)]
}
let output = try JSONSerialization.data(withJSONObject: ["cases":results,"patterns":patterns], options: [.sortedKeys])
FileHandle.standardOutput.write(output)
''')
    run=subprocess.run(["swift","-module-cache-path",str(tmp/"cache"),str(program),str(inputs)],capture_output=True,text=True,check=True,timeout=60)
    results=json.loads(run.stdout);cases=results["cases"]
output=root/"rust/crates/xls42-core/fixtures/macos-number-display-cases.json"
output.write_text(json.dumps({"source":"Sources/XLS42/NumberFormat.swift:general","source_sha256":hashlib.sha256(source.read_bytes()).hexdigest(),"decimal_separator":".","cases":cases,"patterns":results["patterns"]},ensure_ascii=False,indent=2)+"\n")
print(f"{len(cases)} actual Swift General and {len(patterns)} fixed-pattern cases: {output}")
