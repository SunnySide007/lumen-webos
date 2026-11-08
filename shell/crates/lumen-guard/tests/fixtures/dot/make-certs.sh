#!/bin/sh
# Makes the test certificates for tests/forward.rs's local DNS-over-TLS
# server: a CA, and a server certificate for dot.lumen.test signed by it, all
# DER, valid for 100 years from the day they were made. Test-only: the key is
# committed and protects nothing. Run in the build container:
#
#   tools/dev.sh sh shell/crates/lumen-guard/tests/fixtures/dot/make-certs.sh
set -eu

cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# The SDK's openssl, first on the container's PATH, looks for a config file
# its build machine had; an empty one will do.
: >"$tmp/empty.cnf"
export OPENSSL_CONF="$tmp/empty.cnf"

cat >"$tmp/ca.cnf" <<'EOF'
[req]
distinguished_name = dn
prompt = no
x509_extensions = ext
[dn]
CN = Lumen Guard test CA
[ext]
basicConstraints = critical, CA:TRUE
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
EOF

cat >"$tmp/server.cnf" <<'EOF'
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = serverAuth
subjectAltName = DNS:dot.lumen.test
authorityKeyIdentifier = keyid
subjectKeyIdentifier = hash
EOF

openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/ca.key"
openssl req -new -x509 -key "$tmp/ca.key" -config "$tmp/ca.cnf" -days 36500 \
    -sha256 -outform DER -out ca.der
openssl x509 -inform DER -in ca.der -out "$tmp/ca.pem"

openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/server.key"
openssl req -new -key "$tmp/server.key" -subj "/CN=dot.lumen.test" -out "$tmp/server.csr"
openssl x509 -req -in "$tmp/server.csr" -CA "$tmp/ca.pem" -CAkey "$tmp/ca.key" \
    -CAcreateserial -days 36500 -sha256 -extfile "$tmp/server.cnf" \
    -outform DER -out server.der
openssl pkcs8 -topk8 -nocrypt -in "$tmp/server.key" -outform DER -out server.key.der
ls -l ca.der server.der server.key.der
