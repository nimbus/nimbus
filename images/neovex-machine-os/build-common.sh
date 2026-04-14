#!/usr/bin/env bash
set -euo pipefail

mkdir -p \
  /etc/containers/registries.conf.d \
  /etc/ssh/sshd_config.d \
  /etc/sysctl.d \
  /var/lib/neovex/control \
  /var/lib/neovex/data

cat >/etc/containers/registries.conf.d/999-neovex-machine.conf <<'EOF'
unqualified-search-registries=["docker.io"]
EOF

cat >/etc/ssh/sshd_config.d/99-neovex-machine-sshd.conf <<'EOF'
PerSourcePenalties authfail:0
MaxStartups 65535
EOF

cat >/etc/sysctl.d/10-neovex-machine-inotify.conf <<'EOF'
fs.inotify.max_user_instances=524288
EOF

dnf install --best -y \
  aardvark-dns \
  buildah \
  conmon \
  containers-common \
  containers-common-extra \
  cpp \
  crun \
  fuse-overlayfs \
  git-core \
  iproute \
  netavark \
  openssh-server \
  procps-ng \
  socat

dnf remove -y moby-engine containerd runc toolbox docker-cli || true

ln -fs /usr/lib/systemd/system/sshd.service \
  /etc/systemd/system/multi-user.target.wants/sshd.service

mkdir -p /usr/local/bin
chmod 0755 /usr/local/bin /var/lib/neovex /var/lib/neovex/control /var/lib/neovex/data

rm -rf /var/cache /usr/share/man
dnf clean all -y
