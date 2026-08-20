"""Pacing-budget validators: phase windows over the mission clock.

Structural checks live here (windows parse, are ordered, and do not
overlap); the content-facing coverage check — every authored deadline falls
in exactly one phase — lives in `content.py` because it reads the world
file.
"""

from __future__ import annotations

import re

from pasm.core.findings import Finding, FindingCategory, Severity
from pasm.domains.game_design.causality import effective_kind


_DURATION_RE = re.compile(r"^\s*(\d+(?:\.\d+)?)\s*(s|m|h)?\s*$")
_UNIT_SECONDS = {None: 1.0, "s": 1.0, "m": 60.0, "h": 3600.0}


def parse_duration_seconds(raw: str | None) -> float | None:
    if raw is None:
        return None
    found = _DURATION_RE.match(raw)
    if found is None:
        return None
    return float(found.group(1)) * _UNIT_SECONDS[found.group(2)]


def validate_pacing(entities) -> list[Finding]:
    findings: list[Finding] = []
    for entity in entities:
        if effective_kind(entity) != "pacing" or entity.game_design is None:
            continue
        design = entity.game_design
        if not design.phases:
            findings.append(_finding(
                f"pacing-missing-phases:{entity.id}", entity,
                f"Pacing budget '{entity.id}' declares no phases.",
                "A pacing entity is a budget of phase windows over the mission clock.",
                "game-design.pacing-phases-required",
                "Add 'game_design.phases'.", entity.source_location,
            ))
            continue

        windows: list[tuple[float, float, str]] = []
        for index, phase in enumerate(design.phases):
            label = phase.phase_id or f"#{index + 1}"
            location = phase.source_location or entity.source_location
            start = parse_duration_seconds(phase.start)
            end = parse_duration_seconds(phase.end)
            if start is None or end is None:
                findings.append(_finding(
                    f"pacing-window-unparseable:{entity.id}:{label}", entity,
                    f"Pacing phase '{label}' on '{entity.id}' has an unparseable window ({phase.start!r}..{phase.end!r}).",
                    "Phase windows are quoted durations: seconds by default, or with an s/m/h suffix.",
                    "game-design.pacing-window-parseable",
                    "Fix the 'from'/'to' values.", location,
                ))
                continue
            if end <= start:
                findings.append(_finding(
                    f"pacing-window-inverted:{entity.id}:{label}", entity,
                    f"Pacing phase '{label}' on '{entity.id}' ends at or before it starts.",
                    "A phase window must have positive duration.",
                    "game-design.pacing-window-ordered",
                    "Fix the 'from'/'to' values.", location,
                ))
                continue
            windows.append((start, end, label))

        for (start_a, end_a, label_a), (start_b, end_b, label_b) in zip(windows, windows[1:]):
            if start_b < end_a:
                findings.append(_finding(
                    f"pacing-phases-overlap:{entity.id}:{label_a}:{label_b}", entity,
                    f"Pacing phases '{label_a}' and '{label_b}' on '{entity.id}' overlap.",
                    "Phase windows must be declared in order and must not overlap, so every "
                    "moment of the mission belongs to exactly one intended intensity.",
                    "game-design.pacing-phases-disjoint",
                    "Adjust the phase windows so they are ordered and disjoint.",
                    entity.source_location,
                ))
    return findings


def validate_idle_roles(entities) -> list[Finding]:
    """A role no pacing phase engages is the missing workload map, as a check.

    Scoped to the scenario's cast — roles owning verbs the causal chain
    references — because fleet specs keep whole-game roles (debug operators,
    mod-pack authors) alongside scenario design, and those never belong to a
    mission clock.
    """
    pacings = [
        entity for entity in entities
        if effective_kind(entity) == "pacing" and entity.game_design is not None and entity.game_design.phases
    ]
    if not pacings:
        return []
    engaged = {
        role
        for pacing in pacings
        for phase in pacing.game_design.phases
        for role in phase.engaged_roles
    }
    index = {entity.id: entity for entity in entities}
    cast_verbs = {
        verb
        for entity in entities
        if entity.game_design is not None
        for verb in (
            tuple(entity.game_design.requires_player_action)
            + ((entity.id,) if entity.kind in {"verb", "action"}
               and (entity.game_design.on_success or entity.game_design.on_failure) else ())
        )
    }
    cast = set()
    for verb in cast_verbs:
        owner = getattr(index.get(verb), "game_design", None)
        if owner is not None and owner.owner_role is not None:
            cast.add(owner.owner_role)
    findings = []
    for entity in entities:
        if entity.kind not in {"role", "player_role"}:
            continue
        if entity.id not in cast or entity.id in engaged:
            continue
        findings.append(_finding(
            f"role-idle-in-pacing:{entity.id}", entity,
            f"Role '{entity.id}' is engaged by no pacing phase.",
            "A consistently idle station indicates bundling or pacing work; declaring the "
            "engagement per phase is the workload map this check enforces.",
            "game-design.role-engaged-somewhere",
            "Add the role to a phase's 'engaged_roles', or record why it sits out this scenario.",
            entity.source_location,
            severity=Severity.WARNING,
            category=FindingCategory.DESIGN_RISK,
        ))
    return findings


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
