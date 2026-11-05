#!/usr/bin/python3 -I
"""The user's own podman for an app in the screenshot sandbox, run outside it.

No podman may run inside the sandbox (sandbox.sh): its pause process would land in the
sandbox's namespaces and break the user's rootless podman. For pictures of a screen that needs
a container tool (draw.io, Kroki, OCRmyPDF), this file plays two parts:

- outside, `podman-relay.py serve <socket> <demo-home>` listens on a Unix socket and, for each
  call, runs the real podman in the user's normal session, with the caller's own standard
  input, output and error;
- inside, bound over the sandbox's /usr/bin/podman (`SANDBOX_ARGS="--ro-bind <this file>
  /usr/bin/podman"`), it sends its arguments and those three file descriptors to the socket,
  found at $XDG_RUNTIME_DIR/podman-relay.sock, and exits with podman's exit code.

So the dialogs and results the pictures show are podman's own. The one change on the way: a
mount's source under /home/demo (the sandbox's home) becomes the same folder in <demo-home>.
Images are never pulled by the relay: the app runs with --pull=never, and Pull in Settings would
pull into the user's own store, so pull only what the pictures need and remove it after.

    docs/screenshots/podman-relay.py serve "$RUNTIME/podman-relay.sock" "$DEMO_HOME" &
    SANDBOX_ARGS="--ro-bind docs/screenshots/podman-relay.py /usr/bin/podman" uv run my-shots.py

Stop the relay by its own PID when done.
"""

import json, os, signal, socket, subprocess, sys, threading

SANDBOX_HOME = "/home/demo"


def client():
    s = socket.socket(socket.AF_UNIX)
    s.connect(os.path.join(os.environ["XDG_RUNTIME_DIR"], "podman-relay.sock"))
    socket.send_fds(s, [json.dumps(sys.argv[1:]).encode()], [0, 1, 2])
    code = s.recv(16)
    sys.exit(int(code) if code else 1)


def outside(arg, demo):
    # Only the mount source: `type=bind,"source=/home/demo/…",target=/src`.
    return arg.replace(f'"source={SANDBOX_HOME}/', f'"source={demo}/').replace(f'"source={SANDBOX_HOME}"', f'"source={demo}"')


def handle(conn, demo):
    with conn:
        msg, fds, _, _ = socket.recv_fds(conn, 1 << 20, 3)
        args = [outside(a, demo) for a in json.loads(msg)]
        p = subprocess.Popen(["/usr/bin/podman", *args], stdin=fds[0], stdout=fds[1], stderr=fds[2])
        for fd in fds:
            os.close(fd)

        # The caller gone (the app stopped it): stop its podman too.
        def watch():
            try:
                conn.recv(1)
            except OSError:
                pass
            if p.poll() is None:
                p.send_signal(signal.SIGTERM)
        threading.Thread(target=watch, daemon=True).start()
        try:
            conn.sendall(str(p.wait()).encode())
        except OSError:
            pass


def serve(path, demo):
    demo = os.path.realpath(demo)
    if os.path.exists(path):
        os.unlink(path)
    s = socket.socket(socket.AF_UNIX)
    s.bind(path)
    os.chmod(path, 0o600)
    s.listen(16)
    while True:
        conn, _ = s.accept()
        threading.Thread(target=handle, args=(conn, demo), daemon=True).start()


if __name__ == "__main__":
    if sys.argv[1:2] == ["serve"]:
        serve(sys.argv[2], sys.argv[3])
    else:
        client()
