#!/bin/sh
# Print (do not run) the activation steps: enabling a network service is
# the operator's explicit decision.
cat <<'MSG'
nimbus installed. To run it as a service:

    sudo systemctl daemon-reload
    sudo systemctl enable --now nimbus

The server binds 127.0.0.1:8080 by default; see
https://nimbusdocs.com/operators/deploy-linux/ for hardening and
reverse-proxy guidance.
MSG
exit 0
