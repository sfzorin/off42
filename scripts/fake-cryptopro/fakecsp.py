#!/usr/bin/env python3
"""Stand-in for CryptoPro's cryptcp / certmgr / csptest, for regression tests of PDF42's process plumbing
on machines without CryptoPro CSP. It signs with OpenSSL (RSA), not GOST; it never replaces a real CSP check.

Store: $FAKE_CSP_STORE/<THUMBPRINT>.crt|.key (PEM), $FAKE_CSP_STORE/root.pem (trusted root),
optional <THUMBPRINT>.pin. FAKE_CSP_UNTRUSTED=1 makes chain checks fail."""
import os, subprocess, sys, glob, shutil

OPENSSL = os.environ.get("FAKE_OPENSSL", "/opt/homebrew/opt/openssl@3/bin/openssl")
STORE = os.environ.get("FAKE_CSP_STORE", "")
OK = "[ErrorCode: 0x00000000]"

def out(*lines, code=0):
    print("CryptCP 5.0 (c) \"Crypto-Pro\", 2002-2024.\nUtility for signing and encrypting files.")
    for l in lines:
        print(l)
    sys.exit(code)

def opt(args, name):
    return args[args.index(name) + 1] if name in args and args.index(name) + 1 < len(args) else None

def ossl(*a):
    return subprocess.run([OPENSSL, *a], capture_output=True, text=True)

def thumb(path):
    r = ossl("x509", "-in", path, "-noout", "-fingerprint", "-sha1")
    return r.stdout.strip().split("=")[-1].replace(":", "").upper()

def certs():
    return sorted(glob.glob(os.path.join(STORE, "*.crt")))

def cryptcp(args):
    valued = {"-thumbprint", "-pin", "-cadestsa", "-f", "-dir"}
    pos, i = [], 0
    while i < len(args):
        if args[i] in valued: i += 2; continue
        if not args[i].startswith("-"): pos.append(args[i])
        i += 1
    if "-sign" in args:
        tp = (opt(args, "-thumbprint") or "").upper()
        crt, key = os.path.join(STORE, tp + ".crt"), os.path.join(STORE, tp + ".key")
        if not os.path.exists(crt):
            out("Certificates are not found.", "[ErrorCode: 0x2000012d]", code=1)
        pinfile = os.path.join(STORE, tp + ".pin")
        if os.path.exists(pinfile) and open(pinfile).read().strip() != (opt(args, "-pin") or ""):
            out("Wrong PIN.", "[ErrorCode: 0x8010006b]", code=1)
        if "-detached" not in args or len(pos) != 2:
            out("Unsupported in fake.", "[ErrorCode: 0x80070057]", code=1)
        r = ossl("cms", "-sign", "-binary", "-outform", "DER", "-signer", crt, "-inkey", key,
                 "-certfile", os.path.join(STORE, "root.pem"), "-in", pos[0], "-out", pos[1])
        if r.returncode != 0:
            out(r.stderr, "[ErrorCode: 0x80090008]", code=1)
        out("Signed message is created.", OK)
    if "-verify" in args:
        sig = opt(args, "-f")
        if "-detached" not in args or not sig or len(pos) != 1:
            out("Unsupported in fake.", "[ErrorCode: 0x80070057]", code=1)
        chain = "-nochain" not in args
        a = ["cms", "-verify", "-binary", "-inform", "DER", "-in", sig, "-content", pos[0], "-out", "/dev/null", "-purpose", "any"]
        if chain and os.environ.get("FAKE_CSP_UNTRUSTED") != "1":
            a += ["-CAfile", os.path.join(STORE, "root.pem")]
        elif chain:
            r = ossl(*a, "-noverify")
            if r.returncode == 0:
                out("The certificate or certificate chain is based on an untrusted root.", "[ErrorCode: 0x20000133]", code=1)
            out("Signature verification error.", "[ErrorCode: 0x200001f9]", code=1)
        else:
            a += ["-noverify"]
        r = ossl(*a)
        if r.returncode == 0:
            out("Signature verified.", OK)
        out("Signature verification error.", "[ErrorCode: 0x200001f9]", code=1)
    out("Unknown command.", "[ErrorCode: 0x80070057]", code=1)

def certmgr(args):
    print("Certmgr 1.1 (c) \"Crypto-Pro\",  2007-2024.\nprogram for managing certificates, CRLs and stores\n")
    if "-list" in args:
        n = 0
        for c in certs():
            n += 1
            tp = thumb(c)
            subj = ossl("x509", "-in", c, "-noout", "-subject", "-nameopt", "RFC2253").stdout.strip().split("=", 1)[1]
            iss = ossl("x509", "-in", c, "-noout", "-issuer", "-nameopt", "RFC2253").stdout.strip().split("=", 1)[1]
            end = ossl("x509", "-in", c, "-noout", "-enddate").stdout.strip().split("=", 1)[1]
            has_key = os.path.exists(c[:-4] + ".key")
            print("=" * 77)
            print(f"{n}-------")
            print(f"Issuer              : {iss}")
            print(f"Subject             : {subj}")
            print(f"Serial              : 0x01")
            print(f"SHA1 Hash           : 0x{tp.lower()}")
            print(f"Not valid before    : 01/01/2026  00:00:00 UTC")
            print(f"Not valid after     : {end}")
            print(f"PrivateKey Link     : {'Yes' if has_key else 'No'}")
            if has_key:
                print(f"Container           : HDIMAGE\\\\fake{n}.000\\0001")
        print("=" * 77)
        print(OK)
        return
    if "-export" in args:
        tp, dest = (opt(args, "-thumbprint") or "").upper(), opt(args, "-dest")
        src = os.path.join(STORE, tp + ".crt")
        if not os.path.exists(src):
            print("[ErrorCode: 0x8010002c]"); sys.exit(1)
        ossl("x509", "-in", src, "-outform", "DER", "-out", dest)
        print(OK)
        return
    if "-inst" in args:
        print(OK)
        return
    print("[ErrorCode: 0x80070057]"); sys.exit(1)

def csptest(args):
    if "-enum_cont" in args:
        print("CSP (Type:80) v5.0.10003 KC1 Release Ver:5.0.13000 OS:MacOS CPU:ARM64 FastCode:READY:NEON.")
        for n, _ in enumerate(certs(), 1):
            print(f"\\\\.\\HDIMAGE\\fake{n}|\\\\.\\HDIMAGE\\HDIMAGE\\\\fake{n}.000\\0001")
        print("OK.\n[ErrorCode: 0x00000000]")
        return
    if "-verifycontext" in args:
        print("CSP (Type:80) v5.0.10003 KC1 Release Ver:5.0.13000 OS:MacOS CPU:ARM64 FastCode:READY:NEON.\n[ErrorCode: 0x00000000]")
        return
    print("[ErrorCode: 0x80070057]"); sys.exit(1)

tool = os.path.basename(sys.argv[0])
{"cryptcp": cryptcp, "certmgr": certmgr, "csptest": csptest}[tool](sys.argv[1:])
