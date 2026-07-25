from __future__ import annotations

from pasm.scanners.rust import scan_rust_imports, scan_rust_symbols


def test_bare_pub_and_private_items_are_scanned() -> None:
    text = "\n".join(
        [
            "fn private_helper() {}",
            "pub fn exported() {}",
            "pub struct Exported;",
            "async fn waited() {}",
            "pub async fn exported_wait() {}",
            "const LIMIT: u8 = 3;",
        ]
    )
    assert [name for name, _ in scan_rust_symbols(text)] == [
        "private_helper",
        "exported",
        "Exported",
        "waited",
        "exported_wait",
        "LIMIT",
    ]


def test_restricted_visibility_items_are_scanned() -> None:
    """pub(crate) and friends are ordinary declarations, not a special case.

    A workspace that keeps its surface crate-private declares most of its
    symbols this way; missing them makes `pasm scan` report code that exists.
    """
    text = "\n".join(
        [
            "pub(crate) fn crate_visible() {}",
            "pub(super) struct SuperVisible;",
            "pub(in crate::domains) enum Scoped { A }",
            "pub(crate) async fn crate_wait() {}",
            "pub(crate) const BOUND: u8 = 7;",
        ]
    )
    assert [name for name, _ in scan_rust_symbols(text)] == [
        "crate_visible",
        "SuperVisible",
        "Scoped",
        "crate_wait",
        "BOUND",
    ]


def test_symbol_line_numbers_are_one_based() -> None:
    text = "\n".join(["// header", "pub(crate) fn second_line() {}"])
    assert scan_rust_symbols(text) == [("second_line", 2)]


def test_restricted_visibility_modules_are_scanned() -> None:
    text = "\n".join(
        [
            "mod private_mod;",
            "pub mod public_mod;",
            "pub(crate) mod crate_mod;",
            "pub(super) mod super_mod;",
        ]
    )
    mods = [(kind, name) for kind, name, _ in scan_rust_imports(text)]
    assert mods == [
        ("rust-mod", "private_mod"),
        ("rust-mod", "public_mod"),
        ("rust-mod", "crate_mod"),
        ("rust-mod", "super_mod"),
    ]


def test_use_statements_are_scanned_separately_from_mods() -> None:
    text = "\n".join(
        [
            "use crate::core::model::Entity;",
            "use super::helper;",
            "use std::collections::HashMap;",
            "pub(crate) mod scoped;",
        ]
    )
    assert [(kind, name) for kind, name, _ in scan_rust_imports(text)] == [
        ("rust-use", "crate::core::model::Entity"),
        ("rust-use", "super::helper"),
        ("rust-mod", "scoped"),
    ]
