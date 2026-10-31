"""Tests for tools/remote/ctl.py: python3 -I -m unittest discover -s tools/tests

A fake ssh on PATH plays lumen-ctl serve (or runs the real binary over its
fake bus, when cargo has built it) and the TV's side of port forwards. The
host, port and keys are made up; nothing reaches a network or a TV.
"""

import io
import json
import os
import sys
import tempfile
import textwrap
import unittest
from contextlib import redirect_stderr
from pathlib import Path
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'remote'))

import ctl  # noqa: E402

LUMEN_CTL = Path(__file__).resolve().parents[2] / 'shell' / 'target' / 'debug' / 'lumen-ctl'

FAKE_SSH = '''\
import json, os, socket, sys, time

argv = sys.argv[1:]
with open(os.environ['FAKE_SSH_LOG'], 'a') as f:
    f.write(json.dumps(argv) + '\\n')
forwards = os.environ.get('FAKE_SSH_FORWARDS', '')
forwards_ok = forwards == 'ok'

# -R, as ssh -v tells it: granted, refused, never answered (the B9's
# dropbear), or no way in at all.
if '-R' in argv:
    if forwards == 'noauth':
        sys.stderr.write('root@tv.example: Permission denied (publickey).\\n')
        sys.exit(255)
    sys.stderr.write('debug1: Remote connections from 127.0.0.1:0 forwarded to local address 127.0.0.1:9\\n'
                     'Authenticated to tv.example ([tv.example]:2222) using "publickey".\\n'
                     'debug1: ssh_init_forwarding: expecting replies for 1 forwards\\n')
    sys.stderr.flush()
    if forwards_ok:
        sys.stderr.write('debug1: remote forward success for: listen 0, connect 127.0.0.1:9\\n'
                         'Allocated port 40001 for remote forward to 127.0.0.1:9\\n')
        sys.stderr.flush()
        time.sleep(60)
    if forwards == 'silent':
        time.sleep(60)
    sys.stderr.write('Error: remote port forwarding failed for listen port 0\\n'
                     'debug1: Exit status 255\\n')
    sys.exit(255)
if '-L' in argv:
    port = int(argv[argv.index('-L') + 1].split(':')[1])
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(('127.0.0.1', port))
    listener.listen()
    while True:
        conn, _ = listener.accept()
        if forwards_ok:
            conn.sendall(b'SSH-2.0-dropbear_2022.83\\r\\n')
            time.sleep(1)
        conn.close()

preload = [a.split('=', 2)[2] for a in argv if a.startswith('SetEnv=LD_PRELOAD=')]
if preload and os.environ.get('FAKE_SSH_ENV') == 'passed':
    sys.stderr.write("ERROR: ld.so: object '%s' from LD_PRELOAD cannot be preloaded: ignored.\\n"
                     % preload[0])
    sys.stderr.flush()

mode = os.environ.get('FAKE_SSH_MODE', 'serve')
if mode == 'exec':
    command = json.loads(os.environ['FAKE_SSH_EXEC'])
    os.execv(command[0], command)

def out(value):
    sys.stdout.write(json.dumps(value) + '\\n')
    sys.stdout.flush()

def received():
    for line in sys.stdin:
        with open(os.environ['FAKE_SSH_LOG'] + '.in', 'a') as f:
            f.write(line)
        yield line

if mode == 'silent':
    for _ in received():
        pass
    sys.exit(0)
if mode == 'shell':
    sys.stdout.write('sh: no job control in this shell\\n')
    sys.stdout.flush()
    for _ in received():
        pass
    sys.exit(0)

out({'event': 'hello', 'data': {
    'protocol': int(os.environ.get('FAKE_SSH_PROTOCOL', '1')), 'server': 'lumen-ctl 0.0.1',
    'verbs': ['volume.set', 'apps.list'], 'polling': ['power'], 'bus': 'fake', 'keys': 'none'}})
out({'event': 'power', 'data': {'state': 'active', 'lg': 'Active'}})
out({'event': 'volume', 'data': {'level': 14, 'muted': False}})
out({'event': 'foreground', 'data': {'id': 'netflix'}})
out({'event': 'inputs', 'data': {'inputs': []}})
out({'event': 'settings', 'data': {'values': {}}})
if mode == 'close':
    out({'event': 'alive', 'data': {}})
    sys.exit(0)
for line in received():
    try:
        request = json.loads(line)
    except ValueError:
        out({'id': None, 'ok': False, 'error': {'code': 'bad-line', 'message': 'not JSON'}})
        continue
    verb, rid = request.get('verb'), request.get('id')
    if verb == 'volume.set':
        time.sleep(0.01)
        out({'id': rid, 'ok': True, 'result': {}})
        out({'event': 'volume', 'data': {'level': request['args']['level'], 'muted': False}})
    elif verb == 'apps.list':
        out({'event': 'alive', 'data': {}})
        out({'id': rid, 'ok': True, 'result': {'apps': [{'id': 'netflix', 'title': 'Netflix'}]}})
    else:
        out({'id': rid, 'ok': False,
             'error': {'code': 'unknown-verb', 'message': 'unknown verb "%s"' % verb}})
'''


class EnvFileTest(unittest.TestCase):
    def test_reads_assignments_as_sh_would_without_running_them(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, '.env')
            with open(path, 'w', encoding='utf-8') as f:
                f.write(textwrap.dedent('''\
                    # a comment
                    TV_HOST=tv.example   # the TV
                    export TV_PORT="2222"
                    TV_KEY='~/.ssh/lumen tv'
                    TV_USER=$(id -un)
                    not a line
                    2BAD=x
                    EMPTY=
                    '''))
            self.assertEqual(ctl.read_env_file(path), {
                'TV_HOST': 'tv.example', 'TV_PORT': '2222', 'TV_KEY': '~/.ssh/lumen tv',
                'TV_USER': '$(id', 'EMPTY': ''})

    def test_a_missing_file_is_empty(self):
        self.assertEqual(ctl.read_env_file('/no/such/.env'), {})


class ConfigTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.key = os.path.join(self.dir.name, 'id_ed25519')
        Path(self.key).write_text('made-up private key\n')
        self.env = os.path.join(self.dir.name, '.env')

    def config(self, text, **environ):
        Path(self.env).write_text(text)
        return ctl.config({'LUMEN_ENV_FILE': self.env, 'LUMEN_REMOTE_KEY': self.key, **environ})

    def failure(self, text, **environ):
        with self.assertRaises(ctl.Failure) as caught:
            self.config(text, **environ)
        self.assertEqual(caught.exception.code, 2)
        return str(caught.exception)

    def test_the_file_wins_over_the_environment_as_in_tv_sh(self):
        cfg = self.config('TV_HOST=tv.example\nTV_PORT=2222\n', TV_HOST='other.example')
        self.assertEqual((cfg.host, cfg.port, cfg.key, cfg.ssh), ('tv.example', 2222, self.key, 'ssh'))
        cfg = self.config('', TV_HOST='env.example', LUMEN_SSH='/opt/ssh')
        self.assertEqual((cfg.host, cfg.port, cfg.ssh), ('env.example', 22, '/opt/ssh'))

    def test_the_key_defaults_to_lumen_remotes(self):
        Path(self.env).write_text('TV_HOST=tv.example\n')
        with mock.patch.object(ctl.Path, 'home', return_value=Path(self.dir.name)):
            with self.assertRaises(ctl.Failure) as caught:
                ctl.config({'LUMEN_ENV_FILE': self.env})
        self.assertIn(os.path.join('.lumen', 'remote', 'id_ed25519'), str(caught.exception))
        self.assertIn('tools/remote.sh keygen', str(caught.exception))

    def test_refuses_what_ssh_would_take_as_an_option(self):
        self.assertIn('TV_HOST is not set', self.failure(''))
        self.assertIn('not a host', self.failure('TV_HOST=-oProxyCommand=sh\n'))
        self.assertIn('not a host', self.failure('TV_HOST="tv example"\n'))
        self.assertIn('not a port', self.failure('TV_HOST=tv.example\nTV_PORT=0\n'))
        self.assertIn('not a port', self.failure('TV_HOST=tv.example\nTV_PORT=-p1\n'))
        self.assertIn('no key', self.failure('TV_HOST=tv.example\n',
                                             LUMEN_REMOTE_KEY=self.key + '.missing'))


class ArgvTest(unittest.TestCase):
    cfg = ctl.Config('tv.example', 2222, '/k/id_ed25519', 'ssh')

    def test_only_the_key_no_password_no_terminal_no_config(self):
        argv = ctl.ssh_argv(self.cfg)
        self.assertEqual(argv[:6], ['ssh', '-F', 'none', '-i', '/k/id_ed25519', '-p'])
        self.assertEqual(argv[-1], 'root@tv.example')
        for option in ('IdentitiesOnly=yes', 'BatchMode=yes', 'PasswordAuthentication=no',
                       'KbdInteractiveAuthentication=no', 'ControlPath=none',
                       'ClearAllForwardings=yes'):
            self.assertIn(option, argv)
        self.assertIn('-T', argv)

    def test_a_command_goes_last_and_forwards_keep_theirs(self):
        argv = ctl.ssh_argv(self.cfg, '-N', '-L', 'x', command='sh', forwards=True)
        self.assertEqual(argv[-4:], ['-L', 'x', 'root@tv.example', 'sh'])
        self.assertNotIn('ClearAllForwardings=yes', argv)


class VerbsTest(unittest.TestCase):
    def test_words_become_requests_with_typed_arguments(self):
        self.assertEqual(ctl.parse_verbs(['volume.set', 'level=20', 'apps.list', 'mute.set',
                                          'muted=true', 'text.insert', 'text=two words',
                                          'launch', 'id:="123"', 'settings.get',
                                          'keys:=["pictureMode"]', 'x', 'n=null', 'm=-3']),
                         [('volume.set', {'level': 20}), ('apps.list', {}),
                          ('mute.set', {'muted': True}), ('text.insert', {'text': 'two words'}),
                          ('launch', {'id': '123'}), ('settings.get', {'keys': ['pictureMode']}),
                          ('x', {'n': None, 'm': -3})])

    def test_mistakes_are_refused_before_anything_runs(self):
        for words in (['level=20'], ['volume.set', 'level:={'], ['volume.set', '=1'],
                      ['volume set']):
            with self.assertRaises(ctl.Failure) as caught:
                ctl.parse_verbs(words)
            self.assertEqual(caught.exception.code, 2)


@unittest.skipIf(os.name == 'nt', 'the fake ssh is a script with a #! line')
class SessionTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        d = Path(self.dir.name)
        (d / 'bin').mkdir()
        ssh = d / 'bin' / 'ssh'
        ssh.write_text(f'#!{sys.executable}\n' + FAKE_SSH)
        ssh.chmod(0o755)
        (d / 'id_ed25519').write_text('made-up private key\n')
        (d / '.env').write_text('TV_HOST=tv.example\nTV_PORT=2222\n')
        self.log = d / 'ssh.log'
        self.environ = {
            'PATH': f"{d / 'bin'}{os.pathsep}{os.environ.get('PATH', '')}",
            'LUMEN_ENV_FILE': str(d / '.env'), 'LUMEN_REMOTE_KEY': str(d / 'id_ed25519'),
            'FAKE_SSH_LOG': str(self.log)}
        for name in ('LUMEN_SSH', 'TV_HOST', 'TV_PORT', 'FAKE_SSH_MODE', 'FAKE_SSH_FORWARDS'):
            self.environ.setdefault(name, '')
        patcher = mock.patch.object(ctl, 'HELLO_WAIT', 2.0)
        patcher.start()
        self.addCleanup(patcher.stop)

    def run_ctl(self, *argv, **environ):
        out, err = io.StringIO(), io.StringIO()
        env = {k: v for k, v in {**self.environ, **environ}.items() if v}
        with mock.patch.dict(os.environ, env, clear=True), redirect_stderr(err):
            code = ctl.main(['--timeout', '3', '--linger', '0.3', *argv], out=out)
        return code, out.getvalue(), err.getvalue()

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def sent(self):
        path = Path(f'{self.log}.in')
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_hello_shows_lumen_ctl_and_the_state_and_sends_nothing(self):
        code, out, err = self.run_ctl('hello')
        self.assertEqual(code, 0, err)
        self.assertIn('hello: lumen-ctl 0.0.1, protocol 1, bus fake, keys none; polled: power', out)
        self.assertIn('  verbs: volume.set apps.list', out)
        self.assertIn('power: {"state":"active","lg":"Active"}', out)
        self.assertIn('settings: {"values":{}}', out)
        self.assertEqual(self.sent(), [])
        argv = self.calls()[0]
        self.assertEqual(argv[:6], ['-F', 'none', '-i', self.environ['LUMEN_REMOTE_KEY'],
                                    '-p', '2222'])
        self.assertIn('IdentitiesOnly=yes', argv)
        self.assertEqual(argv[-1], 'root@tv.example')

    def test_a_verb_its_reply_with_the_round_trip_and_the_event_after(self):
        code, out, err = self.run_ctl('volume.set', 'level=20')
        self.assertEqual(code, 0, err)
        self.assertRegex(out, r'volume\.set #1: ok in [0-9]+\.[0-9] ms \{\}')
        self.assertIn('volume: {"level":20,"muted":false}', out)
        self.assertEqual(self.sent(), [{'id': 1, 'verb': 'volume.set', 'args': {'level': 20}}])

    def test_several_verbs_in_order_and_a_refusal_fails_the_run(self):
        code, out, _ = self.run_ctl('apps.list', 'sh', 'volume.set', 'level=3')
        self.assertEqual(code, 1)
        self.assertIn('apps.list #1: ok in', out)
        self.assertIn('sh #2: unknown-verb: unknown verb "sh"', out)
        self.assertIn('volume.set #3: ok in', out)
        self.assertNotIn('alive', out)
        self.assertEqual([r['id'] for r in self.sent()], [1, 2, 3])
        self.assertNotIn('args', self.sent()[0])

    def test_raw_prints_lines_as_lumen_ctl_sent_them(self):
        code, out, err = self.run_ctl('--raw', 'apps.list')
        self.assertEqual(code, 0)
        lines = out.splitlines()
        self.assertEqual(json.loads(lines[0])['event'], 'hello')
        self.assertIn('{"event": "alive", "data": {}}', lines)
        self.assertIn('(apps.list #1:', err)

    def test_nothing_is_sent_to_what_is_not_lumen_ctl(self):
        for mode, words in (('silent', 'no hello'), ('shell', "not lumen-ctl's hello")):
            code, _, err = self.run_ctl('volume.set', 'level=20', FAKE_SSH_MODE=mode)
            self.assertEqual(code, 3, mode)
            self.assertIn(words, err)
        code, _, err = self.run_ctl('volume.set', 'level=20', FAKE_SSH_PROTOCOL='2')
        self.assertEqual(code, 3)
        self.assertIn('protocol 2', err)
        self.assertEqual(self.sent(), [])

    def test_watch_shows_every_event_until_the_connection_ends(self):
        code, out, err = self.run_ctl('watch', FAKE_SSH_MODE='close')
        self.assertEqual(code, 3)
        self.assertIn('foreground: {"id":"netflix"}', out)
        self.assertIn('alive: {}', out)
        self.assertIn('the connection closed', err)

    def test_usage_mistakes_need_no_tv(self):
        code, _, err = self.run_ctl('level=20')
        self.assertEqual(code, 2)
        code, _, err = self.run_ctl('hello', 'more')
        self.assertEqual(code, 2)
        self.assertFalse(self.log.exists())

    def test_check_door_passes_when_the_tv_refuses_shell_env_and_forwards(self):
        code, out, err = self.run_ctl('check-door')
        self.assertEqual(code, 0, out + err)
        self.assertIn('door 1/4: `ssh ... sh` gets lumen-ctl: ok', out)
        self.assertIn("door 2/4: the client's environment stays out: ok", out)
        self.assertIn('door 3/4: a remote forward (-R) is refused: ok (ssh exit 255: '
                      'Error: remote port forwarding failed for listen port 0)', out)
        self.assertIn('door 4/4: a local forward (-L) is refused: ok', out)
        calls = self.calls()
        self.assertEqual(calls[0][-1], 'sh')
        self.assertIn(f'SetEnv=LD_PRELOAD={ctl.PRELOAD}', calls[1])
        self.assertIn('-R', calls[2])
        self.assertIn('-v', calls[2])
        self.assertIn('ExitOnForwardFailure=yes', calls[2])
        self.assertIn('-L', calls[3])
        for argv in calls[2:]:
            self.assertNotIn('ClearAllForwardings=yes', argv)

    def test_check_door_fails_when_a_forward_goes_through(self):
        code, out, _ = self.run_ctl('--timeout', '1', 'check-door', FAKE_SSH_FORWARDS='ok')
        self.assertEqual(code, 1)
        self.assertIn('door 1/4: `ssh ... sh` gets lumen-ctl: ok', out)
        self.assertIn('door 3/4: a remote forward (-R) is refused: FAILED (the TV granted it: '
                      'debug1: remote forward success for: listen 0, connect 127.0.0.1:9)', out)
        self.assertIn("door 4/4: a local forward (-L) is refused: FAILED (the forward reached the TV: b'SSH-2.0", out)

    def test_check_door_takes_a_tv_that_never_answers_the_forward_as_never_granting_it(self):
        code, out, err = self.run_ctl('--timeout', '1', 'check-door', FAKE_SSH_FORWARDS='silent')
        self.assertEqual(code, 0, out + err)
        self.assertIn('door 3/4: a remote forward (-R) is refused: ok (never granted: '
                      'the TV did not answer the request in 1 s)', out)

    def test_check_door_fails_the_forward_check_when_ssh_never_gets_in(self):
        code, out, _ = self.run_ctl('check-door', FAKE_SSH_FORWARDS='noauth')
        self.assertEqual(code, 1)
        self.assertIn('door 3/4: a remote forward (-R) is refused: FAILED (ssh never got in, '
                      'so nothing was checked: root@tv.example: Permission denied (publickey).)', out)

    def test_check_door_fails_when_the_tv_takes_the_clients_environment(self):
        code, out, _ = self.run_ctl('check-door', FAKE_SSH_ENV='passed')
        self.assertEqual(code, 1)
        self.assertIn("door 2/4: the client's environment stays out: FAILED (the TV saw the LD_PRELOAD", out)
        self.assertIn('door 3/4: a remote forward (-R) is refused: ok', out)

    @unittest.skipUnless(LUMEN_CTL.is_file(), 'cargo test --workspace builds lumen-ctl')
    def test_against_lumen_ctl_itself_on_its_fake_bus(self):
        d = Path(self.dir.name)
        command = [str(LUMEN_CTL), 'serve', '--bus', 'fake', '--log', str(d / 'ctl.log'),
                   '--config', str(d / 'no-ctl.conf')]
        code, out, err = self.run_ctl('volume.set', 'level=20', 'apps.list', 'sh',
                                      FAKE_SSH_MODE='exec', FAKE_SSH_EXEC=json.dumps(command))
        self.assertEqual(code, 1, err)
        self.assertIn('bus fake, keys none; polled: power', out)
        self.assertRegex(out, r'volume\.set #1: ok in [0-9.]+ ms \{\}')
        self.assertIn('volume: {"level":20,"muted":false}', out)
        self.assertIn('apps.list #2: ok in', out)
        self.assertIn('sh #3: unknown-verb: unknown verb "sh"', out)


if __name__ == '__main__':
    unittest.main()
