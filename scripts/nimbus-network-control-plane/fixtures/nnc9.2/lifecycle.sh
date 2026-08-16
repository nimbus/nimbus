#!/bin/sh
set -eu

marker=/nnc92-restarted
state=/nnc92-state
exit_trigger=/nnc92-exit-now

if [ ! -e "${marker}" ]; then
  : >"${marker}"
  printf '%s\n' first-ready >"${state}"
  # Private readiness inspection can connect more than once. Keep the first
  # attempt serving until the proof owner records a successful public request
  # and writes the explicit exit trigger into this exact guest rootfs.
  /bin/busybox httpd -f -p 8080 &
  server_pid=$!
  cleanup_server() {
    kill "${server_pid}" 2>/dev/null || :
    wait "${server_pid}" 2>/dev/null || :
  }
  trap cleanup_server EXIT HUP INT TERM
  while [ ! -e "${exit_trigger}" ]; do
    kill -0 "${server_pid}" 2>/dev/null || exit 70
    /bin/busybox sleep 1
  done
  cleanup_server
  trap - EXIT HUP INT TERM
  printf '%s\n' first-exited >"${state}"
  exit 23
fi

printf '%s\n' restarted-ready >"${state}"
exec /bin/busybox httpd -f -p 8080
