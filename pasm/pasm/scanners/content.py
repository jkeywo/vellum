"""Suffix-dispatched authored-content extraction.

One entry point for design validators and tooling: TOML worlds and RON
tuning/scenario files today; a new suffix means a new extractor module and
one line here.
"""

from __future__ import annotations

from pathlib import Path

from pasm.scanners.content_toml import ContentDocument, ContentParseError, extract_content_document
from pasm.scanners.content_ron import extract_ron_document


def extract_content(text: str, suffix: str) -> ContentDocument:
    if suffix.lower() == ".ron":
        return extract_ron_document(text)
    return extract_content_document(text)


def load_content_document(path: Path) -> ContentDocument:
    """Read and extract; raises OSError or ContentParseError."""
    return extract_content(path.read_text(encoding="utf-8"), path.suffix)


__all__ = ["ContentDocument", "ContentParseError", "extract_content", "load_content_document"]
