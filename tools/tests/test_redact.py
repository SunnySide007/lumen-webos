"""Tests for tools/recon/redact.py: python3 -I -m unittest discover -s tools/tests"""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'recon'))

from redact import redact_line, redact_lines  # noqa: E402

# Made up, like the keys in tools/tests/fixtures/remote/authorized_keys.
ED25519 = 'AAAAC3NzaC1lZDI1NTE5AAAAIFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE0'
RSA = 'AAAAB3NzaC1yc2EAAAADAQABAAABAQFAKEFAKEFAKEFAKE+/FAKEFAKE=='


def redact_text(text):
    return ''.join(redact_lines(text.splitlines(keepends=True)))


class RedactTest(unittest.TestCase):
    def test_mac_addresses(self):
        self.assertEqual(redact_line('ether 3c:cd:93:0A:1b:2C up'),
                         'ether xx:xx:xx:xx:xx:xx up')
        self.assertEqual(redact_line('hw 3C-CD-93-0A-1B-2C'), 'hw xx:xx:xx:xx:xx:xx')

    def test_private_addresses_only(self):
        self.assertEqual(redact_line('192.168.1.50 10.0.0.7 172.20.3.4'),
                         '<lan-ip> <lan-ip> <lan-ip>')
        self.assertEqual(redact_line('0.0.0.0 ads.example 127.0.0.1 8.8.8.8 172.32.0.1'),
                         '0.0.0.0 ads.example 127.0.0.1 8.8.8.8 172.32.0.1')

    def test_ipv6_addresses(self):
        self.assertEqual(redact_line('"ip": "fd12:3456:789a:1:7640:beff:fe01:2345"'),
                         '"ip": "<ipv6>"')
        self.assertEqual(redact_line('fe80::7640:beff:fe01:2345/64 2001:db8::1'),
                         '<ipv6>/64 <ipv6>')
        self.assertEqual(redact_line('::1 up 04:37:52, std::string'),
                         '::1 up 04:37:52, std::string')

    def test_identifying_json_values(self):
        line = '"serialNumber": "903KXYZ123", "modelName": "OLED55B9PLA",'
        self.assertEqual(redact_line(line),
                         '"serialNumber": "<redacted>", "modelName": "OLED55B9PLA",')
        self.assertEqual(redact_line('"deviceName": "Living room TV"'),
                         '"deviceName": "<redacted>"')
        self.assertEqual(redact_line('"latitude": 40.71, "zipCode": "10001"'),
                         '"latitude": "<redacted>", "zipCode": "<redacted>"')

    def test_ordinary_settings_stay(self):
        line = '"pictureMode": "cinema", "oledLight": "80", "firmwareVersion": "05.50.00"'
        self.assertEqual(redact_line(line), line)

    def test_escaped_quotes_in_values(self):
        self.assertEqual(redact_line(r'"accountId": "a\"b", "x": 1'),
                         '"accountId": "<redacted>", "x": 1')

    def test_input_device_phys_and_uniq(self):
        # Bluetooth addresses as the kernel writes them, and in the forms the
        # MAC pattern misses: no separators, a prefix.
        self.assertEqual(redact_line('P: Phys=a4:b1:c2:d3:e4:f5\n'), 'P: Phys=<redacted>\n')
        self.assertEqual(redact_line('P: Phys=bluetooth-A4B1C2D3E4F5\n'), 'P: Phys=<redacted>\n')
        self.assertEqual(redact_line('U: Uniq=a4b1c2d3e4f5\n'), 'U: Uniq=<redacted>\n')
        self.assertEqual(redact_line('U: Uniq=a4b1c2d3e4f5'), 'U: Uniq=<redacted>')
        # Virtual devices have none, and the other lines stay.
        for line in ('P: Phys=\n', 'U: Uniq=\n', 'N: Name="LGE RCU"\n',
                     'S: Sysfs=/devices/virtual/misc/uhid/0005:045E:0B13.0001/input/input5\n',
                     'H: Handlers=js0 event5 \n'):
            self.assertEqual(redact_line(line), line)

    def test_wifi_name_from_the_connection_manager(self):
        line = ('{ "wifi": { "ssid": "HomeNet-5G", "bssid": "aa:bb:cc:dd:ee:01", '
                '"displayName": "HomeNet-5G", "ipAddress": "192.168.1.50", '
                '"gateway": "192.168.1.1", "state": "connected", "onInternet": "yes" } }')
        self.assertEqual(
            redact_line(line),
            '{ "wifi": { "ssid": "<redacted>", "bssid": "<redacted>", '
            '"displayName": "<redacted>", "ipAddress": "<redacted>", '
            '"gateway": "<lan-ip>", "state": "connected", "onInternet": "yes" } }')
        self.assertEqual(redact_line('"networkName": "HomeNet-5G"'), '"networkName": "<redacted>"')

    def test_ssh_public_keys(self):
        self.assertEqual(redact_line(f'ssh-ed25519 {ED25519} pc@home\n'),
                         'ssh-ed25519 <ssh-key> pc@home\n')
        self.assertEqual(redact_line(f'restrict,command="/x serve" ssh-rsa {RSA}\n'),
                         'restrict,command="/x serve" ssh-rsa <ssh-key>\n')
        # A blob with no key type in front of it, and the same after a key type
        # that redact.py does not know.
        self.assertEqual(redact_line(f'{ED25519}\n'), '<ssh-key>\n')
        self.assertEqual(redact_line(f'sk-ssh-ed25519@openssh.com {ED25519}\n'),
                         'sk-ssh-ed25519@openssh.com <ssh-key>\n')

    def test_base64_lines(self):
        body = 'b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW'
        self.assertEqual(redact_line(body + '\n'), '<base64>\n')
        # Ordinary text, hex dumps and short words stay.
        for line in ('Dropbear v2020.81\n', 'KEY=ffffffff ffffffff fffffffe\n',
                     'a4b1c2d3e4f5a4b1c2d3e4f5\n', 'restrict: found\n'):
            self.assertEqual(redact_line(line), line)

    def test_private_key_blocks_are_dropped_whole(self):
        text = ('before\n'
                '-----BEGIN OPENSSH PRIVATE KEY-----\n'
                'b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ\n'
                'AAAAAAAAAAAA\n'
                '-----END OPENSSH PRIVATE KEY-----\n'
                'after\n')
        self.assertEqual(redact_text(text), 'before\n<private key removed>\nafter\n')
        # A block with no end takes the rest of the text with it.
        self.assertEqual(redact_text('x\n-----BEGIN RSA PRIVATE KEY-----\nabc\nmore\n'),
                         'x\n<private key removed>\n')
        # A public key block is not private.
        self.assertEqual(redact_text('-----BEGIN PUBLIC KEY-----\nshort\n'),
                         '-----BEGIN PUBLIC KEY-----\nshort\n')

    def test_the_remote_surveys_own_lines_stay(self):
        # What tools/recon/remote.sh prints about keys and dropbear is the
        # report; none of it may be masked.
        text = ('--- /home/root/.ssh/authorized_keys\n'
                '-rw------- 1 root root 91 Oct 31 05:44 /home/root/.ssh/authorized_keys\n'
                '2 key line(s)\n'
                '  key 1: options: restrict,command=…; type: ssh-ed25519\n'
                '  key 2: options: no-pty,no-port-forwarding,from=…; type: ssh-rsa\n'
                '  key 3: options: ?; type: (unrecognised)\n'
                '$ /media/developer/apps/usr/palm/services/org.webosbrew.hbchannel.service/bin/dropbear -V\n'
                'Dropbear v2020.81\n'
                'restrict: found\n'
                'pid 612: uid 0 (root), gid 0, groups 0 , capabilities 0000003fffffffff\n')
        self.assertEqual(redact_text(text), text)


if __name__ == '__main__':
    unittest.main()
