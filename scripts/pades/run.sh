#!/bin/zsh
# PAdES / CSC regression for PDF42 with a throw-away test PKI on localhost.
#   scripts/pades/run.sh WORKDIR [TSA-URL]
# Creates root → issuing CA → signer certificates (AIA/CRL URLs on 127.0.0.1), serves CA certificates and CRLs
# (port 8880), OCSP (8881 root, 8882 issuing CA, `openssl ocsp`) and a mock CSC v2 service (8890), signs
# WORKDIR/in.pdf at every level locally and remotely, prints `PDF42 --verify`, then validates offline with EU DSS
# (needs JDK 17+; Maven 3.9 is downloaded into WORKDIR from Maven Central with its SHA-512 checked).
# Servers are stopped on exit. Network: the TSA (default DigiCert) and Maven Central.
set -euo pipefail
W=${1:?workdir}; TSA=${2:-http://timestamp.digicert.com}
HERE=${0:A:h}; ROOT=${HERE:h:h}; B=$ROOT/.build/debug/PDF42
mkdir -p $W/pki/www $W/out
cd $W/pki
sed -e 's/\$ENV::DB/root.db/; s/\$ENV::SERIAL/root.srl/' $HERE/ca.cnf > root.cnf
sed -e 's/\$ENV::DB/inter.db/; s/\$ENV::SERIAL/inter.srl/' $HERE/ca.cnf > inter.cnf
: > root.db; : > inter.db; echo 1000 > root.srl; echo 2000 > inter.srl
q() { "$@" >/dev/null 2>&1 || { echo "FAILED: $*"; exit 1; }; }
q openssl req -new -x509 -newkey rsa:2048 -nodes -keyout root.key -out root.pem -days 3650 -subj "/CN=PDF42 Test Root CA/O=PDF42 Test" -config root.cnf -extensions v3_root -sha256
q openssl req -new -newkey rsa:2048 -nodes -keyout inter.key -out inter.csr -subj "/CN=PDF42 Test Issuing CA/O=PDF42 Test" -config root.cnf
q openssl ca -batch -config root.cnf -cert root.pem -keyfile root.key -extensions v3_inter -in inter.csr -out inter.pem -notext
q openssl req -new -newkey rsa:2048 -nodes -keyout leaf.key -out leaf.csr -subj "/CN=Test Signer PAdES/O=PDF42 Test" -config root.cnf
q openssl ca -batch -config inter.cnf -cert inter.pem -keyfile inter.key -extensions v3_leaf -in leaf.csr -out leaf.pem -notext
q openssl req -new -newkey rsa:2048 -nodes -keyout ocsp.key -out ocsp.csr -subj "/CN=PDF42 Test OCSP/O=PDF42 Test" -config root.cnf
q openssl ca -batch -config inter.cnf -cert inter.pem -keyfile inter.key -extensions v3_ocsp -in ocsp.csr -out ocsp.pem -notext
q openssl ca -gencrl -config root.cnf -cert root.pem -keyfile root.key -out root.crl.pem
q openssl ca -gencrl -config inter.cnf -cert inter.pem -keyfile inter.key -out inter.crl.pem
openssl x509 -in root.pem -outform der -out www/root.cer; openssl x509 -in inter.pem -outform der -out www/inter.cer
openssl crl -in root.crl.pem -outform der -out www/root.crl; openssl crl -in inter.crl.pem -outform der -out www/inter.crl
openssl pkcs12 -export -inkey leaf.key -in leaf.pem -out leaf.p12 -passout pass:test -name "Test Signer PAdES"

pids=()
trap 'kill ${pids[@]} 2>/dev/null' EXIT
python3 -m http.server 8880 --bind 127.0.0.1 --directory www >/dev/null 2>&1 & pids+=($!)
openssl ocsp -port 8881 -index root.db -CA root.pem -rsigner root.pem -rkey root.key -nmin 60 >/dev/null 2>&1 & pids+=($!)
openssl ocsp -port 8882 -index inter.db -CA inter.pem -rsigner ocsp.pem -rkey ocsp.key -nmin 60 >/dev/null 2>&1 & pids+=($!)
python3 $HERE/csc_mock.py $W/pki >/dev/null 2>&1 & pids+=($!)
sleep 1

cd $W/out
[ -f in.pdf ] || cupsfilter -m application/pdf $ROOT/README.md > in.pdf 2>/dev/null
for L in B-B B-T B-LT B-LTA; do $B --sign in.pdf ../pki/leaf.p12 test $L.pdf tsa=$TSA level=$L; done
$B --sign in.pdf ../pki/leaf.p12 test pkcs7.pdf tsa=$TSA level=pkcs7
$B --ltv B-T.pdf T-ltv.pdf
$B --doc-timestamp T-ltv.pdf T-ltv-docts.pdf $TSA
$B --doc-timestamp in.pdf unsigned-docts.pdf $TSA
U=http://127.0.0.1:8890/csc/v2
$B --csc-credentials $U user=alice password=pw
$B --csc-sign in.pdf csc-B-LTA.pdf $U cred-explicit user=alice password=pw pin=1234 otp=123456 tsa=$TSA level=B-LTA
python3 - <<'PY'
d = bytearray(open("B-LTA.pdf", "rb").read()); i = d.find(b"stream", 2000) + 20; d[i] ^= 1
open("tampered.pdf", "wb").write(d)
PY
for f in *.pdf; do [ $f = in.pdf ] && continue; echo "-- $f"; $B --verify $f; done

# EU DSS, offline: only validation data inside the PDFs; trust anchors = macOS system roots + the test root.
if [ ! -x $W/maven/bin/mvn ]; then
  U=https://repo1.maven.org/maven2/org/apache/maven/apache-maven/3.9.9/apache-maven-3.9.9-bin.tar.gz
  curl -sfL -o $W/maven.tgz $U
  echo "$(curl -sfL $U.sha512)  $W/maven.tgz" | shasum -a 512 -c
  mkdir -p $W/maven && tar xzf $W/maven.tgz -C $W/maven --strip-components 1
fi
JAVA_HOME=${JAVA_HOME:-$(/usr/libexec/java_home 2>/dev/null || echo /opt/homebrew/opt/openjdk@21)}
export JAVA_HOME
rsync -a $HERE/validator/ $W/validator/
(cd $W/validator && $W/maven/bin/mvn -q -Dmaven.repo.local=$W/m2 package dependency:copy-dependencies -DoutputDirectory=target/lib)
security find-certificate -a -p /System/Library/Keychains/SystemRootCertificates.keychain > $W/trust.pem
cat $W/pki/root.pem >> $W/trust.pem
$JAVA_HOME/bin/java -cp "$W/validator/target/classes:$W/validator/target/lib/*" Validate $W/trust.pem ${${(@f)"$(ls *.pdf)"}:#in.pdf} 2>&1 | grep -v "INFO\|WARN\|SLF4J"
