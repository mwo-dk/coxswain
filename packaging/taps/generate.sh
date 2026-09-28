#!/usr/bin/env bash
# Write the Homebrew formula and the Scoop manifest for a released tag, from its .sha256 assets.
#
#   packaging/taps/generate.sh v1.2.0 <homebrew-tap-dir> <scoop-bucket-dir>
set -euo pipefail
tag="$1" brew="$2" scoop="$3"
version="${tag#v}"
repo="https://github.com/mwo-dk/bosum"
base="$repo/releases/download/$tag"
desc="Norton Commander style file manager with Everything-speed search"

sha() { gh release download "$tag" -R mwo-dk/bosum -p "bosum-terminal-$tag-$1.$2.sha256" -O - | cut -d' ' -f1; }

mkdir -p "$brew/Formula" "$scoop/bucket"
url() { echo "$base/bosum-terminal-$tag-$1.tar.gz"; }
cat > "$brew/Formula/bosum.rb" <<RUBY
class Bosum < Formula
  desc "$desc"
  homepage "$repo"
  version "$version"
  license "MIT"

  on_macos do
    on_arm do
      url "$(url aarch64-apple-darwin)"
      sha256 "$(sha aarch64-apple-darwin tar.gz)"
    end
    on_intel do
      url "$(url x86_64-apple-darwin)"
      sha256 "$(sha x86_64-apple-darwin tar.gz)"
    end
  end

  on_linux do
    on_arm do
      url "$(url aarch64-unknown-linux-musl)"
      sha256 "$(sha aarch64-unknown-linux-musl tar.gz)"
    end
    on_intel do
      url "$(url x86_64-unknown-linux-musl)"
      sha256 "$(sha x86_64-unknown-linux-musl tar.gz)"
    end
  end

  def install
    bin.install "bosum"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/bosum --version")
  end
end
RUBY

win="x86_64-pc-windows-msvc"
cat > "$scoop/bucket/bosum.json" <<JSON
{
    "version": "$version",
    "description": "$desc",
    "homepage": "$repo",
    "license": "MIT",
    "architecture": {
        "64bit": {
            "url": "$base/bosum-terminal-$tag-$win.zip",
            "hash": "$(sha $win zip)",
            "extract_dir": "bosum-terminal-$tag-$win"
        }
    },
    "bin": "bosum.exe",
    "checkver": "github",
    "autoupdate": {
        "architecture": {
            "64bit": {
                "url": "$repo/releases/download/v\$version/bosum-terminal-v\$version-$win.zip",
                "hash": { "url": "\$url.sha256" },
                "extract_dir": "bosum-terminal-v\$version-$win"
            }
        }
    }
}
JSON
