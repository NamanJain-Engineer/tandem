# The SSH host for remote-workspace tests (MILESTONES M14, D-10): OpenSSH
# and git on the same Ubuntu as the dev machines, so a test can copy a
# binary built here (the fake engine) into it. User `tandem`, key-only
# sign-in with the key from scripts/gen-ssh-fixture.sh, and a small git
# repository at /home/tandem/project. Host keys are made when the container
# starts; E2E-M14-2 regenerates them to prove a changed key is refused.
FROM ubuntu:24.04
RUN apt-get update \
 && apt-get install -y --no-install-recommends openssh-server git ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY setup.sh /usr/local/sbin/tandem-sshd-setup
RUN sh /usr/local/sbin/tandem-sshd-setup build
EXPOSE 22
CMD ["sh", "/usr/local/sbin/tandem-sshd-setup", "run"]
