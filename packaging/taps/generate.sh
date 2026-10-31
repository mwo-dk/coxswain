#!/usr/bin/env bash
# Write the Homebrew formula and cask for a released tag, from its SHA256SUMS asset.
#
#   packaging/taps/generate.sh v1.2.0 <homebrew-tap-dir>
set -euo pipefail
tag="$1" brew="$2"
version="${tag#v}"
repo="https://github.com/mwo-dk/coxswain"
base="$repo/releases/download/$tag"
desc="Norton Commander style file manager with Everything-speed search"

# Every sum goes into a variable before a heredoc uses it: set -e does not stop on a failed
# $(...) inside a heredoc, and a missing or empty sum must fail the run, not reach the tap.
sums=$(gh release download "$tag" -R mwo-dk/coxswain -p SHA256SUMS -O -)
sum() {
  local s
  s=$(awk -v f="$1" '{ n = $2; sub(/^\*/, "", n) } n == f { print $1 }' <<<"$sums")
  [[ $s =~ ^[0-9a-f]{64}$ ]] || { echo "no sha256 for $1 in SHA256SUMS" >&2; exit 1; }
  echo "$s"
}
s_mac_arm=$(sum "coxswain-terminal-$tag-aarch64-apple-darwin.tar.gz")
s_mac_x64=$(sum "coxswain-terminal-$tag-x86_64-apple-darwin.tar.gz")
s_lin_arm=$(sum "coxswain-terminal-$tag-aarch64-unknown-linux-musl.tar.gz")
s_lin_x64=$(sum "coxswain-terminal-$tag-x86_64-unknown-linux-musl.tar.gz")
d_arm=$(sum "Coxswain_${version}_aarch64.dmg")
d_x64=$(sum "Coxswain_${version}_x64.dmg")
d_appimage=$(sum "Coxswain_${version}_amd64.AppImage")

# No `version` line in the formula: Homebrew reads it from the URLs, and `brew audit` calls one
# redundant. Readers take it from the first release URL (taps job, release-check.yml).
mkdir -p "$brew/Formula"
url() { echo "$base/coxswain-terminal-$tag-$1.tar.gz"; }
cat > "$brew/Formula/coxswain.rb" <<RUBY
class Coxswain < Formula
  desc "$desc"
  homepage "$repo"
  license "MIT"

  on_macos do
    on_arm do
      url "$(url aarch64-apple-darwin)"
      sha256 "$s_mac_arm"
    end
    on_intel do
      url "$(url x86_64-apple-darwin)"
      sha256 "$s_mac_x64"
    end
  end

  on_linux do
    on_arm do
      url "$(url aarch64-unknown-linux-musl)"
      sha256 "$s_lin_arm"
    end
    on_intel do
      url "$(url x86_64-unknown-linux-musl)"
      sha256 "$s_lin_x64"
    end
  end

  def install
    bin.install "coxswain", "cox"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/coxswain --version")
  end
end
RUBY

# The desktop app, as a cask: the .dmg on macOS, the AppImage on Linux. Homebrew 7 wants
# declarative *_steps stanzas: the Ruby preflight/postflight blocks are deprecated and fail on
# macOS.
appimage="Coxswain_#{version}_amd64.AppImage"
mkdir -p "$brew/Casks"
cat > "$brew/Casks/coxswain-gui.rb" <<RUBY
cask "coxswain-gui" do
  version "$version"
  sha256 arm:          "$d_arm",
         intel:        "$d_x64",
         x86_64_linux: "$d_appimage"

  on_macos do
    arch arm: "aarch64", intel: "x64"

    url "$repo/releases/download/v#{version}/Coxswain_#{version}_#{arch}.dmg"

    app "Coxswain.app"
  end
  on_linux do
    url "$repo/releases/download/v#{version}/$appimage"

    depends_on arch: :x86_64

    binary "$appimage", target: "coxswain-gui"
  end

  name "Coxswain"
  desc "$desc (desktop app)"
  homepage "$repo"

  preflight_steps do
    on_linux do
      set_permissions "Coxswain_{{version}}_amd64.AppImage", "0755"
    end
  end

  # Not notarized: without the quarantine flag cleared, macOS says the app is damaged.
  postflight_steps do
    on_macos do
      run "/usr/bin/xattr", args: ["-cr", "{{appdir}}/Coxswain.app"]
    end
  end

  # The search helper runs from inside the bundle: stopped before an upgrade or uninstall
  # replaces it, or macOS kills it and reports a crash. A launchctl remove, not the uninstall
  # stanza's launchctl, which would delete the LaunchAgent and so turn off Start with my session; launchd loads it
  # again at login, and the next app start hands it back at once.
  uninstall_preflight_steps do
    on_macos do
      run "/bin/launchctl", args: ["remove", "dk.mwo.coxswain.index"], must_succeed: false
    end
  end

  # Config and state in the coxswain config folder stay: the terminal app shares them.
  zap trash: [
    "~/Library/Caches/coxswain",
    "~/Library/WebKit/dk.mwo.coxswain",
  ]
end
RUBY
