#!/usr/bin/env python3
"""Network probes used by the NNC9.2 offline lifecycle adapter."""

from __future__ import annotations

import argparse
import json
import socket
import time
from typing import Any


class HttpConnectionFailure(OSError):
    """The endpoint rejected or could not establish the TCP connection."""


def _http_request(host: str, port: int, path: str, timeout: float) -> dict[str, Any]:
    started = time.monotonic()
    try:
        connection = socket.create_connection((host, port), timeout=timeout)
    except OSError as error:
        raise HttpConnectionFailure(str(error)) from error
    with connection:
        connection.settimeout(timeout)
        request = (
            f"GET {path} HTTP/1.1\r\n"
            f"Host: {host}:{port}\r\n"
            "Connection: close\r\n\r\n"
        ).encode("ascii")
        connection.sendall(request)
        chunks: list[bytes] = []
        while True:
            chunk = connection.recv(65536)
            if not chunk:
                break
            chunks.append(chunk)
    payload = b"".join(chunks)
    header, separator, body = payload.partition(b"\r\n\r\n")
    if not separator:
        raise RuntimeError("HTTP response has no header terminator")
    status_line = header.splitlines()[0].decode("ascii", errors="replace")
    fields = status_line.split()
    if len(fields) < 2 or not fields[1].isdigit():
        raise RuntimeError(f"HTTP response has invalid status line: {status_line!r}")
    return {
        "host": host,
        "port": port,
        "path": path,
        "status": int(fields[1]),
        "status_line": status_line,
        "body": body.decode("utf-8", errors="replace"),
        "elapsed_ms": round((time.monotonic() - started) * 1000),
    }


def _wait_http(args: argparse.Namespace) -> dict[str, Any]:
    deadline = time.monotonic() + args.timeout_seconds
    attempts = 0
    last_error = "not attempted"
    while time.monotonic() < deadline:
        attempts += 1
        try:
            result = _http_request(args.host, args.port, args.path, args.connect_timeout)
        except (OSError, RuntimeError) as error:
            last_error = f"{type(error).__name__}: {error}"
        else:
            body_matches = args.expect_body in result["body"]
            status_matches = result["status"] == args.expect_status
            if body_matches and status_matches:
                result.update({"attempts": attempts, "passed": True})
                return result
            last_error = (
                f"status={result['status']} body={result['body']!r}; "
                f"expected status={args.expect_status} body containing "
                f"{args.expect_body!r}"
            )
        time.sleep(args.interval_seconds)
    raise RuntimeError(
        f"HTTP expectation did not converge after {attempts} attempts: {last_error}"
    )


def _expect_unreachable(args: argparse.Namespace) -> dict[str, Any]:
    deadline = time.monotonic() + args.timeout_seconds
    attempts = 0
    while time.monotonic() < deadline:
        attempts += 1
        try:
            result = _http_request(args.host, args.port, args.path, args.connect_timeout)
        except HttpConnectionFailure:
            time.sleep(args.interval_seconds)
            continue
        raise RuntimeError(
            "retired endpoint remained reachable: "
            f"status={result['status']} body={result['body']!r}"
        )
    return {
        "host": args.host,
        "port": args.port,
        "path": args.path,
        "attempts": attempts,
        "passed": True,
        "unreachable_for_seconds": args.timeout_seconds,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="NNC9.2 lifecycle network probe")
    subparsers = parser.add_subparsers(dest="command", required=True)
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--host", default="127.0.0.1")
    common.add_argument("--port", type=int, required=True)
    common.add_argument("--path", default="/")
    common.add_argument("--timeout-seconds", type=float, default=60.0)
    common.add_argument("--connect-timeout", type=float, default=1.0)
    common.add_argument("--interval-seconds", type=float, default=0.2)

    wait_http = subparsers.add_parser("wait-http", parents=[common])
    wait_http.add_argument("--expect-status", type=int, default=200)
    wait_http.add_argument("--expect-body", required=True)

    subparsers.add_parser("expect-unreachable", parents=[common])
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "wait-http":
            result = _wait_http(args)
        else:
            result = _expect_unreachable(args)
    except (OSError, RuntimeError) as error:
        print(json.dumps({"passed": False, "error": str(error)}, sort_keys=True))
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
