#!/bin/sh
# Installs a release .rpm in a clean Fedora container (D-832). It checks that
# dnf resolves the package's dependencies and that every binary finds its
# libraries, then starts the app with a fresh home on a virtual display. The
# screenshot (welcome.png) and logs land in $OUT. The site names only the
# Fedora releases this passes on.
#   sh scripts/agent/test-rpm-fedora.sh dist/Vibezeo-x.y.z-1.x86_64-unsigned.rpm [fedora release]
# In agent containers the only way out is the HTTPS proxy, so dnf goes through
# it with the proxy's CA, over https mirrors only.
set -eu
[ -n "${1:-}" ] || { echo "usage: test-rpm-fedora.sh <file.rpm> [release]" >&2; exit 2; }
RPM="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
VER="${2:-41}"
OUT="${OUT:-/tmp/rpm-fedora-$VER}"
rm -rf "$OUT"
mkdir -p "$OUT"
set -- --rm -v "$RPM:/pkg/vibezeo.rpm:ro" -v "$OUT:/out"
CA="${SSL_CERT_FILE:-/root/.ccr/ca-bundle.crt}"
if [ -n "${HTTPS_PROXY:-}" ] && [ -f "$CA" ]; then
  set -- "$@" --network host -e https_proxy="$HTTPS_PROXY" -e http_proxy="$HTTPS_PROXY" \
    -v "$CA:/etc/pki/ca-trust/source/anchors/proxy-ca.crt:ro"
fi
docker run "$@" "fedora:$VER" sh -c '
  set -e
  if [ -f /etc/pki/ca-trust/source/anchors/proxy-ca.crt ]; then
    update-ca-trust
    sed -i "s/^metalink=\(.*\)$/metalink=\1\&protocol=https/" /etc/yum.repos.d/*.repo
    # Cisco serves openh264 over plain http only, which the proxy refuses; it
    # is an optional codec, so the check runs without that repository.
    rm -f /etc/yum.repos.d/fedora-cisco-openh264.repo
  fi
  rpm -qpR /pkg/vibezeo.rpm > /out/requires.txt
  if ! dnf -y install /pkg/vibezeo.rpm > /out/dnf.log 2>&1; then
    echo "dnf: the package does not install"; tail -30 /out/dnf.log; exit 1
  fi
  echo "dnf: installed $(rpm -qp --qf "%{NAME} %{VERSION}-%{RELEASE}" /pkg/vibezeo.rpm)"
  missing=0
  for b in /usr/bin/tandem /usr/bin/tandem-hook /usr/bin/tandem-tasks; do
    if ldd "$b" | grep "not found"; then missing=1; fi
  done
  if [ "$missing" = 1 ]; then echo "libraries: missing (above)"; exit 1; fi
  echo "libraries: every binary finds them"
  dnf -y install xorg-x11-server-Xvfb dbus-daemon ImageMagick > /out/dnf-test.log 2>&1
  export HOME=/tmp/home TANDEM_HOME=/tmp/home/.tandem NO_AT_BRIDGE=1 DISPLAY=:99
  mkdir -p "$HOME"
  Xvfb :99 -screen 0 1440x900x24 > /out/xvfb.log 2>&1 &
  sleep 2
  dbus-run-session -- /usr/bin/tandem > /out/app.log 2>&1 &
  pid=$!
  sleep 20
  import -window root /out/welcome.png || true
  if kill -0 "$pid" 2>/dev/null; then
    echo "app: still running after 20 s"
    kill "$pid"
  else
    echo "app: exited early"; tail -30 /out/app.log; exit 1
  fi
'
echo "requires, logs and welcome.png in $OUT"
