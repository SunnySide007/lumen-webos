//! Guard's rules, stage G2 (docs/specs/privacy.md, decision 11): a name
//! resolves only when a rule allows it. They are read from `profiles/guard/`
//! as pushed to the TV, plus the owner's own file, which stays off the public
//! repository (decision 12):
//!
//! * `allowlist.txt`: `name` allows the name and every name under it, `=name`
//!   that name only. The syntax `tools/store-safe-check.sh` reads.
//! * `carve-outs.txt`: the same syntax, but each rule blocks: names under an
//!   allowed suffix that stay blocked.
//! * `imported/*.txt`: block lists kept as their sources publish them
//!   (hagezi, Perflyst, Glasshouse's names): `name` blocks that name only,
//!   `*.name` the name and every name under it, and a hosts line's sinkhole
//!   address in front is skipped.
//! * `required-blocks.txt`: LG's firmware hosts, one exact name a line. They
//!   are blocked whatever any other rule says (docs/safety.md, rule 8).
//! * The owner's file: the allowlist's syntax, plus `!name` and `!=name` to
//!   block. It wins over every list but the firmware hosts.
//!
//! Between the allowlist, the carve-outs and the imported lists the most
//! specific rule wins: an exact rule over any suffix, a longer suffix over a
//! shorter one, and an allow over a block of the very same rule. So
//! `us.lgtvsdp.com` would stay allowed under a list's `*.lgtvsdp.com`, and
//! hagezi's `*.rdx2.lgtvsdp.com` blocks `us.rdx2.lgtvsdp.com` beside it. What
//! no rule covers is blocked, or learned while the owner's learning window is
//! open. Text after `#` is a comment, and case does not matter.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::net::IpAddr;
use std::path::Path;

pub const ALLOWLIST: &str = "allowlist.txt";
pub const CARVE_OUTS: &str = "carve-outs.txt";
pub const REQUIRED: &str = "required-blocks.txt";
pub const IMPORTED: &str = "imported";

/// Names Guard never blocks, whatever the files say: the store, DRM and the
/// paid apps that `tools/store-safe-check.sh` asks for on the TV, and the
/// boot hook's heartbeat, which has to reach the upstream to prove it. Rules
/// that would block one are refused: Guard keeps to logging, or keeps the
/// owner's previous file.
pub const MUST_RESOLVE: &[&str] = &[
    "us.lgtvsdp.com",
    "lgtvsdp.com",
    "us.lgeapi.com",
    "www.netflix.com",
    "www.amazon.com",
    "atv-ps.amazon.com",
    "plex.tv",
    "www.googleapis.com",
    "www.youtube.com",
    "spclient.wg.spotify.com",
    "heartbeat.lumen.invalid",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// Covered by no rule, and allowed because a learning window is open.
    Learn,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Allow,
    Block,
}

/// Exact and suffix rules, each allowing or blocking.
#[derive(Debug, Default, Clone)]
pub struct Table {
    exact: HashMap<String, Act>,
    suffix: HashMap<String, Act>,
}

impl Table {
    /// Adds a rule. Where the same rule both allows and blocks, it allows: the
    /// lists disagree, and the store-safe side wins until someone decides.
    pub fn insert(&mut self, name: &str, exact: bool, act: Act) {
        if !plain(name) {
            return;
        }
        let map = if exact {
            &mut self.exact
        } else {
            &mut self.suffix
        };
        let slot = map.entry(name.to_owned()).or_insert(act);
        if act == Act::Allow {
            *slot = Act::Allow;
        }
    }

    /// The most specific rule that covers `name`, if any.
    pub fn find(&self, name: &str) -> Option<Act> {
        if let Some(act) = self.exact.get(name) {
            return Some(*act);
        }
        let mut rest = name;
        loop {
            if let Some(act) = self.suffix.get(rest) {
                return Some(*act);
            }
            rest = rest.split_once('.')?.1;
        }
    }

    pub fn len(&self) -> usize {
        self.exact.len() + self.suffix.len()
    }

    fn count(&self, act: Act) -> usize {
        self.exact
            .values()
            .chain(self.suffix.values())
            .filter(|a| **a == act)
            .count()
    }

    /// Lines in the allowlist's syntax: `name`, `=name`, and, when `bangs`
    /// is set (the owner's file), `!name` and `!=name` to block.
    fn add_allow_syntax(&mut self, text: &str, act: Act, bangs: bool) {
        for line in text.lines() {
            let rule = strip(line);
            let (rule, act) = match rule.strip_prefix('!') {
                Some(rest) if bangs => (rest, Act::Block),
                _ => (rule.as_str(), act),
            };
            match rule.strip_prefix('=') {
                Some(exact) => self.insert(exact, true, act),
                None if !rule.is_empty() => self.insert(rule, false, act),
                None => {}
            }
        }
    }

    /// Block lists as published: plain names, `*.name` for a subtree, and
    /// hosts lines whose first field is a sinkhole address. A hosts line
    /// with a real address resolves its names rather than blocking them, so
    /// it is no rule.
    fn add_block_list(&mut self, text: &str) {
        for line in text.lines() {
            let line = strip(line);
            let mut fields = line.split_whitespace().peekable();
            match fields.peek().map(|f| f.parse::<IpAddr>()) {
                Some(Ok(addr)) if sinkhole(&addr) => {
                    fields.next();
                }
                Some(Ok(_)) => continue,
                _ => {}
            }
            for field in fields {
                match field.strip_prefix("*.") {
                    Some(suffix) => self.insert(suffix, false, Act::Block),
                    None => self.insert(field, true, Act::Block),
                }
            }
        }
    }
}

/// The owner's file, parsed. An empty file, or none, is no rules.
pub fn parse_owner(text: &str) -> Table {
    let mut table = Table::default();
    table.add_allow_syntax(text, Act::Allow, true);
    table
}

/// The public lists: what `profiles/guard/` holds.
#[derive(Debug, Default)]
pub struct Rules {
    required: HashSet<String>,
    lists: Table,
    /// Allow rules read from allowlist.txt, so a missing one shows.
    allow_rules: usize,
}

impl Rules {
    /// Reads the files from `dir`. A missing or unreadable file counts as
    /// empty and is named in the returned warnings; `imported/` may be absent.
    pub fn load(dir: &Path) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let mut read = |path: &Path| {
            fs::read_to_string(path).unwrap_or_else(|err| {
                warnings.push(format!("{}: {err}", path.display()));
                String::new()
            })
        };
        let allowlist = read(&dir.join(ALLOWLIST));
        let carve_outs = read(&dir.join(CARVE_OUTS));
        let required = read(&dir.join(REQUIRED));
        let mut imported = Vec::new();
        if let Ok(entries) = fs::read_dir(dir.join(IMPORTED)) {
            let mut paths: Vec<_> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "txt"))
                .collect();
            paths.sort();
            for path in paths {
                imported.push(read(&path));
            }
        }
        let imported: Vec<&str> = imported.iter().map(String::as_str).collect();
        let rules = Self::parse(&allowlist, &carve_outs, &required, &imported);
        (rules, warnings)
    }

    pub fn parse(allowlist: &str, carve_outs: &str, required: &str, imported: &[&str]) -> Self {
        let mut lists = Table::default();
        lists.add_allow_syntax(allowlist, Act::Allow, false);
        let allow_rules = lists.len();
        lists.add_allow_syntax(carve_outs, Act::Block, false);
        for text in imported {
            lists.add_block_list(text);
        }
        let required = required
            .lines()
            .map(strip)
            .filter(|name| plain(name))
            .collect();
        Self {
            required,
            lists,
            allow_rules,
        }
    }

    /// The verdict for `name`: firmware hosts, then the owner's rules, then
    /// the most specific of the lists, then the default (blocked, or learned
    /// while a window is open).
    pub fn verdict(&self, name: &str, owner: &Table, learning: bool) -> Verdict {
        if self.required.contains(name) {
            return Verdict::Block;
        }
        match owner.find(name).or_else(|| self.lists.find(name)) {
            Some(Act::Allow) => Verdict::Allow,
            Some(Act::Block) => Verdict::Block,
            None if learning => Verdict::Learn,
            None => Verdict::Block,
        }
    }

    /// The first name Guard must resolve that these rules, with `owner`'s,
    /// would block (or `allowlist.txt` when it held no rule); `None` when
    /// blocking is safe to switch on. One word, for the status file.
    pub fn unsafe_for(&self, owner: &Table) -> Option<&'static str> {
        if self.allow_rules == 0 {
            return Some(ALLOWLIST);
        }
        MUST_RESOLVE
            .iter()
            .copied()
            .find(|name| self.verdict(name, owner, false) != Verdict::Allow)
    }

    /// Rule counts for Guard's start-up line: allow, block (carve-outs and
    /// imported), firmware.
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.lists.count(Act::Allow),
            self.lists.count(Act::Block),
            self.required.len(),
        )
    }
}

/// A rule line without its comment, blanks or case.
fn strip(line: &str) -> String {
    line.split('#')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// A host name as the log writes plain ones: letters, digits, `-`, `_` and
/// dots, no empty label. Anything else in a file is not a rule.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// An address a hosts line uses to block a name rather than to resolve it.
fn sinkhole(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(a) => a.is_unspecified() || a.is_loopback(),
        IpAddr::V6(a) => a.is_unspecified() || a.is_loopback(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    fn none() -> Table {
        Table::default()
    }

    #[test]
    fn the_most_specific_rule_wins() {
        let rules = Rules::parse(
            "netflix.com\n=lgtvsdp.com\nus.lgtvsdp.com\namazon.com # Prime\n",
            "unagi-na.amazon.com\n=logs.netflix.com\n",
            "snu.lge.com\n",
            &["*.lgtvsdp.com\nrdx2.us.lgtvsdp.com\n0.0.0.0 ads.example *.track.example\n"],
        );
        for (name, want) in [
            ("netflix.com", Verdict::Allow),
            ("www.netflix.com", Verdict::Allow),
            ("notnetflix.com", Verdict::Block),
            // An exact carve-out under an allowed suffix blocks that name only.
            ("logs.netflix.com", Verdict::Block),
            ("nrdp.logs.netflix.com", Verdict::Allow),
            // A suffix carve-out blocks the subtree.
            ("unagi-na.amazon.com", Verdict::Block),
            ("x.unagi-na.amazon.com", Verdict::Block),
            ("api.amazon.com", Verdict::Allow),
            // An exact allow beats hagezi's wider suffix block.
            ("lgtvsdp.com", Verdict::Allow),
            // A longer allowed suffix beats a shorter blocked one.
            ("us.lgtvsdp.com", Verdict::Allow),
            ("cdn.us.lgtvsdp.com", Verdict::Allow),
            ("us.rdx2.lgtvsdp.com", Verdict::Block),
            // An imported plain name is exact.
            ("rdx2.us.lgtvsdp.com", Verdict::Block),
            ("x.rdx2.us.lgtvsdp.com", Verdict::Allow),
            ("ads.example", Verdict::Block),
            ("a.track.example", Verdict::Block),
            ("nobody.example", Verdict::Block),
            ("snu.lge.com", Verdict::Block),
        ] {
            assert_eq!(rules.verdict(name, &none(), false), want, "{name}");
        }
    }

    #[test]
    fn learning_allows_only_what_no_rule_covers() {
        let rules = Rules::parse(
            "netflix.com\n",
            "=logs.netflix.com\n",
            "snu.lge.com\n",
            &["ads.example\n"],
        );
        assert_eq!(rules.verdict("new.example", &none(), true), Verdict::Learn);
        assert_eq!(
            rules.verdict("www.netflix.com", &none(), true),
            Verdict::Allow
        );
        for name in ["logs.netflix.com", "ads.example", "snu.lge.com"] {
            assert_eq!(rules.verdict(name, &none(), true), Verdict::Block, "{name}");
        }
    }

    #[test]
    fn the_owner_wins_over_the_lists_but_not_the_firmware_hosts() {
        let rules = Rules::parse("netflix.com\n", "", "snu.lge.com\n", &["ads.example\n"]);
        let owner = parse_owner(
            "# the owner's own\n=ads.example\nnew.example\n!=logs.netflix.com\n\
             !ichnaea.netflix.com\n=snu.lge.com\n",
        );
        for (name, want) in [
            ("ads.example", Verdict::Allow),
            ("cdn.new.example", Verdict::Allow),
            ("logs.netflix.com", Verdict::Block),
            ("x.ichnaea.netflix.com", Verdict::Block),
            ("www.netflix.com", Verdict::Allow),
            ("snu.lge.com", Verdict::Block),
        ] {
            assert_eq!(rules.verdict(name, &owner, false), want, "{name}");
        }
        // Without the owner's file a `!` line is not a rule.
        let plain_list = Rules::parse("!netflix.com\n", "", "", &[]);
        assert_eq!(
            plain_list.verdict("netflix.com", &none(), false),
            Verdict::Block
        );
    }

    #[test]
    fn the_same_rule_both_ways_allows() {
        let rules = Rules::parse(
            "=a.example\nb.example\n",
            "=a.example\nb.example\n",
            "",
            &[],
        );
        assert_eq!(rules.verdict("a.example", &none(), false), Verdict::Allow);
        assert_eq!(rules.verdict("x.b.example", &none(), false), Verdict::Allow);
    }

    #[test]
    fn case_crlf_comments_and_junk() {
        let rules = Rules::parse(
            "Netflix.COM\r\n  =LGTVSDP.com  # apex\r\nbad name.example\n=\n*.star.example\n",
            "",
            "SU.lge.com\r\n",
            &[
                "*.ADS.example\r\n:: v6.example\n127.0.0.1\tlocal.example # x\n192.0.2.1 real.example\n",
            ],
        );
        let v = |n| rules.verdict(n, &none(), false);
        assert_eq!(v("api.netflix.com"), Verdict::Allow);
        assert_eq!(v("lgtvsdp.com"), Verdict::Allow);
        assert_eq!(v("x.lgtvsdp.com"), Verdict::Block);
        assert_eq!(v("su.lge.com"), Verdict::Block);
        assert_eq!(v("x.ads.example"), Verdict::Block);
        assert_eq!(v("v6.example"), Verdict::Block);
        assert_eq!(v("local.example"), Verdict::Block);
        // A hosts line with a real address is no rule: learned, not blocked.
        assert_eq!(rules.verdict("real.example", &none(), true), Verdict::Learn);
        assert_eq!(rules.verdict("192.0.2.1", &none(), true), Verdict::Learn);
        // The allowlist has no wildcard syntax: "*.star.example" is no rule.
        assert_eq!(v("a.star.example"), Verdict::Block);
        assert_eq!(rules.counts().0, 2);
    }

    #[test]
    fn missing_files_are_empty_named_and_unsafe() {
        let (rules, warnings) = Rules::load(Path::new("/nonexistent/lumen-guard-rules"));
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings[0].contains(ALLOWLIST));
        assert_eq!(
            rules.verdict("us.lgtvsdp.com", &none(), false),
            Verdict::Block
        );
        assert!(rules.unsafe_for(&none()).is_some());
    }

    #[test]
    fn an_owner_file_that_blocks_the_store_is_unsafe() {
        let (rules, warnings) = Rules::load(&repo().join("profiles/guard"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(rules.unsafe_for(&none()), None);
        let owner = parse_owner("!=us.lgtvsdp.com\n");
        assert_eq!(rules.unsafe_for(&owner), Some("us.lgtvsdp.com"));
        let owner = parse_owner("!lumen.invalid\n");
        assert_eq!(rules.unsafe_for(&owner), Some("heartbeat.lumen.invalid"));
    }

    /// The shared cases (tools/tests/fixtures/guard-g2), which
    /// tools/tests/test_guard_rules.py checks against tools/guard/rules.py.
    #[test]
    fn the_shared_fixture_cases() {
        let dir = repo().join("tools/tests/fixtures/guard-g2");
        let (rules, warnings) = Rules::load(&dir.join("rules"));
        assert!(warnings.is_empty(), "{warnings:?}");
        let owner = parse_owner(&fs::read_to_string(dir.join("owner.txt")).unwrap());
        let cases = fs::read_to_string(dir.join("cases.txt")).unwrap();
        let mut n = 0;
        for line in cases.lines() {
            let line = strip(line);
            let mut f = line.split_whitespace();
            let (Some(name), Some(want), Some(learn)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            let want = match want {
                "allow" => Verdict::Allow,
                "learn" => Verdict::Learn,
                "block" => Verdict::Block,
                other => panic!("bad verdict {other} in cases.txt"),
            };
            let learning = learn == "learning";
            assert_eq!(
                rules.verdict(name, &owner, learning),
                want,
                "{name} ({learn})"
            );
            n += 1;
        }
        assert!(n >= 20, "only {n} cases read");
    }

    #[test]
    fn the_shipped_profile_keeps_the_store_and_blocks_the_rest() {
        let (rules, warnings) = Rules::load(&repo().join("profiles/guard"));
        assert!(warnings.is_empty(), "{warnings:?}");
        let v = |n| rules.verdict(n, &none(), false);
        for name in MUST_RESOLVE.iter().chain(&[
            // Measured in G1's week (2026-10-31 to 11-08) from the store,
            // Netflix, Prime Video and YouTube.
            "us.security.lgtvsdp.com",
            "us.tvsdp.lgeapi.com",
            "gfts.lge.com",
            "nrdp.logs.netflix.com",
            "lg-tv-2019.prod.partner.netflix.net",
            "occ-0-1503-1504.1.nflxso.net",
            "api.amazon.com",
            "s0s7.api.amazonvideo.com",
            "m.media-amazon.com",
            "d184dfn36gombl.cloudfront.net",
            "www.gstatic.com",
            "yt3.googleusercontent.com",
            "rr5---sn-bvvbaxivnuxqjvm-n5oe.googlevideo.com",
            "plex.direct",
            "lumen-probe.invalid",
        ]) {
            assert_eq!(v(name), Verdict::Allow, "{name}");
        }
        for name in [
            "snu.lge.com",
            "su.lge.com",
            "su-ssl.lge.com",
            "su-dev.lge.com",
            "snu-dev.lge.com",
            "nsu.lge.com",
            // LG Home, ads, beacons, AI and UEI, measured in G1's week.
            "aic.cdpbeacon.lgtvcommon.com",
            "aic.cdpsvc.lgtvcommon.com",
            "aic.homeprv.lgtvcommon.com",
            "aic.netflixvoice.lgtvcommon.com",
            "aic-ngfts.lge.com",
            "ngfts.lge.com",
            "us.info.lgsmartad.com",
            "us.ibs.lgappstv.com",
            "us.lgrecommends.lgappstv.com",
            "www.ueiwsp.com",
            // Carve-outs and imported names under allowed suffixes.
            "unagi-na.amazon.com",
            "eligibility-panelresearch.googlevideo.com",
            "us.rdx2.lgtvsdp.com",
            "us.rdx2.nextlgsdp.com",
            "us.nextlgsdp.com",
            "cert-test.sandbox.google.com",
            // Ad and telemetry hosts no rule allows.
            "static.doubleclick.net",
            "global.telemetry.insights.video.a2z.com",
            "api.alphonso.tv",
            "us.elastic.lgwebostv.com",
            "154.23.20.104.in-addr.arpa",
        ] {
            assert_eq!(v(name), Verdict::Block, "{name}");
        }
    }
}
