#!/usr/bin/env python3
"""Removes identifying details from recon output before it is committed.

Reads text on stdin and writes the redacted text to stdout:

* MAC addresses become ``xx:xx:xx:xx:xx:xx``.
* Private IPv4 addresses (10/8, 172.16/12, 192.168/16) become ``<lan-ip>``.
* Global, unique-local and link-local IPv6 addresses become ``<ipv6>``: their
  interface half is often built from the MAC address.
* A JSON value whose key names a serial number, a device, account or network
  identifier, a location or a secret becomes ``"<redacted>"``. That covers
  the network's name, which the connection manager gives as ``ssid`` and as
  ``displayName``.
* The ``Phys`` and ``Uniq`` values in /proc/bus/input/devices, which hold a
  Bluetooth address in forms the MAC pattern misses, become ``<redacted>``.
* SSH public keys become ``<ssh-key>``, a line that is only base64 becomes
  ``<base64>``, and a PEM private key block is dropped whole. The surveys never
  print a key; this is the second lock.

Model names, firmware versions, app ids and settings values stay, since they
are what the report is for.
"""

import re
import sys

MAC = re.compile(r'\b[0-9A-Fa-f]{2}(?:[:-][0-9A-Fa-f]{2}){5}\b')
INPUT_ID = re.compile(r'^(P: Phys=|U: Uniq=)[^\r\n]+')
# Every SSH public key blob starts with a four-byte length, which is AAAA in
# base64.
SSH_KEY = re.compile(r'\bAAAA[0-9A-Za-z+/]{16,}={0,2}')
BASE64_LINE = re.compile(r'^[0-9A-Za-z+/]{60,}={0,2}[ \t\r\n]*$')
PEM_BEGIN = re.compile(r'-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----')
PEM_END = re.compile(r'-----END [A-Z0-9 ]*PRIVATE KEY-----')
PRIVATE_IP = re.compile(
    r'\b(?:10\.\d{1,3}|192\.168|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b')
# 2000::/3, fc00::/7 and fe80::/10, which leaves ::1 and timestamps alone.
IPV6 = re.compile(
    r'\b(?:[23][0-9a-f]{3}|f[cd][0-9a-f]{2}|fe[89ab][0-9a-f]):'
    r'(?:[0-9a-f]{0,4}:){1,6}[0-9a-f]{1,4}\b',
    re.IGNORECASE)
SENSITIVE_KEY = re.compile(
    r'serial|nduid|uuid|udid|device_?id|device_?name|friendly_?name|tv_?name'
    r'|mac_?addr|adid|advertis|ssid|account|e-?mail|user_?id|token|passw'
    r'|secret|cookie|session|signature|zip|postal|latitude|longitude'
    r'|location|ip_?addr|hostname|display_?name|network_?name',
    re.IGNORECASE)
JSON_PAIR = re.compile(
    r'("((?:[^"\\]|\\.)*)"\s*:\s*)("(?:[^"\\]|\\.)*"|-?\d+(?:\.\d+)?)')


def redact_line(line):
    line = INPUT_ID.sub(r'\1<redacted>', line)
    line = SSH_KEY.sub('<ssh-key>', line)
    line = BASE64_LINE.sub('<base64>\n', line)
    line = MAC.sub('xx:xx:xx:xx:xx:xx', line)
    line = PRIVATE_IP.sub('<lan-ip>', line)
    line = IPV6.sub('<ipv6>', line)

    def pair(match):
        if SENSITIVE_KEY.search(match.group(2)):
            return match.group(1) + '"<redacted>"'
        return match.group(0)

    return JSON_PAIR.sub(pair, line)


def redact_lines(lines):
    """redact_line over many lines, dropping PEM private key blocks whole.

    A block that never ends drops everything after its first line.
    """
    in_key = False
    for line in lines:
        if in_key:
            in_key = not PEM_END.search(line)
        elif PEM_BEGIN.search(line):
            in_key = not PEM_END.search(line)
            yield '<private key removed>\n'
        else:
            yield redact_line(line)


def main():
    # Windows Python would write CRLF and read in the locale's code page.
    sys.stdin.reconfigure(encoding='utf-8', errors='replace')
    sys.stdout.reconfigure(encoding='utf-8', newline='\n')
    for line in redact_lines(sys.stdin):
        sys.stdout.write(line)


if __name__ == '__main__':
    main()
