#!/usr/bin/env python3
"""Independent, high-precision numerical oracle. QA only: requires mpmath==1.3.0."""
import json
from pathlib import Path
import mpmath as mp
mp.mp.dps=160
cases=[]
def add(name,values,expected):
    formula='='+name+'('+','.join('TRUE' if v is True else 'FALSE' if v is False else repr(v) for v in values)+')'
    cases.append({'formula':formula,'expected':float(expected)})
def inv(p,func,hi=10):
    p=mp.mpf(p);lo=mp.mpf(0);hi=mp.mpf(hi)
    while func(hi)<p:hi*=2
    for _ in range(400):
        mid=(lo+hi)/2
        if func(mid)<p:lo=mid
        else:hi=mid
    return (lo+hi)/2
for z in [-20.,-8.,-2.,0.,2.,8.,20.]:
    z0=mp.mpf(z)
    add('NORM.S.DIST',[z,True],mp.erfc(-z0/mp.sqrt(2))/2)
    add('NORM.S.DIST',[z,False],mp.exp(-z0*z0/2)/mp.sqrt(2*mp.pi))
for p in [1e-100,1e-20,1e-12,.01,.25,.5,.99,1.-1e-12]:
    add('NORM.S.INV',[p],mp.sqrt(2)*mp.erfinv(2*mp.mpf(p)-1))
for a in [.5,1.,2.,10.,100.]:
    for x in [.0001,.1,1.,10.,100.]:
        aa=mp.mpf(a);xx=mp.mpf(x);scale=mp.mpf(1.5)
        add('GAMMA.DIST',[x,a,1.5,True],mp.gammainc(aa,0,xx/scale,regularized=True))
for a,b in [(.5,.5),(2.,5.),(10.,10.),(100.,200.)]:
    for x in [.0001,.01,.1,.4,.9]:add('BETA.DIST',[x,a,b,True],mp.betainc(mp.mpf(a),mp.mpf(b),0,mp.mpf(x),regularized=True))
for df in [1.,2.,10.,100.]:
    dd=mp.mpf(df)
    for t in [-20.,-2.,0.,2.,20.]:
        tt=mp.mpf(t);tail=mp.betainc(dd/2,mp.mpf(.5),0,dd/(dd+tt*tt),regularized=True)/2
        add('T.DIST',[t,df,True],1-tail if t>=0 else tail)
        add('T.DIST.RT',[t,df],tail if t>=0 else 1-tail)
for df in [1.,2.,10.,100.]:
    for x in [.0001,.1,5.,50.,500.]:
        add('CHISQ.DIST',[x,df,True],mp.gammainc(mp.mpf(df)/2,0,mp.mpf(x)/2,regularized=True))
        add('CHISQ.DIST.RT',[x,df],mp.gammainc(mp.mpf(df)/2,mp.mpf(x)/2,mp.inf,regularized=True))
for a,b in [(1.,1.),(5.,8.),(20.,30.)]:
    aa=mp.mpf(a);bb=mp.mpf(b)
    for x in [.001,.5,1.,10.,100.]:
        xx=mp.mpf(x);tail=mp.betainc(bb/2,aa/2,0,bb/(bb+aa*xx),regularized=True)
        add('F.DIST',[x,a,b,True],1-tail);add('F.DIST.RT',[x,a,b],tail)
for p in [.1,.5,.9]:
    pp=mp.mpf(p)
    for k in [0,1,5,20]:
        add('BINOM.DIST',[k,20,p,False],mp.binomial(20,k)*pp**k*(1-pp)**(20-k))
        add('BINOM.DIST',[k,20,p,True],mp.fsum(mp.binomial(20,j)*pp**j*(1-pp)**(20-j)for j in range(k+1)))
for mean in [.01,1.,5.,100.]:
    mm=mp.mpf(mean)
    for k in [0,2,10,100]:
        add('POISSON.DIST',[k,mean,False],mp.exp(-mm)*mm**k/mp.factorial(k))
        add('POISSON.DIST',[k,mean,True],mp.exp(-mm)*mp.fsum(mm**j/mp.factorial(j)for j in range(k+1)))
for p in [1e-20,1e-12,.01,.5]:
    for df in [2.,10.,100.]:
        dd=mp.mpf(df);target=mp.mpf(1)-mp.mpf(p)
        add('CHISQ.INV.RT',[p,df],inv(target,lambda x:mp.gammainc(dd/2,0,x/2,regularized=True),df*4))
        add('T.INV.2T',[p,df],inv(mp.mpf(1)-mp.mpf(p)/2,lambda t:1-mp.betainc(dd/2,mp.mpf(.5),0,dd/(dd+t*t),regularized=True)/2))
for a,b in [(5.,8.),(20.,30.)]:
    aa=mp.mpf(a);bb=mp.mpf(b)
    for p in [1e-20,1e-12,.01,.5]:add('F.INV.RT',[p,a,b],inv(mp.mpf(1)-mp.mpf(p),lambda x:mp.betainc(aa/2,bb/2,0,aa*x/(aa*x+bb),regularized=True)))
for x in [0.,.1,2.,8.,20.]:add('ERFC',[x],mp.erfc(mp.mpf(x)))
for lo,hi in [(0.,1.),(8.,9.),(-9.,-8.),(9.,8.)]:add('ERF',[lo,hi],mp.erf(mp.mpf(hi))-mp.erf(mp.mpf(lo)))
for alpha in [1e-20,1e-12,.05]:add('CONFIDENCE.NORM',[alpha,2.,100],-mp.sqrt(2)*mp.erfinv(mp.mpf(alpha)-1)*mp.mpf(2)/10)
root=Path(__file__).resolve().parents[1]
out=root/'crates/xls42-core/fixtures/independent-distribution-cases.json'
out.write_text(json.dumps({'oracle':'mpmath '+mp.__version__,'digits':mp.mp.dps,'cases':cases},separators=(',',':'))+'\n')
print(f'{len(cases)} cases -> {out}')
