#!/usr/bin/env python3
"""Turkish e-imza / PKCS#11 smart-card signing, against a SoftHSM token that imitates an AKİS card.

  python3 scripts/audit-eimza.py [--pdf42 BIN] [--doc42 BIN] [--softhsm MODULE] [--tsa URL]

No real card is used. The script creates a test root "Kamu Sertifikasyon Merkezi Test" and a qualified-style signer
certificate (Law 5070 policy 2.16.792.1.61.0.1.5070.1.1, ETSI QcCompliance, TC Kimlik No in serialNumber,
nonRepudiation) plus a plain authentication certificate on the same token, as on e-imza cards. It then checks:
  - P11Bridge42 lists the token and both certificates (--pkcs11-list), the signing one marked qualified;
  - PDF42 signs PAdES B-B (and B-T when --tsa is given) with the card key; the signature verifies cryptographically
    and is reported as a qualified Law 5070 certificate from Kamu SM with a masked TC Kimlik No;
  - DOC42 signs a DOCX with the same key (XAdES) and it verifies;
  - a wrong PIN is reported as such and produces no file;
  - if /usr/local/lib/libakisp11.dylib exists, its architecture is reported and the bridge is started accordingly
    (only C_GetSlotList/C_GetTokenInfo, never a login).
Exit status 0 only when every check passes.
"""
import argparse
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print("%s  %s%s" % ("PASS" if ok else "FAIL", name, (" — " + detail.strip()) if detail and not ok else ""))


def run(args, env=None):
    return subprocess.run(args, capture_output=True, text=True, env=env)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pdf42", default=os.path.join(ROOT, ".build/debug/PDF42"))
    ap.add_argument("--doc42", default=os.path.join(ROOT, ".build/debug/DOC42"))
    ap.add_argument("--softhsm", default="/opt/homebrew/lib/softhsm/libsofthsm2.so")
    ap.add_argument("--tsa")
    a = ap.parse_args()
    if not os.path.exists(a.softhsm) or not shutil.which("softhsm2-util") or not shutil.which("pkcs11-tool"):
        print("SKIP: needs SoftHSM 2 (brew install softhsm) and OpenSC pkcs11-tool")
        return 2
    tmp = tempfile.mkdtemp(prefix="suite42-eimza-")
    os.chdir(tmp)
    env = dict(os.environ, SOFTHSM2_CONF=os.path.join(tmp, "softhsm2.conf"))
    try:
        os.makedirs("tokens")
        open("softhsm2.conf", "w").write("directories.tokendir = %s/tokens\nobjectstore.backend = file\nlog.level = ERROR\n" % tmp)
        ossl = "/usr/bin/openssl"
        run([ossl, "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.pem", "-days", "3650", "-utf8",
             "-subj", "/C=TR/O=Kamu Sertifikasyon Merkezi Test/CN=Test Nitelikli Elektronik Sertifika Hizmet Sağlayıcısı",
             "-addext", "basicConstraints=critical,CA:true", "-addext", "keyUsage=critical,keyCertSign,cRLSign"])
        # QcStatements: SEQUENCE { SEQUENCE { id-etsi-qcs-QcCompliance } }
        open("nes.cnf", "w").write(
            "keyUsage=critical,nonRepudiation\nbasicConstraints=CA:false\n"
            "certificatePolicies=2.16.792.1.61.0.1.5070.1.1\n"
            "1.3.6.1.5.5.7.1.3=DER:300a3008060604008e460101\n")
        open("auth.cnf", "w").write("keyUsage=critical,digitalSignature,keyEncipherment\nbasicConstraints=CA:false\n")
        for name, subj in [("nes", "/C=TR/serialNumber=12345678901/CN=AYŞE YILMAZ"), ("auth", "/C=TR/serialNumber=12345678901/CN=AYŞE YILMAZ")]:
            run([ossl, "req", "-newkey", "rsa:2048", "-nodes", "-keyout", name + ".key", "-out", name + ".csr", "-utf8", "-subj", subj])
            run([ossl, "x509", "-req", "-in", name + ".csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial",
                 "-out", name + ".pem", "-days", "700", "-extfile", name + ".cnf"])
            run([ossl, "pkcs8", "-topk8", "-nocrypt", "-in", name + ".key", "-out", name + ".p8"])
            run([ossl, "x509", "-in", name + ".pem", "-outform", "DER", "-out", name + ".der"])
        r = run(["softhsm2-util", "--init-token", "--free", "--label", "AKIS TEST", "--pin", "123456", "--so-pin", "654321"], env)
        check("SoftHSM token created", r.returncode == 0, r.stderr)
        for name, kid in [("auth", "01"), ("nes", "02")]:
            run(["softhsm2-util", "--import", name + ".p8", "--token", "AKIS TEST", "--label", name, "--id", kid, "--pin", "123456"], env)
            run(["pkcs11-tool", "--module", a.softhsm, "--login", "--pin", "123456", "--token-label", "AKIS TEST",
                 "--write-object", name + ".der", "--type", "cert", "--id", kid, "--label", name], env)

        r = run([a.pdf42, "--pkcs11-list", a.softhsm], env)
        check("bridge lists both certificates", r.stdout.count("cert slot=") == 2, r.stdout + r.stderr)
        check("the non-repudiation certificate is marked qualified", "nonRepudiation=true qualified" in r.stdout, r.stdout)

        run([a.doc42, "--demo", "demo.docx"])
        run([a.doc42, "--export", "demo.docx", "demo.pdf"])
        r = run([a.pdf42, "--pkcs11-sign", "demo.pdf", "signed.pdf", a.softhsm, "pin=123456", "level=B-B", "reason=Onaylıyorum"], env)
        check("PDF42 signs PAdES B-B with the card key", r.returncode == 0 and os.path.exists("signed.pdf"), r.stdout)
        r = run([a.pdf42, "--verify-tr", "signed.pdf"], env)
        check("signature intact (untrusted test root)", r.stdout.startswith("untrusted") and "PAdES B-B" in r.stdout, r.stdout)
        check("reported as qualified Law 5070, Kamu SM, masked TC Kimlik No",
              "Qualified electronic certificate (Law 5070)" in r.stdout and "Kamu SM" in r.stdout and "123******01" in r.stdout, r.stdout)
        if a.tsa:
            r = run([a.pdf42, "--pkcs11-sign", "demo.pdf", "signed-t.pdf", a.softhsm, "pin=123456", "level=B-T", "tsa=" + a.tsa], env)
            check("PDF42 signs PAdES B-T with the card key", r.returncode == 0, r.stdout)
            r = run([a.pdf42, "--verify-tr", "signed-t.pdf"], env)
            check("B-T signature intact", "PAdES B-T" in r.stdout and not r.stdout.startswith("invalid"), r.stdout)
        r = run([a.pdf42, "--pkcs11-sign", "demo.pdf", "wrong.pdf", a.softhsm, "pin=000000"], env)
        check("wrong PIN is reported and nothing is written", r.returncode != 0 and "Wrong PIN" in r.stdout and not os.path.exists("wrong.pdf"), r.stdout)
        r = run([a.doc42, "--sign-document", "demo.docx", "signed.docx", "--pkcs11", a.softhsm, "--pin", "123456"], env)
        check("DOC42 signs DOCX with the card key", r.returncode == 0, r.stdout)
        r = run([a.doc42, "--verify-document", "signed.docx"], env)
        check("card-signed DOCX verifies", r.stdout.startswith("INTACT"), r.stdout)

        akis = "/usr/local/lib/libakisp11.dylib"
        if os.path.exists(akis):
            r = run([a.pdf42, "--pkcs11-list", akis])
            arch = r.stdout.splitlines()[0] if r.stdout else ""
            ok = "x86_64" in arch and ("Rosetta" in r.stdout or "slot" in r.stdout or "no card" in r.stdout.lower()) or "arm64" in arch
            check("AKİS module detected and handled (%s)" % arch.split("[")[-1].split("]")[0], ok, r.stdout)
            print("      " + "\n      ".join(r.stdout.strip().splitlines()))
        return 0 if all(results) else 1
    finally:
        os.chdir(ROOT)
        shutil.rmtree(tmp, ignore_errors=True)
        print("%d/%d checks passed" % (sum(results), len(results)))


if __name__ == "__main__":
    sys.exit(main())
