#!/usr/bin/env python3
"""Controlled PTY checks; no real SSH connections or project commands."""
import os, pty, select, signal, struct, subprocess, sys, tempfile, termios, time, fcntl
binary=os.path.abspath(sys.argv[1])
def run(exit_code=0, interrupt=False, missing=False):
    with tempfile.TemporaryDirectory() as root:
        helper=os.path.join(root,'editor')
        with open(helper,'w') as f:
            f.write('#!/bin/sh\nstty -a\nprintf "HANDOFF_READY\\n"\n'+('sleep 20\n' if interrupt else '')+f'exit {exit_code}\n')
        os.chmod(helper,0o755)
        config=os.path.join(root,'config.toml')
        with open(config,'w') as f:f.write(f'project_roots = []\npinned_projects = ["{root}"]\n[editor]\nprogram = "{helper if not missing else root+"/missing"}"\n')
        master,slave=pty.openpty();before=termios.tcgetattr(slave)
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',38,120,0,0))
        def setup():
            os.setsid();fcntl.ioctl(0,termios.TIOCSCTTY,0)
        p=subprocess.Popen([binary,'--config',config,'--state-dir',root+'/state'],stdin=slave,stdout=slave,stderr=slave,preexec_fn=setup,env=dict(os.environ,TERM='xterm-256color'))
        output=b''
        def collect(seconds):
            nonlocal output
            until=time.monotonic()+seconds
            while time.monotonic()<until:
                if select.select([master],[],[],0.03)[0]:
                    try:
                        chunk=os.read(master,65536);output+=chunk
                        if b"\x1b[6n" in chunk:os.write(master,b"\x1b[1;1R")
                    except OSError:break
        try:
            collect(.4);os.write(master,b'e');collect(.5)
            if interrupt:os.write(master,b'\x03');collect(.4)
            for rows,cols in [(18,60),(17,59),(32,100),(38,120)]:
                fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',rows,cols,0,0));os.kill(p.pid,signal.SIGWINCH);collect(.07)
            os.write(master,b'q');collect(.4);p.wait(timeout=5)
            assert p.returncode==0,(p.returncode,output[-3000:])
            after=termios.tcgetattr(slave)
            assert before==after,'terminal attributes did not restore'
            assert b'\x1b[?1049l' in output and b'\x1b[?1000l' in output
            if not missing:assert b'HANDOFF_READY' in output
        finally:
            if p.poll() is None:os.killpg(p.pid,signal.SIGKILL);p.wait()
            os.close(master);os.close(slave)
for code,interrupt,missing in [(0,False,False),(7,False,False),(0,True,False),(0,False,True)]:run(code,interrupt,missing)
print('PTY: success, exit 7, Ctrl-C, missing editor, resize, and terminal restoration passed')
