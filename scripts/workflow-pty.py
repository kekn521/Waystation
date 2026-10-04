#!/usr/bin/env python3
"""Exercise agent switching and task creation without invoking any real AI provider."""
import json
import runpy
import subprocess
import tempfile
import time
from pathlib import Path

helpers = runpy.run_path(str(Path(__file__).with_name('pty-check.py')))
Session = helpers['Session']
await_status = helpers['await_status']


def tmux(root, *args):
    key = json.loads((root / 'state/agents/server.json').read_text())
    return subprocess.run(['tmux', '-L', 'station-' + key, *args], capture_output=True, text=True)


def until(predicate, session, label):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        session.collect(.05)
        if predicate():
            return
    raise AssertionError((label, session.output[-5000:]))


def agent_records(root):
    return [json.loads(p.read_text()) for p in (root / 'state/agents').glob('*.json') if p.name != 'server.json']


with tempfile.TemporaryDirectory(prefix='station-workflow-') as directory:
    root = Path(directory)
    helper = root / 'agent'
    helper.write_text('#!/usr/bin/python3\nimport os\nprint("AGENT_READY", os.getpid(), flush=True)\nwhile True:\n try: print("REPLY:", input(), flush=True)\n except EOFError: break\n')
    helper.chmod(0o755)
    config = root / 'config.toml'
    original = (f'project_roots = []\npinned_projects = ["{root}"]\n'
                f'[tools.codex]\nprogram = "{helper}"\n[tools.claude]\nprogram = "{helper}"\n')
    config.write_text(original)
    session = Session(root, config, {'TMUX': '/fake/outer/socket,123,0'})
    try:
        session.send(b'3nImplementation\x13', .8)
        until(lambda: b'AGENT_READY' in session.output, session, 'first agent opens')
        session.send(b'first-agent-input\r')
        assert b'REPLY: first-agent-input' in session.output
        session.send(b'\x1b[24~', .4)
        until(lambda: not tmux(root, 'list-clients').stdout.strip(), session, 'F12 detaches')
        assert session.process.poll() is None
        session.send(b'nReview\t\x1b[C\x13', .8)
        until(lambda: len(agent_records(root)) == 2, session, 'second session created')
        until(lambda: bool(tmux(root, 'list-clients').stdout.strip()), session, 'second session attached')
        session.send(b'\x1b[24~', .4)
        until(lambda: not tmux(root, 'list-clients').stdout.strip(), session, 'second F12 detaches')
        assert {r['tool'] for r in agent_records(root)} == {'codex', 'claude'}
        session.send(b'q')
        session.finished()
        assert len(tmux(root, 'list-sessions').stdout.splitlines()) == 2
        session.close()
        session = Session(root, config)
        session.send(b'/Implementation\r', .8)
        until(lambda: bool(tmux(root, 'list-clients').stdout.strip()), session, 'reconnect after restart')
        session.send(b'after-restart\r')
        assert b'REPLY: after-restart' in session.output
        session.send(b'\x1b[24~', .4)
        until(lambda: not tmux(root, 'list-clients').stdout.strip(), session, 'return before close')
        session.send(b'x')
        session.send(b'\x1b[B\r', .5)
        until(lambda: len(agent_records(root)) == 1, session, 'close selected session only')
        assert agent_records(root)[0]['name'] == 'Review'
        session.send(b'4aGreeting\tprintf "%s" "TASK_FROM_FORM"\x13', .5)
        until(lambda: bool(list(root.glob('config.tasks/*.json'))), session, 'save task form')
        assert config.read_text() == original
        assert not list((root / 'state/runs').glob('*/record.json'))
        session.send(b'\r', .4)
        run = await_status(root, 'Passed')
        assert 'TASK_FROM_FORM' in (root / 'state/runs' / run['id'] / 'output.log').read_text()
        session.send(b'q')
        session.finished()
        session.close()
        session = Session(root, config)
        session.send(b'4n', .3)
        assert b'Greeting' in session.output
        session.send(b'q')
        session.finished()
    finally:
        session.close()
        if (root / 'state/agents/server.json').exists():
            tmux(root, 'kill-server')
print('Workflow PTY passed: two named agents, F12 return, nested TMUX environment, restart/reconnect,')
print('close isolation, task form save/run, literal argv, config preservation, and recipe reload.')
