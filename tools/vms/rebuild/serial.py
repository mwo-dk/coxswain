#!/usr/bin/env python3
"""Drive a QEMU guest's serial console (a unix socket) like expect(1), stdlib only.

  serial.py SOCKET [LOG] STEP...      steps run in order:
    wait=REGEX     wait until REGEX shows up in the output (python re, DOTALL)
    send=TEXT      type TEXT; \\r and \\n escapes work (use \\r for Enter)
    timeout=SECS   timeout for the following waits (default 600)
    sleep=SECS     pause
    mon=CMD        send CMD to the QEMU monitor socket (MONITOR env var, e.g. "sendkey ret")
    keys=TEXT      type TEXT on the guest keyboard through the monitor (lower case, digits, space)
Output is echoed to LOG (appended). Exit 1 on a timeout, naming the step.
"""
import os, re, socket, sys, time

sock_path, steps = sys.argv[1], sys.argv[2:]
log = None
if steps and "=" not in steps[0]:
    log = open(steps.pop(0), "ab", buffering=0)

def connect(path):
    for _ in range(300):
        try:
            s = socket.socket(socket.AF_UNIX); s.connect(path); return s
        except OSError:
            time.sleep(0.2)
    sys.exit(f"cannot connect to {path}")

s = connect(sock_path)
s.settimeout(0.5)
buf = b""
timeout = 600.0

def pump():
    global buf
    try:
        d = s.recv(65536)
    except socket.timeout:
        return
    if not d:
        sys.exit("serial socket closed (guest stopped?)")
    buf += d
    if log: log.write(d)

for step in steps:
    k, _, v = step.partition("=")
    if k == "wait":
        rx, end = re.compile(v.encode(), re.S), time.time() + timeout
        while not rx.search(buf):
            if time.time() > end:
                sys.exit(f"timeout waiting for {v!r}; last output: {buf[-400:]!r}")
            pump()
        buf = buf[rx.search(buf).end():]
    elif k == "send":
        for ch in v.encode().decode("unicode_escape").encode():
            s.send(bytes([ch])); time.sleep(0.02)
    elif k == "timeout":
        timeout = float(v)
    elif k == "sleep":
        end = time.time() + float(v)
        while time.time() < end: pump()
    elif k == "mon":
        m = connect(os.environ["MONITOR"]); m.send(v.encode() + b"\n"); time.sleep(0.3); m.close()
    elif k == "keys":  # type TEXT on the guest's keyboard (VGA console) via the monitor; \r = Enter
        names = {" ": "spc", "\r": "ret", "-": "minus", "=": "equal", ".": "dot", "/": "slash"}
        m = connect(os.environ["MONITOR"])
        for ch in v.encode().decode("unicode_escape"):
            m.send(f"sendkey {names.get(ch, ch)}\n".encode()); time.sleep(0.08)
        time.sleep(0.3); m.close()
    else:
        sys.exit(f"unknown step {step}")
