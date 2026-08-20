"""Authored-content extraction for TOML world files.

Distinct from `pasm.scanners.toml`, which exposes symbols and imports for the
implementation observation layer. This module reads authored content *values*
so design validators can check spec claims against the authoritative game
data without the spec ever duplicating it.
"""

from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass
from typing import Any


_ARRAY_TABLE_RE = re.compile(r"^\s*\[\[([^\]]+)\]\]\s*(?:#.*)?$", re.MULTILINE)
_SCRIPT_FN_RE = re.compile(r"^\s*fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(", re.MULTILINE)


@dataclass(frozen=True)
class ContentRow:
    table: str
    values: dict[str, Any]
    line: int | None


@dataclass(frozen=True)
class ResolvedValue:
    value: Any
    line: int | None


@dataclass(frozen=True)
class ContentDocument:
    """A parsed authored-content file with row-level line recovery."""

    data: dict[str, Any]
    array_rows: dict[str, tuple[ContentRow, ...]]
    script_functions: tuple[tuple[str, int], ...]

    def rows(self, table: str) -> tuple[ContentRow, ...]:
        return self.array_rows.get(table, ())

    def resolve(self, table: str, match: str | None, key: str | None) -> list[ResolvedValue]:
        """Resolve values from an array-of-tables path.

        `match` selects rows with the form "field=value"; `key` names the value
        to return from each selected row. Without `key`, the row itself is the
        value, which lets callers count matching rows.
        """
        selected = list(self.rows(table))
        if not selected:
            # Not an array of tables: a dotted path to a mapping (or the root,
            # for table "") acts as a single row, so scalar tuning keys resolve.
            node = self._navigate(table)
            if isinstance(node, dict):
                selected = [ContentRow(table=table, values=node, line=None)]
        if match is not None:
            field_name, _, expected = match.partition("=")
            selected = [
                row for row in selected
                if str(row.values.get(field_name.strip())) == expected.strip()
            ]
        if key is None:
            return [ResolvedValue(value=row.values, line=row.line) for row in selected]
        return [
            ResolvedValue(value=row.values[key], line=row.line)
            for row in selected
            if key in row.values
        ]

    def _navigate(self, table: str):
        node: Any = self.data
        if table in ("", None):
            return node
        for part in table.split("."):
            if not isinstance(node, dict) or part not in node:
                return None
            node = node[part]
        return node


class ContentParseError(Exception):
    def __init__(self, message: str, line: int | None = None) -> None:
        super().__init__(message)
        self.line = line


def extract_content_document(text: str) -> ContentDocument:
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        line = getattr(exc, "lineno", None)
        raise ContentParseError(str(exc), line=line) from exc

    # tomllib drops positions, so array-of-tables rows recover their line from
    # the nth occurrence of the [[table]] header in source order.
    header_lines: dict[str, list[int]] = {}
    for header in _ARRAY_TABLE_RE.finditer(text):
        name = header.group(1).strip()
        header_lines.setdefault(name, []).append(text.count("\n", 0, header.start()) + 1)

    collected: dict[str, list[dict[str, Any]]] = {}
    for table, rows in _iter_array_tables(data):
        collected.setdefault(table, []).extend(rows)
    array_rows: dict[str, tuple[ContentRow, ...]] = {}
    for table, rows in collected.items():
        lines = header_lines.get(table, [])
        array_rows[table] = tuple(
            ContentRow(
                table=table,
                values=row,
                line=lines[index] if index < len(lines) else None,
            )
            for index, row in enumerate(rows)
        )

    script_functions = tuple(
        (match.group(1), text.count("\n", 0, match.start()) + 1)
        for match in _SCRIPT_FN_RE.finditer(text)
    )
    return ContentDocument(data=data, array_rows=array_rows, script_functions=script_functions)


def _iter_array_tables(data: dict[str, Any], prefix: str = ""):
    for name, value in data.items():
        dotted = f"{prefix}.{name}" if prefix else name
        if isinstance(value, list) and value and all(isinstance(item, dict) for item in value):
            yield dotted, value
            # Rows may hold nested arrays of tables (e.g. [[entity.overrides.x]]),
            # which source headers name by the full dotted path.
            for row in value:
                yield from _iter_array_tables(row, dotted)
        elif isinstance(value, dict):
            yield from _iter_array_tables(value, dotted)
