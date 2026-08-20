from pathlib import Path

from pasm.core.validation import validate_spec_root


WORLD_TOML = """
[[deadline]]
id = "tether_slip"
due_secs = 40

[[deadline]]
id = "storm_front"
due_secs = 100

[[deadline]]
id = "surprise"
due_secs = 500
"""


def _workspace(tmp_path: Path, spec_text: str) -> Path:
    spec_root = tmp_path / "pasm" / "spec"
    spec_root.mkdir(parents=True)
    (spec_root / "design.yaml").write_text(spec_text, encoding="utf-8")
    world = tmp_path / "assets" / "worlds" / "mini.toml"
    world.parent.mkdir(parents=True)
    world.write_text(WORLD_TOML, encoding="utf-8")
    return spec_root


def test_pacing_structure_and_coverage(tmp_path: Path) -> None:
    spec_root = _workspace(
        tmp_path,
        """entities:
  - scenario_model: mini-scenario
    core: {status: accepted}
    game_design:
      world_file: assets/worlds/mini.toml
  - role: helm
    core: {status: accepted}
  - role: science
    core: {status: accepted}
  - verb: scan-the-band
    core: {status: accepted}
    game_design:
      owner_role: science
  - pacing: mission-clock
    core: {status: proposed}
    game_design:
      clock_source: mission first tick
      phases:
        - phase_id: act-one
          from: "0s"
          to: "95s"
          intensity_intent: setup
          engaged_roles: [helm]
          covers_deadlines: [tether_slip]
        - phase_id: act-two
          from: "95s"
          to: "5m"
          intensity_intent: peak
          engaged_roles: [helm]
  - gate: tether
    core: {status: accepted}
    game_design:
      self_resolving: true
      deadline_id: tether_slip
      benign: true
  - gate: storm
    core: {status: accepted}
    game_design:
      self_resolving: true
      deadline_id: storm_front
      benign: true
  - gate: surprise
    core: {status: accepted}
    game_design:
      requires_player_action: [scan-the-band]
      deadline_id: surprise
      benign: true
""",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    finding_ids = {f.id for f in result.findings}

    # 500s falls outside both windows (act-two ends at 5m = 300s).
    assert "pacing-deadline-uncovered:mission-clock:surprise" in finding_ids
    # 40s and 100s are covered exactly once: no findings for them.
    assert not any("tether_slip" in fid and fid.startswith("pacing-deadline") for fid in finding_ids)
    # science is engaged by no phase.
    idle = next(f for f in result.findings if f.id == "role-idle-in-pacing:science")
    assert idle.severity.value == "warning"


def test_pacing_window_errors(tmp_path: Path) -> None:
    spec_root = tmp_path / "pasm" / "spec"
    spec_root.mkdir(parents=True)
    (spec_root / "design.yaml").write_text(
        """entities:
  - pacing: broken-clock
    core: {status: proposed}
    game_design:
      phases:
        - phase_id: inverted
          from: "100s"
          to: "50s"
        - phase_id: garbled
          from: "soonish"
          to: "later"
        - phase_id: one
          from: "0s"
          to: "100s"
        - phase_id: overlapping
          from: "50s"
          to: "150s"
  - pacing: empty-budget
    core: {status: proposed}
    game_design:
      clock_source: mission first tick
""",
        encoding="utf-8",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    rules = {f.rule for f in result.findings}

    assert "game-design.pacing-window-ordered" in rules
    assert "game-design.pacing-window-parseable" in rules
    assert "game-design.pacing-phases-disjoint" in rules
    assert "game-design.pacing-phases-required" in rules


def test_design_principle_lifecycle(tmp_path: Path) -> None:
    spec_root = tmp_path / "pasm" / "spec"
    spec_root.mkdir(parents=True)
    (spec_root / "design.yaml").write_text(
        """entities:
  - design_principle: unmeasured-soft
    core: {status: accepted}
    game_design:
      strength: soft
      context: act three window
      expected_dynamic: crews debate the trade before the window opens
  - design_principle: unmeasured-hard
    core: {status: accepted}
    game_design:
      strength: hard
      expected_dynamic: the window is always short
  - design_principle: tentative-quiet
    core: {status: accepted}
    game_design:
      strength: soft
      maturity: tentative
      expected_dynamic: something plausible
  - design_principle: contested
    core: {status: accepted}
    game_design:
      strength: soft
      expected_dynamic: scarcity reads as a choice
      measured_by: [window-choice-claim]
      counter_evidence: [ai crews reach the window without understanding the claims]
  - design_principle: hollow
    core: {status: proposed}
    game_design:
      strength: mystic
  - playtest-claim: window-choice-claim
    core: {status: proposed}
    game_design:
      claim: crews state which two claims they will honour before the window closes
      supports: [contested]
""",
        encoding="utf-8",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    by_id = {f.id: f for f in result.findings}

    assert by_id["principle-unmeasured:unmeasured-soft"].severity.value == "warning"
    assert by_id["principle-unmeasured:unmeasured-hard"].severity.value == "error"
    assert by_id["principle-unmeasured:tentative-quiet"].severity.value == "information"
    contested = by_id["principle-counter-evidence:contested"]
    assert contested.severity.value == "concern"
    assert contested.requires_decision
    assert "principle-strength-invalid:hollow" in by_id
    assert "principle-missing-hypothesis:hollow" in by_id
