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

So the dialogs and results the pictures show are podman's own. The relay is a door out of the
sandbox while it runs, so it lets only a few calls through and changes two things:

- only what the app asks to see and run: `run`, `kill`, `info`, `version`, `images`, `ps`,
  `inspect` and `image inspect`/`exists`/`ls`; never a pull or a removal, which would change the
  user's own image store (pull what the pictures need yourself, and remove it after);
- `run` gets `--pull=never`: an image that is not there fails rather than being pulled;
- a mount's source under /home/demo (the sandbox's home) becomes the same folder in
  <demo-home>; a mount of anything else, or a `-v`, is refused.

The socket must be where the sandbox sees it: cap.py binds its own XDG_RUNTIME_DIR
(<tmp>/coxswain-cap-<uid>) into the sandbox at the same path.

    docs/screenshots/podman-relay.py serve /tmp/coxswain-cap-$(id -u)/podman-relay.sock "$DEMO_HOME" &
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


ALLOWED = {"run", "kill", "info", "version", "images", "ps", "inspect"}
IMAGE_ALLOWED = {"inspect", "exists", "ls"}


def outside(args, demo):
    """`args` as podman outside gets them, or the reason they are refused."""
    verb = args[0] if args else ""
    if verb not in ALLOWED and not (verb == "image" and args[1:2] and args[1] in IMAGE_ALLOWED):
        return f"podman-relay: {' '.join(args[:2])} is not let through"
    out = []
    for a in args:
        if a in ("-v", "--volume") or a.startswith(("-v=", "--volume=")):
            return "podman-relay: -v is not let through; the app mounts with --mount"
        if "source=" in a:
            # bind(): `type=bind,"source=/home/demo/…",target=/src`, the source one quoted field.
            head, _, rest = a.partition('"source=')
            src, quote, tail = rest.partition('"')
            if not quote or (tail and not tail.startswith(",")) or "source=" in head or not (src == SANDBOX_HOME or src.startswith(SANDBOX_HOME + "/")):
                return f"podman-relay: a mount outside {SANDBOX_HOME} is not let through: {a}"
            real = os.path.realpath(demo + src[len(SANDBOX_HOME):])
            if real != demo and not real.startswith(demo + "/"):
                return f"podman-relay: {src} leads outside the demo home"
            a = f'{head}"source={real.replace(chr(34), chr(34) * 2)}"{tail}'
        out.append(a)
    if verb == "run":
        out.insert(1, "--pull=never")
    return out


def handle(conn, demo):
    fds = []
    with conn:
        try:
            msg, fds, _, _ = socket.recv_fds(conn, 1 << 20, 3)
            args = outside(json.loads(msg), demo) if len(fds) == 3 else "podman-relay: no standard streams"
            if isinstance(args, str):
                if fds[2:]:
                    os.write(fds[2], (args + "\n").encode())
                conn.sendall(b"125")
                return
            p = subprocess.Popen(["/usr/bin/podman", *args], stdin=fds[0], stdout=fds[1], stderr=fds[2])
        finally:
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
    old = os.umask(0o177)
    s.bind(path)
    os.umask(old)
    s.listen(16)
    while True:
        conn, _ = s.accept()
        threading.Thread(target=handle, args=(conn, demo), daemon=True).start()


def check():
    """`podman-relay.py check`: what is let through, and how."""
    demo = os.path.realpath(os.environ.get("TMPDIR", "/tmp"))
    m = lambda src: ["run", "--mount", f'type=bind,"source={src}",target=/src', "img"]
    assert outside(m("/home/demo/.cache/x"), demo) == ["run", "--pull=never", "--mount", f'type=bind,"source={demo}/.cache/x",target=/src', "img"]
    for bad in (m("/etc"), m("/home/demo/../../etc"), m('/home/demo/a""/../../../etc'), m("/home/demox"), ["run", "-v", "/:/x", "img"], ["pull", "img"], ["image", "rm", "img"], ["system", "reset"]):
        assert isinstance(outside(bad, demo), str), bad
    assert outside(["image", "inspect", "img"], demo) == ["image", "inspect", "img"]
    print("podman-relay: check passed")


if __name__ == "__main__":
    if sys.argv[1:2] == ["check"]:
        check()
    elif sys.argv[1:2] == ["serve"]:
        serve(sys.argv[2], sys.argv[3])
    else:
        client()
