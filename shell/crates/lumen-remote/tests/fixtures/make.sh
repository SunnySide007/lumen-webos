#!/bin/sh
# Remakes the test certificates for lumen-remote's tests (tests/e2e.rs):
# an authority for 127.0.0.1 and localhost made by tools/remote/ca.sh, the
# server's certificate from it, and a "rogue" certificate the same
# authority signs for a name outside its constraints, which a client must
# refuse. Test keys only: they guard nothing. Run from the repository's
# root, in the build container: tools/dev.sh sh shell/crates/lumen-remote/tests/fixtures/make.sh
set -eu
here=shell/crates/lumen-remote/tests/fixtures
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
sh tools/remote/ca.sh init --ip 127.0.0.1 --dns localhost --dir "$tmp" --ca-days 36500 --leaf-days 36500 >/dev/null
cp "$tmp/lumen-ca.crt" "$tmp/server.crt" "$tmp/server.key" "$here/"
# The rogue: signed by the authority for rogue.example, outside its names.
cat >"$tmp/rogue.cnf" <<CNF
[req]
distinguished_name = dn
prompt = no
[dn]
CN = rogue.example
[leaf]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = serverAuth
subjectAltName = DNS:rogue.example,IP:10.9.8.7
CNF
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$here/rogue.key" 2>/dev/null
openssl req -new -key "$here/rogue.key" -config "$tmp/rogue.cnf" -out "$tmp/rogue.csr"
openssl x509 -req -in "$tmp/rogue.csr" -CA "$tmp/lumen-ca.crt" -CAkey "$tmp/lumen-ca.key" \
    -set_serial 0x0badc0de -days 36500 -sha256 -extfile "$tmp/rogue.cnf" -extensions leaf \
    -out "$here/rogue.crt" 2>/dev/null
chmod 644 "$here"/*.key "$here"/*.crt
ls -l "$here"
