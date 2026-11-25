#!/usr/bin/env python3
"""Best-effort printer inventory; a stalled WMI provider must not block native audits."""
import argparse
import json
from pathlib import Path
import subprocess


def inventory(run=subprocess.run):
    result={}
    for key,command in[
        ('printers','Get-Printer -ErrorAction Stop | Select-Object Name,DriverName,PortName'),
        ('drivers','Get-PrinterDriver -ErrorAction Stop | Select-Object Name,MajorVersion'),
    ]:
        try:
            response=run(['powershell.exe','-NoLogo','-NoProfile','-NonInteractive','-Command',
                "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); $ErrorActionPreference='Stop'; @("+command+') | ConvertTo-Json -Depth 5 -Compress'],
                capture_output=True,text=True,encoding='utf-8',errors='replace',timeout=30,check=True)
            data=json.loads(response.stdout.lstrip('\ufeff').strip()or'[]')
            result[key]=data if isinstance(data,list)else[data]
        except subprocess.TimeoutExpired:
            result[key+'_error']='Printer inventory exceeded thirty seconds'
        except (subprocess.SubprocessError,OSError,ValueError)as error:
            result[key+'_error']=str(error)[:2000]
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(inventory(),indent=2,ensure_ascii=False),encoding='utf-8')
