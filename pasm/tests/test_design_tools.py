import hashlib
import json
from pathlib import Path

from pasm.core.validation import validate_spec_root
from pasm.domains.game_design.bootstrap import bootstrap_scenario
from pasm.domains.game_design.digest import build_design_digest, render_design_digest
from pasm.domains.game_design.writeback import WritebackError, apply_writeback

import pytest


WORLD_TOML = """# The mini world. This header comment must survive write-back untouched.
# Second commentary line with design rationale.

[global]
name = "mini"

# Deadlines are measured from first tick.
[[deadline]]
id = "tether_slip"
due_secs = 40  # short on purpose
visible = true

[[deadline]]
id = "storm_front"
due_secs = 100

[script]
setup = '''
fn on_tether_slip(ctx) {
}
'''
"""

SPEC_YAML = """entities:
  - scenario_model: mini-scenario
    core: {status: accepted}
    game_design:
      world_file: assets/worlds/mini.toml
  - gate: tether
    core: {status: accepted}
    game_design:
      self_resolving: true
      deadline_id: tether_slip
      on_failure: [tether-loss]
      anchors:
        - name: tether-due
          path: assets/worlds/mini.toml
          table: deadline
          match: "id=tether_slip"
          key: due_secs
          min: "30"
          max: "180"
  - gate: storm
    core: {status: accepted}
    game_design:
      self_resolving: true
      deadline_id: storm_front
      benign: true
  - consequence: tether-loss
    core: {status: accepted}
    game_design:
      handler: on_tether_slip
      magnitude_source: read from tether stress at slip time
"""


def _workspace(tmp_path: Path) -> Path:
    spec_root = tmp_path / "pasm" / "spec"
    spec_root.mkdir(parents=True)
    (spec_root / "design.yaml").write_text(SPEC_YAML, encoding="utf-8")
    world = tmp_path / "assets" / "worlds" / "mini.toml"
    world.parent.mkdir(parents=True)
    world.write_text(WORLD_TOML, encoding="utf-8")
    return spec_root


def _changes(tmp_path: Path, *ops) -> dict:
    world = (tmp_path / "assets" / "worlds" / "mini.toml").read_bytes()
    return {
        "file": "assets/worlds/mini.toml",
        "expected_sha256": hashlib.sha256(world).hexdigest(),
        "changes": list(ops),
    }


def test_bootstrap_drafts_a_validating_skeleton(tmp_path: Path) -> None:
    spec_root = _workspace(tmp_path)
    drafted = bootstrap_scenario("assets/worlds/mini.toml", tmp_path, scenario_id="draft-scenario")
    (spec_root / "draft.yaml").write_text(drafted, encoding="utf-8")
    (spec_root / "design.yaml").unlink()

    result = validate_spec_root(spec_root, workspace_root=tmp_path)

    assert result.ok, [f.id for f in result.findings if f.severity.value == "error"]
    # Draft gates carry the incompleteness as warnings, not silence.
    warning_ids = {f.id for f in result.findings if f.severity.value == "warning"}
    assert "deadline-consequence-missing:gate-tether-slip" in warning_ids
    assert "gate-zero-input:gate-storm-front" in warning_ids
    assert "on_tether_slip" in drafted


def test_digest_reads_live_values_and_hashes(tmp_path: Path) -> None:
    spec_root = _workspace(tmp_path)
    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    digest = build_design_digest(result.model.entities, tmp_path)

    assert digest["files"][0]["sha256"]
    tether = next(gate for gate in digest["gates"] if gate["id"] == "tether")
    assert tether["authored"]["due_secs"] == 40
    anchor = next(item for item in digest["anchors"] if item["name"] == "tether-due")
    assert anchor["live"] == [40]

    text = render_design_digest(digest)
    assert "tether-loss" in text
    assert "magnitude: read from tether stress" in text


def test_writeback_value_preserves_comments(tmp_path: Path) -> None:
    spec_root = _workspace(tmp_path)
    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    world_path = tmp_path / "assets" / "worlds" / "mini.toml"
    before = world_path.read_text(encoding="utf-8")

    applied = apply_writeback(
        _changes(tmp_path, {"op": "set_value", "table": "deadline", "match": "id=tether_slip", "key": "due_secs", "value": "60"}),
        tmp_path,
        entities=result.model.entities,
    )

    after = world_path.read_text(encoding="utf-8")
    assert len(applied) == 1
    assert "due_secs = 60  # short on purpose" in after
    # Every byte outside the single changed literal is identical.
    assert after.replace("due_secs = 60  # short on purpose", "due_secs = 40  # short on purpose") == before


def test_writeback_refuses_out_of_bounds_and_stale_hash(tmp_path: Path) -> None:
    spec_root = _workspace(tmp_path)
    result = validate_spec_root(spec_root, workspace_root=tmp_path)

    with pytest.raises(WritebackError, match="above design max"):
        apply_writeback(
            _changes(tmp_path, {"op": "set_value", "table": "deadline", "match": "id=tether_slip", "key": "due_secs", "value": "500"}),
            tmp_path,
            entities=result.model.entities,
        )

    stale = _changes(tmp_path, {"op": "set_value", "table": "deadline", "match": "id=tether_slip", "key": "due_secs", "value": "60"})
    stale["expected_sha256"] = "0" * 64
    with pytest.raises(WritebackError, match="hash mismatch"):
        apply_writeback(stale, tmp_path, entities=result.model.entities)


def test_writeback_structural_ops(tmp_path: Path) -> None:
    spec_root = _workspace(tmp_path)
    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    world_path = tmp_path / "assets" / "worlds" / "mini.toml"

    apply_writeback(
        _changes(
            tmp_path,
            {"op": "insert_row", "table": "deadline", "values": {"id": "window_closes", "due_secs": 470, "visible": True}},
            {"op": "append_handler", "name": "on_window_closes", "comment": "The window closes; unresolved claims lapse."},
        ),
        tmp_path,
        entities=result.model.entities,
    )

    after = world_path.read_text(encoding="utf-8")
    assert 'id = "window_closes"' in after
    assert "due_secs = 470" in after
    assert "fn on_window_closes(ctx) {" in after
    assert "// The window closes; unresolved claims lapse." in after
    # The inserted row lands with the other deadlines, before [script].
    assert after.index('id = "window_closes"') < after.index("[script]")

    # Duplicate handler append is refused: write-back never edits existing fns.
    with pytest.raises(WritebackError, match="already exists"):
        apply_writeback(
            _changes(tmp_path, {"op": "append_handler", "name": "on_tether_slip"}),
            tmp_path,
            entities=result.model.entities,
        )


def test_writeback_remove_row_and_dry_run(tmp_path: Path) -> None:
    spec_root = _workspace(tmp_path)
    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    world_path = tmp_path / "assets" / "worlds" / "mini.toml"
    before = world_path.read_text(encoding="utf-8")

    applied = apply_writeback(
        _changes(tmp_path, {"op": "remove_row", "table": "deadline", "match": "id=storm_front"}),
        tmp_path,
        entities=result.model.entities,
        dry_run=True,
    )
    assert applied and world_path.read_text(encoding="utf-8") == before

    apply_writeback(
        _changes(tmp_path, {"op": "remove_row", "table": "deadline", "match": "id=storm_front"}),
        tmp_path,
        entities=result.model.entities,
    )
    after = world_path.read_text(encoding="utf-8")
    assert "storm_front" not in after
    assert 'id = "tether_slip"' in after
