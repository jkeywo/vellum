"""Closed-world checks binding the design spec to authored game content.

The authored world file is authoritative: the spec never duplicates its
numbers, it claims them. A `scenario_model` entity opts a world file into
closed-world completeness — every authored deadline must be claimed by a
gate (with a failure consequence or an explicit `benign: true`), and every
declared consequence handler must exist in the world's script.
"""

from __future__ import annotations

from pathlib import Path

from pasm.core.findings import Finding, FindingCategory, Severity
from pasm.core.model import SpecEntity
from pasm.domains.game_design.causality import effective_kind
from pasm.domains.game_design.pacing import parse_duration_seconds
from pasm.implementation.observation import observe_entity_implementation
from pasm.scanners.content import ContentDocument, ContentParseError, extract_content


DEFAULT_DEADLINE_TABLE = "deadline"
DEFAULT_DEADLINE_ID_KEY = "id"
DEFAULT_DEADLINE_DUE_KEY = "due_secs"
AGGREGATES = {"sum", "min", "max"}


def validate_design_content(entities: tuple[SpecEntity, ...], workspace_root: Path) -> list[Finding]:
    findings: list[Finding] = []
    documents: dict[str, ContentDocument] = {}
    findings.extend(_validate_anchors(entities, workspace_root, documents))
    findings.extend(_validate_invariant_assertions(entities, workspace_root))
    findings.extend(_validate_closed_world(entities, workspace_root, documents))
    return findings


def _validate_anchors(entities, workspace_root: Path, documents: dict[str, ContentDocument]) -> list[Finding]:
    findings: list[Finding] = []
    for entity in entities:
        design = entity.game_design
        if design is None:
            continue
        for index, anchor in enumerate(design.anchors):
            label = anchor.name or f"#{index + 1}"
            location = anchor.source_location or entity.source_location
            if anchor.path is None or anchor.table is None:
                findings.append(_finding(
                    f"anchor-incomplete:{entity.id}:{label}", entity,
                    f"Anchor '{label}' on '{entity.id}' is missing 'path' or 'table'.",
                    "A content anchor names the authored file and the table its value lives in.",
                    "design-content.anchor-complete",
                    "Add the missing 'path'/'table' fields.", location,
                ))
                continue
            document = _load_document(anchor.path, workspace_root, documents)
            if isinstance(document, str):
                findings.append(_finding(
                    f"anchor-file-missing:{entity.id}:{label}", entity,
                    f"Anchor '{label}' on '{entity.id}' points at unreadable content file '{anchor.path}'.",
                    document,
                    "design-content.anchor-file-exists",
                    "Fix the anchor path or restore the authored file.", location,
                    category=FindingCategory.STALE_SPECIFICATION,
                ))
                continue
            resolved = document.resolve(anchor.table, anchor.match, anchor.key)
            if not resolved:
                findings.append(_finding(
                    f"anchor-unresolved:{entity.id}:{label}", entity,
                    f"Anchor '{label}' on '{entity.id}' resolves to nothing in '{anchor.path}'.",
                    f"No row in table '{anchor.table}' matches "
                    f"'{anchor.match or '<any>'}' with key '{anchor.key or '<row>'}'.",
                    "design-content.anchor-resolves",
                    "Fix the anchor selector or update the spec after the content change.", location,
                    category=FindingCategory.STALE_SPECIFICATION,
                ))
                continue
            findings.extend(_check_expectations(entity, anchor, label, resolved, location))
    return findings


def _load_document(path_value: str, workspace_root: Path, documents: dict[str, ContentDocument]):
    """Return a cached ContentDocument, or an error string when unreadable."""
    cached = documents.get(path_value)
    if cached is not None:
        return cached
    content_path = (workspace_root / path_value).resolve()
    if not content_path.is_file():
        return f"'{path_value}' does not exist under the workspace root."
    try:
        document = extract_content(content_path.read_text(encoding="utf-8"), content_path.suffix)
    except (OSError, ContentParseError) as exc:
        return str(exc)
    documents[path_value] = document
    return document


def _check_expectations(entity, anchor, label, resolved, location) -> list[Finding]:
    findings: list[Finding] = []

    def parse(field_name: str, raw: str) -> float | None:
        try:
            return float(raw)
        except ValueError:
            findings.append(_finding(
                f"anchor-expectation-unparseable:{entity.id}:{label}:{field_name}", entity,
                f"Anchor '{label}' on '{entity.id}' has non-numeric '{field_name}' value '{raw}'.",
                "Quoted expectation values must parse as numbers at check time.",
                "design-content.expectation-parseable",
                f"Fix the '{field_name}' value.", location,
            ))
            return None

    if anchor.expect_count is not None:
        expected = parse("expect_count", anchor.expect_count)
        if expected is not None and len(resolved) != int(expected):
            findings.append(_finding(
                f"anchor-count-mismatch:{entity.id}:{label}", entity,
                f"Anchor '{label}' on '{entity.id}' expects {int(expected)} rows but resolved {len(resolved)}.",
                "The authored content no longer matches the spec's structural count claim.",
                "design-content.count-matches",
                "Update the spec or the authored content so the count claim holds.", location,
                category=FindingCategory.STALE_SPECIFICATION,
            ))

    numeric = [item.value for item in resolved if isinstance(item.value, (int, float)) and not isinstance(item.value, bool)]
    values: list[float]
    if anchor.aggregate is not None:
        if anchor.aggregate not in AGGREGATES:
            findings.append(_finding(
                f"anchor-aggregate-unknown:{entity.id}:{label}", entity,
                f"Anchor '{label}' on '{entity.id}' uses unknown aggregate '{anchor.aggregate}'.",
                "Aggregate must be one of sum, min, or max.",
                "design-content.aggregate-valid",
                "Use a supported aggregate.", location,
            ))
            return findings
        if not numeric:
            return findings
        folded = {"sum": sum(numeric), "min": min(numeric), "max": max(numeric)}[anchor.aggregate]
        values = [folded]
    else:
        values = [float(value) for value in numeric]

    if anchor.expect is not None:
        expected = parse("expect", anchor.expect)
        if expected is not None:
            actual = values if values else [str(item.value) for item in resolved]
            mismatched = (
                any(value != expected for value in values)
                if values
                else all(str(item.value) != anchor.expect for item in resolved)
            )
            if mismatched:
                findings.append(_finding(
                    f"anchor-expect-mismatch:{entity.id}:{label}", entity,
                    f"Anchor '{label}' on '{entity.id}' expects {anchor.expect} but the authored value is {actual}.",
                    "An 'expect' anchor is a structural claim; its drift means the design "
                    "model and the authored content now disagree about a fact.",
                    "design-content.value-expected",
                    "Update the spec's claim or the authored content — they must agree.", location,
                    category=FindingCategory.STALE_SPECIFICATION,
                ))

    for bound_name, breaks_bound in (("min", lambda v, b: v < b), ("max", lambda v, b: v > b)):
        raw = getattr(anchor, bound_name)
        if raw is None:
            continue
        bound = parse(bound_name, raw)
        if bound is None:
            continue
        outside = [value for value in values if breaks_bound(value, bound)]
        if outside:
            findings.append(_finding(
                f"anchor-outside-bounds:{entity.id}:{label}:{bound_name}", entity,
                f"Anchor '{label}' on '{entity.id}': authored value {outside[0]} is outside design {bound_name} {raw}.",
                "Bounds are design intent, not a gate on the human designer: the authored "
                "content is authoritative, and this warning is the start of a conversation "
                "about whether the intent or the tuning should change.",
                "design-content.value-in-bounds",
                "Retune the content within bounds, or update the declared design intent.", location,
                severity=Severity.WARNING,
                category=FindingCategory.DESIGN_RISK,
            ))
    return findings


def _validate_invariant_assertions(entities, workspace_root: Path) -> list[Finding]:
    findings: list[Finding] = []
    for entity in entities:
        design = entity.game_design
        if design is None or effective_kind(entity) != "design_invariant":
            continue
        if not design.asserted_by:
            continue
        if entity.implementation is None or not entity.implementation.paths:
            findings.append(_finding(
                f"invariant-assertion-unlocated:{entity.id}", entity,
                f"Design invariant '{entity.id}' names asserting tests but declares no implementation paths.",
                "The asserting test symbols can only be verified inside declared files.",
                "design-content.invariant-assertion-located",
                "Add 'implementation.paths' pointing at the test file(s).", entity.source_location,
            ))
            continue
        observed = observe_entity_implementation(entity, workspace_root)
        for test_name in design.asserted_by:
            if observed.contains_text(test_name):
                continue
            findings.append(_finding(
                f"invariant-assertion-missing:{entity.id}:{test_name}", entity,
                f"Design invariant '{entity.id}' claims test '{test_name}' asserts it, but no declared file contains it.",
                "A renamed or deleted asserting test silently kills the design claim; "
                "this finding is where it dies loudly instead.",
                "design-content.invariant-asserted",
                "Update 'asserted_by' to the current test name, or restore the test.",
                _location(entity, "asserted_by"),
                category=FindingCategory.STALE_SPECIFICATION,
            ))
    return findings


def _validate_closed_world(entities, workspace_root: Path, documents: dict[str, ContentDocument]) -> list[Finding]:
    scenario_models = [
        entity for entity in entities
        if effective_kind(entity) == "scenario_model" and entity.game_design is not None
    ]
    if not scenario_models:
        return []

    findings: list[Finding] = []
    known_deadline_ids: set[str] = set()
    known_deadline_dues: dict[str, float] = {}
    script_functions: set[str] = set()

    pacings: list[tuple[SpecEntity, list[tuple[float, float, object]]]] = []
    for entity in entities:
        if effective_kind(entity) != "pacing" or entity.game_design is None:
            continue
        windows = []
        for phase in entity.game_design.phases:
            start = parse_duration_seconds(phase.start)
            end = parse_duration_seconds(phase.end)
            if start is not None and end is not None and end > start:
                windows.append((start, end, phase))
        if windows:
            pacings.append((entity, windows))

    gates = [
        entity for entity in entities
        if effective_kind(entity) == "gate"
        and entity.game_design is not None
        and entity.game_design.deadline_id is not None
    ]
    claims: dict[str, list[SpecEntity]] = {}
    for gate in gates:
        claims.setdefault(gate.game_design.deadline_id, []).append(gate)

    for model in scenario_models:
        design = model.game_design
        if design.world_file is None:
            findings.append(_finding(
                f"scenario-model-missing-world-file:{model.id}", model,
                f"Scenario model '{model.id}' declares no world file.",
                "A scenario model anchors closed-world checks to one authored content file.",
                "design-content.scenario-model-world-file-required",
                "Add 'game_design.world_file' with the workspace-relative content path.",
                _location(model, "world_file"),
            ))
            continue
        world_path = (workspace_root / design.world_file).resolve()
        if not world_path.is_file():
            findings.append(_finding(
                f"world-file-missing:{model.id}", model,
                f"Scenario model '{model.id}' world file '{design.world_file}' does not exist.",
                "The design model claims an authored content file the repository does not contain.",
                "design-content.world-file-exists",
                "Fix 'world_file' or restore the authored content file.",
                _location(model, "world_file"),
                category=FindingCategory.STALE_SPECIFICATION,
            ))
            continue
        document = documents.get(design.world_file)
        if document is None:
            try:
                document = extract_content(world_path.read_text(encoding="utf-8"), world_path.suffix)
            except (OSError, ContentParseError) as exc:
                findings.append(_finding(
                    f"world-file-unreadable:{model.id}", model,
                    f"Scenario model '{model.id}' world file '{design.world_file}' could not be parsed.",
                    str(exc),
                    "design-content.world-file-parses",
                    "Fix the authored content file; the design model reads it as truth.",
                    _location(model, "world_file"),
                ))
                continue
            documents[design.world_file] = document

        script_functions.update(name for name, _ in document.script_functions)

        deadline_table = design.deadline_table or DEFAULT_DEADLINE_TABLE
        deadline_id_key = design.deadline_id_key or DEFAULT_DEADLINE_ID_KEY
        deadline_due_key = design.deadline_due_key or DEFAULT_DEADLINE_DUE_KEY
        for row in document.rows(deadline_table):
            deadline_id = row.values.get(deadline_id_key)
            if not isinstance(deadline_id, str):
                continue
            known_deadline_ids.add(deadline_id)
            due_value = row.values.get(deadline_due_key)
            if isinstance(due_value, (int, float)) and not isinstance(due_value, bool):
                known_deadline_dues[deadline_id] = float(due_value)
            if isinstance(due_value, (int, float)) and not isinstance(due_value, bool):
                for pacing_entity, windows in pacings:
                    containing = [
                        window for window in windows
                        if window[0] <= float(due_value) < window[1]
                    ]
                    if len(containing) != 1:
                        problem = "no phase window" if not containing else f"{len(containing)} phase windows"
                        findings.append(_finding(
                            f"pacing-deadline-uncovered:{pacing_entity.id}:{deadline_id}", pacing_entity,
                            f"Authored deadline '{deadline_id}' (at {due_value}) falls in {problem} of pacing '{pacing_entity.id}'.",
                            "Every authored deadline must land in exactly one declared phase window; "
                            "an unplanned deadline is pacing the budget never accounted for.",
                            "design-content.pacing-covers-authored-deadlines",
                            "Extend or split the phase windows so this deadline is covered exactly once.",
                            pacing_entity.source_location,
                        ))
                    else:
                        phase = containing[0][2]
                        declared_elsewhere = any(
                            deadline_id in other.covers_deadlines
                            for _, other_windows in pacings
                            for _, _, other in other_windows
                            if other is not phase
                        )
                        if (phase.covers_deadlines and deadline_id not in phase.covers_deadlines) or declared_elsewhere:
                            findings.append(_finding(
                                f"pacing-coverage-mismatch:{pacing_entity.id}:{deadline_id}", pacing_entity,
                                f"Deadline '{deadline_id}' lands in phase '{phase.phase_id}' but the declared coverage disagrees.",
                                "The 'covers_deadlines' declaration and the authored timing tell different stories.",
                                "design-content.pacing-coverage-declared",
                                "Fix 'covers_deadlines' or retune the deadline into its intended phase.",
                                pacing_entity.source_location,
                                severity=Severity.WARNING,
                                category=FindingCategory.DESIGN_RISK,
                            ))
            claimers = claims.get(deadline_id, [])
            if not claimers:
                findings.append(_finding(
                    f"deadline-unmapped:{model.id}:{deadline_id}", model,
                    f"Authored deadline '{deadline_id}' in '{design.world_file}' is claimed by no gate.",
                    "Every authored deadline must map to a gate declaring its failure consequence "
                    f"(or 'benign: true'). Authored at line {row.line} of the world file.",
                    "design-content.deadline-mapped",
                    "Add a gate with 'deadline_id' for this deadline, stating 'on_failure' or 'benign: true'.",
                    _location(model, "world_file"),
                ))
            elif len(claimers) > 1:
                names = ", ".join(sorted(claimer.id.value for claimer in claimers))
                findings.append(_finding(
                    f"deadline-claimed-twice:{deadline_id}", claimers[0],
                    f"Authored deadline '{deadline_id}' is claimed by multiple gates ({names}).",
                    "Two gates claiming one authored deadline usually means a copy-paste error.",
                    "design-content.deadline-claimed-once",
                    "Keep one claiming gate per authored deadline.",
                    _location(claimers[0], "deadline_id"),
                    severity=Severity.WARNING,
                    category=FindingCategory.DESIGN_RISK,
                ))

    # Order-only clock discipline: when gate A requires gate B and both carry
    # authored deadlines, A's clock must land after B's. This is the check
    # that survives retuning — values move freely, order does not.
    gate_index = {gate.id: gate for gate in gates}
    for gate in gates:
        own_due = known_deadline_dues.get(gate.game_design.deadline_id)
        if own_due is None:
            continue
        for required in gate.game_design.requires:
            prerequisite = gate_index.get(required)
            if prerequisite is None:
                continue
            required_due = known_deadline_dues.get(prerequisite.game_design.deadline_id)
            if required_due is not None and own_due <= required_due:
                findings.append(_finding(
                    f"deadline-order:{gate.id}:{required}", gate,
                    f"Gate '{gate.id}' (at {own_due:g}) requires '{required}' whose deadline lands later ({required_due:g}).",
                    "The causal order the design declares and the authored clock disagree: "
                    "a prerequisite's deadline must land before the gate that requires it.",
                    "design-content.deadline-order",
                    "Retune the authored clocks or fix the gate's 'requires'.",
                    _location(gate, "requires"),
                ))

    for gate in gates:
        if gate.game_design.deadline_id not in known_deadline_ids:
            findings.append(_finding(
                f"deadline-unknown:{gate.id}:{gate.game_design.deadline_id}", gate,
                f"Gate '{gate.id}' claims deadline '{gate.game_design.deadline_id}' that no anchored world file authors.",
                "The gate names a deadline id absent from every scenario model's world file — "
                "the authored content moved on and the spec did not.",
                "design-content.deadline-exists",
                "Fix 'deadline_id' or update the world file.",
                _location(gate, "deadline_id"),
                category=FindingCategory.STALE_SPECIFICATION,
            ))

    if documents:
        for entity in entities:
            if effective_kind(entity) != "consequence" or entity.game_design is None:
                continue
            handler = entity.game_design.handler
            if handler is not None and handler not in script_functions:
                findings.append(_finding(
                    f"consequence-unimplemented:{entity.id}:{handler}", entity,
                    f"Consequence '{entity.id}' names handler '{handler}' that no anchored world script defines.",
                    "The consequence claims an implementing script function absent from every "
                    "scenario model's world file.",
                    "design-content.consequence-handler-exists",
                    "Fix 'handler' or implement the script function.",
                    _location(entity, "handler"),
                    category=FindingCategory.UNIMPLEMENTED_SPECIFICATION,
                ))

    return findings


def _location(entity, *field_names):
    design = entity.game_design
    if design is not None:
        for field_name in field_names:
            location = design.field_locations.get(field_name)
            if location is not None:
                return location
    return entity.source_location


def _finding(identifier, entity, summary, details, rule, resolution, location,
             severity=Severity.ERROR, category=FindingCategory.VIOLATION):
    return Finding(
        id=identifier,
        category=category,
        severity=severity,
        confidence="confirmed",
        summary=summary,
        details=details,
        rule=rule,
        spec_entities=(entity.id,),
        implementation_locations=(location,),
        evidence=(),
        suggested_resolution=resolution,
        requires_decision=False,
    )
