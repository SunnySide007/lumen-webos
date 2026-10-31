#!/usr/bin/env python3
"""Drives lumen-ctl on the TV from the PC, over SSH with Lumen Remote's key.

    python tools/remote/ctl.py hello                      the hello and the state
    python tools/remote/ctl.py volume.set level=20        one verb, or several:
    python tools/remote/ctl.py volume.up volume.up apps.list
    python tools/remote/ctl.py launch id=netflix
    python tools/remote/ctl.py text.insert text='two words'
    python tools/remote/ctl.py settings.get keys:='["pictureMode"]'
    python tools/remote/ctl.py watch                      every event, until Ctrl-C
    python tools/remote/ctl.py check-door                 R2's door checks

A word without `=` is a verb; the words after it are its arguments. `k=v`
reads v as a whole number, true, false or null where it can, else as text;
`k:=JSON` takes the JSON as written. Each reply shows its round trip: from
the request leaving here to its reply arriving, SSH both ways included.
Events print as they come; --raw prints every line as lumen-ctl sent it.

check-door runs R2's checks of the door: `ssh … sh` must get lumen-ctl's
hello, an LD_PRELOAD the client sends must not reach it, and a remote (-R)
and a local (-L) port forward must be refused.

The TV's address and port come from .env (TV_HOST, TV_PORT) as tools/tv.sh
reads them, or the file LUMEN_ENV_FILE names. The user is root; the key is
~/.lumen/remote/id_ed25519 (tools/remote.sh keygen), or LUMEN_REMOTE_KEY.
LUMEN_SSH names another ssh. No SSH configuration file is read (-F none), so
nothing in it can reuse another connection or add a forward. Python 3.8+,
standard library only.

Exit status: 0 when every reply was ok; 1 when one was refused or a door
check failed; 2 for a mistake on the command line or in the settings; 3 when
the connection failed or lumen-ctl did not answer as lumen-ctl does.
"""

import argparse
import json
import os
import queue
import re
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROTOCOL = 1
# lumen-ctl sends these right after the hello (remote/ui/src/protocol.ts).
SNAPSHOT = ('power', 'volume', 'foreground', 'inputs', 'settings')
HELLO_WAIT = 20.0
SNAPSHOT_WAIT = 5.0
MAX_LINE = 1 << 20
# check-door's LD_PRELOAD: a path that exists on no TV.
PRELOAD = '/nonexistent/lumen-door-check.so'
HOST = re.compile(r'[A-Za-z0-9_.:%-]+')
VERB = re.compile(r'[A-Za-z0-9_.-]+')
NAME = re.compile(r'[A-Za-z_][A-Za-z0-9_]*')
INT = re.compile(r'-?[0-9]+')


class Failure(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


class Config:
    def __init__(self, host, port, key, ssh):
        self.host, self.port, self.key, self.ssh = host, port, key, ssh


def read_env_file(path):
    """NAME=value lines as sh would assign them, without running anything."""
    values = {}
    try:
        with open(path, encoding='utf-8') as f:
            lines = f.read().splitlines()
    except FileNotFoundError:
        return values
    for line in lines:
        line = line.strip()
        if line.startswith('export '):
            line = line[len('export '):].lstrip()
        name, sep, value = line.partition('=')
        if not sep or not NAME.fullmatch(name):
            continue
        if value[:1] in ('"', "'"):
            end = value.find(value[0], 1)
            value = value[1:end] if end > 0 else value[1:]
        else:
            # An unquoted value ends at a space, as a trailing comment does.
            value = value.split()[0] if value.split() else ''
        values[name] = value
    return values


def config(environ=None):
    """The TV's address from .env as tools/tv.sh reads it: the file wins."""
    environ = os.environ if environ is None else environ
    values = read_env_file(environ.get('LUMEN_ENV_FILE') or ROOT / '.env')

    def get(name, default=''):
        return values[name] if name in values else environ.get(name, default)

    host = get('TV_HOST')
    if not host:
        raise Failure(2, 'TV_HOST is not set: copy .env.example to .env and fill it in')
    if not HOST.fullmatch(host) or host.startswith('-'):
        raise Failure(2, f'TV_HOST is not a host name or address: {host!r}')
    port = get('TV_PORT', '22') or '22'
    if not port.isdigit() or not 1 <= int(port) <= 65535:
        raise Failure(2, f'TV_PORT is not a port number: {port!r}')
    key = environ.get('LUMEN_REMOTE_KEY') or str(Path.home() / '.lumen' / 'remote' / 'id_ed25519')
    if not os.path.isfile(key):
        raise Failure(2, f'no key at {key}: tools/remote.sh keygen makes one')
    return Config(host, int(port), key, environ.get('LUMEN_SSH') or 'ssh')


def ssh_argv(cfg, *extra, command=None, forwards=False):
    """ssh with only Lumen Remote's key, never a password, no terminal."""
    argv = [cfg.ssh, '-F', 'none', '-i', cfg.key, '-p', str(cfg.port), '-T',
            '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes',
            '-o', 'PasswordAuthentication=no', '-o', 'KbdInteractiveAuthentication=no',
            '-o', 'ControlPath=none', '-o', 'ConnectTimeout=8',
            '-o', 'StrictHostKeyChecking=accept-new', '-o', 'ServerAliveInterval=15']
    if not forwards:
        argv += ['-o', 'ClearAllForwardings=yes']
    argv += [*extra, f'root@{cfg.host}']
    if command:
        argv.append(command)
    return argv


def typed(text):
    if INT.fullmatch(text):
        return int(text)
    return {'true': True, 'false': False, 'null': None}.get(text, text)


def parse_verbs(words):
    """[(verb, args)] from `verb k=v k:=json verb …`."""
    requests = []
    for word in words:
        name, sep, value = word.partition('=')
        if not sep:
            if not VERB.fullmatch(word):
                raise Failure(2, f'not a verb: {word!r}')
            requests.append((word, {}))
            continue
        if not requests:
            raise Failure(2, f'{word!r} comes before any verb')
        if name.endswith(':'):
            name = name[:-1]
            try:
                parsed = json.loads(value)
            except ValueError as e:
                raise Failure(2, f'{word!r}: not JSON: {e}') from None
        else:
            parsed = typed(value)
        if not name:
            raise Failure(2, f'{word!r} has no name')
        requests[-1][1][name] = parsed
    return requests


def compact(value):
    return json.dumps(value, ensure_ascii=False, separators=(',', ':'))


class Session:
    """One ssh to lumen-ctl serve, its lines read on a thread as they come."""

    def __init__(self, argv, stderr=None):
        try:
            self.proc = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                         stderr=stderr)
        except OSError as e:
            raise Failure(3, f'cannot run {argv[0]}: {e}') from None
        self.lines = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        while True:
            raw = self.proc.stdout.readline(MAX_LINE)
            if not raw:
                break
            self.lines.put((time.monotonic(), raw))
        self.lines.put((time.monotonic(), None))

    def next(self, timeout):
        """(arrival time, the line's JSON, the line), or None after `timeout`."""
        try:
            at, raw = self.lines.get(timeout=max(timeout, 0))
        except queue.Empty:
            return None
        if raw is None:
            self.lines.put((at, None))
            code = self.proc.poll()
            raise Failure(3, 'the connection closed' + (f' (ssh exit {code})' if code else ''))
        text = raw.decode('utf-8', errors='replace').rstrip('\r\n')
        try:
            value = json.loads(text)
        except ValueError:
            value = None
        return at, value, text

    def send(self, request):
        line = (compact(request) + '\n').encode('utf-8')
        try:
            self.proc.stdin.write(line)
            self.proc.stdin.flush()
        except OSError as e:
            raise Failure(3, f'the connection closed: {e}') from None
        return time.monotonic()

    def close(self):
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        try:
            self.proc.wait(3)
        except subprocess.TimeoutExpired:
            self.proc.terminate()
            try:
                self.proc.wait(2)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


class Printer:
    def __init__(self, raw, out):
        self.raw, self.out = raw, out

    def say(self, text):
        print(text, file=self.out, flush=True)

    def event(self, value, text, alive=False):
        if self.raw:
            self.say(text)
        elif value.get('event') != 'alive' or alive:
            data = value.get('data')
            self.say(value['event'] + ('' if data is None else ': ' + compact(data)))

    def hello(self, data, text):
        if self.raw:
            self.say(text)
            return
        polled = ' '.join(data.get('polling') or []) or 'nothing'
        self.say(f"hello: {data.get('server')}, protocol {data.get('protocol')}, "
                 f"bus {data.get('bus')}, keys {data.get('keys')}; polled: {polled}")
        self.say('  verbs: ' + ' '.join(data.get('verbs') or []))

    def reply(self, verb, value, text, ms):
        if self.raw:
            self.say(text)
            print(f'({verb} #{value.get("id")}: {ms:.1f} ms)', file=sys.stderr, flush=True)
        elif value.get('ok') is True:
            self.say(f"{verb} #{value['id']}: ok in {ms:.1f} ms {compact(value.get('result'))}")
        else:
            error = value.get('error') or {}
            self.say(f"{verb} #{value.get('id')}: {error.get('code')}: {error.get('message')} "
                     f"({ms:.1f} ms)")


def is_event(value):
    return isinstance(value, dict) and isinstance(value.get('event'), str)


def is_reply(value):
    return isinstance(value, dict) and 'ok' in value and 'id' in value


def wait_hello(session, printer):
    """The hello, checked: nothing is sent to what isn't lumen-ctl."""
    timeout = HELLO_WAIT
    line = session.next(timeout)
    if line is None:
        raise Failure(3, f'no hello in {timeout:.0f} s: is the key in authorized_keys? '
                         '(tools/remote.sh status)')
    _, value, text = line
    if not is_event(value) or value['event'] != 'hello' or not isinstance(value.get('data'), dict):
        raise Failure(3, f'the first line is not lumen-ctl\'s hello: {text[:80]!r}')
    data = value['data']
    if data.get('protocol') != PROTOCOL:
        raise Failure(3, f'lumen-ctl speaks protocol {data.get("protocol")!r}, this speaks {PROTOCOL}')
    if printer:
        printer.hello(data, text)
    return data


def wait_snapshot(session, printer):
    """The state that follows the hello, or what of it comes in SNAPSHOT_WAIT."""
    seen, deadline = set(), time.monotonic() + SNAPSHOT_WAIT
    while not seen.issuperset(SNAPSHOT):
        line = session.next(deadline - time.monotonic())
        if line is None:
            missing = ' '.join(n for n in SNAPSHOT if n not in seen)
            printer.say(f'(no {missing} after {SNAPSHOT_WAIT:.0f} s)')
            return
        _, value, text = line
        other(printer, value, text)
        if is_event(value):
            seen.add(value['event'])


def other(printer, value, text, alive=False):
    """A line that isn't the reply being waited for."""
    if is_event(value):
        printer.event(value, text, alive)
    else:
        printer.say(text if printer.raw else f'(unexpected: {text[:200]})')


def run_verbs(session, printer, requests, timeout, linger):
    failed = 0
    for n, (verb, args) in enumerate(requests, 1):
        request = {'id': n, 'verb': verb}
        if args:
            request['args'] = args
        sent = session.send(request)
        deadline = sent + timeout
        while True:
            line = session.next(deadline - time.monotonic())
            if line is None:
                raise Failure(3, f'no reply to {verb} #{n} in {timeout:.0f} s')
            at, value, text = line
            # A reply with no id is the refusal of a line lumen-ctl couldn't read.
            if is_reply(value) and value['id'] in (n, None):
                printer.reply(verb, value, text, (at - sent) * 1000)
                failed += value.get('ok') is not True
                break
            other(printer, value, text)
    end = time.monotonic() + linger
    while (line := session.next(end - time.monotonic())) is not None:
        other(printer, line[1], line[2])
    return 1 if failed else 0


def watch(session, printer):
    while True:
        line = session.next(3600)
        if line is not None:
            other(printer, line[1], line[2], alive=True)


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


def stop(proc):
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(2)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


def last_line(data):
    lines = data.decode('utf-8', errors='replace').strip().splitlines()
    return lines[-1] if lines else 'nothing on stderr'


def door_shell(cfg):
    with Session(ssh_argv(cfg, command='sh')) as s:
        try:
            data = wait_hello(s, None)
        except Failure as e:
            return False, str(e)
    if not str(data.get('server', '')).startswith('lumen-ctl'):
        return False, f"a hello from {data.get('server')!r}"
    return True, f"lumen-ctl answered ({data.get('server')})"


def door_environment(cfg):
    """A client's LD_PRELOAD would run its code as root before lumen-ctl's
    main: the TV's dynamic loader names a missing one on stderr."""
    argv = ssh_argv(cfg, '-o', f'SetEnv=LD_PRELOAD={PRELOAD}')
    with Session(argv, stderr=subprocess.PIPE) as s:
        try:
            wait_hello(s, None)
        except Failure as e:
            return False, str(e)
    err = s.proc.stderr.read()
    if PRELOAD.encode() in err:
        return False, f'the TV saw the LD_PRELOAD sent: {last_line(err)}'
    return True, 'lumen-ctl answered, and the TV never saw the LD_PRELOAD sent'


def door_remote_forward(cfg, timeout):
    argv = ssh_argv(cfg, '-N', '-o', 'ExitOnForwardFailure=yes',
                    '-R', '127.0.0.1:0:127.0.0.1:9', forwards=True)
    proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                            stderr=subprocess.PIPE)
    try:
        _, err = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        stop(proc)
        return False, f'ssh still held the forward after {timeout:.0f} s'
    return proc.returncode != 0, f'ssh exit {proc.returncode}: {last_line(err)}'


def door_local_forward(cfg, timeout):
    port = free_port()
    argv = ssh_argv(cfg, '-N', '-L', f'127.0.0.1:{port}:127.0.0.1:22', forwards=True)
    proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                            stderr=subprocess.PIPE)
    try:
        deadline = time.monotonic() + timeout
        while True:
            if proc.poll() is not None:
                return False, f'ssh ended before its end of the forward was up: {last_line(proc.stderr.read())}'
            try:
                conn = socket.create_connection(('127.0.0.1', port), timeout=1)
                break
            except OSError:
                if time.monotonic() > deadline:
                    return False, f'nothing listened on 127.0.0.1:{port} in {timeout:.0f} s'
                time.sleep(0.2)
        with conn:
            conn.settimeout(timeout)
            try:
                data = conn.recv(64)
            except ConnectionError:
                data = b''
            except socket.timeout:
                return False, f'no answer and no refusal in {timeout:.0f} s'
        if data:
            return False, f'the forward reached the TV: {data[:24]!r}'
        return True, 'the TV closed the forwarded connection'
    finally:
        stop(proc)


def check_door(cfg, printer, timeout):
    checks = [('`ssh ... sh` gets lumen-ctl', lambda: door_shell(cfg)),
              ("the client's environment stays out", lambda: door_environment(cfg)),
              ('a remote forward (-R) is refused', lambda: door_remote_forward(cfg, timeout)),
              ('a local forward (-L) is refused', lambda: door_local_forward(cfg, timeout))]
    failed = 0
    for n, (what, check) in enumerate(checks, 1):
        ok, detail = check()
        failed += not ok
        printer.say(f"door {n}/{len(checks)}: {what}: {'ok' if ok else 'FAILED'} ({detail})")
    return 1 if failed else 0


def parse_args(argv):
    parser = argparse.ArgumentParser(
        prog='ctl.py', description='Drives lumen-ctl on the TV over SSH.',
        epilog='See the top of tools/remote/ctl.py for examples.')
    parser.add_argument('--raw', action='store_true', help="print lumen-ctl's lines as sent")
    parser.add_argument('--timeout', type=float, default=10.0, metavar='S',
                        help='how long a reply or a door check may take (default 10)')
    parser.add_argument('--linger', type=float, default=1.0, metavar='S',
                        help='how long to show events after the last reply (default 1)')
    parser.add_argument('command', help='hello, watch, check-door, or a verb')
    parser.add_argument('words', nargs=argparse.REMAINDER, help="the verb's arguments, and more verbs")
    return parser.parse_args(argv)


def main(argv=None, out=None):
    args = parse_args(argv)
    printer = Printer(args.raw, out or sys.stdout)
    try:
        if args.command in ('hello', 'watch', 'check-door') and args.words:
            raise Failure(2, f'{args.command} takes no more words')
        requests = []
        if args.command not in ('hello', 'watch', 'check-door'):
            requests = parse_verbs([args.command, *args.words])
        cfg = config()
        if args.command == 'check-door':
            return check_door(cfg, printer, args.timeout)
        with Session(ssh_argv(cfg)) as session:
            wait_hello(session, printer)
            if args.command == 'watch':
                watch(session, printer)
            wait_snapshot(session, printer)
            if args.command == 'hello':
                return 0
            return run_verbs(session, printer, requests, args.timeout, args.linger)
    except Failure as e:
        print(f'ctl.py: {e}', file=sys.stderr)
        return e.code
    except KeyboardInterrupt:
        return 130


if __name__ == '__main__':
    for stream in (sys.stdout, sys.stderr):
        # A Windows console may not take every character an app's name has.
        if hasattr(stream, 'reconfigure'):
            stream.reconfigure(errors='backslashreplace')
    sys.exit(main())
