#!/usr/bin/env python3
"""Office Open XML and OpenDocument signatures: sign with DOC42/XLS42, verify three ways, tamper, read Office files.

  python3 scripts/audit-document-signatures.py [--doc42 BIN] [--xls42 BIN] [--no-libreoffice] [--no-download]
                                               [--pkcs11 MODULE --pin PIN]

Checks
  1. DOC42 writes DOCX/ODT, XLS42 writes XLSX/ODS; each is signed with a test-CA .p12 (twice for DOCX/ODT).
  2. Every signed file verifies in Suite42 (--verify-document), in an independent checker (verify_xmldsig.py: libxml2
     C14N + OpenSSL) and, unless --no-libreoffice, in LibreOffice (lo_verify.py, DocumentDigitalSignatures).
  3. Tampering: a changed part, an added part and a changed signature each make Suite42 report INVALID
     (LibreOffice is asked about the changed part as well).
  4. LibreOffice 25.2.4 signatures (committed, scripts/office-signatures/fixtures/): a DOCX and an XLSX from our
     writers, signed by LibreOffice (lo_verify.py --sign). They must verify; a changed part, a retargeted signed
     relationship, changed whitespace in a signed .rels part and a broken signature value must not.
  5. Real third-party signatures (downloaded unless --no-download): Office 2010 DOCX/XLSX/PPTX and Office 2007 DOCX
     from Apache POI's test data, LibreOffice good/bad ODT from LibreOffice's tests — the expected verdicts are fixed.
  6. With --pkcs11: the same signing through a PKCS#11 token (P11Bridge42), e.g. SoftHSM with an imported key.
Exit status 0 only when every check passes.
"""
import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
SIG = os.path.join(HERE, "office-signatures")

FIXTURES = [
    # (file, url, expected Suite42 verdict prefix)
    ("ms-office-2010-signed.docx", "https://raw.githubusercontent.com/apache/poi/trunk/test-data/xmldsign/ms-office-2010-signed.docx", "INTACT"),
    ("ms-office-2010-signed.xlsx", "https://raw.githubusercontent.com/apache/poi/trunk/test-data/xmldsign/ms-office-2010-signed.xlsx", "INTACT"),
    ("ms-office-2010-signed.pptx", "https://raw.githubusercontent.com/apache/poi/trunk/test-data/xmldsign/ms-office-2010-signed.pptx", "INTACT"),
    ("hello-world-signed.docx", "https://raw.githubusercontent.com/apache/poi/trunk/test-data/xmldsign/hello-world-signed.docx", "INTACT"),
    ("hyperlink-example-signed.docx", "https://raw.githubusercontent.com/apache/poi/trunk/test-data/xmldsign/hyperlink-example-signed.docx", "INTACT"),
    ("lo-good.odt", "https://raw.githubusercontent.com/LibreOffice/core/master/xmlsecurity/qa/unit/signing/data/good.odt", "INTACT"),
    ("lo-bad.odt", "https://raw.githubusercontent.com/LibreOffice/core/master/xmlsecurity/qa/unit/signing/data/bad.odt", "INVALID"),
]

results = []


def check(name, ok, detail=""):
    results.append(ok)
    print("%s  %s%s" % ("PASS" if ok else "FAIL", name, (" — " + detail) if detail and not ok else ""))


def run(args, **kw):
    return subprocess.run(args, capture_output=True, text=True, **kw)


def rewrite(src, dst, edit):
    """Copies a ZIP, letting edit(name, data) change or drop (None) entries; edit may add via the returned dict."""
    zi = zipfile.ZipFile(src)
    zo = zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED)
    extra = {}
    for info in zi.infolist():
        data = edit(info.filename, zi.read(info.filename), extra)
        if data is not None:
            zo.writestr(info, data)
    for name, data in extra.items():
        zo.writestr(name, data)
    zo.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--doc42", default=os.path.join(ROOT, ".build/debug/DOC42"))
    ap.add_argument("--xls42", default=os.path.join(ROOT, ".build/debug/XLS42"))
    ap.add_argument("--no-libreoffice", action="store_true")
    ap.add_argument("--no-download", action="store_true")
    ap.add_argument("--pkcs11")
    ap.add_argument("--pin", default="")
    a = ap.parse_args()
    tmp = tempfile.mkdtemp(prefix="suite42-docsig-")
    lo = not a.no_libreoffice and os.path.exists("/Applications/LibreOffice.app")
    try:
        # Test PKI: a root CA and a signer with nonRepudiation (Turkish name to exercise UTF-8 in DNs).
        os.chdir(tmp)
        ossl = "/usr/bin/openssl"
        run([ossl, "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.pem", "-days", "3650",
             "-subj", "/C=TR/O=Suite42 Test/CN=Suite42 Test Root", "-addext", "basicConstraints=critical,CA:true",
             "-addext", "keyUsage=critical,keyCertSign,cRLSign"], check=True)
        run([ossl, "req", "-newkey", "rsa:2048", "-nodes", "-keyout", "user.key", "-out", "user.csr", "-utf8",
             "-subj", "/C=TR/O=Suite42 Test/CN=Ayşe Yılmaz/emailAddress=ayse@example.com"], check=True)
        open("ext.cnf", "w").write("keyUsage=critical,digitalSignature,nonRepudiation\nbasicConstraints=CA:false\n")
        run([ossl, "x509", "-req", "-in", "user.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", "user.pem",
             "-days", "1000", "-extfile", "ext.cnf"], check=True)
        run([ossl, "pkcs12", "-export", "-inkey", "user.key", "-in", "user.pem", "-certfile", "ca.pem", "-out", "user.p12",
             "-passout", "pass:test", "-name", "Ayse"], check=True)
        p12 = ["--p12", os.path.join(tmp, "user.p12"), "--password", "test"]

        # 1. Documents from our own writers.
        run([a.doc42, "--demo", "demo.docx"], check=True)
        run([a.doc42, "--export", "demo.docx", "demo.odt"], check=True)
        open("book.csv", "w").write("Ürün,Adet,Fiyat\nKalem,10,2.5\nDefter,4,12\n")
        run([a.xls42, "--convert", "book.csv", "book.xlsx"], check=True)
        run([a.xls42, "--convert", "book.csv", "book.ods"], check=True)
        signed = []
        for src, tool in [("demo.docx", a.doc42), ("demo.odt", a.doc42), ("book.xlsx", a.xls42), ("book.ods", a.xls42)]:
            out = "signed-" + src
            r = run([tool, "--sign-document", src, out, "--comment", "Onaylıyorum"] + p12)
            check("sign %s" % src, r.returncode == 0, r.stdout + r.stderr)
            signed.append(out)
        for src in ["signed-demo.docx", "signed-demo.odt"]:
            out = "twice-" + src[7:]
            r = run([a.doc42, "--sign-document", src, out] + p12)
            check("second signature on %s" % src, r.returncode == 0, r.stdout)
            signed.append(out)
        if a.pkcs11:
            out = "token-demo.docx"
            r = run([a.doc42, "--sign-document", "demo.docx", out, "--pkcs11", a.pkcs11, "--pin", a.pin])
            check("sign demo.docx with PKCS#11 %s" % os.path.basename(a.pkcs11), r.returncode == 0, r.stdout + r.stderr)
            if r.returncode == 0:
                signed.append(out)

        # 2. Verification: Suite42, independent checker, LibreOffice.
        for f in signed:
            r = run([a.doc42, "--verify-document", f])
            lines = r.stdout.strip().splitlines()
            expected = 2 if f.startswith("twice-") else 1
            check("Suite42 verifies %s (%d signature%s)" % (f, expected, "s" if expected > 1 else ""),
                  r.returncode == 0 and len(lines) == expected and all(l.startswith("INTACT-UNTRUSTED") for l in lines), r.stdout)
        r = run([sys.executable, os.path.join(SIG, "verify_xmldsig.py")] + signed)
        check("independent XMLDSig check (libxml2 C14N + OpenSSL) of %d files" % len(signed), r.returncode == 0, r.stdout)
        if lo:
            r = run([sys.executable, os.path.join(SIG, "lo_verify.py")] + [os.path.abspath(f) for f in signed], timeout=600)
            check("LibreOffice reports every signature valid", r.returncode == 0, r.stdout)
            print("      " + "\n      ".join(l.replace(os.path.realpath(tmp) + "/", "") for l in r.stdout.strip().splitlines()))

        # 3. Tampering.
        def change_part(name, data, extra):
            if name in ("word/document.xml", "content.xml"):
                return data.replace(b"</", b" </", 1)
            return data
        def add_part(name, data, extra):
            # OOXML: a new part only exists for Office when a relationship points to it.
            if name == "word/_rels/document.xml.rels":
                extra["word/media/added.png"] = b"\x89PNG"
                return data.replace(b"</Relationships>", b'<Relationship Id="rIdAdded" Type="http://schemas.openxmlformats.org/'
                                    b'officeDocument/2006/relationships/image" Target="media/added.png"/></Relationships>')
            if name == "content.xml":
                extra["Pictures/added.png"] = b"\x89PNG"
            return data
        def break_signature(name, data, extra):
            if name.startswith("_xmlsignatures/sig") or name == "META-INF/documentsignatures.xml":
                return data.replace(b"<SignatureValue>", b"<SignatureValue>AAAA", 1)
            return data
        tampered = []
        for f in ["signed-demo.docx", "signed-demo.odt"]:
            for label, fn in [("changed part", change_part), ("added part", add_part), ("broken signature value", break_signature)]:
                out = "tampered-%s-%s" % (label.replace(" ", "-"), f)
                rewrite(f, out, fn)
                r = run([a.doc42, "--verify-document", out])
                check("Suite42 rejects %s in %s" % (label, f), r.returncode != 0 and "INVALID" in r.stdout, r.stdout)
                if label == "changed part":
                    tampered.append(out)
        if lo:
            r = run([sys.executable, os.path.join(SIG, "lo_verify.py")] + [os.path.abspath(f) for f in tampered], timeout=600)
            check("LibreOffice also rejects the changed parts", "SignatureIsValid=True" not in r.stdout and r.stdout.count("SignatureIsValid=False") == len(tampered), r.stdout)

        # 4. Signatures made by LibreOffice 25.2.4 (committed fixtures: DOC42/XLS42 files signed by LibreOffice through
        #    DocumentDigitalSignatures.signDocumentWithCertificate with a throw-away test certificate; LibreOffice
        #    reports them valid). Their relationship-transform digests count the whitespace in .rels parts.
        lo_fixtures = [os.path.join(SIG, "fixtures", n) for n in ("libreoffice-25.2-signed.docx", "libreoffice-25.2-signed.xlsx")]
        for f in lo_fixtures:
            r = run([a.doc42, "--verify-document", f])
            check("Suite42 verifies LibreOffice-signed %s" % os.path.basename(f),
                  r.returncode == 0 and r.stdout.startswith("INTACT-UNTRUSTED") and len(r.stdout.strip().splitlines()) == 1, r.stdout)
        r = run([sys.executable, os.path.join(SIG, "verify_xmldsig.py")] + lo_fixtures)
        check("independent XMLDSig check of the LibreOffice-signed fixtures", r.returncode == 0, r.stdout)
        def lo_change_part(name, data, extra):
            return data.replace(b"</", b" </", 1) if name in ("word/document.xml", "xl/worksheets/sheet1.xml") else data
        def lo_retarget(name, data, extra):
            # A signed relationship pointed elsewhere; the whitespace of the part is left as LibreOffice signed it.
            if name in ("word/_rels/document.xml.rels", "xl/_rels/workbook.xml.rels"):
                return data.replace(b'Target="styles.xml"', b'Target="theme/theme1.xml"', 1)
            return data
        def lo_whitespace(name, data, extra):
            # Changed whitespace in a signed relationships part changes LibreOffice's digest (LibreOffice rejects it too).
            return data.replace(b"/><Relationship ", b"/>\n<Relationship ", 1) if name == "_rels/.rels" else data
        lo_tampered = []
        for f in lo_fixtures:
            for label, fn in [("changed part", lo_change_part), ("retargeted relationship", lo_retarget),
                              ("broken signature value", break_signature)]:
                out = "lo-tampered-%s-%s" % (label.replace(" ", "-"), os.path.basename(f))
                rewrite(f, out, fn)
                r = run([a.doc42, "--verify-document", out])
                check("Suite42 rejects %s in LibreOffice-signed %s" % (label, os.path.basename(f)),
                      r.returncode != 0 and "INVALID" in r.stdout, r.stdout)
                lo_tampered.append(out)
        out = "lo-tampered-whitespace-" + os.path.basename(lo_fixtures[0])
        rewrite(lo_fixtures[0], out, lo_whitespace)
        r = run([a.doc42, "--verify-document", out])
        check("Suite42 rejects changed whitespace in LibreOffice-signed _rels/.rels", r.returncode != 0 and "INVALID" in r.stdout, r.stdout)
        if lo:
            r = run([sys.executable, os.path.join(SIG, "lo_verify.py")] + lo_fixtures, timeout=600)
            check("LibreOffice reports its own fixtures valid", r.returncode == 0, r.stdout)

        # 5. Signatures made by Microsoft Office and LibreOffice.
        if not a.no_download:
            os.makedirs("fixtures", exist_ok=True)
            for name, url, expected in FIXTURES:
                path = os.path.join("fixtures", name)
                try:
                    urllib.request.urlretrieve(url, path)
                except Exception as e:  # noqa: BLE001
                    check("download %s" % name, False, str(e))
                    continue
                r = run([a.doc42, "--verify-document", path])
                check("Suite42 reads %s as %s" % (name, expected), r.stdout.startswith(expected), r.stdout.strip())
        return 0 if all(results) else 1
    finally:
        os.chdir(ROOT)
        shutil.rmtree(tmp, ignore_errors=True)
        print("%d/%d checks passed" % (sum(results), len(results)))


if __name__ == "__main__":
    sys.exit(main())
