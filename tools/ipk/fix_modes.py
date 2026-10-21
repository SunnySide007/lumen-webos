#!/usr/bin/env python3
"""Gives the files in an .ipk sane permissions, rewriting it in place.

ares-package (LG's packager, 3.2.6) stores every folder as 0777, and the files
it writes itself (control, packageinfo.json) as 0666, whatever the staged tree
says; the TV unpacks them that way. A world-writable app folder lets any user
on the TV replace the shell's binary. This makes folders 0755, files with an
execute bit 0755, and other files 0644. Nothing else changes: the ar members,
their order, the tar entries' names, contents, owners and times, and the
control fields all stay as ares-package wrote them.

    python3 -I tools/ipk/fix_modes.py dist/*.ipk
"""

import gzip
import io
import os
import sys
import tarfile

AR_MAGIC = b'!<arch>\n'
AR_HEADER = 60
MEMBERS = ('debian-binary', 'control.tar.gz', 'data.tar.gz')


def read_ar(data):
    """Splits an ar archive into (60-byte header, body) pairs."""
    if not data.startswith(AR_MAGIC):
        raise ValueError('not an ar archive')
    members = []
    pos = len(AR_MAGIC)
    while pos < len(data):
        header = data[pos:pos + AR_HEADER]
        if len(header) < AR_HEADER or header[58:60] != b'`\n':
            raise ValueError(f'bad ar header at byte {pos}')
        size = int(header[48:58].decode('ascii'))
        body = data[pos + AR_HEADER:pos + AR_HEADER + size]
        if len(body) != size:
            raise ValueError(f'ar member at byte {pos} is truncated')
        members.append((header, body))
        pos += AR_HEADER + size + (size & 1)
    return members


def write_ar(members):
    """Joins (header, body) pairs back into an ar archive, fixing each size."""
    out = [AR_MAGIC]
    for header, body in members:
        out.append(header[:48] + f'{len(body):<10}'.encode('ascii') + header[58:60])
        out.append(body)
        if len(body) & 1:
            out.append(b'\n')
    return b''.join(out)


def member_name(header):
    """An ar member's name, without GNU ar's trailing slash."""
    return header[:16].decode('ascii').rstrip(' ').rstrip('/')


def fixed_mode(info):
    if info.isdir():
        return 0o755
    if info.isfile():
        return 0o755 if info.mode & 0o111 else 0o644
    return info.mode


def fix_tar_gz(blob):
    """Returns the gzipped tar `blob` with fixed modes, and how many changed."""
    changed = 0
    out = io.BytesIO()
    # PAX keeps long names and pax times; an entry that needs neither comes
    # out as the same ustar header.
    with tarfile.open(fileobj=io.BytesIO(blob), mode='r:gz') as src, \
            gzip.GzipFile(fileobj=out, mode='wb', mtime=0) as gz, \
            tarfile.open(fileobj=gz, mode='w', format=tarfile.PAX_FORMAT) as dst:
        for info in src:
            content = src.extractfile(info) if info.isfile() else None
            mode = fixed_mode(info)
            if mode != info.mode:
                info.mode = mode
                changed += 1
            dst.addfile(info, content)
    return out.getvalue(), changed


def fix_ipk(data):
    """Returns the .ipk `data` with fixed modes, and how many entries changed."""
    members = read_ar(data)
    names = sorted(member_name(header) for header, _ in members)
    if names != sorted(MEMBERS):
        raise ValueError(f'unexpected ipk members: {", ".join(names)}')
    changed = 0
    fixed = []
    for header, body in members:
        if member_name(header).endswith('.tar.gz'):
            body, n = fix_tar_gz(body)
            changed += n
        fixed.append((header, body))
    return write_ar(fixed), changed


def main(paths):
    if not paths:
        print('usage: fix_modes.py IPK...', file=sys.stderr)
        return 2
    for path in paths:
        with open(path, 'rb') as f:
            data, changed = fix_ipk(f.read())
        tmp = path + '.tmp'
        try:
            with open(tmp, 'wb') as f:
                f.write(data)
            os.replace(tmp, path)
        finally:
            if os.path.exists(tmp):
                os.remove(tmp)
        print(f'{path}: {changed} mode(s) fixed')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
