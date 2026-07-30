#!/usr/bin/env python3
"""Thin CLI composition root for the NNC4.7 sovereignty tripwire."""

from __future__ import annotations

import sys

from .evidence import EvidenceValidationError
from .environment import FAIL_EXIT, TripwireConfig, TripwireError
from .isolation import build_parser, run_live


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.repeat != 2:
        parser.error("--repeat must be exactly 2")
    if args.command_timeout_seconds < 5 or args.command_timeout_seconds > 300:
        parser.error("--command-timeout-seconds must be between 5 and 300")
    config = TripwireConfig(
        runner_id=args.runner_id,
        expected_hostname=args.expected_hostname,
        host_class=args.host_class,
        provider_kind=args.provider_kind,
        output_dir=args.output_dir,
        repeat=args.repeat,
        command_timeout_seconds=args.command_timeout_seconds,
    )
    try:
        return run_live(config)
    except EvidenceValidationError as error:
        print(f"FAIL evidence validation: {error}", file=sys.stderr)
        return FAIL_EXIT
    except TripwireError as error:
        print(f"FAIL {error}", file=sys.stderr)
        return error.exit_code
