#!/usr/bin/env bash
set -euo pipefail

ports=(5173 5174 8080 8082 8083 8084 8085 8086 8087 8088 8089)
pids=()

for port in "${ports[@]}"; do
  while IFS= read -r pid; do
    if [[ -n "${pid}" ]]; then
      pids+=("${pid}")
    fi
  done < <(lsof -tiTCP:"${port}" -sTCP:LISTEN 2>/dev/null || true)
done

if [[ "${#pids[@]}" -eq 0 ]]; then
  echo "No demo listeners to stop."
  exit 0
fi

unique_pids=()
while IFS= read -r pid; do
  if [[ -n "${pid}" ]]; then
    unique_pids+=("${pid}")
  fi
done < <(printf '%s\n' "${pids[@]}" | awk '!seen[$0]++')

kill "${unique_pids[@]}"

echo "Stopped demo listeners on ports: ${ports[*]}"
