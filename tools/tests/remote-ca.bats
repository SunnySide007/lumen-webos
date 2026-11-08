#!/usr/bin/env bats
# tools/remote/ca.sh (R4, D4): Lumen's own authority, limited to the TV's
# names, with openssl as the judge. Runs on the PC only; nothing here
# reaches a TV.

setup() {
    T="$BATS_TEST_TMPDIR"
    CA="$BATS_TEST_DIRNAME/../remote/ca.sh"
    D="$T/ca"
}

ca() {
    run sh "$CA" "$@" --dir "$D"
}

# A certificate for NAMES (a subjectAltName value) signed with the
# authority's own key, as a thief with that key would sign one.
rogue() {
    openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$T/rogue.key" 2>/dev/null
    printf '[req]\ndistinguished_name = dn\nprompt = no\n[dn]\nCN = Rogue certificate\n[leaf]\nsubjectAltName = %s\nextendedKeyUsage = serverAuth\n' \
        "$1" >"$T/rogue.cnf"
    openssl req -new -key "$T/rogue.key" -config "$T/rogue.cnf" -out "$T/rogue.csr"
    openssl x509 -req -in "$T/rogue.csr" -CA "$D/lumen-ca.crt" -CAkey "$D/lumen-ca.key" \
        -set_serial 7 -days 30 -extfile "$T/rogue.cnf" -extensions leaf -out "$T/rogue.crt" 2>/dev/null
}

@test "init makes an authority and a certificate for the TV that verifies" {
    ca init --ip 192.168.86.20 --dns lumen-tv.lan
    [ "$status" -eq 0 ]
    openssl verify -CAfile "$D/lumen-ca.crt" "$D/server.crt"
    run openssl x509 -in "$D/server.crt" -noout -ext subjectAltName,extendedKeyUsage,keyUsage
    [[ "$output" == *"IP Address:192.168.86.20"* ]]
    [[ "$output" == *"DNS:lumen-tv.lan"* ]]
    [[ "$output" == *"TLS Web Server Authentication"* ]]
    [[ "$output" == *"Digital Signature"* ]]
    [ "$(stat -c %a "$D")" = 700 ]
    [ "$(stat -c %a "$D/lumen-ca.key")" = 600 ]
    [ "$(stat -c %a "$D/server.key")" = 600 ]
    [ ! -e "$D/server.csr" ] && [ ! -e "$D/.ca.cnf" ] && [ ! -e "$D/.leaf.cnf" ]
}

@test "the authority carries critical name constraints, exactly the TV's names" {
    ca init --ip 192.168.86.20 --dns lumen-tv.lan
    [ "$status" -eq 0 ]
    run openssl x509 -in "$D/lumen-ca.crt" -noout -text
    [[ "$output" == *"X509v3 Name Constraints: critical"* ]]
    [[ "$output" == *"IP:192.168.86.20/255.255.255.255"* ]]
    [[ "$output" == *"DNS:lumen-tv.lan"* ]]
    [[ "$output" == *"CA:TRUE, pathlen:0"* ]]
    # An address alone still binds DNS names, to the reserved .invalid.
    rm -rf "$D"
    ca init --ip 192.168.86.20
    run openssl x509 -in "$D/lumen-ca.crt" -noout -text
    [[ "$output" == *"DNS:lumen.invalid"* ]]
}

@test "a certificate for any other name, signed with the authority's key, does not verify" {
    # With an address alone, and with a DNS name too.
    for dns in "" lumen-tv.lan; do
        rm -rf "$D"
        ca init --ip 192.168.86.20 ${dns:+--dns $dns}
        [ "$status" -eq 0 ]
        for names in DNS:accounts.google.com IP:192.168.86.21 IP:10.0.0.1 "IP:192.168.86.20,DNS:example.com" \
            DNS:lumen-tv.lan.example.com IP:::1; do
            rogue "$names"
            run openssl verify -CAfile "$D/lumen-ca.crt" "$T/rogue.crt"
            [ "$status" -ne 0 ]
            [[ "$output" == *"violation"* ]] || [[ "$output" == *"name constraint"* ]]
        done
        # The TV's own name still does, so the refusals above are the constraints'.
        rogue IP:192.168.86.20
        openssl verify -CAfile "$D/lumen-ca.crt" "$T/rogue.crt"
    done
}

@test "init again keeps the authority and issues a new certificate for the TV" {
    ca init --ip 192.168.86.20
    [ "$status" -eq 0 ]
    before=$(openssl x509 -in "$D/lumen-ca.crt" -noout -fingerprint -sha256)
    leaf=$(openssl x509 -in "$D/server.crt" -noout -serial)
    ca init --ip 192.168.86.20
    [ "$status" -eq 0 ]
    [[ "$output" == *"the authority is there already"* ]]
    [ "$(openssl x509 -in "$D/lumen-ca.crt" -noout -fingerprint -sha256)" = "$before" ]
    [ "$(openssl x509 -in "$D/server.crt" -noout -serial)" != "$leaf" ]
}

@test "init for another address refuses, changing nothing" {
    ca init --ip 192.168.86.20
    [ "$status" -eq 0 ]
    sum=$(cat "$D/lumen-ca.crt" "$D/server.crt" | sha256sum)
    ca init --ip 192.168.86.30
    [ "$status" -eq 1 ]
    [[ "$output" == *"move $D away"* ]]
    [ "$(cat "$D/lumen-ca.crt" "$D/server.crt" | sha256sum)" = "$sum" ]
}

@test "issue makes a new certificate from the same authority, which verifies" {
    ca init --ip 192.168.86.20 --leaf-days 30
    [ "$status" -eq 0 ]
    ca issue
    [ "$status" -eq 0 ]
    openssl verify -CAfile "$D/lumen-ca.crt" "$D/server.crt"
    run openssl x509 -in "$D/server.crt" -noout -checkend $((390 * 86400))
    [ "$status" -eq 0 ]
}

@test "issue and show without an authority say what to run" {
    ca issue
    [ "$status" -eq 1 ]
    [[ "$output" == *"ca.sh init --ip"* ]]
    ca show
    [ "$status" -eq 1 ]
}

@test "show prints the names, the dates and the fingerprints, never a key" {
    ca init --ip 192.168.86.20
    ca show
    [ "$status" -eq 0 ]
    [[ "$output" == *"names:     IP:192.168.86.20"* ]]
    [[ "$output" == *"SHA-256"* ]]
    [[ "$output" != *"PRIVATE KEY"* ]]
}

@test "bad addresses, names and day counts are refused before anything is made" {
    for args in "--ip 192.168.86.256" "--ip lumen-tv" "--ip 1.2.3.4 --dns Lumen.TV" \
        "--ip 1.2.3.4 --dns 10.0.0.1" "--ip 1.2.3.4 --ca-days 0" "--ip 1.2.3.4 --leaf-days x"; do
        # shellcheck disable=SC2086
        ca init $args
        [ "$status" -eq 1 ]
    done
    ca init
    [ "$status" -eq 1 ]
    [ ! -e "$D" ]
}
