from pathlib import Path

from pasm.core.validation import validate_spec_root


WORLD_TOML = """
[[deadline]]
id = "tether_slip"
due_secs = 40

[[deadline]]
id = "storm_front"
due_secs = 100

[[capacity]]
id = "claim_committee"
amount = 30

[[capacity]]
id = "claim_havelock"
amount = 20

[[capacity]]
id = "claim_convoy"
amount = 16
"""

TEST_RS = """
#[test]
fn a_crew_who_did_everything_still_reach_the_window_short() {
}
"""


def _workspace(tmp_path: Path, spec_text: str) -> Path:
    spec_root = tmp_path / "pasm" / "spec"
    spec_root.mkdir(parents=True)
    (spec_root / "design.yaml").write_text(spec_text, encoding="utf-8")
    world = tmp_path / "assets" / "worlds" / "mini.toml"
    world.parent.mkdir(parents=True)
    world.write_text(WORLD_TOML, encoding="utf-8")
    tests_dir = tmp_path / "tests"
    tests_dir.mkdir()
    (tests_dir / "headless_runner.rs").write_text(TEST_RS, encoding="utf-8")
    return spec_root


def test_anchor_bounds_warn_and_expect_errors(tmp_path: Path) -> None:
    spec_root = _workspace(
        tmp_path,
        """entities:
  - tuning: tether-clock
    core: {status: accepted}
    game_design:
      affected_mechanics: [tether-clock]
      intended_directional_effect: longer gives the crew more reading time
      bounds: 60..180 seconds
      maturity: tentative
      anchors:
        - name: tether-due
          path: assets/worlds/mini.toml
          table: deadline
          match: "id=tether_slip"
          key: due_secs
          min: "60"
          max: "180"
  - gate: fixed-fact
    core: {status: accepted}
    game_design:
      self_resolving: true
      anchors:
        - name: storm-due
          path: assets/worlds/mini.toml
          table: deadline
          match: "id=storm_front"
          key: due_secs
          expect: "90"
""",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)

    bounds = next(f for f in result.findings if f.rule == "design-content.value-in-bounds")
    assert bounds.severity.value == "warning"
    assert "40" in bounds.summary

    expect = next(f for f in result.findings if f.rule == "design-content.value-expected")
    assert expect.severity.value == "error"


def test_anchor_aggregate_and_count(tmp_path: Path) -> None:
    spec_root = _workspace(
        tmp_path,
        """entities:
  - resource: window-passage
    core: {status: accepted}
    game_design:
      sources: [ladder-lift]
      sinks: [claims]
      anchors:
        - name: total-claims
          path: assets/worlds/mini.toml
          table: capacity
          key: amount
          aggregate: sum
          expect: "66"
          expect_count: "3"
        - name: wrong-count
          path: assets/worlds/mini.toml
          table: capacity
          key: amount
          expect_count: "4"
        - name: unresolvable
          path: assets/worlds/mini.toml
          table: capacity
          match: "id=claim_nobody"
          key: amount
""",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    rules = [f.rule for f in result.findings]

    # sum(30+20+16)=66 matches, count of 3 matches: total-claims is clean.
    assert not any(f.id.startswith("anchor-expect-mismatch:window-passage:total-claims") for f in result.findings)
    assert "design-content.count-matches" in rules
    assert "design-content.anchor-resolves" in rules


def test_invariant_triangle(tmp_path: Path) -> None:
    spec_root = _workspace(
        tmp_path,
        """entities:
  - design_invariant: window-structural-scarcity
    core: {status: accepted}
    game_design:
      relation: sum(capacity.amount) exceeds best possible lift, any two claims fit
      asserted_by: [a_crew_who_did_everything_still_reach_the_window_short, renamed_or_deleted_test]
      anchors:
        - name: total-claims
          path: assets/worlds/mini.toml
          table: capacity
          key: amount
          aggregate: sum
          expect: "66"
    implementation:
      status: declared
      paths: [tests/headless_runner.rs]
  - design_invariant: hollow-invariant
    core: {status: accepted}
    game_design:
      magnitude_source: unused
""",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    finding_ids = {f.id for f in result.findings}

    assert "invariant-assertion-missing:window-structural-scarcity:renamed_or_deleted_test" in finding_ids
    assert not any(
        f.id == "invariant-assertion-missing:window-structural-scarcity:a_crew_who_did_everything_still_reach_the_window_short"
        for f in result.findings
    )
    assert "invariant-missing-relation:hollow-invariant" in finding_ids
    assert "invariant-missing-anchors:hollow-invariant" in finding_ids
    assert "invariant-missing-asserted-by:hollow-invariant" in finding_ids


def test_anchor_stale_paths_and_selectors(tmp_path: Path) -> None:
    spec_root = _workspace(
        tmp_path,
        """entities:
  - gate: stale
    core: {status: accepted}
    game_design:
      self_resolving: true
      anchors:
        - name: gone-file
          path: assets/worlds/deleted.toml
          table: deadline
        - name: incomplete
          key: due_secs
        - name: bad-expect
          path: assets/worlds/mini.toml
          table: deadline
          match: "id=tether_slip"
          key: due_secs
          expect: "not-a-number"
""",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    rules = {f.rule for f in result.findings}

    assert "design-content.anchor-file-exists" in rules
    assert "design-content.anchor-complete" in rules
    assert "design-content.expectation-parseable" in rules


def test_deadline_order_follows_requires(tmp_path: Path) -> None:
    spec_root = _workspace(
        tmp_path,
        """entities:
  - scenario_model: mini-scenario
    core: {status: accepted}
    game_design:
      world_file: assets/worlds/mini.toml
  - gate: early
    core: {status: accepted}
    game_design:
      self_resolving: true
      deadline_id: storm_front
      benign: true
      requires: [late]
  - gate: late
    core: {status: accepted}
    game_design:
      self_resolving: true
      deadline_id: tether_slip
      benign: true
""",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)

    # 'early' (100s) requires 'late' (40s): the prerequisite's clock lands
    # first, so the declared order and the authored order agree.
    assert not any(f.rule == "design-content.deadline-order" for f in result.findings)


def test_deadline_order_violation_is_an_error(tmp_path: Path) -> None:
    spec_root = _workspace(
        tmp_path,
        """entities:
  - scenario_model: mini-scenario
    core: {status: accepted}
    game_design:
      world_file: assets/worlds/mini.toml
  - gate: first
    core: {status: accepted}
    game_design:
      self_resolving: true
      deadline_id: tether_slip
      benign: true
      requires: [second]
  - gate: second
    core: {status: accepted}
    game_design:
      self_resolving: true
      deadline_id: storm_front
      benign: true
""",
    )

    result = validate_spec_root(spec_root, workspace_root=tmp_path)
    finding = next(f for f in result.findings if f.rule == "design-content.deadline-order")

    # 'first' at 40s requires 'second' at 100s: the prerequisite lands later.
    assert finding.severity.value == "error"
    assert "first" in finding.id
