#!/bin/sh
# Create the system user the nimbus.service unit runs as. The state
# directory (/var/lib/nimbus) is created by systemd's StateDirectory=.
set -e
if ! getent group nimbus >/dev/null 2>&1; then
    groupadd --system nimbus
fi
if ! getent passwd nimbus >/dev/null 2>&1; then
    useradd --system --gid nimbus --home-dir /var/lib/nimbus \
        --no-create-home --shell /usr/sbin/nologin nimbus
fi
exit 0
