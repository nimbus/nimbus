#!/usr/bin/env bash

# Best-effort by design: visibility must never fail a test lane.
set -u

job_name="${1:?usage: append-rerun-visibility.sh <job-name>}"
attempt="${GITHUB_RUN_ATTEMPT:-1}"

if [[ "${attempt}" =~ ^[0-9]+$ ]] && (( attempt > 1 )); then
  line="RERUN-ATTEMPT ${attempt} on ${job_name}"
  printf '%s\n' "${line}"
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    printf '%s\n' "${line}" >> "${GITHUB_STEP_SUMMARY}"
  fi
fi || true
