"""Tests for tools/recon/redact.py: python3 -I -m unittest discover -s tools/tests"""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'recon'))

from redact import redact_line  # noqa: E402


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


if __name__ == '__main__':
    unittest.main()
