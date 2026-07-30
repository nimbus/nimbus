#!/usr/bin/env python3
"""In-namespace control/profile probes for the NNC4.7 live harness."""

from __future__ import annotations

import argparse
import ipaddress
import json
import os
from pathlib import Path
import socket
import struct
import sys
import threading
from typing import Any


def _read_process_security() -> dict[str, Any]:
    fields: dict[str, str] = {}
    for line in Path("/proc/self/status").read_text(encoding="utf-8").splitlines():
        key, separator, value = line.partition(":")
        if separator:
            fields[key] = value.strip()
    required = (
        "CapAmb",
        "CapBnd",
        "CapEff",
        "CapInh",
        "CapPrm",
        "Gid",
        "NoNewPrivs",
        "Uid",
    )
    missing = [key for key in required if key not in fields]
    if missing:
        raise RuntimeError(
            "required process security fields are absent: " + ", ".join(missing)
        )
    capabilities = {
        key.lower(): int(fields[key], 16)
        for key in ("CapAmb", "CapBnd", "CapEff", "CapInh", "CapPrm")
    }
    return {
        **capabilities,
        "uids": [int(value) for value in fields["Uid"].split()],
        "gids": [int(value) for value in fields["Gid"].split()],
        "no_new_privs": int(fields["NoNewPrivs"]),
    }


def _connect(family: int, address: str, port: int, *, timeout: float = 0.35) -> bool:
    with socket.socket(family, socket.SOCK_STREAM) as stream:
        stream.settimeout(timeout)
        return stream.connect_ex((address, port)) == 0


def _one_shot_loopback_server(port: int, ready: threading.Event) -> None:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
        server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server.bind(("127.0.0.1", port))
        server.listen(1)
        ready.set()
        connection, _ = server.accept()
        with connection:
            connection.sendall(b"NNC47-LOOPBACK\n")


def _loopback_control(port: int) -> bool:
    ready = threading.Event()
    thread = threading.Thread(
        target=_one_shot_loopback_server, args=(port, ready), daemon=True
    )
    thread.start()
    if not ready.wait(timeout=2):
        return False
    with socket.create_connection(("127.0.0.1", port), timeout=1) as stream:
        body = stream.recv(64)
    thread.join(timeout=2)
    return not thread.is_alive() and body == b"NNC47-LOOPBACK\n"


def _dns_query(name: str, identifier: int) -> bytes:
    labels = name.rstrip(".").split(".")
    question = b"".join(bytes([len(label)]) + label.encode("ascii") for label in labels)
    question += b"\x00"
    header = struct.pack("!HHHHHH", identifier, 0x0100, 1, 0, 0, 0)
    return header + question + struct.pack("!HH", 1, 1)


def _udp_dns(address: str, name: str) -> bool:
    packet = _dns_query(name, 0x4E47)
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as stream:
        return stream.sendto(packet, (address, 53)) == len(packet)


def _tcp_dns(address: str, name: str) -> bool:
    packet = _dns_query(name, 0x4E54)
    framed = struct.pack("!H", len(packet)) + packet
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as stream:
        stream.settimeout(0.35)
        status = stream.connect_ex((address, 53))
        if status == 0:
            stream.sendall(framed)
            return True
        return False


def run_probe(args: argparse.Namespace) -> int:
    security = _read_process_security()
    unprivileged = (
        all(value == 65534 for value in security["uids"])
        and all(value == 65534 for value in security["gids"])
        and security["no_new_privs"] == 1
        and all(
            security[key] == 0
            for key in ("capamb", "capbnd", "capeff", "capinh", "capprm")
        )
    )
    results: dict[str, Any] = {
        "security": security,
        "subject_unprivileged": unprivileged,
        "cap_net_admin": bool(security["capeff"] & (1 << 12)),
        "loopback": _loopback_control(args.loopback_port),
        "private_ipv4": _connect(socket.AF_INET, args.peer_ipv4, args.peer_port),
        "private_ipv6": _connect(socket.AF_INET6, args.peer_ipv6, args.peer_port),
    }
    if args.mode == "control":
        results.update(
            {
                "unenumerated_private_denied": not _connect(
                    socket.AF_INET, args.unenumerated_private, args.peer_port
                ),
                "dns_udp_attempted": _udp_dns(
                    args.dns_ipv4, "nnc47-udp-control.invalid"
                ),
                "dns_tcp_attempted": _tcp_dns(
                    args.dns_ipv4, "nnc47-tcp-control.invalid"
                ),
                "public_ipv4_denied": not _connect(
                    socket.AF_INET, args.public_ipv4, 443
                ),
                "public_ipv6_denied": not _connect(
                    socket.AF_INET6, args.public_ipv6, 443
                ),
            }
        )

    passed = (
        results["loopback"]
        and results["private_ipv4"]
        and results["private_ipv6"]
        and results["subject_unprivileged"]
    )
    if args.mode == "control":
        passed = passed and all(
            results[key]
            for key in (
                "unenumerated_private_denied",
                "dns_udp_attempted",
                "dns_tcp_attempted",
                "public_ipv4_denied",
                "public_ipv6_denied",
            )
        )
    results["passed"] = passed
    print(json.dumps(results, sort_keys=True), flush=True)
    return 0 if passed else 1


def run_peer_server(args: argparse.Namespace) -> int:
    listeners: list[tuple[socket.socket, str]] = []
    for family, address in (
        (socket.AF_INET, args.peer_ipv4),
        (socket.AF_INET6, args.peer_ipv6),
    ):
        listener = socket.socket(family, socket.SOCK_STREAM)
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        if family == socket.AF_INET6:
            listener.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
        listener.bind((address, args.peer_port))
        listener.listen(8)
        listeners.append((listener, "private-peer"))
    dns_tcp = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    dns_tcp.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    dns_tcp.bind((args.dns_ipv4, 53))
    dns_tcp.listen(8)
    listeners.append((dns_tcp, "dns-tcp"))
    dns_udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    dns_udp.bind((args.dns_ipv4, 53))
    listeners.append((dns_udp, "dns-udp"))

    print(
        json.dumps(
            {
                "status": "READY",
                "pid": os.getpid(),
                "ipv4": args.peer_ipv4,
                "ipv6": args.peer_ipv6,
                "port": args.peer_port,
            },
            sort_keys=True,
        ),
        flush=True,
    )
    while True:
        sockets = [listener for listener, _ in listeners]
        readable, _, _ = select_with_timeout(sockets, timeout=30)
        for listener in readable:
            mode = next(mode for candidate, mode in listeners if candidate is listener)
            if mode == "dns-udp":
                listener.recvfrom(4096)
                continue
            connection, _ = listener.accept()
            with connection:
                if mode == "dns-tcp":
                    connection.recv(4096)
                else:
                    connection.sendall(b"NNC47-PRIVATE-PEER\n")


def select_with_timeout(
    sockets: list[socket.socket], *, timeout: float
) -> tuple[list[socket.socket], list[socket.socket], list[socket.socket]]:
    import select

    return select.select(sockets, [], [], timeout)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--peer-ipv4", required=True, type=_ip_address)
    common.add_argument("--peer-ipv6", required=True, type=_ip_address)
    common.add_argument("--peer-port", type=int, default=18080)
    common.add_argument("--dns-ipv4", required=True, type=_ip_address)

    server = subparsers.add_parser("peer-server", parents=[common])
    server.set_defaults(handler=run_peer_server)

    probe = subparsers.add_parser("probe", parents=[common])
    probe.add_argument("--mode", choices=("control", "profile"), required=True)
    probe.add_argument("--loopback-port", type=int, default=18081)
    probe.add_argument("--unenumerated-private", default="10.253.0.1", type=_ip_address)
    probe.add_argument("--public-ipv4", default="192.0.2.1", type=_ip_address)
    probe.add_argument("--public-ipv6", default="2001:db8::1", type=_ip_address)
    probe.set_defaults(handler=run_probe)
    return parser


def _ip_address(value: str) -> str:
    return str(ipaddress.ip_address(value))


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    return int(args.handler(args))


if __name__ == "__main__":
    sys.exit(main())
