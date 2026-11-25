#!/usr/bin/env python3
"""GOST signing / verification / power-of-attorney (МЧД) regression checks for PDF42's command line.

Default mode runs against scripts/fake-cryptopro (OpenSSL RSA stand-ins for cryptcp/certmgr/csptest). It checks
PDF42's process plumbing, PDF embedding, verdict mapping and МЧД matching, NOT GOST cryptography or CryptoPro
compatibility.

    python3 scripts/audit-gost-cli.py [--binary .build/debug/PDF42]
    python3 scripts/audit-gost-cli.py --real --thumbprint SHA1 [--pin PIN] [--snils 12345678901]

--real uses the installed CryptoPro CSP (/opt/cprocsp) and a certificate with a key in uMy (e.g. from the CryptoPro
test CA); МЧД checks then use the given SNILS (the certificate's) for the representative.
"""
import argparse, os, pathlib, shutil, subprocess, sys, tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
OPENSSL = os.environ.get("FAKE_OPENSSL", "/opt/homebrew/opt/openssl@3/bin/openssl")

ap = argparse.ArgumentParser()
ap.add_argument("--binary", default=str(ROOT / ".build/debug/PDF42"))
ap.add_argument("--real", action="store_true")
ap.add_argument("--thumbprint")
ap.add_argument("--pin")
ap.add_argument("--snils", default="12345678901")
args = ap.parse_args()

work = pathlib.Path(tempfile.mkdtemp(prefix="gost-audit-"))
env = dict(os.environ)
results = []

def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS " if ok else "FAIL ") + name + ("" if ok else f"\n      {detail}"))

def run(*a, extra_env=None):
    e = dict(env)
    if extra_env: e.update(extra_env)
    r = subprocess.run([args.binary, *map(str, a)], capture_output=True, text=True, env=e, timeout=300)
    return r.returncode, r.stdout + r.stderr

def ossl(*a):
    subprocess.run([OPENSSL, *map(str, a)], check=True, capture_output=True)

def minimal_pdf(path):
    objs = ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
            None, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"]
    stream = b"BT /F1 24 Tf 72 760 Td (GOST signature test) Tj ET"
    out = bytearray(b"%PDF-1.7\n")
    offsets = []
    for i, o in enumerate(objs, 1):
        offsets.append(len(out))
        body = (f"<< /Length {len(stream)} >>\nstream\n".encode() + stream + b"\nendstream") if o is None else o.encode()
        out += f"{i} 0 obj\n".encode() + body + b"\nendobj\n"
    x = len(out)
    out += f"xref\n0 {len(objs)+1}\n0000000000 65535 f \n".encode()
    for off in offsets: out += f"{off:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objs)+1} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n".encode()
    path.write_bytes(bytes(out))

def poa_emchd(snils, start="2026-01-01", end="2027-12-31"):
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<Доверенность ВерсФорм="EMCHD_1" ПрЭлФорм="00000000">
 <Документ ВидДовер="1" ПрПередов="1">
  <СвДов НомДовер="7f0e1c2a-0000-4000-8000-000000000042" ДатаВыдДовер="{start}" СрокДейст="{end}" ВнНомДовер="42"/>
  <Доверит ТипДоверит="1">
   <РосОргДовер>
    <СвРосОрг НаимОрг="ООО &quot;Ромашка&quot;" ИННЮЛ="7701234567" КПП="770101001" ОГРН="1027700000000"/>
    <ЛицоБезДов><СвФЛ СНИЛС="999-999-999 99"><ФИО Фамилия="Директоров" Имя="Пётр"/></СвФЛ></ЛицоБезДов>
   </РосОргДовер>
  </Доверит>
  <Пред ТипПред="1">
   <СведФизЛ СНИЛС="{snils[:3]}-{snils[3:6]}-{snils[6:9]} {snils[9:]}" ИННФЛ="771234567890">
    <ФИО Фамилия="Иванов" Имя="Иван" Отчество="Иванович"/>
   </СведФизЛ>
  </Пред>
  <Полном><МашПолн КодПолн="NALOG_DECL" НаимПолн="Подписание деклараций"/></Полном>
 </Документ>
</Доверенность>
""".encode("utf-8")

def poa_002_cp1251(snils):
    return f"""<?xml version="1.0" encoding="windows-1251"?>
<Файл ИдФайл="ON_DOVEL_7701_20260101_1" ВерсФорм="002">
 <Документ>
  <СвДов НомДовер="b1c2d3e4-0000-4000-8000-000000000002" ДатаВыдДовер="2026-01-01" ДатаКонДовер="2027-01-01"/>
  <СвДоверит><СвРосОрг НаимОрг="АО Тест" ИННЮЛ="7709999999" ОГРН="1027799999999"/></СвДоверит>
  <СвУпПред><СвПред><СведФизЛ СНИЛС="{snils}" ИННФЛ="771234567890"><ФИО Фамилия="Иванов" Имя="Иван"/></СведФизЛ></СвПред></СвУпПред>
  <СвПолн><ТекстПолн>Подписывать акты сверки</ТекстПолн></СвПолн>
 </Документ>
</Файл>
""".encode("cp1251")

# --- environment -----------------------------------------------------------------------------------------------
if args.real:
    if not args.thumbprint:
        sys.exit("--real needs --thumbprint")
    tp, principal_tp, snils = args.thumbprint, args.thumbprint, args.snils
    env.pop("SUITE42_CPROCSP_BIN", None)
else:
    store = work / "store"; store.mkdir()
    env["SUITE42_CPROCSP_BIN"] = str(ROOT / "scripts/fake-cryptopro")
    env["FAKE_CSP_STORE"] = str(store)
    ossl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", work / "ca.key", "-out", store / "root.pem",
         "-subj", "/CN=Fake Test CA/O=Suite42", "-days", "30", "-addext", "basicConstraints=critical,CA:TRUE",
         "-addext", "keyUsage=critical,keyCertSign")
    def issue(name, subj):
        ossl("req", "-new", "-newkey", "rsa:2048", "-nodes", "-utf8", "-keyout", work / f"{name}.key", "-out", work / f"{name}.csr", "-subj", subj)
        ext = work / "ext.cnf"; ext.write_text("keyUsage=critical,digitalSignature,nonRepudiation\n")
        ossl("x509", "-req", "-in", work / f"{name}.csr", "-CA", store / "root.pem", "-CAkey", work / "ca.key", "-CAcreateserial",
             "-days", "30", "-out", work / f"{name}.crt", "-extfile", ext)
        t = subprocess.run([OPENSSL, "x509", "-in", work / f"{name}.crt", "-noout", "-fingerprint", "-sha1"],
                           capture_output=True, text=True).stdout.strip().split("=")[-1].replace(":", "").upper()
        shutil.copy(work / f"{name}.crt", store / f"{t}.crt")
        shutil.copy(work / f"{name}.key", store / f"{t}.key")
        return t
    snils = "12345678901"
    tp = issue("rep", "/CN=Иванов Иван Иванович/SN=Иванов/GN=Иван Иванович/1.2.643.100.3=12345678901/1.2.643.3.131.1.1=771234567890")
    principal_tp = issue("dir", "/CN=Директоров Пётр/O=ООО Ромашка/1.2.643.100.4=7701234567/1.2.643.100.3=99999999999")
    (store / f"{tp}.pin").write_text("1234")
    args.pin = "1234"

pin = ["--pin", args.pin] if args.pin else []
pdf = work / "doc.pdf"; minimal_pdf(pdf)

# --- checks ----------------------------------------------------------------------------------------------------
code, out = run("--gost-status")
check("status: CSP detected", code == 0 and "installed" in out, out)

code, out = run("--gost-certs")
check("certificates: signing certificate listed with SNILS", code == 0 and tp.upper() in out.upper() and ("SNILS" in out or args.real), out)
check("certificates: key containers listed", "containers:" in out and "\\\\.\\" in out, out)

data = work / "contract.txt"; data.write_text("Договор поставки № 42\n")
code, out = run("--gost-sign-file", data, "--thumbprint", tp, *pin)
check("detached: sign any file to .sig", code == 0 and (work / "contract.txt.sig").exists(), out)
code, out = run("--gost-verify", data, "--sig", work / "contract.txt.sig")
check("detached: verify valid", code == 0 and "valid" in out, out)
if not args.real:
    check("detached: signer and SNILS from the certificate", "Иванов" in out and "12345678901" in out, out)
    code, out = run("--gost-sign-file", data, "--out", work / "wrongpin.sig", "--thumbprint", tp, "--pin", "0000")
    check("detached: wrong PIN reported", code == 1 and "Wrong PIN" in out, out)

tampered = work / "contract-tampered.txt"; tampered.write_text("Договор поставки № 43\n")
code, out = run("--gost-verify", tampered, "--sig", work / "contract.txt.sig")
check("detached: changed file is invalid", code == 1 and "invalid" in out, out)

if not args.real:
    code, out = run("--gost-verify", data, "--sig", work / "contract.txt.sig", extra_env={"FAKE_CSP_UNTRUSTED": "1"})
    check("detached: untrusted chain gives 'untrusted'", code == 2 and "untrusted" in out, out)

# МЧД: the representative signs; the principal signs the МЧД itself.
poa = work / "poa.xml"; poa.write_bytes(poa_emchd(snils))
code, out = run("--gost-sign-file", poa, "--thumbprint", principal_tp, *([] if not args.real else pin))
check("МЧД: principal signs the power of attorney", code == 0 and (work / "poa.xml.sig").exists(), out)
code, out = run("--poa-check", poa, "--sig", work / "contract.txt.sig")
check("МЧД: unified format parsed and matched", code == 0 and "EMCHD_1" in out and "Ромашка" in out and "The signer is the representative" in out, out)
check("МЧД: its own signature verified", "power of attorney's signature is valid" in out, out)

poa2 = work / "poa-002.xml"; poa2.write_bytes(poa_002_cp1251(snils))
code, out = run("--poa-check", poa2, "--sig", work / "contract.txt.sig")
check("МЧД: format 002 in windows-1251 parsed", code == 0 and "АО Тест" in out and "акты сверки" in out and "No signature of the power of attorney" in out, out)

bad = work / "poa-other.xml"; bad.write_bytes(poa_emchd("11111111111"))
code, out = run("--poa-check", bad, "--sig", work / "contract.txt.sig")
check("МЧД: another representative is an error", code == 4 and "differs from the representative" in out, out)

expired = work / "poa-expired.xml"; expired.write_bytes(poa_emchd(snils, "2020-01-01", "2021-01-01"))
code, out = run("--poa-check", expired, "--sig", work / "contract.txt.sig")
check("МЧД: expired power of attorney is an error", code == 4 and "expired" in out, out)

# PDF: signature dictionary with the CSP signature, МЧД embedded and covered by it.
signed = work / "signed.pdf"
code, out = run("--gost-sign", pdf, signed, "--thumbprint", tp, *pin, "--poa", poa, "--reason", "Согласовано", "--visible")
check("PDF: sign with МЧД attached", code == 0 and signed.exists(), out)
raw = signed.read_bytes()
check("PDF: CryptoPro PDF handler and detached CMS", b"/Filter /CryptoPro#20PDF" in raw and b"/SubFilter /adbe.pkcs7.detached" in raw, "")
code, out = run("--attachments", signed)
check("PDF: МЧД and its signature embedded", "poa.xml" in out and "poa.xml.sig" in out, out)
code, out = run("--gost-verify", signed)
check("PDF: signature valid and covers the file", code == 0 and "#1 valid" in out and "whole=true" in out, out)
check("PDF: МЧД checked against the signer", "The signer is the representative" in out and "Valid on the signing date" in out, out)
check("PDF: reason kept", "Согласовано" in out, out)

second = work / "signed-twice.pdf"
code, out = run("--gost-sign", signed, second, "--thumbprint", principal_tp, *([] if not args.real else pin))
code, out = run("--gost-verify", second)
check("PDF: second signature keeps the first valid", code == 0 and "#1 valid" in out and "whole=false" in out and "#2 valid" in out, out)

broken = bytearray(raw)
i = broken.find(b"GOST signature test")
broken[i] = ord("g")
(work / "broken.pdf").write_bytes(bytes(broken))
code, out = run("--gost-verify", work / "broken.pdf")
check("PDF: changed content is invalid", code == 1 and "invalid" in out, out)

code, out = run("--gost-verify", signed, extra_env={"SUITE42_CPROCSP_BIN": str(work / "missing")})
check("PDF: without CSP the signature is reported as not checked", code == 3 and "notChecked" in out, out)

passed = sum(results)
print(f"\n{passed}/{len(results)} checks passed ({'real CryptoPro CSP' if args.real else 'fake CSP: plumbing only, no GOST crypto'})")
print(f"work dir: {work}")
sys.exit(0 if passed == len(results) else 1)
