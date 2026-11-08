//! The command line, parsed by hand as `lumen-shell` does. Unlike the shell,
//! Guard refuses anything it doesn't know: only Lumen's own scripts start it.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;

use crate::control::Mode;
use crate::lookups::DEFAULT_MAX_BYTES;

pub const USAGE: &str = "\
usage: lumen-guard --listen ADDR:PORT --upstream ADDR:PORT --rules DIR --log DIR
                   --status FILE [--mode log|block] [--mode-file FILE]
                   [--owner FILE] [--learn FILE] [--dscp N] [--max-log-bytes N]
                   [--tls on|off] [--tls-file FILE] [--tls-server ADDR:PORT]...
                   [--tls-name NAME] [--tls-ca FILE] [--clock-floor UNIX]
       lumen-guard --probe ADDR:PORT [--name NAME]

Takes the DNS queries that reach --listen (UDP and TCP) and logs each to
DIR/lookups.log with the verdict of the rules in --rules: a name resolves only
when a rule allows it. In log mode (the default) every query goes to
--upstream unchanged all the same; in block mode a name no rule allows gets
0.0.0.0 or :: from Guard itself, and nothing goes upstream.

--mode-file, --owner and --learn name the owner's control files, read again
within a second of a change: the mode (`block`, `block <boot id>` for this
boot only, or `log`; without the file, --mode), the owner's own rules, and a
learning window (`<start> <end>` in unix seconds, an hour at most) during which
names no rule covers resolve. Each counts only if Guard's own user owns it and
no one else can write it. Guard never blocks while the rules would block the
store, DRM or the paid apps.

--dscp marks the packets Guard sends upstream (0 to 63, default 7; 0 marks
nothing) so the TV's nat rule can leave them alone. --max-log-bytes is the size
at which lookups.log moves to lookups.log.1 (default 4 MiB).

With TLS on, what is forwarded goes to --tls-server over DNS over TLS
(RFC 7858), checked against --tls-name (default Quad9: 9.9.9.9:853, then
149.112.112.112:853, as dns.quad9.net, against the built-in Mozilla roots, or
the one DER certificate in --tls-ca instead). --upstream then serves only as a
fallback, in clear: while the TV's clock reads before --clock-floor (unix
seconds, default 2026-11-01), so no certificate can be checked, and for a
while after the TLS server could not be reached. Each fallback is logged and
counted in the status file. --tls-file is the owner's switch, read as the mode
file is: `tls`, `tls <boot id>` for this boot only, or `clear`; without the
file, --tls says (default off).

--probe asks ADDR:PORT for NAME (default lumen-probe.invalid) and exits 0 on
any answer within 2 s, 1 otherwise.
";

/// The DSCP value Guard marks its upstream packets with unless told otherwise:
/// from the pool for local use (RFC 2474), and in the same priority class as
/// unmarked traffic, so the mark changes nothing but the nat rule's choice.
pub const DEFAULT_DSCP: u8 = 7;
pub const DEFAULT_PROBE_NAME: &str = "lumen-probe.invalid";
/// Quad9 (decision 11, P4): its two anycast addresses, which pass no client
/// subnet, and the name their certificate carries.
pub const QUAD9: [SocketAddrV4; 2] = [
    SocketAddrV4::new(Ipv4Addr::new(9, 9, 9, 9), 853),
    SocketAddrV4::new(Ipv4Addr::new(149, 112, 112, 112), 853),
];
pub const QUAD9_NAME: &str = "dns.quad9.net";
/// 2026-11-01 00:00 UTC, when G3 was built: a clock before it is wrong. The
/// B9 has no RTC and boots at 2019-01-01 until LG's time server answers.
pub const DEFAULT_CLOCK_FLOOR: u64 = 1_793_491_200;
/// At most this many TLS servers; Guard tries the next after a failure.
const MAX_TLS_SERVERS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub listen: SocketAddrV4,
    pub upstream: SocketAddrV4,
    pub rules: PathBuf,
    pub log: PathBuf,
    pub status: PathBuf,
    /// The mode while there is no mode file.
    pub mode: Mode,
    pub mode_file: Option<PathBuf>,
    pub owner: Option<PathBuf>,
    pub learn: Option<PathBuf>,
    pub dscp: u8,
    pub max_log_bytes: u64,
    /// Whether forwarding goes over TLS while there is no TLS file.
    pub tls: bool,
    pub tls_file: Option<PathBuf>,
    pub tls_servers: Vec<SocketAddrV4>,
    pub tls_name: String,
    /// One DER certificate trusted instead of the built-in roots (tests).
    pub tls_ca: Option<PathBuf>,
    pub clock_floor: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(Box<Config>),
    Probe { addr: SocketAddrV4, name: String },
    Help,
}

pub fn parse(args: &[String]) -> Result<Command, String> {
    let (mut listen, mut upstream, mut rules, mut log, mut status) = (None, None, None, None, None);
    let (mut mode, mut mode_file, mut owner, mut learn) = (Mode::Log, None, None, None);
    let (mut dscp, mut max_log_bytes) = (DEFAULT_DSCP, DEFAULT_MAX_BYTES);
    let (mut probe, mut name) = (None, None);
    let (mut tls, mut tls_file, mut tls_servers, mut tls_name, mut tls_ca) =
        (false, None, Vec::new(), None, None);
    let mut clock_floor = DEFAULT_CLOCK_FLOOR;
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
            "--mode" => {
                mode = match value.as_str() {
                    "log" => Mode::Log,
                    "block" => Mode::Block,
                    other => return Err(format!("--mode must be log or block, not {other}")),
                };
            }
            "--mode-file" => mode_file = Some(PathBuf::from(value)),
            "--owner" => owner = Some(PathBuf::from(value)),
            "--learn" => learn = Some(PathBuf::from(value)),
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
            "--tls" => {
                tls = match value.as_str() {
                    "on" => true,
                    "off" => false,
                    other => return Err(format!("--tls must be on or off, not {other}")),
                };
            }
            "--tls-file" => tls_file = Some(PathBuf::from(value)),
            "--tls-server" => {
                if tls_servers.len() == MAX_TLS_SERVERS {
                    return Err(format!("at most {MAX_TLS_SERVERS} --tls-server"));
                }
                tls_servers.push(addr(value)?);
            }
            "--tls-name" => {
                tls_name = Some(
                    probe_name(value)
                        .map_err(|_| format!("--tls-name must be a host name, not {value}"))?,
                );
            }
            "--tls-ca" => tls_ca = Some(PathBuf::from(value)),
            "--clock-floor" => {
                clock_floor = value
                    .parse()
                    .map_err(|_| format!("--clock-floor must be unix seconds, not {value}"))?;
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
        mode,
        mode_file,
        owner,
        learn,
        dscp,
        max_log_bytes,
        tls,
        tls_file,
        tls_servers: if tls_servers.is_empty() {
            QUAD9.to_vec()
        } else {
            tls_servers
        },
        tls_name: tls_name.unwrap_or_else(|| QUAD9_NAME.to_owned()),
        tls_ca,
        clock_floor,
    };
    if config.upstream == config.listen || config.tls_servers.contains(&config.listen) {
        return Err("--upstream or --tls-server is --listen: Guard would ask itself".to_owned());
    }
    Ok(Command::Run(Box::new(config)))
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
        assert_eq!(config.mode, Mode::Log);
        assert_eq!(
            (config.mode_file, config.owner, config.learn),
            (None, None, None)
        );

        let mut more = args(TV);
        more.extend(args(&[
            "--mode",
            "block",
            "--mode-file",
            "/var/lib/lumen/guard/mode",
            "--owner",
            "/var/lib/lumen/guard/owner.txt",
            "--learn",
            "/var/lib/lumen/guard/learn",
            "--dscp",
            "0",
            "--max-log-bytes",
            "1000",
        ]));
        let Ok(Command::Run(config)) = parse(&more) else {
            panic!("not a run");
        };
        assert_eq!((config.dscp, config.max_log_bytes), (0, 1000));
        assert_eq!(config.mode, Mode::Block);
        assert_eq!(
            config.owner,
            Some(PathBuf::from("/var/lib/lumen/guard/owner.txt"))
        );
        assert!(config.mode_file.is_some() && config.learn.is_some());
    }

    #[test]
    fn refuses_bad_values() {
        for extra in [
            &["--mode", "loud"][..],
            &["--learn"],
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
    fn tls_defaults_to_quad9_and_off() {
        let Ok(Command::Run(config)) = parse(&args(TV)) else {
            panic!("not a run");
        };
        assert!(!config.tls);
        assert_eq!(config.tls_servers, QUAD9.to_vec());
        assert_eq!(config.tls_servers[0].to_string(), "9.9.9.9:853");
        assert_eq!(config.tls_name, "dns.quad9.net");
        assert_eq!((config.tls_file, config.tls_ca), (None, None));
        assert_eq!(config.clock_floor, DEFAULT_CLOCK_FLOOR);

        let mut more = args(TV);
        more.extend(args(&[
            "--tls",
            "on",
            "--tls-file",
            "/var/lib/lumen/guard/tls",
            "--tls-server",
            "127.0.0.1:8853",
            "--tls-name",
            "DoT.Lumen.Test",
            "--tls-ca",
            "ca.der",
            "--clock-floor",
            "0",
        ]));
        let Ok(Command::Run(config)) = parse(&more) else {
            panic!("not a run");
        };
        assert!(config.tls);
        assert_eq!(config.tls_servers.len(), 1);
        assert_eq!(config.tls_name, "dot.lumen.test");
        assert_eq!(config.clock_floor, 0);
        assert_eq!(
            config.tls_file,
            Some(PathBuf::from("/var/lib/lumen/guard/tls"))
        );
    }

    #[test]
    fn refuses_bad_tls_values() {
        for extra in [
            &["--tls", "maybe"][..],
            &["--tls-name", "a b"],
            &["--tls-server", "dns.quad9.net:853"],
            &[
                "--tls-server",
                "1.1.1.1:853",
                "--tls-server",
                "2.2.2.2:853",
                "--tls-server",
                "3.3.3.3:853",
            ],
            &["--tls-server", "127.0.0.2:53"],
            &["--clock-floor", "yesterday"],
        ] {
            let mut list = args(TV);
            list.extend(args(extra));
            assert!(parse(&list).is_err(), "{extra:?}");
        }
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
