#!/usr/bin/env python3
"""Run Bun's generated shared-adapter smoke loader with a time limit."""

from __future__ import annotations

import argparse
import math
import subprocess
import sys
from pathlib import Path


TIMEOUT_EXIT_STATUS = 124


def positive_seconds(value: str) -> float:
    try:
        seconds = float(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("must be a number") from error
    if not math.isfinite(seconds) or seconds <= 0:
        raise argparse.ArgumentTypeError("must be a finite number greater than zero")
    return seconds


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--timeout-seconds",
        required=True,
        type=positive_seconds,
        help="maximum time for the generated loader",
    )
    parser.add_argument("loader", type=Path, help="generated Python smoke loader")
    parser.add_argument("shared_library", type=Path, help="Bun/JSC shared adapter")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    if not args.loader.is_file():
        print(f"missing Bun/JSC shared adapter smoke loader: {args.loader}", file=sys.stderr)
        return 2
    if not args.shared_library.is_file():
        print(f"missing Bun/JSC shared adapter artifact: {args.shared_library}", file=sys.stderr)
        return 2

    command = [sys.executable, str(args.loader), str(args.shared_library)]
    print(
        "running Bun/JSC shared adapter smoke "
        f"with a {args.timeout_seconds:g}-second limit",
        flush=True,
    )
    try:
        result = subprocess.run(command, check=False, timeout=args.timeout_seconds)
    except subprocess.TimeoutExpired:
        print(
            "Bun/JSC shared adapter smoke exceeded "
            f"{args.timeout_seconds:g} seconds",
            file=sys.stderr,
            flush=True,
        )
        return TIMEOUT_EXIT_STATUS
    except OSError as error:
        print(f"could not start Bun/JSC shared adapter smoke: {error}", file=sys.stderr)
        return 2

    if result.returncode < 0:
        return 128 + min(-result.returncode, 127)
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
