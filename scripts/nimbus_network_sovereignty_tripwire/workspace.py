#!/usr/bin/env python3
"""Exclusive filesystem ownership for privileged tripwire evidence."""

from __future__ import annotations

import os
from pathlib import Path
import stat

from .environment import TripwireConfigurationError


def _write_new_text(path: Path, payload: str) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags, 0o600)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            descriptor = -1
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
    finally:
        if descriptor >= 0:
            os.close(descriptor)


def _prepare_output_directory(path: Path) -> None:
    if (
        not path.is_absolute()
        or path == Path("/")
        or path.name in {"", ".", ".."}
        or ".." in path.parts
    ):
        raise TripwireConfigurationError(
            f"refusing unsafe evidence output directory: {path}"
        )
    parent = path.parent
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open("/", flags)
    current = Path("/")
    try:
        for component in parent.parts[1:]:
            try:
                next_descriptor = os.open(component, flags, dir_fd=descriptor)
            except OSError as error:
                raise TripwireConfigurationError(
                    "evidence parent contains a missing, non-directory, or "
                    f"symlink component: {current / component}"
                ) from error
            os.close(descriptor)
            descriptor = next_descriptor
            current /= component
            metadata = os.fstat(descriptor)
            if os.geteuid() == 0 and (
                metadata.st_uid != 0 or metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH)
            ):
                raise TripwireConfigurationError(
                    "root evidence parent ancestry must be root-owned and "
                    f"non-writable by other principals: {current}"
                )
        try:
            os.mkdir(path.name, 0o700, dir_fd=descriptor)
        except FileExistsError as error:
            raise TripwireConfigurationError(
                f"evidence output directory must not already exist: {path}"
            ) from error
        child_descriptor = os.open(path.name, flags, dir_fd=descriptor)
        os.close(child_descriptor)
    except OSError as error:
        raise TripwireConfigurationError(
            f"could not create exclusive evidence directory: {path}"
        ) from error
    finally:
        os.close(descriptor)
