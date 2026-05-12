#!/usr/bin/env python3
from __future__ import annotations

import csv
import datetime as dt
import html
import json
import pathlib
import re
import subprocess
import sys
import urllib.request
from dataclasses import dataclass
from typing import Any


REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
OUTPUT_ROOT = REPO_ROOT / "docs" / "architecture" / "runtime" / "node-lts-compat"
DENO_REPO = pathlib.Path.home() / "src" / "github.com" / "nimbus" / "deno"

NODE20_URL = "https://nodejs.org/download/release/latest-v20.x/docs/api/all.json"
NODE22_URL = "https://nodejs.org/download/release/latest-v22.x/docs/api/all.json"
DENO_COMPAT_URL = "https://docs.deno.com/runtime/reference/node_apis/"

RUNTIME_PRESETS = ("Application", "Tooling")
COMPATIBILITY_TARGET = "Node22"
VERIFICATION_LANE = "pending-node-upstream"

TARGET_MODULES = [
    "node:assert",
    "node:async_hooks",
    "node:buffer",
    "node:child_process",
    "node:cluster",
    "node:console",
    "node:constants",
    "node:crypto",
    "node:dgram",
    "node:diagnostics_channel",
    "node:dns",
    "node:domain",
    "node:events",
    "node:fs",
    "node:fs/promises",
    "node:http",
    "node:http2",
    "node:https",
    "node:inspector",
    "node:module",
    "node:net",
    "node:os",
    "node:path",
    "node:perf_hooks",
    "node:process",
    "node:punycode",
    "node:querystring",
    "node:readline",
    "node:repl",
    "node:sea",
    "node:stream",
    "node:string_decoder",
    "node:sys",
    "node:test",
    "node:test/reporters",
    "node:timers",
    "node:tls",
    "node:trace_events",
    "node:tty",
    "node:url",
    "node:util",
    "node:v8",
    "node:vm",
    "node:wasi",
    "node:worker_threads",
    "node:zlib",
    "node:sqlite",
]

SOURCE_TO_MODULE = {
    "assert": "node:assert",
    "async_context": "node:async_hooks",
    "async_hooks": "node:async_hooks",
    "buffer": "node:buffer",
    "child_process": "node:child_process",
    "cluster": "node:cluster",
    "console": "node:console",
    "crypto": "node:crypto",
    "diagnostics_channel": "node:diagnostics_channel",
    "dns": "node:dns",
    "domain": "node:domain",
    "events": "node:events",
    "fs": "node:fs",
    "http": "node:http",
    "http2": "node:http2",
    "https": "node:https",
    "inspector": "node:inspector",
    "module": "node:module",
    "net": "node:net",
    "os": "node:os",
    "path": "node:path",
    "perf_hooks": "node:perf_hooks",
    "process": "node:process",
    "punycode": "node:punycode",
    "querystring": "node:querystring",
    "readline": "node:readline",
    "repl": "node:repl",
    "sea": "node:sea",
    "stream": "node:stream",
    "string_decoder": "node:string_decoder",
    "test": "node:test",
    "timers": "node:timers",
    "tls": "node:tls",
    "trace_events": "node:trace_events",
    "tty": "node:tty",
    "url": "node:url",
    "util": "node:util",
    "v8": "node:v8",
    "vm": "node:vm",
    "wasi": "node:wasi",
    "worker_threads": "node:worker_threads",
    "zlib": "node:zlib",
    "sqlite": "node:sqlite",
}

SOURCE_ONLY_MODULES = {
    "node:constants": "doc/api/constants.md",
    "node:sys": "doc/api/util.md",
}

INTERESTING_TYPES = {"class", "method", "property", "event"}
NESTED_KEYS = ("modules", "classes", "methods", "properties", "events", "miscs")

BACKTICK_RE = re.compile(r"`([^`]+)`")
STRIP_TAGS_RE = re.compile(r"<[^>]+>")
MODULE_SECTION_RE = re.compile(
    r'<h3 id="(?P<id>[^"]+)"[^>]*><a href="(?P<href>[^"]+)">(?P<module>node:[^<]+)</a>.*?</h3>\s*'
    r'<div class="item-content">(?P<content>.*?)</div>',
    re.S,
)
STRONG_NOTE_RE = re.compile(r"<p><strong>([^<]+)</strong>:\s*(.*?)</p>", re.S)


@dataclass
class RemoteDocument:
    body: Any
    source_label: str
    url: str
    etag: str
    last_modified: str


def fetch_json(url: str, label: str) -> RemoteDocument:
    request = urllib.request.Request(url, headers={"User-Agent": "nimbus-node-compat-generator/1"})
    with urllib.request.urlopen(request) as response:
        body = json.load(response)
        return RemoteDocument(
            body=body,
            source_label=label,
            url=url,
            etag=response.headers.get("ETag", ""),
            last_modified=response.headers.get("Last-Modified", ""),
        )


def fetch_text(url: str, label: str) -> RemoteDocument:
    request = urllib.request.Request(url, headers={"User-Agent": "nimbus-node-compat-generator/1"})
    with urllib.request.urlopen(request) as response:
        body = response.read().decode("utf-8")
        return RemoteDocument(
            body=body,
            source_label=label,
            url=url,
            etag=response.headers.get("ETag", ""),
            last_modified=response.headers.get("Last-Modified", ""),
        )


def git_output(*args: str, cwd: pathlib.Path) -> str:
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def normalize_node_status(entry: dict[str, Any]) -> str:
    stability = entry.get("stability")
    stability_text = str(entry.get("stabilityText") or "").lower()
    if stability == 0 or "deprecated" in stability_text:
        return "deprecated"
    if stability == 1 or "experimental" in stability_text:
        return "experimental"
    return "stable"


def extract_added(entry: dict[str, Any]) -> str:
    meta = entry.get("meta") or {}
    added = meta.get("added") or []
    return added[0] if added else ""


def first_code_span(text_raw: str) -> str:
    match = BACKTICK_RE.search(text_raw or "")
    return match.group(1) if match else ""


def normalize_symbol(module_id: str, entry: dict[str, Any], context_symbol: str | None) -> str:
    module_short = module_id.removeprefix("node:")
    name = str(entry.get("name") or "").strip()
    text_raw = str(entry.get("textRaw") or "").strip()
    type_name = str(entry.get("type") or "")

    if type_name == "class" and name:
        return name if "." in name else f"{module_short}.{name}"

    if name:
        if context_symbol and not name.startswith(context_symbol) and not name.startswith(module_short + "."):
            return f"{context_symbol}.{name}"
        if not context_symbol and name != module_short and not name.startswith(module_short + "."):
            return f"{module_short}.{name}"
        return name

    code = first_code_span(text_raw)
    if code:
        base = code.split("(")[0].split("[")[0].strip()
        if base.startswith("new "):
            base = base[4:]
        if context_symbol and not base.startswith(context_symbol) and not base.startswith(module_short + "."):
            return f"{context_symbol}.{base}"
        if not context_symbol and not base.startswith(module_short):
            return f"{module_short}.{base}"
        return base

    return context_symbol or module_short


def normalize_kind(entry: dict[str, Any]) -> str:
    type_name = str(entry.get("type") or "")
    if type_name in INTERESTING_TYPES:
        return type_name
    return "misc"


def module_id_for_source(source: str) -> str | None:
    stem = pathlib.Path(source).stem
    return SOURCE_TO_MODULE.get(stem)


def find_special_module_override(module_id: str, entry: dict[str, Any]) -> str | None:
    text = " ".join(filter(None, [str(entry.get("textRaw") or ""), str(entry.get("name") or "")]))
    lowered = text.lower()
    if "test/reporters" in lowered:
        return "node:test/reporters"
    if "node:sys" in lowered or lowered == "sys":
        return "node:sys"
    if "node:constants" in lowered or lowered == "constants":
        return "node:constants"
    if module_id == "node:fs" and "fs/promises" in lowered:
        return "node:fs/promises"
    return None


def walk_node_entries(
    module_id: str,
    entry: dict[str, Any],
    rows: dict[tuple[str, str, str], dict[str, str]],
    context_symbol: str | None = None,
) -> None:
    override_module = find_special_module_override(module_id, entry)
    if override_module and override_module in TARGET_MODULES:
        module_id = override_module
        context_symbol = None

    entry_type = str(entry.get("type") or "")
    current_symbol = context_symbol
    if entry_type in INTERESTING_TYPES:
        symbol = normalize_symbol(module_id, entry, context_symbol)
        row_key = (module_id, symbol, normalize_kind(entry))
        if row_key not in rows:
            rows[row_key] = {
                "module": module_id,
                "symbol": symbol,
                "kind": normalize_kind(entry),
                "added_in": extract_added(entry),
                "deprecated_in": "",
                "node_status": normalize_node_status(entry),
                "notes": " | ".join(
                    part for part in [str(entry.get("stabilityText") or "").strip(), str(entry.get("source") or "").strip()] if part
                ),
            }
        current_symbol = symbol if entry_type == "class" else context_symbol

    for key in NESTED_KEYS:
        for child in entry.get(key, []) or []:
            walk_node_entries(module_id, child, rows, current_symbol)


def build_node_symbol_inventory(document: RemoteDocument) -> list[dict[str, str]]:
    rows: dict[tuple[str, str, str], dict[str, str]] = {}
    covered_modules: set[str] = set()

    for entry in document.body.get("modules", []):
        source = str(entry.get("source") or "")
        module_id = module_id_for_source(source)
        if module_id and module_id in TARGET_MODULES:
            covered_modules.add(module_id)
            walk_node_entries(module_id, entry, rows)

    for module_id, source in SOURCE_ONLY_MODULES.items():
        if module_id in TARGET_MODULES and module_id not in covered_modules:
            rows[(module_id, module_id, "module")] = {
                "module": module_id,
                "symbol": module_id,
                "kind": "module",
                "added_in": "",
                "deprecated_in": "",
                "node_status": "deprecated" if module_id == "node:sys" else "stable",
                "notes": f"placeholder module row; source mapping unresolved in {source}",
            }

    for module_id in TARGET_MODULES:
        if not any(row["module"] == module_id for row in rows.values()):
            rows[(module_id, module_id, "module")] = {
                "module": module_id,
                "symbol": module_id,
                "kind": "module",
                "added_in": "",
                "deprecated_in": "",
                "node_status": "stable",
                "notes": "placeholder module row; symbol extraction unresolved in first generated baseline",
            }

    result = sorted(rows.values(), key=lambda row: (row["module"], row["symbol"], row["kind"]))
    return result


def parse_deno_module_sections(html_text: str) -> dict[str, dict[str, Any]]:
    sections: dict[str, dict[str, Any]] = {}
    for match in MODULE_SECTION_RE.finditer(html_text):
        module_id = html.unescape(match.group("module"))
        if module_id not in TARGET_MODULES:
            continue
        raw_content = match.group("content")
        plain_text = html.unescape(STRIP_TAGS_RE.sub(" ", raw_content))
        plain_text = " ".join(plain_text.split())
        notes = []
        for note_match in STRONG_NOTE_RE.finditer(raw_content):
            subject = html.unescape(note_match.group(1)).strip()
            text = html.unescape(STRIP_TAGS_RE.sub(" ", note_match.group(2))).strip()
            notes.append({"subject": subject, "text": " ".join(text.split())})

        if not plain_text:
            docs_status = "supported"
        elif "all exports are non-functional stubs" in plain_text.lower():
            docs_status = "stub-only"
        else:
            docs_status = "partial"

        sections[module_id] = {
            "docs_status": docs_status,
            "docs_notes": notes,
            "docs_text": plain_text,
        }
    return sections


def parse_deno_module_sources() -> dict[str, dict[str, str]]:
    lib_rs = (DENO_REPO / "ext" / "node" / "lib.rs").read_text()
    source_map: dict[str, dict[str, str]] = {}
    pattern = re.compile(r'"(node:[^"]+)"\s*=\s*"([^"]+)"')
    for module_id, rel_path in pattern.findall(lib_rs):
        if module_id not in TARGET_MODULES:
            continue
        source_map[module_id] = {
            "source_present": "yes",
            "implementation_path": f"ext/node/polyfills/{rel_path}",
        }
    return source_map


def build_deno_inventory(deno_docs: RemoteDocument) -> list[dict[str, str]]:
    sections = parse_deno_module_sections(deno_docs.body)
    source_map = parse_deno_module_sources()
    rows = []
    for module_id in sorted(TARGET_MODULES):
        source_info = source_map.get(module_id, {})
        docs_info = sections.get(module_id, {})
        rows.append(
            {
                "module": module_id,
                "source_present": source_info.get("source_present", "no"),
                "implementation_path": source_info.get("implementation_path", ""),
                "docs_status": docs_info.get("docs_status", "not_listed"),
                "docs_text": docs_info.get("docs_text", ""),
                "docs_notes": " | ".join(
                    f"{note['subject']}: {note['text']}" for note in docs_info.get("docs_notes", [])
                ),
            }
        )
    return rows


def infer_deno_coverage(module_id: str, symbol: str, deno_inventory_by_module: dict[str, dict[str, str]]) -> str:
    row = deno_inventory_by_module.get(module_id)
    if not row or row.get("source_present") != "yes":
        return "NotImplemented"

    docs_status = row.get("docs_status", "")
    if docs_status == "stub-only":
        return "StubOnly"

    docs_notes = row.get("docs_notes", "")
    docs_text = row.get("docs_text", "")
    tail = symbol.split(".")[-1]
    if tail and (f"{tail}:" in docs_notes or tail in docs_text):
        return "ImplementedPartial"
    if docs_status == "partial":
        return "ImplementedPartial"
    return "NeedsVerification"


def support_state_from_coverage(coverage: str) -> str:
    if coverage == "NotImplemented":
        return "NotSupported"
    if coverage == "StubOnly":
        return "StubOnly"
    if coverage == "ImplementedPartial":
        return "Partial"
    return "NeedsVerification"


def build_delta_rows(node20_rows: list[dict[str, str]], node22_rows: list[dict[str, str]]) -> list[dict[str, str]]:
    key = lambda row: (row["module"], row["symbol"], row["kind"])
    node20 = {key(row): row for row in node20_rows}
    node22 = {key(row): row for row in node22_rows}
    keys = sorted(set(node20) | set(node22))
    delta_rows = []
    for item in keys:
        row20 = node20.get(item)
        row22 = node22.get(item)
        if row20 and row22:
            if row20["node_status"] != row22["node_status"]:
                delta_kind = "status_changed"
            else:
                delta_kind = "unchanged"
        elif row22:
            delta_kind = "added_in_node22"
        else:
            delta_kind = "removed_after_node20"
        delta_rows.append(
            {
                "module": item[0],
                "symbol": item[1],
                "kind": item[2],
                "delta": delta_kind,
                "node20_status": row20["node_status"] if row20 else "",
                "node22_status": row22["node_status"] if row22 else "",
                "node20_added_in": row20["added_in"] if row20 else "",
                "node22_added_in": row22["added_in"] if row22 else "",
            }
        )
    return delta_rows


def build_matrix_rows(
    node20_rows: list[dict[str, str]],
    node22_rows: list[dict[str, str]],
    deno_inventory: list[dict[str, str]],
) -> list[dict[str, str]]:
    key = lambda row: (row["module"], row["symbol"], row["kind"])
    node20 = {key(row): row for row in node20_rows}
    node22 = {key(row): row for row in node22_rows}
    deno_inventory_by_module = {row["module"]: row for row in deno_inventory}
    keys = sorted(set(node20) | set(node22))
    matrix_rows = []
    for item in keys:
        row20 = node20.get(item)
        row22 = node22.get(item)
        deno_coverage = infer_deno_coverage(item[0], item[1], deno_inventory_by_module)
        for runtime_preset in RUNTIME_PRESETS:
            matrix_rows.append(
                {
                    "module": item[0],
                    "symbol": item[1],
                    "kind": item[2],
                    "node20_status": row20["node_status"] if row20 else "",
                    "node22_status": row22["node_status"] if row22 else "",
                    "added_in": row22["added_in"] if row22 else (row20["added_in"] if row20 else ""),
                    "deprecated_in": row22["deprecated_in"] if row22 else (row20["deprecated_in"] if row20 else ""),
                    "deno_coverage": deno_coverage,
                    "verification_status": "NeedsVerification",
                    "notes": " | ".join(
                        part
                        for part in [
                            row20["notes"] if row20 else "",
                            row22["notes"] if row22 and row22 != row20 else "",
                            deno_inventory_by_module.get(item[0], {}).get("docs_notes", ""),
                        ]
                        if part
                    ),
                    "compatibility_target": COMPATIBILITY_TARGET,
                    "runtime_preset": runtime_preset,
                    "support_state": support_state_from_coverage(deno_coverage),
                    "verification_lane": VERIFICATION_LANE,
                }
            )
    return matrix_rows


def metadata_comments(metadata: dict[str, str]) -> list[str]:
    return [f"# {key}={value}" for key, value in metadata.items()]


def write_csv(path: pathlib.Path, rows: list[dict[str, str]], metadata: dict[str, str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="", encoding="utf-8") as handle:
        for line in metadata_comments(metadata):
            handle.write(line + "\n")
        writer = csv.DictWriter(handle, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)


def build_summary(
    metadata: dict[str, str],
    node20_rows: list[dict[str, str]],
    node22_rows: list[dict[str, str]],
    deno_inventory: list[dict[str, str]],
    delta_rows: list[dict[str, str]],
    matrix_rows: list[dict[str, str]],
) -> str:
    unresolved_modules = sorted(
        {
            row["module"]
            for row in matrix_rows
            if row["support_state"] == "NeedsVerification"
        }
    )
    added_in_node22 = [row for row in delta_rows if row["delta"] == "added_in_node22"]
    partial_or_stub = [row for row in deno_inventory if row["docs_status"] in {"partial", "stub-only"}]
    module_counts_20: dict[str, int] = {}
    module_counts_22: dict[str, int] = {}
    for row in node20_rows:
        module_counts_20[row["module"]] = module_counts_20.get(row["module"], 0) + 1
    for row in node22_rows:
        module_counts_22[row["module"]] = module_counts_22.get(row["module"], 0) + 1
    deno_inventory_by_module = {row["module"]: row for row in deno_inventory}
    lines = [
        "# Node LTS Compatibility Summary",
        "",
        "Generated machine-owned baseline for NLC1.",
        "",
        "## Metadata",
        "",
    ]
    for key, value in metadata.items():
        lines.append(f"- `{key}`: `{value}`")
    lines.extend(
        [
            "",
            "## Counts",
            "",
            f"- Node 20 symbol rows: `{len(node20_rows)}`",
            f"- Node 22 symbol rows: `{len(node22_rows)}`",
            f"- Node 20 → Node 22 delta rows: `{len(delta_rows)}`",
            f"- Deno module inventory rows: `{len(deno_inventory)}`",
            f"- Joined compatibility matrix rows: `{len(matrix_rows)}`",
            "",
            "## Initial Findings",
            "",
            f"- Node 22-only symbol rows: `{len(added_in_node22)}`",
            f"- Deno docs modules with partial/stub caveats: `{len(partial_or_stub)}`",
            f"- Modules still starting at `NeedsVerification`: `{len(unresolved_modules)}`",
            "",
            "## Modules With Published Deno Caveats",
            "",
        ]
    )
    for row in partial_or_stub:
        lines.append(f"- `{row['module']}`: `{row['docs_status']}`")
    lines.extend(
        [
            "",
            "## Per-Module Baseline Snapshot",
            "",
            "| Module | Node 20 symbols | Node 22 symbols | Deno docs status | First-baseline support state |",
            "| --- | ---: | ---: | --- | --- |",
        ]
    )
    for module in TARGET_MODULES:
        deno_row = deno_inventory_by_module.get(module, {})
        support_state = "NeedsVerification"
        if deno_row:
            support_state = support_state_from_coverage(
                infer_deno_coverage(module, module, deno_inventory_by_module)
            )
        lines.append(
            f"| `{module}` | `{module_counts_20.get(module, 0)}` | `{module_counts_22.get(module, 0)}` | "
            f"`{deno_row.get('docs_status', 'not_listed')}` | `{support_state}` |"
        )
    lines.extend(
        [
            "",
            "## First-Baseline Caveats",
            "",
            "- This first generated baseline is intentionally conservative.",
            "- Module and symbol coverage unresolved from the source scrape remain `NeedsVerification` instead of being guessed.",
            "- `support_state` values in this baseline are source- and docs-derived starting points; NLC2 and later family items must refine them with measured Nimbus verification.",
        ]
    )
    return "\n".join(lines) + "\n"


def main() -> int:
    generated_at = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()
    node20 = fetch_json(NODE20_URL, "node20")
    node22 = fetch_json(NODE22_URL, "node22")
    deno_docs = fetch_text(DENO_COMPAT_URL, "deno_node_compat")

    node20_rows = build_node_symbol_inventory(node20)
    node22_rows = build_node_symbol_inventory(node22)
    deno_inventory = build_deno_inventory(deno_docs)
    delta_rows = build_delta_rows(node20_rows, node22_rows)
    matrix_rows = build_matrix_rows(node20_rows, node22_rows, deno_inventory)

    deno_git_commit = git_output("git", "rev-parse", "HEAD", cwd=DENO_REPO)
    deno_git_branch = git_output("git", "branch", "--show-current", cwd=DENO_REPO)

    metadata = {
        "generated_at_utc": generated_at,
        "node20_url": node20.url,
        "node20_etag": node20.etag,
        "node20_last_modified": node20.last_modified,
        "node22_url": node22.url,
        "node22_etag": node22.etag,
        "node22_last_modified": node22.last_modified,
        "deno_compat_url": deno_docs.url,
        "deno_compat_etag": deno_docs.etag,
        "deno_compat_last_modified": deno_docs.last_modified,
        "deno_repo": str(DENO_REPO),
        "deno_git_branch": deno_git_branch,
        "deno_git_commit": deno_git_commit,
        "generator_path": str(pathlib.Path(__file__).relative_to(REPO_ROOT)),
    }

    write_csv(OUTPUT_ROOT / "node20-symbols.csv", node20_rows, metadata)
    write_csv(OUTPUT_ROOT / "node22-symbols.csv", node22_rows, metadata)
    write_csv(OUTPUT_ROOT / "node20-vs-node22-delta.csv", delta_rows, metadata)
    write_csv(OUTPUT_ROOT / "deno-node-impl-inventory.csv", deno_inventory, metadata)
    write_csv(OUTPUT_ROOT / "node-lts-compat-matrix.csv", matrix_rows, metadata)
    (OUTPUT_ROOT / "node-lts-compat-summary.md").write_text(
        build_summary(metadata, node20_rows, node22_rows, deno_inventory, delta_rows, matrix_rows),
        encoding="utf-8",
    )

    return 0


if __name__ == "__main__":
    sys.exit(main())
