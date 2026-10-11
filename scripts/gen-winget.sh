#!/usr/bin/env bash
# Writes the winget manifests for one Windows release (D-1715) and validates
# them against winget's JSON schemas (packaging/winget/schemas).
#
#   bash scripts/gen-winget.sh <installer.exe> <public URL of that installer> [release date YYYY-MM-DD]
#
# The URL must be where the published, signed NSIS installer lives (the
# public tandem-releases repository); winget downloads it and checks the
# SHA-256 written here.
set -euo pipefail

installer="${1:?usage: gen-winget.sh <installer.exe> <url> [date]}"
url="${2:?usage: gen-winget.sh <installer.exe> <url> [date]}"
date="${3:-$(date -u +%Y-%m-%d)}"
root="$(cd "$(dirname "$0")/.." && pwd)"
conf="$root/apps/desktop/src-tauri/tauri.conf.json"
version="$(node -p "require('$conf').version")"
scheme="$(node -p "require('$conf').plugins['deep-link'].desktop.schemes[0]")"
sha="$(sha256sum "$installer" | cut -d' ' -f1 | tr '[:lower:]' '[:upper:]')"
case "$url" in https://*) ;; *) echo "gen-winget: the installer URL must be https" >&2; exit 2 ;; esac

id="Vibezeo.Vibezeo"
dir="$root/dist/winget/manifests/v/Vibezeo/Vibezeo/$version"
mkdir -p "$dir"
header='# yaml-language-server: $schema=https://aka.ms/winget-manifest'

cat >"$dir/$id.yaml" <<YAML
$header.version.1.9.0.schema.json
PackageIdentifier: $id
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.9.0
YAML

cat >"$dir/$id.installer.yaml" <<YAML
$header.installer.1.9.0.schema.json
PackageIdentifier: $id
PackageVersion: $version
InstallerType: nullsoft
Scope: user
InstallModes:
  - interactive
  - silent
UpgradeBehavior: install
Protocols:
  - $scheme
ReleaseDate: $date
Installers:
  - Architecture: x64
    InstallerUrl: $url
    InstallerSha256: $sha
ManifestType: installer
ManifestVersion: 1.9.0
YAML

cat >"$dir/$id.locale.en-US.yaml" <<YAML
$header.defaultLocale.1.9.0.schema.json
PackageIdentifier: $id
PackageVersion: $version
PackageLocale: en-US
Publisher: Vibezeo
PublisherUrl: https://vibezeo.com
PrivacyUrl: https://vibezeo.com/privacy
PackageName: Vibezeo
PackageUrl: https://vibezeo.com/download
License: Proprietary
ShortDescription: A desktop workspace for coding agents
Moniker: vibezeo
Tags:
  - coding-agents
  - terminal
  - developer-tools
ReleaseNotesUrl: https://vibezeo.com/changelog/$version/
ManifestType: defaultLocale
ManifestVersion: 1.9.0
YAML

python3 -I "$root/scripts/check-winget.py" "$dir" "$root/packaging/winget/schemas"
echo "winget: $dir"
