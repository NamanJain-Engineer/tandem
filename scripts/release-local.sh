#!/usr/bin/env bash
# Reproduces the release workflow's no-secrets path locally (F-TL-04 AC1,
# ARCHITECTURE §17.4): verify, build the Linux bundles, mark them unsigned,
# SBOMs, THIRD_PARTY_NOTICES.md, SHA-256 list and a summary in dist/.
#
#   bash scripts/release-local.sh --dry-run [--target x86_64-unknown-linux-gnu] [--skip-build]
#
# --dry-run: every signing secret is ignored (the fork path) and nothing is
#            published; a model sha256 still TBD is a warning, not a failure.
# --skip-build: reuse the bundles already in target/<target>/release/bundle.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
dist="$root/dist"
target="x86_64-unknown-linux-gnu"
dry=0
build=1
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) dry=1 ;;
    --skip-build) build=0 ;;
    --target) target="$2"; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done

secrets=(TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID
  AZURE_TENANT_ID AZURE_CLIENT_ID AZURE_CLIENT_SECRET AZURE_SIGNING_ENDPOINT AZURE_SIGNING_ACCOUNT AZURE_CERT_PROFILE
  LINUX_GPG_PRIVATE_KEY LINUX_GPG_PASSPHRASE RELEASE_API_TOKEN)
if [ "$dry" = 1 ]; then
  for s in "${secrets[@]}"; do unset "$s"; done
fi
has() { [ -n "${!1:-}" ]; }

rm -rf "$dist"
mkdir -p "$dist"
summary="$dist/RELEASE_SUMMARY.md"
note() { echo "$*" | tee -a "$summary"; }
: >"$summary"
note "# Release ${dry:+(dry run) }for $target"
note ""

# 1. verify: one version everywhere, a changelog section, pinned models.
version="$(node -p "require('$root/apps/desktop/src-tauri/tauri.conf.json').version")"
pkg="$(node -p "require('$root/apps/desktop/package.json').version")"
cargo_v="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
if [ "$version" != "$pkg" ] || [ "$version" != "$cargo_v" ]; then
  echo "version mismatch: tauri.conf.json $version, package.json $pkg, Cargo.toml $cargo_v" >&2
  exit 1
fi
# F-WB-03 AC1: CHANGELOG.md's format holds and this version has a section.
if ! (cd "$root" && pnpm exec tsx scripts/check-changelog.ts --version "$version" >/dev/null); then
  exit 1
fi
if grep -q '"TBD"' "$root/crates/tandem-voice/models/catalog.json"; then
  if [ "$dry" = 1 ]; then
    note "- warning: model sha256 values are still TBD (a real release refuses this; pinned by scripts/pin-models.ts in M12)"
  else
    echo "model sha256 values are still TBD" >&2
    exit 1
  fi
fi
note "- version $version"

# 2. third-party notices first: the build embeds them (About › Licenses).
(cd "$root" && pnpm tsx scripts/gen-notices.ts --out "$dist" --resources)

# 3. build the Linux bundles.
bundle="$root/target/$target/release/bundle"
if [ "$build" = 1 ]; then
  # tandem-host for SSH hosts (F-RM-01): static Linux x86_64 and aarch64
  # (cargo-zigbuild), with their SHA-256 manifest. A macOS helper needs a Mac
  # (release.yml builds it); this download offers Linux hosts only.
  hostdir="$root/apps/desktop/src-tauri/resources/tandem-host"
  find "$hostdir" -mindepth 1 -maxdepth 1 -type d -exec rm -rf {} +
  rm -f "$hostdir/manifest.json"
  (cd "$root" && sh scripts/ci/build-tandem-host.sh linux-x86_64 && sh scripts/ci/build-tandem-host.sh linux-aarch64)
  (cd "$root" && TANDEM_HOST_PREBUILT=1 sh scripts/build-tandem-host.sh >/dev/null)
  (cd "$root" && pnpm --filter desktop build)
  # The phone web app LAN mode serves at /remote (F-RM-04).
  (cd "$root" && sh scripts/build-remote-web.sh)
  # The tasks MCP server ships beside the app (F-TK-02, bundle.externalBin).
  sh "$root/scripts/build-sidecars.sh" "$target"
  # AppImage tooling runs without FUSE in containers.
  (cd "$root/apps/desktop" && APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=true pnpm tauri build --target "$target" --bundles appimage,deb,rpm --config src-tauri/tauri.sidecars.conf.json)
fi

# 4. signing, each only with its secrets; otherwise the artifact says so.
suffix=""
if has LINUX_GPG_PRIVATE_KEY; then
  note "- Linux: AppImage, deb and rpm signed with the release GPG key"
else
  suffix="-unsigned"
  note "- unsigned: Linux AppImage, deb and rpm (missing LINUX_GPG_PRIVATE_KEY, LINUX_GPG_PASSPHRASE)"
fi
found=0
# Only this version's bundles: an earlier release's can still be in the folder.
for f in "$bundle"/appimage/*_"$version"_*.AppImage "$bundle"/deb/*_"$version"_*.deb "$bundle"/rpm/*-"$version"-*.rpm; do
  [ -e "$f" ] || continue
  found=1
  name="$(basename "$f")"
  out="$dist/${name%.*}$suffix.${name##*.}"
  cp "$f" "$out"
  if has LINUX_GPG_PRIVATE_KEY; then
    gpg --batch --pinentry-mode loopback --passphrase "${LINUX_GPG_PASSPHRASE:-}" --armor --detach-sign "$out"
  fi
done
if [ "$found" = 0 ]; then
  echo "no bundles under $bundle (build them, or drop --skip-build)" >&2
  exit 1
fi
# Every bundle carries tandem-host, or SSH workspaces can't connect.
for f in "$bundle"/appimage/*_"$version"_*.AppImage "$bundle"/deb/*_"$version"_*.deb "$bundle"/rpm/*-"$version"-*.rpm; do
  [ -e "$f" ] || continue
  (cd "$root" && sh scripts/check-host-bundle.sh "$f" linux-x86_64,linux-aarch64) | sed "s|^|- $(basename "$f"): |" | tee -a "$summary"
done
# The sidecars sit beside the app (D-828): the tasks server and the
# engines' status hook. Without the hook, panes fall back to heuristics.
for bin in tandem-tasks tandem-hook; do
  for f in "$bundle"/deb/*_"$version"_*.deb; do
    [ -e "$f" ] || continue
    # No grep -q: it stops reading early, and pipefail counts dpkg-deb's broken pipe.
    dpkg-deb -c "$f" | grep "usr/bin/$bin\$" >/dev/null || { echo "$(basename "$f") has no $bin" >&2; exit 1; }
  done
  for f in "$bundle"/rpm/*-"$version"-*.rpm; do
    [ -e "$f" ] || continue
    # rpm reads the package itself: rpm2cpio | cpio -t breaks rpm2cpio's pipe at cpio's trailer.
    rpm -qlp "$f" | grep -x "/usr/bin/$bin" >/dev/null || { echo "$(basename "$f") has no $bin" >&2; exit 1; }
  done
  if [ -d "$bundle/appimage/Vibezeo.AppDir" ] && [ ! -x "$bundle/appimage/Vibezeo.AppDir/usr/bin/$bin" ]; then
    echo "the AppImage has no $bin" >&2
    exit 1
  fi
done
note "- sidecars in every bundle: tandem-tasks, tandem-hook"
if has TAURI_SIGNING_PRIVATE_KEY; then
  note "- updater: artifacts signed; latest.json written"
else
  note "- unsigned: no update artifacts or latest.json (missing TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD)"
fi
note "- macOS and Windows builds run on their own runners (release.yml); not built here"

# 5. SBOMs.
(cd "$root" && cargo cyclonedx --format json --spec-version 1.5 --override-filename tandem-rust.cdx >/dev/null)
find "$root/apps/desktop/src-tauri" -maxdepth 1 -name 'tandem-rust.cdx.json' -exec mv {} "$dist/tandem-rust.cdx.json" \;
find "$root/crates" "$root/tests" -name 'tandem-rust.cdx.json' -delete 2>/dev/null || true
(cd "$root" && pnpm tsx scripts/gen-sbom-npm.ts --out "$dist/tandem-npm.cdx.json")
note "- SBOMs: tandem-rust.cdx.json, tandem-npm.cdx.json (CycloneDX 1.5); THIRD_PARTY_NOTICES.md (embedded in the build)"

# 6. checksums.
(cd "$dist" && sha256sum -- *.AppImage *.deb *.rpm *.cdx.json THIRD_PARTY_NOTICES.md 2>/dev/null >SHA256SUMS)
if [ "$dry" = 1 ]; then
  note "- dry run: nothing published"
fi
note ""
note "Artifacts:"
(cd "$dist" && ls -1 | sed 's/^/- /') | tee -a "$summary"
