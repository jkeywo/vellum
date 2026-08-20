import hashlib
from pathlib import Path

from pasm.core.validation import validate_spec_root
from pasm.domains.game_design.writeback import apply_writeback
from pasm.scanners.content_ron import extract_ron_document


SIM_RON = """// Simulation rules - the numbers that apply to every ship at once.
(
    // Fraction of damage a braced ship still takes.
    brace_damage_factor: 0.35,

    cannonball_lifetime: 2.5, // seconds
    torpedo_locks_max: 10,
    friendly_fire: false,

    waves: [
        (name: "First Blood", strength: 1),
        (name: "The Squall", strength: 2),
        (name: "Executioner", strength: 4),
    ],

    drift: (
        damping: 0.8,
        cap: Some(3.0),
    ),
)
"""


def test_ron_extraction_resolves_scalars_rows_and_nested(tmp_path: Path) -> None:
    document = extract_ron_document(SIM_RON)

    assert document.resolve("", None, "brace_damage_factor")[0].value == 0.35
    assert document.resolve("", None, "torpedo_locks_max")[0].value == 10
    assert document.resolve("", None, "friendly_fire")[0].value is False
    assert document.resolve("drift", None, "cap")[0].value == 3.0
    waves = document.resolve("waves", None, "strength")
    assert [item.value for item in waves] == [1, 2, 4]
    squall = document.resolve("waves", "name=The Squall", "strength")
    assert [item.value for item in squall] == [2]


def test_ron_anchors_validate_and_writeback(tmp_path: Path) -> None:
    spec_root = tmp_path / "pasm" / "spec"
    spec_root.mkdir(parents=True)
    ron_path = tmp_path / "assets" / "sim.tuning.ron"
    ron_path.parent.mkdir(parents=True)
    ron_path.write_text(SIM_RON, encoding="utf-8")
    (spec_root / "design.yaml").write_text(
        """entities:
  - tuning: brace-tuning
    core: {status: accepted}
    game_design:
      affected_mechanics: [brace-tuning]
      intended_directional_effect: lower means bracing matters more
      bounds: 0.2..0.6
      maturity: established
      anchors:
        - name: brace-factor
          path: assets/sim.tuning.ron
          table: ""
          key: brace_damage_factor
          min: "0.2"
          max: "0.6"
        - name: wave-count
          path: assets/sim.tuning.ron
          table: waves
          key: strength
          expect_count: "3"
""",
        encoding="utf-8",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    assert not any(f.rule.startswith("design-content.") for f in result.findings), [
        f.id for f in result.findings
    ]

    changes = {
        "file": "assets/sim.tuning.ron",
        "expected_sha256": hashlib.sha256(ron_path.read_bytes()).hexdigest(),
        "changes": [
            {"op": "set_value", "table": "", "key": "brace_damage_factor", "value": "0.5"},
        ],
    }
    apply_writeback(changes, tmp_path, entities=result.model.entities)

    after = ron_path.read_text(encoding="utf-8")
    assert "brace_damage_factor: 0.5," in after
    # Comments and the rest of the file survive.
    assert "// Fraction of damage a braced ship still takes." in after
    assert "cannonball_lifetime: 2.5, // seconds" in after
