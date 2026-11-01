"""Times LocalSend on this machine with the official LocalSend CLI 1.18.2 as
the receiver, driven through a pseudo terminal (it answers Y the moment the
request appears, like LocalSend's Quick Save would). The sender is either
the official CLI too (Enter on its device list) or, for folders the CLI
cannot take on its command line, sweaxdrop-bench speaking the LocalSend v2
protocol with LocalSend 1.18's two uploads at a time.

    python tools/localsend_bench.py CLI WORKDIR CASE [--sender cli|proto] [--bench EXE] [--folder DIR] [files...]

The clock starts when the send is triggered and stops when every byte of
every file is in the receiver's folder (polled every 20 ms). One JSON line
per run on stdout.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time

from winpty import PtyProcess

ap = argparse.ArgumentParser()
ap.add_argument('cli')
ap.add_argument('work')
ap.add_argument('case')
ap.add_argument('--sender', default='cli')
ap.add_argument('--bench')
ap.add_argument('--folder')
ap.add_argument('--concurrency', type=int, default=2)
ap.add_argument('files', nargs='*')
a = ap.parse_args()

files = list(a.files)
if a.folder:
    for root, _, fs in os.walk(a.folder):
        files += [os.path.join(root, f) for f in fs]
total = sum(os.path.getsize(f) for f in files)
ansi = re.compile(r'\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07')
ENTER = chr(13)


def spawn(name, port, extra, dest):
    env = dict(os.environ)
    env['XDG_CONFIG_HOME'] = os.path.join(a.work, 'cfg-' + name)
    cmd = [a.cli, '--alias', name, '--port', str(port), '--destination', dest] + extra
    return PtyProcess.spawn(cmd, env=env, dimensions=(50, 200))


class Log:
    def __init__(self, p):
        self.p = p
        self.text = ''
        threading.Thread(target=self.pump, daemon=True).start()

    def pump(self):
        while self.p.isalive():
            try:
                d = self.p.read(8192)
            except EOFError:
                break
            self.text += ansi.sub('', d)

    def wait(self, needle, timeout=30):
        until = time.time() + timeout
        while time.time() < until:
            if needle in self.text:
                return True
            time.sleep(0.01)
        return False


def received(dest):
    n = size = 0
    for root, _, fs in os.walk(dest):
        for f in fs:
            n += 1
            try:
                size += os.path.getsize(os.path.join(root, f))
            except OSError:
                pass
    return n, size


dest = os.path.join(a.work, 'inbox')
shutil.rmtree(dest, ignore_errors=True)
os.makedirs(dest, exist_ok=True)
r = spawn('lsrecv', 53390, [], dest)
rl = Log(r)
if not rl.wait('Ready to accept', 20):
    sys.exit('receiver did not start')

sender = None
if a.sender == 'cli':
    args = [x for f in files for x in ('-f', f)]
    sender = spawn('lssend', 53391, args, os.path.join(a.work, 'unused'))
    sl = Log(sender)
    if not sl.wait('lsrecv', 30):
        sys.exit('sender did not discover the receiver')
    time.sleep(0.3)
    seen = len(rl.text)
    t0 = time.perf_counter()
    sender.write(ENTER)
else:
    seen = len(rl.text)
    t0 = time.perf_counter()
    cmd = [a.bench, 'localsend', '--to', '127.0.0.1:53390', '--concurrency', str(a.concurrency), '--repeat', '1']
    cmd += ['--path', a.folder] if a.folder else [x for f in files for x in ('--path', f)]
    sender = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

answered = None
until = time.time() + 300
while time.time() < until:
    if 'Accept?' in rl.text[seen:]:
        r.write('y')
        answered = time.perf_counter() - t0
        break
    if a.sender == 'proto' and sender.poll() is not None:
        sys.exit('the LocalSend protocol sender stopped before the request was shown')
    time.sleep(0.005)
if answered is None:
    sys.exit('no request reached the receiver')

first = None
while time.perf_counter() - t0 < 3600:
    n, size = received(dest)
    if first is None and size > 0:
        first = time.perf_counter() - t0
    if n >= len(files) and size >= total:
        break
    if 'failed' in rl.text[seen:] and 'Received' in rl.text[seen:]:
        break
    time.sleep(0.02)
secs = time.perf_counter() - t0
n, size = received(dest)
tool = 'localsend-cli-1.18.2' if a.sender == 'cli' else 'localsend-cli-1.18.2 receiver, LocalSend v2 sender (%d at a time)' % a.concurrency
print(json.dumps({
    'tool': tool, 'case': a.case, 'bytes': total, 'files': len(files), 'seconds': secs,
    'ttfb_ms': (first or secs) * 1000, 'mb_s': total / secs / 1e6, 'answered_ms': (answered or 0) * 1000,
    'complete': n >= len(files) and size >= total,
}), flush=True)
if isinstance(sender, PtyProcess):
    sender.terminate(force=True)
else:
    sender.terminate()
r.terminate(force=True)
time.sleep(0.5)
shutil.rmtree(dest, ignore_errors=True)
