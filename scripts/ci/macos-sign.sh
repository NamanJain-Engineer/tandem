#!/usr/bin/env bash
# macOS signing, notarization and checks for release builds (D-2102). Used by
# .github/workflows/release.yml and codemagic.yaml; runs on a Mac only.
# Every credential comes from the environment (CI secrets), never the repo.
#
#   macos-sign.sh identity
#       Prints the signing identity: a Developer ID from APPLE_CERTIFICATE
#       (base64 .p12) and APPLE_CERTIFICATE_PASSWORD, imported into a
#       throwaway keychain; "-" (ad-hoc) when there's no certificate.
#   macos-sign.sh sign IDENTITY FILE...
#       Signs each executable with the hardened runtime and a secure
#       timestamp (notarization rejects any Mach-O without them). Run it on
#       the tandem-host helper and both sidecars before the app is bundled:
#       Tauri signs the app's own executables, not resources.
#   macos-sign.sh notarize FILE
#       Submits FILE (.dmg, .zip or .pkg) with notarytool, waits, prints
#       Apple's log on rejection, and staples the ticket.
#   macos-sign.sh verify [--notarized] APP [DMG]
#       Every Mach-O in APP is universal (arm64 and x86_64) and signed; with
#       a Developer ID, each one has the hardened runtime, a timestamp and
#       APPLE_TEAM_ID. With --notarized, Gatekeeper accepts APP (and DMG) and
#       their tickets are stapled. Prints the evidence the release attaches.
#
# Notarization credentials, either:
#   APPLE_API_ISSUER, APPLE_API_KEY and APPLE_API_KEY_PATH (an App Store
#   Connect API key, .p8), or
#   APPLE_ID, APPLE_PASSWORD (an app-specific password) and APPLE_TEAM_ID.
set -euo pipefail

die() { echo "macos-sign: $*" >&2; exit 1; }
[ "$(uname -s)" = Darwin ] || die "runs on macOS only"

# Sets `auth` to notarytool's credential flags; fails without credentials.
# (No mapfile: macOS ships bash 3.2.)
notary_auth() {
  if [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY_PATH:-}" ]; then
    auth=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER")
  elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
    auth=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
  else
    return 1
  fi
}

# Every Mach-O file under a path (executables, dylibs, helpers).
macho_files() {
  find "$1" -type f -print0 | while IFS= read -r -d '' f; do
    if file -b "$f" | grep -q 'Mach-O'; then printf '%s\n' "$f"; fi
  done
}

cmd="${1:-}"
shift || true
case "$cmd" in
  identity)
    if [ -z "${APPLE_CERTIFICATE:-}" ]; then
      echo "-"
      exit 0
    fi
    [ -n "${APPLE_CERTIFICATE_PASSWORD:-}" ] || die "APPLE_CERTIFICATE_PASSWORD is missing"
    tmp="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
    keychain="$tmp/vibezeo-signing.keychain-db"
    pass="$(openssl rand -hex 24)"
    if [ ! -f "$keychain" ]; then
      security create-keychain -p "$pass" "$keychain" >&2
      security set-keychain-settings -lut 21600 "$keychain" >&2
      security unlock-keychain -p "$pass" "$keychain" >&2
      echo "$APPLE_CERTIFICATE" | base64 --decode >"$tmp/vibezeo-cert.p12"
      security import "$tmp/vibezeo-cert.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" \
        -T /usr/bin/codesign -T /usr/bin/security >&2
      rm -f "$tmp/vibezeo-cert.p12"
      security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$pass" "$keychain" >/dev/null
      # Searched first, keeping the login keychain for everything else.
      # shellcheck disable=SC2046
      security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
    fi
    if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
      echo "$APPLE_SIGNING_IDENTITY"
    else
      id="$(security find-identity -v -p codesigning "$keychain" | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -1)"
      [ -n "$id" ] || die "no Developer ID Application identity in APPLE_CERTIFICATE"
      echo "$id"
    fi
    ;;

  sign)
    identity="${1:?usage: macos-sign.sh sign IDENTITY FILE...}"
    shift
    [ $# -gt 0 ] || die "nothing to sign"
    for f in "$@"; do
      [ -f "$f" ] || die "$f doesn't exist"
      if [ "$identity" = "-" ]; then
        # Ad-hoc: Apple silicon runs nothing unsigned. No timestamp server.
        codesign --force --options runtime --sign - "$f"
      else
        codesign --force --options runtime --timestamp --sign "$identity" "$f"
      fi
      codesign --verify --strict "$f"
      echo "signed: $f"
    done
    ;;

  notarize)
    f="${1:?usage: macos-sign.sh notarize FILE}"
    auth=()
    notary_auth || die "no notarization credentials (APPLE_API_* or APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID)"
    out="$(xcrun notarytool submit "$f" "${auth[@]}" --wait --timeout 30m --output-format json)"
    echo "$out"
    status="$(echo "$out" | plutil -extract status raw -o - - 2>/dev/null || true)"
    id="$(echo "$out" | plutil -extract id raw -o - - 2>/dev/null || true)"
    if [ "$status" != "Accepted" ]; then
      if [ -n "$id" ]; then xcrun notarytool log "$id" "${auth[@]}" || true; fi
      die "notarization of $f: ${status:-no status}"
    fi
    xcrun stapler staple "$f"
    xcrun stapler validate "$f"
    ;;

  verify)
    notarized=0
    if [ "${1:-}" = "--notarized" ]; then notarized=1; shift; fi
    app="${1:?usage: macos-sign.sh verify [--notarized] APP [DMG]}"
    dmg="${2:-}"
    [ -d "$app" ] || die "$app isn't an app bundle"
    fail=0
    team=""
    developer_id=0
    if codesign -dv "$app" 2>&1 | grep -q '^Authority=Developer ID Application'; then
      developer_id=1
      team="$(codesign -dv "$app" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
      if [ -n "${APPLE_TEAM_ID:-}" ] && [ "$team" != "$APPLE_TEAM_ID" ]; then
        echo "FAIL $app: team $team, expected $APPLE_TEAM_ID"; fail=1
      fi
    fi
    echo "== Mach-O files in $(basename "$app")"
    while IFS= read -r f; do
      rel="${f#"$app"/}"
      archs="$(lipo -archs "$f" 2>/dev/null || echo '?')"
      case " $archs " in
        *" arm64 "*" x86_64 "* | *" x86_64 "*" arm64 "*) ;;
        *) echo "FAIL $rel: architectures '$archs' (a universal build has arm64 and x86_64)"; fail=1 ;;
      esac
      if ! codesign --verify --strict "$f" 2>/dev/null; then
        echo "FAIL $rel: not signed"; fail=1; continue
      fi
      info="$(codesign -dv --verbose=4 "$f" 2>&1)"
      if [ "$developer_id" = 1 ]; then
        echo "$info" | grep -q 'flags=.*runtime' || { echo "FAIL $rel: no hardened runtime"; fail=1; }
        echo "$info" | grep -q '^Timestamp=' || { echo "FAIL $rel: no secure timestamp"; fail=1; }
        echo "$info" | grep -q "^TeamIdentifier=$team\$" || { echo "FAIL $rel: another team"; fail=1; }
      fi
      echo "ok   $rel ($archs)"
    done < <(macho_files "$app")
    echo "== codesign --verify --deep --strict"
    codesign --verify --deep --strict --verbose=2 "$app" || fail=1
    echo "== entitlements"
    codesign -d --entitlements - --xml "$app" 2>/dev/null | plutil -p - || true
    if [ "$notarized" = 1 ]; then
      [ "$developer_id" = 1 ] || { echo "FAIL $app: --notarized, but not signed with a Developer ID"; fail=1; }
      echo "== Gatekeeper"
      spctl -a -vv -t exec "$app" 2>&1 || fail=1
      xcrun stapler validate "$app" || fail=1
      if [ -n "$dmg" ]; then
        spctl -a -vv -t open --context context:primary-signature "$dmg" 2>&1 || fail=1
        xcrun stapler validate "$dmg" || fail=1
      fi
    elif [ "$developer_id" = 1 ]; then
      echo "== signed, not notarized: Gatekeeper blocks it until it is"
    else
      echo "== ad-hoc signed: Gatekeeper will ask the user to allow it (Privacy & Security, Open Anyway)"
    fi
    [ "$fail" = 0 ] || die "verification failed"
    echo "verified: $app${dmg:+ and $dmg}"
    ;;

  *)
    die "usage: macos-sign.sh identity | sign IDENTITY FILE... | notarize FILE | verify [--notarized] APP [DMG]"
    ;;
esac
