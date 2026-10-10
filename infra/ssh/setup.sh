#!/bin/sh
# Build and start steps for the sshd fixture (infra/ssh/Dockerfile).
set -e
case "$1" in
build)
  useradd -m -s /bin/bash tandem
  # A locked password still blocks key sign-in on some PAM setups.
  usermod -p '*' tandem
  mkdir -p /run/sshd /home/tandem/.ssh /home/tandem/project
  cat > /etc/ssh/sshd_config.d/tandem.conf <<'CONF'
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
PermitRootLogin no
MaxSessions 64
MaxStartups 64:30:128
AllowAgentForwarding no
X11Forwarding no
CONF
  cd /home/tandem/project
  git init -q -b main
  printf '# project\n' > README.md
  printf 'fn main() {\n    println!("hi");\n}\n' > main.rs
  git -c user.name=Tandem -c user.email=tandem@example.com add .
  git -c user.name=Tandem -c user.email=tandem@example.com commit -q -m "first"
  git config user.name Tandem
  git config user.email tandem@example.com
  chown -R tandem:tandem /home/tandem
  ;;
run)
  # The test key, mounted read-only from infra/ssh/id_ed25519.pub.
  install -o tandem -g tandem -m 600 /fixture/id_ed25519.pub /home/tandem/.ssh/authorized_keys
  chmod 700 /home/tandem/.ssh
  ssh-keygen -A >/dev/null
  exec /usr/sbin/sshd -D -e
  ;;
esac
