#!/usr/bin/env python3
"""Signal and host-lock boundaries for privileged tripwire effects."""

from __future__ import annotations

from contextlib import contextmanager
import fcntl
from pathlib import Path
import signal
from typing import Any, Iterator

from .environment import TripwireProofFailure


@contextmanager
def _defer_termination_signals() -> Iterator[list[int]]:
    observed: list[int] = []
    previous: dict[signal.Signals, Any] = {}

    def record(signum: int, _frame: Any) -> None:
        observed.append(signum)

    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        previous[signum] = signal.getsignal(signum)
        signal.signal(signum, record)
    try:
        yield observed
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


@contextmanager
def _exclusive_host_lock(lock_path: Path) -> Iterator[None]:
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    with lock_path.open("a+", encoding="utf-8") as lock:
        try:
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise TripwireProofFailure(
                "another sovereignty tripwire owns the host-global lock"
            ) from error
        yield
