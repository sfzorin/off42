#!/usr/bin/env python3
"""Regenerate QA-only 100-digit annuity/cash-flow reference values (mpmath 1.3.0)."""
import json
from pathlib import Path
import mpmath as mp
mp.mp.dps=100
cases=[]
def add(name,values,result):
    cases.append({'formula':'='+name+'('+','.join(repr(float(v))for v in values)+')','expected':float(result)})
def pmt(r,n,pv,fv,d):
    return -(pv+fv)/n if not r else -r*(pv*(1+r)**n+fv)/((1+r*d)*((1+r)**n-1))
def fv(r,n,p,pv,d):return -(pv+p*n)if not r else -(pv*(1+r)**n+p*(1+r*d)*((1+r)**n-1)/r)
def interest(r,k,n,pv,f,d):
    payment=pmt(r,n,pv,f,d)
    if k==1:return 0 if d else -pv*r
    return (fv(r,k-2,payment,pv,1)-payment)*r if d else fv(r,k-1,payment,pv,0)*r
for rate,periods,present,future in [(0,10,100,0),(1e-14,10,100,0),(.1/12,36,8000,0),(.08/12,240,0,100000),(-.01,20,1000,100),(.5,100,1000,0)]:
    for due in [0,1]:
        r,n,p,f,d=map(mp.mpf,[rate,periods,present,future,due]);payment=pmt(r,n,p,f,d)
        add('PMT',[rate,periods,present,future,due],payment)
        add('FV',[rate,periods,float(payment),present,due],fv(r,n,mp.mpf(float(payment)),p,d))
        supplied=mp.mpf(-100.0)
        value=-(f+supplied*n) if not r else -(f+supplied*(1+r*d)*((1+r)**n-1)/r)/(1+r)**n
        add('PV',[rate,periods,-100,future,due],value)
        for per in [1,2,periods]:
            v=interest(r,mp.mpf(per),n,p,f,d)
            add('IPMT',[rate,per,periods,present,future,due],v)
            add('PPMT',[rate,per,periods,present,future,due],payment-v)
for payment in [-100,-400]:
    root=mp.findroot(lambda r:1000*(1+r)**12+payment*((1+r)**12-1)/r,.03 if payment==-100 else .4)
    add('RATE',[12,payment,1000],root)
for present in [1000,4000]:
    r=mp.mpf(float(.1/12));v=sum(interest(r,mp.mpf(k),mp.mpf(36),mp.mpf(present),mp.mpf(0),mp.mpf(0))for k in range(1,13))
    add('CUMIPMT',[.1/12,36,present,1,12,0],v)
output=Path(__file__).resolve().parents[1]/'crates/xls42-core/fixtures/independent-finance-cases.json'
output.write_text(json.dumps({'generator':'mpmath 1.3.0, 100 decimal digits, binary-f64 inputs','cases':cases},indent=2)+'\n')
print(len(cases),output)
