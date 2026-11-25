"""Mock CSC API v2 service for testing PDF42 remote signing (test PKI keys only)."""
import base64, hashlib, json, os, secrets, subprocess, sys, urllib.parse
from http.server import BaseHTTPRequestHandler, HTTPServer

PKI = sys.argv[1]
PORT = 8890
BASE = "/csc/v2/"
LOG = open(os.path.join(PKI, "csc-requests.log"), "a")

def der(path):
    return subprocess.run(["openssl", "x509", "-in", path, "-outform", "der"], capture_output=True, check=True).stdout

CHAIN = [base64.b64encode(der(os.path.join(PKI, p))).decode() for p in ("leaf.pem", "inter.pem", "root.pem")]
codes, tokens, sads = {}, {}, {}

def cred(cid, mode):
    info = {"credentialID": cid, "description": f"mock {mode}", "key": {"status": "enabled", "algo": ["1.2.840.113549.1.1.1", "1.2.840.113549.1.1.11"], "len": 2048},
            "cert": {"status": "valid", "certificates": CHAIN}, "SCAL": "SCAL2", "multisign": 1, "auth": {"mode": mode}}
    if mode == "explicit":
        info["auth"]["objects"] = [{"type": "Password", "id": "PIN", "format": "N"}, {"type": "OTP", "id": "OTP", "generator": "totp"}]
    return info

CREDS = {"cred-explicit": cred("cred-explicit", "explicit"), "cred-oauth": cred("cred-oauth", "oauth2code")}

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass

    def reply(self, code, obj, headers=None):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        for k, v in (headers or {}).items(): self.send_header(k, v)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def bearer(self):
        a = self.headers.get("Authorization", "")
        return tokens.get(a[7:]) if a.startswith("Bearer ") else None

    def do_GET(self):
        u = urllib.parse.urlparse(self.path)
        if u.path == BASE + "oauth2/authorize":
            q = dict(urllib.parse.parse_qsl(u.query))
            code = secrets.token_hex(8)
            codes[code] = q
            LOG.write(f"authorize {json.dumps(q)}\n"); LOG.flush()
            loc = q["redirect_uri"] + "?" + urllib.parse.urlencode({"code": code, "state": q["state"]})
            self.send_response(302); self.send_header("Location", loc); self.send_header("Content-Length", "0"); self.end_headers()
            return
        self.reply(404, {"error": "not_found"})

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(n)
        path = urllib.parse.urlparse(self.path).path[len(BASE):]
        LOG.write(f"POST {path} {len(raw)} bytes {raw[:400]!r}\n"); LOG.flush()
        if path == "oauth2/token":
            f = dict(urllib.parse.parse_qsl(raw.decode()))
            if f.get("grant_type") == "client_credentials":
                if f.get("client_secret") != "s3cret": return self.reply(400, {"error": "invalid_client"})
                t = secrets.token_hex(16); tokens[t] = {"scope": "service"}; return self.reply(200, {"access_token": t, "token_type": "Bearer", "expires_in": 3600})
            q = codes.pop(f.get("code", ""), None)
            if not q: return self.reply(400, {"error": "invalid_grant"})
            challenge = base64.urlsafe_b64encode(hashlib.sha256(f.get("code_verifier", "").encode()).digest()).rstrip(b"=").decode()
            if challenge != q.get("code_challenge") or f.get("redirect_uri") != q.get("redirect_uri"):
                return self.reply(400, {"error": "invalid_grant", "error_description": "PKCE or redirect mismatch"})
            t = secrets.token_hex(16); tokens[t] = q
            return self.reply(200, {"access_token": t, "token_type": "Bearer", "expires_in": 3600})
        body = json.loads(raw or b"{}")
        if path == "info":
            return self.reply(200, {"specs": "2.0.0.2", "name": "PDF42 mock QTSP", "region": "EU", "lang": "en",
                                    "authType": ["basic", "oauth2code", "oauth2client"], "oauth2": f"http://127.0.0.1:{PORT}/csc/v2",
                                    "methods": ["auth/login", "credentials/list", "credentials/info", "credentials/authorize", "signatures/signHash"]})
        if path == "auth/login":
            if self.headers.get("Authorization") != "Basic " + base64.b64encode(b"alice:pw").decode(): return self.reply(401, {"error": "invalid_request"})
            t = secrets.token_hex(16); tokens[t] = {"scope": "service"}; return self.reply(200, {"access_token": t})
        tok = self.bearer()
        if tok is None: return self.reply(401, {"error": "invalid_token"})
        if path == "credentials/list":
            return self.reply(200, {"credentialIDs": list(CREDS), "credentialInfos": list(CREDS.values()) if body.get("credentialInfo") else None})
        if path == "credentials/info":
            c = CREDS.get(body.get("credentialID"))
            return self.reply(200, c) if c else self.reply(400, {"error": "invalid_request"})
        if path == "credentials/authorize":
            auth = {a["id"]: a["value"] for a in body.get("authData", [])}
            if auth.get("PIN") != "1234" or auth.get("OTP") != "123456": return self.reply(400, {"error": "invalid_request", "error_description": "wrong PIN or OTP"})
            sad = secrets.token_hex(16); sads[sad] = (body["credentialID"], body["hashes"])
            return self.reply(200, {"SAD": sad, "expiresIn": 300})
        if path == "signatures/signHash":
            hashes = body["hashes"]
            if CREDS[body["credentialID"]]["auth"]["mode"] == "oauth2code":
                if tok.get("scope") != "credential" or tok.get("credentialID") != body["credentialID"] or [base64.urlsafe_b64decode(h + "=" * (-len(h) % 4)) for h in tok.get("hashes", "").split(",")] != [base64.b64decode(h) for h in hashes]:
                    return self.reply(400, {"error": "invalid_request", "error_description": "token not authorized for these hashes"})
            elif sads.pop(body.get("SAD"), None) != (body["credentialID"], hashes):
                return self.reply(400, {"error": "invalid_request", "error_description": "invalid SAD"})
            sigs = []
            for h in hashes:
                r = subprocess.run(["openssl", "pkeyutl", "-sign", "-inkey", os.path.join(PKI, "leaf.key"), "-pkeyopt", "digest:sha256"],
                                   input=base64.b64decode(h), capture_output=True, check=True)
                sigs.append(base64.b64encode(r.stdout).decode())
            return self.reply(200, {"signatures": sigs})
        self.reply(404, {"error": "not_found"})

HTTPServer(("127.0.0.1", PORT), H).serve_forever()
