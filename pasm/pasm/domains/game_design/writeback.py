"""Guarded write-back from the design layer into authoritative content.

The authored file is truth, so writes are surgical: value substitutions and
block insertions edit the text in place, never re-serialize — authored
comments survive byte-identical. Every application is guarded by the
content hash captured at digest time, refuses values outside declared
design bounds (the AI is bounded by intent; humans edit the file directly),
and reparses before writing so a mangled edit never lands.
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass
from pathlib import Path

from pasm.core.model import SpecEntity
from pasm.scanners.content import ContentParseError, extract_content


class WritebackError(Exception):
    pass


@dataclass(frozen=True)
class AppliedChange:
    op: str
    description: str


def apply_writeback(
    changes: dict,
    workspace_root: Path,
    entities: tuple[SpecEntity, ...] = (),
    dry_run: bool = False,
) -> list[AppliedChange]:
    file_value = changes.get("file")
    if not isinstance(file_value, str):
        raise WritebackError("The changes document must name the content 'file'.")
    content_path = (workspace_root / file_value).resolve()
    if not content_path.is_file():
        raise WritebackError(f"Content file '{file_value}' does not exist under the workspace root.")

    # Exact I/O: newline='' keeps the file's own line endings in the string,
    # so the hash matches the bytes on disk and unchanged lines survive
    # byte-identical on write.
    with open(content_path, "r", encoding="utf-8", newline="") as handle:
        text = handle.read()
    expected = changes.get("expected_sha256")
    if not isinstance(expected, str):
        raise WritebackError("The changes document must carry 'expected_sha256' from the digest.")
    actual = hashlib.sha256(text.encode("utf-8")).hexdigest()
    if actual != expected:
        raise WritebackError(
            "Content hash mismatch: the file changed since the digest was taken. "
            "Re-run `pasm design digest` and rebase the changes."
        )

    bounds = _collect_bounds(entities, file_value)
    applied: list[AppliedChange] = []
    for index, change in enumerate(changes.get("changes", [])):
        op = change.get("op")
        if op == "set_value":
            text, description = _set_value(text, change, bounds)
        elif op == "insert_row":
            text, description = _insert_row(text, change)
        elif op == "remove_row":
            text, description = _remove_row(text, change)
        elif op == "append_handler":
            text, description = _append_handler(text, change)
        else:
            raise WritebackError(f"Change #{index + 1} has unknown op '{op}'.")
        applied.append(AppliedChange(op=op, description=description))

    try:
        extract_content(text, content_path.suffix)
    except ContentParseError as exc:
        raise WritebackError(f"The edited content no longer parses ({exc}); nothing was written.") from exc

    if not dry_run:
        with open(content_path, "w", encoding="utf-8", newline="") as handle:
            handle.write(text)
    return applied


def _collect_bounds(entities, file_value: str):
    bounds = []
    for entity in entities:
        design = entity.game_design
        if design is None:
            continue
        for anchor in design.anchors:
            if anchor.path == file_value and (anchor.min is not None or anchor.max is not None):
                bounds.append(anchor)
    return bounds


def _row_span(text: str, table: str, match: str | None) -> tuple[int, int]:
    """Return (start, end) line indices (0-based, end exclusive) of the row."""
    lines = text.split("\n")
    header = f"[[{table}]]"
    spans: list[tuple[int, int]] = []
    start: int | None = None
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("[") and start is not None:
            spans.append((start, index))
            start = None
        if stripped == header or stripped.startswith(header + " ") or stripped.split("#")[0].strip() == header:
            start = index
    if start is not None:
        spans.append((start, len(lines)))
    if match is None:
        if len(spans) != 1:
            raise WritebackError(
                f"Table '{table}' has {len(spans)} rows; a 'match' selector is required."
            )
        return spans[0]
    field_name, _, expected = match.partition("=")
    pattern = re.compile(
        rf"^\s*{re.escape(field_name.strip())}\s*=\s*\"?{re.escape(expected.strip())}\"?\s*(#.*)?$"
    )
    for span in spans:
        if any(pattern.match(lines[i]) for i in range(span[0], span[1])):
            return span
    raise WritebackError(f"No row in table '{table}' matches '{match}'.")


def _set_value(text: str, change: dict, bounds) -> tuple[str, str]:
    table, match, key = change.get("table"), change.get("match"), change.get("key")
    value = change.get("value")
    if not isinstance(table, str) or not isinstance(key, str) or value is None:
        raise WritebackError("set_value needs 'table', 'key', and 'value'.")

    numeric = _as_number(value)
    for anchor in bounds:
        if anchor.table == table and anchor.key == key and (anchor.match or None) == (match or None):
            if numeric is None:
                raise WritebackError(f"Value '{value}' for bounded anchor '{anchor.name}' is not numeric.")
            if anchor.min is not None and numeric < float(anchor.min):
                raise WritebackError(
                    f"Refusing write: {value} is below design min {anchor.min} for anchor '{anchor.name}'. "
                    "Update the declared design intent first, or leave the tuning to a human."
                )
            if anchor.max is not None and numeric > float(anchor.max):
                raise WritebackError(
                    f"Refusing write: {value} is above design max {anchor.max} for anchor '{anchor.name}'. "
                    "Update the declared design intent first, or leave the tuning to a human."
                )

    lines = text.split("\n")
    if table in ("", None):
        # Top-level key (RON tuning files): the whole document is the row.
        start, end = 0, len(lines)
    else:
        start, end = _row_span(text, table, match)
    # TOML `key = value  # comment` and RON `key: value, // comment` both match.
    assignment = re.compile(
        rf"^(\s*{re.escape(key)}\s*[:=]\s*)([^,#/]*?)(,?\s*)((?:#|//).*)?(\r?)$"
    )
    for index in range(start, end):
        found = assignment.match(lines[index])
        if found is None:
            continue
        literal = _to_toml_literal(value, found.group(2).strip())
        trailing = f"{found.group(3)}{found.group(4) or ''}{found.group(5)}"
        lines[index] = f"{found.group(1)}{literal}{trailing}"
        return "\n".join(lines), f"{table or '<root>'}[{match}].{key} = {literal}"
    raise WritebackError(f"Key '{key}' was not found in the matched '{table}' row.")


def _insert_row(text: str, change: dict) -> tuple[str, str]:
    table, values = change.get("table"), change.get("values")
    if not isinstance(table, str) or not isinstance(values, dict) or not values:
        raise WritebackError("insert_row needs 'table' and a non-empty 'values' mapping.")
    eol = "\r" if "\r\n" in text else ""
    block = [f"[[{table}]]{eol}"] + [f"{key} = {_to_toml_literal(value)}{eol}" for key, value in values.items()]

    lines = text.split("\n")
    header = f"[[{table}]]"
    insert_at: int | None = None
    current: int | None = None
    for index, line in enumerate(lines):
        stripped = line.split("#")[0].strip()
        if stripped.startswith("[") and current is not None:
            insert_at = index
            current = None
        if stripped == header:
            current = index
    if current is not None:
        insert_at = len(lines)
    if insert_at is None:
        # Table absent: insert before [script] so data stays ahead of code.
        insert_at = next(
            (index for index, line in enumerate(lines) if line.split("#")[0].strip() == "[script]"),
            len(lines),
        )
    new_lines = lines[:insert_at] + block + [eol] + lines[insert_at:]
    return "\n".join(new_lines), f"inserted [[{table}]] row ({', '.join(values)})"


def _remove_row(text: str, change: dict) -> tuple[str, str]:
    table, match = change.get("table"), change.get("match")
    if not isinstance(table, str) or not isinstance(match, str):
        raise WritebackError("remove_row needs 'table' and 'match'.")
    start, end = _row_span(text, table, match)
    lines = text.split("\n")
    while end < len(lines) and not lines[end].strip():
        end += 1
    return "\n".join(lines[:start] + lines[end:]), f"removed [[{table}]] row matching '{match}'"


def _append_handler(text: str, change: dict) -> tuple[str, str]:
    name, comment = change.get("name"), change.get("comment")
    if not isinstance(name, str):
        raise WritebackError("append_handler needs the handler 'name'.")
    if re.search(rf"^\s*fn\s+{re.escape(name)}\s*\(", text, re.MULTILINE):
        raise WritebackError(f"Handler '{name}' already exists; write-back never edits existing functions.")

    script_at = text.find("[script]")
    if script_at == -1:
        raise WritebackError("No [script] section to append a handler to.")
    closing = max(text.rfind("'''"), text.rfind('"""'))
    if closing <= script_at:
        raise WritebackError("Could not locate the script block's closing delimiter.")

    eol = "\r\n" if "\r\n" in text else "\n"
    stub_lines = []
    if isinstance(comment, str) and comment:
        stub_lines.extend(f"// {line}" for line in comment.splitlines())
    stub_lines.extend([f"fn {name}(ctx) {{", "    // TODO: implement; wired from the design model.", "}"])
    stub = eol + eol.join(stub_lines) + eol
    return text[:closing] + stub + text[closing:], f"appended handler stub fn {name}"


def _as_number(value) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str):
        try:
            return float(value)
        except ValueError:
            return None
    return None


def _to_toml_literal(value, previous: str | None = None) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(int(value)) if float(value).is_integer() else str(value)
    text = str(value)
    number = _as_number(text)
    if number is not None and (previous is None or not previous.startswith('"')):
        return str(int(number)) if number.is_integer() else str(number)
    return '"' + text.replace('"', '\\"') + '"'
