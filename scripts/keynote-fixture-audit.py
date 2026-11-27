#!/usr/bin/env python3
"""Independent protobuf/IWA fixture for editable Keynote graph import."""
import base64, hashlib, shutil, struct, subprocess, sys, tempfile, zipfile
from pathlib import Path

def v(n):
    b = bytearray()
    while n > 127: b.append((n & 127) | 128); n >>= 7
    return bytes(b + bytes([n]))
def i(n,x): return v(n*8)+v(x)
def b(n,x): return v(n*8+2)+v(len(x))+x
def f(n,x): return v(n*8+5)+struct.pack('<f',x)
def ref(n,x): return b(n,i(1,x))
def point(x,y): return f(1,x)+f(2,y)
def geom(x,y,w,h): return b(1,b(1,point(x,y))+b(2,point(w,h))+i(3,3))
def ellipse(w,h):
    k=.55228475
    def element(t,pts): return b(1,i(1,t)+b''.join(b(2,point(x,y)) for x,y in pts))
    e = element(1,[(w/2,0)])
    for pts in [[(w/2+k*w/2,0),(w,h/2-k*h/2),(w,h/2)],[(w,h/2+k*h/2),(w/2+k*w/2,h),(w/2,h)],[(w/2-k*w/2,h),(0,h/2+k*h/2),(0,h/2)],[(0,h/2-k*h/2),(w/2-k*w/2,0),(w/2,0)]]: e += element(4,pts)
    e += element(5,[])
    return b(5,b(2,point(w,h))+b(3,e))
def record(id,typ,payload):
    header=i(1,id)+b(2,i(1,typ)+i(3,len(payload)))
    return v(len(header))+header+payload
def stream(data):
    # Snappy literal with independently encoded extended length.
    n=len(data)-1
    tag=bytes([n<<2]) if n<60 else bytes([60<<2,n&255]) if n<256 else bytes([61<<2,n&255,n>>8])
    raw=v(len(data))+tag+data
    return b'\0'+len(raw).to_bytes(3,'little')+raw

def fixture(path, unsupported=False):
    objs=[(1,1,ref(2,10)),(2,11006,b(4,i(1,99)+b(3,b'pixel.png'))),(10,2,b(3,ref(2,11)+ref(2,12))+b(4,point(800,600))),
          (11,4,ref(2,21)),(12,4,ref(2,22)),
          (21,5,ref(7,31)+ref(7,32)+ref(42,31)+ref(42,32)),(22,5,ref(7,33)+ref(7,34)+ref(42,33)+ref(42,34)),
          (31,2011,b(1,b(1,geom(10,20,200,50)))+ref(4,41)+i(6,1)),(41,2001,b(3,'Привет 🌍 Türkçe'.encode())+b(5,b(1,i(1,0)+ref(2,42)))),
          (42,2022,b(11,i(1,1)+f(3,32)+b(5,b'Helvetica')+b(7,i(1,1)+f(3,1)+f(4,0)+f(5,0)))+b(12,i(1,2))),
          (32,2011,b(1,b(1,geom(30,90,100,80))+ref(2,43))),
          (43,2025,b(1,b(11,b(1,b(1,i(1,1)+f(3,0)+f(4,0)+f(5,1)))))),(33,2011,b(1,b(1,geom(140,100,120,100))+b(3,ellipse(120,100)))),
          (34,9999 if unsupported else 3005,b(1,geom(300,110,40,30))+ref(11,99))]
    png=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=')
    with zipfile.ZipFile(path,'w') as z:
        z.writestr('Index/Document.iwa',stream(b''.join(record(*o) for o in objs)))
        z.writestr('Data/pixel.png',png)

def run(binary):
    with tempfile.TemporaryDirectory() as tmp:
        d=Path(tmp); key=d/'fixture.key'; fixture(key); before=hashlib.sha256(key.read_bytes()).digest()
        def call(*args,code=0):
            p=subprocess.run([str(binary),*map(str,args)],capture_output=True,text=True)
            assert p.returncode == code, p.stdout+'\n'+p.stderr
            return p.stdout,p.stderr
        out,err=call('--info',key)
        assert '2 slides, 800×600' in out and 'Привет 🌍 Türkçe' in out and 'shape rect [30,90 100×80]' in out and 'shape ellipse [140,100 120×100]' in out and 'picture [300,110 40×30]' in out and 'warning' in err.lower(),out+err
        ppt=d/'result.pptx';call('--convert',key,ppt);out,_=call('--info',ppt)
        with zipfile.ZipFile(ppt) as z:
            slide=z.read('ppt/slides/slide1.xml').decode()
            assert 'sz="3200"' in slide and 'FF0000' in slide and '0000FF' in slide and 'algn="ctr"' in slide,slide

        assert all(x in out for x in ['2 slides, 800×600','Привет 🌍 Türkçe','shape rect [30,90 100×80]','shape ellipse [140,100 120×100]','picture [300,110 40×30]']),out
        call('--convert',key,key,code=1);assert hashlib.sha256(key.read_bytes()).digest()==before
        package=d/'package.key';package.mkdir()
        with zipfile.ZipFile(key) as z:z.extractall(package)
        package_before={str(p.relative_to(package)):hashlib.sha256(p.read_bytes()).digest() for p in package.rglob('*') if p.is_file()}
        call('--info',package)
        assert package_before=={str(p.relative_to(package)):hashlib.sha256(p.read_bytes()).digest() for p in package.rglob('*') if p.is_file()}
        (package/'Data'/'escape').symlink_to('/etc/passwd')
        call('--info',package,code=1)
        huge=d/'oversized.key'
        with huge.open('wb') as handle: handle.truncate(256 * 1024 * 1024 + 1)
        call('--info',huge,code=1)
        bad=d/'unsupported.key';fixture(bad,True);_,err=call('--info',bad,code=1);assert 'drawable type 9999' in err,err
    print('Keynote independent fixture: two slides, Unicode, rectangle, ellipse, image, geometry, PPTX reopen and source preservation passed')
if __name__=='__main__':run(Path(sys.argv[1]).resolve())
