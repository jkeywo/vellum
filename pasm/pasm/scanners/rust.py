from __future__ import annotations

import re


# `pub` carries an optional restriction: pub(crate), pub(super), pub(in path).
# Matching only bare `pub` silently drops every restricted item, so a spec that
# declares a `pub(crate) fn` gets a missing-observed-symbol finding against code
# that is right there. Crate-private surface is the norm in a workspace, so this
# is most of the symbols in some modules rather than an edge case.
_RUST_VIS = r"(?:pub(?:\([^)]*\))?\s+)?"

RUST_SYMBOL_RE = re.compile(
    r"^\s*"
    + _RUST_VIS
    + r"(?:(?:async\s+)?fn|struct|enum|trait|type|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)",
    re.MULTILINE,
)
RUST_USE_RE = re.compile(r"^\s*use\s+((?:crate|self|super)::[A-Za-z_][A-Za-z0-9_:]*)", re.MULTILINE)
RUST_MOD_RE = re.compile(r"^\s*" + _RUST_VIS + r"mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;", re.MULTILINE)


def scan_rust_symbols(text: str) -> list[tuple[str, int]]:
    return _scan_symbols(text, RUST_SYMBOL_RE)


def scan_rust_imports(text: str) -> list[tuple[str, str, int]]:
    imports = _scan_imports(text, RUST_USE_RE, "rust-use")
    imports.extend(_scan_imports(text, RUST_MOD_RE, "rust-mod"))
    return imports


def _scan_symbols(text: str, pattern: re.Pattern[str]) -> list[tuple[str, int]]:
    symbols: list[tuple[str, int]] = []
    for match in pattern.finditer(text):
        line = text.count("\n", 0, match.start()) + 1
        symbols.append((match.group(1), line))
    return symbols


def _scan_imports(text: str, pattern: re.Pattern[str], kind: str) -> list[tuple[str, str, int]]:
    return [
        (kind, match.group(1), text.count("\n", 0, match.start()) + 1)
        for match in pattern.finditer(text)
    ]
