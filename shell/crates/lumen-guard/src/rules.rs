//! The block and allow rules, read from the files in `profiles/glasshouse/`
//! as they are. The syntax is the one `tools/store-safe-check.sh` and
//! `tools/capture/summarize.py` already read:
//!
//! * `allowlist.txt`: `name` covers the name and every name under it, `=name`
//!   that name only;
//! * `required-blocks.txt`: one exact name per line;
//! * `b9-aggressive.hosts`: hosts lines whose address is a sinkhole (0.0.0.0,
//!   127.x, `::`, `::1`) block their names exactly; commented lines are
//!   candidates, not blocks.
//!
//! Text after `#` is a comment, case does not matter, and the allowlist wins
//! over everything.

use std::collections::HashSet;
use std::fs;
use std::net::IpAddr;
use std::path::Path;

pub const ALLOWLIST: &str = "allowlist.txt";
pub const REQUIRED: &str = "required-blocks.txt";
pub const HOSTS: &str = "b9-aggressive.hosts";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// On a block list and not allowlisted. G1 only logs it.
    WouldBlock,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::WouldBlock => "would-block",
        }
    }
}

#[derive(Debug, Default)]
pub struct Rules {
    allow: HashSet<String>,
    allow_exact: HashSet<String>,
    block: HashSet<String>,
}

impl Rules {
    /// Reads the three files from `dir`. A missing or unreadable file counts
    /// as empty, and is named in the returned warnings.
    pub fn load(dir: &Path) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let mut read = |name: &str| {
            let path = dir.join(name);
            fs::read_to_string(&path).unwrap_or_else(|err| {
                warnings.push(format!("{}: {err}", path.display()));
                String::new()
            })
        };
        let rules = Self::parse(&read(ALLOWLIST), &read(REQUIRED), &read(HOSTS));
        (rules, warnings)
    }

    pub fn parse(allowlist: &str, required: &str, hosts: &str) -> Self {
        let mut rules = Self::default();
        for line in allowlist.lines() {
            let rule = strip(line);
            if let Some(exact) = rule.strip_prefix('=') {
                rules.allow_exact.insert(exact.to_owned());
            } else if !rule.is_empty() {
                rules.allow.insert(rule);
            }
        }
        for line in required.lines() {
            let name = strip(line);
            if !name.is_empty() {
                rules.block.insert(name);
            }
        }
        for line in hosts.lines() {
            let line = line.split('#').next().unwrap_or("");
            let mut fields = line.split_whitespace();
            if fields.next().is_some_and(sinkhole) {
                rules.block.extend(fields.map(str::to_ascii_lowercase));
            }
        }
        rules
    }

    pub fn verdict(&self, name: &str) -> Verdict {
        if self.allowed(name) {
            Verdict::Allow
        } else if self.block.contains(name) {
            Verdict::WouldBlock
        } else {
            Verdict::Allow
        }
    }

    fn allowed(&self, name: &str) -> bool {
        if self.allow_exact.contains(name) {
            return true;
        }
        let mut rest = name;
        loop {
            if self.allow.contains(rest) {
                return true;
            }
            match rest.split_once('.') {
                Some((_, parent)) => rest = parent,
                None => return false,
            }
        }
    }

    /// Rule counts for Guard's start-up line: allow, exact allow, block.
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.allow.len(), self.allow_exact.len(), self.block.len())
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

/// An address a hosts line uses to block a name rather than to resolve it.
fn sinkhole(addr: &str) -> bool {
    match addr.parse::<IpAddr>() {
        Ok(IpAddr::V4(a)) => a.is_unspecified() || a.is_loopback(),
        Ok(IpAddr::V6(a)) => a.is_unspecified() || a.is_loopback(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // tools/tests/test_capture_summary.py, ListsTest.test_rules.
    fn capture_test_rules() -> Rules {
        Rules::parse(
            "# store\nnetflix.com  # Netflix\n=lgtvsdp.com\nus.lgtvsdp.com\n",
            "# firmware\nsnu.lge.com\n",
            "# header\n0.0.0.0\tads.example\n\
             # 0.0.0.0 maybe.example  # confirm with capture\n\
             0.0.0.0 api.netflix.com\n",
        )
    }

    #[test]
    fn summarize_py_cases() {
        let rules = capture_test_rules();
        for (name, want) in [
            ("netflix.com", Verdict::Allow),
            ("www.netflix.com", Verdict::Allow),
            ("notnetflix.com", Verdict::Allow),
            ("lgtvsdp.com", Verdict::Allow),
            ("ad.lgtvsdp.com", Verdict::Allow),
            ("x.us.lgtvsdp.com", Verdict::Allow),
            ("snu.lge.com", Verdict::WouldBlock),
            ("ads.example", Verdict::WouldBlock),
            // A commented hosts line is a candidate, not a block.
            ("maybe.example", Verdict::Allow),
            // On a block list and allowlisted: the allowlist wins.
            ("api.netflix.com", Verdict::Allow),
        ] {
            assert_eq!(rules.verdict(name), want, "{name}");
        }
        assert!(rules.allowed("www.netflix.com"));
        assert!(!rules.allowed("notnetflix.com"));
        assert!(!rules.allowed("ad.lgtvsdp.com"));
    }

    // tools/tests/fixtures/store-safe/profile, as store-safe-check.bats reads it.
    #[test]
    fn store_safe_fixture_cases() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tools/tests/fixtures/store-safe/profile");
        let (rules, warnings) = Rules::load(&fixture);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(rules.verdict("api.netflix.com"), Verdict::Allow);
        assert_eq!(rules.verdict("lgtvsdp.com"), Verdict::Allow);
        // An exact rule covers the apex only.
        assert!(!rules.allowed("us.rdx2.lgtvsdp.com"));
        assert!(!rules.allowed("notnetflix.com"));
        assert_eq!(rules.verdict("snu.lge.com"), Verdict::WouldBlock);
        // "su.lge.com  # comment"
        assert_eq!(rules.verdict("su.lge.com"), Verdict::WouldBlock);
        assert_eq!(rules.verdict("ads.example.com"), Verdict::WouldBlock);
        assert_eq!(rules.verdict("x.ads.example.com"), Verdict::Allow);
        assert_eq!(rules.verdict("candidate.example.com"), Verdict::Allow);
    }

    #[test]
    fn the_shipped_profile_blocks_the_firmware_hosts_and_keeps_the_store() {
        let profile = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../profiles/glasshouse");
        let (rules, warnings) = Rules::load(&profile);
        assert!(warnings.is_empty(), "{warnings:?}");
        for name in [
            "snu.lge.com",
            "su.lge.com",
            "su-ssl.lge.com",
            "su-dev.lge.com",
            "nsu.lge.com",
        ] {
            assert_eq!(rules.verdict(name), Verdict::WouldBlock, "{name}");
        }
        for name in [
            "us.lgtvsdp.com",
            "lgtvsdp.com",
            "www.netflix.com",
            "plex.tv",
            "googleapis.com",
        ] {
            assert_eq!(rules.verdict(name), Verdict::Allow, "{name}");
        }
    }

    #[test]
    fn case_crlf_and_every_sinkhole_form() {
        let rules = Rules::parse(
            "Netflix.COM\r\n=LGTVSDP.com\r\n",
            "SU.lge.com\r\n",
            "127.0.0.2 a.example\r\n:: b.example c.example\n::1\td.example\n\
             0:0:0:0:0:0:0:0 e.example\n192.0.2.1 real.example\n  0.0.0.0 f.example # note\n",
        );
        assert_eq!(rules.verdict("api.netflix.com"), Verdict::Allow);
        assert!(rules.allowed("lgtvsdp.com"));
        for name in [
            "su.lge.com",
            "a.example",
            "b.example",
            "c.example",
            "d.example",
            "e.example",
            "f.example",
        ] {
            assert_eq!(rules.verdict(name), Verdict::WouldBlock, "{name}");
        }
        assert_eq!(rules.verdict("real.example"), Verdict::Allow);
        assert_eq!(rules.verdict("note"), Verdict::Allow);
    }

    #[test]
    fn missing_files_are_empty_and_named() {
        let (rules, warnings) = Rules::load(Path::new("/nonexistent/lumen-guard-rules"));
        assert_eq!(warnings.len(), 3);
        assert!(warnings[0].contains(ALLOWLIST));
        assert_eq!(rules.verdict("snu.lge.com"), Verdict::Allow);
    }
}
