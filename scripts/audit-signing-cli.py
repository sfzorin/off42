#!/usr/bin/env python3
"""Offline PDF42 signing CLI regressions using generated keys and localhost CSC/TSA fixtures.

Requires pypdf and OpenSSL 3. No Keychain changes, external TSA/provider, Maven or downloads.
The CSC credentials are fixed dummy values from scripts/pades/csc_mock.py, never real credentials.
Private keys live in a temporary directory removed even when setup or a check fails.
"""
from pathlib import Path
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import argparse, hashlib, importlib.util, json, os, shutil, socket, subprocess, sys, tempfile, threading, time
from pypdf import PdfReader, PdfWriter


def main():
    ROOT = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / '.build/debug/PDF42')
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'build/signing-cli-audit')
    candidates = ['/opt/homebrew/opt/openssl@3/bin/openssl', '/usr/local/opt/openssl@3/bin/openssl']
    detected = next((p for p in candidates if Path(p).is_file()), shutil.which('openssl'))
    parser.add_argument('--openssl', default=detected, help='OpenSSL 3 executable (detected by default)')
    args = parser.parse_args()
    BINARY, OUT = args.binary.resolve(), args.output_dir.resolve()
    if not BINARY.is_file():
        parser.error(f'binary does not exist: {BINARY}')
    if not args.openssl:
        parser.error('OpenSSL 3 is required; provide --openssl')
    OPENSSL = str(Path(args.openssl).resolve())
    OUT.mkdir(parents=True, exist_ok=True)
    ENV = dict(os.environ)
    ENV['PATH'] = str(Path(OPENSSL).parent) + os.pathsep + ENV.get('PATH', '')
    report = {'started_at': datetime.now(timezone.utc).isoformat(), 'binary': str(BINARY),
              'binary_sha256': hashlib.sha256(BINARY.read_bytes()).hexdigest(), 'checks': [], 'commands': [],
              'network_scope': 'Generated certificates have no AIA/CRL/OCSP URLs; CSC and failing TSA bind loopback only.'}
    log = (OUT / 'commands.log').open('w')
    server = tsa_server = mock_log = None
    tsa_requests = []
    spec = importlib.util.spec_from_file_location('strings', ROOT / 'scripts/check_strings.py')
    strings = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(strings)
    def require(ok,msg):
        if not ok: raise AssertionError(msg)

    def command(args, timeout=60):
        started=time.monotonic()
        p=subprocess.run([str(x) for x in args],capture_output=True,text=True,env=ENV,timeout=timeout)
        combined=p.stdout+p.stderr
        report['commands'].append({'argv':[str(x) for x in args],'returncode':p.returncode,'seconds':round(time.monotonic()-started,3)})
        log.write(json.dumps([str(x) for x in args],ensure_ascii=False)+'\n'+combined+'\nexit='+str(p.returncode)+'\n');log.flush()
        return p.returncode,combined

    def run(*args,success=True):
        code,text=command([BINARY,*args])
        require((code==0)==success,f'Unexpected exit {code}: {text[-1800:]}')
        return text

    def openssl(*args):
        code,text=command([OPENSSL,*args]); require(code==0,text)

    def check(name,operation):
        started=time.monotonic()
        try: detail=operation(); status='PASS'
        except Exception as e: detail=str(e);status='FAIL'
        report['checks'].append({'name':name,'status':status,'detail':detail,'seconds':round(time.monotonic()-started,3)})
        print(status,name+':',detail,flush=True)

    def valid_signature(path, level, count=1):
        text=run('--verify',path)
        rows=[l for l in text.splitlines() if ' | whole=' in l]
        require(len(rows)==count and all('invalid' not in l.lower() for l in rows),text)
        require(level in rows[-1] or has_translation(rows[-1], level),text)
        return rows

    def preserved(path,operation):
        path.write_bytes(b'EXISTING OUTPUT MUST SURVIVE')
        text=operation()
        require(path.read_bytes()==b'EXISTING OUTPUT MUST SURVIVE','existing output changed')
        return text

    def metadata(path):
        r=PdfReader(path)
        require(r.metadata.title=='Signing metadata audit',str(r.metadata))
        require(r.attachments['payload.txt'][0]==b'preserve this attachment','attachment lost')

    def has_translation(text, key):
        values = {key}
        for row in strings.table_rows(ROOT / 'Sources/PDF42/Strings.swift'):
            if row[0] == key:
                values.update(row[1:])
        for path in (ROOT / 'Sources/Kit42/Resources/Translations').glob('*.json'):
            value = json.loads(path.read_text()).get(key)
            if value:
                values.add(value)
        return any(value in text for value in values)

    with tempfile.TemporaryDirectory(prefix="suite42-signing-") as temporary:
        PKI = Path(temporary)
        try:
            # Generate private CA -> issuing CA -> signing key. No certificate contains any network lookup URL.
            openssl('req','-new','-x509','-newkey','rsa:2048','-nodes','-keyout',PKI/'root.key','-out',PKI/'root.pem','-days','2','-subj','/CN=Local Smoke Root','-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign,cRLSign')
            for name,issuer,cn,ext in [('inter','root','Local Smoke Issuing CA','basicConstraints=critical,CA:TRUE,pathlen:0\nkeyUsage=critical,keyCertSign,cRLSign\n'),('leaf','inter','Local Smoke Signer','basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\n')]:
                (PKI/(name+'.ext')).write_text(ext+'subjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n')
                openssl('req','-new','-newkey','rsa:2048','-nodes','-keyout',PKI/(name+'.key'),'-out',PKI/(name+'.csr'),'-subj','/CN='+cn)
                openssl('x509','-req','-in',PKI/(name+'.csr'),'-CA',PKI/(issuer+'.pem'),'-CAkey',PKI/(issuer+'.key'),'-CAcreateserial','-out',PKI/(name+'.pem'),'-days','2','-sha256','-extfile',PKI/(name+'.ext'))
            (PKI/'chain.pem').write_bytes((PKI/'inter.pem').read_bytes()+(PKI/'root.pem').read_bytes())
            openssl('pkcs12','-export','-inkey',PKI/'leaf.key','-in',PKI/'leaf.pem','-certfile',PKI/'chain.pem','-out',PKI/'leaf.p12','-passout','pass:local-smoke','-keypbe','PBE-SHA1-3DES','-certpbe','PBE-SHA1-3DES','-macalg','sha1')
            for p in list(PKI.glob('*.key'))+list(PKI.glob('*.p12')):p.chmod(0o600)
            # Use the established PDF fixture, then add metadata and an embedded file independently.
            spec=importlib.util.spec_from_file_location('pdf_fixture',ROOT/'scripts/audit-pdf-cli.py'); fixture=importlib.util.module_from_spec(spec);spec.loader.exec_module(fixture)
            fixture.fixture(OUT/'raw.pdf')
            w=PdfWriter();w.clone_document_from_reader(PdfReader(OUT/'raw.pdf'));w.add_metadata({'/Title':'Signing metadata audit'});w.add_attachment('payload.txt',b'preserve this attachment')
            with (OUT/'source.pdf').open('wb') as f:w.write(f)
            source=OUT/'source.pdf'; p12=PKI/'leaf.p12'; bb=OUT/'bb.pdf'; csc=OUT/'csc.pdf'

            # Reuse the repository CSC fixture with only an isolated ephemeral loopback port substituted.
            with socket.socket() as sock:sock.bind(('127.0.0.1',0)); csc_port=sock.getsockname()[1]
            mock=ROOT/'scripts/pades/csc_mock.py'; mock_text=mock.read_text().replace('PORT = 8890',f'PORT = {csc_port}')
            report['csc_fixture_sha256']=hashlib.sha256(mock.read_bytes()).hexdigest();report['csc_fixture_adaptation']='Only PORT = 8890 replaced with ephemeral loopback port '+str(csc_port)
            mock_log=(OUT/'csc-server.log').open('w')
            server=subprocess.Popen([sys.executable,'-u','-c',mock_text,str(PKI)],stdout=mock_log,stderr=subprocess.STDOUT,env=ENV)
            for _ in range(100):
                try:
                    with socket.create_connection(('127.0.0.1',csc_port),timeout=.1):break
                except OSError:
                    require(server.poll() is None,'CSC fixture exited');time.sleep(.03)
            else: raise RuntimeError('CSC fixture did not start')
            csc_url=f'http://127.0.0.1:{csc_port}/csc/v2'
            tsa_requests=[]
            class FailingTSA(BaseHTTPRequestHandler):
                def log_message(self,*args):pass
                def do_POST(self):
                    body=self.rfile.read(int(self.headers.get('Content-Length',0)))
                    tsa_requests.append({'path':self.path,'content_type':self.headers.get('Content-Type'),'length':len(body),'body_sha256':hashlib.sha256(body).hexdigest()})
                    self.send_response(503);self.send_header('Content-Length','0');self.end_headers()
            tsa_server=ThreadingHTTPServer(('127.0.0.1',0),FailingTSA)
            threading.Thread(target=tsa_server.serve_forever,daemon=True).start()
            tsa_url=f'http://127.0.0.1:{tsa_server.server_port}/timestamp'
            def local_bb():
                run('--sign',source,p12,'local-smoke',bb,'visible','level=B-B')
                require(b'/ETSI.CAdES.detached' in bb.read_bytes(),'missing PAdES subfilter')
                require('whole=true' in valid_signature(bb,'level=PAdES B-B')[-1],'whole-file coverage missing');metadata(bb)
                return 'B-B visible signature valid for content; self-signed chain untrusted as expected; title and attachment retained'
            check('Local PAdES B-B and metadata',local_bb)
            def legacy():
                path=OUT/'legacy.pdf';run('--sign',source,p12,'local-smoke',path,'level=pkcs7')
                require(b'/adbe.pkcs7.detached' in path.read_bytes(),'legacy subfilter missing');valid_signature(path,'Basic PDF signature (not PAdES)');metadata(path)
                return 'legacy CMS stays available with preserved metadata'
            check('Explicit legacy signature',legacy)
            def incremental():
                path=OUT/'twice.pdf';run('--sign',bb,p12,'local-smoke',path,'level=B-B')
                rows=valid_signature(path,'level=PAdES B-B',2);require('whole=false' in rows[0] and 'whole=true' in rows[1],str(rows))
                return 'first and second signatures remain intact, last covers complete file'
            check('Incremental PAdES signing',incremental)
            def tamper():
                data=bytearray((OUT/'twice.pdf').read_bytes());data[7]=ord('6') if data[7]!=ord('6') else ord('7');path=OUT/'tampered.pdf';path.write_bytes(data)
                require('invalid' in run('--verify',path).lower(),'tampering not reported');return 'same-length alteration detected'
            check('PAdES tamper detection',tamper)
            def bad_local(option,expected):
                text=preserved(OUT/'rejected-local.pdf',lambda:run('--sign',source,p12,'local-smoke',OUT/'rejected-local.pdf',option,success=False))
                require(expected in text,text);return 'rejected before replacing existing output'
            check('Reject unknown PAdES level',lambda:bad_local('level=invalid','Unknown PAdES level'))
            check('Reject non-HTTP timestamp URL',lambda:bad_local('tsa=file:///tmp/no-tsa','Invalid HTTP or HTTPS URL'))
            def incomplete_command():
                require('Usage:' in run('--sign',source,success=False),'missing usage diagnostic')
                return 'missing arguments rejected without opening the application'
            check('Reject incomplete signing command',incomplete_command)
            def credentials(*auth):
                text=run('--csc-credentials',csc_url,*auth,'no-browser');require('cred-explicit' in text and 'cred-oauth' in text,text);return 'both local fixture credentials listed'
            check('CSC HTTP Basic credentials',lambda:credentials('user=alice','password=pw'))
            check('CSC client-credentials token',lambda:credentials('client-id=smoke','client-secret=s3cret'))
            def wrong_secret():
                text=run('--csc-credentials',csc_url,'client-id=smoke','client-secret=wrong','no-browser',success=False)
                require('invalid_client' in text,text);return 'local service authentication error propagated'
            check('CSC wrong client secret',wrong_secret)
            def csc_bb():
                run('--csc-sign',source,csc,csc_url,'cred-explicit','user=alice','password=pw','pin=1234','otp=123456','tsa','level=B-B','no-browser')
                valid_signature(csc,'level=PAdES B-B');metadata(csc);require(not tsa_requests,'B-B contacted TSA')
                return 'bare tsa accepted; explicit B-B disables timestamp and signs via localhost with PIN/OTP; metadata retained'
            check('CSC bare-tsa parser and B-B signature',csc_bb)
            def rejected_csc(pin,otp):
                text=preserved(OUT/'rejected-csc.pdf',lambda:run('--csc-sign',source,OUT/'rejected-csc.pdf',csc_url,'cred-explicit','user=alice','password=pw','pin='+pin,'otp='+otp,'level=B-B','no-browser',success=False))
                require('wrong PIN or OTP' in text,text);return 'authorization rejected; previous destination unchanged'
            check('CSC wrong PIN preserves output',lambda:rejected_csc('0000','123456'))
            check('CSC wrong OTP preserves output',lambda:rejected_csc('1234','000000'))
            def invalid_csc():
                for url,flag in [('file:///tmp/csc','port=8765'),(csc_url,'port=0')]:
                    text=run('--csc-credentials',url,flag,'no-browser',success=False);require('Invalid' in text,text)
                return 'non-HTTP service URL and unusable redirect port rejected'
            check('CSC URL and port validation',invalid_csc)
            def unknown_option():
                text=preserved(OUT/'rejected-csc.pdf',lambda:run('--csc-sign',source,OUT/'rejected-csc.pdf',csc_url,'cred-explicit','unknown=value',success=False))
                require('Unknown option' in text,text);return 'unknown option rejected; previous destination unchanged'
            check('CSC unknown option preserves output',unknown_option)
            def unsigned_ltv():
                text=preserved(OUT/'rejected-ltv.pdf',lambda:run('--ltv',source,OUT/'rejected-ltv.pdf',success=False));require(has_translation(text,'the document has no signatures'),text)
                return 'unsigned input rejected without replacing existing output'
            check('LTV rejects unsigned input',unsigned_ltv)
            def ltv():
                path=OUT/'ltv.pdf';text=run('--ltv',bb,path)
                require(b'/DSS' in path.read_bytes(),'DSS absent');valid_signature(path,'PAdES B-B');metadata(path)
                require('warning:' in text,'expected incomplete-revocation warning for offline fixture')
                return 'DSS certificates appended; missing revocation evidence explicitly warned; original signature intact'
            check('Offline LTV extension',ltv)
            def invalid_timestamp():
                text=preserved(OUT/'rejected-ts.pdf',lambda:run('--doc-timestamp',source,OUT/'rejected-ts.pdf','file:///tmp/not-http',success=False))
                require('Invalid HTTP or HTTPS URL' in text,text);return 'invalid TSA URL rejected; previous destination unchanged'
            check('Document timestamp URL validation',invalid_timestamp)
            def failing_timestamp(kind):
                previous=len(tsa_requests);path=OUT/('failed-'+kind+'.pdf')
                if kind=='document':args=['--doc-timestamp',source,path,tsa_url]
                elif kind=='local-sign':args=['--sign',source,p12,'local-smoke',path,'tsa='+tsa_url,'level=B-T']
                else:args=['--csc-sign',source,path,csc_url,'cred-explicit','user=alice','password=pw','pin=1234','otp=123456','tsa='+tsa_url,'level=B-T','no-browser']
                preserved(path,lambda:run(*args,success=False));require(len(tsa_requests)==previous+1,'expected exactly one local TSA request')
                req=tsa_requests[-1];require(req['content_type']=='application/timestamp-query' and req['length']>32,str(req))
                return 'RFC3161 query reaches localhost; HTTP503 fails and preserves previous output'
            check('Document timestamp failure preservation',lambda:failing_timestamp('document'))
            check('Local B-T timestamp failure preservation',lambda:failing_timestamp('local-sign'))
            check('CSC B-T timestamp failure preservation',lambda:failing_timestamp('csc-sign'))
        except Exception as error:
            report['checks'].append({'name': 'Fixture setup', 'status': 'FAIL', 'detail': str(error)})
            print('FAIL Fixture setup:', error, flush=True)
        finally:
            if server is not None:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()
            if mock_log is not None:
                mock_log.close()
            if tsa_server is not None:
                tsa_server.shutdown()
                tsa_server.server_close()
            report['deleted_private_key_files'] = [p.name for p in PKI.iterdir() if p.suffix in {'.key', '.p12'}]
            public = OUT / 'pki'
            public.mkdir(exist_ok=True)
            for path in PKI.iterdir():
                if path.is_file() and path.suffix not in {'.key', '.p12'}:
                    shutil.copy2(path, public / path.name)
    report['private_key_directory_removed'] = not Path(temporary).exists()
    report['local_tsa_requests'] = tsa_requests
    report['finished_at'] = datetime.now(timezone.utc).isoformat()
    report['binary_sha256_after'] = hashlib.sha256(BINARY.read_bytes()).hexdigest()
    if report['binary_sha256_after'] != report['binary_sha256']:
        report['checks'].append({'name': 'Binary identity', 'status': 'FAIL', 'detail': 'binary changed during checks'})
    report['summary'] = {status: sum(c['status'] == status for c in report['checks']) for status in ['PASS', 'FAIL', 'SKIP']}
    report['limitations'] = [
        'CSC uses a repository mock and generated RSA certificates, not a real provider.',
        'The local TSA returns HTTP503; trusted B-T/B-LT/B-LTA issuance and external validation are not covered.',
        'The offline LTV fixture has no revocation endpoints and does not establish trusted long-term validation.',
        'No signing dialogs, physical token, CryptoPro installation or browser OAuth flow are exercised.',
    ]
    (OUT / 'result.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    log.close()
    print(f"{report['summary']['PASS']} passed, {report['summary']['FAIL']} failed, {report['summary']['SKIP']} skipped; {OUT / 'result.json'}", flush=True)
    return 1 if report['summary']['FAIL'] else 0


if __name__ == '__main__':
    sys.exit(main())
