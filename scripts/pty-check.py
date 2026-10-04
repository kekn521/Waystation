#!/usr/bin/env python3
"""Exercise disposable Station instances through a real PTY; never contact SSH hosts."""
import fcntl
import json
import os
import pty
import select
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path

BINARY = os.path.abspath(sys.argv[1])


class Session:
    def __init__(self, root, config=None, extra_env=None):
        self.master, self.slave = pty.openpty()
        self.before = termios.tcgetattr(self.slave)
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack('HHHH', 38, 120, 0, 0))
        self.output = b''

        def setup():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        env = dict(os.environ, TERM='xterm-256color', HOME=str(root),
                   XDG_CONFIG_HOME=str(root / 'config'), XDG_STATE_HOME=str(root / 'state-home'))
        env.update(extra_env or {})
        args = [BINARY, '--state-dir', str(root / 'state')]
        if config:
            args += ['--config', str(config)]
        self.process = subprocess.Popen(args, stdin=self.slave, stdout=self.slave,
                                        stderr=self.slave, preexec_fn=setup, env=env)
        self.collect(.4)

    def collect(self, seconds):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if select.select([self.master], [], [], .02)[0]:
                try:
                    chunk = os.read(self.master, 65536)
                    self.output += chunk
                    if b'\x1b[6n' in chunk:
                        os.write(self.master, b'\x1b[1;1R')
                except OSError:
                    break

    def send(self, text, delay=.25):
        os.write(self.master, text)
        self.collect(delay)

    def resize(self, rows, cols):
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, 0, 0))
        os.kill(self.process.pid, signal.SIGWINCH)
        self.collect(.08)

    def finished(self):
        self.collect(.2)
        self.process.wait(timeout=5)
        assert self.process.returncode == 0, self.output[-3000:]
        assert termios.tcgetattr(self.slave) == self.before, 'terminal mode was not restored'
        assert b'\x1b[?1049l' in self.output and b'\x1b[?1000l' in self.output

    def close(self):
        if self.process.poll() is None:
            os.killpg(self.process.pid, signal.SIGKILL)
            self.process.wait()
        os.close(self.master)
        os.close(self.slave)


def foreground(exit_code=0, interrupt=False, missing=False, ssh=False, shell=False):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        helper = root / 'helper'
        helper.write_text('#!/bin/sh\nstty -a\nprintf "HANDOFF_READY\\n"\n' +
                          ('sleep 20\n' if interrupt else '') + f'exit {exit_code}\n')
        helper.chmod(0o755)
        command = root / 'missing' if missing else helper
        config = root / 'config.toml'
        config.write_text(f'project_roots = []\npinned_projects = ["{root}"]\n'
                          f'[editor]\nprogram = "{command}"\n'
                          f'[tools.ssh]\nprogram = "{helper}"\n')
        (root / '.ssh').mkdir()
        (root / '.ssh/config').write_text('Host fixture-local-only\n')
        session = Session(root, config, {'SHELL': str(helper)})
        try:
            if ssh:
                session.send(b'6', .4)
                session.send(b'\r', .4)
            else:
                session.send(b't' if shell else b'e', .4)
            if interrupt:
                session.send(b'\x03', .4)
            for rows, cols in [(45, 140), (38, 120), (32, 100), (30, 89),
                               (24, 80), (18, 60), (17, 59), (38, 120)]:
                session.resize(rows, cols)
            session.send(b'q')
            session.finished()
            if not missing:
                assert b'HANDOFF_READY' in session.output, session.output[-3000:]
        finally:
            session.close()


def records(root):
    return [json.loads(p.read_text()) for p in (root / 'state/runs').glob('*/record.json')]


def await_status(root, status):
    until = time.monotonic() + 8
    while time.monotonic() < until:
        found = records(root)
        if found and found[-1]['status'] == status:
            return found[-1]
        time.sleep(.03)
    raise AssertionError((status, records(root)))


def durable_task():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        config = root / 'config.toml'
        config.write_text(f'project_roots=[]\npinned_projects=["{root}"]\n'
                          '[[tasks]]\nid="fixture"\nlabel="PTY fixture"\n'
                          'command={program="/bin/sh",args=["-c","echo TASK_READY; sleep 2; exit 0"]}\n')
        session = Session(root, config)
        try:
            session.send(b'n')
            session.send(b'\r', .3)
            await_status(root, 'Running')
            session.collect(1.1)  # receive the task provider snapshot
            session.send(b'q')
            assert b'Keep tasks running' in session.output
            session.send(b'\r')
            session.finished()
            record = await_status(root, 'Passed')
            assert record['exit_code'] == 0
            assert 'TASK_READY' in (root / 'state/runs' / record['id'] / 'output.log').read_text()
        finally:
            session.close()
            # Authenticated IPC only, never signal a saved PID.
            for r in records(root):
                run_dir = root / 'state/runs' / r['id']
                try:
                    with socket.socket(socket.AF_UNIX) as control:
                        control.settimeout(.5)
                        control.connect(str(run_dir / 'control.sock'))
                        control.sendall(('stop ' + (run_dir / 'capability').read_text() + '\n').encode())
                        control.recv(16)
                except (OSError, FileNotFoundError):
                    pass


def fresh_two_instances():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        a, b = Session(root), Session(root)
        try:
            a.send(b'/q3h')
            assert a.process.poll() is None
            a.send(b'\x1b')
            a.send(b'q')
            b.send(b'q')
            a.finished()
            b.finished()
            assert json.loads((root / 'state/state.json').read_text())['schema_version'] == 1
        finally:
            a.close()
            b.close()


if __name__ == "__main__":
    for code, interrupt, missing in [(0, False, False), (7, False, False),
                                     (0, True, False), (0, False, True)]:
        foreground(code, interrupt, missing)
    foreground(ssh=True)
    foreground(shell=True)
    durable_task()
    fresh_two_instances()
    print('PTY passed: editor/shell/SSH handoff, exit 7, Ctrl-C, missing tools, all resize thresholds,')
    print('terminal restoration, task completion after UI exit, fresh HOME, and two UI instances.')
