"""Tests for tools/capture/summarize.py: python3 -I -m unittest discover -s tools/tests

Every pcap here is built in code from synthetic addresses (192.0.2.0/24, 10/8,
198.51.100.0/24, 2001:db8::/32) and example names.
"""

import io
import ipaddress
import os
import struct
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'capture'))

import summarize  # noqa: E402
from summarize import Lists, Summary  # noqa: E402

TV = '192.0.2.10'
ROUTER = '192.0.2.1'
T0 = 1792483200  # 2026-10-20 08:00:00Z


# --- packet builders -------------------------------------------------------

def dns_name(name):
    return b''.join(bytes([len(p)]) + p.encode() for p in name.split('.')) + b'\0'


def dns_query(name, qid=1, qtype=1):
    return struct.pack('>HHHHHH', qid, 0x0100, 1, 0, 0, 0) + dns_name(name) + struct.pack('>HH', qtype, 1)


def dns_response(name, answers, qid=1):
    """answers: list of (type, value); owners are compressed pointers to the question."""
    msg = struct.pack('>HHHHHH', qid, 0x8180, 1, len(answers), 0, 0)
    msg += dns_name(name) + struct.pack('>HH', 1, 1)
    for rtype, value in answers:
        if rtype == 1:
            rdata = ipaddress.IPv4Address(value).packed
        elif rtype == 28:
            rdata = ipaddress.IPv6Address(value).packed
        else:
            rdata = dns_name(value)
        msg += b'\xc0\x0c' + struct.pack('>HHIH', rtype, 1, 60, len(rdata)) + rdata
    return msg


def client_hello(sni, cut_after_sni=False):
    sni_b = sni.encode()
    entry = b'\x00' + struct.pack('>H', len(sni_b)) + sni_b
    ext_sni = struct.pack('>HH', 0, len(entry) + 2) + struct.pack('>H', len(entry)) + entry
    ext_other = struct.pack('>HH', 0x000a, 4) + b'\x00\x02\x00\x1d'
    exts = ext_other + ext_sni + struct.pack('>HH', 0x002b, 3) + b'\x02\x03\x04'
    body = (b'\x03\x03' + b'\x11' * 32 + b'\x00' + struct.pack('>H', 2) + b'\x13\x01'
            + b'\x01\x00' + struct.pack('>H', len(exts)) + exts)
    hs = b'\x01' + struct.pack('>I', len(body))[1:] + body
    rec = b'\x16\x03\x01' + struct.pack('>H', len(hs)) + hs
    if cut_after_sni:
        rec = rec[:rec.index(sni_b) + len(sni_b)]
    return rec


def udp(sport, dport, payload):
    return struct.pack('>HHHH', sport, dport, 8 + len(payload), 0) + payload


def tcp(sport, dport, flags, payload=b''):
    return struct.pack('>HHIIBBHHH', sport, dport, 1, 0, 5 << 4, flags, 65535, 0, 0) + payload


SYN, ACK, PSH = 0x02, 0x10, 0x08


def ipv4(src, dst, proto, seg):
    return (struct.pack('>BBHHHBBH', 0x45, 0, 20 + len(seg), 0, 0, 64, proto, 0)
            + ipaddress.IPv4Address(src).packed + ipaddress.IPv4Address(dst).packed + seg)


def ipv6(src, dst, proto, seg):
    return (struct.pack('>IHBB', 6 << 28, len(seg), proto, 64)
            + ipaddress.IPv6Address(src).packed + ipaddress.IPv6Address(dst).packed + seg)


def sll(packet, pkttype=summarize.PACKET_OUTGOING):
    ethertype = 0x0800 if packet[0] >> 4 == 4 else 0x86DD
    return struct.pack('>HHH8sH', pkttype, 772, 6, b'\0' * 8, ethertype) + packet


def sll2(packet, pkttype=summarize.PACKET_OUTGOING):
    ethertype = 0x86DD if packet[0] >> 4 == 6 else 0x0800
    return struct.pack('>HHIHBB8s', ethertype, 0, 2, 1, pkttype, 6, b'\0' * 8) + packet


def ether(packet, vlan=False):
    ethertype = 0x0800 if packet[0] >> 4 == 4 else 0x86DD
    head = b'\x02' * 6 + b'\x04' * 6
    if vlan:
        head += struct.pack('>HH', 0x8100, 7)
    return head + struct.pack('>H', ethertype) + packet


def pcap(linktype, frames, magic=0xA1B2C3D4, endian='<'):
    """frames: list of (seconds offset from T0, frame bytes)."""
    out = struct.pack(endian + 'IHHiIII', magic, 2, 4, 0, 0, 262144, linktype)
    for offset, frame in frames:
        frac = 0 if magic == 0xA1B2C3D4 else 5
        out += struct.pack(endian + 'IIII', T0 + offset, frac, len(frame), len(frame)) + frame
    return out


class CaptureFiles(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)

    def write(self, name, data):
        path = os.path.join(self.dir.name, name)
        with open(path, 'wb') as f:
            f.write(data)
        return path

    def summarize(self, *blobs):
        summary = Summary()
        for i, blob in enumerate(blobs):
            summary.add_file(self.write(f'lumen-test.pcap{i}', blob))
        return summary


# --- tests -------------------------------------------------------------------

class ParserTest(unittest.TestCase):
    def test_dns_query_and_response(self):
        is_resp, qs, ans = summarize.parse_dns(dns_query('Api.Example.com'))
        self.assertFalse(is_resp)
        self.assertEqual(qs, [('api.example.com', 1)])
        msg = dns_response('a.example', [(5, 'b.example.net'), (1, '198.51.100.7'),
                                         (28, '2001:db8::7')])
        is_resp, qs, ans = summarize.parse_dns(msg)
        self.assertTrue(is_resp)
        self.assertEqual(ans[0], ('a.example', 5, 'b.example.net'))
        self.assertEqual(ans[1][2], ipaddress.IPv4Address('198.51.100.7'))
        self.assertEqual(ans[2][2], ipaddress.IPv6Address('2001:db8::7'))

    def test_dns_pointer_loop_does_not_hang(self):
        msg = struct.pack('>HHHHHH', 1, 0x0100, 1, 0, 0, 0) + b'\xc0\x0c'
        self.assertIsNone(summarize.parse_dns(msg))

    def test_dns_truncated(self):
        self.assertIsNone(summarize.parse_dns(b'\x00\x01'))
        self.assertIsNone(summarize.parse_dns(dns_query('example.com')[:15]))

    def test_sni(self):
        self.assertEqual(summarize.parse_sni(client_hello('Play.Example.com')),
                         'play.example.com')
        self.assertEqual(summarize.parse_sni(client_hello('cut.example', cut_after_sni=True)),
                         'cut.example')
        self.assertIsNone(summarize.parse_sni(b'\x17\x03\x03\x00\x10' + b'\0' * 50))
        self.assertIsNone(summarize.parse_sni(client_hello('x.example')[:30]))


class SummaryTest(CaptureFiles):
    def test_sll_dns_lookup_and_connection(self):
        s = self.summarize(pcap(113, [
            (0, sll(ipv4(TV, ROUTER, 17, udp(40000, 53, dns_query('api.example.com'))))),
            (1, sll(ipv4(ROUTER, TV, 17, udp(53, 40000, dns_response(
                'api.example.com', [(1, '198.51.100.7')]))), summarize.PACKET_HOST)),
            (2, sll(ipv4(TV, '198.51.100.7', 6, tcp(50000, 443, SYN)))),
        ]))
        self.assertEqual(s.lookups()['api.example.com'].count, 1)
        self.assertIn('api.example.com', s.resolved[ipaddress.IPv4Address('198.51.100.7')])
        report = summarize.render(s, Lists())
        self.assertIn('| api.example.com | 1 | 0 |', report)
        self.assertIn('| dns |', report)
        self.assertIn('2026-10-20 08:00:00Z', report)
        self.assertIn('| none |', report)  # the SYN went to a resolved address

    def test_ip_only_contacts(self):
        s = self.summarize(pcap(113, [
            (0, sll(ipv4(TV, '198.51.100.99', 6, tcp(50000, 8443, SYN)))),
            (1, sll(ipv4(TV, '198.51.100.99', 6, tcp(50000, 8443, SYN)))),  # retransmit
            (5, sll(ipv4(TV, '198.51.100.99', 6, tcp(50001, 8443, SYN)))),
            (6, sll(ipv4(TV, '198.51.100.99', 6, tcp(50001, 8443, ACK | PSH,
                                                     client_hello('hidden.example'))))),
            (7, sll(ipv4(TV, '10.0.0.50', 6, tcp(50002, 32400, SYN)))),
        ]))
        report = summarize.render(s, Lists())
        self.assertIn('| 198.51.100.99 | tcp/8443 | 2 |', report)
        self.assertIn('| hidden.example | internet |', report)
        self.assertIn('| 10.0.0.50 | tcp/32400 | 1 |', report)
        self.assertRegex(report, r'\| 10\.0\.0\.50 \|[^\n]*\| lan \|')
        self.assertRegex(report, r'\| 198\.51\.100\.99 \|[^\n]*\| internet \|')
        self.assertIn('| hidden.example | 0 | 1 |', report)
        self.assertIn('| sni |', report)

    def test_sll2_ipv6_sni_and_quic(self):
        s = self.summarize(pcap(276, [
            (0, sll2(ipv6('2001:db8::10', '2001:db8::443', 6,
                          tcp(50000, 443, ACK | PSH, client_hello('v6.example.org'))))),
            (1, sll2(ipv6('2001:db8::10', '2001:db8::443', 17,
                          udp(50000, 443, b'\xc3' + b'\0' * 30)))),
        ]))
        self.assertEqual(s.sni['v6.example.org'].count, 1)
        report = summarize.render(s, Lists())
        self.assertIn('| 2001:db8::443 | udp/443 | 1 |', report)
        self.assertIn('v6.example.org', report)

    def test_ethernet_vlan_and_dns_over_tcp(self):
        q = dns_query('tcp.example.com')
        s = self.summarize(pcap(1, [
            (0, ether(ipv4(TV, ROUTER, 6, tcp(40000, 53, ACK | PSH,
                                             struct.pack('>H', len(q)) + q)), vlan=True)),
        ]))
        self.assertEqual(s.lookups()['tcp.example.com'].count, 1)
        self.assertEqual(s.linktypes, {1})

    def test_loopback_queries_count_once_and_win(self):
        q = dns_query('app.example.com')
        s = self.summarize(pcap(113, [
            # The app asks the local proxy: tcpdump sees it going out and coming in.
            (0, sll(ipv4('127.0.0.1', '127.0.0.1', 17, udp(40000, 53, q)))),
            (0, sll(ipv4('127.0.0.1', '127.0.0.1', 17, udp(40000, 53, q)), summarize.PACKET_HOST)),
            # The proxy forwards it upstream, and makes a query of its own.
            (0, sll(ipv4(TV, ROUTER, 17, udp(41000, 53, q)))),
            (9, sll(ipv4(TV, ROUTER, 17, udp(41000, 53, dns_query('proxy.example'))))),
        ]))
        self.assertEqual(s.lookups()['app.example.com'].count, 1)
        report = summarize.render(s, Lists())
        self.assertIn('| app.example.com | 1 | 0 |', report)
        self.assertIn('| proxy.example | 0 | 0 |', report)

    def test_inbound_connections_are_listed_apart(self):
        s = self.summarize(pcap(113, [
            (0, sll(ipv4('192.0.2.77', TV, 6, tcp(51000, 8008, SYN)), summarize.PACKET_HOST)),
        ]))
        report = summarize.render(s, Lists())
        self.assertIn('## Connections to the TV', report)
        self.assertIn('| 192.0.2.77 | 8008 | 1 |', report)
        self.assertNotIn('ip-only', report)

    def test_several_files_and_formats(self):
        a = pcap(113, [(0, sll(ipv4(TV, ROUTER, 17, udp(1, 53, dns_query('one.example')))))])
        b = pcap(113, [(60, sll(ipv4(TV, ROUTER, 17, udp(1, 53, dns_query('one.example')))))],
                 magic=0xA1B23C4D, endian='>')
        s = self.summarize(a, b)
        seen = s.lookups()['one.example']
        self.assertEqual((seen.count, seen.last - seen.first), (2, 60))
        self.assertEqual(s.files, 2)

    def test_truncated_last_record_is_ignored(self):
        blob = pcap(113, [(0, sll(ipv4(TV, ROUTER, 17, udp(1, 53, dns_query('ok.example')))))])
        blob += struct.pack('<IIII', T0, 0, 100, 100) + b'\0' * 10
        s = self.summarize(blob)
        self.assertEqual(s.packets, 1)

    def test_pcapng_and_junk_are_refused(self):
        for blob in (b'\x0a\x0d\x0d\x0a' + b'\0' * 30, b'not a pcap file at all.....'):
            path = self.write('bad.pcap', blob)
            with redirect_stderr(io.StringIO()) as err:
                self.assertEqual(summarize.main([path]), 1)
            self.assertIn('summarize.py:', err.getvalue())


class ListsTest(CaptureFiles):
    def test_rules(self):
        allow = self.write('allow.txt', b'# store\nnetflix.com  # Netflix\n=lgtvsdp.com\n'
                                        b'us.lgtvsdp.com\n')
        required = self.write('req.txt', b'# firmware\nsnu.lge.com\n')
        hosts = self.write('h.hosts', b'# header\n0.0.0.0\tads.example\n'
                                      b'# 0.0.0.0 maybe.example  # confirm with capture\n'
                                      b'0.0.0.0 api.netflix.com\n')
        lists = Lists.load(allow, required, hosts)
        self.assertEqual(lists.status('netflix.com'), 'allow')
        self.assertEqual(lists.status('www.netflix.com'), 'allow')
        self.assertEqual(lists.status('notnetflix.com'), '')
        self.assertEqual(lists.status('lgtvsdp.com'), 'allow')
        self.assertEqual(lists.status('ad.lgtvsdp.com'), '')
        self.assertEqual(lists.status('x.us.lgtvsdp.com'), 'allow')
        self.assertEqual(lists.status('snu.lge.com'), 'block')
        self.assertEqual(lists.status('ads.example'), 'block')
        self.assertEqual(lists.status('maybe.example'), 'candidate')
        self.assertEqual(lists.status('api.netflix.com'), 'CONFLICT: allow + block')

    def test_prose_in_comments_is_not_a_candidate(self):
        hosts = self.write('h.hosts', b'# Glasshouse takes\n# 0.0.0.0 lines, at most 10,000 names.\n'
                                      b'# 0.0.0.0\treal.example.com\n')
        lists = Lists.load(None, None, hosts)
        self.assertEqual(lists.candidate, {'real.example.com'})

    def test_names_from_the_wire_stay_in_one_cell(self):
        s = Summary()
        s.sni['evil|name\nx.example'] = summarize.Seen()
        s.sni['evil|name\nx.example'].add(T0)
        report = summarize.render(s, Lists())
        self.assertIn('| evil\\|name?x.example | 0 | 1 |', report)

    def test_main_writes_markdown(self):
        path = self.write('lumen.pcap0', pcap(113, [
            (0, sll(ipv4(TV, ROUTER, 17, udp(1, 53, dns_query('snu.lge.com'))))),
        ]))
        required = self.write('req.txt', b'snu.lge.com\n')
        with redirect_stdout(io.StringIO()) as out:
            code = summarize.main([path, '--required', required, '--allowlist', '/nonexistent',
                                   '--hosts', '/nonexistent'])
        self.assertEqual(code, 0)
        self.assertIn('| snu.lge.com | 1 | 0 |', out.getvalue())
        self.assertIn('| block |', out.getvalue())


if __name__ == '__main__':
    unittest.main()
