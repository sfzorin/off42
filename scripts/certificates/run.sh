#!/bin/zsh
# "Get a Certificate…" regression for PDF42 with a throw-away test CA on localhost.
#   scripts/certificates/run.sh WORKDIR [TSA-URL] [presence]
# Creates a Secure Enclave P-256 key and a Keychain RSA key (in WORKDIR/keys via SUITE42_KEY_DIR; the Keychain key
# and its certificate go to the login Keychain and are deleted at the end), builds PKCS#10 requests with PDF42,
# checks them with `openssl req -verify`, issues certificates from a local OpenSSL CA (root → issuing CA, AIA/CRL/OCSP
# on 127.0.0.1:898x), pairs them with the keys, signs WORKDIR/out/in.pdf at every PAdES level, prints
# `PDF42 --verify` and validates offline with EU DSS (the Maven/DSS setup of scripts/pades/run.sh, cached in WORKDIR).
# `presence`: the Secure Enclave key asks for Touch ID / the password at each signature (interactive).
set -euo pipefail
W=${1:?workdir}; TSA=${2:-http://timestamp.digicert.com}; PRESENCE=${3:-no-presence}
[ $PRESENCE = presence ] && PRESENCE=""
HERE=${0:A:h}; ROOT=${HERE:h:h}; B=$ROOT/.build/debug/PDF42
export SUITE42_KEY_DIR=$W/keys
rm -rf $W/pki $W/keys $W/out/*.pdf(N) && mkdir -p $W/pki/www $W/out
cd $W/pki
for n in root inter; do
  sed -e "s/\$ENV::DB/$n.db/; s/\$ENV::SERIAL/$n.srl/" $HERE/ca.cnf > $n.cnf
done
: > root.db; : > inter.db; echo 1000 > root.srl; echo 2000 > inter.srl
q() { "$@" >/dev/null 2>&1 || { echo "FAILED: $*"; exit 1; }; }
q openssl req -new -x509 -newkey rsa:2048 -nodes -keyout root.key -out root.pem -days 3650 -subj "/CN=Suite42 Test Root CA/O=Suite42 Test" -config root.cnf -extensions v3_root -sha256
q openssl req -new -newkey rsa:2048 -nodes -keyout inter.key -out inter.csr -subj "/CN=Suite42 Test Issuing CA/O=Suite42 Test" -config root.cnf
q openssl ca -batch -config root.cnf -cert root.pem -keyfile root.key -extensions v3_inter -in inter.csr -out inter.pem -notext
q openssl req -new -newkey rsa:2048 -nodes -keyout ocsp.key -out ocsp.csr -subj "/CN=Suite42 Test OCSP/O=Suite42 Test" -config root.cnf
q openssl ca -batch -config inter.cnf -cert inter.pem -keyfile inter.key -extensions v3_ocsp -in ocsp.csr -out ocsp.pem -notext
q openssl ca -gencrl -config root.cnf -cert root.pem -keyfile root.key -out root.crl.pem
q openssl ca -gencrl -config inter.cnf -cert inter.pem -keyfile inter.key -out inter.crl.pem
openssl x509 -in root.pem -outform der -out www/root.cer; openssl x509 -in inter.pem -outform der -out www/inter.cer

# Keys and requests made by PDF42; the CA signs them as they are.
SE=$($B --key-new enclave "Secure Enclave test key" ${PRESENCE:+$PRESENCE})
KC=$($B --key-new keychain "Suite42 Keychain test key" rsa2048)
trap '$B --key-delete $KC >/dev/null 2>&1; security delete-certificate -c "Suite42 Keychain Signer" >/dev/null 2>&1' EXIT
$B --key-csr $SE se.csr "CN=Suite42 Enclave Signer" O="Suite42 Test" C=TR email=enclave@example.com serial=PNOTR-12345678901
$B --key-csr $KC kc.csr "CN=Suite42 Keychain Signer" O="Suite42 Test" C=RU email=keychain@example.com
for r in se kc; do openssl req -in $r.csr -noout -verify 2>&1 | grep -q "verify OK" && echo "$r.csr: openssl req -verify OK" || { echo "$r.csr: verify FAILED"; exit 1; }; done
q openssl ca -batch -config inter.cnf -cert inter.pem -keyfile inter.key -extensions v3_leaf -in se.csr -out se.pem -notext
q openssl ca -batch -config inter.cnf -cert inter.pem -keyfile inter.key -extensions v3_leaf -in kc.csr -out kc.pem -notext
openssl x509 -in se.pem -noout -subject -ext keyUsage,subjectAltName 2>/dev/null || openssl x509 -in se.pem -noout -subject
q openssl ca -gencrl -config inter.cnf -cert inter.pem -keyfile inter.key -out inter.crl.pem
openssl crl -in root.crl.pem -outform der -out www/root.crl; openssl crl -in inter.crl.pem -outform der -out www/inter.crl
cat se.pem inter.pem > se-bundle.pem
$B --key-import any se-bundle.pem         # found by public key, with the intermediate
openssl x509 -in kc.pem -outform der -out kc.cer
$B --key-import $KC kc.cer                 # DER, chain fetched from caIssuers at signing
openssl x509 -in root.pem -outform der -out wrong.cer
$B --key-import $SE wrong.cer && { echo "FAILED: a foreign certificate was accepted"; exit 1; } || true
$B --key-list

pids=()
trap 'kill ${pids[@]} 2>/dev/null; $B --key-delete $KC >/dev/null 2>&1; security delete-certificate -c "Suite42 Keychain Signer" >/dev/null 2>&1' EXIT
python3 -m http.server 8980 --bind 127.0.0.1 --directory www >/dev/null 2>&1 & pids+=($!)
openssl ocsp -port 8981 -index root.db -CA root.pem -rsigner root.pem -rkey root.key -nmin 60 >/dev/null 2>&1 & pids+=($!)
openssl ocsp -port 8982 -index inter.db -CA inter.pem -rsigner ocsp.pem -rkey ocsp.key -nmin 60 >/dev/null 2>&1 & pids+=($!)
sleep 1

cd $W/out
[ -f in.pdf ] || cupsfilter -m application/pdf $ROOT/README.md > in.pdf 2>/dev/null
for L in B-B B-T B-LT B-LTA; do $B --key-sign in.pdf $SE enclave-$L.pdf tsa=$TSA level=$L; done
$B --key-sign in.pdf $KC keychain-B-LT.pdf tsa=$TSA level=B-LT
$B --key-sign enclave-B-T.pdf $KC enclave-then-keychain.pdf tsa=$TSA level=B-T
for f in *.pdf; do [ $f = in.pdf ] && continue; echo "-- $f"; $B --verify $f; done

if [ ! -x $W/maven/bin/mvn ]; then
  U=https://repo1.maven.org/maven2/org/apache/maven/apache-maven/3.9.9/apache-maven-3.9.9-bin.tar.gz
  curl -sfL -o $W/maven.tgz $U
  echo "$(curl -sfL $U.sha512)  $W/maven.tgz" | shasum -a 512 -c
  mkdir -p $W/maven && tar xzf $W/maven.tgz -C $W/maven --strip-components 1
fi
JAVA_HOME=${JAVA_HOME:-$(/usr/libexec/java_home 2>/dev/null || echo /opt/homebrew/opt/openjdk@21)}
export JAVA_HOME
rsync -a $ROOT/scripts/pades/validator/ $W/validator/
(cd $W/validator && $W/maven/bin/mvn -q -Dmaven.repo.local=$W/m2 package dependency:copy-dependencies -DoutputDirectory=target/lib)
security find-certificate -a -p /System/Library/Keychains/SystemRootCertificates.keychain > $W/trust.pem
cat $W/pki/root.pem >> $W/trust.pem
$JAVA_HOME/bin/java -cp "$W/validator/target/classes:$W/validator/target/lib/*" Validate $W/trust.pem ${${(@f)"$(ls *.pdf)"}:#in.pdf} 2>&1 | grep -v "INFO\|WARN\|SLF4J"
