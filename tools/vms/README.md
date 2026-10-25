# Test virtual machines

Four throwaway virtual machines to run Coxswain's tests on FreeBSD, the other BSDs and illumos
before CI does: each is made from the system's official image, unattended, and a branch's tests
run in it with one command. They run under KVM with user-mode networking and are reached only on
`127.0.0.1`.

| VM | Made from | ssh port | VNC | Desktop app |
| --- | --- | --- | --- | --- |
| FreeBSD 15.1 | `FreeBSD-15.1-RELEASE-amd64-BASIC-CLOUDINIT-ufs.raw.xz`, nuageinit | 2222 | :1 (5901) | builds; no X |
| OpenBSD 7.9 | `miniroot79.img`, autoinstall(8) | 2223 | :2 (5902) | builds; Xfce |
| NetBSD 10.1 | the official live image | 2224 | :3 (5903) | none |
| OmniOS r151058 | the official cloud image, cloud-init | 2225 | :4 (5904) | none |

Each has 4 vCPUs, 4 GB of memory and a sparse disk of 24 GB (FreeBSD 80 GB: it also builds the desktop app), user `test` with
passwordless sudo or doas, and Coxswain installed by its install script (`install/install-freebsd.sh` on FreeBSD,
`install/install-unix.sh` on the others).

## Contents

- [What it needs](#what-it-needs)
- [Settings](#settings)
- [Making a VM](#making-a-vm)
- [Running the tests in one](#running-the-tests-in-one)
- [In each VM's folder](#in-each-vms-folder)
- [Throwing one away](#throwing-one-away)

## What it needs

Linux with KVM (`/dev/kvm`), and `qemu-system-x86_64`, `socat`, `curl`, `openssl`,
`ssh-keygen`, `python3`, `xz` (FreeBSD), `zstd` and UEFI firmware (OVMF: `edk2-ovmf` or
`ovmf`; OmniOS), `mtools` (FreeBSD, OmniOS), `uv` (OpenBSD, to check the signature) and `rsync`
(for quicker copies). About 15 GB of disk per VM once its tests have run.

## Settings

From the environment, read when a VM is made (the ports are written into its scripts) and by
`test-in.sh`:

| Variable | Default | What |
| --- | --- | --- |
| `COXSWAIN_VMS` | `$XDG_DATA_HOME/coxswain-vms` (`~/.local/share/coxswain-vms`) | where the VMs and the downloaded images (`cache/`) are kept |
| `COXSWAIN_VM_PORT_BASE` | `2222` | FreeBSD's ssh port on `127.0.0.1`; OpenBSD, NetBSD and OmniOS take the next three |
| `COXSWAIN_VM_VNC_BASE` | `1` | FreeBSD's VNC display (port 5900 + display); the others take the next three |
| `COXSWAIN_VM_HTTP_PORT` | `8080` | the short-lived server on `127.0.0.1` that answers OpenBSD's installer |
| `OVMF_CODE`, `OVMF_VARS` | found | the UEFI firmware files, when they are not where Arch, Debian, Ubuntu or Fedora put them |

## Making a VM

```sh
tools/vms/rebuild/freebsd-15.1.sh     # or openbsd-7.9.sh, netbsd-10.1.sh, omnios.sh
```

Each run takes 10 to 20 minutes and needs no answers: it downloads the image into the cache once
and checks it (SHA-256; OpenBSD's signify signature too), installs, makes user `test` with a new
ssh key and a random password (both kept in the VM's folder), installs the packages and
Coxswain, checks, and leaves the VM stopped. A second run wipes the disk and does it again, with
the same key and password. It refuses to run while the VM is running, and refuses a folder it did
not make, so pointing `COXSWAIN_VMS` at a folder of your own never wipes it.

## Running the tests in one

```sh
tools/vms/test-in.sh <freebsd|openbsd|netbsd|omnios> [checkout] [-- cargo args]
```

| Part | What |
| --- | --- |
| `checkout` | the checkout to test, by default the current folder |
| `cargo args` | by default what CI runs there: `test -p coxswain-core -p coxswain -p coxswain-gui --locked` on FreeBSD and OpenBSD, `test -p coxswain-core -p coxswain --locked -j2` on NetBSD and OmniOS (with more jobs in 4 GB, rustc runs out of memory) |

It starts the VM when it is down (and stops it afterwards only then), copies the checkout to
`~/coxswain-src` in the VM without `target/`, `node_modules/` and `.git` (rsync, or tar when the
VM has none; `gui/dist` goes along when built, else an empty one lets the desktop app compile),
runs cargo there with its target folder in the VM (`~/coxswain-target`, so the next run is
incremental; cleared first when the VM's free disk is less than it), streams the output and exits
with cargo's status. As in CI, debug info is line tables only and the data size limit is raised
to the hard one (OpenBSD's soft limit makes rustc abort). Several VMs at once are fine;
one `test-in.sh` per VM at a time.

```sh
cd .worktrees/my-fix && ../../tools/vms/test-in.sh openbsd
tools/vms/test-in.sh netbsd . -- test -p coxswain-core --locked fs::
tools/vms/test-in.sh omnios . -- build -p coxswain --locked
```

`test-in.sh` uses a VM only through its folder's `start.sh`, `ssh.sh` and `stop.sh`, so a VM
made by hand works too, in a folder of that name (`freebsd-15.1`, `openbsd-7.9`, `netbsd-10.1`,
`omnios`) under `COXSWAIN_VMS`.

## In each VM's folder

| File | What |
| --- | --- |
| `start.sh` | starts it headless (qemu in the background); ssh answers after 30 to 60 seconds. `VM_DISPLAY=gtk ./start.sh` opens a window instead |
| `ssh.sh [command]` | ssh in as `test` with the folder's key |
| `stop.sh` | presses the ACPI power button, waits up to 90 seconds, then pulls the plug |
| `rebuild.sh` | makes it again, with the same settings |
| `password` | the password of `test` and `root` |
| `README.txt` | what is installed and how it was made |
| `verify.txt` | `uname`, cargo and `coxswain --version` from the end of the last rebuild |

## Throwing one away

`<folder>/stop.sh`, then delete the folder (or only its `.raw` disk); `rebuild.sh` or the script
in `tools/vms/rebuild/` brings it back. Delete `cache/` to free the downloaded images; a rebuild
downloads them again.
