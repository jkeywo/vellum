"""Design principles: falsifiable soft hypotheses, never dogma.

A principle states context, the constructions that implement it, the
dynamic it expects, and how anyone will know whether it actually happened.
An accepted principle without a measurement is unverified; declared
counter-evidence demands a decision rather than silent coexistence.
"""

from __future__ import annotations

from pasm.core.findings import Finding, FindingCategory, Severity
from pasm.core.model import Status
from pasm.domains.game_design.causality import effective_kind


_SETTLED_STATUSES = {Status.ACCEPTED, Status.PARTIALLY_IMPLEMENTED, Status.IMPLEMENTED}
_STRENGTHS = {"hard", "soft"}


def validate_design_principles(entities) -> list[Finding]:
    findings: list[Finding] = []
    index = {entity.id: entity for entity in entities}
    for entity in entities:
        if effective_kind(entity) != "design_principle" or entity.game_design is None:
            continue
        design = entity.game_design
        strength = design.strength or "soft"

        if design.strength is not None and design.strength not in _STRENGTHS:
            findings.append(_finding(
                f"principle-strength-invalid:{entity.id}", entity,
                f"Design principle '{entity.id}' has unknown strength '{design.strength}'.",
                "Strength is 'hard' (a constraint) or 'soft' (a context-scoped hypothesis).",
                "game-design.principle-strength-valid",
                "Use 'hard' or 'soft'.", _location(entity, "strength"),
            ))

        if not design.experience_hypothesis and not design.expected_dynamic:
            findings.append(_finding(
                f"principle-missing-hypothesis:{entity.id}", entity,
                f"Design principle '{entity.id}' states no expected dynamic or experience hypothesis.",
                "A principle without an expected outcome cannot be contradicted by evidence — "
                "it is decoration, not design knowledge.",
                "game-design.principle-hypothesis-required",
                "Add 'expected_dynamic' and/or 'experience_hypothesis'.",
                _location(entity, "expected_dynamic", "experience_hypothesis"),
            ))

        if entity.status in _SETTLED_STATUSES and not design.measured_by:
            tentative = (design.maturity or "").lower() == "tentative"
            if strength == "hard":
                severity = Severity.ERROR
            elif tentative:
                severity = Severity.INFORMATION
            else:
                severity = Severity.WARNING
            findings.append(_finding(
                f"principle-unmeasured:{entity.id}", entity,
                f"Design principle '{entity.id}' is {entity.status.value} but nothing measures it.",
                "An accepted principle needs at least one measurement — a playtest claim, a "
                "test, or a telemetry signal — or its acceptance is untestable folklore.",
                "game-design.principle-measured",
                "Add 'measured_by' entries (playtest-claim IDs, test names, telemetry signals).",
                _location(entity, "measured_by"),
                severity=severity,
                category=FindingCategory.UNVERIFIED,
            ))

        for target in design.measured_by:
            # Entries that name entities must resolve to playtest claims;
            # non-entity entries (test names, telemetry signals) pass through.
            candidate = _lookup(index, target)
            if candidate is not None and candidate.kind not in {"playtest-claim", "metric"}:
                findings.append(_finding(
                    f"principle-measure-not-claim:{entity.id}:{target}", entity,
                    f"Design principle '{entity.id}' is measured by '{target}', which is not a playtest claim.",
                    "Entity-valued measurements must point at playtest-claim entities.",
                    "game-design.principle-measure-kind",
                    "Point 'measured_by' at a playtest-claim, or use a test/telemetry name.",
                    _location(entity, "measured_by"),
                ))

        if design.counter_evidence:
            hard = strength == "hard"
            findings.append(Finding(
                id=f"principle-counter-evidence:{entity.id}",
                category=FindingCategory.CONFLICTING_INTENT,
                severity=Severity.ERROR if hard else Severity.CONCERN,
                confidence="confirmed",
                summary=f"Design principle '{entity.id}' has declared counter-evidence.",
                details="Evidence against the principle is on record; the principle's scope, "
                "strength, or status needs a human decision rather than silent coexistence.",
                rule="game-design.principle-counter-evidence-decided",
                spec_entities=(entity.id,),
                implementation_locations=(_location(entity, "counter_evidence"),),
                evidence=tuple(design.counter_evidence),
                suggested_resolution="Narrow the context, soften or retire the principle, or refute the counter-evidence.",
                requires_decision=True,
            ))
    return findings


def _lookup(index, target: str):
    for entity_id, entity in index.items():
        if entity_id.value == target:
            return entity
    return None


def _location(entity, *field_names):
    for field_name in field_names:
        location = entity.game_design.field_locations.get(field_name)
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
