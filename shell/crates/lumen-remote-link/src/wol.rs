//! Wake-on-LAN: the only way to turn the TV on from the PC (remote.md, "Power
//! on"), since its services, lumen-ctl's door included, stop when it is off.

use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};

use crate::settings::Mac;

/// Six 0xff bytes, then the MAC sixteen times.
pub fn magic_packet(mac: Mac) -> [u8; 102] {
    let mut p = [0xffu8; 102];
    for chunk in p[6..].as_chunks_mut::<6>().0 {
        chunk.copy_from_slice(&mac.0);
    }
    p
}

/// Where the packet goes: the limited broadcast and, when the TV's address
/// is an IPv4 address, its /24's broadcast too. Windows sends the limited
/// broadcast out of one interface only, and home networks (the Nest Wifi
/// Pro's 192.168.86.0/24 among them) are /24s.
pub fn targets(host: Option<&str>) -> Vec<Ipv4Addr> {
    let mut t = vec![Ipv4Addr::BROADCAST];
    if let Some(ip) = host.and_then(|h| h.parse::<Ipv4Addr>().ok()) {
        let [a, b, c, _] = ip.octets();
        let directed = Ipv4Addr::new(a, b, c, 255);
        if !ip.is_loopback() && directed != Ipv4Addr::BROADCAST {
            t.push(directed);
        }
    }
    t
}

/// Sends the packet to UDP port 9 of each target, twice, since Wi-Fi drops
/// broadcasts more often than it drops anything else. Returns how many were
/// sent; an error only when none was.
pub fn wake(mac: Mac, targets: &[Ipv4Addr]) -> io::Result<usize> {
    wake_to(mac, targets, 9)
}

fn wake_to(mac: Mac, targets: &[Ipv4Addr], port: u16) -> io::Result<usize> {
    let packet = magic_packet(mac);
    let sock = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    sock.set_broadcast(true)?;
    let mut sent = 0;
    let mut last = None;
    for _ in 0..2 {
        for ip in targets {
            match sock.send_to(&packet, SocketAddrV4::new(*ip, port)) {
                Ok(_) => sent += 1,
                Err(e) => last = Some(e),
            }
        }
    }
    match (sent, last) {
        (0, Some(e)) => Err(e),
        _ => Ok(sent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_packet_is_ff_then_sixteen_macs() {
        let mac = Mac([1, 2, 3, 4, 5, 6]);
        let p = magic_packet(mac);
        assert_eq!(&p[..6], &[0xff; 6]);
        for n in 0..16 {
            assert_eq!(&p[6 + n * 6..12 + n * 6], &mac.0, "copy {n}");
        }
    }

    #[test]
    fn broadcasts_on_the_tvs_24_too() {
        assert_eq!(
            targets(Some("192.168.86.20")),
            [Ipv4Addr::BROADCAST, Ipv4Addr::new(192, 168, 86, 255)]
        );
        assert_eq!(targets(Some("tv.lan")), [Ipv4Addr::BROADCAST]);
        assert_eq!(targets(Some("127.0.0.1")), [Ipv4Addr::BROADCAST]);
        assert_eq!(targets(None), [Ipv4Addr::BROADCAST]);
    }

    #[test]
    fn sends_the_packet_where_it_is_told() {
        // A listener on loopback stands in for the TV's Wi-Fi.
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let port = rx.local_addr().unwrap().port();
        let mac = Mac([0xa8, 0x23, 0xfe, 1, 2, 3]);
        assert_eq!(wake_to(mac, &[Ipv4Addr::LOCALHOST], port).unwrap(), 2);
        let mut buf = [0u8; 200];
        for _ in 0..2 {
            let (n, _) = rx.recv_from(&mut buf).unwrap();
            assert_eq!(&buf[..n], &magic_packet(mac)[..]);
        }
    }
}
