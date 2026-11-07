"""The license check `make check` runs over the Rust workspace's crates.

    cd shell && cargo metadata --format-version 1 --locked \
        | python3 -I ../tools/licenses.py ../THIRD-PARTY.md

Reads `cargo metadata` on stdin and fails when:

* a crate in Cargo.lock has no SPDX license, or one whose terms can't be
  met under Lumen's GPL-3.0-or-later (an expression is met when one side of
  each OR, and both sides of each AND, are on the list below);
* a crate the workspace's own crates depend on directly is not named in
  THIRD-PARTY.md, where each says what it is used for.

Standard library only, so it runs in the build container and on the host.
"""

import json
import re
import sys

# Licenses whose terms a GPL-3.0-or-later program can meet.
ALLOWED = {
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "GPL-3.0-or-later",
    "ISC",
    "MIT",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Unlicense",
    "Zlib",
}
# The exceptions a license may carry with WITH.
EXCEPTIONS = {"LLVM-exception"}
# Old spellings crates.io still has.
ALIASES = {"Apache-2.0/MIT": "Apache-2.0 OR MIT", "MIT/Apache-2.0": "MIT OR Apache-2.0"}


class Bad(Exception):
    """An expression that isn't SPDX as this check reads it."""


def tokens(expr):
    return re.findall(r"\(|\)|[A-Za-z0-9.+-]+", expr)


def met(expr):
    """Whether the SPDX expression `expr` can be met with ALLOWED."""
    expr = ALIASES.get(expr.strip(), expr)
    toks = tokens(expr)
    if "".join(toks) != re.sub(r"\s", "", expr):
        raise Bad(expr)
    pos = 0

    def peek():
        return toks[pos] if pos < len(toks) else None

    def take():
        nonlocal pos
        if pos >= len(toks):
            raise Bad(expr)
        pos += 1
        return toks[pos - 1]

    def atom():
        tok = take()
        if tok == "(":
            value = either()
            if take() != ")":
                raise Bad(expr)
            return value
        if tok in (")", "AND", "OR", "WITH"):
            raise Bad(expr)
        name = tok[:-1] if tok.endswith("+") else tok
        ok = name in ALLOWED
        if peek() == "WITH":
            take()
            ok = ok and take() in EXCEPTIONS
        return ok

    def both():
        value = atom()
        while peek() == "AND":
            take()
            value = atom() and value
        return value

    def either():
        value = both()
        while peek() == "OR":
            take()
            value = both() or value
        return value

    value = either()
    if pos != len(toks):
        raise Bad(expr)
    return value


def problems(metadata, third_party):
    """Every problem, one line each, for `metadata` and THIRD-PARTY.md's text."""
    out = []
    members = set(metadata.get("workspace_members", []))
    packages = metadata.get("packages", [])
    for p in packages:
        if p["id"] in members:
            continue
        lic = p.get("license")
        if not lic:
            out.append(f"{p['name']} {p['version']}: no SPDX license")
            continue
        try:
            if not met(lic):
                out.append(f"{p['name']} {p['version']}: {lic} is not on the list")
        except Bad:
            out.append(f"{p['name']} {p['version']}: unreadable license {lic!r}")
    named = third_party.lower()
    direct = set()
    for p in packages:
        if p["id"] not in members:
            continue
        for d in p.get("dependencies", []):
            if d.get("path") is None and d.get("kind") in (None, "build"):
                direct.add(d["name"])
    for name in sorted(direct):
        if f"[{name.lower()}]" not in named and f"`{name.lower()}`" not in named:
            out.append(f"{name}: a direct dependency THIRD-PARTY.md does not name")
    return out


def main(argv):
    if len(argv) != 2:
        print("usage: cargo metadata ... | licenses.py THIRD-PARTY.md", file=sys.stderr)
        return 2
    metadata = json.load(sys.stdin)
    with open(argv[1], encoding="utf-8") as f:
        third_party = f.read()
    found = problems(metadata, third_party)
    for line in found:
        print(f"licenses: {line}", file=sys.stderr)
    count = len(metadata.get("packages", []))
    if found:
        return 1
    print(f"licenses: {count} crates, all on the list; direct dependencies named")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
