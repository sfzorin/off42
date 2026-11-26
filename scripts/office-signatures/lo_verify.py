#!/usr/bin/env python3
"""Asks LibreOffice to verify (or, with --sign NSSDIR, to create) the digital signatures of OOXML / ODF files.

  python3 scripts/office-signatures/lo_verify.py FILE...

Runs inside soffice as a Python macro (LibreOffice's standalone Python is not used: on some Macs it is killed at
launch). The driver creates a throw-away profile, installs this file as a user macro and starts headless soffice
with the macro URL. The macro opens each file as a package storage and calls
com.sun.star.security.DocumentDigitalSignatures.verifyDocumentContentSignatures — the call LibreOffice uses for its
signature bar — and writes one line per signature: SignatureIsValid, CertificateStatus (0 = trusted; a test CA that
is not in LibreOffice's NSS store gives a non-zero status) and the signer.
Exit status 0 when every file has at least one signature and every SignatureIsValid is true.
"""
import os
import shutil
import subprocess
import sys
import tempfile

SOFFICE = os.environ.get("SOFFICE", "/Applications/LibreOffice.app/Contents/MacOS/soffice")


def run(*_args):
    """Macro entry point (runs inside soffice)."""
    import uno
    from com.sun.star.beans import PropertyValue

    def prop(name, value):
        p = PropertyValue()
        p.Name, p.Value = name, value
        return p

    ctx = uno.getComponentContext()
    smgr = ctx.ServiceManager
    out = open(os.environ["LO_SIG_OUT"], "w")
    try:
        cfg = smgr.createInstanceWithContext("com.sun.star.configuration.ConfigurationProvider", ctx)
        node = cfg.createInstanceWithArguments("com.sun.star.configuration.ConfigurationAccess",
                                               (prop("nodepath", "/org.openoffice.Setup/Product"),))
        out.write("LibreOffice %s\n" % node.getByName("ooSetupVersionAboutBox"))
        factory = smgr.createInstanceWithContext("com.sun.star.embed.StorageFactory", ctx)
        if os.environ.get("LO_SIG_SIGN"):
            # Sign each file in place with the first personal certificate of the NSS database in
            # MOZILLA_CERTIFICATE_FOLDER (DocumentDigitalSignatures.signDocumentWithCertificate, no dialog).
            env = smgr.createInstanceWithContext("com.sun.star.xml.crypto.SEInitializer", ctx).createSecurityContext("").getSecurityEnvironment()
            cert = env.getPersonalCertificates()[0]
            for path in os.environ["LO_SIG_FILES"].split("\n"):
                step = "open"
                try:
                    descriptor = uno.Any("[]com.sun.star.beans.PropertyValue", (prop("StorageFormat", "ZipFormat"),))
                    storage = uno.invoke(factory, "createInstanceWithArguments", ((uno.systemPathToFileUrl(path), 7, descriptor),))
                    dds = smgr.createInstanceWithArgumentsAndContext("com.sun.star.security.DocumentDigitalSignatures", ("1.2",), ctx)
                    if path.lower().endswith(("docx", "xlsx", "pptx")):
                        ok = dds.signDocumentWithCertificate(cert, storage, None)
                    else:
                        step = "META-INF"
                        meta = storage.openStorageElement("META-INF", 7)
                        step = "stream"
                        stream = meta.openStreamElement("documentsignatures.xml", 7)
                        step = "sign"
                        ok = dds.signDocumentWithCertificate(cert, storage, stream)
                        step = "commit META-INF"
                        meta.commit()
                    step = "commit"
                    storage.commit()
                    out.write("%s\tSIGNED=%s\tSigner=%s\n" % (path, ok, cert.SubjectName))
                except Exception as e:  # noqa: BLE001
                    out.write("%s\tERROR at %s: %s %r\n" % (path, step, type(e).__name__, e))
            return
        for path in os.environ["LO_SIG_FILES"].split("\n"):
            try:
                url = uno.systemPathToFileUrl(path)
                # sfx2 checks signatures on the plain ZIP storage (SfxMedium::GetZipStorageToSign_Impl) for ODF and OOXML.
                descriptor = uno.Any("[]com.sun.star.beans.PropertyValue", (prop("StorageFormat", "ZipFormat"),))
                storage = uno.invoke(factory, "createInstanceWithArguments", ((url, 1, descriptor),))  # ElementModes.READ
                dds = smgr.createInstanceWithArgumentsAndContext("com.sun.star.security.DocumentDigitalSignatures", ("1.2",), ctx)
                infos = dds.verifyDocumentContentSignatures(storage, None)
                if not infos:
                    out.write("%s\tNONE\n" % path)
                for i in infos:
                    subject = i.Signer.SubjectName if i.Signer else "?"
                    out.write("%s\tSignatureIsValid=%s\tCertificateStatus=%s\tPartial=%s\tSigner=%s\n" %
                              (path, i.SignatureIsValid, i.CertificateStatus, i.PartialDocumentSignature, subject))
            except Exception as e:  # noqa: BLE001
                out.write("%s\tERROR %s %r\n" % (path, type(e).__name__, e))
    finally:
        out.close()
        try:
            smgr.createInstanceWithContext("com.sun.star.frame.Desktop", ctx).terminate()
        except Exception:  # noqa: BLE001
            pass


g_exportedScripts = (run,)


def main(paths):
    profile = tempfile.mkdtemp(prefix="lo-sig-")
    try:
        env = dict(os.environ)
        if paths and paths[0] == "--nss":
            # --nss NSSDIR FILE...: verify with that certificate database (trust the test CA there).
            env["MOZILLA_CERTIFICATE_FOLDER"] = "sql:" + os.path.abspath(paths[1])
            paths = paths[2:]
        if paths and paths[0] == "--sign":
            # --sign NSSDIR FILE...: LibreOffice signs the files in place (test fixtures for the other direction).
            env["LO_SIG_SIGN"] = "1"
            env["MOZILLA_CERTIFICATE_FOLDER"] = "sql:" + os.path.abspath(paths[1])
            paths = paths[2:]
        env["LO_SIG_FILES"] = "\n".join(os.path.abspath(p) for p in paths)
        env["LO_SIG_OUT"] = os.path.join(profile, "result.txt")
        base = [SOFFICE, "--headless", "--invisible", "--norestore", "--nologo", "-env:UserInstallation=file://" + profile]
        subprocess.run(base + ["--terminate_after_init"], env=env, timeout=120, stdout=subprocess.DEVNULL, stderr=None if os.environ.get("SAL_LOG") else subprocess.DEVNULL)
        scripts = os.path.join(profile, "user", "Scripts", "python")
        os.makedirs(scripts, exist_ok=True)
        shutil.copy(os.path.abspath(__file__), os.path.join(scripts, "suite42_sig.py"))
        subprocess.run(base + ["vnd.sun.star.script:suite42_sig.py$run?language=Python&location=user"], env=env, timeout=180,
                       stdout=subprocess.DEVNULL, stderr=None if os.environ.get("SAL_LOG") else subprocess.DEVNULL)
        if not os.path.exists(env["LO_SIG_OUT"]):
            print("LibreOffice did not run the check")
            return 2
        text = open(env["LO_SIG_OUT"]).read()
        print(text, end="")
        lines = text.splitlines()[1:]
        if env.get("LO_SIG_SIGN"):
            return 0 if all("SIGNED=True" in l for l in lines) and lines else 1
        ok = all(any(l.startswith(os.path.abspath(p) + "\t") for l in lines) for p in paths)
        # PartialDocumentSignature is computed from the ODF stream list and is true for every OOXML file (Microsoft's
        # own included), so it only counts for ODF.
        ok = ok and all("SignatureIsValid=True" in l and ("Partial=False" in l or l.split("\t")[0].lower().endswith(("x", "m")))
                        for l in lines)
        return 0 if ok else 1
    finally:
        shutil.rmtree(profile, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
