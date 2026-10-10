#!/bin/sh
# Builds the sshd fixture image (infra/ssh) in agent containers whose only
# way out is an HTTPS proxy with its own CA (plain-http apt is refused
# there). Elsewhere `docker compose --profile remote up --build sshd` is
# enough. Usage: sh scripts/agent/build-sshd-image.sh
set -e
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CTX="$(mktemp -d)"
trap 'rm -rf "$CTX"' EXIT
cp "$ROOT/infra/ssh/setup.sh" "$CTX/"
CA="${SSL_CERT_FILE:-/root/.ccr/ca-bundle.crt}"
if [ -n "$HTTPS_PROXY" ] && [ -f "$CA" ]; then
  cp "$CA" "$CTX/proxy-ca.crt"
  # Same image, with apt over https through the proxy.
  sed -e '/^FROM /a COPY proxy-ca.crt /etc/ssl/proxy-ca.crt\nRUN sed -i "s#http://#https://#g" /etc/apt/sources.list.d/ubuntu.sources \&\& printf "Acquire::https::CAInfo \\"/etc/ssl/proxy-ca.crt\\";\\nAcquire::https::Proxy \\"'"$HTTPS_PROXY"'\\";\\n" > /etc/apt/apt.conf.d/99proxy' \
    "$ROOT/infra/ssh/Dockerfile" > "$CTX/Dockerfile"
  docker build --network host -t tandem-sshd:dev "$CTX"
else
  docker build -t tandem-sshd:dev "$ROOT/infra/ssh"
fi
