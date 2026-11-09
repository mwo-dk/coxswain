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
  user's own image store (pull what the pictures need yourself, and remove after only what you pulled);
- `run` takes only the options the app gives (`RUN_FLAGS`, `RUN_VALUES`), up to the image, and
  gets `--pull=never` (any other `--pull` is refused): an image that is not there fails rather
  than being pulled; `kill` takes one container's name;
- a bind mount's source under /home/demo (the sandbox's home) becomes the same folder in
  <demo-home>; any other mount, or a `-v`, is refused.

Calls of screens no picture needs are refused too: Tika's server (`run -d`), the virus scan's
(`--userns=keep-id`), and Settings' disk use and removal (`system df`, `image rm`).

The socket must be where the sandbox sees it: cap.py binds its own XDG_RUNTIME_DIR
(<tmp>/coxswain-cap-<uid>) into the sandbox at the same path.

    docs/screenshots/podman-relay.py serve /tmp/coxswain-cap-$(id -u)/podman-relay.sock "$DEMO_HOME" &
    SANDBOX_ARGS="--ro-bind docs/screenshots/podman-relay.py /usr/bin/podman" uv run my-shots.py

Stop the relay by its own PID when done.
"""

import csv, io, json, os, signal, socket, subprocess, sys, threading

SANDBOX_HOME = "/home/demo"


def client():
    s = socket.socket(socket.AF_UNIX)
    s.connect(os.path.join(os.environ["XDG_RUNTIME_DIR"], "podman-relay.sock"))
    socket.send_fds(s, [json.dumps(sys.argv[1:]).encode()], [0, 1, 2])
    code = s.recv(16)
    sys.exit(int(code) if code else 1)


ALLOWED = {"run", "kill", "info", "version", "images", "ps", "inspect"}
IMAGE_ALLOWED = {"inspect", "exists", "ls"}
# `run`'s options as the app gives them (images.rs `sealed`, `TIGHT`, `user`; ebook.rs,
# searchable.rs, viruses.rs …), up to the image: those alone, with what each may say.
RUN_FLAGS = {"--rm", "-i", "--network=none", "--read-only", "--cap-drop=ALL", "--pull=never"}
RUN_PREFIXES = ("--memory=", "--pids-limit=")
RUN_VALUES = {
    "--name": lambda v: not v.startswith("-"),
    "--security-opt": lambda v: v in ("label=disable", "no-new-privileges"),
    "--tmpfs": lambda v: v.startswith("/tmp:"),
    "--user": lambda v: not v.startswith("-"),
    "-e": lambda v: "=" in v and not v.startswith("-"),
    "--entrypoint": lambda v: not v.startswith("-"),
    "-w": lambda v: v.startswith("/"),
    "--mount": None,
}


def mount(value, demo):
    """A `--mount` of a folder under /home/demo, as `images::bind` writes it, with its source in
    `demo`; or the reason it is refused. Read as podman reads it: CSV fields."""
    try:
        fields = next(csv.reader([value]))
    except csv.Error:
        return None
    kinds = [f for f in fields if f.split("=", 1)[0] == "type"]
    sources = [f for f in fields if f.split("=", 1)[0] in ("source", "src")]
    known = ("type", "source", "src", "target", "dst", "destination", "readonly", "ro")
    if kinds != ["type=bind"] or len(sources) != 1 or any(f.split("=", 1)[0] not in known for f in fields):
        return None
    src = sources[0].split("=", 1)[1]
    if not (src == SANDBOX_HOME or src.startswith(SANDBOX_HOME + "/")):
        return None
    # shortcut: the app owns the demo home and could swap a folder for a link after this check;
    # fine for our own app, not for one we do not trust.
    real = os.path.realpath(demo + src[len(SANDBOX_HOME):])
    if real != demo and not real.startswith(demo + "/"):
        return None
    out = io.StringIO()
    csv.writer(out, lineterminator="").writerow([f if f not in sources else "source=" + real for f in fields])
    return out.getvalue()


def outside(args, demo):
    """`args` as podman outside gets them, or the reason they are refused."""
    verb = args[0] if args else ""
    if verb not in ALLOWED and not (verb == "image" and args[1:2] and args[1] in IMAGE_ALLOWED):
        return f"podman-relay: {' '.join(args[:2])} is not let through"
    if verb == "kill":
        if len(args) != 2 or args[1].startswith("-"):
            return "podman-relay: kill takes one container's name"
        return list(args)
    if verb != "run":
        return list(args)
    out, i = ["run", "--pull=never"], 1
    while i < len(args) and args[i].startswith("-"):
        a = args[i]
        if a in RUN_FLAGS or a.startswith(RUN_PREFIXES):
            if a != "--pull=never":
                out.append(a)
            i += 1
            continue
        flag, eq, value = a.partition("=")
        if flag not in RUN_VALUES or (not eq and i + 1 >= len(args)):
            return f"podman-relay: run {a} is not let through"
        if not eq:
            value, i = args[i + 1], i + 1
        if flag == "--mount":
            value = mount(value, demo)
            if value is None:
                return f"podman-relay: only a bind mount of a folder under {SANDBOX_HOME} is let through: {a}"
        elif not RUN_VALUES[flag](value):
            return f"podman-relay: run {flag} {value} is not let through"
        out += [flag, value]
        i += 1
    if i >= len(args):
        return "podman-relay: run without an image"
    return out + args[i:]


def handle(conn, demo):
    fds = []
    with conn:
        try:
            msg, fds, _, _ = socket.recv_fds(conn, 1 << 20, 3)
            got = json.loads(msg)
            if not (isinstance(got, list) and all(isinstance(a, str) for a in got)):
                got = ["(not a list of words)"]
            args = outside(got, demo) if len(fds) == 3 else "podman-relay: no standard streams"
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
            code = p.wait()
            # Ended by a signal: as a shell says it, 128 + the signal.
            conn.sendall(str(code if code >= 0 else 128 - code).encode())
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
    assert outside(m("/home/demo/.cache/x"), demo) == ["run", "--pull=never", "--mount", f"type=bind,source={demo}/.cache/x,target=/src", "img"]
    odd = outside(m('/home/demo/a,b""c'), demo)
    assert odd[3] == f'type=bind,"source={demo}/a,b""c",target=/src', odd
    app = ["run", "--rm", "-i", "--network=none", "--security-opt", "label=disable", "--name", "n", "--read-only", "--tmpfs", "/tmp:rw,size=1g",
           "--memory=3g", "--pids-limit=256", "--cap-drop=ALL", "--security-opt", "no-new-privileges", "--mount", 'type=bind,"source=/home/demo/x",target=/src,readonly',
           "--user", "0:0", "-e", "HOME=/tmp", "-w", "/src", "img", "--skip-text", "-v", "/src/a.pdf"]
    got = outside(app, demo)
    assert isinstance(got, list) and got[-4:] == ["img", "--skip-text", "-v", "/src/a.pdf"], got
    for bad in (m("/etc"), m("/home/demo/../../etc"), m('/home/demo/a""/../../../etc'), m("/home/demox"),
                ["run", "-v", "/:/x", "img"], ["run", "-v/:/x", "img"], ["run", "-iv", "/:/x", "img"], ["run", "--volume=/:/x", "img"],
                ["run", "--mount", "type=bind,src=/,dst=/host", "img"], ["run", "--mount=type=glob,src=/home/demo/*,dst=/k", "img"],
                ["run", "--mount", 'type=bind,"source=/home/demo/x",target=/src,src=/etc', "img"], ["run", "--mount", "type=volume,src=v,dst=/v", "img"],
                ["run", "--rootfs", "/", "cat", "/x"], ["run", "--env-file", "/etc/passwd", "img"], ["run", "--cidfile", "/tmp/x", "img"],
                ["run", "--privileged", "img"], ["run", "--device", "/dev/sda", "img"], ["run", "--volumes-from", "c", "img"], ["run", "--pid=host", "img"],
                ["run", "--pull=always", "img"], ["run", "--pull", "always", "img"], ["run", "--network=host", "img"], ["run", "--security-opt", "seccomp=unconfined", "img"],
                ["run", "--rm"], ["kill", "--all"], ["kill", "-a"], ["kill", "--cidfile", "/x"], ["kill", "a", "b"],
                ["--url", "x", "run", "img"], ["pull", "img"], ["image", "rm", "img"], ["image", "pull", "img"], ["system", "reset"]):
        assert isinstance(outside(bad, demo), str), bad
    assert outside(["run", "--pull=never", "img"], demo) == ["run", "--pull=never", "img"]
    assert outside(["kill", "n"], demo) == ["kill", "n"]
    assert isinstance(outside(["run", "--mount", "type=bind,source=/home/demo/a\nb", "img"], demo), str)
    assert outside(["image", "inspect", "img"], demo) == ["image", "inspect", "img"]
    print("podman-relay: check passed")


if __name__ == "__main__":
    # Inside the sandbox, as /usr/bin/podman, every call is relayed.
    if os.path.basename(sys.argv[0]) == "podman":
        client()
    elif sys.argv[1:2] == ["check"]:
        check()
    elif sys.argv[1:2] == ["serve"]:
        serve(sys.argv[2], sys.argv[3])
    else:
        client()
