#!/usr/bin/env bash
# Write the winget manifests for the terminal app (mwo-dk.Coxswain.Terminal) for a released tag.
# Not `komac update`: the exe's path inside the zip contains the version, and an update would
# carry the previous version's path forward.
#
#   packaging/winget/terminal.sh v1.4.0 <out-dir>
set -euo pipefail
tag="$1" out="$2"
version="${tag#v}"
name="coxswain-terminal-$tag-x86_64-pc-windows-msvc"
sha=$(gh release download "$tag" -R mwo-dk/coxswain -p "$name.zip.sha256" -O - | cut -d' ' -f1 | tr a-f A-F)
dir="$out/manifests/m/mwo-dk/Coxswain/Terminal/$version"
mkdir -p "$dir"
id=mwo-dk.Coxswain.Terminal
head() { printf '# yaml-language-server: $schema=https://aka.ms/winget-manifest.%s.1.12.0.schema.json\n\nPackageIdentifier: %s\nPackageVersion: %s\n' "$1" "$id" "$version"; }

{ head version; printf 'DefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.12.0\n'; } > "$dir/$id.yaml"

{ head installer; cat <<YAML
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
- RelativeFilePath: $name\\coxswain.exe
  PortableCommandAlias: coxswain
ReleaseDate: $(date -u +%F)
Installers:
- Architecture: x64
  InstallerUrl: https://github.com/mwo-dk/coxswain/releases/download/$tag/$name.zip
  InstallerSha256: $sha
ManifestType: installer
ManifestVersion: 1.12.0
YAML
} > "$dir/$id.installer.yaml"

{ head defaultLocale; cat <<YAML
PackageLocale: en-US
Publisher: Michael W. Olesen
PublisherUrl: https://github.com/mwo-dk
PublisherSupportUrl: https://github.com/mwo-dk/coxswain/issues
Author: Michael W. Olesen
PackageName: Coxswain (terminal)
PackageUrl: https://github.com/mwo-dk/coxswain
License: MIT
LicenseUrl: https://github.com/mwo-dk/coxswain/blob/master/LICENSE
ShortDescription: Norton Commander style file manager with Everything-speed search, in the terminal
Description: 'A two-panel file manager in the Norton Commander tradition, built for developers: instant search across every file on the machine and git status in every panel. The desktop app is mwo-dk.Coxswain.'
Moniker: coxswain
Tags:
- everything
- file-manager
- git
- norton-commander
- orthodox-file-manager
- search
- terminal
- tui
ReleaseNotesUrl: https://github.com/mwo-dk/coxswain/releases/tag/$tag
ManifestType: defaultLocale
ManifestVersion: 1.12.0
YAML
} > "$dir/$id.locale.en-US.yaml"
