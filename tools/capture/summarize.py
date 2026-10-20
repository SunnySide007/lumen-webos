#!/usr/bin/env python3
"""Summarises an on-TV capture (tools/capture.sh) as a markdown host table.

    tools/dev.sh python3 -I tools/capture/summarize.py tools/out/capture/*.pcap*

Reads classic pcap files (link types Linux SLL 113, SLL2 276, Ethernet 1 and
raw IP 101) and reports, for each name the TV looked up (DNS over UDP or TCP)
or sent in a TLS ClientHello (SNI): how often, first and last seen, how it was
seen, and whether profiles/glasshouse lists it as allowed or blocked. A second
table lists the addresses the TV connected to (TCP SYN, QUIC) that no DNS
answer in the capture returned.

DNS counts are queries by apps. The TV's apps ask the connman proxy on 127.0.0.1,
which asks the router only on a cache miss, so when the capture holds
loopback DNS only those queries are counted; tcpdump's second copy of each
loopback packet (the outgoing one) is skipped. Times are UTC.
"""

import argparse
import ipaddress
import os
import re
import struct
import sys
from datetime import datetime, timezone

LINKTYPE_ETHERNET = 1
LINKTYPE_RAW = 101
LINKTYPE_LINUX_SLL = 113
LINKTYPE_LINUX_SLL2 = 276

PACKET_HOST = 0
PACKET_OUTGOING = 4

ETH_IPV4 = 0x0800
ETH_IPV6 = 0x86DD
ETH_VLAN = (0x8100, 0x88A8)

DNS_A, DNS_CNAME, DNS_AAAA = 1, 5, 28

SINK_ADDRS = ('0.0.0.0', '127.0.0.1', '::', '::1')
HOST_NAME = re.compile(r'^[a-z0-9_]([a-z0-9_-]*[a-z0-9_])?(\.[a-z0-9_]([a-z0-9_-]*[a-z0-9_])?)+$')

# Home networks only: ipaddress's is_private also covers documentation ranges.
LAN_NETS = [ipaddress.ip_network(n) for n in (
    '10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16', '169.254.0.0/16',
    'fc00::/7', 'fe80::/10')]

PROFILE = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                       '..', '..', 'profiles', 'glasshouse')


class PcapError(Exception):
    pass


def read_pcap(path):
    """Yields (timestamp, linktype, frame) for each record in a pcap file.

    A record cut short at the end (tcpdump was killed) ends the file quietly.
    """
    with open(path, 'rb') as f:
        header = f.read(24)
        if len(header) < 24:
            raise PcapError(f'{path}: too short for a pcap file')
        magic = header[:4]
        if magic == b'\x0a\x0d\x0d\x0a':
            raise PcapError(f'{path}: pcapng is not supported; capture with tcpdump -w')
        formats = {
            b'\xd4\xc3\xb2\xa1': ('<', 1e-6), b'\xa1\xb2\xc3\xd4': ('>', 1e-6),
            b'\x4d\x3c\xb2\xa1': ('<', 1e-9), b'\xa1\xb2\x3c\x4d': ('>', 1e-9),
        }
        if magic not in formats:
            raise PcapError(f'{path}: not a pcap file')
        endian, scale = formats[magic]
        linktype = struct.unpack(endian + 'I', header[20:24])[0] & 0x0FFFFFFF
        record = struct.Struct(endian + 'IIII')
        while True:
            rh = f.read(16)
            if len(rh) < 16:
                return
            sec, frac, caplen, _ = record.unpack(rh)
            if caplen > 0x40000:
                raise PcapError(f'{path}: corrupt record length {caplen}')
            frame = f.read(caplen)
            if len(frame) < caplen:
                return
            yield sec + frac * scale, linktype, frame


def decode_link(linktype, frame):
    """Returns (pkttype or None, ethertype, network-layer bytes) or None."""
    if linktype == LINKTYPE_LINUX_SLL:
        if len(frame) < 16:
            return None
        pkttype = struct.unpack('>H', frame[0:2])[0]
        return pkttype, struct.unpack('>H', frame[14:16])[0], frame[16:]
    if linktype == LINKTYPE_LINUX_SLL2:
        if len(frame) < 20:
            return None
        return frame[10], struct.unpack('>H', frame[0:2])[0], frame[20:]
    if linktype == LINKTYPE_ETHERNET:
        if len(frame) < 14:
            return None
        ethertype, off = struct.unpack('>H', frame[12:14])[0], 14
        while ethertype in ETH_VLAN and len(frame) >= off + 4:
            ethertype, off = struct.unpack('>H', frame[off + 2:off + 4])[0], off + 4
        return None, ethertype, frame[off:]
    if linktype == LINKTYPE_RAW:
        if not frame:
            return None
        version = frame[0] >> 4
        return None, ETH_IPV4 if version == 4 else ETH_IPV6, frame
    return None


def decode_ip(ethertype, data):
    """Returns (src, dst, protocol, transport bytes) or None.

    Non-first IPv4 fragments are dropped; IPv6 skips the common extension
    headers.
    """
    if ethertype == ETH_IPV4 and len(data) >= 20 and data[0] >> 4 == 4:
        ihl = (data[0] & 0x0F) * 4
        total = struct.unpack('>H', data[2:4])[0]
        if struct.unpack('>H', data[6:8])[0] & 0x1FFF:
            return None
        end = min(len(data), total) if total >= ihl else len(data)
        return (ipaddress.IPv4Address(data[12:16]), ipaddress.IPv4Address(data[16:20]),
                data[9], data[ihl:end])
    if ethertype == ETH_IPV6 and len(data) >= 40 and data[0] >> 4 == 6:
        nxt, off = data[6], 40
        while nxt in (0, 43, 60) and len(data) >= off + 8:
            nxt, off = data[off], off + (data[off + 1] + 1) * 8
        if nxt == 44:
            return None
        return (ipaddress.IPv6Address(data[8:24]), ipaddress.IPv6Address(data[24:40]),
                nxt, data[off:])
    return None


def read_name(msg, off):
    """Reads a possibly compressed DNS name; returns (name, offset after it)."""
    labels, end, jumps = [], None, 0
    while True:
        if off >= len(msg):
            raise ValueError('name runs past the message')
        length = msg[off]
        if length & 0xC0 == 0xC0:
            if off + 1 >= len(msg):
                raise ValueError('cut pointer')
            if end is None:
                end = off + 2
            jumps += 1
            if jumps > 32:
                raise ValueError('pointer loop')
            off = ((length & 0x3F) << 8) | msg[off + 1]
            continue
        if length & 0xC0:
            raise ValueError('bad label type')
        off += 1
        if length == 0:
            break
        labels.append(msg[off:off + length].decode('ascii', 'replace'))
        off += length
    return '.'.join(labels).lower(), (end if end is not None else off)


def parse_dns(msg):
    """Returns (is_response, [(qname, qtype)], [(owner, type, value)]) or None.

    Answer values are an address for A/AAAA and a name for CNAME.
    """
    if len(msg) < 12:
        return None
    _, flags, qd, an = struct.unpack('>HHHH', msg[:8])
    try:
        off, questions, answers = 12, [], []
        for _ in range(qd):
            name, off = read_name(msg, off)
            if off + 4 > len(msg):
                return None
            questions.append((name, struct.unpack('>H', msg[off:off + 2])[0]))
            off += 4
        for _ in range(an):
            owner, off = read_name(msg, off)
            if off + 10 > len(msg):
                break
            rtype, _, _, rdlen = struct.unpack('>HHIH', msg[off:off + 10])
            off += 10
            rdata = msg[off:off + rdlen]
            if rtype == DNS_A and rdlen == 4:
                answers.append((owner, rtype, ipaddress.IPv4Address(rdata)))
            elif rtype == DNS_AAAA and rdlen == 16:
                answers.append((owner, rtype, ipaddress.IPv6Address(rdata)))
            elif rtype == DNS_CNAME:
                answers.append((owner, rtype, read_name(msg, off)[0]))
            off += rdlen
    except ValueError:
        return None
    return bool(flags & 0x8000), questions, answers


def parse_sni(payload):
    """Returns the server name from a TLS ClientHello, or None.

    Works on a ClientHello cut short after its server_name extension.
    """
    if len(payload) < 43 or payload[0] != 0x16 or payload[5] != 0x01:
        return None
    off = 9 + 2 + 32
    try:
        off += 1 + payload[off]
        off += 2 + struct.unpack('>H', payload[off:off + 2])[0]
        off += 1 + payload[off]
        ext_end = off + 2 + struct.unpack('>H', payload[off:off + 2])[0]
        off += 2
        while off + 4 <= min(ext_end, len(payload)):
            etype, elen = struct.unpack('>HH', payload[off:off + 4])
            off += 4
            if etype == 0:
                # server_name_list length, then entries of type, length, name.
                p = off + 2
                while p + 3 <= off + elen and p + 3 <= len(payload):
                    ntype, nlen = payload[p], struct.unpack('>H', payload[p + 1:p + 3])[0]
                    name = payload[p + 3:p + 3 + nlen]
                    if ntype == 0 and len(name) == nlen:
                        return name.decode('ascii', 'replace').lower().rstrip('.')
                    p += 3 + nlen
                return None
            off += elen
    except (IndexError, struct.error):
        return None
    return None


class Seen:
    """Count and first/last time for one key."""

    __slots__ = ('count', 'first', 'last')

    def __init__(self):
        self.count, self.first, self.last = 0, None, None

    def add(self, ts, count=1):
        self.count += count
        self.first = ts if self.first is None else min(self.first, ts)
        self.last = ts if self.last is None else max(self.last, ts)


class Summary:
    def __init__(self):
        self.packets = 0
        self.files = 0
        self.linktypes = set()
        self.start = self.end = None
        self.dns_local = {}       # name -> Seen, queries to a loopback resolver
        self.dns_remote = {}      # name -> Seen, queries to any other resolver
        self.sni = {}             # name -> Seen
        self.resolved = {}        # address -> set of names that returned it
        self.contacts = {}        # address -> Seen
        self.contact_ports = {}   # address -> set of 'tcp/443' strings
        self.contact_sni = {}     # address -> set of SNI names
        self.inbound = {}         # (source, port) -> Seen
        self.flows = set()        # (src, sport, dst, dport) already counted

    def add_file(self, path):
        self.files += 1
        for ts, linktype, frame in read_pcap(path):
            self.linktypes.add(linktype)
            self.add_frame(ts, linktype, frame)

    def add_frame(self, ts, linktype, frame):
        self.packets += 1
        self.start = ts if self.start is None else min(self.start, ts)
        self.end = ts if self.end is None else max(self.end, ts)
        link = decode_link(linktype, frame)
        if link is None:
            return
        pkttype, ethertype, data = link
        ip = decode_ip(ethertype, data)
        if ip is None:
            return
        src, dst, proto, seg = ip
        # -i any sees each loopback packet twice: going out, then coming in.
        if pkttype == PACKET_OUTGOING and dst.is_loopback:
            return
        if proto == 17 and len(seg) >= 8:
            sport, dport = struct.unpack('>HH', seg[:4])
            if 53 in (sport, dport):
                self.add_dns(ts, dst, seg[8:])
            elif dport == 443 and len(seg) > 8 and seg[8] & 0xC0 == 0xC0:
                self.add_contact(ts, (src, sport, dst, 'udp/443'))
        elif proto == 6 and len(seg) >= 20:
            sport, dport = struct.unpack('>HH', seg[:4])
            flags = seg[13]
            payload = seg[(seg[12] >> 4) * 4:]
            if flags & 0x02 and not flags & 0x10:
                if pkttype == PACKET_HOST:
                    if not dst.is_loopback:
                        self.inbound.setdefault((src, dport), Seen()).add(ts)
                else:
                    self.add_contact(ts, (src, sport, dst, f'tcp/{dport}'))
            if 53 in (sport, dport) and payload:
                off = 0
                while off + 2 <= len(payload):
                    n = struct.unpack('>H', payload[off:off + 2])[0]
                    if n == 0 or off + 2 + n > len(payload):
                        break
                    self.add_dns(ts, dst, payload[off + 2:off + 2 + n])
                    off += 2 + n
            elif payload:
                name = parse_sni(payload)
                if name:
                    self.sni.setdefault(name, Seen()).add(ts)
                    if not dst.is_loopback:
                        self.contact_sni.setdefault(dst, set()).add(name)

    def add_dns(self, ts, dst, msg):
        parsed = parse_dns(msg)
        if parsed is None:
            return
        is_response, questions, answers = parsed
        if not is_response:
            table = self.dns_local if dst.is_loopback else self.dns_remote
            for name, _ in questions:
                if name:
                    table.setdefault(name, Seen()).add(ts)
            return
        # Each address is credited to the asked name and every name on the
        # CNAME chain, so blocking any of them is visible against it.
        asked = {name for name, _ in questions}
        chain = asked | {owner for owner, _, _ in answers}
        for _, rtype, value in answers:
            if rtype in (DNS_A, DNS_AAAA):
                self.resolved.setdefault(value, set()).update(chain)

    def add_contact(self, ts, flow):
        """Counts one connection per flow: SYN retransmits and the later
        QUIC long-header packets of a connection are not new ones."""
        _, _, dst, port = flow
        if dst.is_loopback or dst.is_multicast or dst.is_unspecified:
            return
        seen = self.contacts.setdefault(dst, Seen())
        seen.add(ts, 0 if flow in self.flows else 1)
        self.flows.add(flow)
        self.contact_ports.setdefault(dst, set()).add(port)

    def lookups(self):
        """Per-name app lookups (see the module docstring)."""
        return self.dns_local if self.dns_local else self.dns_remote


class Lists:
    """profiles/glasshouse rules: allowlist suffixes and block entries."""

    def __init__(self, allow=(), exact=(), block=(), candidate=()):
        self.allow, self.exact = set(allow), set(exact)
        self.block, self.candidate = set(block), set(candidate)

    @classmethod
    def load(cls, allowlist, required, hosts):
        allow, exact, block, candidate = set(), set(), set(), set()
        for line in read_lines(allowlist):
            rule = line.split('#', 1)[0].strip().lower()
            if rule.startswith('='):
                exact.add(rule[1:])
            elif rule:
                allow.add(rule)
        for line in read_lines(required):
            name = line.split('#', 1)[0].strip().lower()
            if name:
                block.add(name)
        for line in read_lines(hosts):
            text = line.strip()
            commented = text.startswith('#')
            fields = text.lstrip('#').split('#', 1)[0].split()
            if len(fields) >= 2 and fields[0] in SINK_ADDRS:
                # Host names only: prose in a comment ("0.0.0.0 lines, at
                # most ...") is not a candidate.
                names = [f.lower() for f in fields[1:]]
                if all(HOST_NAME.match(n) for n in names):
                    (candidate if commented else block).update(names)
        return cls(allow, exact, block, candidate)

    def allowed(self, name):
        if name in self.exact:
            return True
        parts = name.split('.')
        return any('.'.join(parts[i:]) in self.allow for i in range(len(parts)))

    def status(self, name):
        allowed = self.allowed(name)
        if name in self.block:
            return 'CONFLICT: allow + block' if allowed else 'block'
        if name in self.candidate:
            return 'CONFLICT: allow + candidate' if allowed else 'candidate'
        return 'allow' if allowed else ''


def read_lines(path):
    if not path or not os.path.exists(path):
        return []
    with open(path, encoding='utf-8') as f:
        return f.read().splitlines()


def cell(text):
    """Names come off the wire: keep them to one markdown table cell."""
    return ''.join(c if c.isprintable() else '?' for c in text).replace('|', '\\|')


def when(ts):
    if ts is None:
        return ''
    return datetime.fromtimestamp(ts, timezone.utc).strftime('%Y-%m-%d %H:%M:%SZ')


def render(summary, lists):
    out = ['# Capture summary', '']
    out.append(f'{summary.files} file(s), {summary.packets} packets, '
               f'{when(summary.start)} to {when(summary.end)}; '
               f'link types {", ".join(str(t) for t in sorted(summary.linktypes)) or "none"}.')
    source = 'apps to the local resolver' if summary.dns_local else 'all DNS queries'
    out.append(f'DNS queries count {source} (A and AAAA separately). List: allowlist.txt, '
               'required-blocks.txt and b9-aggressive.hosts (candidates are its commented '
               'entries).')
    out += ['', '## Hosts', '',
            '| Host | DNS queries | TLS | First seen | Last seen | Seen as | List |',
            '|---|---:|---:|---|---|---|---|']
    lookups = summary.lookups()
    names = set(lookups) | set(summary.sni) | set(summary.dns_remote)
    rows = []
    for name in names:
        dns = lookups.get(name) or summary.dns_remote.get(name)
        dns_count = lookups[name].count if name in lookups else 0
        tls = summary.sni.get(name)
        seen = [s for s in (dns, tls) if s is not None]
        how = '+'.join(k for k, s in (('dns', dns), ('sni', tls)) if s is not None)
        rows.append((-(dns_count + (tls.count if tls else 0)), name, dns_count,
                     tls.count if tls else 0, min(s.first for s in seen),
                     max(s.last for s in seen), how, lists.status(name)))
    for _, name, dns_count, tls_count, first, last, how, status in sorted(rows):
        out.append(f'| {cell(name)} | {dns_count} | {tls_count} | {when(first)} | {when(last)} '
                   f'| {how} | {status} |')

    out += ['', '## Addresses contacted without a DNS lookup', '',
            'TCP SYNs and QUIC packets to addresses no DNS answer in the capture returned. '
            'LAN addresses are local devices (the Plex server, a phone casting).', '',
            '| Address | Ports | Connections | First seen | Last seen | TLS names | Scope |',
            '|---|---|---:|---|---|---|---|']
    ip_only = [(a, s) for a, s in summary.contacts.items() if a not in summary.resolved]
    for addr, seen in sorted(ip_only, key=lambda x: (-x[1].count, str(x[0]))):
        ports = ', '.join(sorted(summary.contact_ports[addr]))
        names = ', '.join(cell(n) for n in sorted(summary.contact_sni.get(addr, ())))
        scope = 'lan' if any(addr in net for net in LAN_NETS) else 'internet'
        out.append(f'| {addr} | {ports} | {seen.count} | {when(seen.first)} '
                   f'| {when(seen.last)} | {names} | {scope} |')
    if not ip_only:
        out.append('| none | | | | | | |')

    if summary.inbound:
        out += ['', '## Connections to the TV', '',
                '| From | TV port | Connections | First seen | Last seen |',
                '|---|---:|---:|---|---|']
        for (src, port), seen in sorted(summary.inbound.items(),
                                        key=lambda x: (-x[1].count, str(x[0][0]), x[0][1])):
            out.append(f'| {src} | {port} | {seen.count} | {when(seen.first)} '
                       f'| {when(seen.last)} |')
    return '\n'.join(out) + '\n'


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n', 1)[0])
    parser.add_argument('pcaps', nargs='+', help='pcap files from tools/capture.sh fetch')
    parser.add_argument('--allowlist', default=os.path.join(PROFILE, 'allowlist.txt'))
    parser.add_argument('--required', default=os.path.join(PROFILE, 'required-blocks.txt'))
    parser.add_argument('--hosts', default=os.path.join(PROFILE, 'b9-aggressive.hosts'))
    args = parser.parse_args(argv)
    summary = Summary()
    for path in args.pcaps:
        try:
            summary.add_file(path)
        except (OSError, PcapError) as e:
            print(f'summarize.py: {e}', file=sys.stderr)
            return 1
    sys.stdout.write(render(summary, Lists.load(args.allowlist, args.required, args.hosts)))
    return 0


if __name__ == '__main__':
    sys.exit(main())
