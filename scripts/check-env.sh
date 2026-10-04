#!/usr/bin/env bash
# Reports which development prerequisites are missing (MILESTONES M0 task 3).
missing=0
check() {
  local name="$1" cmd="$2" hint="$3"
  if command -v "$cmd" >/dev/null 2>&1; then
    printf '  ok       %-16s %s\n' "$name" "$(command -v "$cmd")"
  else
    printf '  MISSING  %-16s %s\n' "$name" "$hint"
    missing=$((missing + 1))
  fi
}
echo "Tandem development prerequisites:"
check node node "install Node 22 (see .nvmrc)"
check pnpm pnpm "corepack enable && corepack prepare pnpm@9.15.9 --activate"
check cargo cargo "install Rust stable with rustup"
check docker docker "install Docker with compose v2 (infra: postgres, redis, mailpit, stripe-mock)"
check xvfb-run xvfb-run "apt install xvfb (desktop E2E)"
check WebKitWebDriver WebKitWebDriver "apt install webkit2gtk-driver (desktop E2E)"
check tauri-driver tauri-driver "cargo install tauri-driver --locked (desktop E2E)"
if command -v pkg-config >/dev/null 2>&1; then
  for lib in webkit2gtk-4.1 gtk+-3.0 libsoup-3.0 alsa dbus-1; do
    if pkg-config --exists "$lib"; then
      printf '  ok       %-16s pkg-config\n' "$lib"
    else
      printf '  MISSING  %-16s see scripts/bootstrap-linux.sh\n' "$lib"
      missing=$((missing + 1))
    fi
  done
fi
if [ "$missing" -gt 0 ]; then
  echo "$missing prerequisite(s) missing. scripts/bootstrap-linux.sh installs the Linux ones."
  exit 1
fi
echo "All prerequisites present."
