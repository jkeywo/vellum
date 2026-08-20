"""Authored-content extraction for RON (Rusty Object Notation) files.

A deliberately small reader: enough to resolve named fields, nested
structs, and arrays of structs in the fleet's tuning and scenario files.
It is not a full RON implementation — unsupported constructs raise
ContentParseError rather than mis-reading silently.
"""

from __future__ import annotations

import re
from typing import Any

from pasm.scanners.content_toml import ContentDocument, ContentParseError, ContentRow


_TOKEN_RE = re.compile(
    r"""
    (?P<comment>//[^\n]*|/\*.*?\*/)
  | (?P<string>"(?:[^"\\]|\\.)*")
  | (?P<number>-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)
  | (?P<ident>[A-Za-z_][A-Za-z0-9_]*)
  | (?P<punct>[()\[\]:,{}])
  | (?P<ws>\s+)
""",
    re.VERBOSE | re.DOTALL,
)


class _Tokens:
    def __init__(self, text: str) -> None:
        self.items: list[tuple[str, str, int]] = []
        line = 1
        position = 0
        for match in _TOKEN_RE.finditer(text):
            if match.start() != position:
                raise ContentParseError(
                    f"Unrecognised RON syntax near line {line}.", line=line
                )
            position = match.end()
            kind = match.lastgroup
            value = match.group()
            if kind not in {"comment", "ws"}:
                self.items.append((kind, value, line))
            line += value.count("\n")
        if position != len(text):
            raise ContentParseError(f"Unrecognised RON syntax near line {line}.", line=line)
        self.index = 0

    def peek(self):
        return self.items[self.index] if self.index < len(self.items) else (None, None, None)

    def next(self):
        item = self.peek()
        if item[0] is None:
            raise ContentParseError("Unexpected end of RON document.")
        self.index += 1
        return item

    def expect(self, value: str):
        kind, actual, line = self.next()
        if actual != value:
            raise ContentParseError(f"Expected '{value}' but found '{actual}' at line {line}.", line=line)


def extract_ron_document(text: str) -> ContentDocument:
    tokens = _Tokens(text)
    value, _line = _parse_value(tokens)
    if tokens.peek()[0] is not None:
        kind, extra, line = tokens.peek()
        raise ContentParseError(f"Trailing RON content '{extra}' at line {line}.", line=line)
    data = value if isinstance(value, dict) else {"root": value}

    array_rows: dict[str, tuple[ContentRow, ...]] = {}
    _collect_rows(data, "", array_rows)
    return ContentDocument(data=data, array_rows=array_rows, script_functions=())


def _parse_value(tokens) -> tuple[Any, int]:
    kind, value, line = tokens.next()
    if kind == "string":
        return value[1:-1].replace('\\"', '"').replace("\\\\", "\\"), line
    if kind == "number":
        number = float(value)
        return (int(number) if number.is_integer() and "." not in value and "e" not in value.lower() else number), line
    if kind == "ident":
        if value in {"true", "false"}:
            return value == "true", line
        # Enum variant or wrapper: Name or Name(...); Some(x) unwraps to x.
        if tokens.peek()[1] == "(":
            tokens.next()
            inner: list[Any] = []
            while tokens.peek()[1] != ")":
                item, _ = _parse_value(tokens)
                inner.append(item)
                if tokens.peek()[1] == ",":
                    tokens.next()
            tokens.expect(")")
            if value == "Some" and len(inner) == 1:
                return inner[0], line
            if value == "None":
                return None, line
            return f"{value}({', '.join(str(item) for item in inner)})", line
        return value, line
    if value == "(":
        # Struct (fields with names) or tuple (positional values).
        if tokens.peek()[1] == ")":
            tokens.next()
            return {}, line
        is_struct = tokens.peek()[0] == "ident" and tokens.items[tokens.index + 1][1] == ":"
        if is_struct:
            fields: dict[str, Any] = {}
            while tokens.peek()[1] != ")":
                _, field_name, _ = tokens.next()
                tokens.expect(":")
                fields[field_name], _ = _parse_value(tokens)
                if tokens.peek()[1] == ",":
                    tokens.next()
            tokens.expect(")")
            return fields, line
        items = []
        while tokens.peek()[1] != ")":
            item, _ = _parse_value(tokens)
            items.append(item)
            if tokens.peek()[1] == ",":
                tokens.next()
        tokens.expect(")")
        return items, line
    if value == "[":
        items = []
        while tokens.peek()[1] != "]":
            item, _ = _parse_value(tokens)
            items.append(item)
            if tokens.peek()[1] == ",":
                tokens.next()
        tokens.expect("]")
        return items, line
    if value == "{":
        mapping: dict[Any, Any] = {}
        while tokens.peek()[1] != "}":
            key, _ = _parse_value(tokens)
            tokens.expect(":")
            mapping[key], _ = _parse_value(tokens)
            if tokens.peek()[1] == ",":
                tokens.next()
        tokens.expect("}")
        return mapping, line
    raise ContentParseError(f"Unexpected RON token '{value}' at line {line}.", line=line)


def _collect_rows(value: Any, prefix: str, out: dict[str, tuple[ContentRow, ...]]) -> None:
    if isinstance(value, dict):
        for name, item in value.items():
            if not isinstance(name, str):
                continue
            dotted = f"{prefix}.{name}" if prefix else name
            if isinstance(item, list) and item and all(isinstance(entry, dict) for entry in item):
                out[dotted] = tuple(
                    ContentRow(table=dotted, values=entry, line=None) for entry in item
                )
                for entry in item:
                    _collect_rows(entry, dotted, out)
            else:
                _collect_rows(item, dotted, out)
