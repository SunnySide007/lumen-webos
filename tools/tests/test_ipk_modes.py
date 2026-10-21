"""Tests for tools/ipk/fix_modes.py: python3 -I -m unittest discover -s tools/tests"""

import gzip
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from contextlib import redirect_stdout

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'ipk'))

import fix_modes  # noqa: E402
from fix_modes import fix_ipk, read_ar, write_ar  # noqa: E402

APP = 'usr/palm/applications/com.example.app'
T0 = 1792483200  # 2026-10-20 08:00:00Z


def ar_header(name, size):
    return (f'{name:<16}{T0:<12}{0:<6}{0:<6}{"100644":<8}{size:<10}'.encode('ascii')
            + b'`\n')


def tar_gz(entries):
    """entries: (name, mode, content or None for a folder), as ares-package
    writes them: ustar, owned by 0:0 with no user or group names."""
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode='w', format=tarfile.USTAR_FORMAT) as tar:
        for name, mode, content in entries:
            info = tarfile.TarInfo(name)
            info.mode = mode
            info.mtime = T0
            if content is None:
                info.type = tarfile.DIRTYPE
                tar.addfile(info)
            else:
                info.size = len(content)
                tar.addfile(info, io.BytesIO(content))
    return gzip.compress(raw.getvalue(), mtime=0)


def ipk(members):
    return write_ar([(ar_header(name, len(body)), body) for name, body in members])


def ares_like_ipk():
    """An .ipk with ares-package's modes: folders 0777, its own files 0666."""
    control = tar_gz([('control', 0o666, b'Package: com.example.app\nVersion: 1.0.0\n')])
    data = tar_gz([
        ('usr', 0o777, None),
        ('usr/palm', 0o777, None),
        ('usr/palm/applications', 0o777, None),
        (APP, 0o777, None),
        (f'{APP}/appinfo.json', 0o644, b'{"id": "com.example.app"}'),
        (f'{APP}/app', 0o755, b'\x7fELF binary'),
        (f'{APP}/icons', 0o777, None),
        (f'{APP}/icons/a.png', 0o644, b'\x89PNG'),
        ('usr/palm/packages/com.example.app/packageinfo.json', 0o666, b'{}'),
    ])
    return ipk([('debian-binary', b'2.0\n'), ('control.tar.gz', control),
                ('data.tar.gz', data)])


def entries(ipk_bytes, member):
    """{name: (mode, everything else tar keeps about it)} of one tar member
    of an .ipk."""
    for header, body in read_ar(ipk_bytes):
        if fix_modes.member_name(header) == member:
            with tarfile.open(fileobj=io.BytesIO(body), mode='r:gz') as tar:
                out = {}
                for info in tar:
                    content = tar.extractfile(info).read() if info.isfile() else None
                    out[info.name] = (info.mode, info.type, info.uid, info.gid, info.uname,
                                      info.gname, info.mtime, info.linkname,
                                      dict(info.pax_headers), content)
                return out
    raise KeyError(member)


class FixModesTest(unittest.TestCase):
    def test_folders_and_files_get_sane_modes(self):
        fixed, changed = fix_ipk(ares_like_ipk())
        data = entries(fixed, 'data.tar.gz')
        modes = {name: e[0] for name, e in data.items()}
        self.assertEqual(modes['usr'], 0o755)
        self.assertEqual(modes[APP], 0o755)
        self.assertEqual(modes[f'{APP}/icons'], 0o755)
        self.assertEqual(modes[f'{APP}/app'], 0o755)
        self.assertEqual(modes[f'{APP}/appinfo.json'], 0o644)
        self.assertEqual(modes['usr/palm/packages/com.example.app/packageinfo.json'], 0o644)
        self.assertEqual(entries(fixed, 'control.tar.gz')['control'][0], 0o644)
        # Five folders, packageinfo.json and control.
        self.assertEqual(changed, 7)

    def test_nothing_else_changes(self):
        before = ares_like_ipk()
        fixed, _ = fix_ipk(before)
        old, new = read_ar(before), read_ar(fixed)
        # The same members in the same order, with the same headers but size.
        self.assertEqual([h[:48] for h, _ in old], [h[:48] for h, _ in new])
        self.assertEqual(old[0][1], new[0][1])
        for member in ('control.tar.gz', 'data.tar.gz'):
            a, b = entries(before, member), entries(fixed, member)
            self.assertEqual(list(a), list(b))
            for name in a:
                self.assertEqual(a[name][1:], b[name][1:], name)

    def test_long_names_and_pax_times_survive(self):
        # ares-package writes neither today; a rewrite must not lose them.
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode='w', format=tarfile.PAX_FORMAT) as tar:
            info = tarfile.TarInfo(f'{APP}/' + 'n' * 120)
            info.mode, info.mtime, info.size = 0o666, T0 + 0.5, 1
            info.pax_headers = {'atime': str(T0)}
            tar.addfile(info, io.BytesIO(b'x'))
        before = ipk([('debian-binary', b'2.0\n'), ('control.tar.gz', tar_gz([])),
                      ('data.tar.gz', gzip.compress(raw.getvalue(), mtime=0))])
        fixed, changed = fix_ipk(before)
        self.assertEqual(changed, 1)
        [(name, old)] = entries(before, 'data.tar.gz').items()
        [new] = entries(fixed, 'data.tar.gz').values()
        self.assertEqual(new[0], 0o644)
        self.assertEqual(new[1:], old[1:], name)
        self.assertEqual(new[6], T0 + 0.5)

    def test_running_twice_changes_nothing_more(self):
        once, _ = fix_ipk(ares_like_ipk())
        twice, changed = fix_ipk(once)
        self.assertEqual(changed, 0)
        self.assertEqual(once, twice)

    def test_odd_sized_members_are_padded(self):
        members = [(ar_header('debian-binary', 3), b'2.0'),
                   (ar_header('control.tar.gz', 4), b'abcd')]
        data = write_ar(members)
        self.assertEqual(data[8 + 60 + 3:8 + 60 + 4], b'\n')
        self.assertEqual(read_ar(data), members)

    def test_refuses_what_is_not_an_ipk(self):
        with self.assertRaisesRegex(ValueError, 'not an ar archive'):
            fix_ipk(b'PK\x03\x04')
        with self.assertRaisesRegex(ValueError, 'unexpected ipk members'):
            fix_ipk(ipk([('debian-binary', b'2.0\n'), ('evil.tar.gz', b'')]))
        with self.assertRaisesRegex(ValueError, 'truncated'):
            fix_ipk(ares_like_ipk()[:-10])

    def test_main_rewrites_in_place(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, 'com.example.app_1.0.0_arm.ipk')
            with open(path, 'wb') as f:
                f.write(ares_like_ipk())
            out = io.StringIO()
            with redirect_stdout(out):
                self.assertEqual(fix_modes.main([path]), 0)
            self.assertEqual(out.getvalue(), f'{path}: 7 mode(s) fixed\n')
            self.assertEqual(os.listdir(tmp), [os.path.basename(path)])
            with open(path, 'rb') as f:
                self.assertEqual(entries(f.read(), 'data.tar.gz')['usr'][0], 0o755)

    @unittest.skipUnless(shutil.which('ares-package'), 'needs ares-package (build container)')
    def test_fixes_what_ares_package_writes(self):
        with tempfile.TemporaryDirectory() as tmp:
            stage = os.path.join(tmp, 'stage')
            os.makedirs(os.path.join(stage, 'assets'))
            with open(os.path.join(stage, 'appinfo.json'), 'w') as f:
                json.dump({'id': 'com.example.app', 'version': '1.0.0', 'vendor': 'Example',
                           'type': 'native', 'main': 'app', 'title': 'Example',
                           'icon': 'icon.png'}, f)
            with open(os.path.join(stage, 'icon.png'), 'wb') as f:
                f.write(b'\x89PNG')
            with open(os.path.join(stage, 'app'), 'wb') as f:
                f.write(b'\x7fELF')
            os.chmod(os.path.join(stage, 'app'), 0o755)
            with open(os.path.join(stage, 'assets', 'a.txt'), 'w') as f:
                f.write('a')
            subprocess.run(['ares-package', '--no-minify', '--outdir', tmp, stage],
                           check=True, capture_output=True)
            [name] = [n for n in os.listdir(tmp) if n.endswith('.ipk')]
            with open(os.path.join(tmp, name), 'rb') as f:
                raw = f.read()
        fixed, changed = fix_ipk(raw)
        self.assertGreater(changed, 0, 'ares-package no longer writes 0777 folders?')
        for member in ('control.tar.gz', 'data.tar.gz'):
            before = entries(raw, member)
            after = entries(fixed, member)
            self.assertEqual(list(before), list(after))
            for entry, (mode, *rest) in after.items():
                self.assertEqual(mode & 0o022, 0, f'{entry} is {oct(mode)}')
                self.assertEqual(rest, list(before[entry][1:]), entry)
        data = entries(fixed, 'data.tar.gz')
        self.assertEqual(data['usr/palm/applications/com.example.app/app'][0], 0o755)


if __name__ == '__main__':
    unittest.main()
