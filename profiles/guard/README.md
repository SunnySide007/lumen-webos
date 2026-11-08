# Lumen Guard's rules

What Lumen Guard (`shell/crates/lumen-guard`) decides every lookup by, stage
G2 of [docs/specs/privacy.md](../../docs/specs/privacy.md): a name resolves
only when a rule allows it (decision 11). `tools/guard.sh push` copies the
rule files to `/var/lib/lumen/guard/rules/` on the TV.

| File | What it holds | Each rule |
|---|---|---|
| [allowlist.txt](allowlist.txt) | The generic list: the Content Store, DRM, the paid apps, casting and Lumen's probes, each with its reason and whether G1's week measured it | allows |
| [carve-outs.txt](carve-outs.txt) | Lumen's picks of names under an allowed suffix that stay blocked | blocks |
| [imported/](imported/README.md) | hagezi's, Perflyst's and Glasshouse's lists, each in its own file with its source and license | blocks |
| [required-blocks.txt](required-blocks.txt) | LG's firmware hosts | blocks, whatever any other rule says |

Not here: **the owner's own file** (decision 12), which says what the
household uses beyond the generic list. It lives on the TV
(`/var/lib/lumen/guard/owner.txt`), in the owner's backups, and on the PC
in `private/guard/owner.txt`, which git ignores. `tools/guard.sh keep`,
`drop` and `owner` change and push it.

## Syntax

* `allowlist.txt`, `carve-outs.txt` and the owner's file: `name` covers the
  name and every name under it, `=name` that name only. In the owner's file
  `!name` and `!=name` block instead of allowing.
* `imported/*.txt`, as their sources publish them: `name` is that name
  only, `*.name` the name and every name under it, and a hosts line's
  sinkhole address in front is skipped.
* `required-blocks.txt`: one exact name a line.

Text after `#` is a comment, and case does not matter.

## Which rule wins

1. The firmware hosts are blocked.
2. Then the owner's file, its most specific rule.
3. Then the most specific rule of the allowlist, the carve-outs and the
   imported lists: an exact rule beats a suffix, a longer suffix a shorter
   one, and where the very same rule both allows and blocks, it allows.
4. A name no rule covers is blocked, or, while the owner's learning window
   is open, allowed and logged as `learn`.

Guard refuses to block while these rules, with the owner's file, would block
a name it must resolve (the store, DRM, the paid apps and its own heartbeat:
`MUST_RESOLVE` in `rules.rs`). It keeps to logging, or keeps the owner's
previous file, and says so. `tools/guard/rules.py` decides the same way on
the PC: `python3 -I tools/guard/rules.py NAME...` prints each name's
verdict and the rule that decides, and `--check` is what
`tools/store-safe-check.sh --stage 5` and `tools/guard.sh` run before they
change anything on the TV.
