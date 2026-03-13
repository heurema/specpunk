#!/usr/bin/env python3
"""Generate a minimal review artifact from structured task input."""

from __future__ import annotations

import argparse
import fnmatch
import json
import sys
from pathlib import Path, PurePosixPath
from typing import Any


def normalize_path(value: str) -> str:
    return str(PurePosixPath(value.strip()))


def ensure_string_list(value: Any, field_name: str) -> list[str]:
    if value is None:
        return []
    if not isinstance(value, list) or any(not isinstance(item, str) for item in value):
        raise ValueError(f"Field '{field_name}' must be a list of strings.")
    return [normalize_path(item) if field_name != "evidence" else item.strip() for item in value]


def load_task(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text())
    except FileNotFoundError as exc:
        raise ValueError(f"Task file not found: {path}") from exc
    except json.JSONDecodeError as exc:
        raise ValueError(f"Task file is not valid JSON: {path}") from exc

    if not isinstance(payload, dict):
        raise ValueError("Task input must be a JSON object.")

    task_name = payload.get("task", "").strip()
    if not task_name:
        raise ValueError("Field 'task' is required.")

    allowed = ensure_string_list(payload.get("allowed"), "allowed")
    changed = ensure_string_list(payload.get("changed"), "changed")
    blocked = ensure_string_list(payload.get("blocked"), "blocked")
    evidence = ensure_string_list(payload.get("evidence"), "evidence")

    if not allowed:
        raise ValueError("Field 'allowed' must include at least one path or pattern.")
    if not changed:
        raise ValueError("Field 'changed' must include at least one changed file.")

    return {
        "task": task_name,
        "allowed": dedupe(allowed),
        "changed": dedupe(changed),
        "blocked": dedupe(blocked),
        "evidence": dedupe(evidence),
    }


def dedupe(items: list[str]) -> list[str]:
    seen: set[str] = set()
    result: list[str] = []
    for item in items:
        if item in seen:
            continue
        seen.add(item)
        result.append(item)
    return result


def matches_any(path: str, patterns: list[str]) -> bool:
    return any(fnmatch.fnmatch(path, pattern) for pattern in patterns)


def classify(task: dict[str, Any]) -> dict[str, Any]:
    blocked_touched = [path for path in task["changed"] if matches_any(path, task["blocked"])]
    in_scope = [
        path
        for path in task["changed"]
        if matches_any(path, task["allowed"]) and path not in blocked_touched
    ]
    out_of_scope = [path for path in task["changed"] if path not in in_scope]

    scope_status = "respected" if not out_of_scope else "drifted"

    if blocked_touched:
        decision = "inspect"
        reason = "blocked files were touched"
    elif out_of_scope:
        decision = "inspect"
        reason = "changed files exceeded the declared scope"
    elif task["evidence"]:
        decision = "approve"
        reason = "scope stayed bounded and evidence is attached"
    else:
        decision = "inspect"
        reason = "scope stayed bounded but evidence is missing"

    return {
        "scope_status": scope_status,
        "decision": decision,
        "reason": reason,
        "in_scope": in_scope,
        "out_of_scope": out_of_scope,
        "blocked_touched": blocked_touched,
    }


def format_bullets(items: list[str], empty_label: str) -> str:
    if not items:
        return f"- {empty_label}"
    return "\n".join(f"- `{item}`" for item in items)


def render_review(task: dict[str, Any], result: dict[str, Any]) -> str:
    return f"""# Generated Review Artifact

Task: {task["task"]}
Decision: {result["decision"]}
Reason: {result["reason"]}

## Scope Summary

- Declared allowed patterns: {len(task["allowed"])}
- Declared blocked patterns: {len(task["blocked"])}
- Changed files: {len(task["changed"])}
- In scope: {len(result["in_scope"])}
- Out of scope: {len(result["out_of_scope"])}
- Blocked touched: {len(result["blocked_touched"])}
- Scope status: {result["scope_status"]}

## Allowed Patterns

{format_bullets(task["allowed"], "none")}

## Blocked Patterns

{format_bullets(task["blocked"], "none")}

## Changed Files

{format_bullets(task["changed"], "none")}

## Out Of Scope Files

{format_bullets(result["out_of_scope"], "none")}

## Evidence

{format_bullets(task["evidence"], "none")}

## Reviewer Posture

- {"approve the bounded change" if result["decision"] == "approve" else "inspect the change before approval"}
- {"scope stayed within the declared boundary" if result["scope_status"] == "respected" else "scope drift is visible and must be understood"}
- {"evidence is attached to support the change" if task["evidence"] else "evidence still needs to be attached"}
"""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate a minimal review artifact from a structured task JSON file."
    )
    parser.add_argument("--task", required=True, help="Path to the structured task JSON file.")
    parser.add_argument(
        "--output",
        help="Path to write the markdown artifact. If omitted, prints to stdout.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    try:
        task = load_task(Path(args.task))
        result = classify(task)
        content = render_review(task, result)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    if args.output:
        output_path = Path(args.output)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(content)
    else:
        sys.stdout.write(content)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
