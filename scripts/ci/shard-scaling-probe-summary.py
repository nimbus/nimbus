#!/usr/bin/env python3
"""Summarize shard-scaling probe timing artifacts."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Any


def load_timings(root: Path) -> list[dict[str, Any]]:
    timings: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        if data.get("workflow") == "shard-scaling-probe" and "k" in data:
            timings.append(data)
    return sorted(timings, key=lambda item: int(item["k"]))


def fmt_seconds(value: Any) -> str:
    if isinstance(value, (int, float)):
        return f"{float(value):.3f}s"
    if value is None:
        return "PENDING(not-measured)"
    return str(value)


def partition_summary(timing: dict[str, Any]) -> str:
    return "; ".join(
        f"{row['partition']}={float(row['wall_seconds']):.3f}s"
        for row in timing.get("partitions", [])
    )


def is_green(item: dict[str, Any]) -> bool:
    """A K arm counts only if every partition in it succeeded (review finding:
    a red/short-circuited K must never become the movement-rule candidate)."""
    partitions = item.get("partitions", [])
    if not partitions:
        return False
    return all(int(part.get("status", 1)) == 0 for part in partitions)


def movement_rule(timings: list[dict[str, Any]]) -> str:
    by_k = {int(item["k"]): item for item in timings}
    current = by_k.get(3)
    if current is None:
        return "MOVEMENT-RULE current=K=3 verdict=PENDING(missing K=3 timing)"
    if not is_green(current):
        return "MOVEMENT-RULE current=K=3 verdict=PENDING(non-green K=3 baseline)"

    baseline = float(current["max_partition_seconds"])
    alternatives = [
        item for item in timings if int(item["k"]) != 3 and is_green(item)
    ]
    if not alternatives:
        return "MOVEMENT-RULE current=K=3 verdict=PENDING(no green alternative timings)"

    best = max(
        alternatives,
        key=lambda item: baseline - float(item["max_partition_seconds"]),
    )
    improvement = baseline - float(best["max_partition_seconds"])
    improvement_pct = (improvement / baseline * 100.0) if baseline > 0 else 0.0
    threshold_met = improvement > 45.0 or improvement_pct > 10.0
    if threshold_met:
        verdict = "HOLD(candidate requires two consecutive green probes before changing default)"
    else:
        verdict = "HOLD(no alternative clears >45s or >10% slowest-lane improvement)"
    return (
        f"MOVEMENT-RULE current=K=3 best=K={int(best['k'])} "
        f"improvement={improvement:.3f}s improvement_pct={improvement_pct:.2f}% "
        f"verdict={verdict}"
    )


def render_summary(timings: list[dict[str, Any]]) -> str:
    lines = [
        "## Shard scaling probe",
        "",
        "| K | per-partition wall | download+extract | max-partition wall | total |",
        "|---|---|---|---|---|",
    ]
    for timing in timings:
        lines.append(
            f"| {int(timing['k'])} | {partition_summary(timing)} | "
            f"{fmt_seconds(timing.get('download_extract_seconds'))} | "
            f"{fmt_seconds(timing.get('max_partition_seconds'))} | "
            f"{fmt_seconds(timing.get('total_seconds'))} |"
        )
    lines.extend(["", movement_rule(timings), ""])
    return "\n".join(lines)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("timing_dir", type=Path)
    parser.add_argument("--summary", type=Path, default=Path(os.environ["GITHUB_STEP_SUMMARY"]) if os.environ.get("GITHUB_STEP_SUMMARY") else None)
    parser.add_argument("--combined-json", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    timings = load_timings(args.timing_dir)
    text = render_summary(timings)
    print(text)
    if args.summary is not None:
        with args.summary.open("a", encoding="utf-8") as summary:
            summary.write(text)
            summary.write("\n")
    if args.combined_json is not None:
        args.combined_json.parent.mkdir(parents=True, exist_ok=True)
        args.combined_json.write_text(
            json.dumps({"schema_version": 1, "timings": timings}, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
