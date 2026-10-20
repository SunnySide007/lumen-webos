#!/usr/bin/env python3
"""Removes identifying details from recon output before it is committed.

Reads text on stdin and writes the redacted text to stdout:

* MAC addresses become ``xx:xx:xx:xx:xx:xx``.
* Private IPv4 addresses (10/8, 172.16/12, 192.168/16) become ``<lan-ip>``.
* A JSON value whose key names a serial number, a device, account or network
  identifier, a location or a secret becomes ``"<redacted>"``.

Model names, firmware versions, app ids and settings values stay, since they
are what the report is for.
"""

import re
import sys

MAC = re.compile(r'\b[0-9A-Fa-f]{2}(?:[:-][0-9A-Fa-f]{2}){5}\b')
PRIVATE_IP = re.compile(
    r'\b(?:10\.\d{1,3}|192\.168|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b')
SENSITIVE_KEY = re.compile(
    r'serial|nduid|uuid|udid|device_?id|device_?name|friendly_?name|tv_?name'
    r'|mac_?addr|adid|advertis|ssid|account|e-?mail|user_?id|token|passw'
    r'|secret|cookie|session|signature|zip|postal|latitude|longitude'
    r'|location|ip_?addr|hostname',
    re.IGNORECASE)
JSON_PAIR = re.compile(
    r'("((?:[^"\\]|\\.)*)"\s*:\s*)("(?:[^"\\]|\\.)*"|-?\d+(?:\.\d+)?)')


def redact_line(line):
    line = MAC.sub('xx:xx:xx:xx:xx:xx', line)
    line = PRIVATE_IP.sub('<lan-ip>', line)

    def pair(match):
        if SENSITIVE_KEY.search(match.group(2)):
            return match.group(1) + '"<redacted>"'
        return match.group(0)

    return JSON_PAIR.sub(pair, line)


def main():
    for line in sys.stdin:
        sys.stdout.write(redact_line(line))


if __name__ == '__main__':
    main()
