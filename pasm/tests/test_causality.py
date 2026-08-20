from pathlib import Path

from pasm.core.validation import validate_spec_root


def _write_spec(spec_root: Path, text: str) -> None:
    spec_root.mkdir(parents=True, exist_ok=True)
    (spec_root / "design.yaml").write_text(text, encoding="utf-8")


def test_causal_structure_findings(tmp_path: Path) -> None:
    _write_spec(
        tmp_path,
        """entities:
  - gate: silent-act
    core: {status: accepted}
    game_design:
      enables: [locked-act]
  - gate: locked-act
    core: {status: accepted}
    game_design:
      requires: [silent-act]
      requires_player_action: [advance]
      deadline_id: storm_front
  - verb: advance
    core: {status: accepted}
    game_design:
      owner_role: captain
  - role: captain
    core: {status: accepted}
  - consequence: unloved-outcome
    core: {status: accepted}
    game_design:
      magnitude_source: never produced
  - storm-band: odd-specialiser
    core: {status: accepted}
    game_design:
      specialises: nonsense
""",
    )

    result = validate_spec_root(tmp_path)
    finding_ids = {finding.id for finding in result.findings}

    assert "gate-zero-input:silent-act" in finding_ids
    assert "deadline-consequence-missing:locked-act" in finding_ids
    assert "consequence-orphaned:unloved-outcome" in finding_ids
    assert "specialises-unknown-kernel-kind:odd-specialiser:nonsense" in finding_ids


def test_gate_cycle_is_detected(tmp_path: Path) -> None:
    _write_spec(
        tmp_path,
        """entities:
  - gate: first
    core: {status: accepted}
    game_design:
      requires: [second]
      self_resolving: true
  - gate: second
    core: {status: accepted}
    game_design:
      requires: [first]
      self_resolving: true
""",
    )

    result = validate_spec_root(tmp_path)

    assert any(finding.rule == "game-design.gate-cycle-free" for finding in result.findings)


def test_consequence_state_visibility_warns(tmp_path: Path) -> None:
    _write_spec(
        tmp_path,
        """entities:
  - role: captain
    core: {status: accepted}
  - role: control
    core: {status: accepted}
  - verb: order-the-lift
    core: {status: accepted}
    game_design:
      owner_role: captain
      on_success: [worker-casualties]
  - consequence: worker-casualties
    core: {status: accepted}
    game_design:
      magnitude_source: workers' disposition at the moment the order is given
      depends_on_state: [worker-disposition]
  - information: worker-disposition
    core: {status: accepted}
    game_design:
      visibility: role-visible
      permitted_viewers: [control]
""",
    )

    result = validate_spec_root(tmp_path)
    finding = next(
        item for item in result.findings
        if item.rule == "game-design.consequence-state-visible"
    )

    assert finding.severity.value == "warning"
    assert "captain" in finding.summary


def test_specialised_kind_gets_kernel_validators(tmp_path: Path) -> None:
    _write_spec(
        tmp_path,
        """entities:
  - storm-band: band-one
    core: {status: accepted}
    game_design:
      specialises: gate
""",
    )

    result = validate_spec_root(tmp_path)

    assert any(finding.id == "gate-zero-input:band-one" for finding in result.findings)


WORLD_TOML = '''
[global]
name = "mini"

[[deadline]]
id = "tether_slip"
due_secs = 40

[[deadline]]
id = "storm_front"
due_secs = 100

[script]
setup = """
fn on_tether_slip(ctx) {
}
"""
'''


def _write_workspace(tmp_path: Path, spec_text: str) -> Path:
    spec_root = tmp_path / "pasm" / "spec"
    _write_spec(spec_root, spec_text)
    world = tmp_path / "assets" / "worlds" / "mini.toml"
    world.parent.mkdir(parents=True, exist_ok=True)
    world.write_text(WORLD_TOML, encoding="utf-8")
    return spec_root


def test_closed_world_deadline_and_handler_checks(tmp_path: Path) -> None:
    spec_root = _write_workspace(
        tmp_path,
        """entities:
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
  - gate: ghost
    core: {status: accepted}
    game_design:
      self_resolving: true
      deadline_id: never_authored
      benign: true
  - consequence: tether-loss
    core: {status: accepted}
    game_design:
      handler: on_tether_slip
  - consequence: phantom-outcome
    core: {status: accepted}
    game_design:
      handler: on_missing_handler
""",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    finding_ids = {finding.id for finding in result.findings}

    # storm_front is authored but claimed by no gate.
    assert "deadline-unmapped:mini-scenario:storm_front" in finding_ids
    # The ghost gate claims a deadline the world never authors.
    assert "deadline-unknown:ghost:never_authored" in finding_ids
    # tether-loss's handler exists; phantom-outcome's does not.
    assert "consequence-unimplemented:phantom-outcome:on_missing_handler" in finding_ids
    assert not any(finding.id.startswith("consequence-unimplemented:tether-loss") for finding in result.findings)


def test_complete_scenario_model_is_clean(tmp_path: Path) -> None:
    spec_root = _write_workspace(
        tmp_path,
        """entities:
  - scenario_model: mini-scenario
    core: {status: accepted}
    game_design:
      world_file: assets/worlds/mini.toml
  - role: captain
    core: {status: accepted}
  - verb: hold-the-tether
    core: {status: accepted}
    game_design:
      owner_role: captain
  - gate: tether
    core: {status: accepted}
    game_design:
      requires_player_action: [hold-the-tether]
      deadline_id: tether_slip
      on_failure: [tether-loss]
  - gate: storm
    core: {status: accepted}
    game_design:
      requires: [tether]
      self_resolving: true
      deadline_id: storm_front
      benign: true
  - consequence: tether-loss
    core: {status: accepted}
    game_design:
      handler: on_tether_slip
      magnitude_source: read from tether stress at slip time
""",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)

    design_findings = [
        finding for finding in result.findings
        if finding.rule.startswith(("game-design.", "design-content."))
    ]
    assert design_findings == []


def test_no_scenario_model_means_no_content_checks(tmp_path: Path) -> None:
    _write_spec(
        tmp_path,
        """entities:
  - gate: unanchored
    core: {status: accepted}
    game_design:
      self_resolving: true
      deadline_id: something
      benign: true
""",
    )

    result = validate_spec_root(tmp_path)

    assert not any(finding.rule.startswith("design-content.") for finding in result.findings)
