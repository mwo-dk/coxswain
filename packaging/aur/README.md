# AUR packages

**Not on the AUR yet:** the AUR's account registration is paused, so `AUR_SSH_PRIVATE_KEY` is
not set and the release job only prints a notice. Until then, on Arch: clone the repository and
run `makepkg -si` in `packaging/aur/coxswain` (from source) or `packaging/aur/coxswain-bin`
(the release's binaries).

- **`coxswain`** builds from the tagged source.
- **`coxswain-bin`** repackages the release's prebuilt Linux binaries.

`release.yml` bumps `pkgver`, updates checksums and pushes both to the AUR after each release.
It needs a one-time setup:

1. Have an account at <https://aur.archlinux.org>.
2. Make a key just for this: `ssh-keygen -t ed25519 -f ~/.ssh/aur -N '' -C coxswain-aur`
3. Paste `~/.ssh/aur.pub` into *My Account → SSH Public Key* on the AUR.
4. Give the private key to GitHub: `gh secret set AUR_SSH_PRIVATE_KEY -R mwo-dk/coxswain < ~/.ssh/aur`
5. Re-run the latest Release workflow (Actions → Release → Run workflow), or wait for the next
   release. The first push creates the packages on the AUR.
