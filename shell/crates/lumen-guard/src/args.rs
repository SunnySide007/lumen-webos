//! The command line, parsed by hand as `lumen-shell` does. Unlike the shell,
//! Guard refuses anything it doesn't know: only Lumen's own scripts start it.

use std::net::SocketAddrV4;
use std::path::PathBuf;

use crate::lookups::DEFAULT_MAX_BYTES;

pub const USAGE: &str = "\
usage: lumen-guard --listen ADDR:PORT --upstream ADDR:PORT --rules DIR --log DIR
                   --status FILE [--mode log] [--dscp N] [--max-log-bytes N]
       lumen-guard --probe ADDR:PORT [--name NAME]

Forwards every DNS query that reaches --listen (UDP and TCP) to --upstream
unchanged, and logs it to DIR/lookups.log with the verdict the rules in --rules
would give. --mode block is stage G2 and not built. --dscp marks the packets
Guard sends upstream (0 to 63, default 7; 0 marks nothing) so the TV's nat rule
can leave them alone. --max-log-bytes is the size at which lookups.log moves to
lookups.log.1 (default 4 MiB).

--probe asks ADDR:PORT for NAME (default lumen-probe.invalid) and exits 0 on
any answer within 2 s, 1 otherwise.
";

/// The DSCP value Guard marks its upstream packets with unless told otherwise:
/// from the pool for local use (RFC 2474), and in the same priority class as
/// unmarked traffic, so the mark changes nothing but the nat rule's choice.
pub const DEFAULT_DSCP: u8 = 7;
pub const DEFAULT_PROBE_NAME: &str = "lumen-probe.invalid";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub listen: SocketAddrV4,
    pub upstream: SocketAddrV4,
    pub rules: PathBuf,
    pub log: PathBuf,
    pub status: PathBuf,
    pub dscp: u8,
    pub max_log_bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(Config),
    Probe { addr: SocketAddrV4, name: String },
    Help,
}

pub fn parse(args: &[String]) -> Result<Command, String> {
    let (mut listen, mut upstream, mut rules, mut log, mut status) = (None, None, None, None, None);
    let (mut dscp, mut max_log_bytes) = (DEFAULT_DSCP, DEFAULT_MAX_BYTES);
    let (mut probe, mut name) = (None, None);
    let mut run_flags = false;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        if flag == "-h" || flag == "--help" {
            return Ok(Command::Help);
        }
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        run_flags |= !matches!(flag.as_str(), "--probe" | "--name");
        match flag.as_str() {
            "--listen" => listen = Some(addr(value)?),
            "--upstream" => upstream = Some(addr(value)?),
            "--rules" => rules = Some(PathBuf::from(value)),
            "--log" => log = Some(PathBuf::from(value)),
            "--status" => status = Some(PathBuf::from(value)),
            "--mode" => match value.as_str() {
                "log" => {}
                "block" => {
                    return Err("--mode block is stage G2 and not built; G1 only logs".to_owned());
                }
                other => return Err(format!("--mode must be log, not {other}")),
            },
            "--dscp" => {
                dscp = value
                    .parse()
                    .ok()
                    .filter(|d| *d <= 63)
                    .ok_or_else(|| format!("--dscp must be 0 to 63, not {value}"))?;
            }
            "--max-log-bytes" => {
                max_log_bytes = value.parse().ok().filter(|n| *n > 0).ok_or_else(|| {
                    format!("--max-log-bytes must be a positive number, not {value}")
                })?;
            }
            "--probe" => probe = Some(addr(value)?),
            "--name" => name = Some(probe_name(value)?),
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if let Some(addr) = probe {
        if run_flags {
            return Err("--probe takes only --name".to_owned());
        }
        let name = name.unwrap_or_else(|| DEFAULT_PROBE_NAME.to_owned());
        return Ok(Command::Probe { addr, name });
    }
    if name.is_some() {
        return Err("--name goes with --probe".to_owned());
    }
    let missing = |what: &str| format!("{what} is required");
    let config = Config {
        listen: listen.ok_or_else(|| missing("--listen"))?,
        upstream: upstream.ok_or_else(|| missing("--upstream"))?,
        rules: rules.ok_or_else(|| missing("--rules"))?,
        log: log.ok_or_else(|| missing("--log"))?,
        status: status.ok_or_else(|| missing("--status"))?,
        dscp,
        max_log_bytes,
    };
    if config.upstream == config.listen {
        return Err("--upstream is --listen: Guard would ask itself".to_owned());
    }
    Ok(Command::Run(config))
}

/// IPv4 only: the TV's nat rule and its DNS are IPv4, and IPv6 is off.
fn addr(value: &str) -> Result<SocketAddrV4, String> {
    value
        .parse()
        .map_err(|_| format!("not an IPv4 ADDR:PORT: {value}"))
}

/// A plain host name, since it is written into a query as it is.
fn probe_name(value: &str) -> Result<String, String> {
    let plain = value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    });
    if plain && value.len() <= 253 {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(format!("--name must be a plain host name, not {value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    const TV: &[&str] = &[
        "--listen",
        "127.0.0.2:53",
        "--upstream",
        "192.0.2.1:53",
        "--rules",
        "/var/lib/lumen/guard/rules",
        "--log",
        "/var/lib/lumen/guard",
        "--status",
        "/var/lib/lumen/guard/status",
    ];

    #[test]
    fn parses_the_hooks_command_line() {
        let Ok(Command::Run(config)) = parse(&args(TV)) else {
            panic!("not a run");
        };
        assert_eq!(
            config.listen,
            SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 2), 53)
        );
        assert_eq!(
            config.upstream,
            SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 53)
        );
        assert_eq!(config.status, PathBuf::from("/var/lib/lumen/guard/status"));
        assert_eq!(config.dscp, 7);
        assert_eq!(config.max_log_bytes, 4 << 20);

        let mut more = args(TV);
        more.extend(args(&[
            "--mode",
            "log",
            "--dscp",
            "0",
            "--max-log-bytes",
            "1000",
        ]));
        let Ok(Command::Run(config)) = parse(&more) else {
            panic!("not a run");
        };
        assert_eq!((config.dscp, config.max_log_bytes), (0, 1000));
    }

    #[test]
    fn refuses_block_mode_and_bad_values() {
        for extra in [
            &["--mode", "block"][..],
            &["--mode", "loud"],
            &["--dscp", "64"],
            &["--dscp", "-1"],
            &["--max-log-bytes", "0"],
            &["--frobnicate", "1"],
            &["--dscp"],
        ] {
            let mut list = args(TV);
            list.extend(args(extra));
            assert!(parse(&list).is_err(), "{extra:?}");
        }
        assert!(
            parse(&args(&TV[..8]))
                .unwrap_err()
                .contains("--status is required")
        );
        assert!(
            parse(&args(&["--listen", "[::1]:53"]))
                .unwrap_err()
                .contains("IPv4")
        );
        let looped = args(&[
            "--listen",
            "127.0.0.2:53",
            "--upstream",
            "127.0.0.2:53",
            "--rules",
            "r",
            "--log",
            "l",
            "--status",
            "s",
        ]);
        assert!(parse(&looped).unwrap_err().contains("ask itself"));
    }

    #[test]
    fn parses_a_probe() {
        assert_eq!(
            parse(&args(&["--probe", "127.0.0.2:53"])),
            Ok(Command::Probe {
                addr: SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 2), 53),
                name: "lumen-probe.invalid".to_owned()
            })
        );
        let Ok(Command::Probe { name, .. }) = parse(&args(&[
            "--probe",
            "127.0.0.1:53",
            "--name",
            "Lumen-Check-42-1793318400.invalid",
        ])) else {
            panic!("not a probe");
        };
        assert_eq!(name, "lumen-check-42-1793318400.invalid");
        assert!(parse(&args(&["--probe", "127.0.0.1:53", "--name", "a b.invalid"])).is_err());
        assert!(parse(&args(&["--probe", "127.0.0.1:53", "--name", "a..invalid"])).is_err());
        assert!(parse(&args(&["--probe", "127.0.0.1:53", "--dscp", "7"])).is_err());
        assert!(parse(&args(&["--name", "x.invalid"])).is_err());
        assert_eq!(parse(&args(&["--help"])), Ok(Command::Help));
    }
}
