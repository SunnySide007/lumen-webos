//! The command line, parsed by hand; anything unknown is refused. Only the
//! owner (by hand, for now) and later a boot hook start lumen-remote.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;

pub const USAGE: &str = "\
usage: lumen-remote --listen ADDR:PORT --allow NET/BITS [--allow ...] --name HOST [--name ...]
                    [--cert FILE] [--key FILE] [--web DIR] [--ctl FILE] [--ctl-arg ARG ...]
                    [--user UID:GID] [--jail DIR] [--log FILE] [--no-drop]

Serves Lumen Remote's web front-end and its WebSocket over HTTPS on one
IPv4 port, to paired devices only, in front of `lumen-ctl serve --link web`,
which it starts as its child before it drops root.

--listen   the address and port, e.g. 192.168.86.20:8443 (IPv4 only)
--allow    the home network: connections from elsewhere are closed unread
           (an INPUT rule should refuse them first)
--name     a name the TV is reached by, as in its certificate: its address
           or a DNS name. Host and Origin must match one, with the port
--cert     the server's certificate chain, PEM (/var/lib/lumen/remote/tls/server.crt)
--key      its key, PEM PKCS#8 (/var/lib/lumen/remote/tls/server.key)
--web      the built front-end, remote/ui/dist (/var/lib/lumen/remote/web)
--ctl      lumen-ctl (/var/lib/lumen/remote/lumen-ctl); --ctl-arg adds an
           argument after `serve --link web` (tests: --bus fake, --state)
--user     who to become (61101:61101, a uid nothing else on the TV uses)
--jail     an empty folder to change root into (/var/lib/lumen/remote/empty,
           made if missing)
--log      what it did, never a token, a code or typed text
           (/var/lib/lumen/remote/lumen-remote.log)
--no-drop  for tests on a PC, as a user: no chroot, no change of user.
           Refused as root

Everything is read before the drop: the certificate, the key and every
file of the front-end, which is served from memory.
";

pub const DIR: &str = "/var/lib/lumen/remote";
/// The user lumen-remote becomes: no file on the TV is theirs.
pub const USER: (u32, u32) = (61_101, 61_101);

/// An IPv4 network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Net {
    pub addr: u32,
    pub bits: u8,
}

impl Net {
    pub fn parse(s: &str) -> Result<Self, String> {
        let bad = || format!("--allow wants NET/BITS (192.168.86.0/24), not {s}");
        let (ip, bits) = s.split_once('/').ok_or_else(bad)?;
        let ip: Ipv4Addr = ip.parse().map_err(|_| bad())?;
        let bits: u8 = bits.parse().ok().filter(|b| *b <= 32).ok_or_else(bad)?;
        let addr = u32::from(ip) & mask(bits);
        Ok(Self { addr, bits })
    }

    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        u32::from(ip) & mask(self.bits) == self.addr
    }
}

fn mask(bits: u8) -> u32 {
    if bits == 0 {
        0
    } else {
        u32::MAX << (32 - bits)
    }
}

/// A name for the TV: an IPv4 address or a DNS name, in lower case.
pub fn check_name(s: &str) -> Result<String, String> {
    let lower = s.to_ascii_lowercase();
    let ok = !lower.is_empty()
        && lower.len() <= 253
        && lower.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    if ok {
        Ok(lower)
    } else {
        Err(format!("--name wants an address or a DNS name, not {s}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub listen: SocketAddrV4,
    pub allow: Vec<Net>,
    pub names: Vec<String>,
    pub cert: PathBuf,
    pub key: PathBuf,
    pub web: PathBuf,
    pub ctl: PathBuf,
    pub ctl_args: Vec<String>,
    pub user: (u32, u32),
    pub jail: PathBuf,
    pub log: PathBuf,
    pub no_drop: bool,
}

pub enum Parsed {
    Run(Box<Args>),
    Help,
}

pub fn parse(argv: &[String]) -> Result<Parsed, String> {
    let (mut listen, mut allow, mut names) = (None, Vec::new(), Vec::new());
    let path = |name: &str| PathBuf::from(format!("{DIR}/{name}"));
    let mut a = Args {
        listen: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
        allow: Vec::new(),
        names: Vec::new(),
        cert: path("tls/server.crt"),
        key: path("tls/server.key"),
        web: path("web"),
        ctl: path("lumen-ctl"),
        ctl_args: Vec::new(),
        user: USER,
        jail: path("empty"),
        log: path("lumen-remote.log"),
        no_drop: false,
    };
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--no-drop" => {
                a.no_drop = true;
                continue;
            }
            _ => {}
        }
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--listen" => {
                listen = Some(
                    value
                        .parse::<SocketAddrV4>()
                        .map_err(|_| format!("--listen wants an IPv4 ADDR:PORT, not {value}"))?,
                )
            }
            "--allow" => allow.push(Net::parse(value)?),
            "--name" => names.push(check_name(value)?),
            "--cert" => a.cert = PathBuf::from(value),
            "--key" => a.key = PathBuf::from(value),
            "--web" => a.web = PathBuf::from(value),
            "--ctl" => a.ctl = PathBuf::from(value),
            "--ctl-arg" => a.ctl_args.push(value.clone()),
            "--jail" => a.jail = PathBuf::from(value),
            "--log" => a.log = PathBuf::from(value),
            "--user" => {
                let bad = || format!("--user wants UID:GID, not {value}");
                let (u, g) = value.split_once(':').ok_or_else(bad)?;
                let (u, g): (u32, u32) =
                    (u.parse().map_err(|_| bad())?, g.parse().map_err(|_| bad())?);
                if u == 0 || g == 0 {
                    return Err("--user must not be root".to_owned());
                }
                a.user = (u, g);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    a.listen = listen.ok_or("--listen is needed")?;
    if allow.is_empty() {
        return Err("--allow is needed: the home network".to_owned());
    }
    if names.is_empty() {
        return Err("--name is needed: the TV's address or name, as in its certificate".to_owned());
    }
    a.allow = allow;
    a.names = names;
    Ok(Parsed::Run(Box::new(a)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        let v: Vec<String> = list.iter().map(|s| (*s).to_owned()).collect();
        match parse(&v)? {
            Parsed::Run(a) => Ok(*a),
            Parsed::Help => Err("help".to_owned()),
        }
    }

    #[test]
    fn needs_the_port_the_network_and_a_name() {
        let a = args(&[
            "--listen",
            "192.168.86.20:8443",
            "--allow",
            "192.168.86.7/24",
            "--name",
            "192.168.86.20",
            "--name",
            "Lumen-TV.home.arpa",
        ])
        .unwrap();
        assert_eq!(a.listen.port(), 8443);
        assert_eq!(
            a.allow,
            [Net {
                addr: 0xC0A8_5600,
                bits: 24
            }]
        );
        assert_eq!(a.names, ["192.168.86.20", "lumen-tv.home.arpa"]);
        assert_eq!(a.user, (61_101, 61_101));
        assert_eq!(a.ctl, PathBuf::from("/var/lib/lumen/remote/lumen-ctl"));
        assert!(!a.no_drop);
        for bad in [
            &["--allow", "10.0.0.0/8", "--name", "x"][..],
            &["--listen", "1.2.3.4:5", "--name", "x"],
            &["--listen", "1.2.3.4:5", "--allow", "10.0.0.0/8"],
            &[
                "--listen",
                "[::1]:5",
                "--allow",
                "10.0.0.0/8",
                "--name",
                "x",
            ],
            &[
                "--listen",
                "1.2.3.4:5",
                "--allow",
                "10.0.0.0/33",
                "--name",
                "x",
            ],
            &[
                "--listen",
                "1.2.3.4:5",
                "--allow",
                "10.0.0.0",
                "--name",
                "x",
            ],
            &[
                "--listen",
                "1.2.3.4:5",
                "--allow",
                "10.0.0.0/8",
                "--name",
                "a b",
            ],
            &[
                "--listen",
                "1.2.3.4:5",
                "--allow",
                "10.0.0.0/8",
                "--name",
                "x",
                "--user",
                "0:0",
            ],
            &[
                "--listen",
                "1.2.3.4:5",
                "--allow",
                "10.0.0.0/8",
                "--name",
                "x",
                "--shell",
                "sh",
            ],
            &["--listen"],
        ] {
            assert!(args(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn networks_match_by_their_bits() {
        let lan = Net::parse("192.168.86.0/24").unwrap();
        assert!(lan.contains("192.168.86.200".parse().unwrap()));
        assert!(!lan.contains("192.168.87.1".parse().unwrap()));
        let one = Net::parse("10.0.0.5/32").unwrap();
        assert!(one.contains("10.0.0.5".parse().unwrap()));
        assert!(!one.contains("10.0.0.6".parse().unwrap()));
        assert!(
            Net::parse("0.0.0.0/0")
                .unwrap()
                .contains("8.8.8.8".parse().unwrap())
        );
    }
}
